#!/usr/bin/env python3
"""Create an explicitly unvalidated, minimal x86 development probe ZIP."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import runpy
import stat
import subprocess
import sys
import zipfile


ROOT = Path(__file__).resolve().parent.parent
VERSION = "0.1.0"
ARTIFACT = f"FutureClassroom-{VERSION}-windows-x86-dev.zip"
TARGET_ROLES = {
    "i686-pc-windows-gnu": "modern-GNU-development-only",
    "i686-win7-windows-gnu": "supplemental-GNU-not-formal-MSVC-baseline",
    "i686-win7-windows-msvc": "formal-candidate-unvalidated",
}
MAX_WEB_FILES = 64
MAX_WEB_BYTES = 5 * 1024 * 1024
MAX_WEB_MANIFEST_BYTES = 64 * 1024
MAX_WEB_NOTICE_BYTES = 1024 * 1024


def real_directory(path: Path) -> None:
    """Reject directory aliases in the selected build path, including ancestors."""
    absolute = path.absolute()
    for directory in (absolute, *absolute.parents):
        info = directory.lstat()
        reparse = getattr(info, "st_file_attributes", 0) & getattr(stat, "FILE_ATTRIBUTE_REPARSE_POINT", 0)
        if not stat.S_ISDIR(info.st_mode) or reparse:
            raise ValueError("Expected a real directory without symbolic links: " + str(directory))


def bounded_regular_file(path: Path, limit: int) -> bytes:
    """Capture an owned build file; never follow a final symbolic link."""
    info = path.lstat()
    reparse = getattr(info, "st_file_attributes", 0) & getattr(stat, "FILE_ATTRIBUTE_REPARSE_POINT", 0)
    if not stat.S_ISREG(info.st_mode) or reparse or info.st_size > limit:
        raise ValueError("Expected a bounded regular file: " + str(path))
    flags = os.O_RDONLY | getattr(os, "O_BINARY", 0) | getattr(os, "O_NOFOLLOW", 0) | getattr(os, "O_NONBLOCK", 0)
    descriptor = os.open(path, flags)
    with os.fdopen(descriptor, "rb") as source:
        opened = os.fstat(source.fileno())
        if not stat.S_ISREG(opened.st_mode) or opened.st_size > limit:
            raise ValueError("Build input is not a bounded regular file: " + str(path))
        data = source.read(limit + 1)
        if len(data) > limit or len(data) != opened.st_size:
            raise ValueError("Build input changed size while reading: " + str(path))
        return data


def valid_web_path(path: object) -> bool:
    """Match the Rust static manifest's flat ASCII allowlist, including Win32 aliases."""
    if path == "index.html":
        return True
    if not isinstance(path, str) or not path.startswith("assets/"):
        return False
    name = path[len("assets/"):]
    if not 1 <= len(name) <= 128 or name.startswith(".") or re.fullmatch(r"[A-Za-z0-9._-]+", name) is None:
        return False
    stem = name.split(".", 1)[0].upper()
    if stem in {"CON", "PRN", "AUX", "NUL"} or re.fullmatch(r"(?:COM|LPT)[1-9]", stem):
        return False
    return "." in name and name.rsplit(".", 1)[-1] in {"js", "css", "svg", "png", "jpg", "jpeg", "webp"}


