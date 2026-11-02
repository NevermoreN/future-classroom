#!/usr/bin/env python3
"""Explicit finite ordinary teacher DOM journey on the authorized Windows host.

Default is inert. Uses disposable TLS/SQLite and an exclusive hidden Edge Job.
Pairing remains actual UI pairing; private rendezvous bytes stay in memory.
"""
import argparse
import base64
import importlib.util
import json
from pathlib import Path
import queue
import re
import subprocess
import sys
import tempfile
import time
import uuid

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent


def load(name):
    spec = importlib.util.spec_from_file_location(name.replace("-", "_"), HERE / (name + ".py"))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


b = load("windows-tls-business-probe")
tls = b._tls
teacher = load("windows-teacher-browser-observer")
require = b.require
CSP = "default-src 'none'; script-src 'self'; worker-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self' blob: data:; media-src blob:; base-uri 'none'; object-src 'none'; frame-ancestors 'none'; form-action 'self'"
FLAG = "--teacher-browser-probe"
OLD_ARGS = "@('--tls-lifecycle-probe',$thumbprint,$marker)"
NEW_ARGS = "@('--teacher-browser-probe',$thumbprint,[string]$teacherPort,('\"'+(Join-Path $root 'web-old')+'\"'),('\"'+(Join-Path $root 'web-new')+'\"'),$marker)"
require(tls.LAUNCH.count(OLD_ARGS) == 1 and tls.STOP.count("--tls-lifecycle-probe") == 1,
        "ORIGINAL_LAUNCH_STOP_TRANSFORM")
require(tls.LAUNCH.count("90000") == 1, "ORIGINAL_OBSERVATION_BOUND")
LAUNCH = "$teacherPort=__PORT__\n" + tls.LAUNCH.replace(OLD_ARGS, NEW_ARGS).replace(
    "90000", "120000").replace("90-second", "120-second")
STOP = tls.STOP.replace("--tls-lifecycle-probe", FLAG)
# ACK 0 and pairing leaves are private and removed before evidence capture.
PUBLIC_NAMES = tuple(f"teacher-action-{i}.request" for i in range(5)) + tuple(
    f"teacher-action-{i}.ack" for i in range(1, 5)) + tuple(
    f"teacher-action-{i}.request.pending" for i in range(5)) + ("g03-teacher-material.txt",)
OLD_RAW_LOOP = "'tls.stdout','tls.stderr','.tls-process.json','.tls-supervisor.json','.tls-setup-process.json','.tls-fixture.json','tls-public.pem'"
require(tls.CAPTURE.count(OLD_RAW_LOOP) == 1, "ORIGINAL_CAPTURE_TRANSFORM")
CAPTURE = tls.CAPTURE.replace(OLD_RAW_LOOP, OLD_RAW_LOOP + "," + ",".join(
    b.ps_literal(name) for name in PUBLIC_NAMES))
tls.RAW_NAMES = (*tls.RAW_NAMES, *PUBLIC_NAMES)


def remaining(deadline):
    value = deadline - time.monotonic()
    require(value > 0, "ORIGINAL_NATIVE_DEADLINE_EXPIRED")
    return value


def root_check(directory, marker):
    return ("$r=" + b.ps_literal(directory) + ";$m=" + b.ps_literal(marker)
            + ";if((Split-Path -Leaf $r)-ne $m-or [IO.File]::ReadAllText((Join-Path $r '.tls-probe-root'))-ne $m)"
              "{throw 'owned teacher root mismatch'};")


def private_read(remote, directory, marker, leaf, limit, deadline, remove=False):
    """No helper logging, raw artifact, hash, or exception echo of private data."""
    require(leaf in {"teacher-pairing-1.secret", "teacher-action-0.ack"}, "PRIVATE_LEAF")
    script = (root_check(directory, marker) + "$p=Join-Path $r " + b.ps_literal(leaf)
              + ";if(!(Test-Path -LiteralPath $p)){exit 3};$f=Get-Item -LiteralPath $p -Force;"
              + "if($f.Length-gt " + str(limit)
              + "-or ($f.Attributes-band [IO.FileAttributes]::ReparsePoint)){throw 'private leaf bound'};"
                "$v=[IO.File]::ReadAllBytes($p);"
              + ("Remove-Item -LiteralPath $p;" if remove else "")
              + "[Console]::Write([Convert]::ToBase64String($v))")
    try:
        child = subprocess.run(remote.command(script), capture_output=True,
                               timeout=min(8, remaining(deadline)))
        if child.returncode == 3:
            return None
        require(child.returncode == 0 and len(child.stdout) <= 4 * limit, "PRIVATE_READ_EXIT_BOUND")
        value = base64.b64decode(child.stdout.strip(), validate=True)
        require(len(value) <= limit, "PRIVATE_READ_BYTE_BOUND")
        return value
    except (OSError, subprocess.TimeoutExpired, ValueError):
        raise RuntimeError("PRIVATE_READ_FAILED") from None


