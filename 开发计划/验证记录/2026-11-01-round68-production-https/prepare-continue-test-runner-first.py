from pathlib import Path
import ast,difflib,json,hashlib,datetime
root=Path('/tmp/fc-round68-production-https');e=root/'evidence';main=Path('/workspace/共享/Eliora/未来教室计划/scripts/windows-probe.py')
s=main.read_text();before=s
needle='''            if result["exitCode"] != 0:
                raise RuntimeError("remote test failed: " + path.name)'''
assert s.count(needle)==1
s=s.replace(needle,'''            if type(result["exitCode"]) is not int:
                raise RuntimeError("invalid remote test exit code: " + path.name)
            if result["exitCode"] != 0:
                report.setdefault("failedTestBinaries", []).append({
                    "name": path.name, "exitCode": result["exitCode"],
                    "binarySha256": result["binarySha256"],
                })''')
needle='''        report["status"] = "PASS"
    except Exception as error:''';assert s.count(needle)==1
s=s.replace(needle,'''        if report.get("failedTestBinaries"):
            raise RuntimeError("remote test binary failures retained after remaining harnesses and native checks; overall suite FAIL")
        report["status"] = "PASS"
    except Exception as error:''')
assert s.split('    finally:\n',1)[1]==before.split('    finally:\n',1)[1]
ast.parse(s)
for name,data in [('windows-probe-original-main-first.py',before),('windows-probe-continue-tests-private-first.py',s),('windows-probe-continue-tests-minimal-first.diff',''.join(difflib.unified_diff(before.splitlines(True),s.splitlines(True),fromfile='main/scripts/windows-probe.py',tofile='evidence/windows-probe-continue-tests-private-first.py')))]:
 with (e/name).open('x') as f:f.write(data)
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
report={'status':'PREPARED_AST_PASS_NOT_EXECUTED','atUtc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'original':{'path':str(main),'bytes':main.stat().st_size,'sha256':sha(main)},'privateCopy':{'path':'windows-probe-continue-tests-private-first.py','bytes':len(s.encode()),'sha256':sha(e/'windows-probe-continue-tests-private-first.py')},'minimalDiff':{'path':'windows-probe-continue-tests-minimal-first.diff','bytes':(e/'windows-probe-continue-tests-minimal-first.diff').stat().st_size,'sha256':sha(e/'windows-probe-continue-tests-minimal-first.diff')},'unchangedCleanupFinallyBytes':True,'nonzeroHarnessExit':'record original failure and continue next original harness; final suite remains FAIL','unsafeExceptions':'unchanged fail-fast timeout/digest/transport/parse/other exceptions','testThreads':2,'testTimeoutSeconds':900,'nativeFlags':['--native-seconds','1','--system-host','--public-desktop'],'windowsExecuted':False,'sourceTreeEdited':False,'cargoInvoked':False,'testsFilteredOrSkipped':False}
with (e/'continue-test-runner-preparation-first.json').open('x') as f:json.dump(report,f,ensure_ascii=False,indent=2);f.write('\n')
print(json.dumps({'status':report['status'],'privateSha256':report['privateCopy']['sha256'],'diffBytes':report['minimalDiff']['bytes']}))
