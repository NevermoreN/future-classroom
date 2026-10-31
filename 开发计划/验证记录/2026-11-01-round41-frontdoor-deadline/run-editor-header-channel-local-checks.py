from pathlib import Path
from types import SimpleNamespace
import ast,base64,hashlib,json,subprocess,sys,datetime
EV=Path('/tmp/fc-round41-frontdoor-deadline-evidence');SOURCE=Path('/tmp/fc-round41-frontdoor-deadline-worktree/scripts/windows-tls-business-probe.py');BEFORE=EV/'windows-tls-business-probe.before-channel-lifecycle.py';OUT=EV/'editor-header-channel-local-checks-first.json';assert not OUT.exists()
sha=lambda b:hashlib.sha256(b).hexdigest();binding={'beforeSha256':sha(BEFORE.read_bytes()),'afterSha256':sha(SOURCE.read_bytes())}
r={'status':'RUNNING','scope':'One full current pure selftest plus CLI; offline parser-only replay of saved SSH bytes with mocked process liveness. NOT a new Windows/Schannel run and NOT retrospective overallPASS/nativeDrain proof.','atUtc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'binding':binding,'commands':[]}
def call(name,args,expected):
 actual=subprocess.run(args,capture_output=True,timeout=15);row={'name':name,'arguments':args,'exitCode':actual.returncode,'expectedExitCode':expected,'streams':{}}
 for stream,raw in [('stdout',actual.stdout),('stderr',actual.stderr)]:
  p=EV/(name+'.'+stream+'.log');assert not p.exists();p.write_bytes(raw);row['streams'][stream]={'path':str(p),'bytes':len(raw),'sha256':sha(raw),'base64':base64.b64encode(raw).decode()}
 r['commands'].append(row);assert actual.returncode==expected,(name,actual.returncode,actual.stderr.decode(errors='replace'));return actual
try:
 result=call('editor-header-channel-selftest-first',[sys.executable,'-B',str(SOURCE),'--self-test'],0)
 prior=json.loads((EV/'editor-header-current-selftest-second.stdout.log').read_text());current=json.loads(result.stdout)
 assert current['status']=='PURE_LOCAL_CHECKS_PASS' and len(current['checks'])==23 and all(x in current['checks'] for x in prior['checks'])
 for i,flags in enumerate([('--self-test',),('--browser',),('--continuous-accept',),('--voice-capabilities',),('--visual-only',),('--visual-fixture','OUTLINED'),('--browser-content','REMINDER'),()],1):call('editor-header-channel-cli-'+str(i),[sys.executable,'-B',str(SOURCE),'--header-deadline-only',*flags],2)
 call('editor-header-channel-cli-help',[sys.executable,'-B',str(SOURCE),'--help'],0)
 original=EV/'windows-header-deadline-first.json';record=json.loads(original.read_text());data=base64.b64decode(record['sshTunnelRaw']['stderr']['base64']);assert sha(data)==record['sshTunnelRaw']['stderr']['sha256']
 lines=data.splitlines(keepends=True);last=max(i for i,l in enumerate(lines) if b'free: direct-tcpip:' in l);prefix=b''.join(lines[:last+1]);assert last+1==232
 alive=SimpleNamespace(poll=lambda:None);fake=SimpleNamespace(process=alive,logs=SimpleNamespace(stderr=bytearray(prefix),overflow=SimpleNamespace(is_set=lambda:False)))
 def namespace(path):
  ns={'__file__':str(SOURCE),'__name__':'offline_ssh_parser_model'};exec(compile(path.read_bytes(),str(SOURCE),'exec'),ns);return ns
 old=namespace(BEFORE);new=namespace(SOURCE)
 try:old['validate_header_forward'](fake,alive)
 except RuntimeError as error:old_error=str(error)
 else:raise AssertionError('old global regex unexpectedly accepted original failed trace')
 parsed=new['validate_header_forward'](fake,alive);assert len(parsed['channels'])==5 and len(parsed['normalChannelCleanup'])==4
 r['offlineOriginalReplay']={'status':'OLD_REJECT_NEW_PARSE_PASS_ONLY','report':str(original),'reportBytes':original.stat().st_size,'reportSha256':sha(original.read_bytes()),'originalStatus':record['status'],'originalCleanup':record['cleanup'],'originalNativeDrain':'NOT_VERIFIED','stderrSha256':sha(data),'prefixBytes':len(prefix),'prefixSha256':sha(prefix),'throughOriginalLine':232,'oldActualParserException':old_error,'newParserResult':parsed,'limits':'Prefix from finally-preserved full original log, through last direct free before explicit cleanup cancel. It can include local-close cleanup that happened after first validation threw; no contemporaneous validation-prefix snapshot existed. Process liveness is mocked ONLY for offline parser classification. No real test status upgraded.'}
 diagnosis=json.loads((EV/'editor-header-channel-diagnosis-first.json').read_text())
 for item in diagnosis['originalBindings']:
  p=Path(item['path']);assert p.stat().st_size==item['bytes'] and sha(p.read_bytes())==item['sha256']
 def defs(p):return {x.name:ast.dump(x,include_attributes=False) for x in ast.parse(p.read_bytes()).body if isinstance(x,(ast.FunctionDef,ast.ClassDef))}
 a,b=defs(BEFORE),defs(SOURCE);changed=[k for k in a if a[k]!=b[k]];assert set(changed)=={'validate_header_forward','self_test_header_deadlines'} and set(a)==set(b)
 assert sha(SOURCE.read_bytes())==binding['afterSha256']
 r.update(status='PASS_PURE_LOCAL_ORIGINAL_FAIL_UNCHANGED',selftestGroups=23,allPrevious22GroupsRetained=True,currentFullSelftestRuns=1,cliNegatives=8,changedExistingDefinitions=changed,allOriginalFilesBytesShaUnchanged=True,negativeCoverage=['direct Brokenpipe before remoteEOF','cross-channel EOF privilege','channel-id reused after old EOF','mux-id reused as direct','mux readfailed without ctlEOF','global SSH connection reset','global fatal Brokenpipe','unsupported errno afterEOF','writefailed afterEOF','duplicate active lifecycle','unpaired openconfirm','unpaired EOF'],TLS='NOT_RUN',Windows='NOT_RUN',Cargo='NOT_RUN',Git='NOT_RUN')
except BaseException as error:
 r.update(status='FAIL',errorType=type(error).__name__,error=str(error));raise
finally:OUT.write_text(json.dumps(r,ensure_ascii=False,indent=2)+'\n')
print(json.dumps({'path':str(OUT),'bytes':OUT.stat().st_size,'sha256':sha(OUT.read_bytes()),'status':r['status'],'groups':r.get('selftestGroups')}))
