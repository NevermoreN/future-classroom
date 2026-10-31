#!/usr/bin/env python3
"""Read-only evidence acceptance; run only after every actual probe is terminal.

Writes only the two aggregate summaries, never a raw report. A failed/missing
input writes FAIL aggregates and exits nonzero; it cannot leave stale PASS.
"""
import argparse
import hashlib
import json
import re
import runpy
import sys
import zipfile
from datetime import datetime, timezone
from pathlib import Path

SOURCE_SHA = "642a33271e94c7b5da27a09c0c7f985a35b50f1b77bf54863ccb709d94c9d4e0"
MODERN_SHA = "12c2140a7c1ca0fb1185a339f2e0c855a0d5d92774a39547ca669569f5625137"
SUPPLEMENTAL_SHA = "b15bd3ea838f9b88949d7e8c669609510069f72a6e3fe450a144f0a00e48bab5"
ZIP_SHA = "08c9ca19c0a3388ec58f71823c54577704e043aa7aa4e461e17246dcd2bde93f"
FULL_REPORT = "modern-windows-tests-and-migration-stage1.json"
NOT_RUN = ["real Win7 SP1 x86", "real Windows 10", "formal MSVC baseline",
           "visible Seewo/PPT/touch/DPI", "real logoff/login", "hardware power loss",
           "production TLS/listener and strict P0 deadlines", "installer/upgrade/uninstall/72h"]


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def sha(path):
    with Path(path).open("rb") as handle:
        return hashlib.file_digest(handle, "sha256").hexdigest()


def load(path):
    return json.loads(Path(path).read_text(encoding="utf-8"))


def output_lines(value):
    require(isinstance(value, list) and all(isinstance(v, str) for v in value), "output is not string[]")
    return value


def counts(lines):
    values = []
    for line in output_lines(lines):
        if "test result:" not in line:
            continue
        match = re.fullmatch(r"test result: ok\. (\d+) passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in .+", line.strip())
        require(match is not None, "failed/malformed/incomplete Rust test summary: " + line)
        values.append(int(match.group(1)))
    require(bool(values), "no completed Rust test summary")
    return values


def accept_tests(expected, report, report_name):
    require(report.get("status") == "PASS", report_name + ": overall report is not PASS")
    require(report.get("cleanup") == "PASS", report_name + ": cleanup is not PASS")
    require(len(expected) == 19 and len(report.get("tests", [])) == 19, "exactly 19 test executables required")
    final = {Path(item["path"]).name: item for item in expected}
    require(len(final) == 19, "duplicate final test filename")
    actual = {item["name"]: item for item in report["tests"]}
    require(len(actual) == 19 and set(actual) == set(final), "missing/duplicate/unexpected executed program")
    uploads = {item["name"]: item["sha256"] for item in report["uploadedArtifacts"]}
    programs = []
    for name, item in sorted(final.items()):
        tested = actual[name]
        require(tested.get("exitCode") == 0, name + ": actual exit is not zero")
        require(tested.get("binarySha256") == item["sha256"] == uploads.get(name), name + ": executed/uploaded/final SHA mismatch")
        values = counts(tested["output"])
        require(len(values) == 1, name + ": one complete Rust result required")
        programs.append(dict(name=name, path=item["path"], bytes=item["bytes"], sha256=item["sha256"], passed=values[0], report=report_name))
    return programs


