#!/usr/bin/env python3
"""Pure fake-data validation of the frozen local R46 evidence helper."""
import copy, hashlib, importlib.util, json
from pathlib import Path
EV=Path('/tmp/fc-round46-ci-evidence')
p=EV/'verify-runtime-and-archive.py';spec=importlib.util.spec_from_file_location('r46_verifier',p);v=importlib.util.module_from_spec(spec);spec.loader.exec_module(v)
sha='a'*64; normal='b'*64; meta='c'*64
expected={f'synthetic_{i}.exe':{'sha256':sha} for i in range(21)}
audit={'normalExecutable':{'producer':{'sha256':normal},'localPath':'/synthetic/product.exe'}}
metadata={'sha256':meta}
command={'status':'PASS_COMMAND','exitCode':0,'actualExit':True,'scriptUnchanged':True,'sourceSha':v.SOURCE,'inputNormalSha256':normal,'metadataSha256':meta}
output=['running 1 test','test synthetic ... ok','test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s']
report={'status':'PASS','cleanup':'PASS','win7':'NOT_VALIDATED','testTimeoutSeconds':60,'exeSha256':normal,'platform':{'caption':'Microsoft Windows 11 synthetic','build':'26300'},'tests':[{'name':name,'exitCode':0,'binarySha256':sha,'timeoutSeconds':60,'output':output.copy()} for name in expected],'uploadedArtifacts':[{'name':'product.exe','sha256':normal}]+[{'name':name,'sha256':sha} for name in expected],'selfCheck':{'exitCode':0,'output':[]},'system':{'exitCode':0,'output':[]},'native':{'exitCode':0,'hostExitCode':0,'sessionId':1,'output':[],'hostOutput':[]}}
assert v.validate_runtime(audit,report,command,expected,metadata)['passedTests']==21
cases=[]
mutations=[('nonterminal_command',lambda r,c:c.update(status='RUNNING')),('command_not_exited',lambda r,c:c.update(actualExit=False)),('wrong_metadata',lambda r,c:c.update(metadataSha256='0'*64)),('runtime_failed',lambda r,c:r.update(status='FAIL')),('cleanup_failed',lambda r,c:r.update(cleanup='FAIL')),('normal_sha_changed',lambda r,c:r.update(exeSha256='0'*64)),('test_sha_changed',lambda r,c:r['tests'][0].update(binarySha256='0'*64)),('missing_test',lambda r,c:r['tests'].pop()),('duplicate_test',lambda r,c:r['tests'].__setitem__(1,copy.deepcopy(r['tests'][0]))),('uploaded_sha_changed',lambda r,c:r['uploadedArtifacts'][0].update(sha256='0'*64)),('timeout_even_with_pass_status',lambda r,c:r.update(timedOutTest={})),('wrong_os_scope',lambda r,c:r['platform'].update(caption='Microsoft Windows 7 synthetic')),('formal_win7_upgrade',lambda r,c:r.update(win7='VALIDATED')),('hidden_host_failed',lambda r,c:r['native'].update(hostExitCode=1)),('noninteractive_session',lambda r,c:r['native'].update(sessionId=0))]
for name,mutate in mutations:
 r=copy.deepcopy(report);c=copy.deepcopy(command);mutate(r,c)
 try:v.validate_runtime(audit,r,c,expected,metadata)
 except ValueError:cases.append({'name':name,'rejected':True})
 else:raise RuntimeError('negative model unexpectedly accepted: '+name)
result={'status':'PASS_PURE_RUNTIME_IDENTITY_MODELS_ONLY','helperSha256':hashlib.sha256(p.read_bytes()).hexdigest(),'positive':1,'negative':cases,'limitations':['Synthetic in-memory reports only; no real executable, Windows, SSH, CI, Cargo, cleanup or runtime pass is implied.']}
out=EV/'runtime-independent-identity-model-first.json'
with out.open('x') as f:json.dump(result,f,indent=2);f.write('\n')
print(json.dumps({'status':result['status'],'negativeCases':len(cases),'report':str(out)}))
