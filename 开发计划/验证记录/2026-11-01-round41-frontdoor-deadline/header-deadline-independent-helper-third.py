from pathlib import Path
import base64, datetime, hashlib, json, re

BASE=Path('/tmp/fc-round41-frontdoor-deadline-evidence')
OUT=BASE/'header-deadline-independent-third.json'
REPORT=BASE/'windows-header-deadline-second.json'
RUNNER=BASE/'windows-tls-business-probe.header-deadline-frozen-third.py'
WORKTREE=Path('/tmp/fc-round41-frontdoor-deadline-worktree')
EXE_SHA='3ac0f3728f819c610de16fb0e2b79df5d9e6b0b2992fa151429168ba92957937'
RUNNER_SHA='0cb59751f0bd1e79dd86de9eb26ac780e264a969d74cbdbe074be988210c7208'
THRESHOLD=5_000_000_000
RESERVE=25_000_000
sha=lambda b:hashlib.sha256(b).hexdigest()
assert not OUT.exists()
r={'schemaVersion':1,'status':'RUNNING','scope':'Independent original-file audit of the actual second finite R41 four-overlapping-incomplete-TLS then one-raw-TCP run; no test/network/Windows/Cargo/Git execution. Timings are before-local-TCP-connect to observed peer terminal including SSH; not kernel ESTABLISHED or complete backlog/production P0 proof. No per-client/native-slot mapping.','checks':[]}
inputs={}
def check(name,ok):
 r['checks'].append({'name':name,'status':'PASS' if ok else 'FAIL'})
 assert ok,name

def read(p):
 p=Path(p);data=p.read_bytes();fact={'path':str(p),'bytes':len(data),'sha256':sha(data)}
 if str(p) in inputs:check('immutable repeated original '+p.name,inputs[str(p)]==fact)
 else:inputs[str(p)]=fact
 return data

def js(p):return json.loads(read(p).decode('utf-8-sig'))

def stream(x,name):
 data=base64.b64decode(x['base64'],validate=True)
 check(name+' complete original bytes/SHA',len(data)==x['bytes']==x.get('observedBytes',len(data)) and sha(data)==x['sha256'] and x.get('captureScope','COMPLETE')=='COMPLETE')
 return data

def ssh_lifecycles(data,label):
 records=[];active={};cleanups=[];created=0
 fatal=re.compile(r'open failed|connect failed|administratively prohibited|connection refused|connection closed by|connection to .* closed|timed out|received disconnect|disconnected from|killed by signal|terminated by signal|fatal|corrupted mac|message authentication code incorrect',re.I)
 for number,line in enumerate(data.decode('utf-8').splitlines(),1):
  check(label+' no global transport/open failure line'+str(number),fatal.search(line) is None)
  m=re.fullmatch(r'debug[123]: channel (\d+): new ([^\[]+) \[.*',line)
  if m:
   channel=int(m[1]);check(label+' no active channel reuse line'+str(number),channel not in active)
   created+=1;record={'instance':created,'channel':channel,'kind':m[2].strip(),'newLine':number,'openConfirmedLine':None,'remoteEofLine':None,'remoteCloseLine':None,'controlEofLine':None}
   if record['kind']=='mux listener':
    creation_label=re.search(r'\[([^\]]+)\]',line)
    check(label+' exact original mux listener creation label line'+str(number),creation_label is not None)
    record['muxListenerLabel']=creation_label[1]
   active[channel]=record
   if record['kind']=='direct-tcpip':record['ordinal']=len(records)+1;records.append(record)
   continue
  m=re.fullmatch(r'(?:debug[123]: )?channel (\d+): (.*)',line)
  detail=m[2] if m else None;record=active.get(int(m[1])) if m else None
  if re.search(r'read failed|write failed|broken pipe|connection reset',line,re.I):
   allowed_mux=record is not None and record['kind']=='mux-control' and record['controlEofLine'] is not None and detail=='read failed'
   boundary=(record['remoteEofLine'] or record['remoteCloseLine']) if record else None
   allowed_direct=record is not None and record['kind']=='direct-tcpip' and record['openConfirmedLine'] is not None and boundary is not None and re.fullmatch(r'read failed(?: rfd \d+ maxlen \d+: (?:Broken pipe|Connection reset by peer))?',detail) is not None
   check(label+' error only exact same-instance post-boundary cleanup line'+str(number),allowed_mux or allowed_direct)
   cleanups.append({'line':number,'channel':record['channel'],'instance':record['instance'],'kind':record['kind'],'priorBoundaryLine':record['controlEofLine'] if allowed_mux else boundary,'text':line})
   continue
  if not m:continue
  meaningful=detail.startswith(('open confirm ','rcvd eof','rcvd close','free: ','ctl read<=0'))
  if record is None:
   check(label+' no orphan lifecycle line'+str(number),not meaningful);continue
  if detail.startswith('ctl read<=0'):
   check(label+' exact control EOF line'+str(number),record['kind']=='mux-control' and record['controlEofLine'] is None and re.fullmatch(r'ctl read<=0 rfd \d+ len 0',detail) is not None)
   record['controlEofLine']=number
  elif detail.startswith('open confirm '):
   check(label+' direct open confirmed once line'+str(number),record['kind']=='direct-tcpip' and record['openConfirmedLine'] is None)
   record['openConfirmedLine']=number
  elif detail.startswith(('rcvd eof','rcvd close')):
   check(label+' direct remote terminal follows own confirm line'+str(number),record['kind']!='direct-tcpip' or record['openConfirmedLine'] is not None)
   field='remoteEofLine' if detail.startswith('rcvd eof') else 'remoteCloseLine'
   check(label+' remote terminal occurs once line'+str(number),record[field] is None)
   record[field]=number
  elif detail.startswith('free: '):
   kind=detail[6:].split(':',1)[0].split(',',1)[0]
   check(label+' free matches active owner kind line'+str(number),kind.replace('-',' ')==record['kind'].replace('-',' ') or record['kind']=='mux listener' and kind==record['muxListenerLabel'])
   record['freedLine']=number;del active[record['channel']]
 check(label+' exactly five independently confirmed direct remote EOF lifecycles',len(records)==5 and all(x['newLine']<x['openConfirmedLine']<x['remoteEofLine'] for x in records))
 return records,cleanups

