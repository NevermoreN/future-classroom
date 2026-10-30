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
        preamble = "$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';[Console]::OutputEncoding=[System.Text.UTF8Encoding]::new($false);$OutputEncoding=[Console]::OutputEncoding;"
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
    parser.add_argument("--test-timeout-seconds", type=int, default=60, help="per supplied test EXE budget in seconds (1..900; default 60)")
    parser.add_argument("--native-seconds", type=int, default=5)
    parser.add_argument("--system-host", action="store_true", help="also run new read-only system and interactive hidden-host probes")
    parser.add_argument("--audio-silent", action="store_true", help="zero PCM only; no volume/microphone/foreground changes")
    parser.add_argument("--materials", action="store_true", help="run synthetic file/validation/SQLite/private Shell receiver integration")
    parser.add_argument("--autostart", action="store_true", help="isolated hidden native durable startup settings; generated scratch Registry leaf only")
    parser.add_argument("--calendar", action="store_true", help="isolated hidden native calendar candidate review/adoption/policy/CAS; synthetic feed")
    parser.add_argument("--calendar-download", action="store_true", help="one fixed2026 WinHTTP HTTPS feed request; no application writes")
    parser.add_argument("--application", action="store_true", help="exercise the actual workbench in an isolated hidden test deployment")
    parser.add_argument("--web-voice", action="store_true", help="interactive hidden-task authenticated voice upload/jobs and native zero-PCM three-repeat/cancel integration")
    parser.add_argument("--web-pairing", action="store_true", help="noninteractive CNG/DPAPI/SQLite pairing and synthetic trusted-TLS metadata port probe; no HTTPS listener/browser")
    parser.add_argument("--report", type=Path, required=True)
    args = parser.parse_args()
    if not 1 <= args.native_seconds <= 30:
        parser.error("native seconds must be in 1..30")
    if not 1 <= args.test_timeout_seconds <= 900:
        parser.error("test timeout seconds must be in 1..900")
    if args.autostart and (args.application or args.calendar):
        parser.error("run the dedicated autostart workflow separately to preserve independent budgets")
    if args.calendar and args.application:
        parser.error("run the dedicated calendar and full application workflows separately to preserve their independent observation budgets")
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
    # Freeze input identities before uploading. A local rebuild during a remote
    # test must never rewrite the identity of the already-uploaded executable.
    input_digests = {
        path: hashlib.sha256(path.read_bytes()).hexdigest()
        for path in dict.fromkeys([args.exe, *tests])
    }
    remote = Remote(args.host, args.identity)
    token = uuid.uuid4().hex
    task = "FutureClassroom-RustProbe-" + token
    directory = remote.powershell("$root=Join-Path $env:TEMP " + ps_literal(task) + ";New-Item -ItemType Directory -Path $root -ErrorAction Stop | Out-Null;Set-Content -LiteralPath (Join-Path $root '.future-classroom-probe-root') -Value " + ps_literal(token) + ";$root")
    root = ps_literal(directory)
    # Each child process must also place scratch files beneath our cleanup marker.
    child_temp = "$env:TEMP=Join-Path " + root + " 'temp';New-Item -ItemType Directory -Force -Path $env:TEMP|Out-Null;$env:TMP=$env:TEMP;"
    task_name = ps_literal(task)
    report = {"schemaVersion": 1, "testScope": "Remote Windows x86 supplemental development verification", "win7": "NOT_VALIDATED", "cleanup": "NOT_RUN", "testTimeoutSeconds": args.test_timeout_seconds}
    try:
        report["platform"] = json.loads(remote.powershell("$os=Get-CimInstance Win32_OperatingSystem;@{caption=$os.Caption;version=$os.Version;build=$os.BuildNumber;osArchitecture=$os.OSArchitecture}|ConvertTo-Json -Compress"))
        report["uploadedArtifacts"] = []
        for path in dict.fromkeys([args.exe, *tests]):
            remote.upload(path, directory)
            actual = remote.powershell("(Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path " + root + " " + ps_literal(path.name) + ")).Hash.ToLowerInvariant()")
            if actual != input_digests[path]:
                raise RuntimeError("uploaded executable digest mismatch: " + path.name)
            report["uploadedArtifacts"].append({"name": path.name, "sha256": actual})
        report["exeSha256"] = input_digests[args.exe]
        script = child_temp + "$root=" + root + ";$exe=Join-Path $root " + ps_literal(args.exe.name) + ";$output=@(& $exe --self-check 2>&1 | ForEach-Object {\"$_\"});$code=$LASTEXITCODE;@{exitCode=$code;output=$output}|ConvertTo-Json -Compress"
        report["selfCheck"] = json.loads(remote.powershell(script, timeout=180))
        if report["selfCheck"]["exitCode"] != 0:
            raise RuntimeError("Windows self-check failed")
        if args.calendar_download:
            script = child_temp + "$exe=Join-Path " + root + " " + ps_literal(args.exe.name) + ";$ErrorActionPreference='Continue';$output=@(& $exe --calendar-download-probe 2>&1|ForEach-Object{\"$_\"});$code=$LASTEXITCODE;$ErrorActionPreference='Stop';@{exitCode=$code;output=$output}|ConvertTo-Json -Compress"
            report["calendarDownload"] = json.loads(remote.powershell(script, timeout=90))
            result = report["calendarDownload"]
            if result.get("exitCode") != 0 or not any(isinstance(line, str) and line.startswith("CALENDAR_DOWNLOAD_PROBE_PASS ") for line in result.get("output", [])):
                raise RuntimeError("fixed source WinHTTP calendar download failed")
            report["calendarDownload"]["scope"] = "one fixed2026 outbound HTTPS download+strict parse; government notice body and native hard preemption NOT_PROVEN; no application writes"
            print("calendar HTTPS download PASS", flush=True)
        if args.web_pairing:
            # This probe uses only our marked child TEMP and runs directly over
            # noninteractive SSH; it does not need the user's foreground session.
            script = child_temp + "$exe=Join-Path " + root + " " + ps_literal(args.exe.name) + ";$ErrorActionPreference='Continue';$output=@(& $exe --web-pairing-probe 2>&1|ForEach-Object{\"$_\"});$code=$LASTEXITCODE;$ErrorActionPreference='Stop';@{exitCode=$code;output=$output}|ConvertTo-Json -Compress"
            report["webPairing"] = json.loads(remote.powershell(script, timeout=300))
            report["webPairing"]["scope"] = "CNG/DPAPI/SQLite and synthetic trusted-TLS request metadata through the application port; no real HTTPS/TLS/browser/listening delivery"
            if report["webPairing"].get("exitCode") != 0:
                raise RuntimeError("isolated noninteractive Web pairing probe failed")
            output = report["webPairing"].get("output", [])
            if not isinstance(output, list) or not any(isinstance(line, str) and (line == "WEB_PAIRING_PASS" or line.startswith("WEB_PAIRING_PASS ")) for line in output):
                raise RuntimeError("Web pairing result marker missing")
            report["webPairing"]["status"] = "PASS"
            print("Web pairing port probe PASS; real HTTPS/TLS/browser/listening delivery remains unvalidated", flush=True)
        if args.system_host:
            script = child_temp + "$exe=Join-Path " + root + " " + ps_literal(args.exe.name) + ";$output=@(& $exe --system-probe 2>&1|ForEach-Object{\"$_\"});@{exitCode=$LASTEXITCODE;output=$output}|ConvertTo-Json -Compress"
            report["system"] = json.loads(remote.powershell(script))
            if report["system"]["exitCode"] != 0:
                raise RuntimeError("read-only system probe failed")
        report["tests"] = []
        for test_index, path in enumerate(tests):
            test_arguments = " --test-threads=2" + (" --nocapture" if path.name.startswith("fc_windows-") else "")
            log = ps_literal(directory + "\\test-" + str(test_index) + ".log")
            script = child_temp + "$exe=Join-Path " + root + " " + ps_literal(path.name) + ";$log=" + log + ";$digest=(Get-FileHash -Algorithm SHA256 -LiteralPath $exe).Hash.ToLowerInvariant();if($digest-ne " + ps_literal(input_digests[path]) + "){throw 'test executable digest mismatch'};$ErrorActionPreference='Continue';$output=@(& $exe" + test_arguments + " 2>&1 | ForEach-Object {\"$_\"} | Tee-Object -FilePath $log);$code=$LASTEXITCODE;$ErrorActionPreference='Stop';@{name=" + ps_literal(path.name) + ";binarySha256=$digest;exitCode=$code;output=$output}|ConvertTo-Json -Compress"
            try:
                result = json.loads(remote.powershell(script, timeout=args.test_timeout_seconds))
            except subprocess.TimeoutExpired:
                # PowerShell's Tee-Object log is UTF-16LE. Read only the tail
                # through a bounded shared FileStream while our child may live;
                # the unchanged outer finally then terminates owned processes.
                timed_out = {"name": path.name, "budgetSeconds": args.test_timeout_seconds, "partialOutput": "", "partialOutputTruncated": False, "partialOutputReadLimitBytes": 64 * 1024}
                report["timedOutTest"] = timed_out
                read_log = "$log=" + log + ";if(!(Test-Path -LiteralPath $log)){@{output='';truncated=$false;missing=$true}|ConvertTo-Json -Compress}else{$stream=[IO.File]::Open($log,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::ReadWrite);try{$length=$stream.Length;$count=[int][Math]::Min([long]65536,$length);$null=$stream.Seek(-$count,[IO.SeekOrigin]::End);$bytes=New-Object byte[] $count;$read=0;while($read-lt $count){$n=$stream.Read($bytes,$read,$count-$read);if($n-eq 0){break};$read+=$n};$text=if($read-gt 0){[Text.Encoding]::Unicode.GetString($bytes,0,$read)}else{''};@{output=$text;truncated=($length-gt 65536);missing=$false}|ConvertTo-Json -Compress}finally{$stream.Dispose()}}"
                try:
                    partial = json.loads(remote.powershell(read_log, timeout=15))
                    text = partial.get("output", "")
                    if not isinstance(text, str):
                        raise ValueError("invalid partial output")
                    encoded = text.encode("utf-8", "replace")
                    timed_out["partialOutput"] = encoded[-64 * 1024:].decode("utf-8", "ignore")
                    timed_out["partialOutputTruncated"] = bool(partial.get("truncated")) or len(encoded) > 64 * 1024
                    timed_out["partialOutputLogMissing"] = bool(partial.get("missing"))
                except Exception as read_error:
                    # TimeoutExpired.__str__ includes the entire encoded SSH
                    # command. Keep diagnostics useful without copying it.
                    timed_out["partialOutputReadError"] = type(read_error).__name__
                raise RuntimeError(f"remote test timed out: {path.name} ({args.test_timeout_seconds}s); inspect timedOutTest") from None
            if result.get("binarySha256") != input_digests[path]:
                raise RuntimeError("executed test digest mismatch: " + path.name)
            result["timeoutSeconds"] = args.test_timeout_seconds
            report["tests"].append(result)
            print(path.name, "PASS" if result["exitCode"] == 0 else "FAIL", flush=True)
            if result["exitCode"] != 0:
                raise RuntimeError("remote test failed: " + path.name)
        # The interactive task only runs our own probe, with a hidden console.
        # It does not send keys, minimize windows, change focus, capture the desktop,
        # install services, touch production ProgramData, or alter SSH authorization.
        body = child_temp + "$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';try{$ErrorActionPreference='Continue';$out=@(& " + ps_literal(directory + "\\" + args.exe.name) + " --native-probe " + str(args.native_seconds) + " 2>&1 | ForEach-Object {\"$_\"});$code=$LASTEXITCODE;@{exitCode=$code;output=$out;sessionId=(Get-Process -Id $PID).SessionId}|ConvertTo-Json -Compress|Set-Content -Encoding UTF8 -LiteralPath " + ps_literal(directory + "\\native-result.json") + "}catch{@{exitCode=1;error=$_.Exception.Message}|ConvertTo-Json -Compress|Set-Content -Encoding UTF8 -LiteralPath " + ps_literal(directory + "\\native-result.json") + "}"
        if args.system_host:
            host_command = "$hostOutput=@(& " + ps_literal(directory + "\\" + args.exe.name) + " --host-probe 2 2>&1|ForEach-Object{\"$_\"});$hostCode=$LASTEXITCODE;"
            body = body.replace("$code=$LASTEXITCODE;@{exitCode=", "$code=$LASTEXITCODE;" + host_command + "@{hostExitCode=$hostCode;hostOutput=$hostOutput;exitCode=")
        if args.application:
            app_command = "$appOutput=@(& " + ps_literal(directory + "\\" + args.exe.name) + " --application-probe 2>&1|ForEach-Object{\"$_\"});$appCode=$LASTEXITCODE;"
            body = body.replace("@{exitCode=$code;", app_command + "@{applicationExitCode=$appCode;applicationOutput=$appOutput;exitCode=$code;") if not args.system_host else body.replace("@{hostExitCode=$hostCode;", app_command + "@{applicationExitCode=$appCode;applicationOutput=$appOutput;hostExitCode=$hostCode;")
        if args.materials:
            material_command = "$materialsOutput=@(& " + ps_literal(directory + "\\" + args.exe.name) + " --materials-probe 2>&1|ForEach-Object{\"$_\"});$materialsCode=$LASTEXITCODE;"
            anchor = "@{applicationExitCode=$appCode;" if args.application else ("@{hostExitCode=$hostCode;" if args.system_host else "@{exitCode=$code;")
            if body.count(anchor) != 1:
                raise RuntimeError("materials report insertion point is ambiguous")
            body = body.replace(anchor, material_command + "@{materialsExitCode=$materialsCode;materialsOutput=$materialsOutput;" + anchor[2:])

        if args.audio_silent:
            command = "$audioOutput=@(& " + ps_literal(directory + "\\" + args.exe.name) + " --audio-silent-probe 2>&1|ForEach-Object{\"$_\"});$audioCode=$LASTEXITCODE;"
            anchor = "@{materialsExitCode=$materialsCode;" if args.materials else ("@{applicationExitCode=$appCode;" if args.application else ("@{hostExitCode=$hostCode;" if args.system_host else "@{exitCode=$code;"))
            if body.count(anchor) != 1:
                raise RuntimeError("audio report insertion point is ambiguous")
            body = body.replace(anchor, command + "@{audioExitCode=$audioCode;audioOutput=$audioOutput;" + anchor[2:])

        if args.web_voice:
            command = "$webVoiceOutput=@(& " + ps_literal(directory + "\\" + args.exe.name) + " --web-voice-probe 2>&1|ForEach-Object{\"$_\"});$webVoiceCode=$LASTEXITCODE;"
            anchor = "@{audioExitCode=$audioCode;" if args.audio_silent else ("@{materialsExitCode=$materialsCode;" if args.materials else ("@{applicationExitCode=$appCode;" if args.application else ("@{hostExitCode=$hostCode;" if args.system_host else "@{exitCode=$code;")))
            if body.count(anchor) != 1:
                raise RuntimeError("voice report insertion point is ambiguous")
            body = body.replace(anchor, command + "@{webVoiceExitCode=$webVoiceCode;webVoiceOutput=$webVoiceOutput;" + anchor[2:])

        if args.calendar:
            command = "$calendarOutput=@(& " + ps_literal(directory + "\\" + args.exe.name) + " --calendar-probe 2>&1|ForEach-Object{\"$_\"});$calendarCode=$LASTEXITCODE;"
            report_anchors = ["@{webVoiceExitCode=$webVoiceCode;", "@{audioExitCode=$audioCode;", "@{materialsExitCode=$materialsCode;", "@{applicationExitCode=$appCode;", "@{hostExitCode=$hostCode;", "@{exitCode=$code;"]
            matched = [anchor for anchor in report_anchors if anchor in body]
            if len(matched) != 1 or body.count(matched[0]) != 1:
                raise RuntimeError("calendar report insertion point is ambiguous")
            anchor = matched[0]
            body = body.replace(anchor, command + "@{calendarExitCode=$calendarCode;calendarOutput=$calendarOutput;" + anchor[2:])

        if args.autostart:
            command = "$autostartOutput=@(& " + ps_literal(directory + "\\" + args.exe.name) + " --autostart-probe 2>&1|ForEach-Object{\"$_\"});$autostartCode=$LASTEXITCODE;"
            report_anchors = ["@{webVoiceExitCode=$webVoiceCode;", "@{audioExitCode=$audioCode;", "@{materialsExitCode=$materialsCode;", "@{applicationExitCode=$appCode;", "@{hostExitCode=$hostCode;", "@{exitCode=$code;"]
            matched = [anchor for anchor in report_anchors if anchor in body]
            if len(matched) != 1 or body.count(matched[0]) != 1:
                raise RuntimeError("autostart report insertion point is ambiguous")
            anchor = matched[0]
            body = body.replace(anchor, command + "@{autostartExitCode=$autostartCode;autostartOutput=$autostartOutput;" + anchor[2:])

        body = "[Console]::OutputEncoding=[System.Text.UTF8Encoding]::new($false);$OutputEncoding=[Console]::OutputEncoding;" + body
        # Upload a script rather than nesting UTF-16 base64 in Task Scheduler
        # arguments: OpenSSH's default cmd.exe command line is limited to 8191 chars.
        with tempfile.TemporaryDirectory(prefix="future-classroom-probe-script-") as local:
            native_script = Path(local) / "native-probe.ps1"
            native_script.write_text(body, encoding="utf-8-sig")
            remote.upload(native_script, directory)
        arguments = '-NoProfile -NonInteractive -WindowStyle Hidden -ExecutionPolicy Bypass -File "' + directory + '\\native-probe.ps1"'
        launch = "$action=New-ScheduledTaskAction -Execute 'powershell.exe' -Argument " + ps_literal(arguments) + ";$principal=New-ScheduledTaskPrincipal -UserId ([System.Security.Principal.WindowsIdentity]::GetCurrent().Name) -LogonType Interactive -RunLevel Limited;Register-ScheduledTask -TaskName " + task_name + " -Action $action -Principal $principal|Out-Null;Start-ScheduledTask -TaskName " + task_name + ";$until=(Get-Date).AddSeconds(40);$result=Join-Path " + root + " 'native-result.json';while(!(Test-Path -LiteralPath $result)){if((Get-Date)-gt $until){throw 'interactive probe timed out'};Start-Sleep -Milliseconds 250};Get-Content -Raw -LiteralPath $result"
        if args.application or args.materials or args.web_voice or args.calendar or args.autostart:
            launch = launch.replace("AddSeconds(40)", "AddSeconds(240)")
        report["native"] = json.loads(remote.powershell(launch, timeout=250 if args.application or args.materials or args.web_voice or args.calendar or args.autostart else 50).lstrip("\ufeff"))
        if report["native"]["exitCode"] != 0:
            raise RuntimeError("native probe failed")
        if args.system_host and report["native"].get("hostExitCode") != 0:
            raise RuntimeError("interactive hidden-host/tray probe failed")
        if args.application and report["native"].get("applicationExitCode") != 0:
            raise RuntimeError("isolated native workbench application failed")
        if args.application:
            application = report["native"].get("applicationOutput", [])
            marker = "profiles_public_joint_cas_subject_exact_native_canvas=PASS"
            if not isinstance(application, list) or not any(isinstance(line, str) and marker in line for line in application):
                raise RuntimeError("native public-layout/subject-profile/canvas result marker missing")
            report["profileNativeStatus"] = "PASS"
            content_marker = "profile_content_text_images_clear_undo_real_preview=PASS"
            if not any(content_marker in line for line in application if isinstance(line, str)):
                raise RuntimeError("native independent-content workflow result marker missing")
            content_steps = [
                int(line.split("index=", 1)[1].split()[0])
                for line in application
                if isinstance(line, str) and line.startswith("PROFILE_CONTENT_UI_STEP index=")
            ]
            if content_steps != list(range(52)):
                raise RuntimeError("native independent-content workflow steps incomplete or repeated")
            report["profileContentNativeStatus"] = "PASS"
            report["profileContentNativeSteps"] = len(content_steps)
            if not any(isinstance(line, str) and "schedule_roles_activity_split_merge_confirm_native_worker=PASS schedule_operations_native_steps=65" in line for line in application):
                raise RuntimeError("schedule operations result marker missing")
            schedule_steps = [
                int(line.split("step=", 1)[1].split(" ", 1)[0])
                for line in application
                if isinstance(line, str) and line.startswith("SCHEDULE_OPERATIONS_PROBE step=")
            ]
            if schedule_steps != list(range(65)):
                raise RuntimeError("schedule operations did not complete exactly 65 ordered steps")
            report["scheduleOperationsNativeSteps"] = len(schedule_steps)
        if args.autostart:
            output = report["native"].get("autostartOutput", [])
            if report["native"].get("autostartExitCode") != 0 or not isinstance(output, list):
                raise RuntimeError("isolated native autostart workflow failed")
            steps = [int(line.split("index=", 1)[1].split()[0]) for line in output if isinstance(line, str) and line.startswith("AUTOSTART_NATIVE_STEP index=")]
            marker = "autostart_intent_readback_confirm_cancel_lock_stale_drift_reconcile_native_worker=PASS autostart_native_steps=25 real_run=UNTOUCHED foreground=UNTOUCHED isolated_cleanup=PASS"
            if steps != list(range(25)) or not any(isinstance(line, str) and marker in line for line in output):
                raise RuntimeError("native autostart result or exact25 ordered steps missing")
            report["autostartNativeSteps"] = len(steps)
            report["autostartNativeStatus"] = "PASS"
        if args.calendar:
            output = report["native"].get("calendarOutput", [])
            if report["native"].get("calendarExitCode") != 0 or not isinstance(output, list):
                raise RuntimeError("isolated native calendar workflow failed")
            steps = [int(line.split("index=", 1)[1].split()[0]) for line in output if isinstance(line, str) and line.startswith("CALENDAR_NATIVE_STEP index=")]
            if steps != list(range(30)) or not any(isinstance(line, str) and "calendar_candidates_review_adopt_policy_lock_stale_native_worker=PASS calendar_native_steps=30" in line for line in output):
                raise RuntimeError("native calendar result or exact30 ordered steps missing")
            report["calendarNativeSteps"] = len(steps)
            report["calendarNativeStatus"] = "PASS"
        if args.materials and report["native"].get("materialsExitCode") != 0:
            raise RuntimeError("isolated materials integration failed")
        if args.audio_silent:
            audio = report["native"].get("audioOutput", [])
            if report["native"].get("audioExitCode") != 0:
                raise RuntimeError("isolated silent WinMM integration failed")
            report["audioStatus"] = "NOT_RUN" if any("AUDIO_SILENT_NOT_RUN" in line or "VOICE_PIPELINE_NOT_RUN" in line for line in audio) else "PASS"
            if report["audioStatus"] == "PASS" and not (any("AUDIO_SILENT_PASS" in line for line in audio) and any("VOICE_PIPELINE_PASS" in line for line in audio) and any("VOICE_RUNTIME_PASS" in line for line in audio)):
                raise RuntimeError("silent audio result missing")
        if args.web_voice:
            voice_output = report["native"].get("webVoiceOutput", [])
            if report["native"].get("webVoiceExitCode") != 0:
                raise RuntimeError("isolated authenticated native zero-PCM Web voice integration failed")
            for marker in ["WEB_VOICE_PASS", "WEB_MATERIALS_PASS", "WEB_CONTENT_PASS", "WEB_PAIRING_PASS"]:
                if not any(isinstance(line, str) and line.startswith(marker + " ") for line in voice_output):
                    raise RuntimeError("Web voice diagnostic result marker missing: " + marker)
            report["webVoiceStatus"] = "PASS"
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
