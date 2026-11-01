#!/usr/bin/env python3
"""Pure hostile-input/authoring checks; does not build or install an MSI."""
from __future__ import annotations

import ast
import copy
import io
import json
from pathlib import Path
import runpy
import stat
import struct
import subprocess
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch
import warnings
import xml.etree.ElementTree as ET
import zipfile

M = runpy.run_path(str(Path(__file__).absolute().with_name("package-msi-candidate.py")))
ROOT, SHA = M["ROOT"], M["digest"]


def normal_pe_fixture(subsystem=2):
    """Synthetic structural input, never executable/compiler provenance evidence."""
    data = bytearray(512)
    data[:2] = b"MZ"
    struct.pack_into("<I", data, 60, 128)
    data[128:132] = b"PE\0\0"
    struct.pack_into("<H", data, 132, 0x14C)
    struct.pack_into("<H", data, 148, 224)
    struct.pack_into("<H", data, 152, 0x10B)
    struct.pack_into("<HH", data, 200, 6, 1)
    struct.pack_into("<H", data, 220, subsystem)
    return bytes(data)


def real_packager_features():
    tree = ast.parse((ROOT / "scripts/package-probe.py").read_bytes())
    for node in ast.walk(tree):
        if isinstance(node, ast.Dict):
            for key, value in zip(node.keys, node.values):
                if isinstance(key, ast.Constant) and key.value == "features":
                    return ast.literal_eval(value)
    raise AssertionError("Real package-probe manifest features were not found")


def fixture(web=True):
    payloads = {"fc-bootstrap.exe": b"synthetic fixture only; not a PE or executable",
                "LICENSE": (ROOT / "LICENSE").read_bytes(),
                "THIRD-PARTY-NOTICES.txt": (ROOT / "packaging/THIRD-PARTY-NOTICES.txt").read_bytes(),
                "PROBE-README.txt": b"NOT_VALIDATED development fixture"}
    frontend = {"included": False, "status": "NOT_INCLUDED", "scope": "synthetic no-Web fixture"}
    if web:
        html = b"<!doctype html><title>synthetic</title>"
        document = {"schemaVersion": 1, "apiVersion": 1, "uiCompatibilityVersion": 1,
                    "assets": [{"path": "index.html", "sizeBytes": len(html), "sha256": SHA(html)}]}
        raw = json.dumps(document).encode()
        payloads.update({"web/index.html": html, "web/fc-assets.json": raw,
                         "web/THIRD-PARTY-NOTICES.txt": (ROOT / "web/THIRD-PARTY-NOTICES.txt").read_bytes()})
        frontend = {"included": True, "status": "PARTIAL_NOT_VALIDATED", "manifest_path": "web/fc-assets.json",
                    "manifest_sha256": SHA(raw), "schema_version": 1, "api_version": 1, "ui_compatibility_version": 1,
                    "asset_count": 1, "asset_bytes": len(html), "third_party_notices": "web/THIRD-PARTY-NOTICES.txt", "scope": "synthetic"}
    manifest = {"schema_version": 1, "product": "FutureClassroom", "version": "0.1.0", "artifact_name": "synthetic.zip",
                "artifact_type": "development-probe", "status": "NOT_VALIDATED", "target": M["TARGET"],
                "target_role": "formal-candidate-unvalidated", "toolchain": {"packaging_environment": "synthetic",
                "declared_build_toolchain": None, "provenance": "NOT_COMPILER_EVIDENCE"}, "sha256": SHA(payloads["fc-bootstrap.exe"]),
                "frontend": frontend, "features": real_packager_features(), "gates": {"release_tag": "NOT_ALLOWED"},
                "pe_audit": {"structural_errors": []}}
    ledger(manifest, payloads)
    return manifest, payloads


def ledger(manifest, payloads):
    manifest["files"] = [{"path": name, "size": len(data), "sha256": SHA(data)} for name, data in payloads.items()]


def zipped(manifest, payloads, extra=()):
    output = io.BytesIO()
    with zipfile.ZipFile(output, "w", compression=zipfile.ZIP_DEFLATED) as archive:
        archive.writestr("manifest.json", json.dumps(manifest).encode())
        for name, data in payloads.items():
            archive.writestr(name, data)
        for name, data in extra:
            with warnings.catch_warnings():
                warnings.simplefilter("ignore", UserWarning)
                archive.writestr(name, data)
    return output.getvalue()


def export_table(name, columns, rows, primary=None):
    """Model the actual three-header msiinfo IDT wire, not an MSI runtime."""
    return ("\t".join(columns) + "\n" + "\t".join("s255" for _ in columns) + "\n" +
            "\t".join([name] + (primary or columns[:1])) + "\n" +
            "".join("\t".join(row) + "\n" for row in rows)).encode()


