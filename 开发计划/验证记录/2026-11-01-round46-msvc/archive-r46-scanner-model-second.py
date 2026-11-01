#!/usr/bin/env python3
"""Pure in-memory scanner models; no archive/SSH/native execution."""
import base64, hashlib, importlib.util, json
from pathlib import Path
E=Path('/tmp/fc-round46-ci-evidence');p=E/'archive-r46.py'
spec=importlib.util.spec_from_file_location('r46_archive',p);m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
cases=[]
def run(name,text=None,obj=None,blocked=True,retired=False,synthetic=False):
 s=m.Scanner()
 if text is not None:s.text(text,'model/'+name,retired=retired,synthetic=synthetic)
 if obj is not None:s.walk(obj,'model/'+name,retired=retired,synthetic=synthetic)
 assert bool(s.blocked)==blocked,name
 cases.append({'case':name,'expectedBlocked':blocked,'findingKinds':sorted({x['kind'] for x in s.blocked}),'annotationKinds':sorted({x['kind'] for x in s.annotations})})
run('benign_source',text='print("synthetic test")',blocked=False)
run('public_certificate',text='-----BEGIN '+'CERTIFICATE-----\nPUBLIC\n-----END CERTIFICATE-----',blocked=False)
run('private_pem',text='-----BEGIN '+'PRIVATE KEY-----\nMODEL\n-----END PRIVATE KEY-----')
run('github_pattern',text='ghp'+'_'+'A'*32)
run('api_pattern',text='sk'+'-'+'A'*35)
cookie='__Host-FutureClassroom='+'0123456789abcdef'*4
run('real_cookie_review',text=cookie)
run('explicit_retired_synthetic_cookie',text=cookie,blocked=False,synthetic=True)
run('repetitive_fixture_cookie',text='__Host-FutureClassroom='+'a'*64,blocked=False)
run('signed_url',text='https://example.invalid/object?'+'sig='+'A'*32)
run('json_unknown_password',obj={'password':'NonRealModelPassword-Only'} )
run('retired_exact_stop',obj={'stopCapability':'0123456789abcdef'*4},blocked=False,retired=True)
run('unproven_stop',obj={'stopCapability':'0123456789abcdef'*4})
encoded=base64.b64encode(json.dumps({'password':'NonRealModelPassword-Only'}).encode()).decode()
run('recursive_json_base64_password',obj={'stdoutBase64':encoded})
utf16=base64.b64encode(('-----BEGIN '+'PRIVATE KEY-----').encode('utf-16le')).decode()
run('recursive_ps_utf16_private_pem',text='powershell -EncodedCommand '+utf16)
nested={'message':'benign'}
for _ in range(10):nested={'payloadBase64':base64.b64encode(json.dumps(nested).encode()).decode()}
run('nested_encoding_explicit_depth_bound',obj=nested)
for path in ['/config/.local/share/future-classroom-test-access/win11-192.168.10.212','/config/.ssh/id_ed25519','/tmp/owned.control.json','/tmp/model.pfx']:
 assert m.sensitive_path(Path(path))
assert not m.sensitive_path(Path('/tmp/fc-round46-ci-evidence/.owner'))
report={'status':'PASS_PURE_MEMORY_SCANNER_MODELS_ONLY','helperSha256':hashlib.sha256(p.read_bytes()).hexdigest(),'cases':cases,'privateAndControlPathsClassifiedWithoutOpening':4,'ownerMarkerIncluded':True,'scope':'No real credential/key read or hashed; dummy input strings only; no files copied/main modified/native/SSH/Cargo/cleanup.'}
out=E/'archive-r46-scanner-model-second.json'
with out.open('x') as f:json.dump(report,f,indent=2);f.write('\n')
print(json.dumps({'status':report['status'],'cases':len(cases),'report':str(out)}))
