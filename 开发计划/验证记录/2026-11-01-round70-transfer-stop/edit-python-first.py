from pathlib import Path
p=Path('/tmp/fc-round70-ordinary-transfer-stop/worktree/scripts/windows-ordinary-https-probe.py');s=p.read_text()
s=s.replace('MaterialReady, campus reachability and Win7/Win10 runtime remain separate tests.','MaterialReady/partial upload stops use the ordinary owner; campus and Win7/Win10 remain separate tests.')
s=s.replace('    "first-real-authenticated-reminder-post-and-get",','    "first-real-authenticated-reminder-post-and-get", "first-material-ready-and-partial-upload-established",')
s=s.replace('    "physical-first-domain-drained",','    "physical-first-domain-drained", "first-two-staging-files-removed",')
s=s.replace('    "native-enable-second-generation", "second-real-tls-response-complete",','''    "native-enable-second-generation", "second-real-tls-response-complete",
    "native-open-second-runtime-pairing-window", "native-approve-second-network-pairing-request",
    "second-old-cookie-rejected-and-two-upload-slots-reused", "second-typed-material-command-receipt-and-cancel",''')
s=s.replace('steps=25','steps=31').replace('material_ready_stop=NOT_EXERCISED campus=NOT_VALIDATED','material_ready_stop=PASS partial_upload_stop=PASS upload_slots_reused=2 typed_material_receipt=PASS campus=NOT_VALIDATED')
s=s.replace('"materialReadyStop": "NOT_EXERCISED"','"materialReadyStop": "PASS"') # transcript only; initial report reset below
s=s.replace('def validate_pair_ready(value, marker, port, generation):','def validate_pair_ready(value, marker, port, generation, cycle=1):').replace('type(value["cycle"]) is int and value["cycle"] == 1','type(value["cycle"]) is int and value["cycle"] == cycle')
s=s.replace('def read_pairing_secret(remote, directory, marker, generation, deadline):','def read_pairing_secret(remote, directory, marker, generation, deadline, cycle=1):').replace('''+ "$p=Join-Path $r 'ordinary-pairing-first.secret';$f=Get-Item -LiteralPath $p;"''','''+ "$p=Join-Path $r "+b.ps_literal(f"ordinary-pairing-{cycle}.secret")+";$f=Get-Item -LiteralPath $p;"''').replace('return parse_pairing_secret(text, marker, generation)','return parse_pairing_secret(text, marker, generation, cycle)')
s=s.replace('def parse_pairing_secret(text, marker, generation):','def parse_pairing_secret(text, marker, generation, cycle=1):')
start=s.index('    require(len(fields) == 6 and fields[0] == "FC_ORDINARY_PAIR_V1"');end=s.index('\n\n\nclass OrdinaryClient',start)
s=s[:start]+'''    require(type(cycle) is int and cycle in (1,2) and len(fields) == 7
            and fields[0] == "FC_ORDINARY_PAIR_V2" and fields[1] == marker
            and fields[2] == f"{generation:020}" and fields[3] == str(cycle)
            and re.fullmatch(r"[0-9]{8}", fields[4])
            and fields[5] == "synthetic-ordinary-teacher" and fields[6] == "", "owned pairing channel exact shape")
    return fields[4], fields[5]'''+s[end:]
s=s.replace('def request(self, case, method, route, body=None, expected=(200,), authenticated=True):','def request(self, case, method, route, body=None, expected=(200,), authenticated=True,\n                content_type="application/json", origin=None, csrf_override=None):')
s=s.replace('payload = b"" if body is None else b.json_bytes(body)','payload = b"" if body is None else body if isinstance(body, bytes) else b.json_bytes(body)')
s=s.replace('f"Origin: https://127.0.0.1:{self.origin_port}", "Connection: close"]','"Origin: " + (origin if origin is not None else f"https://127.0.0.1:{self.origin_port}"), "Connection: close"]')
s=s.replace('headers += ["Content-Type: application/json", f"Content-Length: {len(payload)}"]','headers += ["Content-Type: " + content_type, f"Content-Length: {len(payload)}"]')
s=s.replace('headers.append("X-FC-CSRF: " + self.csrf)','headers.append("X-FC-CSRF: " + (self.csrf if csrf_override is None else csrf_override))')
s=s.replace('def authenticate_and_write(client, code, login):','def authenticate_and_write(client, code, login, cycle=1):\n    prefix = "ordinary-" if cycle == 1 else "ordinary-cycle2-"')
for name in ['private-without-session','pairing-request','pairing-poll-','authenticated-session']:
 s=s.replace('"ordinary-'+name+'"','prefix+"'+name+'"',1)
needle='    client.csrf = session["csrf"]\n';assert s.count(needle)==1
s=s.replace(needle,needle+'''    if cycle == 2:
        return {"status":"PASS", "principalId":"2", "role":"TEACHER_ADMIN",
                "pairingRequestId":request_id, "pollAttempts":index+1}
''')
s=s.replace('    client.cookie = client.csrf = None\n','')
s=s.replace('def signal_business_done(remote, directory, marker, generation, deadline):','def signal_business_done(remote, directory, marker, generation, deadline, cycle, ids):')
s=s.replace('value = f"FC_ORDINARY_DONE_V1\\n{marker}\\n{generation:020}\\n"','value = f"FC_ORDINARY_DONE_V2\\n{marker}\\n{generation:020}\\n{cycle}\\n{ids[0]:020}\\n{ids[1]:020}\\n"')
s=s.replace('''+ "$p=Join-Path $r 'ordinary-business-first.done';$s=[IO.File]::Open($p,''','''+ "$p=Join-Path $r "+b.ps_literal(f"ordinary-business-{cycle}.done")+";$s=[IO.File]::Open($p,''')
# Pure old first-cycle parser fixtures now use new explicitly cycle-bound format.
s=s.replace('FC_ORDINARY_PAIR_V1\\n{marker}\\n{17:020}\\n00000000','FC_ORDINARY_PAIR_V2\\n{marker}\\n{17:020}\\n1\\n00000000').replace('"FC_ORDINARY_PAIR_V1","unrecognized"','"FC_ORDINARY_PAIR_V2","unrecognized"')
s=s.replace('"materialReadyStop": "PASS",\n        "productionSchoolSettings"','"materialReadyStop": "NOT_RUN",\n        "productionSchoolSettings"')
p.write_text(s)
