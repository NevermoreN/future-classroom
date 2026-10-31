from pathlib import Path
import json,hashlib,shutil,datetime,re,base64,os
repo=Path('/workspace/共享/Eliora/未来教室计划');out=repo/'开发计划/验证记录/2026-11-01-round42-43-archive-first.json';assert not out.exists()
pairs=[(Path('/tmp/fc-round42-ci-evidence'),repo/'开发计划/验证记录/2026-11-01-round42-ci'),(Path('/tmp/fc-round43-win7-tls-expect-evidence'),repo/'开发计划/验证记录/2026-11-01-round43-win7-tls')]
pattern=re.compile(rb'-----BEGIN (?:OPENSSH |RSA |EC |DSA )?PRIVATE KEY-----|gh[pousr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,}|sk-proj-[A-Za-z0-9_-]{20,}|(?i:authorization:\s*(?:bearer|basic)\s+)[A-Za-z0-9+/=_-]{12,}')
reports=[];decoded=0
for src,dst in pairs:
 assert src.is_dir() and not src.is_symlink() and not dst.exists();files=[]
 for p in sorted(src.rglob('*')):
  assert not p.is_symlink()
  if not p.is_file():continue
  assert p.stat().st_uid==os.getuid();assert p.suffix.lower() not in {'.exe','.dll','.zip','.key','.pfx','.p12'};assert not p.name.endswith('.control.json')
  data=p.read_bytes();assert not pattern.search(data),str(p)
  if p.suffix=='.json':
   obj=json.loads(data)
   def scan(x,key=''):
    global decoded
    if isinstance(x,dict):
     for k,v in x.items():scan(v,k)
    elif isinstance(x,list):
     for v in x:scan(v,key)
    elif isinstance(x,str) and ('base64' in key.lower() or key.lower() in ('stdoutb64','stderrb64')):
     try:b=base64.b64decode(x,validate=True)
     except Exception:return
     assert not pattern.search(b),str(p);decoded+=1
   scan(obj)
  rel=p.relative_to(src); q=dst/rel;q.parent.mkdir(parents=True,exist_ok=True);q.write_bytes(data);assert q.read_bytes()==data;files.append({'path':rel.as_posix(),'bytes':len(data),'sha256':hashlib.sha256(data).hexdigest()})
 index={'schemaVersion':1,'status':'PASS','scope':'Complete current finished original evidence bytes. Original CI/product/local-guard failures and historical live records are retained; static Win7 checks do not imply execution. CI binary ZIP/EXE files audited separately and not committed. Synthetic public TLS certificate allowed; no private key. R41 prototype remains separate.','sourceRoot':str(src),'files':files,'originalFileCount':len(files),'originalBytes':sum(f['bytes'] for f in files)}
 idx=dst/'INDEX.json';idx.write_text(json.dumps(index,indent=2,ensure_ascii=False)+'\n');reports.append({'sourceRoot':str(src),'archiveRoot':str(dst.relative_to(repo)),'originalFiles':len(files),'originalBytes':index['originalBytes'],'indexSha256':hashlib.sha256(idx.read_bytes()).hexdigest()})
r={'status':'PASS','recordedUtc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'archives':reports,'decodedBase64FieldsScanned':decoded,'secretPatternMatches':0,'scope':'Exact archive only. No completed source directory/build/ZIP/EXE cleanup claimed. R42 package/assets retained for active R41; intentional /mnt/data development ZIP retained. Main staged/committed Git-byte proof follows separately.'};out.write_text(json.dumps(r,indent=2,ensure_ascii=False)+'\n');print(json.dumps(r,ensure_ascii=False))
