#!/usr/bin/env python3
"""Owned finite invalid-static-package startup diagnostic; execution is explicit.

No certificate, trust store, database, firewall, HTTP.sys or SSH setup is done.
The expected loader error proves its distinct source error path, not a kernel
trace of Native.open or a strict TCP five-second/Windows compatibility result.
"""
import argparse
import ast
import base64
import hashlib
import importlib.util
import json
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import uuid

sys.dont_write_bytecode = True
EXPECTED_STAGE = "business verified static package before native open"
FAKE_THUMBPRINT = "F" * 40
RAW_NAMES = ("tls.stdout", "tls.stderr", ".tls-process.json", ".tls-supervisor.json")

GUARD = r'''
function Assert-OwnedRoot([string]$Root,[string]$Marker){
  Assert ($Marker-match '^FutureClassroom-Test-TLS-[0-9a-f]{32}$') 'negative marker shape mismatch'
  $expected=[IO.Path]::GetFullPath((Join-Path $env:TEMP $Marker))
  Assert ([String]::Equals([IO.Path]::GetFullPath($Root),$expected,[StringComparison]::OrdinalIgnoreCase)) 'negative root outside original TEMP identity'
  Assert ((Split-Path -Leaf $Root)-eq $Marker) 'negative root basename mismatch'
  Assert (([IO.File]::ReadAllText((Join-Path $Root '.tls-probe-root')))-ceq $Marker) 'negative root ownership marker mismatch'
  Assert (((Get-Item -LiteralPath $Root -Force).Attributes-band [IO.FileAttributes]::ReparsePoint)-eq 0) 'negative root is a reparse point'
}
'''
INSPECT = r'''
$root=__ROOT__;$marker=__MARKER__
Assert-OwnedRoot $root $marker
$manifest=Join-Path $root 'web-package\fc-assets.json'
$file=Get-Item -LiteralPath $manifest
Assert (($file.Attributes-band [IO.FileAttributes]::ReparsePoint)-eq 0) 'negative manifest became reparse point'
@{businessDirectoryAbsent=!(Test-Path -LiteralPath (Join-Path $root 'FutureClassroom-Test-Business'));manifestBytes=$file.Length;manifestSha256=(Get-FileHash -Algorithm SHA256 -LiteralPath $manifest).Hash.ToLowerInvariant()}|ConvertTo-Json -Compress
'''
CLEANUP = r'''
$root=__ROOT__;$marker=__MARKER__
Assert-OwnedRoot $root $marker
Assert (@(Get-CimInstance Win32_Process|Where-Object{$_.ExecutablePath-and $_.ExecutablePath-eq (Join-Path $root __EXENAME__)}).Count-eq 0) 'owned executable remains before root deletion'
Assert (!(Test-Path -LiteralPath (Join-Path $root 'FutureClassroom-Test-Business'))) 'unexpected Backend deployment; preserve original root for review'
foreach($item in @(Get-ChildItem -LiteralPath $root -Recurse -Force)){
  Assert (($item.Attributes-band [IO.FileAttributes]::ReparsePoint)-eq 0) 'refuse deletion through a descendant reparse point'
}
Remove-Item -LiteralPath $root -Recurse -Force
Assert (!(Test-Path -LiteralPath $root)) 'owned negative root remains'
@{status='PASS';rootAbsent=$true;ownedProcessesRemaining=0;businessDirectoryAbsent=$true;certificatesCreated=0;sshConfiguration='UNTOUCHED';rootTrustStore='UNTOUCHED';httpSys='UNTOUCHED';firewall='UNTOUCHED'}|ConvertTo-Json -Compress
'''


def digest(data):
    return hashlib.sha256(data).hexdigest()


def require(value, message):
    if not value:
        raise RuntimeError(message)