def native(report, kind, expected_sha, validator):
    require(report.get("status") == "PASS" and report.get("cleanup") == "PASS", kind + ": report/cleanup failure")
    require(report.get("exeSha256") == expected_sha, kind + ": wrong executable SHA")
    require(report.get("win7") == "NOT_VALIDATED", "supplemental runs must not claim actual Win7 evidence")
    platform = report["platform"]
    require("Windows 11" in platform["caption"] and int(platform["build"]) >= 22000, "actual Windows 11 platform required")
    require(platform["version"].endswith("." + platform["build"]), "platform build/version disagree")
    require(report["selfCheck"]["exitCode"] == 0 and report["native"]["exitCode"] == 0, kind + ": native/self-check exit failed")
    uploaded = [v for v in report["uploadedArtifacts"] if v["name"] == "fc-bootstrap.exe"]
    require(len(uploaded) == 1 and uploaded[0]["sha256"] == expected_sha, kind + ": uploaded EXE mismatch")
    result = report["native"]
    require(result.get(kind + "ExitCode") == 0, kind + ": diagnostic exit failed")
    lines = output_lines(result[kind + "Output"])
    if kind == "migration":
        parsed = validator(result["migrationExitCode"], lines)
        require(report.get("migrationNativeStatus") == "PASS" and report.get("migrationNativeSteps") == parsed["steps"] == 16, "migration step summary mismatch")
        require(report.get("migrationNativeStepNames") == parsed["stepNames"] and report.get("migrationSubprocessNonce") == parsed["subprocessNonce"], "migration independent-child evidence mismatch")
        require(report.get("migrationRealRunAudit") == "PASS" and report.get("migrationRunBefore") == report.get("migrationRunAfter"), "real Run read-only audit mismatch")
    elif kind == "application":
        require(report.get("profileNativeStatus") == report.get("profileContentNativeStatus") == "PASS", "application profile status missing")
        require(any("profiles_public_joint_cas_subject_exact_native_canvas=PASS" in line for line in lines), "application layout marker missing")
        require(any("profile_content_text_images_clear_undo_real_preview=PASS" in line for line in lines), "application content marker missing")
        for prefix, field, total in [("PROFILE_CONTENT_UI_STEP index=", "profileContentNativeSteps", 52), ("SCHEDULE_OPERATIONS_PROBE step=", "scheduleOperationsNativeSteps", 65)]:
            indices = [int(line[len(prefix):].split()[0]) for line in lines if line.startswith(prefix)]
            require(indices == list(range(total)) and report.get(field) == total, "application exact ordered steps missing")
    elif kind in ("recovery", "autostart"):
        total = 23 if kind == "recovery" else 25
        prefix = kind.upper() + "_NATIVE_STEP index="
        indices = [int(line[len(prefix):].split()[0]) for line in lines if line.startswith(prefix)]
        require(indices == list(range(total)) and report.get(kind + "NativeSteps") == total and report.get(kind + "NativeStatus") == "PASS", kind + ": exact ordered steps missing")
        marker = "reopen=fresh_App_AND_independent_executable_production_Open" if kind == "recovery" else "autostart_intent_readback_confirm_cancel_lock_stale_drift_reconcile_native_worker=PASS autostart_native_steps=25 real_run=UNTOUCHED foreground=UNTOUCHED isolated_cleanup=PASS"
        require(any(marker in line for line in lines), kind + ": completion marker missing")
    elif kind == "reinitialization":
        children = [line.split("stage=", 1)[1].split()[0] for line in lines if line.startswith("REINITIALIZATION_CHILD_PASS stage=")]
        require(children == ["cancel", "commit", "reopen"] and report.get("reinitializationNativeStatus") == "PASS", "reset independent-child evidence incomplete")
        require(sum(line.startswith("reinitialization_native=PASS child_processes=3 ") for line in lines) == 1, "reset completion marker missing/repeated")
    return platform


