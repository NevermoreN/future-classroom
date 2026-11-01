#!/usr/bin/env python3
"""Build an explicitly unvalidated per-user x86 MSI candidate; never install it."""
from __future__ import annotations

import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import re
import runpy
import stat
import struct
import subprocess
import sys
import time
import uuid
import xml.etree.ElementTree as ET
import zipfile

ROOT = Path(__file__).absolute().parent.parent
PROBE = runpy.run_path(str(ROOT / "scripts/package-probe.py"))
PE = runpy.run_path(str(ROOT / "scripts/audit-pe.py"))
TARGET = "i686-win7-windows-msvc"
UPGRADE_CODE = "99378807-098D-5F04-8D4D-F181DFA218D5"
NAMESPACE = uuid.UUID("38ea3be2-5440-51d9-8b7b-c26d7df5e18f")
WIX_NS = "http://schemas.microsoft.com/wix/2006/wi"
MAX_ZIP = 15 * 1024 * 1024
MAX_INSTALLED = 30 * 1024 * 1024
MAX_MANIFEST = 256 * 1024
MAX_LOG = 16 * 1024 * 1024
REGISTRY_KEY = r"Software\FutureClassroom\Installer\Components"
FORBIDDEN_TABLES = {"RemoveRegistry", "RemoveFile", "ServiceInstall", "ServiceControl",
                    "Shortcut", "Environment", "CreateFolder", "IniFile", "RemoveIniFile", "LockPermissions", "MsiLockPermissionsEx"}
REQUIRED_TABLES = {"Property", "Directory", "Component", "File", "Feature", "FeatureComponents", "Media", "Upgrade", "LaunchCondition", "InstallExecuteSequence", "InstallUISequence", "Registry", "Binary", "CustomAction", "MsiEmbeddedChainer"}
HELPER_BINARY = "FcLifecycleHelper"
LIFECYCLE_ACTIONS = {
    "FcBeginGate": (2, 'begin "[FcInstallDir]fc-bootstrap.exe"', 1499),
    "FcRollbackNotify": (1282, "rollback", 1501),
    "FcCommitNotify": (1538, "commit", 1502),
}
LIFECYCLE_MAGIC = b"FCLM73\0\0"
LIFECYCLE_FOOTER = b"FCLIFE73-END-v1\0"
LIFECYCLE_REQUIRED = {"fc-bootstrap.exe", "LICENSE", "PROBE-README.txt", "THIRD-PARTY-NOTICES.txt", "manifest.json",
                      "MSI-CANDIDATE.txt", "web/index.html", "web/fc-assets.json", "web/THIRD-PARTY-NOTICES.txt"}
BASE = {"fc-bootstrap.exe", "LICENSE", "PROBE-README.txt", "THIRD-PARTY-NOTICES.txt"}
MANIFEST_KEYS = {"schema_version", "product", "version", "artifact_name", "artifact_type", "status", "target", "target_role", "toolchain", "sha256", "frontend", "features", "gates", "files", "pe_audit"}
LIMITATIONS = [
    "P5-03 prerequisites P5-01/P5-02/P5-06 and formal release acceptance remain unmet.",
    "Only fresh installation and its known rollback are implemented by this candidate; repair/upgrade/uninstall must refuse before mutation and are NOT_VALIDATED.",
    "Real gate/broker/engine-guardian wiring is candidate code; actual original process, effects and cleanup evidence is required before claiming lifecycle success.",
    "Exact-value/ownership-checked HKCU Run uninstall cleanup is NOT_IMPLEMENTED; no Run-value removal is authored.",
    "Standard-user ProgramData initialization/ACL, deployment-account handover and DPAPI lifecycle require separate validation.",
    "Actual install/repair/upgrade/downgrade/uninstall Q13 on Win7/Win10/Win11 and Seewo are NOT_RUN.",
    "The package does not enable production TLS/listeners or grant certificate readiness.",
    "Source ZIP hashes and static PE inspection do not independently prove compiler provenance or OS compatibility.",
]


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def json_document(data: bytes):
    def unique(pairs):
        value = {}
        for key, item in pairs:
            if key in value:
                raise ValueError("Duplicate JSON field: " + key)
            value[key] = item
        return value

    def invalid(_):
        raise ValueError("Non-finite JSON number")

    try:
        return json.loads(data.decode("utf-8"), object_pairs_hook=unique, parse_constant=invalid)
    except (UnicodeError, RecursionError, json.JSONDecodeError) as error:
        raise ValueError("Invalid bounded UTF-8 JSON") from error


def version_parts(value: object) -> tuple[int, int, int]:
    if not isinstance(value, str) or re.fullmatch(r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)", value) is None:
        raise ValueError("Expected exactly three canonical MSI version fields")
    parts = tuple(map(int, value.split(".")))
    if parts[0] > 255 or parts[1] > 255 or parts[2] > 65535:
        raise ValueError("MSI version is out of range")
    return parts


def valid_path(name: object) -> bool:
    return isinstance(name, str) and (name in BASE | {"manifest.json", "web/fc-assets.json", "web/THIRD-PARTY-NOTICES.txt"}
        or (name.startswith("web/") and PROBE["valid_web_path"](name[4:])))


def valid_features(features: object) -> bool:
    if not isinstance(features, dict) or "declared_probe_commands" not in features:
        return False
    for key, value in features.items():
        if not isinstance(key, str):
            return False
        if key == "declared_probe_commands":
            if not isinstance(value, list) or not 1 <= len(value) <= 128 or not all(isinstance(command, str) and 1 <= len(command) <= 512 for command in value):
                return False
        elif not isinstance(value, str):
            return False
    return True


