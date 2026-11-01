#!/usr/bin/env python3
"""Finite hidden ordinary HTTPS composition, never a school listener deployment.

Uses one supplied EXE, two actual configured loopback generations and two strict
TLS status GETs. Existing certificate/key/process/raw-evidence ownership helpers
are reused. No browser, recording, playback, trust/firewall/HTTP.sys mutation.
MaterialReady, campus reachability and Win7/Win10 runtime remain separate tests.
"""
import argparse
import importlib.util
import json
from pathlib import Path
import queue
import re
import socket
import ssl
import subprocess
import sys
import tempfile
import time
import uuid

sys.dont_write_bytecode = True
_spec = importlib.util.spec_from_file_location("fc_ordinary_https_helpers", Path(__file__).with_name("windows-tls-business-probe.py"))
b = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(b)
tls = b._tls
STEPS = (
    "setup-local-global-administrator", "native-teacher-page-default-off", "native-open-https-settings",
    "native-save-explicit-ip-cidr-reference", "saved-config-no-listener", "native-enable-configured-runtime",
    "physical-first-running-generation", "first-real-tls-response-complete",
    "native-pairing-open-close-and-disable-review-cancel", "native-confirm-first-disable",
    "physical-first-domain-drained", "original-shared-workers-remain-operational",
    "native-enable-second-generation", "second-real-tls-response-complete",
    "native-lock-does-not-stop-established-domain", "native-relogin-confirm-second-disable",
    "physical-second-domain-drained", "native-return-to-local-workbench",
    "actual-shared-worker-writer-shutdown", "owned-deployment-cleanup",
)
_STEP = re.compile(r"ORDINARY_HTTPS_NATIVE_STEP index=(0|[1-9][0-9]*) name=([a-z-]+)")
_FOOTER = re.compile(r"ORDINARY_HTTPS_NATIVE_PASS steps=20 shared_workers=2 tls_cycles=2 physical_shutdown=PASS isolated_cleanup=PASS scope=OWNED_LOOPBACK_COMPOSITION material_ready_stop=NOT_EXERCISED campus=NOT_VALIDATED elapsed_ms=(0|[1-9][0-9]*)")
_old_launch = "@('--tls-lifecycle-probe',$thumbprint,$marker)"
if tls.LAUNCH.count(_old_launch) != 1 or tls.STOP.count("--tls-lifecycle-probe") != 1:
    raise RuntimeError("original exact owned-process launch/cleanup contract changed")
LAUNCH = "$ordinaryPort=__PORT__\n" + tls.LAUNCH.replace(_old_launch,
    "@('--ordinary-https-probe',$thumbprint,[string]$ordinaryPort,('\"'+(Join-Path $root 'web-package')+'\"'),$marker)")
STOP = tls.STOP.replace("--tls-lifecycle-probe", "--ordinary-https-probe")


def require(value, message):
    if not value:
        raise RuntimeError(message)


def validate_ready(value, marker, port, cycle, previous=None):
    require(isinstance(value, dict) and set(value) == {"cycle", "marker", "port", "generation", "origin", "target"}, "ordinary READY shape")
    require(type(value["cycle"]) is int and value["cycle"] == cycle and value["marker"] == marker
            and type(value["port"]) is int and value["port"] == port
            and type(value["generation"]) is int and value["generation"] > 0
            and value["generation"] != previous
            and value["origin"] == f"https://127.0.0.1:{port}"
            and value["target"] == "/fc/api/v1/status", "ordinary READY identity/generation/exact IP origin")


def validate_transcript(lines):
    steps, footer = [], []
    for index, line in enumerate(lines):
        if line.startswith("ORDINARY_HTTPS_NATIVE_STEP"):
            item = _STEP.fullmatch(line)
            require(item is not None, "malformed ordinary step")
            steps.append((int(item[1]), item[2], index))
        elif line.startswith("ORDINARY_HTTPS_NATIVE_PASS"):
            item = _FOOTER.fullmatch(line)
            require(item is not None and int(item[1]) <= 120000, "ordinary original120s footer")
            footer.append(index)
    require([(i, name) for i, name, _ in steps] == list(enumerate(STEPS)), "ordinary exact twenty steps")
    require(len(footer) == 1 and footer[0] > steps[-1][2], "ordinary footer after all steps")
    return {"steps": len(steps), "physicalShutdown": "PASS", "materialReadyStop": "NOT_EXERCISED"}


