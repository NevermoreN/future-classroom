from pathlib import Path
import subprocess,hashlib,json
R=Path('/tmp/fc-round65-header-expiry');E=R/'evidence';W=R/'worktree'
paths=['crates/fc-bootstrap/src/web_transfer/network.rs','crates/fc-windows/src/tls_frontdoor/native.rs','crates/fc-windows/src/tls_frontdoor/native/accept.rs','crates/fc-windows/src/tls_frontdoor/native/stream.rs','crates/fc-windows/src/tls_frontdoor/native/stream/tests.rs']
def info(p):
 b=p.read_bytes();return {'path':str(p),'bytes':len(b),'sha256':hashlib.sha256(b).hexdigest()}
def put(p,b):
 with p.open('xb') as f:f.write(b)
commands=[]
for name,args in [('fmt-check',['rustfmt','+1.97.1','--edition','2021','--config','skip_children=true','--check',*paths]),('diff-check',['git','diff','--check']),('status',['git','status','--porcelain=v1','--untracked-files=all']),('diff',['git','diff','--',*paths])]:
 q=subprocess.run(args,cwd=W,capture_output=True,timeout=60)
 for k,b in [('stdout',q.stdout),('stderr',q.stderr)]:put(E/(name+'-first.'+k+'.log'),b)
 commands.append({'argv':args,'cwd':str(W),'actualExit':True,'exitCode':q.returncode,'stdout':info(E/(name+'-first.stdout.log')),'stderr':info(E/(name+'-first.stderr.log'))});assert q.returncode==0
status=(E/'status-first.stdout.log').read_text().splitlines();assert sorted(x[3:] for x in status)==paths and all(x.startswith(' M ') for x in status)
rows=[]
for n in paths:
 p=W/n;q=E/'source-after'/n;q.parent.mkdir(parents=True,exist_ok=True);put(q,p.read_bytes());rows.append({'relativePath':n,'before':info(E/'source-before'/n),'after':info(q),'worktreeMatchesSnapshot':p.read_bytes()==q.read_bytes()})
tests=['real_bare_post_charges_preparation_against_one_captured_origin','real_bare_expired_preparation_closes_socket_without_posting_io','real_bare_late_native_return_retains_charged_io_until_original_packet','real_iocp_batch_services_due_receive_before_another_top_level_poll','caller_deadline_service_keeps_original_head_request_budget_and_admission']
r={'status':'FROZEN_SOURCE_SCOPED_FORMAT_DIFF_PASS_COMPILE_AND_RUNTIME_NOT_RUN','base':'0f2bec38c9bd0442ae269225b13c3a6ce3aec0db','branch':'codex/header-expiry','worktree':str(W),'source':rows,'commands':commands,'design':info(E/'design-readonly-first.json'),
'implemented':[
'Bare post captures origin before socket/OVERLAPPED preparation. If alarm consumed before AcceptEx, closes only unsubmitted proposed socket and returns error, with no invented IOCP debt. Native success/PENDING always charges serial/state/submission before late negative cancellation; no +5 after native return.',
'Private production post clock always uses Instant::now; test injected origin/clock is private to native owner tests and never remote input. No new syscall clock service/thread/pool.',
'One service_deadlines operation performs only existing negative expiry against fixed bare+4 business owners. Original pending IOCP/Frame ledger, physical retirement and sticky failure rules remain; it neither drains nor rearms.',
'Native sweep boundaries: poll entry; before each of at most5 IOCP packet dequeues, after IOCP batch; before/after each of at most4 Frame deliveries; before each of4 native retirement submissions and final boundary.',
'Network sweep boundaries: before each of at most16 native event deliveries, before each of8 records, after each worker delivery, before each of8 record retire/rearm checks, final. Record is restored before propagating a failure inside its processing closure. No auth/lease/version/body admission relaxation.'],
'boundedWork':{'perServiceOwnerChecks':5,'nativePollMaxServiceCalls':20,'networkAdditionalMaxServiceCalls':41,'combinedMaxOwnerChecks':305,'interpretation':'Finite count, not an elapsed-time/OS-native-call worst-case bound. Subsequent checks skip Closed/Closing owners; no repeated pending debt discharge or unbounded allocation.'},
'newTests':{'count':5,'names':tests,'scope':'Actual Winsock/IOCP and actual SSPI worker mailbox with existing fake record Driver; controlled monotonic time inputs where stated. Not real cryptography, ESTABLISHED timestamp or a claimed new strict5s run.','counterexampleBasis':'Before change, a budget origin minted after native call could not charge preparation/post latency; late pending native return must still be charged/closed. Before change, direct poll_io did no expiry scan, leaving a due receive live until another top-level poll.','execution':'NOT_RUN_ROOT_WILL_BUILD_AND_RUN'},
'semanticsKept':['HEADER_CLOSE_RESERVE remains25ms','Hard header budget remains5_000_000_000ns, never accept-observed+5','Complete valid Head still establishes the same original120s request interval; body admission only tightens it','4 business slots/1 bare owner/4 Frame/5native IO/6sockets and original shared2 business workers unchanged','Cancel/close is not IOCP/SSPI release; original Close/Retired evidence remains','No production listener enablement or new API/auth permission','No main/installer/CI/docs modification'],
'notSolved':['A backlog connection may already be ESTABLISHED before a fresh bare post; this is not reconstructed or hidden.','No documented exact TCP age from SO_CONNECT_TIME seconds.','Arbitrary Windows scheduling/suspend/native call/network delivery cannot be made a hard-real-time guarantee by these bounded user-mode checks.','Original R13 +577us FAIL remains; R41 finite passing samples remain separate.'],
'pendingValidation':['Root full compile/strictClippy/host and actual Windows tests for5new+existing ownership regressions','Real finite Schannel incomplete-head4+raw1 strict5e9 samples, original timing validator/client budget/negative fixtures unchanged','Continuous capacity/body/upload/download/auth/business/browser regression at original budgets','MSVC/Win7/10/visible/production gates remain independent'],
'notRun':['Cargo/compile','Windows/SSH/network client','CI','product listeners','main source changes','commit/push']}
put(E/'implementation-freeze-first.json',(json.dumps(r,ensure_ascii=False,indent=2)+'\n').encode())
print(json.dumps({'freeze':info(E/'implementation-freeze-first.json'),'patch':info(E/'diff-first.stdout.log'),'files':len(rows),'newTests':5,'status':r['status']},ensure_ascii=False))