def validate_zip(raw: bytes) -> tuple[dict, dict[str, bytes]]:
    """Bound metadata before decompression, then verify every closed-set member."""
    if not 1 <= len(raw) <= MAX_ZIP:
        raise ValueError("ZIP exceeds 15 MiB candidate input limit")
    with zipfile.ZipFile(io.BytesIO(raw)) as archive:
        infos = archive.infolist()
        if not 5 <= len(infos) <= 72:
            raise ValueError("ZIP member count exceeds the fixed payload bound")
        names, total = set(), 0
        for info in infos:
            name = info.filename
            mode = info.external_attr >> 16
            if info.orig_filename != name or not valid_path(name) or name.lower() in names or info.is_dir() or info.flag_bits & 1:
                raise ValueError("Unexpected/aliased/encrypted ZIP member: " + name)
            if info.create_system == 3 and stat.S_IFMT(mode) not in (0, stat.S_IFREG):
                raise ValueError("ZIP member is not a regular file")
            bound = 5 * 1024 * 1024 if name.startswith("web/") else 24 * 1024 * 1024
            bound = {"manifest.json": MAX_MANIFEST, "web/fc-assets.json": 64 * 1024,
                     "THIRD-PARTY-NOTICES.txt": 4 * 1024 * 1024, "LICENSE": 1024 * 1024,
                     "PROBE-README.txt": 1024 * 1024, "web/THIRD-PARTY-NOTICES.txt": 1024 * 1024}.get(name, bound)
            if not 1 <= info.file_size <= bound or info.compress_type not in (zipfile.ZIP_STORED, zipfile.ZIP_DEFLATED):
                raise ValueError("ZIP member size/compression is unsupported")
            total += info.file_size
            if total > MAX_INSTALLED:
                raise ValueError("Uncompressed payload exceeds 30 MiB")
            names.add(name.lower())
        payloads = {info.filename: archive.read(info) for info in infos}  # CRC verified by zipfile.
    if "manifest.json" not in payloads:
        raise ValueError("Missing development manifest")
    manifest = json_document(payloads["manifest.json"])
    if not isinstance(manifest, dict) or set(manifest) != MANIFEST_KEYS:
        raise ValueError("Unexpected development manifest fields")
    if (type(manifest["schema_version"]) is not int or manifest["schema_version"] != 1
        or manifest["product"] != "FutureClassroom" or manifest["artifact_type"] != "development-probe"
        or manifest["status"] != "NOT_VALIDATED" or manifest["target"] != TARGET
        or manifest["target_role"] != "formal-candidate-unvalidated"):
        raise ValueError("Only the unvalidated Win7 MSVC development ZIP is currently supported")
    version_parts(manifest["version"])
    if (not isinstance(manifest["artifact_name"], str) or re.fullmatch(r"[A-Za-z0-9._-]+\.zip", manifest["artifact_name"]) is None
        or not isinstance(manifest["toolchain"], dict) or set(manifest["toolchain"]) != {"packaging_environment", "declared_build_toolchain", "provenance"}
        or not valid_features(manifest["features"])
        or not isinstance(manifest["gates"], dict) or not manifest["gates"]
        or not all(isinstance(k, str) and v in ("NOT_VALIDATED", "NOT_ALLOWED") for k, v in manifest["gates"].items())
        or not isinstance(manifest["pe_audit"], dict) or manifest["pe_audit"].get("structural_errors") != []):
        raise ValueError("Invalid development provenance/acceptance metadata")
    entries = manifest["files"]
    if not isinstance(entries, list) or not 4 <= len(entries) <= 71:
        raise ValueError("Invalid file ledger")
    declared = set()
    for entry in entries:
        if not isinstance(entry, dict) or set(entry) != {"path", "size", "sha256"}:
            raise ValueError("Unexpected file ledger fields")
        name, size, sha = entry["path"], entry["size"], entry["sha256"]
        if not valid_path(name) or name == "manifest.json" or name in declared or name not in payloads:
            raise ValueError("Invalid file ledger membership")
        if type(size) is not int or size != len(payloads[name]) or not isinstance(sha, str) or sha != digest(payloads[name]):
            raise ValueError("File ledger size/SHA mismatch")
        declared.add(name)
    if declared != set(payloads) - {"manifest.json"} or not BASE <= declared or manifest["sha256"] != digest(payloads["fc-bootstrap.exe"]):
        raise ValueError("ZIP payload does not exactly match its ledger")
    for name, source in (("LICENSE", ROOT / "LICENSE"), ("THIRD-PARTY-NOTICES.txt", ROOT / "packaging/THIRD-PARTY-NOTICES.txt")):
        if payloads[name] != PROBE["bounded_regular_file"](source, 4 * 1024 * 1024):
            raise ValueError("License/notices do not match this packaging source: " + name)
    frontend = manifest["frontend"]
    if not isinstance(frontend, dict) or type(frontend.get("included")) is not bool:
        raise ValueError("Invalid frontend metadata")
    if frontend["included"]:
        expected_keys = {"included", "status", "manifest_path", "manifest_sha256", "schema_version", "api_version", "ui_compatibility_version", "asset_count", "asset_bytes", "third_party_notices", "scope"}
        if set(frontend) != expected_keys or frontend["status"] != "PARTIAL_NOT_VALIDATED" or frontend["manifest_path"] != "web/fc-assets.json" or frontend["third_party_notices"] != "web/THIRD-PARTY-NOTICES.txt":
            raise ValueError("Unexpected frontend binding fields")
        web_raw = payloads.get("web/fc-assets.json", b"")
        web = json_document(web_raw)
        if not isinstance(web, dict) or set(web) != {"schemaVersion", "apiVersion", "uiCompatibilityVersion", "assets"}:
            raise ValueError("Invalid Web manifest fields")
        for outer, inner in (("schema_version", "schemaVersion"), ("api_version", "apiVersion"), ("ui_compatibility_version", "uiCompatibilityVersion")):
            if type(frontend[outer]) is not int or type(web[inner]) is not int or frontend[outer] != 1 or web[inner] != 1:
                raise ValueError("Incompatible Web versions")
        if frontend["manifest_sha256"] != digest(web_raw) or not isinstance(web["assets"], list) or not 1 <= len(web["assets"]) <= 64:
            raise ValueError("Web manifest SHA/count mismatch")
        assets, web_total = set(), 0
        for asset in web["assets"]:
            if not isinstance(asset, dict) or set(asset) != {"path", "sizeBytes", "sha256"} or not PROBE["valid_web_path"](asset["path"]):
                raise ValueError("Invalid Web asset")
            name = "web/" + asset["path"]
            if name.lower() in assets or name not in payloads or type(asset["sizeBytes"]) is not int or asset["sizeBytes"] != len(payloads[name]) or asset["sha256"] != digest(payloads[name]):
                raise ValueError("Web asset binding mismatch")
            assets.add(name.lower())
            web_total += len(payloads[name])
        if ("web/index.html" not in assets or web_total > 5 * 1024 * 1024
            or type(frontend["asset_count"]) is not int or frontend["asset_count"] != len(assets)
            or type(frontend["asset_bytes"]) is not int or frontend["asset_bytes"] != web_total
            or {name.lower() for name in payloads if name.startswith("web/")} != assets | {"web/fc-assets.json", "web/third-party-notices.txt"}):
            raise ValueError("Web payload is not the exact manifest/notice set")
        if payloads["web/THIRD-PARTY-NOTICES.txt"] != PROBE["bounded_regular_file"](ROOT / "web/THIRD-PARTY-NOTICES.txt", 1024 * 1024):
            raise ValueError("Web notices mismatch")
    else:
        raise ValueError("MSI candidate requires the complete verified Web manifest, assets and notices")
    return manifest, payloads


def product_code(version: str) -> str:
    version_parts(version)
    return str(uuid.uuid5(NAMESPACE, "product:" + TARGET + ":" + version)).upper()


def component_code(path: str) -> str:
    return str(uuid.uuid5(NAMESPACE, "component:per-user:x86:" + path.lower())).upper()


