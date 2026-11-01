"""Exact reviewed benign-location models; no actual archive or mutation of originals."""
from pathlib import Path
import hashlib,importlib.util,json,sys
sys.dont_write_bytecode=True
E=Path('/tmp/fc-round50-https-integration-evidence');p=E/'archive-https-rounds-second.py';spec=importlib.util.spec_from_file_location('archive_second',p);m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m);checks=[]
def scan(row,raw,relative=None,root=None):
 rel=relative or row['file'];s=m.Scanner();s.review_context=(root or row['root'],rel,m.sha(raw));text=raw.decode('utf-8-sig');s.text(text,rel)
 if row['file'].endswith('.json'):s.walk(json.loads(text),rel)
 return s
def ok(name,condition):
 if not condition:raise AssertionError(name)
 checks.append({'name':name,'status':'PASS'})
for i,row in enumerate(m.REVIEWED_BENIGN_EXCEPTIONS):
 raw=(Path(row['root'])/row['file']).read_bytes();s=scan(row,raw);ok('exact reviewed original '+str(i),not s.blocked and s.benign_reviews==[row])
 changed=raw+b' ';s=scan(row,changed);ok('whole-file bytes change still blocked '+str(i),any(x['kind']==row['kind'] for x in s.blocked) and not s.benign_reviews)
 s=scan(row,raw,relative='unreviewed/'+row['file']);ok('same contents at unreviewed path blocked '+str(i),any(x['kind']==row['kind'] for x in s.blocked))
 s=scan(row,raw,root='/tmp/unreviewed-root');ok('same contents at foreign root blocked '+str(i),any(x['kind']==row['kind'] for x in s.blocked))
 # A different sensitive rule must never be exempted, even if a model provides
 # the reviewed context identity directly. Production computes it from bytes.
 s=m.Scanner();s.review_context=(row['root'],row['file'],row['rawSha256']);s.text('ghp_'+'X'*30,row['location']);ok('real token rule never exempt '+str(i),any(x['kind']=='GITHUB_TOKEN' for x in s.blocked))
policy=m.REVIEWED_BENIGN_EXCEPTIONS[0];obj=json.loads((Path(policy['root'])/policy['file']).read_bytes());obj['normative_contract_proposal']['authorization']='Bearer '+'0123456789abcdef'*4;raw=json.dumps(obj).encode();s=scan(policy,raw);ok('changed actual Authorization token blocked',any(x['kind'] in {'AUTHORIZATION_SECRET','JSON_PASSWORD_OR_ACCESS_TOKEN_REVIEW'} for x in s.blocked) and not s.benign_reviews)
s=m.Scanner();s.walk({'authorization':'Natural policy language long enough to require review, in an unreviewed document.'},'other.json');ok('unreviewed same key remains review blocker',any(x['kind']=='JSON_PASSWORD_OR_ACCESS_TOKEN_REVIEW' for x in s.blocked))
s=m.Scanner();s.text('https'+ '://example.test/a?sig='+'X'*32,'source.rs');ok('signed URL unchanged rejection',any(x['kind']=='SIGNED_OR_CREDENTIALED_URL' for x in s.blocked))
s=m.Scanner();s.text('https'+ '://[{}]:{}','other.rs');ok('unreviewed Rust format string still rejected',any(x['kind']=='MALFORMED_URL_REVIEW' for x in s.blocked))
result={'schema':'fc.https.archive.exact-benign-model/v1','status':'PASS_LOCAL_EXACT_BENIGN_MODELS_ONLY','helper':{'path':str(p),'bytes':p.stat().st_size,'sha256':hashlib.sha256(p.read_bytes()).hexdigest()},'checks':checks,'count':len(checks),'scope':'Memory models reading six exact original files only. No original edits, actual archive, main, SSH, Cargo or cleanup.'}
out=E/'archive-https-benign-model-third.json'
with out.open('x') as f:json.dump(result,f,ensure_ascii=False,indent=2);f.write('\n')
print(json.dumps({'status':result['status'],'checks':len(checks),'sha256':hashlib.sha256(out.read_bytes()).hexdigest()}))
