#!/usr/bin/env python3
"""Finite hidden ordinary HTTPS composition, never a school listener deployment.

Uses one supplied EXE, two actual configured loopback generations and two strict
TLS status GETs plus an actual approved teacher session and reminder POST/GET. Existing certificate/key/process/raw-evidence ownership helpers
are reused. No browser, recording, playback, trust/firewall/HTTP.sys mutation.
MaterialReady/partial upload stops use the ordinary owner; campus and Win7/Win10 remain separate tests.
"""
import argparse
import copy
from datetime import date
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
    "setup-local-global-administrator", "native-create-synthetic-class",
    "native-create-independent-teacher-and-complete-first-password-change", "native-teacher-page-default-off", "native-open-https-settings",
    "native-save-explicit-ip-cidr-reference", "saved-config-no-listener", "native-enable-configured-runtime",
    "physical-first-running-generation", "first-real-tls-response-complete",
    "native-open-actual-runtime-pairing-window", "native-approve-actual-network-pairing-request",
    "first-real-authenticated-reminder-post-and-get", "first-material-ready-and-partial-upload-established", "native-pairing-close-and-disable-review-cancel", "native-confirm-first-disable",
    "physical-first-domain-drained", "first-two-staging-files-removed", "original-shared-workers-remain-operational",
    "native-enable-second-generation", "second-real-tls-response-complete",
    "native-open-second-runtime-pairing-window", "native-approve-second-network-pairing-request",
    "second-old-cookie-rejected-and-two-upload-slots-reused", "second-typed-material-command-receipt-and-cancel",
    "native-lock-does-not-stop-established-domain", "native-relogin-confirm-second-disable",
    "physical-second-domain-drained", "native-return-to-local-workbench",
    "actual-shared-worker-writer-shutdown", "owned-deployment-cleanup",
)
_STEP = re.compile(r"ORDINARY_HTTPS_NATIVE_STEP index=(0|[1-9][0-9]*) name=([a-z-]+)")
_FOOTER = re.compile(r"ORDINARY_HTTPS_NATIVE_PASS steps=31 shared_workers=2 tls_cycles=2 authenticated_reminder=PASS physical_shutdown=PASS isolated_cleanup=PASS scope=OWNED_LOOPBACK_COMPOSITION material_ready_stop=PASS partial_upload_stop=PASS upload_slots_reused=2 typed_material_receipt=PASS campus=NOT_VALIDATED elapsed_ms=(0|[1-9][0-9]*)")
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
    require([(i, name) for i, name, _ in steps] == list(enumerate(STEPS)), "ordinary exact thirty-one steps")
    require(len(footer) == 1 and footer[0] > steps[-1][2], "ordinary footer after all steps")
    return {"steps": len(steps), "authenticatedReminder": "PASS", "physicalShutdown": "PASS", "materialReadyStop": "PASS"}


def validate_status(value):
    require(isinstance(value, dict) and set(value) == {"schemaVersion", "apiVersion", "uiCompatibilityVersion", "online"}
            and all(type(value[k]) is int and value[k] == 1 for k in ("schemaVersion", "apiVersion", "uiCompatibilityVersion"))
            and value["online"] is True, "ordinary exact public-status DTO")


def status_get(local_port, origin_port, context, expected_der, deadline, cycle, generation):
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
    return {"case": f"ordinary-public-status-cycle-{cycle}", "method": "GET", "route": "status",
            "cycle": cycle, "generation": generation, "httpStatus": parser.status, "bodyBytes": parser.body_bytes,
            "bodySha256": parser.body_hash.hexdigest(), "cleanTlsEof": True,
            "tls": "TLSv1.2", "serverName": "127.0.0.1", "cookieSent": False}


def validate_pair_ready(value, marker, port, generation, cycle=1):
    require(type(value) is dict and set(value) == {"cycle", "marker", "port", "generation", "origin"}
            and type(value["cycle"]) is int and value["cycle"] == cycle
            and value["marker"] == marker and type(value["port"]) is int and value["port"] == port
            and type(value["generation"]) is int and value["generation"] == generation
            and value["origin"] == f"https://127.0.0.1:{port}", "public pairing rendezvous identity")