def lifecycle_manifest(version: str, payloads: dict[str, bytes]) -> bytes:
    """Closed immutable effects ledger; no caller-selected root, SID or flags."""
    version_parts(version)
    if not LIFECYCLE_REQUIRED <= payloads.keys() or not 1 <= len(payloads) <= 72 or sum(map(len, payloads.values())) > MAX_INSTALLED:
        raise ValueError("Lifecycle payload count/bytes exceeded")
    encoded_version = version.encode("ascii")
    output = bytearray(LIFECYCLE_MAGIC + struct.pack("<HH", 1, len(payloads)))
    output += product_code(version).encode("ascii") + bytes([len(encoded_version)]) + encoded_version
    for name, data in sorted(payloads.items()):
        if not (valid_path(name) or name == "MSI-CANDIDATE.txt") or not 1 <= len(data) <= MAX_INSTALLED:
            raise ValueError("Unreviewed lifecycle payload")
        encoded = name.encode("ascii")
        if not 1 <= len(encoded) <= 240:
            raise ValueError("Lifecycle path length exceeded")
        output += struct.pack("<H", len(encoded)) + encoded + struct.pack("<I", len(data))
        output += bytes.fromhex(digest(data)) + component_code(name).encode("ascii")
    if len(output) > 32768:
        raise ValueError("Lifecycle manifest exceeds bounded native format")
    return bytes(output)


def lifecycle_image(normal_helper: bytes, version: str, payloads: dict[str, bytes]) -> bytes:
    pe_header(normal_helper)
    if normal_helper.endswith(LIFECYCLE_FOOTER):
        raise ValueError("Normal helper already carries a lifecycle overlay")
    manifest = lifecycle_manifest(version, payloads)
    image = normal_helper + manifest + struct.pack("<I", len(manifest)) + LIFECYCLE_FOOTER
    prefix, parsed = parse_lifecycle_image(image)
    if prefix != normal_helper or parsed != {name: {"bytes": len(data), "sha256": digest(data), "component_guid": component_code(name)} for name, data in payloads.items()}:
        raise ValueError("Lifecycle overlay independent readback mismatch")
    return image


def parse_lifecycle_image(image: bytes) -> tuple[bytes, dict]:
    """Independently decode exact footer/header and bounded closed effects rows."""
    if len(image) < 20 or image[-16:] != LIFECYCLE_FOOTER:
        raise ValueError("Missing lifecycle footer")
    length = struct.unpack_from("<I", image, len(image) - 20)[0]
    if not 1 <= length <= 32768 or length > len(image) - 20:
        raise ValueError("Invalid lifecycle overlay length")
    prefix, raw = image[:-20-length], image[-20-length:-20]
    pe_header(prefix)
    if prefix.endswith(LIFECYCLE_FOOTER):
        raise ValueError("Nested lifecycle overlays are forbidden")
    offset = 0
    def take(count):
        nonlocal offset
        if count < 0 or offset + count > len(raw):
            raise ValueError("Truncated lifecycle manifest")
        value = raw[offset:offset + count]
        offset += count
        return value
    if take(8) != LIFECYCLE_MAGIC:
        raise ValueError("Invalid lifecycle manifest magic")
    version, count = struct.unpack("<HH", take(4))
    product = take(36).decode("ascii")
    product_version = take(take(1)[0]).decode("ascii")
    version_parts(product_version)
    if version != 1 or not 1 <= count <= 72 or not 1 <= len(product_version) <= 24 or product != product_code(product_version):
        raise ValueError("Lifecycle header/version/product mismatch")
    records, folded, components, total = {}, set(), set(), 0
    previous = ""
    for _ in range(count):
        size = struct.unpack("<H", take(2))[0]
        if not 1 <= size <= 240:
            raise ValueError("Lifecycle path length exceeded")
        name = take(size).decode("ascii")
        length = struct.unpack("<I", take(4))[0]
        hashed = take(32).hex()
        component = take(36).decode("ascii")
        if (not (valid_path(name) or name == "MSI-CANDIDATE.txt") or name <= previous
            or name.casefold() in folded or component in components or component != component_code(name)
            or not 1 <= length <= MAX_INSTALLED):
            raise ValueError("Lifecycle record namespace/order/identity mismatch")
        previous = name
        folded.add(name.casefold()); components.add(component); total += length
        records[name] = {"bytes": length, "sha256": hashed, "component_guid": component}
    if offset != len(raw) or total > MAX_INSTALLED or not LIFECYCLE_REQUIRED <= records.keys():
        raise ValueError("Lifecycle trailing data/total bound exceeded")
    return prefix, records


