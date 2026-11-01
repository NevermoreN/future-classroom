#!/usr/bin/env python3
"""Read-only R68 finite Windows acceptance evidence audit, never product tests.
Missing/RUNNING final originals yield WAIT without creating a report. Only root
invokes after original business and final Windows inventory have actually exited.
"""
from pathlib import Path
import argparse,ast,base64,datetime,hashlib,importlib.util,json,re,sys
sys.dont_write_bytecode=True
ROOT=Path('/tmp/fc-round68-production-https');E=ROOT/'evidence';W=ROOT/'worktree'
R67=Path('/workspace/共享/Eliora/未来教室计划/开发计划/验证记录/2026-11-01-round67-ci/independent-runtime-first.py')
class Wait(Exception):pass
def need(v,m):
 if not v:raise AssertionError(m)
def sha(b):return hashlib.sha256(b).hexdigest()
def identity(p):
 p=Path(p)
 if not p.is_file():raise Wait('missing original '+str(p))
 need(not p.is_symlink(),'nonaliased original '+str(p))
 b=p.read_bytes();return {'path':str(p),'bytes':len(b),'sha256':sha(b)}
def read(p):
 p=Path(p)
 if not p.is_file():raise Wait('missing original '+str(p))
 try:return json.loads(p.read_text(encoding='utf-8-sig'))
 except json.JSONDecodeError:raise Wait('incomplete original JSON '+str(p)) from None
def exact(row,path=None):
 got=identity(path or row['path']);need((got['bytes'],got['sha256'])==(row['bytes'],row['sha256']),'original byte identity '+got['path']);return got
def load_module(path,name):
 spec=importlib.util.spec_from_file_location(name,path);m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m);return m
def terminal(name,allowed):
 value=read(E/name)
 if value.get('status') in ('RUNNING','STARTED'):raise Wait('original still RUNNING '+name)
 need(value.get('status') in allowed,'terminal status '+name+': '+str(value.get('status')));return value
def build(name):
 d=terminal(name+'.json',{'PASS'});need(d['exitCode']==0 and d['sourceUnchanged'] is True and d['workingFileCount']==595,'actual command exit/source '+name)
 need(d['originalBudgetSeconds']==900 and d['elapsedSeconds']<=900,'original900s budget '+name)
 exact({'path':str(E/d['sourceFreeze']),'bytes':(E/d['sourceFreeze']).stat().st_size,'sha256':d['sourceFreezeSha256']})
 for k in ('stdout','stderr'):exact({'path':str(E/(name+'.'+k+'.log')),'bytes':d[k+'Bytes'],'sha256':d[k+'Sha256']})
 return d

def raw_stdout(report):
 capture=report['rawPreservation'];need(capture['status']=='PASS','original remote raw preservation')
 for name,row in capture['files'].items():
  p=Path(row['artifact']);remote=row['remote'];exact({'path':str(p),'bytes':remote['bytes'],'sha256':remote['sha256']})
  need(row['downloaded']==remote,'raw remote/download metadata exact '+name)
  if 'base64' in row:need(base64.b64decode(row['base64'])==p.read_bytes(),'raw inline exact '+name)
 p=Path(capture['files']['tls.stdout']['artifact']);b=p.read_bytes()
 return b.decode('utf-16') if b.startswith((b'\xff\xfe',b'\xfe\xff')) else b.decode('utf-8-sig')
def fixture_cleanup(report):
 need(report['status']==report['cleanup']=='PASS','actual probe and cleanup PASS')
 d=report['cleanupDetails'];need(d['status']=='PASS' and d['rootAbsent'] is True and d['cngKeyExists'] is False and d['myDigestRestored'] is True
  and d['certificatesRemoved']==1 and d['ownedProcessesRemaining']==0,'exact fixture cert/CNG/root/process restoration')
 need(all(d[k]=='UNTOUCHED' for k in ['httpSys','sshConfiguration','rootTrustStore','firewall']),'persistent configuration untouched')
 need(report['ownedProcessCleanup']['status']=='PASS' and report['ownedProcessCleanup']['ownedProcessesRemaining']==0,'physical own process cleanup')
 verification=report['clientVerification'];need(verification['checkHostname'] is True and verification['verifyMode']=='CERT_REQUIRED','strict owned certificate/hostname verification')

