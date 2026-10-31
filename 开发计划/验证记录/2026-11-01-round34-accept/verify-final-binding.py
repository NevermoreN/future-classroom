from pathlib import Path
import json,hashlib,re,subprocess,datetime,stat
ROOT=Path.cwd();BASE=Path('/tmp/fc-round34-accept-evidence');OUT=BASE/'final-source-and-runtime-binding.json'
def sha(d):return hashlib.sha256(d).hexdigest()
def fact(p):
 d=p.read_bytes();return {'bytes':len(d),'sha256':sha(d)}
def load(n):return json.loads((BASE/n).read_text())
def require(value,message):
 if not value:raise AssertionError(message)
def snapshot():
 files=list((ROOT/'crates').rglob('*.rs'))+list((ROOT/'crates').rglob('Cargo.toml'))+list((ROOT/'.cargo').rglob('*'))+[ROOT/n for n in ['Cargo.toml','Cargo.lock','rust-toolchain.toml']]
 return {p.relative_to(ROOT).as_posix():fact(p) for p in sorted(set(files)) if p.is_file()}
with OUT.open('x') as output:
 r={'status':'FAIL','createdUtc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'scope':'Exact frozen source, GNU artifacts and affected Win11/native/real TLS execution binding. Not full-workspace rerun, MSVC compilation of new Rust, Win7/Win10 execution, production adoption or strict TCP5s evidence.'}
 try:
  names=['windows-affected-first.json','windows-native-first.json','windows-tls-continuous-first.json','windows-after.json','implementation-freeze-second.json','business-probe-freeze.json','review-final-second.json','business-cross-review-first.json','win7-ci-powershell-first.json','clippy-repro-conclusion-first.json']
  r['reports']={n:fact(BASE/n) for n in names}
  build,native,tls,audit=(load(n) for n in names[:4])
  require(build['status']=='PASS' and not build['changedInputs'],'Build did not complete without changed inputs')
  current=snapshot();require(len(current)==383 and current==build['sourceBefore']==build['sourceAfter'],'383 exact Cargo/Rust/config inputs differ')
  r['sourceInputs']=current;r['gitBaseReference']=build['gitHeadBefore'];r['gitReferenceMeaning']='Baseline only; uncommitted current383 input bytes above define the tested new implementation, not that baseline commit.'
  require(build['gitHeadBefore']==build['gitHeadAfter']==subprocess.check_output(['git','rev-parse','HEAD']).decode().strip(),'Git base reference changed during validation')
  for row in load('implementation-freeze-second.json')['files']:
   require(fact(ROOT/row['path'])=={k:row[k] for k in ('bytes','sha256')},'Native frozen source mismatch')
  for row in load('business-probe-freeze.json')['files']:
   require(fact(ROOT/row['path'])=={'bytes':row['afterBytes'],'sha256':row['afterSha256']},'Business frozen source mismatch')
  require(load('review-final-second.json')['status']=='PASS_STATIC_REVIEW_ONLY_NOT_COMPILED_OR_RUN','Native independent review not PASS')
  require(load('business-cross-review-first.json')['blockingFindings']==[],'Business independent review has blockers')
  r['programs']=build['artifacts'];require(len(r['programs'])==2,'Expected exactly two affected executable snapshots')
  for row in r['programs']:
   expected={k:row[k] for k in ('bytes','sha256')}
   require(fact(Path(row['path']))==expected==fact(Path(row['originalPath'])),'Original and copied executable identity differ')
  products=[x for x in r['programs'] if not x['test']];tests=[x for x in r['programs'] if x['test']]
  require(len(products)==len(tests)==1,'Affected artifact roles differ')
  exe_sha=products[0]['sha256'];require(native['exeSha256']==tls['exeSha256']==exe_sha,'Actual normal EXE hashes differ')
  require(native['status']==native['cleanup']=='PASS' and len(native['tests'])==1,'Native runtime/cleanup incomplete')
  test=native['tests'][0];require(test['exitCode']==0 and test['binarySha256']==tests[0]['sha256'],'Native test binary identity/exit differs')
  summaries=[x for x in test['output'] if x.startswith('test result:')]
  require(len(summaries)==1 and re.search(r'177 passed; 0 failed; 0 ignored;',summaries[0]),'177 native tests did not pass exactly')
  old=subprocess.check_output(['git','show',build['gitHeadBefore']+':crates/fc-windows/src/tls_frontdoor/native/stream/tests.rs']).decode()
  new=(ROOT/'crates/fc-windows/src/tls_frontdoor/native/stream/tests.rs').read_text()
  pattern=r'#\[test\]\s*fn\s+(\w+)\s*\('
  old_names=set(re.findall(pattern,old));new_names=set(re.findall(pattern,new))
  require(len(old_names)==16 and len(new_names)==25 and old_names<=new_names,'Original/new stream test coverage names differ')
  r['nativeTests']={'harnesses':1,'passed':177,'failed':0,'ignored':0,'oldStreamTestsRetained':16,'newStreamTests':sorted(new_names-old_names),'summary':summaries[0],'scope':'Real IOCP + controlled record Driver for many transport ownership tests; separate real Schannel fixture below.'}
  require(native['native']['exitCode']==0,'Hidden native lifecycle probe failed')
  require(tls['status']==tls['cleanup']=='PASS' and tls['continuousAcceptEnabled'] is True,'TLS continuous execution/cleanup incomplete')
  events=tls['events'];require(len(events)==55 and all(x['status']=='PASS' for x in events),'55 exact actual client events not PASS')
  expected=['continuous-idle-repost-status','continuous-after-retire-status']+[f'continuous-capacity-reject-{i}' for i in (1,2)]+[f'continuous-original-{i}-exact-receipt' for i in range(1,5)]
  for case in expected:require(sum(x['case']==case for x in events)==1,'Missing/duplicated continuous event')
  idle=tls['continuousIdle'];require(idle['noClientSeconds']>=5.05 and idle['idleReturns']>=1 and idle['repostedSequence']>idle['baselineSequence'],'Actual idle return/repost barrier incomplete')
  stage=tls['business']['continuousAcceptance'];require(stage['capacityCloses']==2 and stage['sameOriginalReceipts']==4 and stage['binaryUploadPermitsUsedByStage']==0,'Actual four-request/capacity result differs')
  sequences=[stage[k] for k in ['admittedSequence','capacityClosedSequence','retiredRearmedSequence','recoveredSequence']];require(all(a<b for a,b in zip(sequences,sequences[1:])),'Actual capacity/retirement/recovery order differs')
  require(len(stage['originalIntervals'])==4 and all(x['deadlineNs']-x['admittedAtNs']==120_000_000_000 for x in stage['originalIntervals']),'Original request interval not preserved')
  snapshots=tls['nativeProgress'];last=snapshots[-1]
  require(len(snapshots)==37 and last['acceptance']['posts']==last['acceptance']['completions']==63,'Final actual accept ledger differs')
  require(last['acceptance']['capacity_rejects']==2 and last['acceptance']['idle_cancellations']==last['acceptance']['idle_returns']==7,'Actual capacity/idle completion ledger differs')
  require(last['acceptance']['owned_sockets']==last['acceptance']['native_pending']==last['acceptance']['bound_slots']==last['jsonBodyRecords']==last['pendingJobs']==0 and not last['requests'],'Final native/request debt not zero')
  maxima={k:max(x['acceptance'][k] for x in snapshots) for k in ['native_pending','owned_sockets','bound_slots']};require(maxima=={'native_pending':5,'owned_sockets':6,'bound_slots':4},'Observed application resource counts differ')
  n=tls['native'];require(n['physicalNativeDrain'] and n['sharedWorkers']==2 and n['workerJoin'] and n['hubPending']==n['downloadHubPending']==n['quarantined']==0 and n['setupCleanup'] and n['heads']==n['responses']==51,'Native/pool/Hub lifecycle incomplete')
  c=tls['cleanupDetails'];require(c['status']=='PASS' and c['rootAbsent'] and c['ownedProcessesRemaining']==0 and c['myDigestRestored'] and c['cngKeyExists'] is False and tls['ownedProcessCleanup']['status']=='PASS','Owned Windows certificate/key/root/process cleanup incomplete')
  require(audit['status']=='PASS' and audit['projectRunUnchanged'] and audit['publicAuthorizationUnchanged'] and not audit['scratchDirectories'] and not audit['ownedProcesses'] and not audit['ownedTasks'] and audit['sshd']=='Running' and audit['persistentKeyMode']=='0o600','Fresh final Windows inventory failed')
  ps=load('win7-ci-powershell-first.json');require(ps['status']=='PASS' and ps['helperSha256']==fact(ROOT/'scripts/windows-msvc-ci.ps1')['sha256'] and ps['result']['cases']==28 and ps['result']['assertions']==421 and ps['result']['failed']==0,'Actual fixed helper pure checks differ')
  r.update(status='PASS_AFFECTED_WINDOWS_SOURCE_AND_RUNTIME_BOUND',normalExeSha256=exe_sha,realTls={'events':55,'heads':51,'responses':51,'progressSnapshots':37,'idleNoClientSeconds':idle['noClientSeconds'],'capacityCloses':2,'preservedOriginalReceipts':4,'applicationObservedMaxima':maxima,'physicalNativeDrain':True,'fixedWorkersJoined':2,'kernelOrSspResourceBudget':'NOT_PROVEN'},scripts={p:fact(ROOT/p) for p in ['scripts/windows-msvc-ci.ps1','scripts/windows-tls-business-probe.py','scripts/windows-probe.py','scripts/package-probe.py']},windows={'os':audit['os'],'build':audit['build'],'architecture':audit['osArchitecture'],'execution':'x86 WOW64','cleanup':'PASS'},remainingGates=['Strict TCP ESTABLISHED to peer-visible close5_000_000_000ns remains original FAIL','Kernel backlog/SSP/full16MiB and72h budgets','New-source MSVC CI, actualWin7/Win10','Ordinary production listener/campus trust/teacher browser flow','VisibleSeewo/PPT/touch/DPI, physical audio, installation/login/upgrade and full plan'])
 except Exception as error:
  r['error']={'kind':type(error).__name__,'detail':str(error)}
 finally:output.write(json.dumps(r,ensure_ascii=False,indent=2)+'\n')
print(json.dumps({'status':r['status'],'error':r.get('error')},ensure_ascii=False));raise SystemExit(0 if r['status'].startswith('PASS_') else 1)