def validate_status(value):
    require(isinstance(value, dict) and set(value) == {"schemaVersion", "apiVersion", "uiCompatibilityVersion", "online"}
            and all(type(value[k]) is int and value[k] == 1 for k in ("schemaVersion", "apiVersion", "uiCompatibilityVersion"))
            and value["online"] is True, "ordinary exact public-status DTO")


def status_get(local_port, origin_port, context, expected_der, deadline):
    require(time.monotonic() < deadline, "original30s client deadline")
    parser = b.HttpResponse()
    with socket.create_connection(("127.0.0.1", local_port), timeout=min(5, deadline-time.monotonic())) as raw:
        with context.wrap_socket(raw, server_hostname="127.0.0.1", suppress_ragged_eofs=False) as connection:
            require(connection.getpeercert(binary_form=True) == expected_der, "actual exact owned certificate DER")
            require(connection.version() == "TLSv1.2", "actual TLS1.2")
            connection.sendall((f"GET /fc/api/v1/status HTTP/1.1\r\nHost: 127.0.0.1:{origin_port}\r\nConnection: close\r\n\r\n").encode("ascii"))
            while True:
                require(time.monotonic() < deadline, "original30s receive deadline")
                connection.settimeout(min(5, deadline-time.monotonic()))
                chunk = connection.recv(16384)
                if not chunk:
                    break  # Actual clean TLS EOF, before our own close.
                parser.feed(chunk)
    parser.finish()
    require(parser.status == 200, "ordinary configured public status HTTP200")
    value = parser.json()
    validate_status(value)
    return {"httpStatus": parser.status, "bodyBytes": parser.body_bytes,
            "bodySha256": parser.body_hash.hexdigest(), "cleanTlsEof": True,
            "tls": "TLSv1.2", "serverName": "127.0.0.1", "cookieSent": False}


