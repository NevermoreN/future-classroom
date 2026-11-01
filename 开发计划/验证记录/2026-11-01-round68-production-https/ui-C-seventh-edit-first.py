from pathlib import Path
import json,hashlib
r=Path('/tmp/fc-round68-production-https');w=r/'worktree';e=r/'evidence';rel='scripts/windows-ordinary-https-probe.py';p=w/rel;old=json.loads((e/'ui-C-source-freeze-sixth.json').read_text())
for row in old['files']:
 b=(w/row['relativePath']).read_bytes();assert (len(b),hashlib.sha256(b).hexdigest())==(row['after']['bytes'],row['after']['sha256']),row['relativePath']
before=p.read_bytes();dest=e/'ui-C-source-seventh-before'/rel;dest.parent.mkdir(parents=True,exist_ok=True)
with dest.open('xb') as f:f.write(before)
s=before.decode();s=s.replace('import argparse\n','import argparse\nimport copy\nfrom datetime import date\n',1)
s=s.replace('def status_get(local_port, origin_port, context, expected_der, deadline):','def status_get(local_port, origin_port, context, expected_der, deadline, cycle, generation):',1)
s=s.replace('''    return {"httpStatus": parser.status, "bodyBytes": parser.body_bytes,''','''    return {"case": f"ordinary-public-status-cycle-{cycle}", "method": "GET", "route": "status",
            "cycle": cycle, "generation": generation, "httpStatus": parser.status, "bodyBytes": parser.body_bytes,''',1)
s=s.replace('''            "authenticatedGet": "PASS", "authenticatedPost": "PASS", "secretPersistence": "NONE"}''','''            "authenticatedGet": "PASS", "authenticatedPost": "PASS", "secretPersistence": "NONE",
            "pairingRequestId": request_id, "pollAttempts": index+1, "contentDate": today}''',1)
needle='''def signal_business_done(remote, directory, marker, generation, deadline):'''
insert='''def validate_journey(events, ready, business, marker, port):
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


'''
assert needle in s;s=s.replace(needle,insert+needle,1)
s=s.replace('''status_get(local_port,port,context,expected_der,client_deadline)''','''status_get(local_port,port,context,expected_der,client_deadline,value["cycle"],value["generation"])''',1)
s=s.replace('''        require(len(ready)==2 and paired and len(report["events"])>=12,"two actual TLS generations and actual authenticated business observed")''','''        require(paired,"actual native approval and authenticated business completed")
        report["wireJourney"]=validate_journey(report["events"],ready,report["authenticatedBusiness"],marker,port)''',1)
needle='''    print(json.dumps({"status": "PASS", "scope": "pure validators only", "checks": 41,'''
insert='''    def journey_fixture(polls):
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
    print(json.dumps({"status": "PASS", "scope": "pure validators only", "checks": 41+8+len(negatives),
                      "wireJourneyPositiveCases":8,"wireJourneyNegativeCases":len(negatives),'''
assert needle in s;s=s.replace(needle,insert,1)
p.write_text(s)
