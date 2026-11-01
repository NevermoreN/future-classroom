from pathlib import Path
import json,hashlib,subprocess,shutil,struct,ast,difflib
P=Path(__file__).parent

def identity(p):
 b=p.read_bytes();return {'path':str(p),'bytes':len(b),'sha256':hashlib.sha256(b).hexdigest()}
def put(p,b):
 with p.open('xb') as f:f.write(b)
exe=P/'registry-write-intent.exe';source=P/'registry-write-intent.c';before=identity(exe);tool=shutil.which('i686-w64-mingw32-objdump');assert tool
rows=[]
for name,args in [('objdump',[tool,'-p',str(exe)]),('objdump-version',[tool,'--version']),('compiler-version',['i686-w64-mingw32-gcc','--version'])]:
 q=subprocess.run(args,cwd=P,capture_output=True,timeout=30);put(P/(name+'-first.stdout.log'),q.stdout);put(P/(name+'-first.stderr.log'),q.stderr)
 rows.append({'argv':args,'cwd':str(P),'actualExit':True,'exitCode':q.returncode,'stdout':identity(P/(name+'-first.stdout.log')),'stderr':identity(P/(name+'-first.stderr.log'))});assert q.returncode==0
raw=(P/'objdump-first.stdout.log').read_text();b=exe.read_bytes();off=struct.unpack_from('<I',b,0x3c)[0]
assert b[off:off+4]==b'PE\0\0' and struct.unpack_from('<H',b,off+4)[0]==0x14c and struct.unpack_from('<H',b,off+24)[0]==0x10b and struct.unpack_from('<H',b,off+24+68)[0]==3
import re
dlls=re.findall(r'DLL Name:\s*(\S+)',raw);assert set(d.lower() for d in dlls)=={'advapi32.dll','kernel32.dll','ktmw32.dll'},dlls
s=source.read_text();ps=(P/'run-windows-probe.ps1').read_text()
checks={
'eight_fixed_cases':"c<='H'" in s and "which<='H'" in s,
'same_value_and_distinct_writer':"(which=='F'||which=='G')?REPLACEMENT:ORIGINAL" in s,
'intent_after_read_has_own_cases':"has_intent&&!before_read" in s and s.index('tx_read_target')<s.index('has_intent&&!before_read'),
'intent_before_read_has_own_cases':"if(has_intent&&before_read)" in s and s.index('if(has_intent&&before_read)')<s.index('tx_read_target'),
'early_child_deleted_before_writer':s.index('tx_delete_intent_BEFORE_writer')<s.index('if(has_writer){'),
'known_writer_exit_before_target_delete':s.index('ordinary_actual_exit_BEFORE_target_delete')<s.index('tx_delete_target_begin'),
'new_disposition_root_and_intent':s.count('REG_CREATED_NEW_KEY')>=3,
'nonce_and_current_sid_root': 'OwnerSid' in s and 'same_owner_sid(root)' in s,
'prepare_uncertain_never_cleanup': 'PREPARE_NOT_SUCCESSFULLY_COMPLETED_RETAIN_ROOT_NO_CLEANUP' in s,
'all_cases_jobs_physical_empty_before_cleanup': 'all_empty=all_empty&&empty' in s and 'if(all_empty){result=supervised(L"--cleanup"' in s,
'actual_process_handles_creation_identity': 'GetProcessTimes(process' in s and 'original_creation_low' in s and 'supervisor_actual_process_exit' in s,
'fixed_single_leaf_no_run': '#define PREFIX L"FutureClassroom-RegistryWriteIntent-Test-"' in s and 'CurrentVersion' not in s,
'runner_exact_exe_pin':before['sha256'] in ps,
'runner_no_registry_compensation':'RegistryAccessByRunner' not in ps and 'registryAccessByRunner=\'NONE\'' in ps and 'DeleteSubKey' not in ps,
'runner_no_pid_reacquisition': 'GetProcessById' not in ps,
'runner_no_kill': all(k not in ps for k in ['Stop-Process','TerminateProcess','Kill()','taskkill']),
}
assert all(checks.values()),checks
assert before==identity(exe)
put(P/'old-to-second-source.patch',''.join(difflib.unified_diff((P.parent/'scratch-fixture/registry-write-intent.c').read_text().splitlines(True),s.splitlines(True),fromfile='preserved-first/registry-write-intent.c',tofile='second/registry-write-intent.c')).encode())
r={'status':'PASS_LOCAL_CROSS_COMPILE_AND_STATIC_ONLY_WINDOWS_NOT_RUN','scope':'Actual gcc exit and PE/import inspection plus source-text invariants. No model is a native TxR outcome. PowerShell syntax has NOT been parsed by PowerShell.','source':identity(source),'executable':before,'runner':identity(P/'run-windows-probe.ps1'),'compile':identity(P/'compile-second.json'),'pe':{'machine':'0x014c','magic':'0x010b','subsystem':3,'imports':dlls},'commands':rows,'textChecks':checks,'unchangedAfterInspection':True,'windows':'NOT_RUN','PowerShellParser':'NOT_RUN_NO_LOCAL_PWSH'}
put(P/'static-first.json',(json.dumps(r,ensure_ascii=False,indent=2)+'\n').encode());print(json.dumps({'status':r['status'],'report':identity(P/'static-first.json'),'exe':before,'textCheckCount':len(checks)},ensure_ascii=False))