def admit_normal_helper(args, payloads: dict[str, bytes], version: str) -> tuple[bytes, dict]:
    """Bind both actual non-test compiler artifacts to one original CI ledger."""
    root = args.build_evidence.absolute()
    PROBE["real_directory"](root)
    result_raw = PROBE["bounded_regular_file"](root / "result.json", MAX_LOG)
    if not re.fullmatch(r"[0-9a-f]{64}", args.build_result_sha256) or digest(result_raw) != args.build_result_sha256:
        raise ValueError("Original CI result does not match the explicit accepted SHA")
    result = json_document(result_raw)
    head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, timeout=10).decode("ascii").strip()
    if (result.get("status") != "COMPILE_CHECKS_PASSED_RUNTIME_NOT_RUN" or result.get("target") != TARGET
        or result.get("baseline") != "Win7" or result.get("source_sha") != head or result.get("dispatch_source_sha") != head):
        raise ValueError("Helper requires the actual Win7 same-source successful CI result")
    inventory_raw = PROBE["bounded_regular_file"](root / "input-sha256.json", MAX_LOG)
    after_raw = PROBE["bounded_regular_file"](root / "input-sha256-after.json", MAX_LOG)
    if digest(inventory_raw) != result.get("input_manifest_sha256") or inventory_raw != after_raw:
        raise ValueError("Original before/after CI source inventory mismatch")
    inventory = json_document(inventory_raw)
    inputs = {}
    for row in inventory:
        if not isinstance(row, dict) or set(row) != {"path", "bytes", "sha256"} or row["path"] in inputs:
            raise ValueError("Invalid CI input ledger")
        inputs[row["path"]] = row
    tracked = subprocess.check_output(["git", "ls-files", "-z"], cwd=ROOT, timeout=10).decode("utf-8").split("\0")
    selected = [name for name in tracked if name and (name.startswith(("crates/", "scripts/", "web/", ".cargo/")) or name in ("Cargo.toml", "Cargo.lock"))]
    for name in selected:
        data = PROBE["bounded_regular_file"](ROOT / name, 64 * 1024 * 1024)
        if inputs.get(name) != {"path": name, "bytes": len(data), "sha256": digest(data)}:
            raise ValueError("Current source differs from helper's actual CI input: " + name)
    ledger_raw = PROBE["bounded_regular_file"](root / "executables.json", MAX_LOG)
    if digest(ledger_raw) != result.get("executables_manifest_sha256"):
        raise ValueError("Normal artifact ledger is not bound by the accepted CI result")
    ledger = json_document(ledger_raw)
    normals = [row for row in ledger if row.get("role") == "product"]
    if len(normals) != 2 or {row.get("target") for row in normals} != {"fc-bootstrap", "fc-installer-lifecycle"}:
        raise ValueError("CI ledger must contain exactly the two normal product targets")
    admitted = {}
    for target, phase in (("fc-bootstrap", "product-compile"), ("fc-installer-lifecycle", "lifecycle-compile")):
        row = next(row for row in normals if row["target"] == target)
        if row.get("package") != "fc-bootstrap" or row.get("cargo_profile_test") is not False or row.get("pe_subsystem") != 2:
            raise ValueError("A test executable or wrong package cannot be a normal helper")
        expected_path = "product/fc-bootstrap/" + target + ".exe"
        if row.get("relative_path") != expected_path:
            raise ValueError("Unexpected normal artifact namespace")
        data = PROBE["bounded_regular_file"](root / expected_path, 32 * 1024 * 1024)
        if len(data) != row["bytes"] or digest(data) != row["sha256"]:
            raise ValueError("Normal artifact bytes differ from original CI ledger")
        pe_header(data)
        steps = [step for step in result["commands"] if step.get("name") == phase]
        if len(steps) != 1 or steps[0].get("passed") is not True or steps[0].get("exit_code") != 0 or steps[0].get("parent_exited") is not True:
            raise ValueError("Actual original normal compiler command did not complete")
        if steps[0].get("stdout") != "logs/" + phase + ".stdout.log":
            raise ValueError("Unexpected compiler log provenance")
        log = PROBE["bounded_regular_file"](root / steps[0]["stdout"], MAX_LOG)
        if digest(log) != steps[0].get("stdout_sha256"):
            raise ValueError("Original compiler log no longer matches the accepted CI result")
        messages = []
        for line in log.splitlines():
            if line.startswith(b"{"):
                message = json_document(line)
                if message.get("reason") == "compiler-artifact" and message.get("target", {}).get("name") == target and message.get("executable"):
                    messages.append(message)
        if len(messages) != 1 or messages[0].get("profile", {}).get("test") is not False or messages[0].get("target", {}).get("kind") != ["bin"]:
            raise ValueError("Original Cargo JSON does not prove the actual normal artifact")
        if not messages[0].get("package_id", "").endswith("#fc-bootstrap@" + version):
            raise ValueError("Normal compiler artifact package/version differs from the admitted ZIP")
        if not messages[0]["executable"].replace("\\", "/").endswith("/" + target + ".exe"):
            raise ValueError("Cargo normal executable identity differs")
        admitted[target] = data
    helper = PROBE["bounded_regular_file"](args.lifecycle_helper.absolute(), 32 * 1024 * 1024)
    if digest(helper) != args.lifecycle_helper_sha256 or helper != admitted["fc-installer-lifecycle"] or payloads["fc-bootstrap.exe"] != admitted["fc-bootstrap"]:
        raise ValueError("Both ordinary payload and helper must be exact same-source normal CI bytes")
    return helper, {"result_sha256": digest(result_raw), "source_sha": head, "normal_helper_sha256": digest(helper),
                    "main_sha256": digest(admitted["fc-bootstrap"]), "profile_test": False, "current_inputs_verified": len(selected)}


def chainer_idt() -> bytes:
    return ("MsiEmbeddedChainer\tCondition\tCommandLine\tSource\tType\r\n"
            "s72\tS255\tS255\ts72\ti2\r\nMsiEmbeddedChainer\tMsiEmbeddedChainer\r\n"
            "FcLifecycleGuardian\tNOT (ACTION = \"ADVERTISE\")\tguardian\t" + HELPER_BINARY + "\t2\r\n").encode("ascii")


def authoring(version: str, names: list[str]) -> bytes:
    """All names have already passed the closed payload namespace admission."""
    ET.register_namespace("", WIX_NS)
    def element(parent, name, **attrs):
        return ET.SubElement(parent, "{" + WIX_NS + "}" + name, attrs)
    root = ET.Element("{" + WIX_NS + "}Wix")
    product = element(root, "Product", Id=product_code(version), Name="FutureClassroom MSI candidate", Language="1033", Codepage="1252", Version=version, Manufacturer="FutureClassroom", UpgradeCode=UPGRADE_CODE)
    element(product, "Package", InstallerVersion="500", Compressed="yes", InstallScope="perUser")
    element(product, "Media", Id="1", Cabinet="payload.cab", EmbedCab="yes")
    element(product, "MajorUpgrade", AllowDowngrades="no", AllowSameVersionUpgrades="no", DowngradeErrorMessage="A newer FutureClassroom candidate is installed.")
    sequence = element(product, "InstallExecuteSequence")
    element(sequence, "InstallExecute", Sequence="6500")
    element(sequence, "RemoveExistingProducts", After="InstallExecute")
    element(product, "Binary", Id=HELPER_BINARY, SourceFile="lifecycle/fc-installer-lifecycle.exe")
    for action, (kind, role, number) in LIFECYCLE_ACTIONS.items():
        attrs = dict(Id=action, BinaryKey=HELPER_BINARY, ExeCommand=role, Return="check", Impersonate="yes")
        if kind in (1282, 1538):
            attrs["Execute"] = "rollback" if kind == 1282 else "commit"
        element(product, "CustomAction", **attrs)
        element(sequence, "Custom", Action=action, Sequence=str(number)).text = "1"
    element(product, "Property", Id="MSIRESTARTMANAGERCONTROL", Value="DisableShutdown")
    element(product, "Condition", Message="This candidate supports only the current user installation context.").text = "NOT ALLUSERS"
    element(product, "Condition", Message="Windows 7 or later is required; actual OS compatibility remains unvalidated.").text = "Installed OR VersionNT >= 601"
    element(product, "Condition", Message="Rollback must remain enabled for this lifecycle candidate.").text = "NOT RollbackDisabled AND NOT DISABLEROLLBACK"
    element(product, "Condition", Message="Automatic application shutdown is not allowed by this lifecycle candidate.").text = 'MSIRESTARTMANAGERCONTROL = "DisableShutdown"'
    target = element(product, "Directory", Id="TARGETDIR", Name="SourceDir")
    local = element(target, "Directory", Id="LocalAppDataFolder")
    programs = element(local, "Directory", Id="UserPrograms", Name="Programs")
    install = element(programs, "Directory", Id="FcInstallDir", Name="FutureClassroom")
    directories = {"": install}
    feature = element(product, "Feature", Id="Main", Title="FutureClassroom candidate", Level="1")
    for name in sorted(names):
        parts = name.split("/")
        prefix = ""
        for part in parts[:-1]:
            parent = directories[prefix]
            prefix = (prefix + "/" + part).lstrip("/")
            if prefix not in directories:
                directories[prefix] = element(parent, "Directory", Id="D" + digest(prefix.encode())[:24], Name=part)
        identifier = digest(name.encode())[:24]
        component = element(directories[prefix], "Component", Id="C" + identifier, Guid=component_code(name), Win64="no")
        element(component, "File", Id="F" + identifier, Source="payload/" + name, Name=parts[-1], KeyPath="no", DiskId="1")
        element(component, "RegistryValue", Id="R" + identifier, Root="HKCU", Key=REGISTRY_KEY, Name=component_code(name), Type="integer", Value="1", KeyPath="yes")
        element(feature, "ComponentRef", Id="C" + identifier)
    ET.indent(root)
    return ET.tostring(root, encoding="utf-8", xml_declaration=True) + b"\n"