def verify(evidence, repo):
    source = load(evidence / "source-inputs-final.json")
    require(source["fileCount"] == len(source["files"]) == 398, "final input count differs from 398")
    encoded = json.dumps(source["files"], sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()
    require(hashlib.sha256(encoded).hexdigest() == source["inputSha256"] == SOURCE_SHA, "final source snapshot SHA mismatch")
    require(len({v["path"] for v in source["files"]}) == 398, "duplicate source input")
    for item in source["files"]:
        require(sha(repo / item["path"]) == item["sha256"], "current source changed: " + item["path"])

    host = load(evidence / "host-summary-stage2.json")
    require(host["status"] == "PASS" and host["actualExitCode"] == 0 and host["passed"] == 706, "Host actual summary not 706/PASS/exit0")
    log = evidence / host["rawLog"]
    require(sha(log) == host["rawLogSha256"], "Host original log hash changed")
    lines = log.read_text().splitlines()
    require(sum(counts(lines)) == 706 and len(counts(lines)) == host["groupsIncludingDocTests"] == 31, "Host raw test totals disagree")
    executed = {match.group(1) for line in lines if (match := re.search(r"Running .+ \((/[^)]+)\)", line))}
    require(executed == {v["path"] for v in host["executables"]} and len(executed) == 19, "Host actual executed files disagree")
    for item in host["executables"]:
        require(sha(item["path"]) == item["sha256"] and Path(item["path"]).stat().st_size == item["bytes"], "Host current artifact hash changed")

    files = load(evidence / "windows-test-files-stage2.json")
    require(files["status"] == "PASS", "final Windows artifact list not PASS")
    for item in files["files"]:
        require(sha(item["path"]) == item["sha256"] and Path(item["path"]).stat().st_size == item["bytes"], "Windows current test SHA/bytes changed")
    full = load(evidence / FULL_REPORT)
    programs = accept_tests(files["files"], full, FULL_REPORT)
    require(sum(v["passed"] for v in programs) == 1037, "Windows exact completed total differs from 1037")
    validator = runpy.run_path(str(repo / "scripts/windows-probe.py"))["validate_migration_output"]
    reports = []
    platform = None
    for target, exe_sha in [("modern", MODERN_SHA), ("supplemental", SUPPLEMENTAL_SHA)]:
        for kind in ["migration", "application", "recovery", "autostart", "reinitialization"]:
            name = FULL_REPORT if target == "modern" and kind == "migration" else f"modern-{kind}-stage1.json" if target == "modern" else "win7-migration-on-win11-stage1.json" if kind == "migration" else f"win7-on-win11-{kind}-stage1.json"
            data = full if name == FULL_REPORT else load(evidence / name)
            actual = native(data, kind, exe_sha, validator)
            require(platform is None or actual == platform, "actual runtime platforms differ")
            platform = actual
            reports.append(dict(report=name, reportSha256=sha(evidence / name), kind=kind, target=target, status="PASS", cleanup="PASS", exeSha256=exe_sha, actualPlatform=actual))

    artifacts = {}
    for key, directory, target, expected_sha, size in [("modern", "fc-round25-windows", "i686-pc-windows-gnu", MODERN_SHA, 11735676), ("supplemental", "fc-round25-win7", "i686-win7-windows-gnu", SUPPLEMENTAL_SHA, 11809792)]:
        exe = Path("/tmp") / directory / target / "release/fc-bootstrap.exe"
        require(sha(exe) == expected_sha and exe.stat().st_size == size, key + ": final EXE differs")
        pe_name = "modern-pe-stage3.json" if key == "modern" else "win7-pe-stage3.json"
        pe = load(evidence / pe_name)
        require(pe["status"] == "NOT_VALIDATED" and pe["format"] == "PE32-x86" and pe["subsystem"] == "Windows CUI" and pe["structural_errors"] == [], "PE structural/formal status mismatch")
        artifacts[key] = dict(exe=str(exe), target=target, bytes=size, sha256=expected_sha, peReport=pe_name, peRiskCount=len(pe["risks"]), actualRuntimeOS=platform, formalStatus="NOT_VALIDATED")

    package = load(evidence / "package-verification-final.json")
    require(package["status"] == "PASS" and package["validationStatus"] == "NOT_VALIDATED", "ZIP report/formal status invalid")
    for key in ["crc", "allManifestFileBytesAndSha256"]:
        require(package[key] == "PASS", "ZIP verification incomplete: " + key)
    require(package["independentSameBasenameRepackIdentical"] == "PASS", "ZIP independent same-basename repack is not byte-identical")
    archive = Path(package["path"])
    require(sha(archive) == package["sha256"] == ZIP_SHA and archive.stat().st_size == package["bytes"] == 5616716, "ZIP exact SHA/bytes mismatch")
    require(package["sourceInputSha256"] == SOURCE_SHA and package["exeSha256"] == MODERN_SHA, "ZIP source/EXE binding mismatch")
    manifest = load(evidence / "package-manifest-final.json")
    require(manifest["status"] == "NOT_VALIDATED" and manifest["sha256"] == MODERN_SHA and all(v in ("NOT_VALIDATED", "NOT_ALLOWED") for v in manifest["gates"].values()), "ZIP manifest claims formal completion")
    with zipfile.ZipFile(archive) as bundle:
        require(bundle.testzip() is None and len(bundle.namelist()) == package["memberCount"] == 13, "ZIP CRC/count failed")
        require(json.loads(bundle.read("manifest.json")) == manifest, "archived manifest differs from recorded manifest")
        require(set(bundle.namelist()) == {v["path"] for v in manifest["files"]} | {"manifest.json"}, "ZIP has unexpected/missing members")
        for item in manifest["files"]:
            data = bundle.read(item["path"])
            require(len(data) == item["size"] and hashlib.sha256(data).hexdigest() == item["sha256"], "ZIP member differs: " + item["path"])
    acceptance = dict(status="PASS", scope="All 19 complete actual executions accepted only at matching final/uploaded/executed SHA; whole remote report must also PASS.", sourceInputSha256=SOURCE_SHA, programCount=19, passed=1037, programs=programs, sourceReport=FULL_REPORT, formalStatus="NOT_VALIDATED")
    summary = dict(status="PASS", sourceInputSha256=SOURCE_SHA, fileCount=398, hostPassed=706, hostSummary="host-summary-stage2.json", hostRunSourceSnapshot=host["sourceSnapshot"], windowsPassed=1037, windowsPrograms=19, windowsArtifactsSnapshot=files["sourceSnapshot"], artifacts=artifacts, runtimeReports=reports, package=package, formalStatus="NOT_VALIDATED", goalStatus="active", notRun=NOT_RUN, scope="Final current source files and exact artifact bytes verified. Prior Host/source snapshot names are retained; no unsupported fresh single-source rebuild claim or actual Win7/Win10 claim is made.")
    return acceptance, summary


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--evidence", type=Path, default=Path("/tmp/fc-round25-evidence"))
    parser.add_argument("--repo", type=Path, default=Path.cwd())
    args = parser.parse_args()
    code = 0
    try:
        acceptance, summary = verify(args.evidence, args.repo)
    except Exception as error:
        code = 1
        acceptance = summary = dict(status="FAIL", error=str(error), formalStatus="NOT_VALIDATED", goalStatus="active", evaluatedAt=datetime.now(timezone.utc).isoformat())
    for name, value in [("windows-test-acceptance-final.json", acceptance), ("verification-summary-final.json", summary)]:
        (args.evidence / name).write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(dict(status=summary["status"], error=summary.get("error"), hostPassed=summary.get("hostPassed"), windowsPassed=summary.get("windowsPassed")), ensure_ascii=False))
    return code


if __name__ == "__main__":
    sys.exit(main())