def templates(tls):
    require(tls.LAUNCH.count("--tls-lifecycle-probe") == 1 and tls.STOP.count("--tls-lifecycle-probe") == 1,
            "known TLS ownership template flag count changed")
    launch = tls.LAUNCH.replace("--tls-lifecycle-probe", "--tls-business-probe")
    require(launch.count("90000") == 1 and launch.count("90-second") == 1, "known supervisor timeout template changed")
    launch = launch.replace("90000", "30000").replace("90-second", "30-second")
    anchor = "$env:TEMP=Join-Path $root 'temp';$env:TMP=$env:TEMP"
    require(launch.count(anchor) == 1, "known TEMP ownership anchor changed")
    launch = launch.replace(anchor, "Assert-OwnedRoot $root $marker\n" + anchor)
    stop = tls.STOP.replace("--tls-lifecycle-probe", "--tls-business-probe")
    require(stop.count("$killed=0") == 1, "known stop ownership anchor changed")
    stop = stop.replace("$killed=0", "Assert-OwnedRoot $root $marker\n$killed=0")
    # The original journal uses exact PID/startTicks/path. For its narrow
    # unjournaled Start-Process gap also compare CIM's microsecond creation time
    # to the retained Process handle before killing; never sweep a process name.
    anchor = "Assert ($ownedProcess.Path-eq (Join-Path $root __EXENAME__)) 'unjournaled diagnostic path changed'"
    require(stop.count(anchor) == 1, "known recovered-process identity anchor changed")
    stop = stop.replace(anchor, anchor + "\n  Assert ($ownedProcess.StartTime.ToUniversalTime().ToString('yyyyMMddHHmmss.ffffff')-eq $candidate.CreationDate.ToUniversalTime().ToString('yyyyMMddHHmmss.ffffff')) 'unjournaled diagnostic creation time changed; refusing reused PID'")
    return {"cleanup": CLEANUP, "stop": stop, "capture": tls.CAPTURE, "inspect": INSPECT, "launch": launch}