def fresh_output(path: Path) -> Path:
    path = path.absolute()
    PROBE["real_directory"](path.parent)
    path.mkdir()  # Exclusive: an existing file, directory or dangling link fails.
    PROBE["real_directory"](path)
    (path / ".owner").write_text("FutureClassroom MSI candidate: " + str(uuid.uuid4()) + "\n", encoding="ascii")
    return path


def tool_file(path: Path) -> Path:
    if not path.is_absolute():
        raise ValueError("Tool paths must be explicit and absolute")
    PROBE["real_directory"](path.parent)
    PROBE["bounded_regular_file"](path, 128 * 1024 * 1024)
    return path


def file_digest(path: Path) -> str:
    hashed = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(64 * 1024), b""):
            hashed.update(block)
    return hashed.hexdigest()


def idt_rows(raw: bytes, table: str) -> list[dict[str, str]]:
    """Read only bounded msiinfo IDT exports, including their real primary keys."""
    if len(raw) > 4 * 1024 * 1024:
        raise ValueError("IDT export exceeds its bound")
    lines = raw.decode("utf-8", "strict").splitlines()
    if len(lines) < 3 or len(lines) > 515 or "\0" in "".join(lines):
        raise ValueError("Invalid bounded IDT document")
    columns, types, primary = (line.split("\t") for line in lines[:3])
    if (not 1 <= len(columns) <= 32 or len(set(columns)) != len(columns) or not all(columns)
        or len(types) != len(columns) or not all(re.fullmatch(r"[sSlLiIvV][0-9]+", item) for item in types)
        or primary[0] != table or not 1 <= len(primary[1:]) <= len(columns)
        or not set(primary[1:]) <= set(columns)):
        raise ValueError("Invalid IDT schema: " + table)
    result, seen = [], set()
    for line in lines[3:]:
        if not line:
            continue
        cells = line.split("\t")
        if len(cells) != len(columns) or any(len(cell) > 4096 for cell in cells):
            raise ValueError("Malformed IDT row: " + table)
        row = dict(zip(columns, cells))
        key = tuple(row[name] for name in primary[1:])
        if key in seen:
            raise ValueError("Duplicate IDT primary key: " + table)
        seen.add(key)
        result.append(row)
    return result


def integer(value: str) -> int:
    if re.fullmatch(r"-?(?:0|[1-9][0-9]*)", value) is None:
        raise ValueError("Invalid canonical MSI integer")
    return int(value)


def verify_transaction_sequence(raw: bytes) -> dict[str, int]:
    rows = idt_rows(raw, "InstallExecuteSequence")
    if any(set(row) != {"Action", "Condition", "Sequence"} for row in rows):
        raise ValueError("Invalid InstallExecuteSequence schema")
    actions = {row["Action"]: integer(row["Sequence"]) for row in rows}
    required = {"CostFinalize", "InstallValidate", "InstallInitialize", "InstallExecute", "RemoveExistingProducts", "InstallFinalize"} | LIFECYCLE_ACTIONS.keys()
    if not required <= actions.keys() or not (actions["CostFinalize"] < actions["InstallValidate"] < actions["FcBeginGate"] < actions["InstallInitialize"] < actions["FcRollbackNotify"] < actions["FcCommitNotify"] < actions["InstallExecute"] < actions["RemoveExistingProducts"] < actions["InstallFinalize"]):
        raise ValueError("Lifecycle gate must execute before mutation; removal must follow InstallExecute and precede InstallFinalize")
    if any(actions[name] != spec[2] for name, spec in LIFECYCLE_ACTIONS.items()):
        raise ValueError("Lifecycle hook sequence differs from the exact reviewed topology")
    for row in rows:
        if row["Action"] in LIFECYCLE_ACTIONS and row["Condition"] != "1":
            raise ValueError("Lifecycle hooks cannot depend on caller-controlled bypass properties")
        if row["Action"] in {"InstallFiles", "RemoveFiles", "WriteRegistryValues", "RemoveRegistryValues", "RegisterProduct", "PublishProduct"} and integer(row["Sequence"]) <= actions["FcCommitNotify"]:
            raise ValueError("An installer mutation precedes lifecycle acquisition/rollback scheduling")
    return {name: actions[name] for name in sorted(required)}


