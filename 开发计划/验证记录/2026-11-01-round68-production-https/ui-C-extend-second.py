from pathlib import Path
w=Path('/tmp/fc-round68-production-https/worktree')
p=w/'crates/fc-bootstrap/src/application/ordinary_https_probe.rs';s=p.read_text()
s=s.replace('''    println!("ORDINARY_HTTPS_PAIR_READY {{\\"cycle\\":1,\\"marker\\":\\"{marker}\\",\\"port\\":{port},\\"generation\\":{first},\\"origin\\":\\"https://127.0.0.1:{port}\\",\\"pairingCode\\":\\"{}\\",\\"loginName\\":\\"{LOGIN_NAME}\\"}}", code.as_str());
    drop(code);''','''    let secret = completion.with_file_name("ordinary-pairing-first.secret");
    {
        use std::io::Write;
        let mut output = fs::OpenOptions::new().write(true).create_new(true).open(&secret)
            .map_err(|_| "fresh owned pairing sidechannel unavailable")?;
        write!(output, "FC_ORDINARY_PAIR_V1\\n{marker}\\n{first:020}\\n{}\\n{LOGIN_NAME}\\n", code.as_str())
            .map_err(|_| "owned pairing sidechannel write failed")?;
    }
    drop(code);
    // Secret values never enter the native transcript or archived report. The
    // external client consumes/deletes the owned sidechannel into memory only.
    println!("ORDINARY_HTTPS_PAIR_READY {{\\"cycle\\":1,\\"marker\\":\\"{marker}\\",\\"port\\":{port},\\"generation\\":{first},\\"origin\\":\\"https://127.0.0.1:{port}\\"}}");''')
s=s.replace('''    loop {
        pump(app, deadline)?;
        match fs::read(completion) {
            Ok(value) => {
                require(value == marker.as_bytes(), "exact owned business completion marker")?;
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("owned business completion marker read failed".into()),
        }''','''    let expected = format!("FC_ORDINARY_DONE_V1\\n{marker}\\n{first:020}\\n");
    loop {
        pump(app, deadline)?;
        match fs::symlink_metadata(completion) {
            Ok(metadata) => {
                require(metadata.file_type().is_file() && metadata.len() == expected.len() as u64,
                    "bounded regular completion marker")?;
                let value = fs::read(completion).map_err(|_| "completion marker read failed")?;
                require(value == expected.as_bytes(), "exact owned marker and generation")?;
                require(Instant::now() < deadline, "completion before original deadline")?;
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("owned business completion marker read failed".into()),
        }''')
p.write_text(s)
p=w/'scripts/windows-ordinary-https-probe.py';s=p.read_text()
s=s.replace('TLS status GETs.', 'TLS status GETs plus an actual approved teacher session and reminder POST/GET.')
s=s.replace('    "setup-local-global-administrator", "native-teacher-page-default-off",','    "setup-local-global-administrator", "native-create-synthetic-class", "native-teacher-page-default-off",')
s=s.replace('    "native-pairing-open-close-and-disable-review-cancel", "native-confirm-first-disable",','    "native-open-actual-runtime-pairing-window", "native-approve-actual-network-pairing-request",\n    "first-real-authenticated-reminder-post-and-get", "native-pairing-close-and-disable-review-cancel", "native-confirm-first-disable",')
s=s.replace('steps=20 shared_workers=2 tls_cycles=2 physical_shutdown=PASS','steps=24 shared_workers=2 tls_cycles=2 authenticated_reminder=PASS physical_shutdown=PASS')
s=s.replace('ordinary exact twenty steps','ordinary exact twenty-four steps')
s=s.replace('"steps": len(steps), "physicalShutdown": "PASS",','"steps": len(steps), "authenticatedReminder": "PASS", "physicalShutdown": "PASS",')
insert='''

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
    fields = text.split("\\n")
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
                connection.sendall(("\\r\\n".join(headers)+"\\r\\n\\r\\n").encode("ascii")+payload)
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
    value = f"FC_ORDINARY_DONE_V1\\n{marker}\\n{generation:020}\\n"
    script = ("$r="+b.ps_literal(directory)+";$m="+b.ps_literal(marker)
        + ";if((Split-Path -Leaf $r)-ne $m-or [IO.File]::ReadAllText((Join-Path $r '.tls-probe-root'))-ne $m){throw 'owned completion mismatch'};"
        + "$p=Join-Path $r 'ordinary-business-first.done';$s=[IO.File]::Open($p,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::None);"
        + "try{$v=[Text.Encoding]::ASCII.GetBytes("+b.ps_literal(value)+");$s.Write($v,0,$v.Length)}finally{$s.Dispose()};'OWNED_CLIENT_JOURNEY_DONE'")
    require(remote.powershell(script, timeout=min(8,deadline-time.monotonic())) == "OWNED_CLIENT_JOURNEY_DONE", "owned completion creation")
'''
s=s.replace('\n\ndef self_test():',insert+'\n\ndef self_test():',1)
s=s.replace('"ownershipSourceSha256": b.digest(Path(__file__).with_name("windows-tls-probe.py").read_bytes()),','"ownershipSourceSha256": b.digest(Path(__file__).with_name("windows-tls-probe.py").read_bytes()),\n        "fixtureSetupSourceSha256": b.digest(tls.SETUP.encode("utf-8")),\n        "businessHelperSourceSha256": b.digest(Path(__file__).with_name("windows-tls-business-probe.py").read_bytes()),')
s=s.replace('        lines, ready, exits, client_deadline = [], [], [], None','        lines, ready, exits, client_deadline, paired = [], [], [], None, False')
s=s.replace('                tunnel.cancel()\n            elif line.startswith(tls.EXIT):','''                tunnel.cancel()
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
            elif line.startswith(tls.EXIT):''')
s=s.replace('require(len(ready)==2 and len(report["events"])==2,"two actual TLS generations observed")','require(len(ready)==2 and paired and len(report["events"])>=12,"two actual TLS generations and actual authenticated business observed")')
s=s.replace('        if directory:b.cleanup_owned(','''        if directory:
            try:
                report["pairingSecretCleanup"]=remote.powershell("$r="+b.ps_literal(directory)+";$m="+b.ps_literal(marker)
                    +";if([IO.File]::ReadAllText((Join-Path $r '.tls-probe-root'))-ne $m){throw 'owned secret cleanup mismatch'};"
                    +"$p=Join-Path $r 'ordinary-pairing-first.secret';if(Test-Path -LiteralPath $p){Remove-Item -LiteralPath $p};"
                    +"if(Test-Path -LiteralPath $p){throw 'owned secret remains'};'PASS'",timeout=8)
            except Exception:
                report.update(status="FAIL",pairingSecretCleanup="FAIL")
        if directory:b.cleanup_owned(''')
p.write_text(s)