def raw_values(report, require_native=True):
    preservation = report.get("rawPreservation", {})
    require(preservation.get("status") == "PASS", "complete original raw artifacts were not preserved")
    records = preservation.get("files", {})
    if require_native:
        require(set(RAW_NAMES) <= records.keys(), "native stdout/stderr/process/supervisor records incomplete")
    values = {}
    for name, record in records.items():
        require(name in RAW_NAMES, "unexpected raw file in certificate-free startup negative")
        require(record.get("status") == "PASS" and "base64" in record, "raw file exceeded bounded full-inline verification: " + name)
        data = base64.b64decode(record["base64"], validate=True)
        require({"bytes": len(data), "sha256": digest(data)} == record["downloaded"], "raw original base64/byte/SHA mismatch")
        require(Path(record["artifact"]).read_bytes() == data, "preserved raw artifact differs from verified original base64")
        values[name] = data
    return values


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--exe", type=Path)
    parser.add_argument("--report", type=Path)
    parser.add_argument("--ssh-target", default="future-classroom-win11")
    parser.add_argument("--identity", type=Path)
    parser.add_argument("--project-root", type=Path, default=Path.cwd())
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    helper = args.project_root / "scripts/windows-tls-probe.py"
    require(helper.is_file(), "run from project root or supply --project-root")
    spec = importlib.util.spec_from_file_location("fc_owned_tls_startup_negative", helper)
    tls = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(tls)
    scripts = templates(tls)
    if args.self_test:
        require(not args.exe and not args.report and not args.identity, "pure self-test takes no remote inputs")
        ast.parse(Path(__file__).read_text())
        for name in ("launch", "stop"):
            require("--tls-business-probe" in scripts[name] and "--tls-lifecycle-probe" not in scripts[name], "business process ownership flag missing")
        require("30000" in scripts["launch"] and "90000" not in scripts["launch"], "30s supervisor cap missing")
        require("New-SelfSignedCertificate" not in "".join(scripts.values()), "negative controls unexpectedly create certificate")
        print(json.dumps({"status": "PURE_STATIC_CHECKS_PASS", "scripts": list(scripts), "windows": "NOT_RUN", "processBudgetSeconds": 30}))
        return
    if not args.exe or not args.report or not args.exe.is_file() or args.exe.suffix.lower() != ".exe":
        parser.error("--exe frozen.exe and --report fresh.json required")
    if args.report.exists() or (args.identity and not args.identity.is_file()):
        parser.error("report must be new and optional private identity must exist")
    if not re.fullmatch(r"[A-Za-z0-9_.@:-]+", args.ssh_target) or args.ssh_target.startswith("-"):
        parser.error("invalid SSH alias/user@host")
    require(args.exe.stat().st_size <= 128 * 1024 * 1024, "EXE byte bound exceeded")
    executable = args.exe.read_bytes()
    pe = int.from_bytes(executable[60:64], "little") if len(executable) >= 64 else 0
    require(executable[:2] == b"MZ" and executable[pe:pe + 6] == b"PE\0\0\x4c\x01", "real i686 Windows PE required")
    token = uuid.uuid4().hex
    marker = "FutureClassroom-Test-TLS-" + token
    report = {"schemaVersion": 1, "status": "FAIL", "cleanup": "NOT_RUN", "marker": marker,
              "scope": "Distinct invalid-static-package startup rejection before native open/Backend setup by known source ordering; not a kernel trace",
              "strictP0": "NOT_VALIDATED", "kernelTrace": "NOT_CAPTURED", "windowsCompatibility": "NOT_CERTIFIED",
              "exeSha256": digest(executable), "runnerSourceSha256": digest(Path(__file__).read_bytes()),
              "ownershipHelperSourceSha256": digest(helper.read_bytes()), "processBudgetSeconds": 30,
              "stopGraceSeconds": 5, "fakeThumbprint": FAKE_THUMBPRINT,
              "invalidManifest": {"bytes": 2, "sha256": digest(b"{}")},
              "certificatesCreated": 0, "privateKeyExported": False, "uploadedScriptSha256": {}}
    remote = tls.Remote(args.ssh_target, args.identity)
    directory = None
    local = None
    uploaded = set()
    phase = "create-owned-root"
    launch_output = None
    launch_attempted = False
    try:
        directory = remote.powershell("$marker=" + tls.ps_literal(marker)
            + ";$root=Join-Path $env:TEMP $marker;New-Item -ItemType Directory -Path $root|Out-Null;"
            + "[IO.File]::WriteAllText((Join-Path $root '.tls-probe-root'),$marker);$root")
        report["remoteRoot"] = directory
        local = tempfile.TemporaryDirectory(prefix="fc-static-startup-negative-")
        phase = "upload-owned-controls"
        for name, template in scripts.items():
            source = Path(local.name) / ("tls-" + name + ".ps1")
            source.write_text(tls.COMMON + GUARD + tls.script(template, directory, marker, args.exe.name, FAKE_THUMBPRINT), encoding="utf-8-sig")
            report["uploadedScriptSha256"][name] = digest(source.read_bytes())
            remote.upload(source, directory)
            uploaded.add(name)
        phase = "remote-control-ast-parse"
        result = remote.powershell("foreach($file in Get-ChildItem -LiteralPath " + tls.ps_literal(directory)
            + " -Filter 'tls-*.ps1'){$tokens=$null;$errors=$null;[Management.Automation.Language.Parser]::ParseFile($file.FullName,[ref]$tokens,[ref]$errors)|Out-Null;"
            + "if($errors.Count){throw (($errors|ForEach-Object{$_.Message})-join '; ')}};'STATIC_NEGATIVE_CONTROL_PARSE_PASS'", timeout=15)
        require(result == "STATIC_NEGATIVE_CONTROL_PARSE_PASS", "control AST parse result missing")
        report["controlScriptsParse"] = "PASS"
        phase = "write-unique-bad-manifest-and-verify-exe"
        remote.upload(args.exe, directory)
        identity = json.loads(remote.powershell("$root=" + tls.ps_literal(directory)
            + ";$package=Join-Path $root 'web-package';New-Item -ItemType Directory -Path $package|Out-Null;"
            + "[IO.File]::WriteAllText((Join-Path $package 'fc-assets.json'),'{}',[Text.UTF8Encoding]::new($false));"
            + "@{exe=(Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $root " + tls.ps_literal(args.exe.name)
            + ")).Hash.ToLowerInvariant();manifest=(Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $package 'fc-assets.json')).Hash.ToLowerInvariant();"
            + "businessDirectoryAbsent=!(Test-Path -LiteralPath (Join-Path $root 'FutureClassroom-Test-Business'))}|ConvertTo-Json -Compress"))
        require(identity["exe"] == report["exeSha256"] == digest(args.exe.read_bytes())
                and identity["manifest"] == digest(b"{}") and identity["businessDirectoryAbsent"] is True,
                "frozen EXE/bad-manifest/pre-setup identity mismatch")
        report["uploadIdentity"] = identity
        phase = "run-owned-30s-startup-negative"
        launch_attempted = True
        launch_output = remote.run_file(directory + "\\tls-launch.ps1", timeout=45)
        exits = [json.loads(line[len(tls.EXIT):]) for line in launch_output.splitlines() if line.startswith(tls.EXIT)]
        require(len(exits) == 1 and type(exits[0].get("code")) is int and exits[0]["code"] == 1, "actual native EXIT1 missing")
        report["nativeProcessExit"] = exits[0]
    except Exception as error:
        report["error"] = tls.failure(error, phase)
    finally:
        stopped = directory is None
        originals = None
        inspection = None
        if directory:
            try:
                if "stop" in uploaded:
                    report["ownedProcessCleanup"] = json.loads(remote.run_file(directory + "\\tls-stop.ps1", timeout=25))
                    stopped = report["ownedProcessCleanup"].get("status") == "PASS"
                else:
                    stopped = "launch" not in uploaded
                require(stopped, "original owned processes not physically stopped")
            except Exception as error:
                report["processCleanupError"] = tls.failure(error, "stop-original-owned-processes")
            try:
                require(stopped, "raw files may still change; process stop unproven")
                if "capture" in uploaded:
                    metadata = json.loads(remote.run_file(directory + "\\tls-capture.ps1", timeout=30))
                    report["remoteRawEvidence"] = metadata
                    tls.preserve_raw(remote, directory, args.report, token, metadata, report)
                    originals = raw_values(report, require_native=launch_attempted)
                    report["rawBase64ByteShaVerification"] = "PASS"
                elif not launch_attempted:
                    originals = {}  # Before any launch was possible.
            except Exception as error:
                report["rawCaptureError"] = tls.failure(error, "preserve-full-original-raw-before-delete")
            try:
                if "inspect" in uploaded:
                    inspection = json.loads(remote.run_file(directory + "\\tls-inspect.ps1", timeout=15))
                    report["inspection"] = inspection
                if "error" not in report:
                    require(originals is not None, "complete original stdout/stderr/process journals missing")
                    stdout, stderr = originals["tls.stdout"].decode("utf-8", "strict"), originals["tls.stderr"].decode("utf-8", "strict")
                    require("READY" not in stdout, "native readiness was published despite invalid package")
                    require(EXPECTED_STAGE in stderr, "distinct verified-static-package loader failure stage missing")
                    require(inspection is not None and inspection.get("businessDirectoryAbsent") is True
                            and inspection.get("manifestBytes") == 2 and inspection.get("manifestSha256") == digest(b"{}"),
                            "unexpected Backend setup directory or changed invalid manifest")
                    journal = json.loads(originals[".tls-process.json"].decode("utf-8-sig"))
                    require(journal.get("marker") == marker and type(journal.get("pid")) is int
                            and journal.get("exe") == directory + "\\" + args.exe.name and isinstance(journal.get("startTicks"), str)
                            and journal["startTicks"].isdigit(), "original exact process journal identity malformed")
                    report["assertions"] = {"actualNativeExit": 1, "completeStdoutNoReady": True,
                        "distinctLoaderStageInCompleteStderr": True, "businessDirectoryAbsent": True,
                        "proofScope": "Source-ordered loader rejection path before unique Native.open/Setup; no kernel trace/timing claim"}
                    report["status"] = "PASS"
            except Exception as error:
                report["verificationError"] = tls.failure(error, "verify-distinct-startup-negative-path")
                report["status"] = "FAIL"
            try:
                require(stopped and originals is not None, "process stop/full original preservation unproven; retain root")
                if "cleanup" in uploaded:
                    report["cleanupDetails"] = json.loads(remote.run_file(directory + "\\tls-cleanup.ps1", timeout=30))
                    report["cleanup"] = report["cleanupDetails"].get("status", "FAIL")
                    require(report["cleanup"] == "PASS", "owned cleanup did not return PASS")
                else:
                    raise RuntimeError("cleanup script upload incomplete; preserve owned root")
            except Exception as error:
                report.update({"status": "FAIL", "cleanup": "FAIL", "cleanupError": tls.failure(error, "cleanup-exact-owned-root"),
                    "retainedRemoteEvidence": {"root": directory, "processStopProven": stopped,
                    "completeOriginalsProven": originals is not None,
                    "nextAction": "recover/verify complete original artifacts, then use this root's exact tls-stop.ps1 and tls-cleanup.ps1; preserve SSH keys/service"}})
        if local:
            try:
                local.cleanup()
            except Exception as error:
                report["localCleanupError"] = tls.failure(error, "cleanup-owned-local-controls")
                report["status"] = "FAIL"
        report["remoteControlRunsRaw"] = remote.script_runs
        args.report.parent.mkdir(parents=True, exist_ok=True)
        with args.report.open("x", encoding="utf-8") as output:
            output.write(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
    summary = {"status": report["status"], "cleanup": report["cleanup"], "report": str(args.report),
               "exeSha256": report["exeSha256"], "strictP0": "NOT_VALIDATED"}
    print("STATIC_STARTUP_NEGATIVE " + json.dumps(summary, separators=(",", ":")))
    if report["status"] != "PASS" or report["cleanup"] != "PASS":
        raise SystemExit(1)


if __name__ == "__main__":
    main()