def frontend_payloads(web_dist: Path) -> tuple[dict[str, bytes], dict]:
    """Only a validated manifest chooses payloads. No directory glob or request path."""
    real_directory(web_dist)
    raw = bounded_regular_file(web_dist / "fc-assets.json", MAX_WEB_MANIFEST_BYTES)

    def unique_object(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise ValueError("Duplicate Web manifest field.")
            result[key] = value
        return result

    def invalid_constant(_):
        raise ValueError("Non-finite Web manifest number.")

    try:
        document = json.loads(raw.decode("utf-8"), object_pairs_hook=unique_object, parse_constant=invalid_constant)
    except (UnicodeError, RecursionError, json.JSONDecodeError) as error:
        raise ValueError("Invalid UTF-8 Web manifest JSON.") from error
    if not isinstance(document, dict) or set(document) != {"schemaVersion", "apiVersion", "uiCompatibilityVersion", "assets"}:
        raise ValueError("Unexpected Web manifest fields.")
    for version in ("schemaVersion", "apiVersion", "uiCompatibilityVersion"):
        if type(document[version]) is not int or document[version] != 1:
            raise ValueError("Unsupported Web manifest version.")
    assets = document["assets"]
    if not isinstance(assets, list) or not 1 <= len(assets) <= MAX_WEB_FILES:
        raise ValueError("Web manifest asset count exceeds its bound.")
    seen = set()
    declared = []
    total = 0
    for asset in assets:
        if not isinstance(asset, dict) or set(asset) != {"path", "sizeBytes", "sha256"}:
            raise ValueError("Unexpected Web asset fields.")
        path, size, digest = asset["path"], asset["sizeBytes"], asset["sha256"]
        if not valid_web_path(path) or path.lower() in seen:
            raise ValueError("Invalid or case-aliased Web asset path.")
        if type(size) is not int or not 1 <= size <= MAX_WEB_BYTES:
            raise ValueError("Invalid Web asset size.")
        if not isinstance(digest, str) or re.fullmatch(r"[0-9a-f]{64}", digest) is None:
            raise ValueError("Invalid Web asset SHA-256.")
        total += size
        if total > MAX_WEB_BYTES:
            raise ValueError("Web static payload exceeds 5 MiB.")
        seen.add(path.lower())
        declared.append((path, size, digest))
    if "index.html" not in seen:
        raise ValueError("Web manifest must declare index.html.")
    payloads = {"web/fc-assets.json": raw}
    for path, size, digest in sorted(declared):
        source = web_dist / path
        if path.startswith("assets/"):
            real_directory(web_dist / "assets")
        data = bounded_regular_file(source, size)
        if len(data) != size or hashlib.sha256(data).hexdigest() != digest:
            raise ValueError("Web asset size or digest mismatch: " + path)
        payloads["web/" + path] = data
    real_directory(ROOT / "web")
    notices = bounded_regular_file(ROOT / "web" / "THIRD-PARTY-NOTICES.txt", MAX_WEB_NOTICE_BYTES)
    if not notices:
        raise ValueError("Web third-party notices are empty.")
    payloads["web/THIRD-PARTY-NOTICES.txt"] = notices
    return payloads, {
        "included": True,
        "status": "PARTIAL_NOT_VALIDATED",
        "manifest_path": "web/fc-assets.json",
        "manifest_sha256": hashlib.sha256(raw).hexdigest(),
        "schema_version": 1,
        "api_version": 1,
        "ui_compatibility_version": 1,
        "asset_count": len(declared),
        "asset_bytes": total,
        "third_party_notices": "web/THIRD-PARTY-NOTICES.txt",
        "scope": "Pairing/session plus authenticated homework/shared-reminder UI and content application ports; material metadata and controlled file preparation/upload/download ports; voice canonical WAV upload/job/cancel ports and browser AudioWorklet recording; normal TLS/listener and event-to-stage scheduling disabled.",
    }


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(64 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def archive_file(archive: zipfile.ZipFile, name: str, source: Path) -> None:
    info = zipfile.ZipInfo(name, (1980, 1, 1, 0, 0, 0))
    info.compress_type = zipfile.ZIP_DEFLATED
    info.create_system = 3
    info.external_attr = 0o100644 << 16
    with source.open("rb") as input_file, archive.open(info, "w") as output:
        for block in iter(lambda: input_file.read(64 * 1024), b""):
            output.write(block)


def archive_bytes(archive: zipfile.ZipFile, name: str, data: bytes) -> None:
    info = zipfile.ZipInfo(name, (1980, 1, 1, 0, 0, 0))
    info.compress_type = zipfile.ZIP_DEFLATED
    info.create_system = 3
    info.external_attr = 0o100644 << 16
    archive.writestr(info, data)


def build_package(executable: Path, output: Path, target: str, objdump: str, declared_toolchain: str | None, web_dist: Path | None = None) -> dict:
    audit_pe = runpy.run_path(str(ROOT / "scripts" / "audit-pe.py"))["audit_pe"]
    audited_digest = sha256_file(executable)
    audit = audit_pe(executable, objdump)
    if audit["structural_errors"]:
        raise ValueError("Probe PE format rejected: " + "; ".join(audit["structural_errors"]))
    if output.suffix.lower() != ".zip":
        raise ValueError("Output must be an explicit .zip file path.")
    if output.exists():
        raise ValueError("Output already exists; choose a new path or explicitly remove the prior generated artifact.")
    toolchain = subprocess.run(["rustc", "-Vv"], cwd=ROOT, check=True, capture_output=True, text=True, timeout=30).stdout.strip()
    files = {
        "fc-bootstrap.exe": executable,
        "LICENSE": ROOT / "LICENSE",
        "PROBE-README.txt": ROOT / "packaging" / "PROBE-README.txt",
        "THIRD-PARTY-NOTICES.txt": ROOT / "packaging" / "THIRD-PARTY-NOTICES.txt",
    }
    web_files, frontend = frontend_payloads(web_dist) if web_dist is not None else ({}, {
        "included": False,
        "status": "NOT_INCLUDED",
        "scope": "No browser payload in this package; pass an explicit --web-dist to include the verified partial teacher UI.",
    })
    entries = [{"path": name, "size": path.stat().st_size, "sha256": sha256_file(path)} for name, path in files.items()]
    if entries[0]["sha256"] != audited_digest:
        raise ValueError("Executable changed while its PE imports were inspected.")
    entries.extend({"path": name, "size": len(data), "sha256": hashlib.sha256(data).hexdigest()} for name, data in sorted(web_files.items()))
    manifest = {
        "schema_version": 1,
        "product": "FutureClassroom",
        "version": VERSION,
        "artifact_name": output.name,
        "artifact_type": "development-probe",
        "status": "NOT_VALIDATED",
        "target": target,
        "target_role": TARGET_ROLES[target],
        "toolchain": {
            "packaging_environment": toolchain,
            "declared_build_toolchain": declared_toolchain,
            "provenance": "Toolchain output records the packager environment; binary build provenance is not independently established.",
        },
        "sha256": entries[0]["sha256"],
        "frontend": frontend,
        "features": {
            "ordinary_backup_restore": "local global-admin native export and opaque-picker verified non-overwrite save; full same-install isolated preflight, sequence-bound explicit impact review, current-security barrier, fixed shared-worker retirement, independent final physical-drain coordinator, three-tree cold switch/startup reconciliation and mandatory new login; cross-install mapping/reinitialization, visible dialogs/device/power-loss and formal three-OS acceptance incomplete",
            "autostart_system_change": "local global-admin captured revision/preimage explicit review; fixed current-user project Run value/current-exe quoted command; durable Prepared/Applying/Succeeded/Failed/NeedsReconcile intent, exact ticket factual settlement and read-only startup CAS reconciliation without replay/automatic repair; fresh session/authority/execution checks, no HTTP effect API; actual logoff/login, Win7/Win10/MSVC and visible Seewo acceptance incomplete",
            "no_code_reinitialization": "explicit abandon-all-local-data wizard in a separate never-opened repair process; real physical close, root identity claim and fixed 13-tree journaled switch retain root/runtime instance lock; two default-unconfirmed sequence-bound acknowledgements; clears local exports/credentials/media/voice, preserves external saved copies and Windows settings; restart creates new school/admin/DPAPI/install identity; cross-install mapping and hardware power-loss/real Win7/Win10 acceptance incomplete",
            "public_calendar_candidates": "replaceable provider, bounded fixed HTTPS WinHTTP source and strict parser; independent cache, exact-source-year SHA/notice provenance, cross-year conflict and manual-priority mapping; explicit review/adoption with candidate+schedule CAS; default-off durable admin grant for same-source pure Rest/Absent changes only; synchronous hard preemption and actual Win7/Win10 remain unvalidated",
            "compound_timetable_drafts": "local global-admin Odd/Even role swap preserving physical table identity and content; lossless shared activity weekday split and compatible merge, synchronized dual-table atomic structure edits; explicit kind-change loss review/cancel/owner lock resume; only final complete-document CAS Save publishes; visible touch/DPI and three-OS acceptance incomplete",
            "declared_probe_commands": ["--version", "--self-check", "--system-probe", "--host-probe <1..30>", "--application-probe", "--reinitialization-probe", "--recovery-probe", "--recovery-probe-reopen <isolated-root> <nonce>", "--autostart-probe", "--calendar-probe", "--calendar-download-probe", "--materials-probe", "--audio-silent-probe", "--web-pairing-probe",
                "--web-voice-probe", "--http-iocp-probe <port> <FutureClassroom-Test-marker> <1..120>", "--native-probe <1..300>"],
            "native_management_workbench": "initial setup/login, school/classes/subjects, account list/create/edit/disable with multiple teaching scopes, explicit review/cancel/CAS and self-change re-login, independent busy lock action, password/recovery rotation, homework, native timetable grid/digit mask/dual history/calendar and display drafts, local global-admin current-lesson override/restore with captured CAS and durable actual-teacher intervals, unsaved-exit guard; full application incomplete",
            "timetable_limits": "64 KiB complete document including bounded dual history; candidate/adopted provenance implemented; declared notice body is not automatically authenticated or interpreted",
            "resident_desktop": "coherent SQLite facts with selected exact class/subject/teacher profile; width-first Row/Column/Wrap tree, native premultiplied subtree group opacity, clipped same-generation inputs, 12/24/analog clocks, schedule/homework/material paging, full recess region backgrounds and small dynamic patches; separate current-lesson control strip; actual visibility QA incomplete",
            "content_measurement": "complete shared card-body description; width-first actual text width, per-block padding/row gaps/action minima and source warnings; full UTF16 bounded text paging, no truncated measurement sample; native geometry and preview use the same rules; visible/device performance acceptance incomplete",
            "public_and_subject_layout": "local global-admin public tree and joint display/layout captured CAS; independent exact class/subject/teacher profiles with Materials styles/1..5 rows/gap and per-node text/image styles; bounded paged catalog, stable scoped node IDs, draft review/cancel/owner lock/exit guard; native preview uses examples for builtins and actual published standalone content; visible device acceptance incomplete",
            "independent_content": "local global-admin node text/TXT and ordered PNG/JPEG/BMP pages; durable latest Begin, exact session/content CAS, explicit Clear/target-revision Undo, actual SHA and complete WIC preparation, worker execution checks at COMMIT, exact-current-scene manual page CAS; teacher Web has no generic content/layout write API",
            "independent_content_limits": "10000 Unicode scalars/40000 UTF8 bytes, TXT UTF8 or BOM UTF16, at most 50 image pages, each 10MiB/8MP; shared 32MiB physical raster cache and 4MiB text cache; original bytes share 2GiB with materials, references retain current plus one Undo version; actual owner exit/retire/exclusive delete/finalize and startup namespace reconciliation; registry 8192/head 16384/receipt 65536 lifetime bounds; hard synchronous-call deadlines and full device coverage remain unvalidated",
            "lesson_history": "durable bounded 64 actual intervals, override revision and complete timetable key; in-lesson/recess restart and rollback invalidation; stable cross-day baseline trust materializes only provable unobserved intervals; legacy unproved history remains neutral",
            "desktop_limits": "16 MiB compositor application RGBA admission including DIB caches, simultaneous CPU scratch and conservative software-target reserve; preview releases the hidden classroom cache and exports at most 960x540/2MiB without a full-screen intermediate; 128 composed logical nodes/depth16, two glass levels/eight outer glass cards/64 actual visible leaves; OS backing, visible display and actual device acceptance remain incomplete",
            "materials": "local owner/global management20 and current-scene5 filename rows; validated pinned immutable originals and independent editable launch copies; constrained PDF helper and full WIC decode; dedicated STA association open; no Web launch",
            "material_limits": "original <=50MiB/image<=10MiB/8MP, originals2GiB, launchcopies200MiB/128files/24h grace; local global-admin inspection and expired exclusive cleanup; full adversarial/device matrices incomplete",
            "voice_execution": "exact original Web session independent of Backend/KDF lock; bounded writer wait and on-demand single physical WinMM thread, tray/WTS/power/shutdown stop fence and endpoint monitor; canonical staged WAV/3x16KiB buffers; canonical teacher Web upload/submit/status/cancel ports and browser recording implemented; real TLS/campus/microphone integration incomplete",
            "http_transport": "HTTP.sys v2 aligned fixed slots/OVERLAPPED/IOCP with bounded body/response pieces, cancel/drain and cycle tokens; explicit two-cycle synthetic loopback diagnostic only; production listener, TLS deployment and browser application incomplete",
            "web_pairing_port": "direct service pairing with process-local eight-digit one-use request and exact local-global-admin approval, 256-bit poll/cookie/CSRF secrets and independent device leases; CNG/DPAPI/SQLite plus synthetic trusted-TLS request metadata through session/CSRF/logout ports only, not end-to-end POST pairing dispatch; real HTTPS/TLS/browser/listening delivery incomplete",
            "teacher_voice_port": "Current-bound-class/exact Web authorization; canonical PCM16 mono16k WAV typed upload, durable jobs/status/cancel and original-command query; AudioWorklet/Offline/Worker browser capture; STOPPING awaits actual driver return; device capability UNVERIFIED; normal TLS/network deadline not validated",
            "teacher_material_port": "Authenticated material scopes/classes paging, controlled fixed-slot file staging, full validation before append, fixed-version delete/download and sender-only durable command query; native synthetic metadata probe includes full50MiB, not actual TLS/network staging deadline",
            "teacher_content_port": "Authenticated Web application-port scopes, current-day homework/shared-reminder read/save/clear, exact CAS, durable original-command outcome, midnight expiry and persistent observed-day rollback protection; CNG/DPAPI/SQLite probe uses synthetic trusted transport metadata; no actual TLS/browser/network delivery evidence",
            "http_transport_initial_header_deadline": "NOT_VALIDATED: HTTP.sys uses driver defaults before the first complete header is attributed to a URL group; group HeaderWait=5 does not prove a first-request 5s bound. Kernel connection limits/QoS and P0 platform decision remain open.",
            "normal_start": "manual workbench; --autostart stays locked and hidden; creates project-only protected ProgramData deployment",
            "native_visual_probe": "3% glass, clock text, click-through and foreground avoidance; hidden scene tests do not replace interactive QA",
            "complete_application": "NOT_IMPLEMENTED",
            "teacher_management_web_application": "PARTIAL: pairing/session, homework/shared reminders with CAS and durable original-command query; material metadata and controlled file preparation/upload/download ports; voice canonical WAV upload/job/cancel ports and browser AudioWorklet recording; normal TLS/listener disabled; payload inclusion is recorded separately in frontend.included",
        },
        "gates": {
            "Win7_SP1_x86": "NOT_VALIDATED",
            "Windows_10": "NOT_VALIDATED",
            "Windows_11_complete_product": "NOT_VALIDATED",
            "formal_MSVC_baseline": "NOT_VALIDATED",
            "real_machine_acceptance": "NOT_VALIDATED",
            "release_tag": "NOT_ALLOWED",
        },
        "files": entries,
        "pe_audit": audit,
    }
    # Never glob the workspace. The fixed native payloads, optionally validated
    # Web manifest entries/notices, and this manifest are the entire allowlist.
    output.parent.mkdir(parents=True, exist_ok=True)
    created = False
    try:
        with zipfile.ZipFile(output, mode="x", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
            created = True
            for name, source in files.items():
                archive_file(archive, name, source)
            for name, data in sorted(web_files.items()):
                archive_bytes(archive, name, data)
            info = zipfile.ZipInfo("manifest.json", (1980, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            info.create_system = 3
            info.external_attr = 0o100644 << 16
            archive.writestr(info, json.dumps(manifest, ensure_ascii=False, indent=2, sort_keys=True).encode("utf-8"))
        # Ensure bytes did not change between hashing and reading the archive.
        with zipfile.ZipFile(output) as archive:
            if set(archive.namelist()) != {*files, *web_files, "manifest.json"}:
                raise ValueError("Unexpected archive member set.")
            for entry in entries:
                digest = hashlib.sha256()
                with archive.open(entry["path"]) as member:
                    for block in iter(lambda: member.read(64 * 1024), b""):
                        digest.update(block)
                if digest.hexdigest() != entry["sha256"]:
                    raise ValueError("Input changed while packaging: " + entry["path"])
    except BaseException:
        if created:
            output.unlink(missing_ok=True)
        raise
    return manifest


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--exe", type=Path, required=True, help="Explicit built EXE path; this script never builds or runs it.")
    parser.add_argument("-o", "--output", type=Path, required=True, help=f"Explicit output ZIP path, usually artifacts/{ARTIFACT}")
    parser.add_argument("--target", choices=TARGET_ROLES, default="i686-pc-windows-gnu")
    parser.add_argument("--objdump", default="i686-w64-mingw32-objdump")
    parser.add_argument("--toolchain", help="Optional declared build toolchain; does not certify binary provenance.")
    parser.add_argument("--web-dist", type=Path, help="Explicit built Web directory; only validated fc-assets.json entries and Web notices are included.")
    args = parser.parse_args()
    try:
        manifest = build_package(args.exe.resolve(), args.output.resolve(), args.target, args.objdump, args.toolchain, args.web_dist)
    except (OSError, ValueError, subprocess.SubprocessError, zipfile.BadZipFile) as error:
        print(f"Probe package failed: {error}", file=sys.stderr)
        return 1
    print(f"Created {args.output}")
    print(f"SHA256 {sha256_file(args.output)}")
    print(f"Status NOT_VALIDATED; Win7 NOT_VALIDATED; PE review triggers: {len(manifest['pe_audit']['risks'])}")
    print("Development probe only. Formal MSVC and real machine gates do not permit a release tag.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
