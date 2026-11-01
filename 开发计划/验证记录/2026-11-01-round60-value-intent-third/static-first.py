from pathlib import Path
import subprocess,json,hashlib,struct,shutil,difflib,ast
P=Path(__file__).parent

def info(p):
 b=p.read_bytes();return {'path':str(p),'bytes':len(b),'sha256':hashlib.sha256(b).hexdigest()}
def put(p,b):
 with p.open('xb') as f:f.write(b)
source=P/'registry-value-intent.c';exe=P/'registry-value-intent.exe';before=info(exe);compile=json.loads((P/'compile-second.json').read_bytes());assert compile['source']==info(source) and compile['executable']==before and compile['exitCode']==0
rows=[];tool=shutil.which('i686-w64-mingw32-objdump');assert tool
for n,a in [('objdump',[tool,'-p',str(exe)]),('objdump-version',[tool,'--version']),('compiler-version',['i686-w64-mingw32-gcc','--version'])]:
 q=subprocess.run(a,cwd=P,capture_output=True,timeout=30)
 for k,b in [('stdout',q.stdout),('stderr',q.stderr)]:put(P/(n+'.'+k+'.log'),b)
 rows.append({'argv':a,'cwd':str(P),'actualExit':True,'exitCode':q.returncode,'stdout':info(P/(n+'.stdout.log')),'stderr':info(P/(n+'.stderr.log'))});assert q.returncode==0
raw=(P/'objdump.stdout.log').read_text();import re
dlls=re.findall(r'DLL Name:\s*(\S+)',raw);assert {d.lower() for d in dlls}=={'advapi32.dll','kernel32.dll','ktmw32.dll'}
b=exe.read_bytes();p=struct.unpack_from('<I',b,0x3c)[0];assert b[p:p+4]==b'PE\0\0' and struct.unpack_from('<H',b,p+4)[0]==0x14c and struct.unpack_from('<H',b,p+24)[0]==0x10b and struct.unpack_from('<H',b,p+24+68)[0]==3
s=source.read_text();candidate=s[s.index('static DWORD run_case('):s.index('\n/* Cleanup is a separate')];reader=s[s.index('static DWORD reader('):s.index('static BOOL rollback(')];runner=(P/'run-windows-probe.ps1').read_text()
checks={
'no_tx_target_read_in_candidate':'query_string(parent' not in candidate and 'query_raw(parent' not in candidate and 'RegQueryValue' not in candidate,
'candidate_stages_fixed_expected': 'set_string(parent,L"FutureClassroom",ORIGINAL,"tx_stage_expected_return")' in candidate,
'write_then_independent_reader': candidate.index('tx_stage_expected_return')<candidate.index('start_child(L"--reader"'),
'reader_has_no_transaction_access':all(w not in reader for w in ['Transacted','CreateTransaction','RegSetValue','set_string']),
'reader_ordinary_read_and_typed_exact_compare':'query_raw(key,L"FutureClassroom"' in reader and 'kind==2?0:24' in reader,
'read_mismatch_forces_rollback':'if(read_exit==24){mismatch=TRUE;' in candidate and 'COMMITTED_PREIMAGE_MISMATCH_ROLLBACK_REQUIRED' in candidate,
'reader_block_never_falls_back': 'FAIL_FRESH_COMMITTED_READ_BLOCKED_UNDER_WRITE_INTENT_NO_FALLBACK' in candidate and 'reader_wait_AFTER_rollback_NOT_A_COMPARISON' in candidate,
'binary_and_distinct_initial_cases': 'c==\'B\'?REPLACEMENT:ORIGINAL' in s and 'REG_BINARY,BINARY_ORIGINAL' in s,
'no_intent_red_control':"const BOOL intent=which!='E'" in candidate,
'cancel_before_and_after_delete': 'EXPLICIT_CANCEL_AFTER_INTENT_AND_EXTERNAL_COMPARE' in candidate and 'EXPLICIT_CANCEL_AFTER_STAGED_DELETE' in candidate,
'original_deadline_inherited_not_reminted': 'value=operation_deadline' in s and 'operation_deadline=operation_deadline*10+digit' in s and 'transaction_deadline<operation_deadline' in s,
'no_zero_timeout_infinite_transaction':'DWORD tx_timeout=remaining(3000);if(!tx_timeout)' in candidate,
'late_native_commit_never_positive': 'LATE_NATIVE_COMMIT_RETURN_NOT_POSITIVE_DELIVERY' in candidate,
'actual_process_creation_identity':'GetProcessTimes(process' in s and 'original_creation_high' in s,
'physical_job_zero_before_cleanup':'if(all_empty){result=supervised(L"--cleanup"' in s and 'counts.ActiveProcesses==0' in s,
'exact_leaf_realnew_nonce_sid': 'REG_CREATED_NEW_KEY' in s and 'same_owner_sid(root)' in s and 'PREPARE_NOT_SUCCESSFULLY_COMPLETED_RETAIN_ROOT_NO_CLEANUP' in s,
'no_real_run_or_other_state':'CurrentVersion' not in s and '#define PREFIX L"FutureClassroom-RegistryWriteIntent-Test-"' in s,
'runner_exact_pin_and_original_90sec':before['sha256'] in runner and 'originalDeadlineMillis=90000' in runner,
}
assert all(checks.values()),checks
put(P/'second-to-third.patch',''.join(difflib.unified_diff((P.parent/'scratch-fixture-second/source-final/registry-write-intent.c').read_text().splitlines(True),s.splitlines(True),fromfile='preserved-second/registry-write-intent.c',tofile='third/registry-value-intent.c')).encode())
assert info(exe)==before
r={'status':'PASS_ACTUAL_LOCAL_BUILD_AND_STATIC_ONLY_NATIVE_WINDOWS_NOT_RUN','scope':'Actual gcc and objdump;18 source-text invariants are not native concurrency tests.','source':info(source),'executable':before,'runner':info(P/'run-windows-probe.ps1'),'producer':info(P/'compile-second.json'),'commands':rows,'pe':{'machine':'PE32 x86','subsystem':3,'imports':dlls},'sourceTextChecks':checks,'PowerShellParser':'NOT_RUN_NO_LOCAL_PWSH','windows':'NOT_RUN'}
put(P/'static-first.json',(json.dumps(r,ensure_ascii=False,indent=2)+'\n').encode());print(json.dumps({'report':info(P/'static-first.json'),'status':r['status'],'checks':len(checks)},ensure_ascii=False))
