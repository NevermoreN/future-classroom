#!/usr/bin/env python3
"""Finite hidden ordinary HTTPS composition, never a school listener deployment.

Uses one supplied EXE, two actual configured loopback generations and two strict
TLS status GETs plus an actual approved teacher session and reminder POST/GET. Existing certificate/key/process/raw-evidence ownership helpers
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
    "setup-local-global-administrator", "native-create-synthetic-class", "native-teacher-page-default-off", "native-open-https-settings",
    "native-save-explicit-ip-cidr-reference", "saved-config-no-listener", "native-enable-configured-runtime",
    "physical-first-running-generation", "first-real-tls-response-complete",
    "native-open-actual-runtime-pairing-window", "native-approve-actual-network-pairing-request",
    "first-real-authenticated-reminder-post-and-get", "native-pairing-close-and-disable-review-cancel", "native-confirm-first-disable",
    "physical-first-domain-drained", "original-shared-workers-remain-operational",
    "native-enable-second-generation", "second-real-tls-response-complete",
    "native-lock-does-not-stop-established-domain", "native-relogin-confirm-second-disable",
    "physical-second-domain-drained", "native-return-to-local-workbench",
    "actual-shared-worker-writer-shutdown", "owned-deployment-cleanup",
)
_STEP = re.compile(r"ORDINARY_HTTPS_NATIVE_STEP index=(0|[1-9][0-9]*) name=([a-z-]+)")
_FOOTER = re.compile(r"ORDINARY_HTTPS_NATIVE_PASS steps=24 shared_workers=2 tls_cycles=2 authenticated_reminder=PASS physical_shutdown=PASS isolated_cleanup=PASS scope=OWNED_LOOPBACK_COMPOSITION material_ready_stop=NOT_EXERCISED campus=NOT_VALIDATED elapsed_ms=(0|[1-9][0-9]*)")
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
    require([(i, name) for i, name, _ in steps] == list(enumerate(STEPS)), "ordinary exact twenty-four steps")
    require(len(footer) == 1 and footer[0] > steps[-1][2], "ordinary footer after all steps")
    return {"steps": len(steps), "authenticatedReminder": "PASS", "physicalShutdown": "PASS", "materialReadyStop": "NOT_EXERCISED"}


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


def validate_pair_ready(value, marker, port, generation):
    require(type(value) is dict and set(value) == {"cycle", "marker", "port", "generation", "origin"}
            and type(value["cycle"]) is int and value["cycle"] == 1
            and value["marker"] == marker and type(value["port"]) is int and value["port"] == port
            and type(value["generation"]) is int and value["generation"] == generation
            and value["origin"] == f"https://127.0.0.1:{port}", "public pairing rendezvous identity")


def read_pairing_secret(remote, directory, marker, generation, deadline):
    # This one SSH subprocess is deliberately outside the raw recording helper.
    # No local file, stdout/stderr artifact, digest, exception contents or report
    # may retain the short code. Remove the exact owned file before delivery.
    require(time.monotonic() < deadline, "original client secret-read deadline")
    script = ("$r=" + b.ps_literal(directory) + ";$m=" + b.ps_literal(marker)
        + ";if((Split-Path -Leaf $r)-ne $m-or [IO.File]::ReadAllText((Join-Path $r '.tls-probe-root'))-ne $m){throw 'owned channel mismatch'};"
        + "$p=Join-Path $r 'ordinary-pairing-first.secret';$f=Get-Item -LiteralPath $p;"
        + "if($f.Length-gt 256-or ($f.Attributes-band [IO.FileAttributes]::ReparsePoint)){throw 'bounded regular secret channel required'};"
        + "$v=[IO.File]::ReadAllText($p);Remove-Item -LiteralPath $p;[Console]::Write($v)")
    try:
        result = subprocess.run(remote.command(script), capture_output=True, timeout=min(8, deadline-time.monotonic()))
    except (OSError, subprocess.TimeoutExpired):
        raise RuntimeError("owned pairing memory-only read failed") from None
    require(result.returncode == 0 and len(result.stdout) <= 256, "owned pairing memory-only read exit")
    try:
        text = result.stdout.decode("ascii", "strict")
    except UnicodeError:
        raise RuntimeError("owned pairing memory-only encoding") from None
    require(time.monotonic() < deadline, "owned pairing read before original deadline")
    return parse_pairing_secret(text, marker, generation)


def parse_pairing_secret(text, marker, generation):
    fields = text.split("\n")
    require(len(fields) == 6 and fields[0] == "FC_ORDINARY_PAIR_V1" and fields[1] == marker
            and fields[2] == f"{generation:020}" and re.fullmatch(r"[0-9]{8}", fields[3])
            and fields[4] == "synthetic-voice-settings-admin" and fields[5] == "", "owned pairing channel exact shape")
    return fields[3], fields[4]


class OrdinaryClient:
    """Strict IP-SAN client with ephemeral auth values retained only in memory."""
    def __init__(self, local_port, origin_port, context, expected_der, deadline, events):
        self.local_port, self.origin_port = local_port, origin_port
        self.context, self.expected_der, self.deadline, self.events = context, expected_der, deadline, events
        self.cookie = self.csrf = None

    def request(self, case, method, route, body=None, expected=(200,), authenticated=True):
        require(time.monotonic() < self.deadline, "original business client deadline")
        payload = b"" if body is None else b.json_bytes(body)
        headers = [f"{method} /fc/api/v1/{route} HTTP/1.1", f"Host: 127.0.0.1:{self.origin_port}",
                   f"Origin: https://127.0.0.1:{self.origin_port}", "Connection: close"]
        if authenticated and self.cookie:
            headers.append("Cookie: " + self.cookie)
        if method == "POST":
            headers += ["Content-Type: application/json", f"Content-Length: {len(payload)}"]
            if authenticated and self.csrf:
                headers.append("X-FC-CSRF: " + self.csrf)
        require(method == "POST" or not payload, "GET has no body")
        parser = b.HttpResponse()
        with socket.create_connection(("127.0.0.1", self.local_port), timeout=min(5, self.deadline-time.monotonic())) as raw:
            with self.context.wrap_socket(raw, server_hostname="127.0.0.1", suppress_ragged_eofs=False) as connection:
                require(connection.getpeercert(binary_form=True) == self.expected_der and connection.version() == "TLSv1.2",
                        "actual exact owned IP-SAN TLS1.2")
                connection.sendall(("\r\n".join(headers)+"\r\n\r\n").encode("ascii")+payload)
                while True:
                    require(time.monotonic() < self.deadline, "original business receive deadline")
                    connection.settimeout(min(5, self.deadline-time.monotonic()))
                    chunk = connection.recv(16384)
                    if not chunk:
                        break
                    parser.feed(chunk)
        parser.finish()
        require(parser.status in expected, "ordinary business unexpected HTTP status")
        self.events.append({"case": case, "method": method, "route": route, "httpStatus": parser.status,
                            "requestBodyBytes": len(payload), "responseBodyBytes": parser.body_bytes,
                            "authenticatedCookieSent": bool(authenticated and self.cookie),
                            "csrfSent": bool(method == "POST" and authenticated and self.csrf),
                            "tls": "TLSv1.2", "serverName": "127.0.0.1", "cleanTlsEof": True})
        return parser


def authenticate_and_write(client, code, login):
    denied = client.request("ordinary-private-without-session", "GET", "session", expected=(401,), authenticated=False)
    require(denied.json().get("code") == "UNAUTHORIZED", "ordinary private session rejected before approval")
    requested = client.request("ordinary-pairing-request", "POST", "pairing/requests",
        {"schemaVersion": 1, "code": code, "loginName": login, "deviceName": "Synthetic ordinary HTTPS teacher"},
        expected=(202,), authenticated=False).json()
    request_id, secret = requested.get("requestId"), requested.get("pollSecret")
    require(requested.get("status") == "PENDING" and isinstance(request_id, str)
            and re.fullmatch(r"[1-9][0-9]{0,19}", request_id) and isinstance(secret, str)
            and re.fullmatch(r"[0-9a-fA-F]{64}", secret), "ordinary pairing receipt contract")
    authorized = None
    for index in range(8):
        time.sleep(.65)  # Respect the actual API's minimum poll interval.
        polled = client.request("ordinary-pairing-poll-"+str(index+1), "POST", f"pairing/requests/{request_id}/poll",
            {"schemaVersion": 1, "pollSecret": secret}, expected=(200,202), authenticated=False)
        if polled.status == 200:
            authorized = polled
            break
        require(polled.json().get("status") == "PENDING", "ordinary pairing pending state")
    require(authorized is not None and authorized.json().get("status") == "AUTHORIZED", "actual native approval observed")
    cookie = authorized.unique("set-cookie")
    require(re.fullmatch(r"__Host-FutureClassroom=[0-9a-fA-F]{64}; Path=/; Secure; HttpOnly; SameSite=Strict", cookie), "ordinary exact secure host cookie")
    client.cookie = cookie.split(";",1)[0]
    session = client.request("ordinary-authenticated-session", "GET", "session").json()
    require(session.get("status") == "AUTHENTICATED" and isinstance(session.get("csrf"), str)
            and re.fullmatch(r"[0-9a-fA-F]{64}", session["csrf"]), "ordinary actual authenticated session")
    client.csrf = session["csrf"]
    scopes = client.request("ordinary-reminder-scopes", "GET", "content/reminders/scopes/0").json()
    today = scopes.get("today")
    require(scopes.get("status") == "SCOPES" and scopes.get("kind") == "REMINDER"
            and isinstance(today,str) and re.fullmatch(r"[0-9]{4}-[0-9]{2}-[0-9]{2}",today)
            and scopes.get("total") == 1 and scopes.get("items",[{}])[0].get("classId") == "1", "ordinary synthetic reminder scope")
    route = "content/reminders/1/"+today
    before = client.request("ordinary-empty-reminder", "GET", route).json()
    require(before.get("status") == "CONTENT" and before.get("revision") == "0" and before.get("text") == "", "ordinary original empty reminder")
    command, text = "68001", "Synthetic ordinary HTTPS reminder committed through the real authenticated API."
    committed = client.request("ordinary-authenticated-reminder-post", "POST", route,
        {"schemaVersion": 1, "commandId": command, "expectedRevision": "0", "text": text}).json()
    require(committed.get("status") == "COMMITTED" and committed.get("commandId") == command
            and committed.get("content",{}).get("revision") == "1" and committed["content"].get("text") == text,
            "ordinary durable POST revision/text")
    actual = client.request("ordinary-authenticated-reminder-readback", "GET", route).json()
    require(actual.get("status") == "CONTENT" and actual.get("revision") == "1" and actual.get("text") == text,
            "ordinary committed reminder GET readback")
    receipt = client.request("ordinary-durable-reminder-receipt", "GET", "content/commands/"+command).json()
    require(receipt.get("status") == "COMMAND_RESULT" and receipt.get("state") == "COMMITTED"
            and receipt.get("commandId") == command and receipt.get("content",{}).get("revision") == "1"
            and receipt["content"].get("text") == text, "ordinary exact durable command receipt")
    client.cookie = client.csrf = None
    return {"status": "PASS", "session": "ACTUAL_NATIVE_APPROVAL", "classId": "1", "commandId": command,
            "beforeRevision": "0", "committedRevision": "1", "readbackRevision": "1", "durableReceipt": "COMMITTED",
            "authenticatedGet": "PASS", "authenticatedPost": "PASS", "secretPersistence": "NONE"}


def signal_business_done(remote, directory, marker, generation, deadline):
    require(time.monotonic() < deadline, "client done within original deadline")
    value = f"FC_ORDINARY_DONE_V1\n{marker}\n{generation:020}\n"
    script = ("$r="+b.ps_literal(directory)+";$m="+b.ps_literal(marker)
        + ";if((Split-Path -Leaf $r)-ne $m-or [IO.File]::ReadAllText((Join-Path $r '.tls-probe-root'))-ne $m){throw 'owned completion mismatch'};"
        + "$p=Join-Path $r 'ordinary-business-first.done';$s=[IO.File]::Open($p,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::None);"
        + "try{$v=[Text.Encoding]::ASCII.GetBytes("+b.ps_literal(value)+");$s.Write($v,0,$v.Length)}finally{$s.Dispose()};'OWNED_CLIENT_JOURNEY_DONE'")
    require(remote.powershell(script, timeout=min(8,deadline-time.monotonic())) == "OWNED_CLIENT_JOURNEY_DONE", "owned completion creation")


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
    footer = "ORDINARY_HTTPS_NATIVE_PASS steps=24 shared_workers=2 tls_cycles=2 authenticated_reminder=PASS physical_shutdown=PASS isolated_cleanup=PASS scope=OWNED_LOOPBACK_COMPOSITION material_ready_stop=NOT_EXERCISED campus=NOT_VALIDATED elapsed_ms=4000"
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
    public_pair = {"cycle":1,"marker":marker,"port":23456,"generation":17,"origin":"https://127.0.0.1:23456"}
    validate_pair_ready(public_pair,marker,23456,17)
    for invalid in [dict(public_pair,pairingCode="00000000"),dict(public_pair,generation=18),dict(public_pair,generation=True),
                    dict(public_pair,marker=marker+"x"),dict(public_pair,cycle=2),dict(public_pair,origin="https://localhost:23456")]:
        try:
            validate_pair_ready(invalid,marker,23456,17)
        except RuntimeError:
            pass
        else:
            raise RuntimeError("secret-bearing/foreign pairing rendezvous accepted")
    secret = f"FC_ORDINARY_PAIR_V1\n{marker}\n{17:020}\n00000000\nsynthetic-voice-settings-admin\n"
    require(parse_pairing_secret(secret,marker,17)==("00000000","synthetic-voice-settings-admin"), "pure synthetic secret channel parser")
    for invalid in [secret+"x",secret.replace(f"{17:020}",f"{18:020}"),secret.replace("00000000\nsynthetic", "not-code\nsynthetic"),
                    secret.replace(marker,marker+"x"),secret.replace("FC_ORDINARY_PAIR_V1","unrecognized"),secret.replace("synthetic-voice-settings-admin","foreign")]:
        try:
            parse_pairing_secret(invalid,marker,17)
        except RuntimeError:
            pass
        else:
            raise RuntimeError("foreign/unbounded/malformed secret channel accepted")
    print(json.dumps({"status": "PASS", "scope": "pure validators only", "checks": 34,
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
        "fixtureSetupSourceSha256": b.digest(tls.SETUP.encode("utf-8")),
        "businessHelperSourceSha256": b.digest(Path(__file__).with_name("windows-tls-business-probe.py").read_bytes()),
        "exeSha256": b.digest(executable), "events": []}
    uploaded, directory, launcher, logs, tunnel = set(), None, None, None, None
    local = tempfile.TemporaryDirectory(prefix="fc-ordinary-https-")
    try:
        directory = remote.powershell("$r=Join-Path $env:TEMP "+b.ps_literal(marker)+";New-Item -ItemType Directory -Path $r|Out-Null;$sid=[Security.Principal.WindowsIdentity]::GetCurrent().User;$acl=[Security.AccessControl.DirectorySecurity]::new();$acl.SetOwner($sid);$acl.SetAccessRuleProtection($true,$false);$rule=[Security.AccessControl.FileSystemAccessRule]::new($sid,[Security.AccessControl.FileSystemRights]::FullControl,([Security.AccessControl.InheritanceFlags]::ContainerInherit-bor [Security.AccessControl.InheritanceFlags]::ObjectInherit),[Security.AccessControl.PropagationFlags]::None,[Security.AccessControl.AccessControlType]::Allow);$acl.AddAccessRule($rule);Set-Acl -LiteralPath $r -AclObject $acl;if(!(Get-Acl -LiteralPath $r).AreAccessRulesProtected){throw 'owned root ACL not protected'};[IO.File]::WriteAllText((Join-Path $r '.tls-probe-root'),"+b.ps_literal(marker)+");$r")
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
        require(context.check_hostname and context.verify_mode == ssl.CERT_REQUIRED, "strict IP-SAN verification")
        report["clientVerification"]={"checkHostname":True,"verifyMode":"CERT_REQUIRED","serverName":"127.0.0.1",
            "minimumTls":"TLSv1.2","maximumTls":"TLSv1.2","peerCertificate":"EXACT_OWNED_DER",
            "trustSource":"only owned public PEM","cleanTlsEof":"REQUIRED","systemTrust":"UNCHANGED"}
        tunnel = tls.Tunnel(remote,local.name)
        launcher = subprocess.Popen(remote.command_file(directory+"\\tls-launch.ps1"),stdout=subprocess.PIPE,stderr=subprocess.PIPE)
        logs = tls.Logs(launcher)
        lines, ready, exits, client_deadline, paired = [], [], [], None, False
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
            elif line.startswith("ORDINARY_HTTPS_PAIR_READY "):
                require(len(ready) == 1 and not paired and client_deadline is not None, "one first-generation pairing rendezvous")
                value=json.loads(line.removeprefix("ORDINARY_HTTPS_PAIR_READY "))
                validate_pair_ready(value,marker,port,ready[0]["generation"])
                code, login = read_pairing_secret(remote,directory,marker,ready[0]["generation"],client_deadline)
                local_port=tunnel.open(port)
                client=OrdinaryClient(local_port,port,context,expected_der,client_deadline,report["events"])
                report["authenticatedBusiness"]=authenticate_and_write(client,code,login)
                del code, login, client
                tunnel.cancel()
                signal_business_done(remote,directory,marker,ready[0]["generation"],client_deadline)
                paired=True
            elif line.startswith(tls.EXIT):exits.append(json.loads(line[len(tls.EXIT):]))
        launcher.wait(timeout=max(.01,until-time.monotonic()));logs.join()
        report["transcript"]=validate_transcript(lines)
        require(launcher.returncode==0 and len(exits)==1 and exits[0].get("code")==0,"actual original native and supervisor successful exit")
        require(len(ready)==2 and paired and len(report["events"])>=12,"two actual TLS generations and actual authenticated business observed")
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
        if directory:
            try:
                report["pairingSecretCleanup"]=remote.powershell("$r="+b.ps_literal(directory)+";$m="+b.ps_literal(marker)
                    +";if([IO.File]::ReadAllText((Join-Path $r '.tls-probe-root'))-ne $m){throw 'owned secret cleanup mismatch'};"
                    +"$p=Join-Path $r 'ordinary-pairing-first.secret';if(Test-Path -LiteralPath $p){Remove-Item -LiteralPath $p};"
                    +"if(Test-Path -LiteralPath $p){throw 'owned secret remains'};'PASS'",timeout=8)
            except Exception:
                report.update(status="FAIL",pairingSecretCleanup="FAIL")
        if directory:b.cleanup_owned(remote,directory,marker,uploaded,args.report,token,report)
        report["remoteControlRunsRaw"]=remote.script_runs
        local.cleanup()
        args.report.parent.mkdir(parents=True,exist_ok=True)
        with args.report.open("x",encoding="utf-8") as output:json.dump(report,output,ensure_ascii=False,indent=2);output.write("\n")
    require(report["status"]==report["cleanup"]=="PASS","ordinary probe failed; preserve report "+str(args.report))
    print("ORDINARY_HTTPS_RUNNER_PASS "+str(args.report))


if __name__ == "__main__":
    main()