def stage_package(remote, local, directory, marker, package, leaf):
    require(leaf in ("web-old", "web-new"), "FIXED_PACKAGE_LEAF")
    local_leaf = Path(local) / leaf
    local_leaf.mkdir()
    result = b.upload_web_package(remote, local_leaf, directory, marker, package)
    outcome = remote.powershell(root_check(directory, marker)
        + "$p=Join-Path $r 'web-package';$to=Join-Path $r " + b.ps_literal(leaf)
        + ";if(Test-Path -LiteralPath $to){throw 'fresh package destination'};"
          "Move-Item -LiteralPath $p -Destination $to;"
          "if(!(Test-Path -LiteralPath $to)-or (Test-Path -LiteralPath $p)){throw 'package rename'};'PASS'")
    require(outcome == "PASS", "VERIFIED_PACKAGE_RENAME")
    return result


class Native:
    def __init__(self, process, logs, marker, port, deadline):
        self.process, self.logs, self.marker, self.port, self.deadline = process, logs, marker, port, deadline
        self.ready = self.pair_ready = self.passed = False
        self.approvals, self.exits, self.actions = [], [], []

    def line(self, timeout=.1):
        require(not self.logs.overflow.is_set(), "NATIVE_LOG_OVERFLOW")
        try:
            line = self.logs.lines.get(timeout=min(timeout, remaining(self.deadline)))
        except queue.Empty:
            return
        if line.startswith("TEACHER_BROWSER_READY "):
            require(not self.ready and line == f"TEACHER_BROWSER_READY origin=https://127.0.0.1:{self.port} marker={self.marker}", "NATIVE_READY")
            self.ready = True
        elif line.startswith("TEACHER_BROWSER_PAIR_READY "):
            cycle = 2 if self.pair_ready else 1
            require(line == f"TEACHER_BROWSER_PAIR_READY cycle={cycle} marker={self.marker}", "NATIVE_PAIR_READY")
            self.pair_ready = True
        elif line.startswith("TEACHER_BROWSER_APPROVED "):
            match = re.fullmatch(r"TEACHER_BROWSER_APPROVED cycle=([12]) marker=" + re.escape(self.marker)
                                 + r" request=([1-9][0-9]*) teacherId=([1-9][0-9]*) role=TEACHER_ADMIN", line)
            require(match is not None and int(match[1]) == len(self.approvals) + 1, "ACTUAL_APPROVAL_ORDER")
            self.approvals.append({"cycle": int(match[1]), "requestId": match[2], "principalId": match[3]})
        elif line.startswith("TEACHER_BROWSER_NATIVE_PASS "):
            require(not self.passed and re.fullmatch(
                r"TEACHER_BROWSER_NATIVE_PASS pairing=ACTUAL_DOM_REQUIRED business=SEPARATE_DOM_REPORT clock=ISOLATED_CIVIL_ONLY audio=ZERO_WAV_CAPTURE_PHYSICAL_OWNER_SEPARATE osClock=UNTOUCHED windows7=NOT_RUN windows10=NOT_RUN elapsed_ms=[0-9]+", line), "NATIVE_FINITE_PASS")
            self.passed = True
        elif line.startswith(tls.EXIT):
            self.exits.append(json.loads(line[len(tls.EXIT):]))

    def wait_ready(self):
        while not (self.ready and self.pair_ready):
            self.line()
            require(self.ready and self.pair_ready or self.process.poll() is None
                    or any(t.is_alive() for t in self.logs.threads) or not self.logs.lines.empty(),
                    "NATIVE_EXITED_BEFORE_READY")

    def finish(self, dom):
        while self.process.poll() is None or any(t.is_alive() for t in self.logs.threads) or not self.logs.lines.empty():
            if self.process.poll() is not None and not any(t.is_alive() for t in self.logs.threads) and self.logs.lines.empty():
                break
            self.line()
        self.process.wait(timeout=remaining(self.deadline))
        self.logs.join()
        require(self.process.returncode == 0 and self.exits == [{"code": 0}] and self.passed,
                "ACTUAL_NATIVE_AND_LAUNCHER_SUCCESSFUL_EXIT")
        expected = [{k: row[k] for k in ("cycle", "requestId", "principalId")} for row in dom["pairingCycles"]]
        require(self.approvals == expected and self.actions == list(teacher.ACTIONS), "DOM_NATIVE_APPROVAL_AND_PHASE_BINDING")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--execute", action="store_true")
    parser.add_argument("--ssh-target", default="future-classroom-win11")
    parser.add_argument("--identity", type=Path)
    parser.add_argument("--exe", type=Path)
    parser.add_argument("--old-web-bundle", type=Path)
    parser.add_argument("--new-web-bundle", type=Path)
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    if not args.execute:
        print(json.dumps({"status": "NOT_RUN", "mode": "DEFAULT_INERT"}))
        return
    if not all((args.exe, args.old_web_bundle, args.new_web_bundle, args.report)) or args.report.exists():
        parser.error("explicit executable, two genuine normal web bundles and fresh report required")
    pending = args.report.with_name(args.report.name + ".pending.json")
    require(not pending.exists(), "FRESH_RECOVERY_CHECKPOINT_REQUIRED")
    require(re.fullmatch(r"[A-Za-z0-9_.@:-]+", args.ssh_target) and not args.ssh_target.startswith("-"), "SSH_TARGET")
    require(args.exe.is_file() and not args.exe.is_symlink() and args.exe.stat().st_size <= 128 * 1024 * 1024, "BOUNDED_EXECUTABLE")
    executable = args.exe.read_bytes()
    offset = int.from_bytes(executable[60:64], "little") if len(executable) >= 64 else 0
    require(executable[:2] == b"MZ" and executable[offset:offset+6] == b"PE\0\0\x4c\x01", "ACTUAL_X86_PE")
    packages = [b.read_web_package(p) for p in (args.old_web_bundle, args.new_web_bundle)]
    manifests = [json.loads(p[0]) for p in packages]
    require(manifests[0] != manifests[1], "DISTINCT_NORMAL_BUNDLES")
    token = uuid.uuid4().hex
    marker = "FutureClassroom-Test-TLS-" + token
    remote = b.Remote(args.ssh_target, args.identity)
    report = {"schemaVersion": 1, "status": "FAIL", "cleanup": "NOT_RUN", "marker": marker,
              "scope": "FINITE_REAL_TEACHER_DOM_SYNTHETIC_DATA_ONLY", "windows7": "NOT_RUN", "windows10": "NOT_RUN",
              "exeSha256": b.digest(executable), "runnerSha256": b.digest(Path(__file__).read_bytes())}
    directory = launcher = logs = observer = native = None
    uploaded = set()
    local = tempfile.TemporaryDirectory(prefix="fc-teacher-browser-")

    def checkpoint(phase):
        # Recover only this exact nonce after an interrupted local controller.
        # Never persist a pairing secret, ACK payload, or browser credentials.
        args.report.parent.mkdir(parents=True, exist_ok=True)
        value = {"status": "IN_PROGRESS", "phase": phase, "marker": marker,
                 "remoteRoot": directory, "executableName": args.exe.name,
                 "exeSha256": report["exeSha256"], "uploadedControls": sorted(uploaded)}
        with pending.open("w", encoding="utf-8") as output:
            json.dump(value, output, indent=2)
            output.write("\n")

    try:
        directory = remote.powershell("$r=Join-Path $env:TEMP " + b.ps_literal(marker)
            + ";New-Item -ItemType Directory -Path $r|Out-Null;$sid=[Security.Principal.WindowsIdentity]::GetCurrent().User;"
              "$acl=[Security.AccessControl.DirectorySecurity]::new();$acl.SetOwner($sid);$acl.SetAccessRuleProtection($true,$false);"
              "$rule=[Security.AccessControl.FileSystemAccessRule]::new($sid,[Security.AccessControl.FileSystemRights]::FullControl,"
              "([Security.AccessControl.InheritanceFlags]::ContainerInherit-bor [Security.AccessControl.InheritanceFlags]::ObjectInherit),"
              "[Security.AccessControl.PropagationFlags]::None,[Security.AccessControl.AccessControlType]::Allow);"
              "$acl.AddAccessRule($rule);Set-Acl -LiteralPath $r -AclObject $acl;"
              "if(!(Get-Acl -LiteralPath $r).AreAccessRulesProtected){throw 'root ACL'};"
              "[IO.File]::WriteAllText((Join-Path $r '.tls-probe-root')," + b.ps_literal(marker) + ");$r")
        report["remoteRoot"] = directory
        checkpoint("OWNED_ROOT_CREATED")
        report["packages"] = [stage_package(remote, local.name, directory, marker, package, leaf)
                              for package, leaf in zip(packages, ("web-old", "web-new"))]
        for name, template in (("cleanup", tls.CLEANUP), ("stop", STOP), ("capture", CAPTURE), ("setup", tls.SETUP)):
            source = Path(local.name) / ("tls-" + name + ".ps1")
            source.write_text(tls.COMMON + tls.script(template, directory, marker, args.exe.name), encoding="utf-8-sig")
            remote.upload(source, directory)
            uploaded.add(name)
            checkpoint("CONTROLS_STAGED")
        fixture = json.loads(remote.run_file(directory + "\\tls-setup.ps1", timeout=45))
        report["fixture"] = {k: v for k, v in fixture.items() if k != "publicPem"}
        port = int(remote.powershell("$l=[Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback,0);try{$l.Start();$p=$l.LocalEndpoint.Port}finally{$l.Stop()};$p"))
        require(1024 <= port <= 65535, "HIGH_PORT")
        source = Path(local.name) / "tls-launch.ps1"
        source.write_text(tls.COMMON + tls.script(LAUNCH.replace("__PORT__", str(port)), directory, marker, args.exe.name, fixture["thumbprint"]), encoding="utf-8-sig")
        remote.upload(source, directory)
        require(remote.powershell("$f=Get-ChildItem -LiteralPath " + b.ps_literal(directory)
            + " -Filter 'tls-*.ps1';foreach($p in $f){$t=$null;$e=$null;[Management.Automation.Language.Parser]::ParseFile($p.FullName,[ref]$t,[ref]$e)|Out-Null;if($e.Count){throw 'AST'}};'PASS'") == "PASS", "ACTUAL_PS51_AST")
        report["controlSources"] = {p.name: {"bytes": p.stat().st_size, "sha256": b.digest(p.read_bytes()), "source": p.read_text(encoding="utf-8-sig")} for p in Path(local.name).glob("tls-*.ps1")}
        material = Path(local.name) / "g03-teacher-material.txt"
        material.write_bytes(b"Synthetic future classroom teacher material.\n")
        remote.upload(material, directory)
        remote.upload(args.exe, directory)
        require(remote.powershell("(Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path " + b.ps_literal(directory) + " " + b.ps_literal(args.exe.name) + ")).Hash.ToLowerInvariant()") == report["exeSha256"] == b.digest(args.exe.read_bytes()), "UPLOADED_EXECUTABLE_IDENTITY")
        # This conservative deadline includes SSH startup; it can never renew
        # the native parent's independent 120-second deadline.
        deadline = time.monotonic() + 120
        launcher = subprocess.Popen(remote.command_file(directory + "\\tls-launch.ps1"), stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        logs = tls.Logs(launcher)
        native = Native(launcher, logs, marker, port, deadline)
        checkpoint("ORIGINAL_NATIVE_LAUNCHED")
        native.wait_ready()
        observer = teacher.make_observer(HERE, remote, Path(local.name), args.report, marker, manifests[0], manifests[1], CSP, args.identity, args.ssh_target)
        observer.prepare(fixture["publicPem"], deadline)
        raw = private_read(remote, directory, marker, "teacher-pairing-1.secret", 256, deadline, remove=True)
        code = teacher.decode_pairing(raw, marker, 1)

        def phase(action, original_deadline):
            require(original_deadline == deadline, "NO_DEADLINE_RENEWAL")
            index = len(native.actions)
            raw_request = teacher.action_request(marker, index, action)
            encoded = base64.b64encode(raw_request).decode("ascii")
            require(remote.powershell(root_check(directory, marker)
                + "$p=Join-Path $r " + b.ps_literal(f"teacher-action-{index}.request")
                + ";$staged=$p+'.pending';if(Test-Path -LiteralPath $p){throw 'request already published'};"
                  "$f=[IO.File]::Open($staged,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::Read);"
                  "try{$v=[Convert]::FromBase64String('" + encoded + "');$f.Write($v,0,$v.Length);$f.Flush($true)}finally{$f.Dispose()};"
                  "[IO.File]::Move($staged,$p);'PASS'",
                timeout=min(8, remaining(deadline))) == "PASS", "ACTUAL_FRESH_NATIVE_REQUEST")
            while True:
                native.line()
                if index == 0:
                    ack = private_read(remote, directory, marker, "teacher-action-0.ack", 1024, deadline)
                else:
                    result = remote.powershell(root_check(directory, marker)
                        + "$p=Join-Path $r " + b.ps_literal(f"teacher-action-{index}.ack")
                        + ";if(Test-Path -LiteralPath $p){$f=Get-Item -LiteralPath $p -Force;if($f.Length-gt 1024-or ($f.Attributes-band [IO.FileAttributes]::ReparsePoint)){throw 'ack bound'};[Console]::Write([Convert]::ToBase64String([IO.File]::ReadAllBytes($p)))}", timeout=min(8, remaining(deadline)))
                    ack = base64.b64decode(result, validate=True) if result else None
                # The original writer creates, writes and flushes the fixed
                # leaf. A poll may observe its empty or partial first write;
                # never consume/delete that still-incomplete rendezvous.
                if ack is not None and ack.endswith(b"\n"):
                    value = teacher.decode_ack(ack, marker, index)
                    if index == 0:
                        require(remote.powershell(root_check(directory, marker)
                            + "$p=Join-Path $r 'teacher-action-0.ack';Remove-Item -LiteralPath $p;"
                              "if(Test-Path -LiteralPath $p){throw 'private ack remains'};'PASS'",
                            timeout=min(8, remaining(deadline))) == "PASS", "PRIVATE_ACK_CONSUMED_AFTER_COMPLETE_READ")
                    native.actions.append(action)
                    return value
                require(launcher.poll() is None or any(t.is_alive() for t in logs.threads)
                        or not logs.lines.empty(), "NATIVE_EXITED_WITHOUT_ORIGINAL_ACK")

        dom = observer.run_teacher({"origin": f"https://127.0.0.1:{port}", "pairingCode": code,
            "loginName": "synthetic-ordinary-teacher", "materialPath": directory + "\\" + material.name,
            "materialName": material.name, "materialBytes": material.stat().st_size,
            "materialSha256": b.digest(material.read_bytes())}, phase, deadline)
        del code, raw
        native.finish(dom)
        report.update(status="PASS", nativeApprovals=native.approvals, nativeActions=native.actions,
                      nativeExit=native.exits[0], dom=dom)
    except Exception as error:
        report["error"] = tls.failure(error, "teacher-browser-composition")
    finally:
        if observer is not None:
            try:
                report["browser"] = observer.close()
                require(report["browser"]["cleanup"] == "PASS"
                        and report["browser"].get("ownerPhysicalCleanup") == "PASS",
                        "ORIGINAL_EDGE_PHYSICAL_EXIT")
                if report["status"] == "PASS":
                    require(report["browser"]["status"] == "PASS"
                            and report["browser"].get("materialBrowserFileVerifiedAfterPhysicalExit") is True,
                            "SUCCESSFUL_DOM_AND_ACTUAL_DOWNLOAD_REQUIRED")
            except Exception as error:
                report.update(status="FAIL", browserCleanupError=tls.failure(error, "original-edge-owner-close"))
        if launcher is not None:
            try:
                if launcher.poll() is None:
                    report["earlyOwnedStop"] = json.loads(remote.run_file(directory + "\\tls-stop.ps1", timeout=25))
                launcher.wait(timeout=8)
                logs.join()
                report["sshLauncherRaw"] = logs.report()
            except Exception as error:
                report.update(status="FAIL", launcherCleanupError=tls.failure(error, "original-native-launcher-exit"))
        if directory:
            try:
                require(remote.powershell(root_check(directory, marker)
                    + "foreach($n in @('teacher-pairing-1.secret','teacher-pairing-2.secret','teacher-action-0.ack')){"
                      "$p=Join-Path $r $n;if(Test-Path -LiteralPath $p){Remove-Item -LiteralPath $p};"
                      "if(Test-Path -LiteralPath $p){throw 'private leaf remains'}};'PASS'", timeout=8) == "PASS", "PRIVATE_LEAVES_REMOVED")
                report["privateRendezvousCleanup"] = "PASS"
            except Exception:
                report.update(status="FAIL", privateRendezvousCleanup="FAIL")
            b.cleanup_owned(remote, directory, marker, uploaded, args.report, token, report)
        report["remoteControlRunsRaw"] = remote.script_runs
        local.cleanup()
        args.report.parent.mkdir(parents=True, exist_ok=True)
        with args.report.open("x", encoding="utf-8") as output:
            json.dump(report, output, ensure_ascii=False, indent=2)
            output.write("\n")
        if pending.exists():
            require(json.loads(pending.read_text())["marker"] == marker, "RECOVERY_CHECKPOINT_OWNER")
            pending.unlink()
    require(report["status"] == report["cleanup"] == "PASS", "TEACHER_BROWSER_FAILED_RETAIN_REPORT")
    print("TEACHER_BROWSER_RUNNER_PASS " + str(args.report))


if __name__ == "__main__":
    main()
