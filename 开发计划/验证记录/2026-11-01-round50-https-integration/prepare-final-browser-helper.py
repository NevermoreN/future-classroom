from pathlib import Path
E=Path('/tmp/fc-round50-https-integration-evidence')
old=Path('/workspace/共享/Eliora/未来教室计划/开发计划/验证记录/2026-11-01-round41-frontdoor-deadline/default-browser-final-independent-helper-first.py').read_text()
part=old[old.index('    network=n[\'network\']'):old.index("    first=json_input(EV/'windows-default-browser-continuous-first.json')")]
part=part.replace("check('derive exact68 raw events64HTTP plus18browser wire equals82native',len(r['events'])==68 and len(http)==64 and len(wire)==18 and final['heads']==final['responses']==len(http)+len(wire)==82)","check('independent conservation rawHTTP plus actual browser wire equals native',len(r['events'])==len(http)+len(nonhttp) and final['heads']==final['responses']==len(http)+len(wire))")
header='''"""Local read-only recomputation. No SSH, test, browser, Cargo or cleanup runs."""
from pathlib import Path
from urllib.parse import urlsplit
from collections import Counter
import ast,base64,datetime,hashlib,json,re,sys
sys.dont_write_bytecode=True
EV=Path('/tmp/fc-round50-https-integration-evidence')
R46=Path('/tmp/fc-round46-ci-evidence')
R51=Path('/tmp/fc-round51-browser-diagnostics-evidence')
SCRIPTS=Path('/tmp/fc-round51-browser-diagnostics-worktree/scripts')
OUT=EV/'browser-readonly-independent-first.json'
sha=lambda b:hashlib.sha256(b).hexdigest()
inputs={};report={'schemaVersion':1,'status':'RUNNING','checks':[],'flows':[]}
TAG=''
def check(name,condition):
 report['checks'].append({'flow':TAG,'name':name,'status':'PASS' if condition else 'FAIL'})
 if not condition:raise AssertionError(name)
def input_bytes(path):
 p=Path(path);data=p.read_bytes();row={'path':str(p),'bytes':len(data),'sha256':sha(data)}
 if str(p) in inputs:check('input bytes unchanged '+p.name,inputs[str(p)]==row)
 else:inputs[str(p)]=row
 return data
def json_input(p):return json.loads(input_bytes(p).decode('utf-8-sig'))
def encoded_stream(row,label):
 data=base64.b64decode(row['base64'],validate=True)
 check(label+' complete bytes SHA',row['bytes']==len(data) and row['sha256']==sha(data) and row.get('captureScope','COMPLETE')=='COMPLETE' and row.get('observedBytes',len(data))==len(data))
 return data
def raw_files(owner,label):
 preservation=owner['rawPreservation'];check(label+' preservation PASS',preservation['status']=='PASS');result={}
 for name,row in preservation['files'].items():
  data=input_bytes(row['artifact']);expected=row.get('downloaded',row)
  check(label+' raw accepted '+name,row['status']=='PASS' and expected['bytes']==len(data) and expected['sha256']==sha(data))
  if 'remote' in row:check(label+' remote/download '+name,row['remote']==expected)
  if 'base64' in row:check(label+' encoded/file '+name,base64.b64decode(row['base64'],validate=True)==data)
  result[name]=data
 return result
def recursive_raw(x,label='raw'):
 count=0
 if isinstance(x,dict):
  if {'bytes','sha256','base64'}<=x.keys():encoded_stream(x,label);count+=1
  for k,v in x.items():count+=recursive_raw(v,label+'/'+k)
 elif isinstance(x,list):
  for i,v in enumerate(x):count+=recursive_raw(v,label+'/'+str(i))
 return count

def validate_wire(network):
 flags=['servedFromCache','fromDiskCache','fromServiceWorker','fromPrefetchCache']
 if not all(x['finished'] is True and x['bodyVerified'] is True for x in network):raise ValueError('unfinished')
 return [x for x in network if x['wireStatus'] is not None and (x['wireStatus']==304 or not any(x[f] for f in flags))]
def validate_readonly(q,base,raw_observed):
 if q['status']!='PASS' or q['integrityStatus']!='PASS' or q['ssh']['exitCode']!=0:raise ValueError('readonly terminal')
 for k,v in raw_observed.items():
  if q[k]!=v:raise ValueError('raw readonly mismatch')
 if q['sshPublicAuthorizationSha256']!=base['sshPublicAuthorizationSha256'] or q['projectRunReadOnlySnapshot']!=base['projectRunReadOnlySnapshot']:raise ValueError('baseline changed')
 if q['sshd']!='Running' or q['persistentKeyMode']!='0o600' or q['persistentKeyMetadata']['contentsRead'] is not False:raise ValueError('ssh invariant')
 if any(q[k] for k in ['scratchDirectories','ownedProcesses','ownedTasks','activityObserved']):raise ValueError('residual')
 if not all(q[k] is True for k in ['publicAuthorizationUnchanged','projectRunUnchanged']):raise ValueError('integrity')
 views=q['projectRunReadOnlySnapshot']['views']
 if len(views)!=2 or {x['view'] for x in views}!={'Registry32','Registry64'}:raise ValueError('views')

def audit(browser_path,readonly_path,exe,provenance):
    global TAG
    TAG=browser_path.name
    r=json_input(browser_path);b=r['browser'];npath=Path(b['nodeReportPath']);n=json_input(npath)
    check('actual completed browser PASS cleanupPASS',r['status']==r['cleanup']==b['status']==b['cleanup']==n['status']=='PASS')
    check('exact independently audited executable SHA',r['exeSha256']==exe)
    check('production and strict deadline gates unchanged',r['browserContentKind']=='HOMEWORK' and r['continuousAcceptEnabled'] is True and r['strictP0']=='NOT_VALIDATED' and r['productionListener']=='NOT_ENABLED' and r['forwardTimingIsNativeDeadlineEvidence'] is False)
    check('node original complete report bytes SHA and embedded equality',n==b['result'] and inputs[str(npath)]['bytes']==b['nodeReport']['bytes'] and inputs[str(npath)]['sha256']==b['nodeReport']['sha256'])
    freeze=json_input(R51/'implementation-freeze-second.json')
    check('second frozen diagnostic closed whitelist26',freeze['closedNames']['count']==len(freeze['closedNames']['names'])==26 and freeze['closedNames']['regexFallback'] is False)
    names={'observer':'windows-native-browser-observer.py','nodeHarness':'windows-native-browser-harness.mjs','edgeOwner':'windows-test-edge-owner.py','tlsOwnershipHelpers':'windows-tls-probe.py'}
    for key,name in names.items():check('source-bound actual '+key,sha(input_bytes(SCRIPTS/name))==b['sourceSha256'][key])
    for row in freeze['ownedFiles'][:2]:check('exact frozen second '+row['path'],b['sourceSha256']['nodeHarness' if row['path'].endswith('.mjs') else 'observer']==row['sha256'])
    check('successful PASS contains no failure diagnostics',not any('fail' in k.lower() for k in n) and 'error' not in n and 'error' not in r)
    runner_raw=input_bytes(SCRIPTS/'windows-tls-business-probe.py')
    check('source-bound actual runner',sha(runner_raw)==r['runnerSourceSha256']=='0cb59751f0bd1e79dd86de9eb26ac780e264a969d74cbdbe074be988210c7208')
    tree=ast.parse(runner_raw.decode());csp=next(ast.literal_eval(s.value) for s in tree.body if isinstance(s,ast.Assign) and any(isinstance(t,ast.Name) and t.id=='CSP' for t in s.targets))
    check('strict exact CSP digest',sha(csp.encode())==n['expectedCspSha256'])
    check('same origin marker generation and original12s deadline',n['marker']==r['marker'] and n['generation']==r['ready'][0]['generation'] and n['origin']==r['ready'][0]['origin'] and 0<n['maxMillis']<=12000 and n['elapsedMillis']<=n['maxMillis'])
    check('original180s browser owner observation215 budgets',b['edgeSessionCapSeconds']==180 and b['ownerExitObservationSeconds']==215)
    for a in n['assets']:
     want=next(x for x in r['staticPackage']['assets'] if x['path']==a['path'])
     check('source-admitted asset hash '+a['path'],a['verified'] is True and a['bytes']==want['sizeBytes'] and a['sha256']==want['sha256'])
'''
tail='''
    check('independent actual wire classification',validate_wire(n['network'])==wire)
    for owner_obj,label in [(r,'native'),(b,'observer'),(owner,'Edge')]:
     for run in owner_obj.get('remoteControlRunsRaw',[]):check(label+' original control exit0',run['exitCode']==0)
    check('actual remote scripts parsed before use',r['controlScriptsParse']==owner['controlScriptsParse']=='PASS')
    rawcount=recursive_raw(r,'browserRunner')+recursive_raw(owner,'edgeOwner')
    check('raw owner journal uses exact actual original edge job',process['executablePath'].lower()==assigned['executablePath'].lower() if 'executablePath' in process and 'executablePath' in assigned else process['pid']==assigned['pid'])
    q=json_input(readonly_path);base=json_input(q['baseline']['path'])
    check('readonly baseline exact SHA',sha(input_bytes(q['baseline']['path']))==q['baseline']['sha256'])
    for k in ['auditSource','remoteAdapterSource']:check('readonly source '+k,sha(input_bytes(q[k]['path']))==q[k]['sha256'])
    ps=q['powershell'];check('readonly original stdin source UTF8/UTF16 bindings',ps['fullSource']==ps['preamble']+ps['script'] and sha(ps['fullSource'].encode())==ps['sourceSha256Utf8'] and sha(ps['fullSource'].encode('utf-16-le'))==ps['sourceSha256Utf16Le'] and base64.b64decode(ps['encodedCommand'],validate=True)==ps['encodedWrapperSource'].encode('utf-16-le'))
    check('readonly actual original source delivered as stdin',encoded_stream(ps['stdin'],'readonly stdin')==ps['fullSource'].encode())
    observed=json.loads(encoded_stream(q['ssh']['stdout'],'readonly stdout').decode('utf-8-sig'));encoded_stream(q['ssh']['stderr'],'readonly stderr')
    validate_readonly(q,base,observed)
    check('fresh readonly is after browser and observed complete',readonly_path.stat().st_mtime_ns>=browser_path.stat().st_mtime_ns and datetime.datetime.fromisoformat(q['completedAtUtc'])>=datetime.datetime.fromisoformat(q['observedAtUtc']))
    report['flows'].append({'browser':str(browser_path),'readonly':str(readonly_path),'exeSha256':exe,'artifactProvenance':provenance,'status':'PASS_FINITE_ACTUAL_BROWSER_AND_FRESH_READONLY','sourceSha256':b['sourceSha256'],'counts':{'rawEvents':len(r['events']),'rawHTTP':len(http),'browserObserved':len(network),'browserWire':len(wire),'browserCacheOnly':len(network)-len(wire),'nativeHeads':final['heads'],'nativeResponses':final['responses'],'api':len(api),'posts':n['contentPosts'],'visualReads':n['visualReads'],'shortcutClicks':n['reminderShortcutClicks'],'recursiveRawContainers':rawcount},'voiceCapabilities':observation,'edgePhysicalOutcome':{k:outcome[k] for k in ['originalPrimaryExited','originalPrimaryExitCode','actualJobProcessesZero','originalHandlesClosed','reason','failure']},'readonly':{'authorizationUnchanged':q['publicAuthorizationUnchanged'],'projectRunUnchanged':q['projectRunUnchanged'],'sshd':q['sshd'],'ownedResiduals':0},'budgets':{'nodeMaxMillis':n['maxMillis'],'nodeElapsedMillis':n['elapsedMillis'],'edgeSessionCapSeconds':b['edgeSessionCapSeconds']}})

def main():
 if OUT.exists():raise FileExistsError('create-new only')
 try:
  prior=json_input(EV/'runtime-certificate-independent-second.json');check('prior R50 independent actual runtime PASS',prior['status'].startswith('PASS'))
  audit46=json_input(R46/'modern-original-independent-first.json');check('original modern artifact audit PASS static only',audit46['status']=='PASS_STATIC_ONLY_RUNTIME_NOT_RUN')
  audit(R46/'modern-msvc-default-browser-continuous-final-third.json',R46/'windows-readonly-final-second.json','bc8f224a97797311e76182486bfa6502f8c11110e3a7151893f5212ff9d911aa',{'audit':str(R46/'modern-original-independent-first.json'),'binding':'Original exact Modern MSVC artifact; separately completed original runtime review'})
  audit(EV/'windows-default-browser-continuous-first.json',EV/'windows-readonly-final-first.json','b6368400a58b87d4725cbd664b5bde88a4f51927a74656e5c566010e8bbf6930',{'audit':str(EV/'runtime-certificate-independent-second.json'),'binding':'R50 actual429-input compiled normal and runtime22EXE audit'})
  first=json_input(R46/'modern-msvc-default-browser-continuous-first.json')
  check('first real R46 FAIL retained cleanupPASS',first['status']=='FAIL' and first['cleanup']=='PASS')
  firstnode=json_input(first['browser']['nodeReportPath'])
  check('first real network failure remains unresolved',firstnode['status']=='FAIL' and firstnode['error']['code']=='NETWORK_LOAD_FAILED')
  report['history']={'firstActualR46':'FAIL_NETWORK_LOAD_FAILED_ORIGINAL_UNCHANGED','rootCause':'UNKNOWN','subsequentPassMeaning':'Finite successful later observations only; no proven correction of original failure.'}
  check('all reviewed input bytes remain unchanged',all(sha(Path(x['path']).read_bytes())==x['sha256'] for x in inputs.values()))
  report['status']='PASS_TWO_FINITE_ACTUAL_BROWSERS_AND_FRESH_READONLY'
 except BaseException as error:
  report['status']='FAIL';report['error']={'type':type(error).__name__,'check':str(error)[:250]}
 report.update(inputs=list(inputs.values()),helper={'path':str(Path(__file__)),'bytes':Path(__file__).stat().st_size,'sha256':sha(Path(__file__).read_bytes())},limitations=['Only local evidence recomputation, no network/browser/tests/build/cleanup run.','Original R46 NETWORK_LOAD_FAILED remains unknown and unchanged. Later PASS does not prove a fix.','Actual observed capabilities UNVERIFIED; no inference of native query, cache efficacy, current output or audible playback.','Normal PASS has no failure diagnostics; negative diagnostic mechanisms are separately modeled, not exercised by this success.','R50 full Host all-target matrix is separate and not promoted by this runtime review.','StrictP0 firstTCP5s, ordinary production listener, Win7/10 actual OS, visible Seewo, campus trust and72h remain separate gates.','Original Edge primary exit1 on deliberate capability-stop is preserved; supervisor exit0, Job0, handles closed and profile/root absent prove cleanup.'])
 with OUT.open('x') as f:json.dump(report,f,ensure_ascii=False,indent=2);f.write('\\n')
 print(json.dumps({'status':report['status'],'report':str(OUT),'sha256':sha(OUT.read_bytes()),'flows':len(report['flows']),'checks':len(report['checks']),'error':report.get('error')},ensure_ascii=False))
 return 0 if report['status'].startswith('PASS') else 1
if __name__=='__main__':raise SystemExit(main())
'''
p=E/'verify-final-browser-readonly-first.py'
with p.open('x') as f:f.write(header+part+tail)
print(p)