def compiled_contract_model():
    # Export schemas/flags are based on root's real wixl 0.103 smoke exports;
    # identifiers below describe a single synthetic file for mutation checks.
    version, path, data = "0.1.0", "LICENSE", b"fixture"
    identifier = SHA(path.encode())[:24]
    cid, fid, rid = "C" + identifier, "F" + identifier, "R" + identifier
    guid = M["component_code"](path)
    tables = {}
    def add(name, columns, rows, primary=None):
        tables[name] = export_table(name, columns.split(), rows, primary)
    add("Property", "Property Value", [[key, value] for key, value in {
        "Manufacturer": "FutureClassroom", "ProductLanguage": "1033", "ProductName": "FutureClassroom MSI candidate",
        "ProductVersion": version, "ProductCode": "{" + M["product_code"](version) + "}", "UpgradeCode": "{" + M["UPGRADE_CODE"] + "}",
        "MSIRESTARTMANAGERCONTROL": "DisableShutdown", "SecureCustomProperties": "WIX_DOWNGRADE_DETECTED;WIX_UPGRADE_DETECTED"}.items()])
    add("Directory", "Directory Directory_Parent DefaultDir", [["TARGETDIR", "", "SourceDir"], ["LocalAppDataFolder", "TARGETDIR", "."], ["UserPrograms", "LocalAppDataFolder", "Programs"], ["FcInstallDir", "UserPrograms", "FutureClassroom"]])
    add("Component", "Component ComponentId Directory_ Attributes Condition KeyPath", [[cid, "{" + guid + "}", "FcInstallDir", "4", "", rid]])
    add("Registry", "Registry Root Key Name Value Component_", [[rid, "1", M["REGISTRY_KEY"], guid, "#1", cid]])
    add("File", "File Component_ FileName FileSize Version Language Attributes Sequence", [[fid, cid, path, str(len(data)), "", "", "512", "1"]])
    add("Feature", "Feature Feature_Parent Title Description Display Level Directory_ Attributes", [["Main", "", "FutureClassroom candidate", "", "2", "1", "", "0"]])
    add("FeatureComponents", "Feature_ Component_", [["Main", cid]], ["Feature_", "Component_"])
    add("Media", "DiskId LastSequence DiskPrompt Cabinet VolumeLabel Source", [["1", "1", "", "#payload.cab", "", ""]])
    add("Upgrade", "UpgradeCode VersionMin VersionMax Language Attributes Remove ActionProperty", [["{" + M["UPGRADE_CODE"] + "}", "", version, "", "1", "", "WIX_UPGRADE_DETECTED"], ["{" + M["UPGRADE_CODE"] + "}", version, "", "", "2", "", "WIX_DOWNGRADE_DETECTED"]], ["UpgradeCode", "VersionMin", "VersionMax", "Language", "Attributes"])
    add("LaunchCondition", "Condition Description", [["NOT WIX_DOWNGRADE_DETECTED", "A newer FutureClassroom candidate is installed."], ["NOT ALLUSERS", "This candidate supports only the current user installation context."], ["Installed OR VersionNT >= 601", "Windows 7 or later is required; actual OS compatibility remains unvalidated."],
        ["NOT RollbackDisabled AND NOT DISABLEROLLBACK", "Rollback must remain enabled for this lifecycle candidate."],
        ['MSIRESTARTMANAGERCONTROL = "DisableShutdown"', "Automatic application shutdown is not allowed by this lifecycle candidate."]])
    add("InstallExecuteSequence", "Action Condition Sequence", [["FindRelatedProducts", "", "25"], ["LaunchConditions", "", "100"], ["CostFinalize", "", "1000"], ["InstallValidate", "", "1400"], ["FcBeginGate", "1", "1499"], ["InstallInitialize", "", "1500"], ["FcRollbackNotify", "1", "1501"], ["FcCommitNotify", "1", "1502"], ["InstallExecute", "", "6500"], ["RemoveExistingProducts", "", "6501"], ["InstallFinalize", "", "6600"]])
    add("InstallUISequence", "Action Condition Sequence", [["FindRelatedProducts", "", "25"], ["LaunchConditions", "", "100"]])
    add("Binary", "Name Data", [["FcLifecycleHelper", "Binary.FcLifecycleHelper"]])
    add("CustomAction", "Action Type Source Target", [["FcBeginGate", "2", "FcLifecycleHelper", 'begin "[FcInstallDir]fc-bootstrap.exe"'], ["FcRollbackNotify", "1282", "FcLifecycleHelper", "rollback"], ["FcCommitNotify", "1538", "FcLifecycleHelper", "commit"]])
    add("MsiEmbeddedChainer", "MsiEmbeddedChainer Condition CommandLine Source Type", [["FcLifecycleGuardian", 'NOT (ACTION = "ADVERTISE")', "guardian", "FcLifecycleHelper", "2"]])
    return b"Template: Intel;1033\nVersion: 500 (1f4)\nSource: 10 (a)\n", tables, version, {path: data}