def verify_msi_contract(summary: bytes, tables: dict[str, bytes], version: str, payloads: dict[str, bytes]) -> dict:
    """Check actual compiler output, not just table existence or authored XML."""
    if len(summary) > 64 * 1024 or not REQUIRED_TABLES <= tables.keys():
        raise ValueError("Missing required MSI table/summary export")
    fields = {}
    for line in summary.decode("utf-8", "strict").splitlines():
        key, separator, value = line.partition(": ")
        if not separator or key in fields:
            raise ValueError("Malformed/duplicate MSI summary field")
        fields[key] = value
    if (fields.get("Template") != "Intel;1033" or fields.get("Version") != "500 (1f4)"
        or fields.get("Source") != "10 (a)"):
        raise ValueError("MSI summary is not compressed per-user Intel/Installer 500")
    parsed = {name: idt_rows(raw, name) for name, raw in tables.items()}
    for name in FORBIDDEN_TABLES:
        if parsed.get(name):
            raise ValueError("Forbidden MSI side-effect table is nonempty: " + name)

    def rows(name, columns):
        found = parsed[name]
        if any(set(row) != set(columns) for row in found):
            raise ValueError("Unexpected MSI columns: " + name)
        return found

    if rows("Binary", ["Name", "Data"]) != [{"Name": HELPER_BINARY, "Data": "Binary." + HELPER_BINARY}]:
        raise ValueError("Only the exact lifecycle helper Binary stream is allowed")
    expected_actions = [{"Action": name, "Type": str(kind), "Source": HELPER_BINARY, "Target": role}
                        for name, (kind, role, _) in LIFECYCLE_ACTIONS.items()]
    actual_actions = rows("CustomAction", ["Action", "Type", "Source", "Target"])
    if sorted(actual_actions, key=lambda row: row["Action"]) != sorted(expected_actions, key=lambda row: row["Action"]):
        raise ValueError("Lifecycle CustomAction type/source/role must match the exact allowlist")
    if rows("MsiEmbeddedChainer", ["MsiEmbeddedChainer", "Condition", "CommandLine", "Source", "Type"]) != [
            {"MsiEmbeddedChainer": "FcLifecycleGuardian", "Condition": 'NOT (ACTION = "ADVERTISE")',
             "CommandLine": "guardian", "Source": HELPER_BINARY, "Type": "2"}]:
        raise ValueError("Only the exact engine-launched lifecycle guardian is allowed")

    properties = {row["Property"]: row["Value"] for row in rows("Property", ["Property", "Value"])}
    expected_properties = {"Manufacturer": "FutureClassroom", "ProductLanguage": "1033", "ProductName": "FutureClassroom MSI candidate",
        "ProductVersion": version, "ProductCode": "{" + product_code(version) + "}", "UpgradeCode": "{" + UPGRADE_CODE + "}",
        "MSIRESTARTMANAGERCONTROL": "DisableShutdown", "SecureCustomProperties": "WIX_DOWNGRADE_DETECTED;WIX_UPGRADE_DETECTED"}
    # Property order is irrelevant, including the order of the two secure names.
    secure = properties.get("SecureCustomProperties", "").split(";")
    if len(secure) != 2 or set(secure) != {"WIX_DOWNGRADE_DETECTED", "WIX_UPGRADE_DETECTED"}:
        raise ValueError("Invalid MSI secure upgrade properties")
    properties["SecureCustomProperties"] = expected_properties["SecureCustomProperties"]
    if properties != expected_properties:
        raise ValueError("MSI product/version/scope properties differ from admitted authoring")

    xml = ET.fromstring(authoring(version, list(payloads)))
    ns = {"w": WIX_NS}
    expected_dirs = {}
    def directory(node, parent):
        expected_dirs[node.get("Id")] = (parent, node.get("Name", "."))
        for child in node.findall("w:Directory", ns):
            directory(child, node.get("Id"))
    directory(xml.find("w:Product/w:Directory", ns), "")
    actual_dirs = {row["Directory"]: (row["Directory_Parent"], row["DefaultDir"]) for row in rows("Directory", ["Directory", "Directory_Parent", "DefaultDir"])}
    if actual_dirs != expected_dirs:
        raise ValueError("MSI directories escaped the fixed per-user installation tree")
    expected_components, expected_registry, expected_files = {}, {}, {}
    for path, data in payloads.items():
        identifier = digest(path.encode())[:24]
        component, file_id, registry_id = "C" + identifier, "F" + identifier, "R" + identifier
        parent = path.rpartition("/")[0]
        directory_id = "D" + digest(parent.encode())[:24] if parent else "FcInstallDir"
        expected_components[component] = {"Component": component, "ComponentId": "{" + component_code(path) + "}",
            "Directory_": directory_id, "Attributes": "4", "Condition": "", "KeyPath": registry_id}
        expected_registry[registry_id] = {"Registry": registry_id, "Root": "1", "Key": REGISTRY_KEY, "Name": component_code(path), "Value": "#1", "Component_": component}
        expected_files[file_id] = (component, path.rsplit("/", 1)[-1], len(data))
    actual_components = {row["Component"]: row for row in rows("Component", ["Component", "ComponentId", "Directory_", "Attributes", "Condition", "KeyPath"])}
    actual_registry = {row["Registry"]: row for row in rows("Registry", ["Registry", "Root", "Key", "Name", "Value", "Component_"])}
    if actual_components != expected_components or actual_registry != expected_registry:
        raise ValueError("MSI component GUID/x86/HKCU KeyPath ownership differs from the exact payload set")
    file_rows = rows("File", ["File", "Component_", "FileName", "FileSize", "Version", "Language", "Attributes", "Sequence"])
    if {row["File"] for row in file_rows} != set(expected_files):
        raise ValueError("MSI file/component set mismatch")
    sequences = set()
    for row in file_rows:
        component, name, size = expected_files[row["File"]]
        filename = row["FileName"].split("|")
        if (not 1 <= len(filename) <= 2 or not all(filename) or filename[-1] != name
            or row["Component_"] != component or integer(row["FileSize"]) != size
            or row["Version"] or row["Language"] or row["Attributes"] != "512"):
            raise ValueError("MSI file name/size/component/compression contract mismatch")
        sequences.add(integer(row["Sequence"]))
    if sequences != set(range(1, len(payloads) + 1)):
        raise ValueError("MSI file sequence is not the exact contiguous payload set")
    features = rows("Feature", ["Feature", "Feature_Parent", "Title", "Description", "Display", "Level", "Directory_", "Attributes"])
    if (len(features) != 1 or features[0]["Feature"] != "Main" or features[0]["Feature_Parent"] or features[0]["Directory_"]
        or features[0]["Level"] != "1" or features[0]["Title"] != "FutureClassroom candidate"
        or features[0]["Description"] or features[0]["Attributes"] != "0"):
        raise ValueError("MSI feature definition mismatch")
    mapped = rows("FeatureComponents", ["Feature_", "Component_"])
    if {(row["Feature_"], row["Component_"]) for row in mapped} != {("Main", name) for name in expected_components}:
        raise ValueError("MSI feature-to-component ownership mismatch")
    media = rows("Media", ["DiskId", "LastSequence", "DiskPrompt", "Cabinet", "VolumeLabel", "Source"])
    if media != [{"DiskId": "1", "LastSequence": str(len(payloads)), "DiskPrompt": "", "Cabinet": "#payload.cab", "VolumeLabel": "", "Source": ""}]:
        raise ValueError("MSI cabinet is not the single embedded full-payload cabinet")
    upgrades = rows("Upgrade", ["UpgradeCode", "VersionMin", "VersionMax", "Language", "Attributes", "Remove", "ActionProperty"])
    actual_upgrades = {tuple(row[key] for key in ("UpgradeCode", "VersionMin", "VersionMax", "Language", "Attributes", "Remove", "ActionProperty")) for row in upgrades}
    if actual_upgrades != {("{" + UPGRADE_CODE + "}", "", version, "", "1", "", "WIX_UPGRADE_DETECTED"),
                           ("{" + UPGRADE_CODE + "}", version, "", "", "2", "", "WIX_DOWNGRADE_DETECTED")}:
        raise ValueError("MSI upgrade ranges/downgrade detection differ from authoring")
    conditions = rows("LaunchCondition", ["Condition", "Description"])
    expected_conditions = {node.text: node.get("Message") for node in xml.findall("w:Product/w:Condition", ns)}
    expected_conditions["NOT WIX_DOWNGRADE_DETECTED"] = "A newer FutureClassroom candidate is installed."
    if {row["Condition"]: row["Description"] for row in conditions} != expected_conditions:
        raise ValueError("MSI scope/OS/downgrade launch conditions differ from authoring")
    transaction = verify_transaction_sequence(tables["InstallExecuteSequence"])
    for name in ("InstallExecuteSequence", "InstallUISequence"):
        sequence = {row["Action"]: integer(row["Sequence"]) for row in rows(name, ["Action", "Condition", "Sequence"])}
        if not 0 < sequence.get("FindRelatedProducts", 0) < sequence.get("LaunchConditions", 0):
            raise ValueError("MSI downgrade detection must precede launch conditions in both sequences")
        if name == "InstallExecuteSequence" and sequence["LaunchConditions"] >= sequence["CostFinalize"]:
            raise ValueError("MSI launch conditions must reject shutdown overrides before costing")
        if {"ForceReboot", "ScheduleReboot", "StartServices", "StopServices", "DeleteServices", "CreateShortcuts", "RemoveShortcuts", "WriteRegistryValues"} & sequence.keys() - ({"WriteRegistryValues"} if name == "InstallExecuteSequence" else set()):
            raise ValueError("Unexpected MSI lifecycle/system action")
    return {"status": "PASS", "template": fields["Template"], "installer_version": 500, "source_flags": 10,
            "payload_files": len(payloads), "registry_scope": "Exact installer-only HKCU component markers; no Run cleanup", "transaction_sequence": transaction}


