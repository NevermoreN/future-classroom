"""Pure local malformed-evidence models. Never launches tests, browsers or SSH."""
from pathlib import Path
import base64,copy,hashlib,importlib.util,json,sys
sys.dont_write_bytecode=True
E=Path('/tmp/fc-round50-https-integration-evidence')
P=E/'verify-final-browser-readonly-second.py'
spec=importlib.util.spec_from_file_location('final_audit',P);m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
checks=[]
def positive(name,fn):
 fn();checks.append({'name':name,'expected':'accept','status':'PASS'})
def negative(name,fn):
 try:fn()
 except (ValueError,AssertionError,KeyError):checks.append({'name':name,'expected':'reject','status':'PASS'});return
 raise AssertionError(name+' malformed fixture was accepted')
r=json.load(open(E/'windows-default-browser-continuous-first.json'));n=r['browser']['result']
q=json.load(open(E/'windows-readonly-final-first.json'));base=json.load(open(q['baseline']['path']));observed=json.loads(base64.b64decode(q['ssh']['stdout']['base64']).decode('utf-8-sig'))
positive('original actual completed network and13API',lambda:(m.validate_wire(n['network']),m.validate_api(n['network'],n),m.validate_no_failure(n)))
positive('original actual readonly complete raw agrees baseline',lambda:m.validate_readonly(q,base,observed))
row=copy.deepcopy(n['network'][0]);row.update(status=200,wireStatus=304,servedFromCache=True,fromDiskCache=True,finished=True,bodyVerified=True)
positive('actual304 remains wire even cache flags',lambda:(_ for _ in ()).throw(AssertionError()) if m.validate_wire([row])!=[row] else None)
row2=copy.deepcopy(row);row2['wireStatus']=200
positive('cached200 excluded without waiting extraInfo',lambda:(_ for _ in ()).throw(AssertionError()) if m.validate_wire([row2]) else None)
for field,value in [('finished',False),('bodyVerified',False)]:
 bad=copy.deepcopy(n['network']);bad[0][field]=value;assert bad!=n['network'];negative('network '+field,lambda bad=bad:m.validate_wire(bad))
for name,change in [
 ('duplicate contentPOST',lambda a:a.append(copy.deepcopy(next(x for x in a if x['method']=='POST')))),
 ('missing visual response',lambda a:a.pop(next(i for i,x in enumerate(a) if x['url'].endswith('/visual')))),
 ('cached API',lambda a:next(x for x in a if '/api/' in x['url']).__setitem__('servedFromCache',True)),
 ('bad API status',lambda a:next(x for x in a if '/api/' in x['url']).__setitem__('wireStatus',401)),
 ('unauthorized other write',lambda a:next(x for x in a if x['method']=='POST').__setitem__('url',n['origin']+'/fc/api/v1/logout'))]:
 bad=copy.deepcopy(n['network']);change(bad);assert bad!=n['network'];negative(name,lambda bad=bad:m.validate_api(bad,n))
for field,value in [('failureDiagnostics',{}),('error',{}),('status','FAIL')]:
 bad=copy.deepcopy(n);bad[field]=value;assert bad!=n;negative('PASS must not contain '+field,lambda bad=bad:m.validate_no_failure(bad))
for field,value in [('bytes',1),('sha256','0'*64),('captureScope','PARTIAL_TIMEOUT'),('observedBytes',999)]:
 raw={'bytes':0,'sha256':hashlib.sha256(b'').hexdigest(),'base64':''};raw[field]=value
 negative('raw binding '+field,lambda raw=raw:m.encoded_stream(raw,'model'))
for field,value in [('status','REVIEW_REQUIRED'),('integrityStatus','FAIL'),('sshd','Stopped'),('ownedProcesses',[{'pid':1}]),('ownedTasks',[{}]),('scratchDirectories',['FutureClassroom-Test-other']),('publicAuthorizationUnchanged',False),('projectRunUnchanged',False)]:
 bad=copy.deepcopy(q);bad[field]=value;assert bad!=q;negative('readonly '+field,lambda bad=bad:m.validate_readonly(bad,base,observed))
for field in ['sshPublicAuthorizationSha256','projectRunReadOnlySnapshot']:
 bad=copy.deepcopy(base);bad[field]='changed';assert bad!=base;negative('baseline '+field,lambda bad=bad:m.validate_readonly(q,bad,observed))
result={'schemaVersion':1,'status':'PASS_PURE_MODELS_ONLY','scope':'Host-only actual metadata copies deliberately corrupted. No remote, product tests or browser runs. Cached200 and confirmed304 semantics explicitly differ.','helper':{'path':str(P),'bytes':P.stat().st_size,'sha256':hashlib.sha256(P.read_bytes()).hexdigest()},'checks':checks,'positiveCount':sum(x['expected']=='accept' for x in checks),'negativeCount':sum(x['expected']=='reject' for x in checks)}
p=E/'browser-readonly-parser-model-first.json'
with p.open('x') as f:json.dump(result,f,indent=2);f.write('\n')
print(json.dumps({'status':result['status'],'positive':result['positiveCount'],'negative':result['negativeCount'],'sha256':hashlib.sha256(p.read_bytes()).hexdigest()}))
