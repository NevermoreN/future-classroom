#!/usr/bin/env python3
"""Package an x86 application ZIP without certifying field or release acceptance."""
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
VERSION = "0.1.5"
ARTIFACT = f"FutureClassroom-{VERSION}-windows-x86-dev.zip"
TARGET_ROLES = {
    "i686-pc-windows-gnu": "modern-GNU-development-only",
    "i686-pc-windows-msvc": "modern-MSVC-development-only",
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
        "scope": "Username/password teacher and administrator Web sessions, role-scoped teaching/management UI, loopback maintenance and OS-authorized Web repair. Static payload inclusion does not establish native readiness, certificate trust, firewall or campus reachability; exact-binary and field evidence remain separate.",
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
        "scope": "No browser payload in this package; local features remain available. Pass an explicit --web-dist to include the teacher UI required by normal HTTPS hosting.",
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
            "gui_startup": "normal PE uses Windows GUI subsystem; test harnesses remain CUI; diagnostics preserve inherited pipes/files without attaching or allocating a console and require actual parent wait/exit-code checks; manual normal/repair startup failure has bounded native error feedback, autostart/diagnostics do not show it; exact-binary/runtime/logon evidence recorded separately",
            "local_https_settings": "Startup independently prepares the loopback control listener, installation-owned IP-SAN certificate and CurrentUser ROOT trust/readback. Web settings retain explicit LAN address/port/CIDRs and eligible CurrentUser MY certificate selection. One IOCP/SSPI domain owns at most two credentials; LAN context retirement precedes certificate replacement. No common private key is bundled or firewall changed automatically.",
            "ordinary_backup_restore": "Authenticated loopback Web export/download and bounded binary upload reuse full same-install preflight, explicit impact confirmation, current-security barrier, original shared-worker retirement, independent physical-drain coordinator, cold switch/reconciliation and new login. Local progress/cancellation survives Backend and worker shutdown; power-loss/device evidence remains separate.",
            "explicit_mapping_restore": "Loopback Web catalogs and complete explicit injective class/subject/principal mapping retain source-byte/mapping/revision binding, current installation/security preservation, whole-teaching-collection replacement and original single-slot physical cold restore. No numeric/name auto-match or credential import; full device/power-loss acceptance remains separate.",
            "local_voice_policy": "Global-admin Web review retains voice/identity/runtime revisions, current session authority, same-writer CAS and fail-closed stop. Duration/queue/repeat configuration does not raise fixed hard limits; target policy survives ordinary/mapped restore. Settings metadata does not certify audio devices or campus operation.",
            "default_output_history": "Global-admin Web query observes default-output format support on the original shared workers, bound to current session/authority/runtime. Original query budgets and stale-result rejection remain; settings/jobs are unchanged and physical playback/audibility is not certified.",
            "autostart_system_change": "Local global-admin Web change invokes only the fixed current-user project Run value/current executable command. Revision/preimage review, durable intent/factual settlement, session/execution checks and read-only startup reconciliation remain; actual logoff/login and device evidence are separate.",
            "no_code_reinitialization": "Loopback global-admin Web reset and separate OS-launched never-opened Web repair reuse the original physical-close proof and fixed 13-tree journal. Repair uses a one-use fragment capability and independent session/CSRF; ordinary pages gain no admin authority. Explicit impact confirmations preserve external copies and Windows settings; restart creates new school/admin/DPAPI/install identity. Power-loss/device evidence remains separate.",
            "public_calendar_candidates": "replaceable provider, bounded fixed HTTPS WinHTTP source and strict parser; independent cache, exact-source-year SHA/notice provenance, cross-year conflict and manual-priority mapping; explicit review/adoption with candidate+schedule CAS; default-off durable admin grant for same-source pure Rest/Absent changes only; synchronous hard preemption and actual Win7/Win10 remain unvalidated",
            "compound_timetable_drafts": "Global-admin Web complete-document drafts preserve Odd/Even roles, bounded lossless shared-activity split/merge, dual-table structure changes, explicit loss review and final CAS publication; full touch/DPI/device acceptance remains separate.",
            "declared_probe_commands": ["--version", "--self-check", "--system-probe", "--host-probe <1..30>", "--application-probe", "--https-settings-probe [--certificate-thumbprint <40hex-current-user-MY-thumbprint>]", "--reinitialization-probe", "--migration-probe", "--recovery-probe", "--recovery-probe-reopen <isolated-root> <nonce>", "--autostart-probe", "--voice-settings-probe", "--voice-output-probe", "--subject-visuals-probe", "--public-desktop-probe", "--calendar-probe", "--calendar-download-probe", "--materials-probe", "--audio-silent-probe", "--web-pairing-probe",
                "--web-voice-probe", "--http-iocp-probe <port> <FutureClassroom-Test-marker> <1..120>", "--tls-lifecycle-probe <40hex-current-user-MY-thumbprint> <FutureClassroom-Test-TLS-32hex>", "--tls-business-probe <40hex-current-user-MY-thumbprint> <FutureClassroom-Test-TLS-32hex>", "--native-probe <1..300>"],
            "native_management_workbench": "Product-native management forms are replaced by unified Web setup/login, school/accounts/roles/assignments, subjects, schedule/calendar, display/layout/profiles, independent text/images, network/certificates, audio/autostart and controlled maintenance. Native product UI is the resident classroom, public lesson actions and tray; legacy diagnostic commands are not the normal settings entry.",
            "subject_visuals": "Subject colour remains the single truth. Authorized Web palette/display/profile/layout editing and previews reuse native geometry/rendering rules; teacher appearance reads remain permission scoped. Full device acceptance remains separate.",
            "timetable_limits": "64 KiB complete document including bounded dual history; candidate/adopted provenance implemented; declared notice body is not automatically authenticated or interpreted",
            "resident_desktop": "Coherent SQLite scene facts drive nonactivating resident topmost windows across ordinary foreground changes. Background and schedule/clock/date/countdown surfaces remain click-through; explicit public lesson buttons require no login. OS lock/security desktops and shutdown remain exceptions; full classroom device acceptance is separate.",
            "content_measurement": "complete shared card-body description; width-first actual text width, per-block padding/row gaps/action minima and source warnings; full UTF16 bounded text paging, no truncated measurement sample; native geometry and preview use the same rules; visible/device performance acceptance incomplete",
            "public_and_subject_layout": "Global-admin Web public/scoped profile trees retain joint display/layout CAS, Materials styles/rows/gaps, text/image styles, bounded catalogs and stable scoped IDs. Draft confirmation and Web previews use native scene/layout rules; full device acceptance remains separate.",
            "independent_content": "Global-admin Web scoped text/TXT and ordered PNG/JPEG/BMP management preserves durable Begin, session/content CAS, Clear/target-revision Undo, SHA/WIC preparation, execution checks and page CAS. Uploads use controlled staging; ordinary teacher roles gain no generic layout/content management authority.",
            "independent_content_limits": "10000 Unicode scalars/40000 UTF8 bytes, TXT UTF8 or BOM UTF16, at most 50 image pages, each 10MiB/8MP; shared 32MiB physical raster cache and 4MiB text cache; original bytes share 2GiB with materials, references retain current plus one Undo version; actual owner exit/retire/exclusive delete/finalize and startup namespace reconciliation; registry 8192/head 16384/receipt 65536 lifetime bounds; hard synchronous-call deadlines and full device coverage remain unvalidated",
            "lesson_history": "durable bounded 64 actual intervals, override revision and complete timetable key; in-lesson/recess restart and rollback invalidation; stable cross-day baseline trust materializes only provable unobserved intervals; legacy unproved history remains neutral",
            "desktop_limits": "Shared compositor admission remains 16MiB with conservative native allocation accounting; previews are bounded to 960x540/2MiB. Existing node/depth/glass/leaf limits remain; OS backing and full visible-device acceptance are separate.",
            "materials": "local owner/global management20 and current-scene5 filename rows; validated pinned immutable originals and independent editable launch copies; constrained PDF helper and full WIC decode; dedicated STA association open; no Web launch",
            "material_limits": "original <=50MiB/image<=10MiB/8MP, originals2GiB, launchcopies200MiB/128files/24h grace; local global-admin inspection and expired exclusive cleanup; full adversarial/device matrices incomplete",
            "voice_execution": "Exact original Web session independent of Backend/KDF lock; bounded writer wait and on-demand single physical WinMM thread, tray/WTS/power/shutdown stop fence and endpoint monitor; canonical staged WAV/3x16KiB buffers; normal HTTPS upload/submit/status/cancel and browser recording are connected. Real microphone, audible playback and campus integration remain separate field acceptance.",
            "http_transport": "Normal startup provides resident loopback control and independently enabled LAN HTTPS on one bounded IOCP/SSPI domain. Business drain completes before cold worker handoff while local progress remains available. Full-backup wire sizes/budgets are admitted only for authenticated loopback maintenance; ordinary business limits and first-header budget remain. HTTP.sys is historical diagnostics only.",
            "tls_frontdoor_candidate": "Schannel TLS1.2 starts loopback control automatically; LAN remains explicitly configured/enabled. Independent eligible CurrentUser MY certificates use at most two credentials on the original SSPI worker, exact SAN validation and fixed source policies. Provisioning/trust, native readiness and campus reachability remain distinct evidence.",
            "tls_business_probe": "explicit 45-second isolated loopback Native TLS diagnostic with synthetic accounts and trusted fixture approval, same two shared business workers, bounded streaming material upload/download, JSON Ready cancellation/reuse and durable append retry; HEAD errors have zero wire body/no fabricated CL0, 405 carries exact Allow, transport Date and 304 framing/revalidation, exact static manifest/SHA/CSP resources; actual native drain, shared-worker join, upload/download Hub cleanup and owned deployment cleanup required; not visible pairing approval UI, ordinary listener, campus deployment, real OS trust provisioning or strict5s acceptance",
            "native_browser_content_probe": "separate owned Win11 headless Edge/CDP observer against the finite Native TLS instance; reuses the client's already-approved HttpOnly cookie, obtains actual session CSRF, selects the synthetic homework scope, submits once and reloads to verify durable content; manifest/CSP assets and wire responses are separated from browser cache hits; original Windows Job actual exit/profile cleanup tracked separately; no browser pairing UI, real microphone, playback, foreground or product/Win7/Win10/MSVC acceptance claim",
            "web_pairing_port": "Normal teacher/administrator login uses username/password and sessionStorage page credentials. Refresh/reconnect/ordinary idle preserve the open-page session; normal close/new open uses standard tab-storage login semantics. Pairing is legacy diagnostic compatibility, not normal product login. Per-request authorization/revocation and CSRF remain enforced.",
            "teacher_voice_port": "Current-bound-class/exact Web authorization; canonical PCM16 mono16k WAV typed upload, durable jobs/status/cancel and original-command query; AudioWorklet/Offline/Worker browser capture; STOPPING awaits actual driver return; finite actual TLS diagnostic verifies zero PCM WAV upload only, not TLS broadcast/playback or hardware capture; device capability UNVERIFIED and production network deadlines not validated",
            "teacher_material_port": "Authenticated material scopes/classes paging, fixed-slot controlled staging, validation before append, versioned delete/download and sender-only command query; local synthetic probe includes full50MiB; finite actual TLS verifies smaller synthetic streaming upload/download, JSON Ready cancellation/reuse and durable append retry; full size/adversarial/network-deadline/campus matrices remain unvalidated",
            "teacher_content_port": "Authenticated current-day homework/shared-reminder scopes/read/save/clear, exact CAS, durable command outcome, midnight expiry and observed-day rollback protection; direct CNG/DPAPI/SQLite application-port probes plus finite real Native TLS Win11 Edge synthetic homework UI single-save/reload evidence with original approved session; shared-reminder browser, visible classroom and ordinary network delivery remain separate acceptance gates",
            "http_transport_initial_header_deadline": "Normal Schannel host retains the initial five-second first-header deadline without resetting it on handoff; timeout requests negative cancellation and retains unreturned owners until actual drain. This is not an arbitrary OS-scheduling hard-realtime peer-close guarantee; full campus/P0 calibration remains separate. Historical HTTP.sys HeaderWait observations do not describe the normal host.",
            "normal_start": "Manual/autostart launch the resident classroom/tray and process-owned local Web control plane. First initialization and all product settings use Web; tray status/settings open the verified loopback endpoint and normal exit needs no administrator password. Deployment remains project-only protected ProgramData.",
            "native_visual_probe": "Finite native compositor/visibility probes are separate from interactive classroom QA. Normal policy is nonactivating residency across foreground changes, with OS lock/security-desktop exceptions.",
            "acceptance_scope": "Win7/Win10 physical tests are NOT_RUN by user decision, including future releases; compatibility design/build targets remain. Limited Win11 runtime and the authorized five-minute observation do not certify complete classroom/audio/campus operation or long-term stability. Historical packaging classifications do not establish feature completion.",
            "complete_application": "Native resident classroom and unified teacher/administrator Web paths are implemented; packaging alone does not certify complete D11/D12 field, performance or long-term stability acceptance.",
            "teacher_management_web_application": "Username/password page sessions serve role-scoped teacher content/material/voice and administrator account/school/schedule/display/profile/layout/network/audio/autostart management. Backup/restore/reset and OS repair use distinct loopback-only controls and original cold ownership. Frontend inclusion is separate from field acceptance.",
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
