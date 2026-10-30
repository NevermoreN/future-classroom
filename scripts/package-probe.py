#!/usr/bin/env python3
"""Create an explicitly unvalidated, minimal x86 development probe ZIP."""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import runpy
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


def build_package(executable: Path, output: Path, target: str, objdump: str, declared_toolchain: str | None) -> dict:
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
    entries = [{"path": name, "size": path.stat().st_size, "sha256": sha256_file(path)} for name, path in files.items()]
    if entries[0]["sha256"] != audited_digest:
        raise ValueError("Executable changed while its PE imports were inspected.")
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
        "features": {
            "declared_probe_commands": ["--version", "--self-check", "--system-probe", "--host-probe <1..30>", "--application-probe", "--materials-probe", "--audio-silent-probe", "--http-iocp-probe <port> <FutureClassroom-Test-marker> <1..120>", "--native-probe <1..300>"],
            "native_management_workbench": "initial setup/login, school/classes/subjects, account creation, password/recovery rotation, homework, native timetable grid/digit mask/dual history/calendar and display drafts, local global-admin current-lesson override/restore with captured CAS and durable actual-teacher intervals, unsaved-exit guard; full application incomplete",
            "timetable_limits": "64 KiB complete document including bounded dual history; public calendar source verification incomplete",
            "resident_desktop": "coherent SQLite facts, native Direct2D cards, 12/24/analog clocks, schedule/homework/material paging, full recess region backgrounds and small dynamic surfaces; separate bounded current-lesson control strip; full profile/layout and actual visibility QA incomplete",
            "lesson_history": "durable bounded 64 actual intervals, override revision and complete timetable key; in-lesson/recess restart and rollback invalidation; stable cross-day baseline trust materializes only provable unobserved intervals; legacy unproved history remains neutral",
            "desktop_limits": "16 MiB aggregate application DIB cap; 4K hidden native region background test uses 4806080 DIB bytes; OS backing, visible display and actual device acceptance remain incomplete",
            "materials": "local owner/global management20 and current-scene5 filename rows; validated pinned immutable originals and independent editable launch copies; constrained PDF helper and full WIC decode; dedicated STA association open; no Web launch",
            "material_limits": "original <=50MiB/image<=10MiB/8MP, originals2GiB, launchcopies200MiB/128files/24h grace; local global-admin inspection and expired exclusive cleanup; full adversarial/device matrices incomplete",
            "voice_execution": "exact original Web session independent of Backend/KDF lock; bounded writer wait and on-demand single physical WinMM thread, tray/WTS/power/shutdown stop fence and endpoint monitor; canonical staged WAV/3x16KiB buffers; actual teacher Web submission and browser recording incomplete",
            "http_transport": "HTTP.sys v2 aligned fixed slots/OVERLAPPED/IOCP with bounded body/response pieces, cancel/drain and cycle tokens; explicit two-cycle synthetic loopback diagnostic only; production listener, TLS deployment, pairing and browser application incomplete",
            "http_transport_initial_header_deadline": "NOT_VALIDATED: HTTP.sys uses driver defaults before the first complete header is attributed to a URL group; group HeaderWait=5 does not prove a first-request 5s bound. Kernel connection limits/QoS and P0 platform decision remain open.",
            "normal_start": "manual workbench; --autostart stays locked and hidden; creates project-only protected ProgramData deployment",
            "native_visual_probe": "3% glass, clock text, click-through and foreground avoidance; hidden scene tests do not replace interactive QA",
            "complete_application": "NOT_IMPLEMENTED",
            "teacher_management_web_application": "NOT_IMPLEMENTED",
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
    # Never glob the workspace. Only these four payloads and this manifest can
    # enter the ZIP; source plans, reference images, secrets and caches cannot.
    output.parent.mkdir(parents=True, exist_ok=True)
    created = False
    try:
        with zipfile.ZipFile(output, mode="x", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
            created = True
            for name, source in files.items():
                archive_file(archive, name, source)
            info = zipfile.ZipInfo("manifest.json", (1980, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            info.create_system = 3
            info.external_attr = 0o100644 << 16
            archive.writestr(info, json.dumps(manifest, ensure_ascii=False, indent=2, sort_keys=True).encode("utf-8"))
        # Ensure bytes did not change between hashing and reading the archive.
        with zipfile.ZipFile(output) as archive:
            if set(archive.namelist()) != {*files, "manifest.json"}:
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
    args = parser.parse_args()
    try:
        manifest = build_package(args.exe.resolve(), args.output.resolve(), args.target, args.objdump, args.toolchain)
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