def read_pairing_secret(remote, directory, marker, generation, deadline, cycle=1):
    # This one SSH subprocess is deliberately outside the raw recording helper.
    # No local file, stdout/stderr artifact, digest, exception contents or report
    # may retain the short code. Remove the exact owned file before delivery.
    require(time.monotonic() < deadline, "original client secret-read deadline")
    script = ("$r=" + b.ps_literal(directory) + ";$m=" + b.ps_literal(marker)
        + ";if((Split-Path -Leaf $r)-ne $m-or [IO.File]::ReadAllText((Join-Path $r '.tls-probe-root'))-ne $m){throw 'owned channel mismatch'};"
        + "$p=Join-Path $r "+b.ps_literal(f"ordinary-pairing-{cycle}.secret")+";$f=Get-Item -LiteralPath $p;"
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
    return parse_pairing_secret(text, marker, generation, cycle)


def parse_pairing_secret(text, marker, generation, cycle=1):
    fields = text.split("\n")
    require(type(cycle) is int and cycle in (1,2) and len(fields) == 7
            and fields[0] == "FC_ORDINARY_PAIR_V2" and fields[1] == marker
            and fields[2] == f"{generation:020}" and fields[3] == str(cycle)
            and re.fullmatch(r"[0-9]{8}", fields[4])
            and fields[5] == "synthetic-ordinary-teacher" and fields[6] == "", "owned pairing channel exact shape")
    return fields[4], fields[5]


# Exact closed disclosure enum from fc_protocol::web::responses::ErrorCode.
ERROR_CODES = frozenset({
    "INVALID_REQUEST", "UNAUTHORIZED", "FORBIDDEN", "NOT_FOUND", "METHOD_NOT_ALLOWED",
    "RATE_LIMITED", "SERVICE_UNAVAILABLE", "RESULT_UNKNOWN", "REVISION_CONFLICT", "DAY_CHANGED",
    "IDEMPOTENCY_CONFLICT", "STALE_CURSOR", "UPLOAD_EXPIRED", "UPLOAD_NOT_READY", "INVALID_RESOURCE",
    "CAPACITY", "VOICE_DISABLED", "TARGET_CHANGED", "INVALID_WAV", "QUEUE_FULL", "OWNER_LIMIT", "EXPIRED",
})


def safe_error_code(body):
    # Diagnostic only: do not relax the HTTP status assertion or preserve an
    # unknown/extra field, response body, body hash or parser exception text.
    if not isinstance(body,(bytes,bytearray)) or len(body)>512:
        return None
    def unique(pairs):
        value={}
        for key,item in pairs:
            if key in value:
                raise ValueError("duplicate error DTO field")
            value[key]=item
        return value
    try:
        value=json.loads(bytes(body).decode("utf-8","strict"),object_pairs_hook=unique)
    except (ValueError,UnicodeError,TypeError):
        return None
    if (type(value) is dict and set(value)=={"schemaVersion","code"}
            and type(value["schemaVersion"]) is int and value["schemaVersion"]==1
            and isinstance(value["code"],str) and value["code"] in ERROR_CODES):
        return value["code"]
    return None


class OrdinaryClient:
    """Strict IP-SAN client with ephemeral auth values retained only in memory."""
    def __init__(self, local_port, origin_port, context, expected_der, deadline, events):
        self.local_port, self.origin_port = local_port, origin_port
        self.context, self.expected_der, self.deadline, self.events = context, expected_der, deadline, events
        self.cookie = self.csrf = None

    def request(self, case, method, route, body=None, expected=(200,), authenticated=True,
                content_type="application/json", origin=None, csrf_override=None):
        require(time.monotonic() < self.deadline, "original business client deadline")
        payload = b"" if body is None else body if isinstance(body, bytes) else b.json_bytes(body)
        headers = [f"{method} /fc/api/v1/{route} HTTP/1.1", f"Host: 127.0.0.1:{self.origin_port}",
                   "Origin: " + (origin if origin is not None else f"https://127.0.0.1:{self.origin_port}"), "Connection: close"]
        if authenticated and self.cookie:
            headers.append("Cookie: " + self.cookie)
        if method == "POST":
            headers += ["Content-Type: " + content_type, f"Content-Length: {len(payload)}"]
            if authenticated and self.csrf:
                headers.append("X-FC-CSRF: " + (self.csrf if csrf_override is None else csrf_override))
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
        # Preserve only safe wire metadata even when the status assertion fails.
        # Never include headers, auth-bearing response JSON or its body digest.
        self.events.append({"case": case, "method": method, "route": route, "httpStatus": parser.status,
                            "requestBodyBytes": len(payload), "responseBodyBytes": parser.body_bytes,
                            "authenticatedCookieSent": bool(authenticated and self.cookie),
                            "csrfSent": bool(method == "POST" and authenticated and self.csrf),
                            "tls": "TLSv1.2", "serverName": "127.0.0.1", "cleanTlsEof": True})
        if parser.status not in expected:
            code=safe_error_code(parser.body)
            if code is not None:
                self.events[-1]["errorCode"]=code
        require(parser.status in expected, "ordinary business unexpected HTTP status")
        return parser


def validate_teacher_session(session):
    require(type(session) is dict and set(session) == {"schemaVersion","status","principal","currentClassId","assignments","csrf"}
            and type(session["schemaVersion"]) is int and session["schemaVersion"] == 1
            and session["status"] == "AUTHENTICATED" and session["currentClassId"] == "1"
            and type(session["principal"]) is dict and set(session["principal"]) == {"id","displayName","role"}
            and session["principal"]["id"] == "2" and session["principal"]["role"] == "TEACHER_ADMIN"
            and session["assignments"] == [{"classId":"1","subjectId":"1"}]
            and isinstance(session["csrf"], str) and re.fullmatch(r"[0-9a-fA-F]{64}",session["csrf"]),
            "actual independent teacher session and exact installed class/subject scope")


def authenticate_and_write(client, code, login, cycle=1):
    prefix = "ordinary-" if cycle == 1 else "ordinary-cycle2-"
    denied = client.request(prefix+"private-without-session", "GET", "session", expected=(401,), authenticated=False)
    require(denied.json().get("code") == "UNAUTHORIZED", "ordinary private session rejected before approval")
    requested = client.request(prefix+"pairing-request", "POST", "pairing/requests",
        {"schemaVersion": 1, "code": code, "loginName": login, "deviceName": "Synthetic ordinary HTTPS teacher"},
        expected=(202,), authenticated=False).json()
    request_id, secret = requested.get("requestId"), requested.get("pollSecret")
    require(requested.get("status") == "PENDING" and isinstance(request_id, str)
            and re.fullmatch(r"[1-9][0-9]{0,19}", request_id) and isinstance(secret, str)
            and re.fullmatch(r"[0-9a-fA-F]{64}", secret), "ordinary pairing receipt contract")
    authorized = None
    for index in range(8):
        time.sleep(.65)  # Respect the actual API's minimum poll interval.
        polled = client.request(prefix+"pairing-poll-"+str(index+1), "POST", f"pairing/requests/{request_id}/poll",
            {"schemaVersion": 1, "pollSecret": secret}, expected=(200,202), authenticated=False)
        if polled.status == 200:
            authorized = polled
            break
        require(polled.json().get("status") == "PENDING", "ordinary pairing pending state")
    require(authorized is not None and authorized.json().get("status") == "AUTHORIZED", "actual native approval observed")
    cookie = authorized.unique("set-cookie")
    require(re.fullmatch(r"__Host-FutureClassroom=[0-9a-fA-F]{64}; Path=/; Secure; HttpOnly; SameSite=Strict", cookie), "ordinary exact secure host cookie")
    client.cookie = cookie.split(";",1)[0]
    session = client.request(prefix+"authenticated-session", "GET", "session").json()
    require(session.get("status") == "AUTHENTICATED" and isinstance(session.get("csrf"), str)
            and re.fullmatch(r"[0-9a-fA-F]{64}", session["csrf"]), "ordinary actual authenticated session")
    validate_teacher_session(session)
    client.csrf = session["csrf"]
    if cycle == 2:
        return {"status":"PASS", "principalId":"2", "role":"TEACHER_ADMIN",
                "pairingRequestId":request_id, "pollAttempts":index+1}
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
    return {"status": "PASS", "session": "ACTUAL_NATIVE_APPROVAL", "principalId": "2", "role": "TEACHER_ADMIN", "classId": "1", "commandId": command,
            "beforeRevision": "0", "committedRevision": "1", "readbackRevision": "1", "durableReceipt": "COMMITTED",
            "authenticatedGet": "PASS", "authenticatedPost": "PASS", "secretPersistence": "NONE",
            "pairingRequestId": request_id, "pollAttempts": index+1, "contentDate": today}


def validate_journey(events, ready, business, marker, port):
    """Close the exact wire operation sequence, including 1..8 actual polls.

    Response DTO/cookie/role/content checks occur at each real request above;
    this final validator binds their safe summary to the ordered observations.
    It does not authorize, send/retry a request, or synthesize native readiness.
    """
    require(type(ready) is list and len(ready) == 2, "two actual native generations")
    validate_ready(ready[0],marker,port,1)
    validate_ready(ready[1],marker,port,2,previous=ready[0]["generation"])
    fixed = {"status":"PASS","session":"ACTUAL_NATIVE_APPROVAL","principalId":"2","role":"TEACHER_ADMIN",
        "classId":"1","commandId":"68001","beforeRevision":"0","committedRevision":"1","readbackRevision":"1",
        "durableReceipt":"COMMITTED","authenticatedGet":"PASS","authenticatedPost":"PASS","secretPersistence":"NONE"}
    require(type(business) is dict and set(business) == set(fixed)|{"pairingRequestId","pollAttempts","contentDate"}
            and all(business.get(k) == v for k,v in fixed.items()), "complete actual teacher/reminder/receipt summary")
    polls, request_id, today = business["pollAttempts"], business["pairingRequestId"], business["contentDate"]
    require(type(polls) is int and 1 <= polls <= 8 and isinstance(request_id,str)
            and re.fullmatch(r"[1-9][0-9]{0,19}",request_id) and isinstance(today,str), "original bounded pairing polls/receipt identity")
    try:
        require(date.fromisoformat(today).isoformat() == today, "canonical actual content date")
    except ValueError:
        raise RuntimeError("invalid actual content date") from None
    route = "content/reminders/1/"+today
    # Each row is a required actual operation, not a lower request-count bound.
    expected = [
        ("ordinary-public-status-cycle-1","GET","status",200,False,False),
        ("ordinary-private-without-session","GET","session",401,False,False),
        ("ordinary-pairing-request","POST","pairing/requests",202,False,False),
    ]
    expected += [(f"ordinary-pairing-poll-{i}","POST",f"pairing/requests/{request_id}/poll",202 if i < polls else 200,False,False)
                 for i in range(1,polls+1)]
    expected += [
        ("ordinary-authenticated-session","GET","session",200,True,False),
        ("ordinary-reminder-scopes","GET","content/reminders/scopes/0",200,True,False),
        ("ordinary-empty-reminder","GET",route,200,True,False),
        ("ordinary-authenticated-reminder-post","POST",route,200,True,True),
        ("ordinary-authenticated-reminder-readback","GET",route,200,True,False),
        ("ordinary-durable-reminder-receipt","GET","content/commands/68001",200,True,False),
        ("ordinary-public-status-cycle-2","GET","status",200,False,False),
    ]
    require(type(events) is list and len(events) == len(expected), "exact required wire operations; no omitted or extra request")
    common = {"case","method","route","httpStatus","tls","serverName","cleanTlsEof"}
    for event,wanted in zip(events,expected):
        require(type(event) is dict and tuple(event.get(k) for k in ("case","method","route","httpStatus")) == wanted[:4]
                and type(event["httpStatus"]) is int and event.get("tls") == "TLSv1.2"
                and event.get("serverName") == "127.0.0.1" and event.get("cleanTlsEof") is True,
                "exact ordered wire method/route/status/TLS clean EOF")
        if wanted[0].startswith("ordinary-public-status-cycle-"):
            cycle = 1 if wanted[0].endswith("-1") else 2
            require(set(event) == common|{"cycle","generation","bodyBytes","bodySha256","cookieSent"}
                    and type(event["cycle"]) is int and event["cycle"] == cycle
                    and type(event["generation"]) is int and event["generation"] == ready[cycle-1]["generation"]
                    and event["cookieSent"] is False and type(event["bodyBytes"]) is int and 0 < event["bodyBytes"] <= b.BODY_LIMIT
                    and isinstance(event["bodySha256"],str) and re.fullmatch(r"[0-9a-f]{64}",event["bodySha256"]),
                    "exact two public status observations bound to actual cycles/generations")
        else:
            require(set(event) == common|{"requestBodyBytes","responseBodyBytes","authenticatedCookieSent","csrfSent"}
                    and event["authenticatedCookieSent"] is wanted[4] and event["csrfSent"] is wanted[5]
                    and type(event["requestBodyBytes"]) is int and type(event["responseBodyBytes"]) is int
                    and 0 < event["responseBodyBytes"] <= b.BODY_LIMIT
                    and (event["requestBodyBytes"] == 0 if wanted[1] == "GET" else 0 < event["requestBodyBytes"] <= b.BODY_LIMIT),
                    "wire body bounds and exact authentication/CSRF observations")
    return {"status":"PASS","publicStatusRequests":2,"businessRequests":8+polls,"pairingPolls":polls,
            "totalRequests":len(expected),"authenticatedReminder":"COMMITTED_REVISION1_READBACK_AND_DURABLE_RECEIPT",
            "scope":"exact ordered operations; successful approval may take one to eight original bounded polls"}


def business_done_value(marker, generation, cycle, ids):
    require(isinstance(marker,str) and re.fullmatch(r"FutureClassroom-Test-TLS-[0-9a-f]{32}",marker)
            and type(generation) is int and 0<generation<=2**64-1
            and type(cycle) is int and cycle in (1,2), "owned completion identity")
    require(type(ids) is list and len(ids)==2 and ids[0]!=ids[1]
            and all(isinstance(value,str) and re.fullmatch(r"[1-9][0-9]{0,19}",value)
                    and int(value)<=2**64-1 for value in ids), "two canonical original u64 upload IDs")
    return (f"FC_ORDINARY_DONE_V2\n{marker}\n{generation:020d}\n{cycle}\n"
            f"{int(ids[0]):020d}\n{int(ids[1]):020d}\n")


def signal_business_done(remote, directory, marker, generation, deadline, cycle, ids):
    require(time.monotonic() < deadline, "client done within original deadline")
    value = business_done_value(marker,generation,cycle,ids)
    script = ("$r="+b.ps_literal(directory)+";$m="+b.ps_literal(marker)
        + ";if((Split-Path -Leaf $r)-ne $m-or [IO.File]::ReadAllText((Join-Path $r '.tls-probe-root'))-ne $m){throw 'owned completion mismatch'};"
        + "$p=Join-Path $r "+b.ps_literal(f"ordinary-business-{cycle}.done")+";$t=Join-Path $r "+b.ps_literal(f"ordinary-business-{cycle}.pending")+";"
        + "$s=[IO.File]::Open($t,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::None);"
        + "try{$v=[Text.Encoding]::ASCII.GetBytes("+b.ps_literal(value)+");$s.Write($v,0,$v.Length);$s.Flush($true)}finally{$s.Dispose()};"
        + "[IO.File]::Move($t,$p);'OWNED_CLIENT_JOURNEY_DONE'")
    require(remote.powershell(script, timeout=min(8,deadline-time.monotonic())) == "OWNED_CLIENT_JOURNEY_DONE", "owned completion creation")



MATERIAL = (b"Synthetic ordinary transfer fixture.\n" * 2048)[:65536]
require(len(MATERIAL) == 65536, "fixed material fixture bytes")


def validate_upload(value, upload_id=None, uploaded=False):
    fields = {"schemaVersion","status","uploadId","bytes"} if uploaded else {"schemaVersion","status","uploadId","expiresInSeconds","maxChunkBytes"}
    require(type(value) is dict and set(value) == fields and type(value["schemaVersion"]) is int
            and value["schemaVersion"] == 1 and isinstance(value["uploadId"],str)
            and re.fullmatch(r"[1-9][0-9]{0,19}",value["uploadId"])
            and int(value["uploadId"]) <= 2**64-1, "typed material upload response")
    require(upload_id is None or value["uploadId"] == upload_id, "exact original upload identity")
    if uploaded:
        require(value["status"] == "UPLOADED" and type(value["bytes"]) is int and value["bytes"] == len(MATERIAL), "actual complete upload receipt")
    else:
        require(value["status"] == "UPLOAD_READY" and type(value["expiresInSeconds"]) is int
                and value["expiresInSeconds"] == 300 and type(value["maxChunkBytes"]) is int
                and value["maxChunkBytes"] == 65536, "actual reservation limits")
    return value["uploadId"]


def create_material(client, case, name, size=65536):
    return validate_upload(client.request(case,"POST","materials/uploads/2/1",
        {"schemaVersion":1,"displayName":name,"bytes":size},expected=(201,)).json())


def upload_material(client, case, upload_id):
    validate_upload(client.request(case,"POST",f"materials/uploads/{upload_id}/bytes",MATERIAL,
        content_type="application/octet-stream").json(), upload_id, uploaded=True)


def validate_material_receipt(value, upload_id, command=False):
    keys = {"schemaVersion","status","commandId","scope","record"} | ({"state"} if command else set())
    require(type(value) is dict and set(value) == keys and type(value["schemaVersion"]) is int
            and value["schemaVersion"] == 1 and value["commandId"] == "69001"
            and value["status"] == ("COMMAND_RESULT" if command else "COMMITTED")
            and (not command or value["state"] == "COMMITTED")
            and value["scope"] == {"ownerTeacherId":"2","subjectId":"1"}, "typed scoped material command receipt")
    record=value["record"]
    require(type(record) is dict and set(record) == {"materialId","folderId","displayName","resourceVersionId","kind","bytes","sha256","allowedClassIds","addedAt","addedSequence","status","revision"}
            and record["resourceVersionId"] == upload_id and record["displayName"] == "R70-reuse-c.txt"
            and record["kind"] == "TXT" and type(record["bytes"]) is int and record["bytes"] == len(MATERIAL)
            and record["sha256"] == b.digest(MATERIAL) and record["allowedClassIds"] == ["1"]
            and record["status"] == "PUBLISHED"
            and all(isinstance(record[k],str) and re.fullmatch(r"[1-9][0-9]{0,19}",record[k])
                    for k in ("materialId","folderId","addedSequence","revision"))
            and all(int(record[k])<=2**64-1 for k in ("materialId","folderId","addedSequence","revision")), "exact published material bytes/hash/version/scope")
    return record


class PartialUpload:
    """One original TLS socket; no own worker or synthetic admission receipt."""
    def __init__(self, client, upload_id):
        self.socket=None
        self.upload_id=upload_id
        raw=socket.create_connection(("127.0.0.1",client.local_port),timeout=min(5,client.deadline-time.monotonic()))
        try:
            self.socket=client.context.wrap_socket(raw,server_hostname="127.0.0.1",suppress_ragged_eofs=False)
            require(self.socket.getpeercert(binary_form=True)==client.expected_der and self.socket.version()=="TLSv1.2","partial exact owned IP-SAN TLS")
            head=(f"POST /fc/api/v1/materials/uploads/{upload_id}/bytes HTTP/1.1\r\nHost: 127.0.0.1:{client.origin_port}\r\n"
                  f"Origin: https://127.0.0.1:{client.origin_port}\r\nCookie: {client.cookie}\r\nX-FC-CSRF: {client.csrf}\r\n"
                  "Content-Type: application/octet-stream\r\nContent-Length: 1048576\r\nConnection: close\r\n\r\n")
            self.socket.sendall(head.encode("ascii")+MATERIAL)
        except BaseException:
            self.close();raw.close();raise
    def peer_stop(self, deadline):
        require(self.socket is not None and time.monotonic()<deadline,"partial original connection retained until native stop")
        self.socket.settimeout(min(5,deadline-time.monotonic()))
        try:
            incoming=self.socket.recv(16384)
            require(not incoming,"incomplete upload must not deliver a successful application response")
            result="TLS_EOF"
        except ssl.SSLEOFError:
            result="TLS_TRUNCATED_BY_STOP"
        except ConnectionResetError:
            result="TCP_RESET_BY_STOP"
        return {"status":"PASS","uploadId":self.upload_id,"declaredBytes":1048576,"sentPrefixBytes":65536,
                "peerTermination":result,"successfulUploadResponse":False,"clientClosedBeforeNativeStop":False}
    def close(self):
        if self.socket is not None:
            self.socket.close();self.socket=None


def stage_sizes(remote,directory,marker,exe_name,ids,deadline):
    require(len(ids)==2 and ids[0]!=ids[1] and all(isinstance(i,str) and re.fullmatch(r"[1-9][0-9]{0,19}",i) for i in ids),"exact two safe stage IDs")
    require(time.monotonic()<deadline,"original stage observation deadline")
    script=("$r="+b.ps_literal(directory)+";$m="+b.ps_literal(marker)
        +";if([IO.File]::ReadAllText((Join-Path $r '.tls-probe-root'))-ne $m){throw 'owned stage root mismatch'};"
        +"$n=Get-Content -Raw -LiteralPath (Join-Path $r '.tls-process.json')|ConvertFrom-Json;"
        +"$p=Get-Process -Id $n.pid;if($n.marker-ne $m-or $p.Path-ne (Join-Path $r "+b.ps_literal(exe_name)+")-or $p.StartTime.ToUniversalTime().Ticks.ToString()-ne $n.startTicks){throw 'owned original process mismatch'};"
        +"$a=Join-Path (Join-Path $r 'temp') ('FutureClassroom-Test-OrdinaryHttps-'+$n.pid+'-'+$m);"
        +"$s=Join-Path $a 'web-material-staging';$v=@();foreach($id in @("+','.join(b.ps_literal(i) for i in ids)+")){"
        +"$f=Join-Path $s ('web-material-'+$id+'.part');if(Test-Path -LiteralPath $f){$i=Get-Item -LiteralPath $f;if($i.PSIsContainer-or ($i.Attributes-band [IO.FileAttributes]::ReparsePoint)){throw 'stage must be regular'};$v+=[long]$i.Length}else{$v+=[long]-1}};ConvertTo-Json -InputObject $v -Compress")
    values=json.loads(remote.powershell(script,timeout=min(8,deadline-time.monotonic())))
    require(type(values) is list and len(values)==2 and all(type(x) is int and -1<=x<=1048576 for x in values),"bounded original stage metadata")
    return values


def first_materials(client,remote,directory,marker,exe_name):
    body={"schemaVersion":1,"displayName":"R70-denied.txt","bytes":65536}
    for case,route,kwargs in [("r70-wrong-origin","materials/uploads/2/1",{"origin":"https://foreign.invalid"}),
        ("r70-wrong-csrf","materials/uploads/2/1",{"csrf_override":"0"*64 if client.csrf!="0"*64 else "1"*64}),
        ("r70-wrong-owner","materials/uploads/1/1",{})]:
        response=client.request(case,"POST",route,body,expected=(403,),**kwargs).json()
        require(response=={"schemaVersion":1,"code":"FORBIDDEN"},"specific source/CSRF/scope denial")
    a=create_material(client,"r70-create-a","R70-ready-a.txt")
    upload_material(client,"r70-bytes-a",a)
    b_id=create_material(client,"r70-create-b","R70-partial-b.txt",1048576)
    require(a!=b_id,"two original upload slots distinct")
    partial=PartialUpload(client,b_id)
    try:
        while True:
            sizes=stage_sizes(remote,directory,marker,exe_name,[a,b_id],client.deadline)
            if sizes[0]==65536 and 0<sizes[1]<1048576:break
            require(time.monotonic()<client.deadline,"actual partial server consumption before original deadline")
            time.sleep(.02)
    except BaseException:
        partial.close();raise
    return [a,b_id],partial,{"status":"PASS","readyStageBytes":sizes[0],"partialStageBytes":sizes[1],"appendBeforeStop":False}


def second_materials(client,old_ids):
    c=create_material(client,"r70-create-c","R70-reuse-c.txt");upload_material(client,"r70-bytes-c",c)
    d=create_material(client,"r70-create-d","R70-reuse-d.txt");upload_material(client,"r70-bytes-d",d)
    require(len(set(old_ids+[c,d]))==4,"new exact upload IDs cannot alias retired generation")
    payload={"schemaVersion":1,"commandId":"69001","uploadId":c,"allowedClassIds":["1"]}
    committed=client.request("r70-append-c","POST","materials/folders/2/1/append",payload)
    record=validate_material_receipt(committed.json(),c)
    replay=client.request("r70-append-c-exact-retry","POST","materials/folders/2/1/append",payload)
    require(replay.body==committed.body,"exact durable material receipt replay")
    receipt=client.request("r70-material-receipt","GET","materials/commands/69001").json()
    require(validate_material_receipt(receipt,c,command=True)==record,"typed durable material readback matches exact original commit")
    cancelled=client.request("r70-cancel-d","POST",f"materials/uploads/{d}/cancel",{"schemaVersion":1}).json()
    require(cancelled=={"schemaVersion":1,"status":"UPLOAD_CANCELLED","uploadId":d},"second physical slot negative cleanup")
    return [c,d],{"status":"PASS","reusedSlots":2,"commandId":"69001","scope":{"ownerTeacherId":"2","subjectId":"1"},"receipt":"COMMITTED","exactRetry":True,"resourceVersionId":c,"sha256":b.digest(MATERIAL),"bytes":65536}


def validate_stop(value,marker,generation,cycle):
    keys={"cycle","marker","generation","elapsedNs","activeRecords","pendingJobs","cleanupPending","retainedCleanup","nativeDrained"}
    require(type(value) is dict and set(value)==keys and type(value["cycle"]) is int and value["cycle"]==cycle
            and value["marker"]==marker and type(value["generation"]) is int and value["generation"]==generation
            and type(value["elapsedNs"]) is int and 0<=value["elapsedNs"]<=5_000_000_000
            and value["nativeDrained"] is True and all(type(value[k]) is int and value[k]==0
                for k in ("activeRecords","pendingJobs","cleanupPending","retainedCleanup")),"one original five-second physical stop receipt")


def validate_transfer_journey(events,ready,business,pair2,ids,marker,port,stops,partial):
    original=[event for event in events if not event.get("case","").startswith(("r70-","ordinary-cycle2-"))]
    summary=validate_journey(original,ready,business,marker,port)
    require(type(pair2) is dict and set(pair2)=={"status","principalId","role","pairingRequestId","pollAttempts"}
            and pair2["status"]=="PASS" and pair2["principalId"]=="2" and pair2["role"]=="TEACHER_ADMIN"
            and type(pair2["pollAttempts"]) is int and 1<=pair2["pollAttempts"]<=8
            and isinstance(pair2["pairingRequestId"],str) and re.fullmatch(r"[1-9][0-9]{0,19}",pair2["pairingRequestId"]),"second actual teacher pairing receipt")
    require(type(ids) is list and len(ids)==4 and len(set(ids))==4 and all(isinstance(i,str) and re.fullmatch(r"[1-9][0-9]{0,19}",i) for i in ids),"four correlated original/reused upload IDs")
    a,b_id,c,d=ids
    extra=[("r70-wrong-origin","POST","materials/uploads/2/1",403,True,True),
        ("r70-wrong-csrf","POST","materials/uploads/2/1",403,True,True),
        ("r70-wrong-owner","POST","materials/uploads/1/1",403,True,True),
        ("r70-create-a","POST","materials/uploads/2/1",201,True,True),
        ("r70-bytes-a","POST",f"materials/uploads/{a}/bytes",200,True,True),
        ("r70-create-b","POST","materials/uploads/2/1",201,True,True)]
    latter=[("r70-old-cookie-rejected","GET","session",401,True,False),
        ("ordinary-cycle2-private-without-session","GET","session",401,False,False),
        ("ordinary-cycle2-pairing-request","POST","pairing/requests",202,False,False)]
    latter += [(f"ordinary-cycle2-pairing-poll-{i}","POST",f"pairing/requests/{pair2['pairingRequestId']}/poll",202 if i<pair2["pollAttempts"] else 200,False,False) for i in range(1,pair2["pollAttempts"]+1)]
    latter += [("ordinary-cycle2-authenticated-session","GET","session",200,True,False),
        ("r70-create-c","POST","materials/uploads/2/1",201,True,True),("r70-bytes-c","POST",f"materials/uploads/{c}/bytes",200,True,True),
        ("r70-create-d","POST","materials/uploads/2/1",201,True,True),("r70-bytes-d","POST",f"materials/uploads/{d}/bytes",200,True,True),
        ("r70-append-c","POST","materials/folders/2/1/append",200,True,True),("r70-append-c-exact-retry","POST","materials/folders/2/1/append",200,True,True),
        ("r70-material-receipt","GET","materials/commands/69001",200,True,False),("r70-cancel-d","POST",f"materials/uploads/{d}/cancel",200,True,True)]
    wanted=original[:-1]+[x[0] for x in extra]+[original[-1]]+[x[0] for x in latter]
    wanted=[item["case"] if isinstance(item,dict) else item for item in wanted]
    require([x.get("case") for x in events]==wanted,"exact full material-stop wire sequence without omitted/reordered/extra request")
    by_case={x["case"]:x for x in events}
    for case,method,route,status,auth,csrf in extra+latter:
        event=by_case[case]
        require(set(event)=={"case","method","route","httpStatus","requestBodyBytes","responseBodyBytes","authenticatedCookieSent","csrfSent","tls","serverName","cleanTlsEof"}
            and (event["method"],event["route"],event["httpStatus"])==(method,route,status)
            and type(event["httpStatus"]) is int and event["authenticatedCookieSent"] is auth and event["csrfSent"] is csrf
            and event["tls"]=="TLSv1.2" and event["serverName"]=="127.0.0.1" and event["cleanTlsEof"] is True
            and type(event["requestBodyBytes"]) is int and type(event["responseBodyBytes"]) is int
            and 0<event["responseBodyBytes"]<=b.BODY_LIMIT
            and (event["requestBodyBytes"]==0 if method=="GET" else 0<event["requestBodyBytes"]<=b.BODY_LIMIT),"typed complete material request observation")
        if case.startswith("r70-bytes-"):
            require(event["requestBodyBytes"]==len(MATERIAL),"exact complete synthetic binary request length")
    require(type(stops) is list and len(stops)==2,"two original physical stop observations")
    for index,value in enumerate(stops):validate_stop(value,marker,ready[index]["generation"],index+1)
    require(type(partial) is dict and set(partial)=={"status","uploadId","declaredBytes","sentPrefixBytes","peerTermination","successfulUploadResponse","clientClosedBeforeNativeStop"}
            and partial["status"]=="PASS" and partial["uploadId"]==b_id and partial["declaredBytes"]==1048576
            and partial["sentPrefixBytes"]==65536 and partial["peerTermination"] in ("TLS_EOF","TLS_TRUNCATED_BY_STOP","TCP_RESET_BY_STOP")
            and partial["successfulUploadResponse"] is False and partial["clientClosedBeforeNativeStop"] is False,"original partial upload actually retired by native stop")
    return {**summary,"status":"PASS","totalRequests":len(events),"materialReadyStop":"PASS","partialUploadStop":"PASS","reusedSlots":2,"oldCookie":"401","secondNativeApproval":"PASS","materialReceipt":"COMMITTED"}


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
    footer = "ORDINARY_HTTPS_NATIVE_PASS steps=31 shared_workers=2 tls_cycles=2 authenticated_reminder=PASS physical_shutdown=PASS isolated_cleanup=PASS scope=OWNED_LOOPBACK_COMPOSITION material_ready_stop=PASS partial_upload_stop=PASS upload_slots_reused=2 typed_material_receipt=PASS campus=NOT_VALIDATED elapsed_ms=4000"
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
    secret = f"FC_ORDINARY_PAIR_V2\n{marker}\n{17:020}\n1\n00000000\nsynthetic-ordinary-teacher\n"
    require(parse_pairing_secret(secret,marker,17)==("00000000","synthetic-ordinary-teacher"), "pure synthetic secret channel parser")
    for invalid in [secret+"x",secret.replace(f"{17:020}",f"{18:020}"),secret.replace("00000000\nsynthetic", "not-code\nsynthetic"),
                    secret.replace(marker,marker+"x"),secret.replace("FC_ORDINARY_PAIR_V2","unrecognized"),secret.replace("synthetic-ordinary-teacher","foreign")]:
        try:
            parse_pairing_secret(invalid,marker,17)
        except RuntimeError:
            pass
        else:
            raise RuntimeError("foreign/unbounded/malformed secret channel accepted")
    teacher_session = {"schemaVersion":1,"status":"AUTHENTICATED","principal":{"id":"2","displayName":"Synthetic","role":"TEACHER_ADMIN"},
        "currentClassId":"1","assignments":[{"classId":"1","subjectId":"1"}],"csrf":"0"*64}
    validate_teacher_session(teacher_session)
    for invalid in [dict(teacher_session,principal=dict(teacher_session["principal"],role="GLOBAL_ADMIN")),
                    dict(teacher_session,principal=dict(teacher_session["principal"],id="1")),
                    dict(teacher_session,currentClassId=None),dict(teacher_session,assignments=[]),
                    dict(teacher_session,assignments=[{"classId":"2","subjectId":"1"}]),dict(teacher_session,csrf="invalid")]:
        try:
            validate_teacher_session(invalid)
        except RuntimeError:
            pass
        else:
            raise RuntimeError("wrong-role/foreign/unauthorized session accepted")
    def journey_fixture(polls):
        actual_ready = [ready,dict(ready,cycle=2,generation=18)]
        business = {"status":"PASS","session":"ACTUAL_NATIVE_APPROVAL","principalId":"2","role":"TEACHER_ADMIN","classId":"1",
            "commandId":"68001","beforeRevision":"0","committedRevision":"1","readbackRevision":"1","durableReceipt":"COMMITTED",
            "authenticatedGet":"PASS","authenticatedPost":"PASS","secretPersistence":"NONE","pairingRequestId":"7","pollAttempts":polls,"contentDate":"2026-11-01"}
        def public(cycle,generation):
            return {"case":f"ordinary-public-status-cycle-{cycle}","method":"GET","route":"status","cycle":cycle,"generation":generation,
                "httpStatus":200,"bodyBytes":75,"bodySha256":"a"*64,"cleanTlsEof":True,"tls":"TLSv1.2","serverName":"127.0.0.1","cookieSent":False}
        def event(case,method,route,status=200,authenticated=True,csrf=False):
            return {"case":case,"method":method,"route":route,"httpStatus":status,"requestBodyBytes":0 if method=="GET" else 100,
                "responseBodyBytes":100,"authenticatedCookieSent":authenticated,"csrfSent":csrf,"tls":"TLSv1.2","serverName":"127.0.0.1","cleanTlsEof":True}
        actual = [public(1,17),event("ordinary-private-without-session","GET","session",401,False),
            event("ordinary-pairing-request","POST","pairing/requests",202,False)]
        actual += [event(f"ordinary-pairing-poll-{i}","POST","pairing/requests/7/poll",202 if i<polls else 200,False) for i in range(1,polls+1)]
        route="content/reminders/1/2026-11-01"
        actual += [event("ordinary-authenticated-session","GET","session"),event("ordinary-reminder-scopes","GET","content/reminders/scopes/0"),
            event("ordinary-empty-reminder","GET",route),event("ordinary-authenticated-reminder-post","POST",route,csrf=True),
            event("ordinary-authenticated-reminder-readback","GET",route),event("ordinary-durable-reminder-receipt","GET","content/commands/68001"),public(2,18)]
        return actual,actual_ready,business
    for polls in range(1,9):
        actual,actual_ready,business=journey_fixture(polls)
        checked=validate_journey(actual,actual_ready,business,marker,23456)
        require(checked["totalRequests"] == 10+polls, "valid one-to-eight poll operation closure")
    actual,actual_ready,business=journey_fixture(1)
    negatives=[(actual[:i]+actual[i+1:],actual_ready,business) for i in range(len(actual))]
    reordered=copy.deepcopy(actual);reordered[4],reordered[5]=reordered[5],reordered[4]
    negatives += [(reordered,actual_ready,business),(actual+[actual[-1]],actual_ready,business),(actual[:2]+[actual[1]]+actual[2:],actual_ready,business)]
    for index,changes in [(0,{"httpStatus":401}),(2,{"httpStatus":200}),(3,{"httpStatus":202}),(3,{"case":"ordinary-pairing-poll-2"}),
            (3,{"route":"pairing/requests/8/poll"}),(7,{"authenticatedCookieSent":False}),(7,{"csrfSent":False}),(8,{"cleanTlsEof":False}),
            (10,{"generation":17}),(8,{"route":"content/reminders/1/2026-11-02"}),(4,{"httpStatus":202}),(4,{"cookie":"unwanted-secret-field"})]:
        changed=copy.deepcopy(actual);changed[index].update(changes);negatives.append((changed,actual_ready,business))
    for changes in [{"role":"GLOBAL_ADMIN"},{"principalId":"1"},{"readbackRevision":"0"},{"durableReceipt":"NOT_FOUND"},
                    {"contentDate":"2026-02-30"},{"pollAttempts":0},{"pollAttempts":9},{"pollAttempts":True}]:
        negatives.append((actual,actual_ready,dict(business,**changes)))
    multiple,multiple_ready,multiple_business=journey_fixture(3)
    multiple[3]["httpStatus"]=200
    negatives.append((multiple,multiple_ready,multiple_business))
    for invalid_events,invalid_ready,invalid_business in negatives:
        try:
            validate_journey(invalid_events,invalid_ready,invalid_business,marker,23456)
        except RuntimeError:
            pass
        else:
            raise RuntimeError("missing/reordered/extra/wrong-role/status/auth/receipt/poll operation accepted")
    transfer_checks=0
    def rejects(call, label):
        nonlocal transfer_checks
        try:
            call()
        except RuntimeError:
            transfer_checks+=1
        else:
            raise RuntimeError("invalid transfer contract accepted: "+label)
    upload={"schemaVersion":1,"status":"UPLOAD_READY","uploadId":"11","expiresInSeconds":300,"maxChunkBytes":65536}
    validate_upload(upload)
    for change in [{"uploadId":"0"},{"uploadId":"01"},{"uploadId":True},{"uploadId":str(2**64)},
                   {"schemaVersion":True},{"expiresInSeconds":True},{"expiresInSeconds":301},{"maxChunkBytes":65535},
                   {"cookie":"fake-extra-field"},{"status":"COMMITTED"}]:
        rejects(lambda value=dict(upload,**change):validate_upload(value),"upload identity/limits/shape")
    uploaded={"schemaVersion":1,"status":"UPLOADED","uploadId":"11","bytes":65536}
    validate_upload(uploaded,"11",True)
    for change in [{"bytes":True},{"bytes":1048576},{"uploadId":"12"},{"status":"UPLOAD_READY"}]:
        rejects(lambda value=dict(uploaded,**change):validate_upload(value,"11",True),"complete upload receipt")
    record={"materialId":"1","folderId":"1","displayName":"R70-reuse-c.txt","resourceVersionId":"13",
        "kind":"TXT","bytes":65536,"sha256":b.digest(MATERIAL),"allowedClassIds":["1"],"addedAt":"2026-11-01T08:00:00",
        "addedSequence":"1","status":"PUBLISHED","revision":"1"}
    receipt={"schemaVersion":1,"status":"COMMITTED","commandId":"69001","scope":{"ownerTeacherId":"2","subjectId":"1"},"record":record}
    validate_material_receipt(receipt,"13")
    durable=dict(receipt,status="COMMAND_RESULT",state="COMMITTED")
    validate_material_receipt(durable,"13",True)
    for change in [{"scope":{"ownerTeacherId":"1","subjectId":"1"}},{"commandId":"69002"},{"cookie":"fake-extra-field"}]:
        rejects(lambda value=dict(receipt,**change):validate_material_receipt(value,"13"),"typed command scope/identity")
    for change in [{"sha256":"0"*64},{"bytes":True},{"allowedClassIds":["2"]},{"resourceVersionId":"14"},
                   {"revision":str(2**64)},{"status":"DELETED"}]:
        rejects(lambda value=dict(receipt,record=dict(record,**change)):validate_material_receipt(value,"13"),"published record exact payload")
    def transfer_fixture(polls1,polls2):
        events,ready_values,business=journey_fixture(polls1)
        def event(case,method,route,status=200,auth=True,csrf=True):
            return {"case":case,"method":method,"route":route,"httpStatus":status,"requestBodyBytes":0 if method=="GET" else 65536 if case.startswith("r70-bytes-") else 100,
                "responseBodyBytes":100,"authenticatedCookieSent":auth,"csrfSent":csrf,"tls":"TLSv1.2","serverName":"127.0.0.1","cleanTlsEof":True}
        extra=[event("r70-wrong-origin","POST","materials/uploads/2/1",403),event("r70-wrong-csrf","POST","materials/uploads/2/1",403),
            event("r70-wrong-owner","POST","materials/uploads/1/1",403),event("r70-create-a","POST","materials/uploads/2/1",201),
            event("r70-bytes-a","POST","materials/uploads/11/bytes"),event("r70-create-b","POST","materials/uploads/2/1",201)]
        more=[event("r70-old-cookie-rejected","GET","session",401,csrf=False),event("ordinary-cycle2-private-without-session","GET","session",401,False,False),
            event("ordinary-cycle2-pairing-request","POST","pairing/requests",202,False,False)]
        more += [event(f"ordinary-cycle2-pairing-poll-{i}","POST","pairing/requests/9/poll",202 if i<polls2 else 200,False,False) for i in range(1,polls2+1)]
        more += [event("ordinary-cycle2-authenticated-session","GET","session",csrf=False),
            event("r70-create-c","POST","materials/uploads/2/1",201),event("r70-bytes-c","POST","materials/uploads/13/bytes"),
            event("r70-create-d","POST","materials/uploads/2/1",201),event("r70-bytes-d","POST","materials/uploads/14/bytes"),
            event("r70-append-c","POST","materials/folders/2/1/append"),event("r70-append-c-exact-retry","POST","materials/folders/2/1/append"),
            event("r70-material-receipt","GET","materials/commands/69001",csrf=False),event("r70-cancel-d","POST","materials/uploads/14/cancel")]
        pair2={"status":"PASS","principalId":"2","role":"TEACHER_ADMIN","pairingRequestId":"9","pollAttempts":polls2}
        stops=[{"cycle":cycle,"marker":marker,"generation":generation,"elapsedNs":1_000_000,"activeRecords":0,"pendingJobs":0,
            "cleanupPending":0,"retainedCleanup":0,"nativeDrained":True} for cycle,generation in [(1,17),(2,18)]]
        partial={"status":"PASS","uploadId":"12","declaredBytes":1048576,"sentPrefixBytes":65536,"peerTermination":"TLS_TRUNCATED_BY_STOP",
            "successfulUploadResponse":False,"clientClosedBeforeNativeStop":False}
        return [events[:-1]+extra+[events[-1]]+more,ready_values,business,pair2,["11","12","13","14"],marker,23456,stops,partial]
    for polls1,polls2 in [(1,1),(1,8),(8,1),(8,8)]:
        args=transfer_fixture(polls1,polls2)
        validate_transfer_journey(*args)
    args=transfer_fixture(1,1)
    for index in range(len(args[0])):
        bad=copy.deepcopy(args);bad[0].pop(index)
        rejects(lambda bad=bad:validate_transfer_journey(*bad),"missing actual wire operation")
    for change in [{"elapsedNs":5_000_000_001},{"elapsedNs":True},{"generation":18},{"pendingJobs":1},{"nativeDrained":False},
                   {"cookie":"fake-extra-field"}]:
        rejects(lambda value=dict(args[7][0],**change):validate_stop(value,marker,17,1),"old stop budget/owner/release")
    for change in [{"successfulUploadResponse":True},{"clientClosedBeforeNativeStop":True},{"declaredBytes":65536},
                   {"uploadId":"13"},{"peerTermination":"LOCAL_CLOSE"}]:
        bad=copy.deepcopy(args);bad[8].update(change)
        rejects(lambda bad=bad:validate_transfer_journey(*bad),"partial stop evidence")
    for index,changes in [(12,{"httpStatus":200}),(13,{"csrfSent":False}),(-1,{"route":"materials/uploads/13/cancel"}),
                          (-2,{"authenticatedCookieSent":False}),(-3,{"cleanTlsEof":False}),(-4,{"cookie":"fake-extra-field"})]:
        bad=copy.deepcopy(args);bad[0][index].update(changes)
        rejects(lambda bad=bad:validate_transfer_journey(*bad),"wrong route/status/auth/secret field")
    for cycle in (1,2):
        code=f"FC_ORDINARY_PAIR_V2\n{marker}\n{17:020}\n{cycle}\n00000000\nsynthetic-ordinary-teacher\n"
        require(parse_pairing_secret(code,marker,17,cycle)==("00000000","synthetic-ordinary-teacher"),"cycle-bound memory-only secret parser")
        rejects(lambda code=code,cycle=cycle:parse_pairing_secret(code,marker,17,3-cycle),"wrong secret cycle")

    completion_positive=0
    completion_negative=0
    for ids in [["1","2"],[str(2**64-2),str(2**64-1)]]:
        for cycle in (1,2):
            fields=business_done_value(marker,17,cycle,ids).split("\n")
            require(len(fields)==7 and fields[:4]==["FC_ORDINARY_DONE_V2",marker,"00000000000000000017",str(cycle)]
                    and fields[6]=="" and [int(v) for v in fields[4:6]]==[int(v) for v in ids]
                    and all(len(v)==20 and v==f"{int(v):020d}" for v in fields[4:6]),
                    "native u64 completion IDs left-pad after verified integer conversion")
            completion_positive+=1
    malformed=[["0","2"],["01","2"],[True,"2"],["1",2],["1","1"],["1",str(2**64)],
               ["-1","2"],[" 1","2"],["1\n","2"],["1"],["1","2","3"],"12"]
    invalid_args=[(marker,17,1,ids) for ids in malformed]
    invalid_args += [(marker,True,1,["1","2"]),(marker,0,1,["1","2"]),(marker,2**64,1,["1","2"]),
                     (marker,17,True,["1","2"]),(marker,17,3,["1","2"]),(marker+"\n",17,1,["1","2"])]
    for invalid in invalid_args:
        try:
            business_done_value(*invalid)
        except RuntimeError:
            completion_negative+=1
        else:
            raise RuntimeError("invalid completion ID/cycle/marker accepted")

    publication_checks=0
    class PublicationOnly:
        # Source-contract observer only: never executes PowerShell or a socket.
        def __init__(self):
            self.calls=[]
        def powershell(self,script,timeout):
            self.calls.append(script)
            require(0<timeout<=8,"publication uses remaining original deadline")
            return "OWNED_CLIENT_JOURNEY_DONE"
    for cycle in (1,2):
        for directory in [r"C:\Temp\owned-root",r"C:\Temp\owned'quoted-root"]:
            observer=PublicationOnly()
            signal_business_done(observer,directory,marker,17,time.monotonic()+1,cycle,["1","2"])
            require(len(observer.calls)==1,"one original publication command")
            script=observer.calls[0]
            require("$r="+b.ps_literal(directory) in script
                    and b.ps_literal(f"ordinary-business-{cycle}.pending") in script
                    and b.ps_literal(f"ordinary-business-{cycle}.done") in script
                    and "[IO.File]::Open($t,[IO.FileMode]::CreateNew" in script
                    and "[IO.File]::Open($p," not in script
                    and script.index("$s.Write(")<script.index("$s.Flush($true)")<script.index("$s.Dispose()")<script.index("[IO.File]::Move($t,$p)")
                    and script.count("[IO.File]::Move(")==1 and "Move($t,$p," not in script,
                    "same-root pending write/flush/dispose then non-overwriting final publication")
            publication_checks+=1

    unexpected_metadata_checks=0
    class MemoryRaw:
        def __enter__(self):return self
        def __exit__(self,*unused):return False
    class MemoryTls(MemoryRaw):
        def __init__(self,status):
            body=b'{"schemaVersion":1,"code":"SERVICE_UNAVAILABLE","opaque":"RAW_BODY_MUST_NOT_PERSIST"}'
            self.chunks=[f"HTTP/1.1 {status} Fixture\r\nContent-Length: {len(body)}\r\nSet-Cookie: FAKE_RESPONSE_COOKIE_MUST_NOT_PERSIST\r\n\r\n".encode("ascii")+body,b""]
        def getpeercert(self,binary_form):return b"fixture-public-der"
        def version(self):return "TLSv1.2"
        def sendall(self,data):pass
        def settimeout(self,timeout):pass
        def recv(self,limit):return self.chunks.pop(0)
    class MemoryContext:
        def __init__(self,status):self.status=status
        def wrap_socket(self,*unused,**options):return MemoryTls(self.status)
    original_connect=socket.create_connection
    try:
        socket.create_connection=lambda *unused,**options:MemoryRaw()
        for status in (400,403,409,429,503):
            observed=[]
            client=OrdinaryClient(1,2,MemoryContext(status),b"fixture-public-der",time.monotonic()+1,observed)
            client.cookie="FAKE_REQUEST_COOKIE_MUST_NOT_PERSIST"
            client.csrf="FAKE_CSRF_MUST_NOT_PERSIST"
            try:
                client.request("fixture-unexpected-append","POST","materials/folders/2/1/append",{"schemaVersion":1})
            except RuntimeError as error:
                require(str(error)=="ordinary business unexpected HTTP status","original unexpected-status assertion remains strict")
            else:
                raise RuntimeError("unexpected HTTP status was accepted")
            require(len(observed)==1 and observed[0]["httpStatus"]==status
                    and observed[0]["case"]=="fixture-unexpected-append" and observed[0]["method"]=="POST"
                    and observed[0]["route"]=="materials/folders/2/1/append" and observed[0]["cleanTlsEof"] is True
                    and not any(secret in json.dumps(observed) for secret in
                        ("RAW_BODY_MUST_NOT_PERSIST","FAKE_REQUEST_COOKIE_MUST_NOT_PERSIST","FAKE_RESPONSE_COOKIE_MUST_NOT_PERSIST","FAKE_CSRF_MUST_NOT_PERSIST")),
                    "safe unexpected status metadata retained without credential/header/body persistence")
            unexpected_metadata_checks+=1
    finally:
        socket.create_connection=original_connect

    error_code_checks=0
    for code in ERROR_CODES:
        require(safe_error_code(b.json_bytes({"schemaVersion":1,"code":code}))==code,
                "exact typed error enum disclosure")
        error_code_checks+=1
    invalid_errors=[b'{"schemaVersion":true,"code":"INVALID_RESOURCE"}',
        b'{"schemaVersion":2,"code":"INVALID_RESOURCE"}', b'{"schemaVersion":1,"code":42}',
        b'{"schemaVersion":1,"code":"FAKE_AUTH_SECRET_MUST_NOT_PERSIST"}',
        b'{"schemaVersion":1,"code":"invalid_resource"}', b'{"schemaVersion":1,"code":"INVALID_RESOURCE","cookie":"FAKE_COOKIE"}',
        b'{"schemaVersion":1,"code":"INVALID_RESOURCE","code":"FORBIDDEN"}',
        b'{"schemaVersion":1,"schemaVersion":1,"code":"INVALID_RESOURCE"}',
        b'{"schemaVersion":1,"code":["INVALID_RESOURCE"]}',b'{"code":"INVALID_RESOURCE"}',
        b'[]',b'null',b'not-json',b'\xff',b' '*513,"INVALID_RESOURCE"]
    for value in invalid_errors:
        require(safe_error_code(value) is None,"unknown/duplicate/extra/secret/malformed error DTO has no disclosure")
        error_code_checks+=1

    print(json.dumps({"status": "PASS", "scope": "pure validators only", "checks": 41+8+len(negatives)+transfer_checks+10+completion_positive+completion_negative+publication_checks+unexpected_metadata_checks+error_code_checks,
                      "wireJourneyPositiveCases":8,"wireJourneyNegativeCases":len(negatives),
                      "transferJourneyPositiveCases":4,"transferTypedAndCyclePositiveCases":6,"transferNegativeCases":transfer_checks,
                      "completionIdPositiveCases":completion_positive,"completionIdNegativeCases":completion_negative,
                      "publicationSourceContractCases":publication_checks,
                      "unexpectedStatusSafeMetadataCases":unexpected_metadata_checks,
                      "closedErrorDtoCases":error_code_checks,
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
        "campus": "NOT_VALIDATED", "win7": "NOT_RUN", "win10": "NOT_RUN", "materialReadyStop": "NOT_RUN",
        "productionSchoolSettings": "UNCHANGED", "runnerSourceSha256": b.digest(Path(__file__).read_bytes()),
        "ownershipSourceSha256": b.digest(Path(__file__).with_name("windows-tls-probe.py").read_bytes()),
        "fixtureSetupSourceSha256": b.digest(tls.SETUP.encode("utf-8")),
        "businessHelperSourceSha256": b.digest(Path(__file__).with_name("windows-tls-business-probe.py").read_bytes()),
        "exeSha256": b.digest(executable), "events": []}
    uploaded, directory, launcher, logs, tunnel = set(), None, None, None, None
    partial = old_client = old_credentials = None
    first_ids = second_ids = None
    stops = []
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
        lines, ready, exits, client_deadline, paired = [], [], [], None, []
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
                report["events"].append(status_get(local_port,port,context,expected_der,client_deadline,value["cycle"],value["generation"]))
                if value["cycle"] == 2:
                    require(len(stops)==1 and old_client is not None and first_ids is not None,"first actual stop precedes second generation")
                    require(stage_sizes(remote,directory,marker,args.exe.name,first_ids,client_deadline)==[-1,-1],"both original stages absent before replacement upload")
                    old_client.local_port=local_port
                    denied=old_client.request("r70-old-cookie-rejected","GET","session",expected=(401,)).json()
                    require(denied=={"schemaVersion":1,"code":"UNAUTHORIZED"},"actual old-generation cookie denied")
                    old_credentials=(old_client.cookie,old_client.csrf)
                    old_client.cookie=old_client.csrf=None
                tunnel.cancel()
            elif line.startswith("ORDINARY_HTTPS_STOPPED "):
                value=json.loads(line.removeprefix("ORDINARY_HTTPS_STOPPED "))
                cycle=len(stops)+1
                require(cycle<=2 and len(ready)==cycle,"exact physical stop order")
                validate_stop(value,marker,ready[cycle-1]["generation"],cycle)
                stops.append(value)
                if cycle==1:
                    require(partial is not None,"actual partial socket still owned at native stop")
                    report["partialUploadStop"]=partial.peer_stop(client_deadline)
                    partial.close();partial=None
                    tunnel.cancel()
            elif line.startswith("ORDINARY_HTTPS_PAIR_READY "):
                cycle=len(ready)
                require(cycle in (1,2) and len(paired)==cycle-1 and client_deadline is not None,"two ordered actual native pairing rendezvous")
                value=json.loads(line.removeprefix("ORDINARY_HTTPS_PAIR_READY "))
                generation=ready[cycle-1]["generation"]
                validate_pair_ready(value,marker,port,generation,cycle)
                code,login=read_pairing_secret(remote,directory,marker,generation,client_deadline,cycle)
                local_port=tunnel.open(port)
                client=OrdinaryClient(local_port,port,context,expected_der,client_deadline,report["events"])
                business=authenticate_and_write(client,code,login,cycle)
                del code,login
                if cycle==1:
                    report["authenticatedBusiness"]=business
                    first_ids,partial,report["materialBeforeStop"]=first_materials(client,remote,directory,marker,args.exe.name)
                    old_client=client
                    signal_business_done(remote,directory,marker,generation,client_deadline,cycle,first_ids)
                    # Preserve this exact forwarding domain and partial socket;
                    # the native Disable, not the client, must terminate it.
                else:
                    require(old_credentials is not None and client.cookie!=old_credentials[0]
                            and client.csrf!=old_credentials[1],"fresh second-generation authentication secrets")
                    old_credentials=None
                    report["secondPairing"]=business
                    second_ids,report["materialReuse"]=second_materials(client,first_ids)
                    client.cookie=client.csrf=None
                    tunnel.cancel()
                    signal_business_done(remote,directory,marker,generation,client_deadline,cycle,second_ids)
                paired.append(cycle)
            elif line.startswith(tls.EXIT):exits.append(json.loads(line[len(tls.EXIT):]))
        launcher.wait(timeout=max(.01,until-time.monotonic()));logs.join()
        report["transcript"]=validate_transcript(lines)
        require(launcher.returncode==0 and len(exits)==1 and exits[0].get("code")==0,"actual original native and supervisor successful exit")
        require(paired==[1,2] and first_ids is not None and second_ids is not None,"both actual native approvals and transfer cycles completed")
        report["wireJourney"]=validate_transfer_journey(report["events"],ready,report["authenticatedBusiness"],
            report["secondPairing"],first_ids+second_ids,marker,port,stops,report.get("partialUploadStop"))
        report["materialReadyStop"]="PASS"
        report["stops"]=stops
        report.update(status="PASS",ready=ready,nativeExit=exits[0])
    except Exception as error:
        report["error"]=tls.failure(error,"ordinary-https-composition")
    finally:
        if partial is not None:
            partial.close()
        old_credentials=None
        if old_client is not None:
            old_client.cookie=old_client.csrf=None
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
                     +"foreach($cycle in @(1,2)){$p=Join-Path $r ('ordinary-pairing-'+$cycle+'.secret');if(Test-Path -LiteralPath $p){Remove-Item -LiteralPath $p};"
                    +"if(Test-Path -LiteralPath $p){throw 'owned secret remains'}};'PASS'",timeout=8)
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
