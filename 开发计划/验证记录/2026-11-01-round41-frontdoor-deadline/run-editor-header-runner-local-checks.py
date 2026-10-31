from pathlib import Path
import ast,base64,hashlib,json,subprocess,sys,datetime
EV=Path('/tmp/fc-round41-frontdoor-deadline-evidence');SOURCE=Path('/tmp/fc-round41-frontdoor-deadline-worktree/scripts/windows-tls-business-probe.py');BEFORE=EV/'windows-tls-business-probe.before-header-deadline.py';OUT=EV/'editor-header-runner-local-checks-first.json';assert not OUT.exists()
sha=lambda b:hashlib.sha256(b).hexdigest()
binding={'beforeSha256':sha(BEFORE.read_bytes()),'afterSha256':sha(SOURCE.read_bytes())}
r={'status':'RUNNING','scope':'Pure host Python validators/CLI/mocked SSH only; no Windows, sockets, Cargo or Git. Old original source self-test reexecuted now, not attributed to a historical run.','recordedAtUtc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'binding':binding,'cases':[]}
def call(name,args,expected):
 result=subprocess.run(args,capture_output=True,timeout=15)
 row={'name':name,'arguments':args,'actualExitCode':result.returncode,'expectedExitCode':expected,'streams':{}}
 for stream,raw in [('stdout',result.stdout),('stderr',result.stderr)]:
  p=EV/(name+'.'+stream+'.log');assert not p.exists();p.write_bytes(raw);row['streams'][stream]={'path':str(p),'bytes':len(raw),'sha256':sha(raw),'base64':base64.b64encode(raw).decode()}
 r['cases'].append(row);assert result.returncode==expected,(name,result.returncode,result.stderr.decode(errors='replace'))
 return result
try:
 baseline="from pathlib import Path;ns={'__name__':'archived_baseline_local','__file__':"+repr(str(SOURCE))+"};exec(compile(Path("+repr(str(BEFORE))+").read_bytes(),"+repr(str(SOURCE)) +",'exec'),ns);ns['self_test']()"
 old=call('editor-header-baseline-selftest-now',[sys.executable,'-B','-c',baseline],0)
 new=call('editor-header-current-selftest-final',[sys.executable,'-B',str(SOURCE),'--self-test'],0)
 old_checks=json.loads(old.stdout);new_checks=json.loads(new.stdout)
 assert old_checks['status']==new_checks['status']=='PURE_LOCAL_CHECKS_PASS'
 assert new_checks['checks'][:len(old_checks['checks'])]==old_checks['checks']
 cases=[('--self-test',),('--browser',),('--continuous-accept',),('--voice-capabilities',),('--visual-only',),('--visual-fixture','OUTLINED'),('--browser-content','REMINDER')]
 for i,flags in enumerate(cases,1):call('editor-header-cli-negative-'+str(i),[sys.executable,'-B',str(SOURCE),'--header-deadline-only',*flags],2)
 call('editor-header-cli-no-remote-inputs',[sys.executable,'-B',str(SOURCE),'--header-deadline-only'],2)
 help_result=call('editor-header-cli-help',[sys.executable,'-B',str(SOURCE),'--help'],0);assert b'--header-deadline-only' in help_result.stdout
 def defs(p):return {x.name:ast.dump(x,include_attributes=False) for x in ast.parse(p.read_bytes()).body if isinstance(x,(ast.FunctionDef,ast.ClassDef))}
 a,b=defs(BEFORE),defs(SOURCE);changed=[k for k in a if a[k]!=b[k]];assert set(changed)=={'ProgressReader','self_test','main'}
 # Every other old function/class, including old Client/exercise/validators,
 # shutdown/cleanup ownership, request framing and optional feature functions,
 # remains literally AST-equivalent; source line shifts are irrelevant.
 r.update(status='PASS_PURE_LOCAL_NOT_WINDOWS',oldSelftestGroups=len(old_checks['checks']),newSelftestGroups=len(new_checks['checks']),allOriginalSelftestGroupsStillPass=True,existingDefinitionsChanged=changed,unchangedExistingDefinitions=[k for k in a if a[k]==b[k]],newDefinitions=[k for k in b if k not in a],pureCliNegativeCases=len(cases)+1)
 assert sha(SOURCE.read_bytes())==binding['afterSha256']
except BaseException as error:
 r.update(status='FAIL',errorType=type(error).__name__,error=str(error));raise
finally:OUT.write_text(json.dumps(r,ensure_ascii=False,indent=2)+'\n')
print(json.dumps({'path':str(OUT),'bytes':OUT.stat().st_size,'sha256':sha(OUT.read_bytes()),'status':r['status'],'oldGroups':r.get('oldSelftestGroups'),'newGroups':r.get('newSelftestGroups'),'cliNegatives':r.get('pureCliNegativeCases')}))