prior=BASE/'header-deadline-independent-first.json'
r['previousCheckerFailure']={'path':str(prior),'bytes':prior.stat().st_size,'sha256':sha(prior.read_bytes()),'reason':'Complete transcript final mux-listener free prints its exact original creation path label, unlike direct/mux-control kind. First checker did not model this normal final cleanup. Second accepts only the same active mux-listener instance exact captured label; direct failure/error/EOF ordering unchanged.'}
prior_second=BASE/'header-deadline-independent-second.json'
r['secondCheckerFailure']={'path':str(prior_second),'bytes':prior_second.stat().st_size,'sha256':sha(prior_second.read_bytes()),'reason':'The independent checker correction accidentally shadowed its diagnostic-label string with a regex Match. Third uses a distinct local variable; all timing/native/SSH acceptance assertions unchanged. Original first/second checker files and reports preserved.'}
try:
 actual=js(REPORT)
 check('actual second run terminal PASS cleanupPASS',actual['status']==actual['cleanup']=='PASS')
 check('actual exact GNU exe and third frozen runner',actual['exeSha256']==EXE_SHA and actual['runnerSourceSha256']==RUNNER_SHA and sha(read(RUNNER))==RUNNER_SHA)
 first=js(BASE/'windows-header-deadline-first.json')
 check('first actual failure retained separately',first['status']=='FAIL' and first['cleanup']=='PASS' and first['exeSha256']==EXE_SHA)
 programs=js(BASE/'programs-independent-first.json')
 check('actual GNU normal bound to independent compiled-source artifact record',programs['normalExecutable']['sha256']==EXE_SHA and programs['normalExecutable']['bytes']==12922615 and programs['sourceBinding']['status']=='PASS' and programs['sourceBinding']['compiledSourceBeforeEqualsAfter'] is True)
 sources={x['path']:x for x in programs['sourceBinding']['inputs']}
 source_refs=[
  ('crates/fc-windows/src/tls_frontdoor/mod.rs',[(36,130,'fixed snapshot types and engineering reserve')]),
  ('crates/fc-windows/src/tls_frontdoor/native.rs',[(35,39,'hard deadline minus25ms alarm'),(436,535,'original token close entry and actual socket return'),(965,992,'actual IOCP packet returns'),(738,780,'actual frame/context result return')]),
  ('crates/fc-windows/src/tls_frontdoor/native/accept.rs',[(127,159,'bare deadline cancellation keeps exact IOCP debt'),(250,291,'actual accept completion before release/repost')]),
  ('crates/fc-windows/src/tls_frontdoor/native/stream.rs',[(137,155,'reserve applies only Header phase'),(969,1020,'actual final IOCP SSPI Frame accounting before retirement')]),
  ('crates/fc-bootstrap/src/application/http_business_probe.rs',[(450,480,'fixed close snapshot original monotonic epoch serialization')]),
 ]
 refs=[]
 for relative,locations in source_refs:
  p=WORKTREE/relative;data=read(p);expected=sources[relative]
  check('reviewed source exact compiled bytes '+relative,len(data)==expected['bytes'] and sha(data)==expected['sha256'])
  refs.append({'path':str(p),'bytes':len(data),'sha256':sha(data),'lineReferences':[{'first':a,'last':b,'purpose':c} for a,b,c in locations]})
 observation=actual['headerDeadlineObservation']
 check('sample remains fixed5e9 epoch with gate not promoted',observation['thresholdNs']==THRESHOLD and observation['epoch']=='PER_CONNECTION_BEFORE_LOCAL_TCP_CONNECT_CALL' and observation['maximumClients']==5 and observation['slotMapping']=='NOT_PROVEN' and actual['strictP0']=='NOT_VALIDATED' and actual['productionListener']=='NOT_ENABLED' and actual['forwardTimingIsNativeDeadlineEvidence'] is False)
 events=actual['events'];cases=['header-deadline-tls-'+str(i) for i in range(1,5)]+['header-deadline-raw-tcp']
 check('exact five original unique client cases in actual order',[e['case'] for e in events]==cases)
 timing=[]
 for event in events:
  name=event['case'];start=event['beforeConnectNs'];end=event['peerCloseNs']
  check(name+' exact unrounded preconnect-to-peer<=5000000000ns',type(start) is int and type(end) is int and 0<=end-start<=THRESHOLD and event['elapsedNs']==end-start)
  check(name+' actual peer observation before any local close',event['status']=='PASS' and event['peerCloseObserved'] is True and event['localCloseUsedAsPeerEvidence'] is False and event['localSocketClosed'] is True and start<=event['localConnectReturnedNs']<=end<=event['localCloseBeganNs']<=event['localCloseReturnedNs'])
  check(name+' no complete head bytes or late app result',event['completeHeadSent'] is False and event['applicationReceivedBytes']==0 and event['sshMasterAliveAtPeerClose'] is True and event['nativeLauncherAliveAtPeerClose'] is True and 'error' not in event)
  if event['transport']=='TLS':
   check(name+' verified real TLS12 then incomplete first head',event['tlsVersion']=='TLSv1.2' and event['certificateVerified'] is True and event['peerDerSha256']==events[0]['peerDerSha256'] and event['localConnectReturnedNs']<=event['handshakeCompletedNs']<=event['incompleteHeadSentNs']<=end and event['peerCloseKind'] in ['TLS_RAGGED_EOF','TLS_CLEAN_EOF','TCP_RESET','TCP_ABORTED'])
  else:check(name+' raw connection sends no TLS/application data',event['transport']=='TCP' and event['sentBytes']==event['sendCalls']==event['dripCalls']==0 and event['peerCloseKind'] in ['TCP_EOF','TCP_RESET','TCP_ABORTED'])
  timing.append({'case':name,'beforeConnectNs':start,'peerCloseNs':end,'elapsedNs':end-start,'remainingTo5sNs':THRESHOLD-(end-start),'peerCloseKind':event['peerCloseKind'],'dripCalls':event['dripCalls']})
 check('four TLS incomplete intervals actually overlap',max(e['incompleteHeadSentNs'] for e in events[:4])<min(e['peerCloseNs'] for e in events[:4]) and observation['fourTlsIntervalsOverlap'] is True)
 check('fourth actual single-byte drips>=2 and other clients do not drip',events[3]['dripCalls']>=2 and all(e['dripCalls']==0 for e in events[:3]+events[4:]))
 check('first phase peer terminals all precede fifth raw connect',all(e['peerCloseNs']<events[4]['beforeConnectNs'] for e in events[:4]) and observation['sequentialPhasesProven'] is True)
 check('original client-events digest immutable',sha(json.dumps(events,sort_keys=True,separators=(',',':')).encode())==actual['clientEventsSha256'])
 raw={}
 check('original native raw retention PASS',actual['rawPreservation']['status']=='PASS')
 for name,info in actual['rawPreservation']['files'].items():
  p=Path(info['artifact']);check('raw input under exact R41 evidence '+name,p.is_relative_to(BASE) and p.resolve().is_relative_to(BASE))
  data=read(p);check('raw actual original hash and remote equality '+name,info['status']=='PASS' and info['remote']==info['downloaded'] and info['remote']['bytes']==len(data) and info['remote']['sha256']==sha(data) and base64.b64decode(info['base64'],validate=True)==data)
  raw[name]=data
 lines=raw['tls.stdout'].decode().splitlines()
 def records(prefix):return [json.loads(x[len(prefix):]) for x in lines if x.startswith(prefix)]
 progress=records('TLS_BUSINESS_PROGRESS ');closes=records('TLS_BUSINESS_CLOSE ');passes=records('TLS_BUSINESS_PASS ')
 check('all actual native progress and close copies exact',progress==actual['nativeProgress'] and closes==actual['nativeCloseObservations'])
 check('one original actual native PASS copy exact',len(passes)==1 and passes[0]==actual['native'] and actual['nativeReports']==passes)
 check('native snapshots unique paired generation and increasing sequence',len(progress)==len(closes)>0 and [x['sequence'] for x in progress]==[x['sequence'] for x in closes] and all(x['marker']==actual['marker'] and x['generation']==actual['ready'][0]['generation'] for x in progress+closes) and all(a['sequence']<b['sequence'] for a,b in zip(progress,progress[1:])))
 checkpoint=observation['fourRetiredBeforeRaw'];cp=checkpoint['progress'];cc=checkpoint['close']
 check('four-retired/rearm checkpoint actually in native originals',cp in progress and cc in closes and cp['sequence']==cc['sequence'])
 check('checkpoint no residual first-four socket owner and all Ready',cp['acceptance']['bound_slots']==0 and cp['acceptance']['ready_slots']==4 and cp['acceptance']['transfers']==4 and cp['retired']>=4 and cp['rearms']>=4 and cp['jsonBodyRecords']==cp['pendingJobs']==0 and cp['requests']==[] and cc['headerBegins']==cc['headerSocketReturns']==4 and len(cc['headers'])==4 and all(x['retirementConfirmedAtNs'] is not None for x in cc['headers']))
 check('checkpoint locally observed after four peers and before fifth TCP call',max(e['peerCloseNs'] for e in events[:4])<=checkpoint['observedAtNs']<=events[4]['beforeConnectNs'])
 unique={};bare={}
 for snapshot in closes:
  check('native fixed engineering alarm reserve every original snapshot',snapshot['reserveNs']==RESERVE and snapshot['headerFailures']==0)
  for entry in snapshot['headers']:
   key=(entry['generation'],entry['slot'],entry['serial'])
   if key in unique:
    prev=unique[key]
    check('same native token facts never change or regress '+str(key),all(v is None or entry[k]==v for k,v in prev.items()))
   unique[key]=entry
  if snapshot['accept'] is not None:bare[snapshot['accept']['serial']]=snapshot['accept']
 check('exactly five distinct native header-close tokens retained as group',len(unique)==5 and closes[-1]['headerBegins']==closes[-1]['headerSocketReturns']==5)
 native_intervals=[]
 for key,entry in unique.items():
  check('native header token original hard/alarm close entry return ordering '+str(key),entry['failed'] is False and entry['hardDeadlineNs']-entry['alarmNs']==RESERVE and 0<=entry['alarmNs']<=entry['beganAtNs']<=entry['socketCloseBeganAtNs']<=entry['socketReturnedAtNs']<=entry['hardDeadlineNs'])
  check('native header token actual IOCP Frame and physical retirement facts '+str(key),entry['retirementConfirmedAtNs'] is not None and entry['retirementConfirmedAtNs']>=entry['socketReturnedAtNs'] and (not entry['ioPendingAtClose'] or entry['ioReturnedAtNs'] is not None and entry['retirementConfirmedAtNs']>=entry['ioReturnedAtNs']) and (not entry['framePendingAtClose'] or entry['frameReturnedAtNs'] is not None and entry['retirementConfirmedAtNs']>=entry['frameReturnedAtNs']))
  native_intervals.append(entry)
 for serial,entry in bare.items():
  check('distinct bare negative close is still original hard/alarm '+str(serial),entry['hardDeadlineNs']-entry['alarmNs']==RESERVE and entry['failed'] is False and entry['socketCloseBeganAtNs'] is not None and entry['socketReturnedAtNs'] is not None and entry['beganAtNs']<=entry['socketCloseBeganAtNs']<=entry['socketReturnedAtNs']<=entry['hardDeadlineNs'] and entry['completionReturnedAtNs'] is not None)
 check('no per-client native slot mapping claimed',actual['headerDeadlineNativeCorrelation']['socketReturnIsPeerClose'] is False and actual['headerDeadlineNativeCorrelation']['rawTcpCapacityRejects']==0 and actual['headerDeadlineNativeCorrelation']['originalStrictP0']=='NOT_VALIDATED')
 last=progress[-1];final=passes[0]
 check('five transfers zero capacity rejects and no outstanding native resources',last['acceptance']['transfers']==5 and last['acceptance']['capacity_rejects']==0 and last['acceptance']['bound_slots']==last['acceptance']['native_pending']==last['acceptance']['owned_sockets']==0 and last['pendingJobs']==last['jsonBodyRecords']==0 and last['requests']==[])
 check('original2 workers join actual native drain and Hubs0',final['physicalNativeDrain'] is True and final['sharedWorkers']==2 and final['workerJoin'] is True and final['setupCleanup'] is True and all(final[k]==0 for k in ['hubPending','downloadHubPending','quarantined','approvals','heads','responses']))
 debug=[x for x in lines if x.startswith('TLS_BUSINESS_REPORT ')]
 check('raw debug confirms no jobs active records cleanup owners and actual native drain',len(debug)==1 and all(re.search(p,debug[0]) for p in [r'active_records: 0,',r'pending_jobs: 0,',r'cleanup_pending: 0,',r'retained_cleanup: 0,',r'native_drained: true,']))
 tunnel_stdout=stream(actual['sshTunnelRaw']['stdout'],'original SSH stdout')
 tunnel_stderr=stream(actual['sshTunnelRaw']['stderr'],'original SSH stderr')
 live=observation['sshDirectTcpip'];length=live['stderrBytesObserved'];prefix=tunnel_stderr[:length]
 check('actual live-at-sample SSH stderr prefix bound to full original',0<length<=len(tunnel_stderr) and sha(prefix)==live['stderrPrefixSha256'] and live['masterAlive'] is True and live['nativeLauncherAlive'] is True)
 prefix_channels,prefix_cleanup=ssh_lifecycles(prefix,'observed-live SSH prefix')
 check('independently parsed five direct lifecycles match original live observation',prefix_channels==live['channels'] and prefix_cleanup==live['normalChannelCleanup'])
 full_channels,full_cleanup=ssh_lifecycles(tunnel_stderr,'complete original SSH transcript')
 check('full transcript retains same five direct open-confirm/EOF lifecycles',len(full_channels)==5 and all(a['newLine']==b['newLine'] and a['openConfirmedLine']==b['openConfirmedLine'] and a['remoteEofLine']==b['remoteEofLine'] for a,b in zip(full_channels,prefix_channels)))
 check('original native process actualexit0 and owned SSH cleanup',actual['nativeProcessExit']=={'code':0} and actual['nativeProcessExits']==[{'code':0}] and actual['ownedTunnelCleanup']=='PASS' and actual['ownedProcessCleanup']['status']=='PASS' and actual['ownedProcessCleanup']['ownedProcessesRemaining']==0)
 cleanup=actual['cleanupDetails']
 check('fixture CNG/MY owned directory and processes actual cleanup without system mutation',cleanup['status']=='PASS' and cleanup['rootAbsent'] is True and cleanup['cngKeyExists'] is False and cleanup['myDigestRestored'] is True and cleanup['ownedProcessesRemaining']==0 and all(cleanup[k]=='UNTOUCHED' for k in ['sshConfiguration','rootTrustStore','firewall','httpSys']))
 check('all original remote control command terminal0',all(x['exitCode']==0 for x in actual['remoteControlRunsRaw']))
 check('all reviewed original files unchanged after independent review',all(Path(x['path']).stat().st_size==x['bytes'] and sha(Path(x['path']).read_bytes())==x['sha256'] for x in inputs.values()))
 r.update(status='PASS_FINITE_SAMPLES_ORIGINAL_STRICT_P0_NOT_VALIDATED',exeSha256=EXE_SHA,runnerSha256=RUNNER_SHA,
  thresholdNs=THRESHOLD,reserveNs=RESERVE,clientIntervals=timing,
  maximumClientIntervalNs=max(x['elapsedNs'] for x in timing),clientCount=5,
  fourTlsOverlap=True,fourRetiredRearmedBeforeRaw=True,dripCalls=events[3]['dripCalls'],
  nativeTokenGroup={'distinctClosedTokens':5,'records':native_intervals,'perClientSlotMapping':'NOT_PROVEN_NOT_ATTEMPTED','ioPendingAtCloseCount':sum(x['ioPendingAtClose'] for x in native_intervals),'actualIoReturnedCount':sum(x['ioReturnedAtNs'] is not None for x in native_intervals),'framePendingAtCloseCount':sum(x['framePendingAtClose'] for x in native_intervals),'actualFrameReturnedCount':sum(x['frameReturnedAtNs'] is not None for x in native_intervals),'retirementConfirmedCount':sum(x['retirementConfirmedAtNs'] is not None for x in native_intervals),'distinctBareCloseObservations':len(bare)},
  sshEvidence={'originalCompleteStderrBytes':len(tunnel_stderr),'sha256':sha(tunnel_stderr),'livePrefixBytes':length,'livePrefixSha256':sha(prefix),'directInstances':full_channels,'livePrefixNormalCleanup':prefix_cleanup,'fullPostBoundaryCleanup':full_cleanup,'scope':'Original SSH original-prefix liveness observations and exact same-instance open-confirm/remoteEOF; not per-client kernel timestamp mapping.'},
  nativeFinalReceipt=final,lastNativeProgress=last,compiledSourceReferences=refs,
  originalFirstFailure={'path':str(BASE/'windows-header-deadline-first.json'),'status':first['status'],'cleanup':first['cleanup'],'sha256':inputs[str(BASE/'windows-header-deadline-first.json')]['sha256']},
  inputs=list(inputs.values()),helper={'path':str(Path(__file__)),'bytes':Path(__file__).stat().st_size,'sha256':sha(Path(__file__).read_bytes())},
  limitations=['Only five actually observed conservative pre-local-TCP-connect to peer-terminal samples, without rounding or tolerance; no arbitrary scheduling hard-real-time proof.',
   'No Windows kernel ESTABLISHED timestamp or backlog age bound; old original strictP0 remains NOT_VALIDATED/FAIL and ordinary production listener remains disabled.',
   'Native times share the finite server monotonic epoch; client ns share local perf_counter epoch. No cross-machine clock alignment or per-client/native-slot association is asserted.',
   '25ms is an earlier engineering alarm relative to unchanged original post-derived hard deadline, not a new TCP-origin definition.',
   'All five header closes had actual IOCP debt returned then physical retirement. Frame-pending-at-close count is recorded faithfully; zero means this run does not separately exercise held-Frame retirement.',
   'Native socket close-return is not peer EOF or IOCP release; client EOF/TLS terminal and later native packets/retirement are separate observations.',
   'Original liveness flags are the frozen instrumented poll observations; this read-only check does not restart or re-poll already finished processes.',
   'Public certificate only; no playback, microphone, authentication/complete application request, product deployment, Win7/10 or visible Seewo acceptance claimed.'])
except BaseException as error:r.update(status='FAIL_INDEPENDENT_AUDIT',errorKind=type(error).__name__,failedCheck=str(error) if isinstance(error,AssertionError) else 'Original evidence invariant or shape failed',inputs=list(inputs.values()))
with OUT.open('x') as f:f.write(json.dumps(r,ensure_ascii=False,indent=2)+'\n')
print(json.dumps({'status':r['status'],'checks':len(r['checks']),'clientElapsedNs':[x['elapsedNs'] for x in r.get('clientIntervals',[])],'maxNs':r.get('maximumClientIntervalNs'),'report':{'path':str(OUT),'bytes':OUT.stat().st_size,'sha256':sha(OUT.read_bytes())}},ensure_ascii=False))
raise SystemExit(0 if r['status']=='PASS_FINITE_SAMPLES_ORIGINAL_STRICT_P0_NOT_VALIDATED' else 1)