def run_tool(output: Path, label: str, args: list[str], records: list) -> bytes:
    stdout, stderr = output / (label + ".stdout.log"), output / (label + ".stderr.log")
    started = time.monotonic()
    reason = None
    with stdout.open("xb") as out, stderr.open("xb") as err:
        child = subprocess.Popen(args, cwd=output, stdin=subprocess.DEVNULL, stdout=out, stderr=err)
        try:
            while child.poll() is None:
                if time.monotonic() - started >= 120 or stdout.stat().st_size > MAX_LOG or stderr.stat().st_size > MAX_LOG:
                    reason = "Tool deadline/log budget exceeded"
                    child.kill()
                    break
                time.sleep(0.02)
        finally:
            if child.poll() is None:
                child.kill()
            code = child.wait()
    record = {"name": label, "command": args, "exit_code": code, "elapsed_seconds": time.monotonic() - started,
              "stdout": {"path": stdout.name, "size": stdout.stat().st_size, "sha256": file_digest(stdout)},
              "stderr": {"path": stderr.name, "size": stderr.stat().st_size, "sha256": file_digest(stderr)}}
    records.append(record)
    if reason or code != 0 or stdout.stat().st_size > MAX_LOG or stderr.stat().st_size > MAX_LOG:
        raise ValueError(reason or (label + " failed, exit " + str(code)))
    captured = PROBE["bounded_regular_file"](stdout, MAX_LOG)
    if label == "wixl-build" and (stderr.stat().st_size or re.search(rb"warning|critical|unhandled|unsupported", captured, re.IGNORECASE)):
        raise ValueError("wixl emitted diagnostics: authoring cannot be accepted on exit code alone")
    return captured


def pe_header(data: bytes) -> None:
    if len(data) < 64 or data[:2] != b"MZ":
        raise ValueError("Missing executable DOS header")
    offset = struct.unpack_from("<I", data, 60)[0]
    if offset > len(data) - 96 or data[offset:offset + 4] != b"PE\0\0":
        raise ValueError("Invalid PE offset/signature")
    if struct.unpack_from("<H", data, offset + 4)[0] != 0x14C or struct.unpack_from("<H", data, offset + 24)[0] != 0x10B:
        raise ValueError("Expected x86 PE32")
    optional_size = struct.unpack_from("<H", data, offset + 20)[0]
    if optional_size < 70 or optional_size > len(data) - offset - 24:
        raise ValueError("Incomplete PE32 optional header")
    if struct.unpack_from("<H", data, offset + 24 + 68)[0] != 2:
        raise ValueError("Application payload requires Windows GUI subsystem 2")
    if struct.unpack_from("<HH", data, offset + 24 + 48) != (6, 1):
        raise ValueError("Win7 candidate requires PE subsystem version 6.1")


