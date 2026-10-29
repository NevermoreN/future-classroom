#!/usr/bin/env python3
"""Run only supplied development probes in a disposable remote Windows directory.
Credentials stay outside the repository. Existing SSH configuration is retained.
"""
import argparse
import base64
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
import uuid


def ps_literal(value):
    return "'" + str(value).replace("'", "''") + "'"


class Remote:
    def __init__(self, host, identity):
        self.host = host
        self.options = ["-i", str(identity), "-o", "IdentitiesOnly=yes", "-o", "BatchMode=yes", "-o", "ConnectTimeout=8"]

    def powershell(self, script, timeout=60):
        preamble = "$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';[Console]::OutputEncoding=[System.Text.UTF8Encoding]::new($false);"
        encoded = base64.b64encode((preamble + script).encode("utf-16-le")).decode("ascii")
        result = subprocess.run(["ssh", *self.options, self.host, "powershell.exe -NoProfile -NonInteractive -EncodedCommand " + encoded], capture_output=True, timeout=timeout)
        if result.returncode:
            raise RuntimeError(result.stderr.decode("utf-8", "replace")[-3000:])
        return result.stdout.decode("utf-8", "replace").strip()

    def upload(self, local, directory):
        subprocess.run(["scp", "-q", *self.options, str(local), self.host + ":" + directory.replace("\\", "/") + "/" + local.name], check=True, timeout=60)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", required=True)
    parser.add_argument("--identity", type=Path, required=True)
    parser.add_argument("--exe", type=Path, required=True)
    parser.add_argument("--test-metadata", type=Path)
    parser.add_argument("--native-seconds", type=int, default=5)
    parser.add_argument("--system-host", action="store_true", help="also run new read-only system and interactive hidden-host probes")
    parser.add_argument("--report", type=Path, required=True)
    args = parser.parse_args()
    if not 1 <= args.native_seconds <= 30:
        parser.error("native seconds must be in 1..30")
    if args.host.startswith("-") or "@" not in args.host or any(c.isspace() for c in args.host):
        parser.error("host must be user@hostname")
    if not args.exe.is_file() or args.exe.suffix.lower() != ".exe" or not args.identity.is_file():
        parser.error("exe and private identity files must exist")
    tests = []
    if args.test_metadata:
        for line in args.test_metadata.read_text().splitlines():
            item = json.loads(line)
            if item.get("reason") == "compiler-artifact" and item.get("profile", {}).get("test") and item.get("executable"):
                path = Path(item["executable"])
                if path.suffix.lower() != ".exe" or not path.is_file():
                    raise RuntimeError("missing Windows test executable")
                tests.append(path)
        if not tests:
            raise RuntimeError("no test executables in supplied Cargo metadata")
    tests = list(dict.fromkeys(tests))
    remote = Remote(args.host, args.identity)
    token = uuid.uuid4().hex
    task = "FutureClassroom-RustProbe-" + token
    directory = remote.powershell("$root=Join-Path $env:TEMP " + ps_literal(task) + ";New-Item -ItemType Directory -Path $root -ErrorAction Stop | Out-Null;Set-Content -LiteralPath (Join-Path $root '.future-classroom-probe-root') -Value " + ps_literal(token) + ";$root")
    root = ps_literal(directory)
    # Each child process must also place scratch files beneath our cleanup marker.
    child_temp = "$env:TEMP=Join-Path " + root + " 'temp';New-Item -ItemType Directory -Force -Path $env:TEMP|Out-Null;$env:TMP=$env:TEMP;"
    task_name = ps_literal(task)
    report = {"schemaVersion": 1, "testScope": "Remote Windows x86 supplemental development verification", "win7": "NOT_VALIDATED", "cleanup": "NOT_RUN"}
    try:
        report["platform"] = json.loads(remote.powershell("$os=Get-CimInstance Win32_OperatingSystem;@{caption=$os.Caption;version=$os.Version;build=$os.BuildNumber;osArchitecture=$os.OSArchitecture}|ConvertTo-Json -Compress"))
        for path in [args.exe, *tests]:
            remote.upload(path, directory)
        digest = hashlib.sha256(args.exe.read_bytes()).hexdigest()
        report["exeSha256"] = digest
        actual = remote.powershell("(Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path " + root + " " + ps_literal(args.exe.name) + ")).Hash.ToLowerInvariant()")
        if actual != digest:
            raise RuntimeError("remote executable digest mismatch")
        script = child_temp + "$root=" + root + ";$exe=Join-Path $root " + ps_literal(args.exe.name) + ";$output=@(& $exe --self-check 2>&1 | ForEach-Object {\"$_\"});$code=$LASTEXITCODE;@{exitCode=$code;output=$output}|ConvertTo-Json -Compress"
        report["selfCheck"] = json.loads(remote.powershell(script, timeout=180))
        if report["selfCheck"]["exitCode"] != 0:
            raise RuntimeError("Windows self-check failed")
        if args.system_host:
            script = child_temp + "$exe=Join-Path " + root + " " + ps_literal(args.exe.name) + ";$output=@(& $exe --system-probe 2>&1|ForEach-Object{\"$_\"});@{exitCode=$LASTEXITCODE;output=$output}|ConvertTo-Json -Compress"
            report["system"] = json.loads(remote.powershell(script))
            if report["system"]["exitCode"] != 0:
                raise RuntimeError("read-only system probe failed")
        report["tests"] = []
        for path in tests:
            script = child_temp + "$exe=Join-Path " + root + " " + ps_literal(path.name) + ";$output=@(& $exe --test-threads=2 2>&1 | ForEach-Object {\"$_\"});$code=$LASTEXITCODE;@{name=" + ps_literal(path.name) + ";exitCode=$code;output=$output}|ConvertTo-Json -Compress"
            result = json.loads(remote.powershell(script, timeout=60))
            result["binarySha256"] = hashlib.sha256(path.read_bytes()).hexdigest()
            report["tests"].append(result)
            print(path.name, "PASS" if result["exitCode"] == 0 else "FAIL", flush=True)
            if result["exitCode"] != 0:
                raise RuntimeError("remote test failed: " + path.name)
        # The interactive task only runs our own probe, with a hidden console.
        # It does not send keys, minimize windows, change focus, capture the desktop,
        # install services, touch production ProgramData, or alter SSH authorization.
        body = child_temp + "$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';try{$out=@(& " + ps_literal(directory + "\\" + args.exe.name) + " --native-probe " + str(args.native_seconds) + " 2>&1 | ForEach-Object {\"$_\"});$code=$LASTEXITCODE;@{exitCode=$code;output=$out;sessionId=(Get-Process -Id $PID).SessionId}|ConvertTo-Json -Compress|Set-Content -Encoding UTF8 -LiteralPath " + ps_literal(directory + "\\native-result.json") + "}catch{@{exitCode=1;error=$_.Exception.Message}|ConvertTo-Json -Compress|Set-Content -Encoding UTF8 -LiteralPath " + ps_literal(directory + "\\native-result.json") + "}"
        if args.system_host:
            host_command = "$hostOutput=@(& " + ps_literal(directory + "\\" + args.exe.name) + " --host-probe 2 2>&1|ForEach-Object{\"$_\"});$hostCode=$LASTEXITCODE;"
            body = body.replace("$code=$LASTEXITCODE;@{exitCode=", "$code=$LASTEXITCODE;" + host_command + "@{hostExitCode=$hostCode;hostOutput=$hostOutput;exitCode=")
        # Upload a script rather than nesting UTF-16 base64 in Task Scheduler
        # arguments: OpenSSH's default cmd.exe command line is limited to 8191 chars.
        with tempfile.TemporaryDirectory(prefix="future-classroom-probe-script-") as local:
            native_script = Path(local) / "native-probe.ps1"
            native_script.write_text(body, encoding="utf-8-sig")
            remote.upload(native_script, directory)
        arguments = '-NoProfile -NonInteractive -WindowStyle Hidden -ExecutionPolicy Bypass -File "' + directory + '\\native-probe.ps1"'
        launch = "$action=New-ScheduledTaskAction -Execute 'powershell.exe' -Argument " + ps_literal(arguments) + ";$principal=New-ScheduledTaskPrincipal -UserId ([System.Security.Principal.WindowsIdentity]::GetCurrent().Name) -LogonType Interactive -RunLevel Limited;Register-ScheduledTask -TaskName " + task_name + " -Action $action -Principal $principal|Out-Null;Start-ScheduledTask -TaskName " + task_name + ";$until=(Get-Date).AddSeconds(40);$result=Join-Path " + root + " 'native-result.json';while(!(Test-Path -LiteralPath $result)){if((Get-Date)-gt $until){throw 'interactive probe timed out'};Start-Sleep -Milliseconds 250};Get-Content -Raw -LiteralPath $result"
        report["native"] = json.loads(remote.powershell(launch, timeout=50).lstrip("\ufeff"))
        if report["native"]["exitCode"] != 0:
            raise RuntimeError("native probe failed")
        if args.system_host and report["native"].get("hostExitCode") != 0:
            raise RuntimeError("interactive hidden-host/tray probe failed")
        print("native probe PASS; foreground/interaction evidence remains limited to counters", flush=True)
        report["status"] = "PASS"
    except Exception as error:
        report["status"] = "FAIL"
        report["error"] = str(error)
        raise
    finally:
        cleanup = "$root=" + root + ";$task=Get-ScheduledTask -TaskName " + task_name + " -ErrorAction SilentlyContinue;if($task){Stop-ScheduledTask -TaskName " + task_name + " -ErrorAction SilentlyContinue;Unregister-ScheduledTask -TaskName " + task_name + " -Confirm:$false};if((Get-Content -Raw -LiteralPath (Join-Path $root '.future-classroom-probe-root')).Trim()-ne " + ps_literal(token) + "){throw 'cleanup marker mismatch'};Get-CimInstance Win32_Process|Where-Object{$_.ExecutablePath -and $_.ExecutablePath.StartsWith($root+'\\',[StringComparison]::OrdinalIgnoreCase)}|ForEach-Object{Stop-Process -Id $_.ProcessId -Force -ErrorAction Stop};Remove-Item -Recurse -Force -LiteralPath $root;if(Test-Path -LiteralPath $root){throw 'probe directory remains'};'CLEANUP_PASS'"
        try:
            outcome = remote.powershell(cleanup, timeout=30)
            report["cleanup"] = "PASS" if outcome == "CLEANUP_PASS" else "FAIL"
        except Exception as error:
            report["cleanup"] = "FAIL"
            report["cleanupError"] = str(error)
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
        print("cleanup:", report["cleanup"], flush=True)
        if report["cleanup"] != "PASS":
            raise RuntimeError("remote test cleanup failed; inspect report")


if __name__ == "__main__":
    main()
