from pathlib import Path
p=Path('/tmp/fc-round70-ordinary-transfer-stop/worktree/scripts/windows-ordinary-https-probe.py');s=p.read_text()
s=s.replace('uploaded, directory, launcher, logs, tunnel = set(), None, None, None, None','uploaded, directory, launcher, logs, tunnel = set(), None, None, None, None\n    partial = old_client = None\n    first_ids = second_ids = None\n    stops = []')
s=s.replace('lines, ready, exits, client_deadline, paired = [], [], [], None, False','lines, ready, exits, client_deadline, paired = [], [], [], None, []')
needle='''                tunnel.cancel()
            elif line.startswith("ORDINARY_HTTPS_PAIR_READY "):''';assert s.count(needle)==1
s=s.replace(needle,'''                if value["cycle"] == 2:
                    require(len(stops)==1 and old_client is not None and first_ids is not None,"first actual stop precedes second generation")
                    require(stage_sizes(remote,directory,marker,args.exe.name,first_ids,client_deadline)==[-1,-1],"both original stages absent before replacement upload")
                    old_client.local_port=local_port
                    denied=old_client.request("r70-old-cookie-rejected","GET","session",expected=(401,)).json()
                    require(denied=={"schemaVersion":1,"code":"UNAUTHORIZED"},"actual old-generation cookie denied")
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
            elif line.startswith("ORDINARY_HTTPS_PAIR_READY "):''')
start=s.index('                require(len(ready) == 1 and not paired');end=s.index('            elif line.startswith(tls.EXIT)',start)
s=s[:start]+'''                cycle=len(ready)
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
                    report["secondPairing"]=business
                    second_ids,report["materialReuse"]=second_materials(client,first_ids)
                    client.cookie=client.csrf=None
                    tunnel.cancel()
                    signal_business_done(remote,directory,marker,generation,client_deadline,cycle,second_ids)
                paired.append(cycle)
''' +s[end:]
s=s.replace('''        require(paired,"actual native approval and authenticated business completed")
        report["wireJourney"]=validate_journey(report["events"],ready,report["authenticatedBusiness"],marker,port)''','''        require(paired==[1,2] and first_ids is not None and second_ids is not None,"both actual native approvals and transfer cycles completed")
        report["wireJourney"]=validate_transfer_journey(report["events"],ready,report["authenticatedBusiness"],
            report["secondPairing"],first_ids+second_ids,marker,port,stops,report.get("partialUploadStop"))
        report["materialReadyStop"]="PASS"
        report["stops"]=stops''')
s=s.replace('''    finally:
        if launcher:''','''    finally:
        if partial is not None:
            partial.close()
        if old_client is not None:
            old_client.cookie=old_client.csrf=None
        if launcher:''')
old='''+"$p=Join-Path $r 'ordinary-pairing-first.secret';if(Test-Path -LiteralPath $p){Remove-Item -LiteralPath $p};"
                    +"if(Test-Path -LiteralPath $p){throw 'owned secret remains'};'PASS'"''';assert s.count(old)==1
s=s.replace(old,''' +"foreach($cycle in @(1,2)){$p=Join-Path $r ('ordinary-pairing-'+$cycle+'.secret');if(Test-Path -LiteralPath $p){Remove-Item -LiteralPath $p};"
                    +"if(Test-Path -LiteralPath $p){throw 'owned secret remains'}};'PASS'"''')
p.write_text(s)
