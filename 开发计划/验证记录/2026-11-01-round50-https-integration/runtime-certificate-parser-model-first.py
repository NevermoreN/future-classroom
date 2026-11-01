#!/usr/bin/env python3
"""Local immutable-original replay plus injected parser-negative models only."""
from pathlib import Path
import base64,copy,hashlib,importlib.util,json
E=Path('/tmp/fc-round50-https-integration-evidence');p=E/'verify-runtime-certificate-second.py';spec=importlib.util.spec_from_file_location('r50review',p);m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
actual=json.loads((E/'windows-https-certificate-native-first.json').read_text());contract=json.loads(m.CONTRACT.read_text());child=Path(actual['rawPreservation']['files']['tls.stdout']['artifact']).read_bytes();fwd=base64.b64decode(actual['remoteFileScriptRuns'][1]['stdout']['base64']);wire=m.validate_wire(child,fwd,contract);assert wire['exactSteps']==22
cases=[]
def rejected(name,c=child,f=fwd,k=contract):
 try:m.validate_wire(c,f,k)
 except (ValueError,KeyError):cases.append({'case':name,'rejected':True})
 else:raise RuntimeError('bad wire model accepted: '+name)
rejected('missing_first_step',b'\n'.join(child.splitlines()[1:]),fwd)
rejected('duplicate_step',child+child.splitlines()[0]+b'\n',fwd)
rejected('grant_readiness',child.replace(b'NOT_GRANTED',b'GRANTED'),fwd)
rejected('wrong_worker_count',child.replace(b'shared_workers=2',b'shared_workers=3'),fwd)
rejected('supervisor_overrun',child.replace(b'elapsed_ms=443',b'elapsed_ms=90001'),fwd)
rejected('nonzero_original_process',child,fwd.replace(b'"code":0',b'"code":1'))
rejected('changed_forwarded_child',child,fwd.replace(b'index=0 ',b'index=9 ',1))
rejected('fact_boolean_not_error_int',child.replace(b'"nativeError":null',b'"nativeError":true',1),fwd)
rejected('missing_fact_key',child.replace(b'"found":',b'"wrong":',1),fwd)
flat={**actual['rawPreservation']['files']['tls.stdout']['remote'],'base64':actual['rawPreservation']['files']['tls.stdout']['base64']};assert m.checked_raw(flat,'model')==child
bad={**flat,'sha256':'0'*64}
try:m.checked_raw(bad,'model')
except ValueError:cases.append({'case':'raw_sha_mismatch','rejected':True})
else:raise RuntimeError('bad raw SHA model accepted')
runtime=json.loads((E/'windows-gnu-runtime-first.json').read_text());zero=next(t for t in runtime['tests']if any('0 passed;'in v for v in t['output']));assert m.validate_summary(zero)['passed']==0
for label,change in [('failed',lambda x:x['output'].__setitem__(-1,x['output'][-1].replace('0 failed','1 failed'))),('ignored',lambda x:x['output'].__setitem__(-1,x['output'][-1].replace('0 ignored','1 ignored'))),('filtered',lambda x:x['output'].__setitem__(-1,x['output'][-1].replace('0 filtered','1 filtered')))]:
 bad=copy.deepcopy(zero);change(bad)
 try:m.validate_summary(bad)
 except ValueError:cases.append({'case':label,'rejected':True})
 else:raise RuntimeError('bad test model accepted')
result={'status':'PASS_LOCAL_PARSER_REPLAY_AND_NEGATIVE_MODELS_ONLY','helperSha256':hashlib.sha256(p.read_bytes()).hexdigest(),'wireOriginalSha256':hashlib.sha256(child).hexdigest(),'positiveCases':3,'negativeCases':cases,'scope':'Read-only original wire/libtest replay and injected malformed copies only; no new Windows/CryptoAPI/native/SSH/Cargo execution.'}
out=E/'runtime-certificate-parser-model-first.json'
with out.open('x')as f:json.dump(result,f,indent=2);f.write('\n')
print(json.dumps({'status':result['status'],'negative':len(cases),'helperSha256':result['helperSha256']}))