class Inputs(unittest.TestCase):
    def reject(self, manifest, payloads, extra=()):
        with self.assertRaises((ValueError, zipfile.BadZipFile)):
            M["validate_zip"](zipped(manifest, payloads, extra))

    def test_exact_closed_ledgers_require_complete_web(self):
        manifest, payloads = fixture()
        actual, admitted = M["validate_zip"](zipped(manifest, payloads))
        self.assertEqual(actual, manifest)
        self.assertEqual(admitted, {**payloads, "manifest.json": json.dumps(manifest).encode()})
        self.reject(*fixture(False))
        with self.assertRaises(ValueError):
            M["pe_header"](fixture()[1]["fc-bootstrap.exe"])

    def test_real_packager_features_list_is_retained_and_not_generalized(self):
        manifest, payloads = fixture()
        original = real_packager_features()
        self.assertIsInstance(original["declared_probe_commands"], list)
        self.assertIn("--version", original["declared_probe_commands"])
        accepted, _ = M["validate_zip"](zipped(manifest, payloads))
        self.assertEqual(accepted["features"], original)
        for key, value in (("declared_probe_commands", "--version"), ("declared_probe_commands", [True]), ("unrelated_feature", ["not a scalar"])):
            with self.subTest(key=key, value=value):
                changed = copy.deepcopy(manifest)
                changed["features"][key] = value
                self.reject(changed, payloads)

    def test_traversal_absolute_ads_reserved_and_case_aliases(self):
        for name in ("../outside", "/fc-bootstrap.exe", "C:/fc-bootstrap.exe", "web/assets/a.js:stream", "web/assets/CON.js", "web/assets/a.JS", "web\\index.html", "FC-BOOTSTRAP.EXE", "web/assets/a.js."):
            with self.subTest(path=name):
                self.reject(*fixture(), [(name, b"attacker")])

    def test_duplicate_central_directory_and_unlisted_known_member(self):
        self.reject(*fixture(), [("LICENSE", b"duplicate")])
        self.reject(*fixture(), [("web/assets/extra.js", b"extra")])

    def test_symlink_and_directory_members_are_not_materialized(self):
        manifest, payloads = fixture()
        info = zipfile.ZipInfo("web/assets/evil.js")
        info.create_system = 3
        info.external_attr = (stat.S_IFLNK | 0o777) << 16
        self.reject(manifest, payloads, [(info, b"../../outside")])

    def test_hash_size_bool_and_ledger_set_mismatch(self):
        for update in ({"size": True}, {"size": 1}, {"sha256": "0" * 64}, {"extra": "field"}):
            with self.subTest(update=update):
                manifest, payloads = fixture()
                manifest["files"][0].update(update)
                self.reject(manifest, payloads)
        manifest, payloads = fixture()
        manifest["files"].pop()
        self.reject(manifest, payloads)

    def test_target_status_or_manifest_schema_cannot_promote_readiness(self):
        for key, value in (("target", "i686-pc-windows-gnu"), ("target", "i686-pc-windows-msvc"), ("status", "VALIDATED"), ("schema_version", True), ("extra", "field")):
            with self.subTest(key=key, value=value):
                manifest, payloads = fixture()
                manifest[key] = value
                self.reject(manifest, payloads)

    def test_license_cannot_be_replaced_even_with_updated_manifest_hash(self):
        for name in ("LICENSE", "THIRD-PARTY-NOTICES.txt", "web/THIRD-PARTY-NOTICES.txt"):
            with self.subTest(name=name):
                manifest, payloads = fixture(True)
                payloads[name] = b"substituted license"
                ledger(manifest, payloads)
                self.reject(manifest, payloads)

    def test_web_versions_and_transitive_manifest_binding(self):
        for kind in ("bad_version", "extra_asset_field", "wrong_digest", "unlisted_asset", "false_count", "unmanifested_payload"):
            with self.subTest(kind=kind):
                manifest, payloads = fixture(True)
                web = json.loads(payloads["web/fc-assets.json"])
                if kind == "bad_version":
                    web["apiVersion"] = True
                elif kind == "extra_asset_field":
                    web["assets"][0]["extra"] = 1
                elif kind == "wrong_digest":
                    web["assets"][0]["sha256"] = "0" * 64
                elif kind == "unlisted_asset":
                    web["assets"][0]["path"] = "assets/absent.js"
                elif kind == "false_count":
                    manifest["frontend"]["asset_count"] = True
                else:
                    payloads["web/assets/unlisted.js"] = b"unlisted"
                payloads["web/fc-assets.json"] = json.dumps(web).encode()
                manifest["frontend"]["manifest_sha256"] = SHA(payloads["web/fc-assets.json"])
                ledger(manifest, payloads)
                self.reject(manifest, payloads)

    def test_duplicate_json_keys_and_nonfinite_numbers(self):
        for raw in (b'{"version":1,"version":2}', b'{"number":NaN}', b'{"number":Infinity}'):
            with self.subTest(raw=raw), self.assertRaises(ValueError):
                M["json_document"](raw)

    def test_msi_version_rejects_fourth_field_and_unrepresentable_versions(self):
        for version in ("1.2.3.4", "01.2.3", "256.0.0", "0.256.0", "0.0.65536", "1.2", "1.2.-1", "1.2.3 ", True):
            with self.subTest(version=version), self.assertRaises(ValueError):
                M["version_parts"](version)

    def test_fresh_output_does_not_replace_directories_or_follow_aliases(self):
        with tempfile.TemporaryDirectory(prefix="FutureClassroom-MSI-input-check-") as name:
            parent = Path(name)
            created = M["fresh_output"](parent / "new")
            original = (created / ".owner").read_bytes()
            with self.assertRaises(FileExistsError):
                M["fresh_output"](created)
            self.assertEqual((created / ".owner").read_bytes(), original)
            (parent / "alias").symlink_to(created, target_is_directory=True)
            with self.assertRaises(ValueError):
                M["fresh_output"](parent / "alias" / "child")
            self.assertFalse((created / "child").exists())