def main():
 parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--final-audit',type=Path,default=E/'windows-readonly-final-first.json');parser.add_argument('--report',type=Path,default=E/'independent-acceptance-first.json');a=parser.parse_args()
 need(a.report.parent.resolve()==E and not a.report.exists(),'fresh exclusive local evidence report required')
 result={'schemaVersion':1,'status':'FAIL','scope':'Independent read-only R68 source7/current22 PE/actual21 Windows harness/native ordinary HTTPS and separate business/browser originals. No new product tests, build, Windows, SSH or network.',
  'limitations':['Finite x86 GNU binaries executed on Win11 build26300 x64/WOW64 only; no actual Win7/Win10/physical Seewo/campus certification.',
   'Ordinary configured HTTPS hidden composition and separate legacy business/continuous/browser composition are separate evidence scopes. Legacy browser reused already approved session; it is not teacher pairing UI evidence.',
   'Per-harness/native normalized merged lines are distinct from exact controller stdout/stderr files. Original strictP0 nanosecond experiments and private concurrency caveat are not promoted by these finite passes.']}
 try:
  # Read final audit first. A historical PASS or current state file is not a
  # substitute for this run's actual terminal final inventory.
  final=read(a.final_audit)
  if final.get('status') in ('RUNNING','STARTED') or not final.get('completedAtUtc'):raise Wait('final inventory not terminal')
  need(final['status']=='PASS','final readonly Windows inventory PASS')
  for name in ['business-tls-operation-first.json','ordinary-https-operation-third.json','supplemental-flows-operation-first.json','header-deadline-operation-first.json']:
   build(name[:-5])
  full=terminal('windows-release-full-third.json',{'PASS'});op=terminal('windows-release-full-third-operation.json',{'PASS_COMMAND'})
  ordinary=terminal('windows-ordinary-https-third.json',{'PASS'});business=terminal('windows-business-tls-first.json',{'PASS'})
  terminal('windows-release-extra-flows-first.json',{'PASS_TWO_DEDICATED_HIDDEN_FLOWS'})
  freeze=read(E/'joint-implementation-freeze-seventh.json');need(len(freeze['files'])==47 and len(freeze['workingFiles'])==freeze['workingFileCount']==595,'complete source7 47/595 inventory')
  for row in freeze['workingFiles']:exact({'path':str(W/row['path']),'bytes':row['bytes'],'sha256':row['sha256']})
  for row in freeze['files']:exact({'path':str(W/row['path']),'bytes':row['bytes'],'sha256':row['sha256']})
  for row in freeze['parents']:exact({'path':str(E/row['path']),'bytes':row['bytes'],'sha256':row['sha256']})
  freezes={key:read(E/('joint-implementation-freeze-'+key+'.json')) for key in ['fifth','sixth','seventh']}
  maps={k:{r['path']:(r['bytes'],r['sha256']) for r in f['workingFiles']} for k,f in freezes.items()}
  need(maps['fifth'].keys()==maps['sixth'].keys()==maps['seventh'].keys(),'same595 membership across source5/6/7')
  delta56=[p for p in maps['fifth'] if maps['fifth'][p]!=maps['sixth'][p]];delta67=[p for p in maps['sixth'] if maps['sixth'][p]!=maps['seventh'][p]]
  need(delta56==['scripts/windows-ordinary-https-probe.py'],'source5-to6 Python only')
  need(set(delta67)=={'crates/fc-bootstrap/src/application/https_settings_controller/tests.rs','crates/fc-bootstrap/src/backend/account_management_tests.rs','crates/fc-bootstrap/src/backend/profiles/tests.rs'},'source6-to7 exact three test fixture changes')
  result['source']={'freeze':identity(E/'joint-implementation-freeze-seventh.json'),'modifiedFiles':47,'workingFiles':595,'allCurrentBytes':'PASS','source5To6':delta56,'source6To7':delta67}
  guard=load_module(E/'root-run-windows-release-first.py','fc_r68_artifact_static_guard');old=load_module(R67,'fc_r67_pure_harness_parser')
  need(sha(R67.read_bytes())=='0317ffb1719737bd9dfa45ca7952c242df6ffff0baa6b1611d7b912277f9e0e5','archived reused R67 parser unchanged')
  product_history=[]
  for suffix,witness in [('first','windows-release-first.json'),('second','windows-ordinary-https-second.json'),('third','windows-ordinary-https-third.json'),('fourth','windows-release-full-third.json')]:
   name='joint-release-product-'+suffix;d=build(name);items=[x for x in guard.compiler_artifacts(E/(name+'.stdout.log')) if x[2]['target']['name']=='fc-bootstrap' and x[2]['profile']['test'] is False]
   need(len(items)==1,'unique actual normal producer '+name);number,line,row=items[0];receipt=read(E/witness)
   if suffix=='first':
    prior=read(E/'windows-release-operation-first.json')['normal'];need(prior['cargoMetadataLine']==number and prior['cargoMetadataLineSha256']==sha(line),'historical first exact producer receipt')
    need(prior['sha256']==receipt['exeSha256'] and {x['name']:x['sha256'] for x in receipt['uploadedArtifacts']}['fc-bootstrap.exe']==prior['sha256'],'source4 historical normal upload receipt')
    record={'status':'HISTORICAL_SOURCE4_RECEIPT_ONLY_ORIGINAL_EXE_REPLACED','identity':prior}
   else:
    current=guard.artifact(number,line,row,2);need(current['sha256']==receipt['exeSha256'],'current actual normal versus original uploaded witness '+suffix)
    record={'status':'ACTUAL_CURRENT_EXE_AND_ORIGINAL_WITNESS_MATCH','identity':current}
   product_history.append({'build':identity(E/(name+'.json')),'freeze':d['sourceFreeze'],'witness':identity(E/witness),**record})
  normal=product_history[-1]['identity'];need(len({r['identity']['sha256'] for r in product_history[1:]})==1,'source5/6/7 normal exact unchanged')
  need(product_history[0]['identity']['sha256']!=normal['sha256'],'preserve distinct historical source4 normal')
  need(op['normal']==normal and full['exeSha256']==normal['sha256'] and ordinary['exeSha256']==business['exeSha256']==normal['sha256'],'all current probes exact normal bytes')
  build('joint-release-tests-third');build('joint-release-clippy-first');build('joint-clippy-seventh')
  metadata=guard.compiler_artifacts(E/'joint-release-tests-third.stdout.log');tests=[guard.artifact(*item,expected_subsystem=3) for item in metadata if item[2]['profile']['test'] is True]
  need(len(tests)==21 and tests==op['tests'],'all actual21 test producer/PE identities exact controller ledger')
  need(full['status']==full['cleanup']=='PASS' and op['exitCode']==0 and op['sourceUnchanged'] is True and op['artifactsUnchanged'] is True,'actual complete full controller result/cleanup/source')
  exact(op['stdout']);exact(op['stderr']);exact(op['result']);need(op['allOriginalHarnessIdentitiesMatch'] is True,'controller actual identities')
  uploaded={x['name']:x['sha256'] for x in full['uploadedArtifacts']};need(uploaded=={Path(x['path']).name:x['sha256'] for x in [normal]+tests},'actual all22 uploaded SHA complete set')
  need(len(full['tests'])==21 and {x['name'] for x in full['tests']}=={Path(t['path']).name for t in tests},'all21 actual harness original complete set')
  inventory=[];all_names={}
  for test in full['tests']:
   need(test['binarySha256']==uploaded[test['name']],'actual harness exact binary SHA');count=old.harness(test)
   names=[old.CASE.fullmatch(s)[1] for s in test['output'] if old.CASE.fullmatch(s)]
   inventory.append(dict(count,caseNames=names));all_names[test['name']]=set(names)
  need(all(row['terminalSummaries']==[[next(i['passed'] for i in inventory if i['name']==row['name']),0,0,0,0]] for row in op['actualHarnessSummaries']),'independent cases vs controller unique summaries')
  native=next(i for i in inventory if i['name'].startswith('fc_windows-'));need(native['passed']==250,'actual native250 complete case inventory')
  required_native=['tls_frontdoor::native::stream::tests::configured_explicit_ipv4_and_ipv6_source_keeps_original_deadline_and_real_drain',
   'tls_frontdoor::native::stream::tests::configured_source_filter_returns_real_ipv4_and_ipv6_accept_before_any_sspi_call',
   'tls_frontdoor::native::stream::tests::real_iocp_and_winsock_release_failure_never_retries_or_becomes_quiescent_in_child']
  need(set(required_native)<=all_names[native['name']],'actual IPv4/6 and original sticky-cleanup child cases present/pass')
  result['products']=product_history;result['fullRegression']={'result':identity(E/'windows-release-full-third.json'),'controller':identity(E/'windows-release-full-third-operation.json'),'harnesses':inventory,'totals':{k:sum(i[k] for i in inventory) for k in ['passed','failed','ignored','measured','filteredOut']},'requiredNativeCases':required_native,'capture':'normalized per-harness merged lines; controller raw files separately byte-bound'}
  # Reuse existing pure native transcript validators only, not product tests.
  constants={}
  for node in ast.parse((W/'scripts/windows-probe.py').read_bytes()).body:
   if isinstance(node,ast.Assign):
    for target in node.targets:
     if isinstance(target,ast.Name) and target.id in ['PUBLIC_DESKTOP_STEPS','SUBJECT_VISUAL_STEPS']:constants[target.id]=ast.literal_eval(node.value)
  public=old.flow(full,'public',constants['PUBLIC_DESKTOP_STEPS']);visual=read(E/'windows-release-visual-first.json');need(visual['exeSha256']==normal['sha256'],'visual normal SHA')
  visual_result=old.flow(visual,'visual',constants['SUBJECT_VISUAL_STEPS']);repair=read(E/'windows-release-repair-first.json');need(repair['status']==repair['cleanup']=='PASS' and repair['exeSha256']==normal['sha256'] and repair['native']['reinitializationExitCode']==0,'repair actual normal/exit/cleanup')
  repair_lines=repair['native']['reinitializationOutput'];children=[re.fullmatch(r'REINITIALIZATION_CHILD_PASS stage=(cancel|commit|reopen) nonce=([0-9a-f]{32})',x) for x in repair_lines if x.startswith('REINITIALIZATION_CHILD_PASS')]
  need(all(children) and [m[1] for m in children]==['cancel','commit','reopen'] and len({m[2] for m in children})==1,'three actual original repair child stages')
  repair_footer=[s for s in repair_lines if s.startswith('reinitialization_native=PASS child_processes=3 ')];need(len(repair_footer)==1 and all(x in repair_footer[0] for x in ['full_reset=PASS','new_dpapi=PASS','old_account_absent=PASS','old_backup_direct_restore_denied=PASS','isolated_cleanup=PASS']),'complete repair footer')
  extra=read(E/'windows-release-extra-flows-first.json');need(extra['normal']==normal and extra['sourceFreezeSha256']==identity(E/'joint-implementation-freeze-seventh.json')['sha256'],'extra flows source/normal')
  for item in extra['operations']:
   need(item['status']==item['resultStatus']==item['cleanup']=='PASS' and item['exitCode']==0,'extra original operation exit')
   for stream in ['stdout','stderr']:exact(dict(item[stream],path=str(E/('windows-release-'+item['kind']+'-first.'+stream+'.log'))))
  result['nativeFlows']={'public':public,'visual':visual_result,'repairChildStages':[m[1] for m in children],'repairFooter':repair_footer[0]}
  fixture_cleanup(ordinary);ordinary_text=raw_stdout(ordinary);ui=load_module(W/'scripts/windows-ordinary-https-probe.py','fc_r68_existing_pure_ordinary_contract')
  need(ordinary['runnerSourceSha256']==sha((W/'scripts/windows-ordinary-https-probe.py').read_bytes()),'C7 ordinary exact source')
  transcript=ui.validate_transcript(ordinary_text.splitlines());journey=ui.validate_journey(ordinary['events'],ordinary['ready'],ordinary['authenticatedBusiness'],ordinary['marker'],ordinary['configuredPort'])
  ready_lines=[json.loads(s.split(' ',1)[1]) for s in ordinary_text.splitlines() if s.startswith('ORDINARY_HTTPS_READY ')];need(ready_lines==ordinary['ready'] and ordinary['nativeExit']=={'code':0},'two actual raw READY and actual native exit')
  need(transcript==ordinary['transcript'] and journey==ordinary['wireJourney'],'ordinary exact full transcript/journey summary')
  result['ordinaryHttps']={'report':identity(E/'windows-ordinary-https-third.json'),'transcript':transcript,'wireJourney':journey,'teacher':{k:ordinary['authenticatedBusiness'][k] for k in ['principalId','role','classId','beforeRevision','committedRevision','readbackRevision','durableReceipt']},'actualGenerations':[x['generation'] for x in ordinary['ready']],'cleanup':'OWNED_CERT_CNG_MY_ROOT_PROCESSES_RESTORED','materialReadyStop':ordinary['materialReadyStop']}
  fixture_cleanup(business);raw_stdout(business);need(business['runnerSourceSha256']==sha((W/'scripts/windows-tls-business-probe.py').read_bytes()),'business original source hash')
  need(business['nativeProcessExit']=={'code':0} and business['continuousAcceptEnabled'] is True,'actual continuous native exit')
  n=business['native'];need(n['physicalNativeDrain'] is True and n['workerJoin'] is True and n['sharedWorkers']==2 and n['hubPending']==n['downloadHubPending']==n['quarantined']==0 and n['heads']==n['responses'] and n['setupCleanup'] is True,'actual native both hubs/two-worker physical return')
  b=business['business'];ca=b['continuousAcceptance'];need(ca['capacityCloses']==2 and ca['sameOriginalReceipts']==4 and len(ca['originalIntervals'])==4,'continuous real four original receipts and capacity closes')
  for interval in ca['originalIntervals']:need(interval['deadlineNs']-interval['admittedAtNs']==120000000000,'original JSON120s interval')
  need(b['materialBytes']==b['downloadBytes'] and b['materialSha256']==b['downloadSha256'] and b['exactRetryReceipt'] is True and b['downloadClientRetainedBodyBytes']==0 and b['subsequentBinaryBeginReusedHubSlot'] is True,'real material upload/exact receipt/stream download and slot reuse')
  need(b['voiceOperation']=='UPLOAD_ONLY_NO_BROADCAST_OR_PLAYBACK' and b['voicePcmAllZero'] is True,'silent fixture upload only')
  vc=business['voiceCapabilitiesObservation'];need(vc['status']=='PASS' and vc['additionalWireRequests']==3 and vc['unauthenticatedStatus']==401 and vc['authenticatedStatuses']==[200,200],'three actual capabilities observations')
  for event in business['events']:need(event['status']=='PASS','actual business client event PASS')
  browser=business['browser'];node=read(Path(browser['nodeReportPath']));owner=read(Path(browser['ownerReportPath']))
  need(browser['status']==browser['cleanup']=='PASS' and browser['nodeActualExitCode']==browser['ownerActualExitCode']==0 and browser['ownerPhysicalCleanup']=='PASS','actual isolated browser node/owner exit/cleanup')
  need(node['status']=='PASS' and node['writeOutcome']=='COMMITTED' and node['contentRevision']=='1' and node['contentPosts']==1 and node['sessionCsrfMatched'] is True and node['contextDisposed'] is True and node['cdpDisconnected'] is True,'actual browser authorized one content save/reload')
  need(node['apiOperations']=={'statusGets':2,'sessionGets':2,'scopeGets':2,'contentGets':2,'contentPosts':1,'visualGets':4} and node['visualReads']==4,'actual browser API ledger')
  need(owner['status']==owner['cleanup']=='PASS' and owner['nativeOutcome']['actualJobProcessesZero'] is True and owner['nativeOutcome']['originalHandlesClosed'] is True and owner['nativeOutcome']['originalPrimaryExited'] is True,'actual original browser job and primary process physical return')
  need(business['strictP0']=='NOT_VALIDATED' and business['productionListener']=='NOT_ENABLED','legacy diagnostic scope remains distinct from configured ordinary host')
  result['separateBusinessAndBrowser']={'report':identity(E/'windows-business-tls-first.json'),'controller':identity(E/'business-tls-operation-first.json'),'clientEventCount':len(business['events']),'native':n,'continuousCapacityCloses':ca['capacityCloses'],'originalJsonIntervals':ca['originalIntervals'],'browserNode':identity(Path(browser['nodeReportPath'])),'browserOwner':identity(Path(browser['ownerReportPath'])),'browserApiOperations':node['apiOperations'],'browserWireResponses':node['wireResponseCount'],'cleanup':'PASS','strictP0':business['strictP0'],'productionListener':business['productionListener']}
  header=terminal('windows-header-deadline-first.json',{'PASS'});fixture_cleanup(header);raw_stdout(header)
  need(header['exeSha256']==normal['sha256'] and header['headerDeadlineOnly'] is True and header['nativeProcessExit']=={'code':0},'actual source-bound header-only native exit')
  need(header['runnerSourceSha256']==sha((W/'scripts/windows-tls-business-probe.py').read_bytes()),'header original runner source')
  observation=header['headerDeadlineObservation'];correlation=header['headerDeadlineNativeCorrelation'];events=header['events']
  need(observation['status']=='PASS' and observation['thresholdNs']==5000000000 and observation['epoch']=='PER_CONNECTION_BEFORE_LOCAL_TCP_CONNECT_CALL'
   and observation['applicationRequestsCompleted']==0 and observation['audioOperation']==observation['authenticationOperation']=='NONE','original conservative header threshold/scope')
  need([x['case'] for x in events]==['header-deadline-tls-1','header-deadline-tls-2','header-deadline-tls-3','header-deadline-tls-4','header-deadline-raw-tcp'],'exact four TLS then one raw TCP cases')
  for event in events:
   need(event['status']=='PASS' and event['peerCloseObserved'] is True and event['localCloseUsedAsPeerEvidence'] is False
    and event['completeHeadSent'] is False and event['applicationReceivedBytes']==0 and event['sshMasterAliveAtPeerClose'] is True
    and event['nativeLauncherAliveAtPeerClose'] is True and event['peerCloseNs']-event['beforeConnectNs']==event['elapsedNs']
    and 0<event['elapsedNs']<=5000000000 and event['localCloseBeganNs']>event['peerCloseNs'],'actual unrounded peer terminal timing before local close')
  need(correlation['status']=='PASS' and correlation['nativeHeaderCloseBegins']==correlation['nativeHeaderSocketReturns']==5
   and len(correlation['nativeFiveClosedTokens'])==5 and correlation['rawTcpCapacityRejects']==0 and correlation['socketReturnIsPeerClose'] is False
   and correlation['originalStrictP0']=='NOT_VALIDATED' and header['strictP0']=='NOT_VALIDATED','finite original-token native correlation without kernel/P0 promotion')
  result['headerDeadlineFiniteSamples']={'report':identity(E/'windows-header-deadline-first.json'),'controller':identity(E/'header-deadline-operation-first.json'),
   'epoch':observation['epoch'],'thresholdNs':observation['thresholdNs'],'samples':[{'case':x['case'],'elapsedNs':x['elapsedNs'],'peerCloseObserved':True,'localCloseUsedAsPeerEvidence':False} for x in events],
   'nativeHeaderReturns':5,'mapping':'Individual TLS peer-to-slot mapping NOT_PROVEN; four actual retire/rearm before single raw TCP phase',
   'strictP0':'NOT_VALIDATED','scope':'Finite conservative before-local-connect to peer-terminal samples only; no universal OS scheduling or kernel ESTABLISHED timing guarantee.'}
  historical=[]
  for name in ['windows-release-first.json','windows-focused-connect-first.json','windows-ordinary-https-first.json','windows-ordinary-https-second.json','windows-release-full-second.json','first-runtime-failures-qualification.json','post-clash-native-full-qualification-first.json','ui-C-second-run-qualification-seventh.json','independent-B-runtime-review-first.json','independent-B-runtime-review-second.json','independent-B-private-stop-reachability-first.json','independent-native-review-first.json']:
   d=read(E/name);historical.append(dict(identity(E/name),originalStatus=d.get('status')))
  need(all(read(E/name)['status']=='FAIL' for name in ['windows-release-first.json','windows-focused-connect-first.json','windows-ordinary-https-first.json','windows-ordinary-https-second.json','windows-release-full-second.json']),'original failures never overwritten/promoted')
  result['historicalOriginalsPreserved']=historical
  need(identity(a.final_audit)['sha256']=='75fdc192da4ad7026a8480118ab2cc305ccfe978a1cc8cb69cdc423defeb8659','root-confirmed completed final audit exact bytes')
  need(final['authentication']=='PASS' and final['sshd']=='Running' and final['persistentKeyMode']=='0o600' and final['publicAuthorizationUnchanged'] is True and final['projectRunUnchanged'] is True and not final['scratchDirectories'] and not final['ownedProcesses'] and not final['ownedTasks'],'final read-only persistent auth/Run and zero owned remnants')
  result['finalWindowsAudit']=identity(a.final_audit);result['helper']=identity(Path(__file__));result['recordedUtc']=datetime.datetime.now(datetime.timezone.utc).isoformat();result['status']='PASS_R68_SOURCE7_FINITE_WIN11_REGRESSION_ONLY'
 except Wait as error:
  print(json.dumps({'status':'WAIT','reason':str(error),'reportCreated':False}));return 3
 except Exception as error:result['error']={'kind':type(error).__name__,'message':str(error)}
 with a.report.open('x') as out:json.dump(result,out,ensure_ascii=False,indent=2);out.write('\n')
 print(json.dumps({'status':result['status'],'report':str(a.report),'sha256':sha(a.report.read_bytes())}));return 0 if result['status'].startswith('PASS_') else 1
if __name__=='__main__':raise SystemExit(main())