def self_test():
    marker = "FutureClassroom-Test-TLS-" + "a" * 32
    ready = {"cycle": 1, "marker": marker, "port": 23456, "generation": 17,
             "origin": "https://127.0.0.1:23456", "target": "/fc/api/v1/status"}
    validate_ready(ready, marker, 23456, 1)
    bad = [dict(ready, generation=True), dict(ready, generation=0), dict(ready, cycle=2),
           dict(ready, origin="https://localhost:23456"), dict(ready, secret="forged"), dict(ready, port=True)]
    for value in bad:
        try:
            validate_ready(value, marker, 23456, 1)
        except RuntimeError:
            pass
        else:
            raise RuntimeError("invalid READY accepted")
    try:
        validate_ready(dict(ready, cycle=2), marker, 23456, 2, previous=17)
    except RuntimeError:
        pass
    else:
        raise RuntimeError("old generation reused")
    lines = [f"ORDINARY_HTTPS_NATIVE_STEP index={i} name={name}" for i, name in enumerate(STEPS)]
    footer = "ORDINARY_HTTPS_NATIVE_PASS steps=20 shared_workers=2 tls_cycles=2 physical_shutdown=PASS isolated_cleanup=PASS scope=OWNED_LOOPBACK_COMPOSITION material_ready_stop=NOT_EXERCISED campus=NOT_VALIDATED elapsed_ms=4000"
    validate_transcript(lines + [footer])
    for invalid in [lines[:-1]+[footer], [footer]+lines, lines+[footer,footer],
                    lines+[footer.replace("4000", "120001")], lines+[footer.replace("physical_shutdown=PASS", "physical_shutdown=FAIL")],
                    lines[:2]+[lines[1]]+lines[2:]+[footer]]:
        try:
            validate_transcript(invalid)
        except RuntimeError:
            pass
        else:
            raise RuntimeError("incomplete/reordered/unretired/overbudget transcript accepted")
    actual_status = {"schemaVersion": 1, "apiVersion": 1, "uiCompatibilityVersion": 1, "online": True}
    validate_status(actual_status)
    for value in [dict(actual_status, online=False), dict(actual_status, apiVersion=True), dict(actual_status, cookie="forged"), {"schemaVersion": 1, "status": "READY"}]:
        try:
            validate_status(value)
        except RuntimeError:
            pass
        else:
            raise RuntimeError("invalid public status accepted")
    print(json.dumps({"status": "PASS", "scope": "pure validators only", "checks": 20,
                      "windows": "NOT_RUN", "tls": "NOT_RUN", "ordinaryRuntime": "NOT_RUN"}))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ssh-target", default="future-classroom-win11")
    parser.add_argument("--identity", type=Path)
    parser.add_argument("--exe", type=Path)
    parser.add_argument("--web-bundle", type=Path)
    parser.add_argument("--report", type=Path)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        if any((args.identity, args.exe, args.web_bundle, args.report)) or args.ssh_target != "future-classroom-win11":
            parser.error("self-test has no remote inputs")
        self_test()
        return
    if not args.exe or not args.web_bundle or not args.report or args.report.exists():
        parser.error("fresh --report plus explicit --exe and --web-bundle required")
    if not re.fullmatch(r"[A-Za-z0-9_.@:-]+", args.ssh_target) or args.ssh_target.startswith("-"):
        parser.error("invalid SSH target")
    if args.exe.is_symlink() or not args.exe.is_file() or args.exe.stat().st_size > 128*1024*1024:
        parser.error("bounded ordinary executable required")
    executable = args.exe.read_bytes()
    offset = int.from_bytes(executable[60:64], "little") if len(executable)>=64 else 0
    if executable[:2] != b"MZ" or executable[offset:offset+6] != b"PE\0\0\x4c\x01":
        parser.error("real x86 PE executable required")
    if not args.web_bundle.is_file() or args.web_bundle.stat().st_size > 128*1024*1024:
        parser.error("bounded verified development ZIP required")
    package = b.read_web_package(args.web_bundle)
    token = uuid.uuid4().hex
    marker = "FutureClassroom-Test-TLS-" + token
    remote = b.Remote(args.ssh_target, args.identity)
    report = {"schemaVersion": 1, "status": "FAIL", "cleanup": "NOT_RUN", "marker": marker,
        "scope": "finite ordinary configured owner with synthetic SQLite and owned IP-SAN certificate",
        "campus": "NOT_VALIDATED", "win7": "NOT_RUN", "win10": "NOT_RUN", "materialReadyStop": "NOT_EXERCISED",
        "productionSchoolSettings": "UNCHANGED", "runnerSourceSha256": b.digest(Path(__file__).read_bytes()),
        "ownershipSourceSha256": b.digest(Path(__file__).with_name("windows-tls-probe.py").read_bytes()),
        "exeSha256": b.digest(executable), "events": []}
    uploaded, directory, launcher, logs, tunnel = set(), None, None, None, None
    local = tempfile.TemporaryDirectory(prefix="fc-ordinary-https-")
    try:
        directory = remote.powershell("$r=Join-Path $env:TEMP "+b.ps_literal(marker)+";New-Item -ItemType Directory -Path $r|Out-Null;[IO.File]::WriteAllText((Join-Path $r '.tls-probe-root'),"+b.ps_literal(marker)+");$r")
        report["remoteRoot"] = directory
        report["staticPackage"] = b.upload_web_package(remote, local.name, directory, marker, package)
        for name, template in (("cleanup", tls.CLEANUP), ("stop", STOP), ("capture", tls.CAPTURE), ("setup", tls.SETUP)):
            source = Path(local.name)/("tls-"+name+".ps1")
            source.write_text(tls.COMMON+tls.script(template,directory,marker,args.exe.name),encoding="utf-8-sig")
            remote.upload(source,directory);uploaded.add(name)
        fixture = json.loads(remote.run_file(directory+"\\tls-setup.ps1",timeout=45))
        report["fixture"] = {k:v for k,v in fixture.items() if k!="publicPem"}
        port = int(remote.powershell("$l=[Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback,0);try{$l.Start();$p=$l.LocalEndpoint.Port}finally{$l.Stop()};$p"))
        require(1024<=port<=65535,"owned high port")
        report["configuredPort"] = port
        source = Path(local.name)/"tls-launch.ps1"
        source.write_text(tls.COMMON+tls.script(LAUNCH.replace("__PORT__",str(port)),directory,marker,args.exe.name,fixture["thumbprint"]),encoding="utf-8-sig")
        remote.upload(source,directory)
        require(remote.powershell("$f=Get-ChildItem -LiteralPath "+b.ps_literal(directory)+" -Filter 'tls-*.ps1';foreach($p in $f){$t=$null;$e=$null;[Management.Automation.Language.Parser]::ParseFile($p.FullName,[ref]$t,[ref]$e)|Out-Null;if($e.Count){throw 'owned script AST failed'}};'PASS'")=="PASS","actual owned PowerShell AST")
        report["uploadedControlSources"] = {p.name:{"bytes":p.stat().st_size,"sha256":b.digest(p.read_bytes()),"source":p.read_text(encoding="utf-8-sig")} for p in Path(local.name).glob("tls-*.ps1")}
        remote.upload(args.exe,directory)
        require(remote.powershell("(Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path "+b.ps_literal(directory)+" "+b.ps_literal(args.exe.name)+")).Hash.ToLowerInvariant()")==report["exeSha256"]==b.digest(args.exe.read_bytes()),"actual uploaded executable identity")
        pem = Path(local.name)/"owned-public.pem";pem.write_text(fixture["publicPem"],encoding="ascii")
        expected_der = ssl.PEM_cert_to_DER_cert(fixture["publicPem"])
        context = tls.context(pem)
        tunnel = tls.Tunnel(remote,local.name)
        launcher = subprocess.Popen(remote.command_file(directory+"\\tls-launch.ps1"),stdout=subprocess.PIPE,stderr=subprocess.PIPE)
        logs = tls.Logs(launcher)
        lines, ready, exits, client_deadline = [], [], [], None
        until=time.monotonic()+90
        while time.monotonic()<until:
            try:
                line=logs.lines.get(timeout=.1)
            except queue.Empty:
                if launcher.poll() is not None and all(not t.is_alive() for t in logs.threads):break
                continue
            lines.append(line)
            if line.startswith("ORDINARY_HTTPS_READY "):
                value=json.loads(line.removeprefix("ORDINARY_HTTPS_READY "))
                validate_ready(value,marker,port,len(ready)+1,ready[-1]["generation"] if ready else None)
                ready.append(value)
                require(len(ready)<=2,"bounded two native generations")
                client_deadline=client_deadline or time.monotonic()+30
                local_port=tunnel.open(port)
                report["events"].append(status_get(local_port,port,context,expected_der,client_deadline))
                tunnel.cancel()
            elif line.startswith(tls.EXIT):exits.append(json.loads(line[len(tls.EXIT):]))
        launcher.wait(timeout=max(.01,until-time.monotonic()));logs.join()
        report["transcript"]=validate_transcript(lines)
        require(launcher.returncode==0 and len(exits)==1 and exits[0].get("code")==0,"actual original native and supervisor successful exit")
        require(len(ready)==2 and len(report["events"])==2,"two actual TLS generations observed")
        report.update(status="PASS",ready=ready,nativeExit=exits[0])
    except Exception as error:
        report["error"]=tls.failure(error,"ordinary-https-composition")
    finally:
        if launcher:
            try:
                if launcher.poll() is None:
                    launcher.terminate()
                    try:launcher.wait(timeout=5)
                    except subprocess.TimeoutExpired:launcher.kill();launcher.wait(timeout=5)
                if logs:logs.join();report["sshLauncherRaw"]=logs.report()
            except Exception as error:report.update(status="FAIL",launcherCleanupError=tls.failure(error,"physical-owned-ssh-exit"))
        if tunnel:
            try:tunnel.close();report["sshTunnelRaw"]=tunnel.logs.report()
            except Exception as error:report.update(status="FAIL",tunnelCleanupError=tls.failure(error,"physical-owned-forward-exit"))
        if directory:b.cleanup_owned(remote,directory,marker,uploaded,args.report,token,report)
        report["remoteControlRunsRaw"]=remote.script_runs
        local.cleanup()
        args.report.parent.mkdir(parents=True,exist_ok=True)
        with args.report.open("x",encoding="utf-8") as output:json.dump(report,output,ensure_ascii=False,indent=2);output.write("\n")
    require(report["status"]==report["cleanup"]=="PASS","ordinary probe failed; preserve report "+str(args.report))
    print("ORDINARY_HTTPS_RUNNER_PASS "+str(args.report))


if __name__ == "__main__":
    main()