def build(args) -> dict:
    source = args.input_zip.absolute()
    PROBE["real_directory"](source.parent)
    raw = PROBE["bounded_regular_file"](source, MAX_ZIP)
    if re.fullmatch(r"[0-9a-f]{64}", args.input_sha256) is None or digest(raw) != args.input_sha256:
        raise ValueError("Source ZIP does not match the explicit expected SHA-256")
    manifest, payloads = validate_zip(raw)
    pe_header(payloads["fc-bootstrap.exe"])
    normal_helper, helper_provenance = admit_normal_helper(args, payloads, manifest["version"])
    tools = {name: tool_file(getattr(args, name)) for name in ("wixl", "msibuild", "msiinfo", "msiextract", "objdump")}
    if not re.fullmatch(r"[0-9a-f]{64}", args.msibuild_sha256) or file_digest(tools["msibuild"]) != args.msibuild_sha256:
        raise ValueError("msibuild does not match its explicit reviewed SHA")
    output = fresh_output(args.output_dir)
    report = {"schema_version": 1, "status": "FAIL", "candidate_status": "NOT_VALIDATED", "scope": "MSI_CANDIDATE_BUILD_AND_STATIC_TABLE_INSPECTION_ONLY", "readiness": "NOT_GRANTED", "installation": "NOT_RUN", "target": TARGET,
              "source_zip": {"path": str(source), "size": len(raw), "sha256": digest(raw)}, "source_manifest": manifest,
              "script_sha256": digest(Path(__file__).read_bytes()), "commands": [], "limitations": LIMITATIONS,
              "normal_helper_provenance": helper_provenance,
              "installation_scope": "per-user; fixed LocalAppData/Programs/FutureClassroom; business data remains original protected ProgramData",
              "tools": {name: {"path": str(path), "sha256": digest(path.read_bytes())} for name, path in tools.items()}}
    report["packaging_dependencies"] = {str(path.relative_to(ROOT)): file_digest(path) for path in (
        ROOT / "scripts/package-probe.py", ROOT / "scripts/audit-pe.py", ROOT / "packaging/MSI-CANDIDATE.md")}
    try:
        report["tools"]["msibuild"]["reviewed_version"] = "0.103+repack-1build3"
        report["tools"]["msibuild"]["version_basis"] = "Caller-reviewed exact binary SHA from the pinned tool package; upstream v0.103 msibuild has no --version option."
        for name in ("wixl", "msiinfo", "msiextract", "objdump"):
            version = run_tool(output, name + "-version", [str(tools[name]), "--version"], report["commands"])
            report["tools"][name]["version_output"] = version.decode("utf-8", "replace")
            if name == "wixl" and version.strip() != b"0.103":
                raise ValueError("Only the reviewed wixl 0.103 authoring subset is accepted")
        payloads["MSI-CANDIDATE.txt"] = PROBE["bounded_regular_file"](ROOT / "packaging/MSI-CANDIDATE.md", 64 * 1024)
        if sum(map(len, payloads.values())) > MAX_INSTALLED:
            raise ValueError("Installed payload including candidate instructions exceeds 30 MiB")
        report["payloads"] = [{"path": name, "size": len(data), "sha256": digest(data), "component_guid": component_code(name)} for name, data in sorted(payloads.items())]
        image = lifecycle_image(normal_helper, manifest["version"], payloads)
        lifecycle = output / "lifecycle"
        lifecycle.mkdir()
        (lifecycle / "fc-installer-lifecycle.exe").write_bytes(image)
        report["lifecycle_binary"] = {"path": "lifecycle/fc-installer-lifecycle.exe", "bytes": len(image), "sha256": digest(image),
                                      "normal_prefix_bytes": len(normal_helper), "normal_prefix_sha256": digest(normal_helper),
                                      "manifest_sha256": digest(lifecycle_manifest(manifest["version"], payloads)), "records": len(payloads)}
        for name, data in payloads.items():
            destination = output / "payload" / name
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_bytes(data)
        inspection = run_tool(output, "payload-pe", [str(tools["objdump"]), "-p", str(output / "payload/fc-bootstrap.exe")], report["commands"])
        audit = PE["parse_objdump"](inspection.decode("utf-8", "strict"))
        report["pe_audit"] = audit
        if audit["structural_errors"] or any(risk["kind"] == "RUNTIME_DEPLOYMENT" for risk in audit["risks"]):
            raise ValueError("Candidate PE structure/static runtime audit failed")
        helper_inspection = run_tool(output, "helper-pe", [str(tools["objdump"]), "-p", str(lifecycle / "fc-installer-lifecycle.exe")], report["commands"])
        helper_audit = PE["parse_objdump"](helper_inspection.decode("utf-8", "strict"))
        report["helper_pe_audit"] = helper_audit
        if helper_audit["structural_errors"] or any(risk["kind"] == "RUNTIME_DEPLOYMENT" for risk in helper_audit["risks"]):
            raise ValueError("Lifecycle helper PE/static runtime audit failed")
        authored = authoring(manifest["version"], list(payloads))
        (output / "candidate.wxs").write_bytes(authored)
        report["authoring"] = {"path": "candidate.wxs", "sha256": digest(authored), "product_code": product_code(manifest["version"]), "upgrade_code": UPGRADE_CODE}
        run_tool(output, "wixl-build", [str(tools["wixl"]), "-a", "x86", "-o", "candidate.msi", "candidate.wxs"], report["commands"])
        (output / "MsiEmbeddedChainer.idt").write_bytes(chainer_idt())
        run_tool(output, "chainer-import", [str(tools["msibuild"]), "candidate.msi", "-i", "MsiEmbeddedChainer.idt"], report["commands"])
        msi = PROBE["bounded_regular_file"](output / "candidate.msi", MAX_INSTALLED)
        report["msi"] = {"path": "candidate.msi", "size": len(msi), "sha256": digest(msi)}
        summary = run_tool(output, "msi-summary", [str(tools["msiinfo"]), "suminfo", "candidate.msi"], report["commands"])
        tables = run_tool(output, "msi-tables", [str(tools["msiinfo"]), "tables", "candidate.msi"], report["commands"]).decode("utf-8", "strict").splitlines()
        if not REQUIRED_TABLES <= set(tables):
            raise ValueError("Missing required MSI tables")
        exports = {}
        for table in sorted(set(tables) & (REQUIRED_TABLES | FORBIDDEN_TABLES)):
            data = run_tool(output, "table-" + table, [str(tools["msiinfo"]), "export", "candidate.msi", table], report["commands"])
            exports[table] = data
        report["msi_contract"] = verify_msi_contract(summary, exports, manifest["version"], payloads)
        embedded = run_tool(output, "lifecycle-binary-extract", [str(tools["msiinfo"]), "extract", "candidate.msi", "Binary." + HELPER_BINARY], report["commands"])
        if embedded != image:
            raise ValueError("Actual MSI Binary stream differs from admitted normal prefix plus exact overlay")
        prefix, decoded = parse_lifecycle_image(embedded)
        if prefix != normal_helper or decoded != {name: {"bytes": len(data), "sha256": digest(data), "component_guid": component_code(name)} for name, data in payloads.items()}:
            raise ValueError("Actual MSI helper overlay/payload ledger mismatch")
        report["msi_lifecycle_binary_readback"] = "EXACT_NORMAL_PREFIX_AND_CLOSED_OVERLAY_BYTES_MATCH"
        report["transaction_sequence"] = report["msi_contract"]["transaction_sequence"]
        extracted = output / "extracted"
        extracted.mkdir()
        run_tool(output, "msi-extract", [str(tools["msiextract"]), "-C", str(extracted), "candidate.msi"], report["commands"])
        # The directory emitted by msiextract may include LocalAppData/Programs.
        matches = list(extracted.rglob("fc-bootstrap.exe"))
        if len(matches) != 1:
            raise ValueError("MSI extraction did not yield exactly one executable")
        extracted_root = matches[0].parent
        actual = {}
        for path in extracted.rglob("*"):
            if path.is_dir():
                PROBE["real_directory"](path)
                continue
            if not path.is_relative_to(extracted_root):
                raise ValueError("Extracted file escaped the fixed installation directory")
            name = path.relative_to(extracted_root).as_posix()
            actual[name] = PROBE["bounded_regular_file"](path, MAX_INSTALLED)
        if actual != payloads:
            raise ValueError("Extracted MSI payload differs from the admitted source bytes")
        report["status"] = "PASS"
        report["payload_extraction"] = "EXACT_BYTES_MATCH"
    except BaseException as error:
        report["error"] = type(error).__name__ + ": " + str(error)
        raise
    finally:
        (output / "report.json").write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    return report


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input-zip", type=Path, required=True)
    parser.add_argument("--input-sha256", required=True)
    parser.add_argument("--lifecycle-helper", type=Path, required=True, help="Actual same-source profile.test=false normal CI helper")
    parser.add_argument("--lifecycle-helper-sha256", required=True)
    parser.add_argument("--build-evidence", type=Path, required=True, help="Original accepted CI artifact directory, including Cargo JSON and source inventories")
    parser.add_argument("--build-result-sha256", required=True)
    parser.add_argument("--output-dir", type=Path, required=True, help="New owned directory; never overwritten or automatically deleted")
    for name in ("wixl", "msibuild", "msiinfo", "msiextract", "objdump"):
        parser.add_argument("--" + name, type=Path, required=True, help="Explicit absolute executable path")
    parser.add_argument("--msibuild-sha256", required=True)
    args = parser.parse_args()
    try:
        report = build(args)
    except (OSError, ValueError, zipfile.BadZipFile, RuntimeError) as error:
        print("MSI candidate failed; retained owned output evidence: " + str(error), file=sys.stderr)
        return 1
    print(json.dumps({"status": report["status"], "scope": report["scope"], "installation": "NOT_RUN", "readiness": "NOT_GRANTED", "output": str(args.output_dir.absolute())}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
