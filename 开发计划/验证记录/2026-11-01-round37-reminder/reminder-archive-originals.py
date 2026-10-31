from pathlib import Path
import hashlib,json,shutil,re,subprocess
root=Path.cwd();ev=Path('/tmp/fc-round37-ci-evidence');dest=root/'开发计划/验证记录/2026-11-01-round37-reminder';assert root==Path('/tmp/fc-round37-reminder-worktree') and not dest.exists()
selected=[p for p in sorted(ev.rglob('*')) if p.is_file() and p.relative_to(ev).parts[0].startswith('reminder-')]
files=[]
for p in selected:
 assert not p.is_symlink() and p.suffix.lower() not in {'.exe','.dll','.zip','.pfx','.key'}
 data=p.read_bytes();assert not re.search(rb'-----BEGIN (?:OPENSSH |RSA |EC |DSA |ENCRYPTED )?PRIVATE KEY-----|gh[pousr]_[A-Za-z0-9]{30,}|github_pat_[A-Za-z0-9_]{30,}|[?&](?:X-Amz-Signature|sig|se|skoid)=',data,re.I),str(p)
 relative=p.relative_to(ev);q=dest/relative;q.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(p,q);assert q.read_bytes()==data
 files.append({'path':relative.as_posix(),'bytes':len(data),'sha256':hashlib.sha256(data).hexdigest()})
assert len(files)>40
r={'schemaVersion':1,'scope':'Original finite REMINDER bottom-shortcut/save/reload browser verification plus independent original HOMEWORK/visual regression, pure model checks, frozen tool/product input proof and owned input cleanup. Synthetic approved GlobalAdmin, not real teacher/mobile/microphone/pairing/production/platform/strict5 acceptance. Separate still-live MSVC CI reports intentionally remain outside this immutable finished-test archive.','sourceDirectory':str(ev),'selection':'All files whose first relative path component starts reminder-, including original raw edge-owner artifacts and helper source copies.','files':files,'originalCount':len(files),'originalBytes':sum(x['bytes'] for x in files),'indexIncludesItself':False}
p=dest/'evidence-index.json';p.write_text(json.dumps(r,ensure_ascii=False,indent=2)+'\n');subprocess.run(['git','add','-f','--',str(dest)],check=True)
print(json.dumps({'status':'PASS','originalCount':len(files),'originalBytes':r['originalBytes'],'indexSha256':hashlib.sha256(p.read_bytes()).hexdigest()}))
