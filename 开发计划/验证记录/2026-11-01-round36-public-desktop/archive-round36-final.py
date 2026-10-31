from pathlib import Path
import hashlib,json,shutil,re,subprocess
root=Path.cwd();source=Path('/tmp/fc-round36-public-desktop-evidence');dest=root/'开发计划/验证记录/2026-11-01-round36-public-desktop'
assert root==Path('/tmp/fc-round35-visuals-worktree') and not dest.exists()
sha=lambda b:hashlib.sha256(b).hexdigest()
files=[]
for p in sorted(source.rglob('*')):
 assert not p.is_symlink()
 if not p.is_file():continue
 assert p.suffix.lower() not in {'.exe','.dll','.zip','.pfx','.key'}
 data=p.read_bytes()
 assert not re.search(rb'-----BEGIN (?:OPENSSH |RSA |EC |DSA |ENCRYPTED )?PRIVATE KEY-----|gh[pousr]_[A-Za-z0-9]{30,}|github_pat_[A-Za-z0-9_]{30,}|[?&](?:X-Amz-Signature|sig|se|skoid)=',data,re.I),str(p)
 files.append({'path':p.relative_to(source).as_posix(),'bytes':len(data),'sha256':sha(data)})
assert len(files)>150
for x in files:
 p=dest/x['path'];p.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(source/x['path'],p)
 assert sha(p.read_bytes())==x['sha256']
r={'schemaVersion':1,'scope':'Complete original integrated public desktop validation: failures, source corrections, focused and complete tests, native probes, browser, package and cleanup. Original reports immutable. Cross compilation and finite Win11 tests do not establish Win7/Win10/visible Seewo acceptance.','sourceDirectory':str(source),'files':files,'originalCount':len(files),'originalBytes':sum(x['bytes'] for x in files),'indexIncludesItself':False}
(dest/'evidence-index.json').write_text(json.dumps(r,ensure_ascii=False,indent=2)+'\n')
subprocess.run(['git','add','-f','--',str(dest)],check=True)
print(json.dumps({'status':'PASS','count':len(files),'bytes':r['originalBytes'],'indexSha256':sha((dest/'evidence-index.json').read_bytes())}))