class AuthoredContract(unittest.TestCase):
    def test_explicit_lifecycle_import_preserves_final_contract(self):
        summary, tables, version, payloads = compiled_contract_model()
        before = M["verify_msi_contract"](summary, tables, version, payloads)
        standard = [row for row in M["idt_rows"](tables["InstallExecuteSequence"], "InstallExecuteSequence")
                    if row["Action"] not in M["LIFECYCLE_ACTIONS"]]
        intermediate = export_table("InstallExecuteSequence", ["Action", "Condition", "Sequence"],
                                    [[row[key] for key in ("Action", "Condition", "Sequence")] for row in standard])
        imported = M["lifecycle_execute_sequence_idt"](intermediate)
        self.assertEqual(M["idt_rows"](imported, "InstallExecuteSequence")[:len(standard)], standard)
        tables["InstallExecuteSequence"] = imported
        tables["CustomAction"] = M["lifecycle_custom_action_idt"]()
        self.assertEqual(M["verify_msi_contract"](summary, tables, version, payloads), before)
        with self.assertRaises(ValueError):
            M["lifecycle_execute_sequence_idt"](imported)
        bad = intermediate.replace(b"CostFinalize\t\t1000", b"CostFinalize\t\t1499")
        with self.assertRaises(ValueError):
            M["lifecycle_execute_sequence_idt"](bad)
        empty = b"Action\tType\tSource\tTarget\tExtendedType\ns72\ti2\tS72\tS255\tI4\nCustomAction\tAction\n"
        tables["CustomAction"] = empty
        with self.assertRaises(ValueError):
            M["verify_msi_contract"](summary, tables, version, payloads)

    def test_resolved_fixed_file_fact_has_no_quoted_trailing_slash(self):
        directory = "C:\\Owned Classroom\\FutureClassroom\\"
        target = M["LIFECYCLE_ACTIONS"]["FcBeginGate"][1]
        self.assertEqual(target, 'begin "[FcInstallDir]fc-bootstrap.exe"')
        expanded = target.replace("[FcInstallDir]", directory)
        self.assertEqual(expanded, subprocess.list2cmdline(["begin", directory + "fc-bootstrap.exe"]))
        # The old directory-only template is not a correct encoding of the
        # intended argv. This is an encoder contract, not Windows execution.
        old = 'begin "[FcInstallDir]"'.replace("[FcInstallDir]", directory)
        self.assertNotEqual(old, subprocess.list2cmdline(["begin", directory]))

    def test_normal_helper_admission_binds_result_ledger_cargo_and_source(self):
        # Pure provenance-gate vector: these structural PE bytes are never
        # executed and are not presented as compiler/MSVC/runtime evidence.
        with tempfile.TemporaryDirectory(prefix="FutureClassroom-helper-admission-") as name:
            root = Path(name)
            head = "7" * 40
            source = (ROOT / "Cargo.toml").read_bytes()
            inputs = [{"path": "Cargo.toml", "bytes": len(source), "sha256": SHA(source)}]
            inventory = json.dumps(inputs).encode()
            for path in ("input-sha256.json", "input-sha256-after.json"):
                (root / path).write_bytes(inventory)
            package_id = "path+file:///C:/owned/source/crates/fc-bootstrap#fc-bootstrap@0.1.0"
            manifest_path = "C:/owned/source/crates/fc-bootstrap/Cargo.toml"
            metadata_targets = {target: {"name": target, "kind": ["bin"], "crate_types": ["bin"],
                "src_path": "C:/owned/source/crates/fc-bootstrap/" + relative} for target, relative in (
                ("fc-bootstrap", "src/main.rs"), ("fc-installer-lifecycle", "src/bin/fc-installer-lifecycle.rs"))}
            metadata = {"version": 1, "workspace_root": "C:/owned/source", "workspace_members": [package_id],
                "packages": [{"name": "fc-bootstrap", "version": "0.1.0", "id": package_id, "source": None,
                              "manifest_path": manifest_path, "targets": list(metadata_targets.values())}]}
            ledger, steps, binaries = [], [], {}
            for target, phase in (("fc-bootstrap", "product-compile"), ("fc-installer-lifecycle", "lifecycle-compile")):
                data = normal_pe_fixture() + target.encode()
                relative = "product/fc-bootstrap/" + target + ".exe"
                path = root / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(data)
                binaries[target] = data
                ledger.append({"role": "product", "package": "fc-bootstrap", "target": target,
                    "cargo_profile_test": False, "pe_subsystem": 2, "relative_path": relative,
                    "bytes": len(data), "sha256": SHA(data)})
                log = (json.dumps({"reason": "compiler-artifact", "target": metadata_targets[target],
                    "package_id": package_id, "manifest_path": manifest_path,
                    "profile": {"test": False}, "executable": "C:/owned/" + target + ".exe"}) + "\n").encode()
                log_path = "logs/" + phase + ".stdout.log"
                (root / "logs").mkdir(exist_ok=True)
                (root / log_path).write_bytes(log)
                steps.append({"name": phase, "passed": True, "exit_code": 0, "parent_exited": True,
                              "stdout": log_path, "stdout_sha256": SHA(log)})
            metadata_raw = json.dumps(metadata).encode()
            (root / "logs/cargo-metadata.stdout.log").write_bytes(metadata_raw)
            steps.append({"name": "cargo-metadata", "passed": True, "exit_code": 0, "parent_exited": True,
                "timed_out_or_pipe_stalled": False, "log_truncated": False,
                "arguments": ["metadata", "--locked", "--format-version", "1", "--no-deps"],
                "stdout": "logs/cargo-metadata.stdout.log", "stdout_sha256": SHA(metadata_raw)})
            ledger_raw = json.dumps(ledger).encode()
            (root / "executables.json").write_bytes(ledger_raw)
            result = {"status": "COMPILE_CHECKS_PASSED_RUNTIME_NOT_RUN", "target": M["TARGET"], "baseline": "Win7",
                      "source_sha": head, "dispatch_source_sha": head, "input_manifest_sha256": SHA(inventory),
                      "executables_manifest_sha256": SHA(ledger_raw), "commands": steps}
            raw = json.dumps(result).encode()
            (root / "result.json").write_bytes(raw)
            args = SimpleNamespace(build_evidence=root, build_result_sha256=SHA(raw),
                lifecycle_helper=root / "product/fc-bootstrap/fc-installer-lifecycle.exe",
                lifecycle_helper_sha256=SHA(binaries["fc-installer-lifecycle"]))
            def git(arguments, **_):
                return head.encode() + b"\n" if arguments == ["git", "rev-parse", "HEAD"] else b"Cargo.toml\0"
            with patch.dict(M["admit_normal_helper"].__globals__["subprocess"].__dict__, check_output=git):
                helper, facts = M["admit_normal_helper"](args, {"fc-bootstrap.exe": binaries["fc-bootstrap"]}, "0.1.0")
                self.assertEqual(helper, binaries["fc-installer-lifecycle"])
                self.assertIs(facts["profile_test"], False)
                # Neither an edited original Cargo stream nor edited ledger can
                # be laundered through the unchanged accepted result SHA.
                path = root / steps[1]["stdout"]
                original = path.read_bytes()
                path.write_bytes(original.replace(b'"test": false', b'"test": true'))
                with self.assertRaises(ValueError):
                    M["admit_normal_helper"](args, {"fc-bootstrap.exe": binaries["fc-bootstrap"]}, "0.1.0")
                path.write_bytes(original)
                (root / "executables.json").write_bytes(ledger_raw + b" ")
                with self.assertRaises(ValueError):
                    M["admit_normal_helper"](args, {"fc-bootstrap.exe": binaries["fc-bootstrap"]}, "0.1.0")
                (root / "executables.json").write_bytes(ledger_raw)
                with self.assertRaises(ValueError):
                    M["admit_normal_helper"](args, {"fc-bootstrap.exe": b"different payload"}, "0.1.0")
                with self.assertRaises(ValueError):
                    M["admit_normal_helper"](args, {"fc-bootstrap.exe": binaries["fc-bootstrap"]}, "0.1.1")

    def test_metadata_binds_both_opaque_package_id_forms_and_rejects_wrong_identity(self):
        # Original metadata/Cargo streams are modeled, not actual compiler evidence.
        with tempfile.TemporaryDirectory(prefix="FutureClassroom-metadata-admission-") as name:
            root = Path(name)
            (root / "logs").mkdir()
            for package_id in ("path+file:///C:/owned/source/crates/fc-bootstrap#0.1.0",
                               "path+file:///C:/owned/source/crates/fc-bootstrap#fc-bootstrap@0.1.0"):
                package = {"name": "fc-bootstrap", "version": "0.1.0", "source": None, "id": package_id,
                    "manifest_path": "C:/owned/source/crates/fc-bootstrap/Cargo.toml", "targets": [
                        {"name": target, "kind": ["bin"], "crate_types": ["bin"], "src_path": "C:/owned/source/crates/fc-bootstrap/" + relative}
                        for target, relative in (("fc-bootstrap", "src/main.rs"), ("fc-installer-lifecycle", "src/bin/fc-installer-lifecycle.rs"))]}
                metadata = {"version": 1, "packages": [package], "workspace_members": [package_id], "workspace_root": "C:/owned/source"}
                def original_result(value):
                    raw = json.dumps(value).encode()
                    (root / "logs/cargo-metadata.stdout.log").write_bytes(raw)
                    return {"commands": [{"name": "cargo-metadata", "passed": True, "exit_code": 0, "parent_exited": True,
                        "timed_out_or_pipe_stalled": False, "log_truncated": False,
                        "arguments": ["metadata", "--locked", "--format-version", "1", "--no-deps"],
                        "stdout": "logs/cargo-metadata.stdout.log", "stdout_sha256": SHA(raw)}]}
                result = original_result(metadata)
                accepted, targets, _ = M["cargo_metadata_normal_package"](root, result, "0.1.0")
                for target in targets:
                    message = {"package_id": package_id, "manifest_path": package["manifest_path"], "target": targets[target]}
                    M["match_normal_metadata"](message, target, accepted, targets)
                    for key, value in (("package_id", "path+file:///wrong#fc-bootstrap@0.1.0"),
                                       ("manifest_path", "C:/wrong/Cargo.toml"),
                                       ("target", dict(targets[target], src_path="C:/wrong.rs"))):
                        altered = dict(message, **{key: value})
                        with self.subTest(package_id=package_id, field=key), self.assertRaises(ValueError):
                            M["match_normal_metadata"](altered, target, accepted, targets)
                for field, value in (("name", "wrong-bootstrap"), ("version", "0.1.1"), ("targets", [])):
                    altered = copy.deepcopy(metadata)
                    altered["packages"][0][field] = value
                    with self.subTest(package_id=package_id, field=field), self.assertRaises(ValueError):
                        M["cargo_metadata_normal_package"](root, original_result(altered), "0.1.0")
                result = original_result(metadata)
                (root / "logs/cargo-metadata.stdout.log").write_bytes(b"changed original")
                with self.assertRaises(ValueError):
                    M["cargo_metadata_normal_package"](root, result, "0.1.0")
                result = original_result(metadata)
                result["commands"][0]["arguments"] = ["metadata", "--no-deps"]
                with self.assertRaises(ValueError):
                    M["cargo_metadata_normal_package"](root, result, "0.1.0")

    def test_overlay_preserves_normal_pe_prefix_and_exact_closed_effects(self):
        normal = normal_pe_fixture()
        payloads = {name: ("synthetic " + name).encode() for name in (
            "fc-bootstrap.exe", "LICENSE", "PROBE-README.txt", "THIRD-PARTY-NOTICES.txt", "manifest.json",
            "MSI-CANDIDATE.txt", "web/index.html", "web/fc-assets.json", "web/THIRD-PARTY-NOTICES.txt")}
        image = M["lifecycle_image"](normal, "0.1.0", payloads)
        prefix, records = M["parse_lifecycle_image"](image)
        self.assertEqual(prefix, normal)
        self.assertEqual(records, {name: {"bytes": len(data), "sha256": SHA(data), "component_guid": M["component_code"](name)} for name, data in payloads.items()})
        self.assertEqual(image[-16:], b"FCLIFE73-END-v1\0")
        length = int.from_bytes(image[-20:-16], "little")
        self.assertEqual(image[len(normal):len(normal)+8], b"FCLM73\0\0")
        self.assertEqual(len(normal) + length + 20, len(image))
        for invalid in (image[:-1], image + b"trailing", image[:-20] + (32769).to_bytes(4, "little") + image[-16:],
                        image[:len(normal)+8] + b"\x02\x00" + image[len(normal)+10:]):
            with self.subTest(invalid_size=len(invalid)), self.assertRaises(ValueError):
                M["parse_lifecycle_image"](invalid)
        with self.assertRaises(ValueError):
            M["lifecycle_image"](image, "0.1.0", payloads)

    def test_overlay_refuses_test_console_and_unknown_effects(self):
        for payloads in ({"../escape": b"x"}, {"any.exe": b"x"}, {"LICENSE": b""}, {"C:/escape": b"x"}):
            with self.subTest(payloads=payloads), self.assertRaises(ValueError):
                M["lifecycle_image"](normal_pe_fixture(), "0.1.0", payloads)
        with self.assertRaises(ValueError):
            M["lifecycle_image"](normal_pe_fixture(3), "0.1.0", {"LICENSE": b"x"})

    def test_only_exact_native_roles_binary_and_flags_are_admitted(self):
        summary, tables, version, payloads = compiled_contract_model()
        changes = [
            ("CustomAction", b"FcBeginGate\t2", b"FcBeginGate\t2050"),
            ("CustomAction", b'\tbegin "[FcInstallDir]fc-bootstrap.exe"\n', b"\tbegin arbitrary-path\n"),
            ("CustomAction", b"FcRollbackNotify\t1282", b"FcRollbackNotify\t1026"),
            ("CustomAction", b"FcCommitNotify\t1538", b"FcCommitNotify\t66"),
            ("Binary", b"Binary.FcLifecycleHelper", b"Binary.Foreign"),
            ("MsiEmbeddedChainer", b"\tguardian\t", b"\tbegin\t"),
        ]
        for table, before, after in changes:
            changed = dict(tables)
            self.assertIn(before, changed[table])
            changed[table] = changed[table].replace(before, after)
            with self.subTest(table=table), self.assertRaises(ValueError):
                M["verify_msi_contract"](summary, changed, version, payloads)
        changed = dict(tables)
        changed["Binary"] += b"Foreign\tBinary.Foreign\n"
        with self.assertRaises(ValueError):
            M["verify_msi_contract"](summary, changed, version, payloads)

    def test_actual_table_contract_rejects_wrong_compiled_semantics(self):
        summary, tables, version, payloads = compiled_contract_model()
        self.assertEqual(M["verify_msi_contract"](summary, tables, version, payloads)["status"], "PASS")
        mutations = [
            ("Property", b"ProductVersion\t0.1.0", b"ProductVersion\t0.2.0"),
            ("Property", M["product_code"](version).encode(), b"00000000-0000-0000-0000-000000000000"),
            ("Directory", b"LocalAppDataFolder\tTARGETDIR", b"LocalAppDataFolder\tProgramDataFolder"),
            ("Directory", b"FcInstallDir", b"INSTALLDIR"),
            ("Component", M["component_code"]("LICENSE").encode(), b"00000000-0000-0000-0000-000000000000"),
            ("Component", b"FcInstallDir\t4\t", b"FcInstallDir\t260\t"),
            ("Component", b"FcInstallDir\t4\t", b"FcInstallDir\t0\t"),
            ("Registry", b"\t1\tSoftware", b"\t2\tSoftware"),
            ("Registry", M["REGISTRY_KEY"].encode(), b"Software\\Microsoft\\Windows\\CurrentVersion\\Run"),
            ("Registry", b"\t#1\t", b"\t#2\t"),
            ("File", b"\tLICENSE\t7\t", b"\tLICENSE\t8\t"),
            ("FeatureComponents", b"\nMain\t", b"\nOther\t"),
            ("Media", b"#payload.cab", b"payload.cab"),
            ("Upgrade", b"\t0.1.0\t\t1\t", b"\t0.2.0\t\t1\t"),
            ("Upgrade", b"\t2\t\tWIX_DOWNGRADE", b"\t514\t\tWIX_DOWNGRADE"),
            ("LaunchCondition", b"NOT WIX_DOWNGRADE_DETECTED", b"1"),
            ("InstallExecuteSequence", b"\t1501\n", b"\t1401\n"),
        ]
        for name, old, new in mutations:
            with self.subTest(table=name, old=old):
                changed = dict(tables)
                self.assertIn(old, changed[name])
                changed[name] = changed[name].replace(old, new, 1)
                with self.assertRaises(ValueError):
                    M["verify_msi_contract"](summary, changed, version, payloads)
        for changed_summary in (summary.replace(b"Intel", b"x64"), summary.replace(b"500 (1f4)", b"501 (1f5)"), summary.replace(b"10 (a)", b"2 (2)")):
            with self.subTest(summary=changed_summary), self.assertRaises(ValueError):
                M["verify_msi_contract"](changed_summary, tables, version, payloads)
        for name in M["FORBIDDEN_TABLES"]:
            with self.subTest(forbidden=name):
                changed = dict(tables)
                changed[name] = export_table(name, ["Key"], [])
                M["verify_msi_contract"](summary, changed, version, payloads)
                changed[name] = export_table(name, ["Key"], [["unexpected side effect"]])
                with self.assertRaises(ValueError):
                    M["verify_msi_contract"](summary, changed, version, payloads)

    def test_bounded_idt_parser_rejects_duplicate_keys_and_malformed_rows(self):
        for raw in (export_table("Property", ["Property", "Value"], [["X", "1"], ["X", "2"]]),
                    b"Property\tValue\ns72\tl0\nProperty\tProperty\nX\t1\textra\n",
                    b"Property\tValue\ns72\tl0\nWrongTable\tProperty\nX\t1\n"):
            with self.subTest(raw=raw), self.assertRaises(ValueError):
                M["idt_rows"](raw, "Property")

    def test_actual_export_requires_removal_inside_transaction(self):
        def export(initialize=1500, remove=6501, finalize=6600):
            return ("Action\tCondition\tSequence\ns72\tS255\tI2\nInstallExecuteSequence\tAction\n"
                    f"CostFinalize\t\t1000\nInstallValidate\t\t1400\nFcBeginGate\t1\t1499\nInstallInitialize\t\t{initialize}\nFcRollbackNotify\t1\t1501\nFcCommitNotify\t1\t1502\nInstallExecute\t\t6500\nRemoveExistingProducts\t\t{remove}\nInstallFinalize\t\t{finalize}\n").encode()
        self.assertEqual(M["verify_transaction_sequence"](export())["RemoveExistingProducts"], 6501)
        for data in (export(remove=1401), export(remove=1500), export(remove=6500), export(remove=6600), export(remove=6700),
                     export().replace(b"RemoveExistingProducts", b"Missing"),
                     export() + b"RemoveExistingProducts\t\t6502\n",
                     export().replace(b"FcBeginGate\t1", b"FcBeginGate\tPUBLIC_BYPASS")):
            with self.subTest(data=data), self.assertRaises(ValueError):
                M["verify_transaction_sequence"](data)

    def test_maintenance_cannot_skip_hooks_or_supply_operation_authority(self):
        summary, tables, version, payloads = compiled_contract_model()
        # Each hook must run on fresh, cached maintenance and removal routes.
        # These are table/encoder negatives, not simulated MSI executions.
        for hook in ("FcBeginGate", "FcRollbackNotify", "FcCommitNotify"):
            for condition in ("NOT Installed", "Installed", 'REMOVE = "ALL"', "REINSTALL", "PUBLIC_MAINTENANCE_ACK"):
                changed = dict(tables)
                old = (hook + "\t1\t").encode()
                self.assertIn(old, changed["InstallExecuteSequence"])
                changed["InstallExecuteSequence"] = changed["InstallExecuteSequence"].replace(old, (hook + "\t" + condition + "\t").encode())
                with self.subTest(hook=hook, condition=condition), self.assertRaises(ValueError):
                    M["verify_msi_contract"](summary, changed, version, payloads)
        for old, new in [(b'begin "[FcInstallDir]fc-bootstrap.exe"', b'begin repair "[FcInstallDir]fc-bootstrap.exe"'),
                         (b'begin "[FcInstallDir]fc-bootstrap.exe"', b'begin "[TARGETDIR]fc-bootstrap.exe"'),
                         (b'\trollback\n', b'\trollback effects_reconciled=true\n'),
                         (b'\tcommit\n', b'\tcommit uninstall=true\n')]:
            changed = dict(tables)
            self.assertIn(old, changed["CustomAction"])
            changed["CustomAction"] = changed["CustomAction"].replace(old, new)
            with self.subTest(target=new), self.assertRaises(ValueError):
                M["verify_msi_contract"](summary, changed, version, payloads)
        changed = dict(tables)
        changed["Property"] += b"FC_MAINTENANCE_APPROVED\t1\n"
        with self.assertRaises(ValueError):
            M["verify_msi_contract"](summary, changed, version, payloads)

    def test_inspected_candidate_capabilities_do_not_claim_runtime_acceptance(self):
        summary, tables, version, payloads = compiled_contract_model()
        inspected = M["verify_msi_contract"](summary, tables, version, payloads)
        capabilities = inspected["lifecycle_capabilities"]
        self.assertEqual(inspected["status"], "PASS")  # Table inspection only.
        self.assertEqual(capabilities["runtime_validation"], "NOT_RUN")
        self.assertEqual(capabilities["candidate_paths"], ["fresh_install", "healthy_same_manifest_version_reapplication", "formal_uninstall", "known_rollback"])
        self.assertEqual(capabilities["unsupported"], ["damaged_repair", "different_manifest_or_version", "major_upgrade", "run_cleanup"])
        self.assertIn("ABSENT_ONLY", capabilities["run_policy"])
        self.assertIn("NOT_REMOVE_INSTALLED_OR_CLI_AUTHORITY", capabilities["admission"])
        # No caller can turn a returned description into later packaging claims.
        capabilities["runtime_validation"] = "PASS"
        self.assertEqual(M["lifecycle_capabilities"]()["runtime_validation"], "NOT_RUN")

    def test_component_identity_survives_payload_reordering_and_version_change(self):
        self.assertEqual(M["component_code"]("web/index.html"), M["component_code"]("WEB/INDEX.HTML"))
        self.assertNotEqual(M["component_code"]("web/index.html"), M["component_code"]("LICENSE"))
        self.assertEqual(M["product_code"]("0.1.0"), M["product_code"]("0.1.0"))
        self.assertNotEqual(M["product_code"]("0.1.0"), M["product_code"]("0.1.1"))
        names = ["fc-bootstrap.exe", "web/index.html", "LICENSE", "MSI-CANDIDATE.txt"]
        self.assertEqual(M["authoring"]("0.1.0", names), M["authoring"]("0.1.0", list(reversed(names))))

    def test_authored_scope_and_exact_ice38_keypaths_without_approval_gate(self):
        raw = M["authoring"]("0.1.0", ["fc-bootstrap.exe", "web/index.html", "LICENSE"])
        root = ET.fromstring(raw)
        nodes = list(root.iter())
        tags = [node.tag.split("}")[-1] for node in nodes]
        self.assertTrue({"Product", "Package", "MajorUpgrade", "Condition", "File"} <= set(tags))
        self.assertFalse({"Registry", "RemoveRegistryValue", "RemoveFile", "ServiceInstall", "ServiceControl", "Launch", "Shortcut"} & set(tags))
        ns = {"w": M["WIX_NS"]}
        self.assertEqual(root.find("w:Product/w:Package", ns).attrib, {"InstallerVersion": "500", "Compressed": "yes", "InstallScope": "perUser"})
        conditions = [node.text for node in root.findall("w:Product/w:Condition", ns)]
        self.assertIn("NOT ALLUSERS", conditions)
        self.assertIn("Installed OR VersionNT >= 601", conditions)
        self.assertEqual(len(conditions), 4)
        self.assertNotIn(b"FC_CANDIDATE_ACK", raw)
        components = root.findall(".//w:Component", ns)
        self.assertEqual(len(components), 3)
        self.assertTrue(all(node.get("Win64") == "no" and len(node) == 2 and node[0].get("KeyPath") == "no" and node[1].get("KeyPath") == "yes" for node in components))
        self.assertTrue(all(node[1].get("Root") == "HKCU" and node[1].get("Key") == M["REGISTRY_KEY"] and node[1].get("Name") == node.get("Guid") and node[1].get("Value") == "1" for node in components))
        self.assertNotIn(b"ProgramData", raw)
        self.assertNotIn(b"Schedule=", raw)
        self.assertEqual(root.find("w:Product/w:InstallExecuteSequence/w:RemoveExistingProducts", ns).attrib, {"After": "InstallExecute"})
        self.assertEqual([node.get("Id") for node in root.findall("w:Product/w:Binary", ns)], ["FcLifecycleHelper"])
        self.assertEqual(root.findall("w:Product/w:CustomAction", ns), [])
        self.assertEqual(root.findall("w:Product/w:InstallExecuteSequence/w:Custom", ns), [])
        self.assertEqual(M["idt_rows"](M["lifecycle_custom_action_idt"](), "CustomAction"), [
            {"Action": name, "Type": str(kind), "Source": M["HELPER_BINARY"], "Target": target}
            for name, (kind, target, _) in M["LIFECYCLE_ACTIONS"].items()])
        directories = {node.get("Id"): node for node in root.findall(".//w:Directory", ns)}
        self.assertEqual(directories["FcInstallDir"].get("Name"), "FutureClassroom")
        self.assertIn(directories["FcInstallDir"], list(directories["UserPrograms"]))


if __name__ == "__main__":
    unittest.main(verbosity=2)
