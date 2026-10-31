from pathlib import Path
import hashlib,json,re,base64,shutil
root=Path('/tmp/fc-round39-voice-capability-worktree');pairs=[(Path('/tmp/fc-round39-voice-capability-evidence'),'2026-11-01-round39-voice-capability'),(Path('/tmp/fc-round40-ci-evidence'),'2026-11-01-round40-ci')]
patterns={'privateKey':rb'-----BEGIN (?:[A-Z0-9]+ )*PRIVATE KEY-----','githubCredential':rb'\b(?:gh[pousr]_[A-Za-z0-9]{25,}|github_pat_[A-Za-z0-9_]{30,})','openaiCredential':rb'\bsk-(?:proj-)?[A-Za-z0-9_-]{35,}','literalAuthorization':rb'Authorization: (?:Bearer|Basic) [A-Za-z0-9_+/=-]{25,}'}
h=lambda d:hashlib.sha256(d).hexdigest();decoded=0

def scan(data):
 for name,p in patterns.items():assert not re.search(p,data),name

def walk(obj):
 global decoded
 if isinstance(obj,dict):
  for k,v in obj.items():
   if k=='base64' and isinstance(v,str):
    try:d=base64.b64decode(v,validate=True)
    except ValueError:continue
    decoded+=1;scan(d)
   else:walk(v)
 elif isinstance(obj,list):
  for x in obj:walk(x)

for source,name in pairs:
 dest=root/'开发计划/验证记录'/name;assert not dest.exists()
 files=sorted(p for p in source.rglob('*') if p.is_file());assert files and not any(p.is_symlink() for p in source.rglob('*'))
 rows=[]
 for p in files:
  data=p.read_bytes();scan(data)
  if p.suffix=='.json':
   try:obj=json.loads(data.decode('utf-8-sig'))
   except (ValueError,UnicodeError):obj=None
   walk(obj)
  if p.suffix=='.pem':assert data.startswith(b'-----BEGIN CERTIFICATE-----') and b'PRIVATE KEY' not in data
  assert p.suffix.lower() not in ['.exe','.dll','.pfx','.p12','.key','.sqlite','.db','.obj','.pdb','.lib']
  relative=p.relative_to(source).as_posix();target=dest/relative;target.parent.mkdir(parents=True,exist_ok=True);target.write_bytes(data);assert target.read_bytes()==data
  rows.append({'path':relative,'bytes':len(data),'sha256':h(data)})
 index={'schemaVersion':1,'status':'PASS','scope':'Complete original finished evidence archive with exact byte preservation. Original failed/pending-at-that-time records remain unchanged and are superseded only by separately named later results. Binary CI payloads are independently audited in original artifact ledgers, not committed here. Public synthetic TLS certificate only; no private key.','sourceRoot':str(source),'files':rows,'fileCount':len(rows),'totalBytes':sum(x['bytes'] for x in rows),'indexExcludedFromOwnLedger':True,'secretScan':{'literalPatterns':list(patterns),'base64FieldsDecodedCumulative':decoded,'matches':0}}
 (dest/'INDEX.json').write_text(json.dumps(index,ensure_ascii=False,indent=2)+'\n')
 print(json.dumps({'archive':name,'files':len(rows),'bytes':index['totalBytes'],'indexSha256':h((dest/'INDEX.json').read_bytes())}),flush=True)
