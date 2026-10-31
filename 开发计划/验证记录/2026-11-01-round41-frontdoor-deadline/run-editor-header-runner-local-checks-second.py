from pathlib import Path
import ast,base64,hashlib,json,subprocess,sys,datetime,difflib
EV=Path('/tmp/fc-round41-frontdoor-deadline-evidence');SOURCE=Path('/tmp/fc-round41-frontdoor-deadline-worktree/scripts/windows-tls-business-probe.py');BEFORE=EV/'windows-tls-business-probe.before-drip-validation.py';OUT=EV/'editor-header-runner-local-checks-second.json';assert not OUT.exists()
sha=lambda b:hashlib.sha256(b).hexdigest();binding={'beforeSha256':sha(BEFORE.read_bytes()),'afterSha256':sha(SOURCE.read_bytes())}
r={'status':'RUNNING','scope':'One full pure local self-test of second source; existing cases retained plus actual-drip/case identity model negatives. CLI/help only. No TLS, Windows, sockets, Cargo, Git or Rust edits.','recordedAtUtc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'binding':binding,'cases':[]}
def call(name,args,expected):
 result=subprocess.run(args,capture_output=True,timeout=15)
 row={'name':name,'arguments':args,'actualExitCode':result.returncode,'expectedExitCode':expected,'streams':{}}
 for stream,raw in [('stdout',result.stdout),('stderr',result.stderr)]:
  p=EV/(name+'.'+stream+'.log');assert not p.exists();p.write_bytes(raw);row['streams'][stream]={'path':str(p),'bytes':len(raw),'sha256':sha(raw),'base64':base64.b64encode(raw).decode()}
 r['cases'].append(row);assert result.returncode==expected,(name,result.returncode,result.stderr.decode(errors='replace'))
 return result
try:
 result=call('editor-header-current-selftest-second',[sys.executable,'-B',str(SOURCE),'--self-test'],0)
 current=json.loads(result.stdout);prior=json.loads((EV/'editor-header-current-selftest-final.stdout.log').read_text())
 assert current['status']==prior['status']=='PURE_LOCAL_CHECKS_PASS'
 assert all(x in current['checks'] for x in prior['checks']) and len(current['checks'])==len(prior['checks'])+1
 for i,flags in enumerate([('--self-test',),('--browser',),('--continuous-accept',),('--voice-capabilities',),('--visual-only',),('--visual-fixture','OUTLINED'),('--browser-content','REMINDER'),()],1):
  call('editor-header-cli-second-'+str(i),[sys.executable,'-B',str(SOURCE),'--header-deadline-only',*flags],2)
 call('editor-header-cli-help-second',[sys.executable,'-B',str(SOURCE),'--help'],0)
 def defs(p):return {x.name:ast.dump(x,include_attributes=False) for x in ast.parse(p.read_bytes()).body if isinstance(x,(ast.FunctionDef,ast.ClassDef))}
 a,b=defs(BEFORE),defs(SOURCE);changed=[k for k in a if a[k]!=b[k]];assert set(changed)=={'validate_header_sample','exercise_header_deadlines','validate_header_native','self_test_header_deadlines'}
 assert [k for k in b if k not in a]==['validate_header_cases']
 assert sha(SOURCE.read_bytes())==binding['afterSha256']
 r.update(status='PASS_PURE_LOCAL_WINDOWS_NOT_RUN',previousGroups=21,currentGroups=22,allPreviousGroupsRetained=True,fullSelftestExecutionsForThisSource=1,cliNegativeCases=8,changedExistingDefinitions=changed,newDefinition='validate_header_cases',originalNativeExactFiveCountsUnchanged=True,defaultMainAndExistingBusinessDefinitionsAstUnchanged=True)
except BaseException as error:
 r.update(status='FAIL',errorType=type(error).__name__,error=str(error));raise
finally:OUT.write_text(json.dumps(r,ensure_ascii=False,indent=2)+'\n')
print(json.dumps({'path':str(OUT),'bytes':OUT.stat().st_size,'sha256':sha(OUT.read_bytes()),'status':r['status'],'groups':r.get('currentGroups')}))
