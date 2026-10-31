from pathlib import Path
import os,subprocess,json,hashlib,shutil,datetime
repo=Path('/workspace/共享/Eliora/未来教室计划'); ev=repo/'开发计划/验证记录'; proof=json.loads((ev/'2026-11-01-round42-43-committed-proof.json').read_text());assert proof['status']=='PASS';commit=proof['commit'];assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=repo,text=True).strip()==commit
out=ev/'2026-11-01-round42-43-cleanup-first.json';assert not out.exists();r={'status':'RUNNING','startedUtc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'commitProof':commit,'removed':[],'retained':['/tmp/fc-round42-package','/tmp/fc-round42-web-assets','/tmp/fc-round41-frontdoor-deadline-worktree','/tmp/fc-round41-frontdoor-deadline-evidence','/tmp/fc-round41-build-target','/tmp/fc-round41-programs-second','/mnt/data development ZIPs','persistent SSH authorization and key','original main target and global tool caches'],'processesKilled':0};out.write_text(json.dumps(r,indent=2)+'\n')
def sha(p):
 h=hashlib.sha256()
 with p.open('rb') as f:
  for b in iter(lambda:f.read(1024*1024),b''):h.update(b)
 return h.hexdigest()
def guard(p):
 assert p.is_absolute() and str(p).startswith('/tmp/fc-round') and p.exists() and not p.is_symlink() and p.resolve()==p and p.stat().st_uid==os.getuid()
 for process in Path('/proc').iterdir():
  if not process.name.isdigit() or int(process.name)==os.getpid():continue
  try:
   args=(process/'cmdline').read_bytes().replace(b'\0',b' ')
   cwd=(process/'cwd').resolve()
  except (FileNotFoundError,ProcessLookupError,PermissionError):continue
  assert str(p).encode() not in args and not cwd.is_relative_to(p),f'active owner for {p} pid{process.name}'
def done(p):r['removed'].append(str(p));out.write_text(json.dumps(r,indent=2)+'\n')
for p,size,digest in [(Path('/tmp/fc-round42-modern-original.zip'),40772166,'93638e76769f5d3910c4e3e9a713a93bedc318c55ebc8e78f24ff42ac4ff265a'),(Path('/tmp/fc-round42-win7-original.zip'),41487448,'35ce3793f54327a948113f891f3dfda6cbe623af1459412eda212443dab7133b')]:
 guard(p);assert p.stat().st_size==size and sha(p)==digest;p.unlink();done(p)
audit=json.loads((ev/'2026-11-01-round42-ci/modern-programs-independent-first.json').read_text());root=Path('/tmp/fc-round42-modern-programs');guard(root)
# Producer-audited exact binary names and hashes bind this directory without relying on a generic cache marker.
print('audit keys',list(audit))
rows=audit.get('binaryFacts',audit.get('programs',audit.get('executables')));assert isinstance(rows,list)
expected={Path(f['path']).name:f for f in rows};assert set(p.name for p in root.iterdir())==set(expected)
for p in root.iterdir():assert p.is_file() and not p.is_symlink() and sha(p)==expected[p.name]['sha256']
shutil.rmtree(root);done(root)
w=Path('/tmp/fc-round43-win7-tls-expect-worktree');guard(w);assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=w,text=True).strip()=='bc2a86fb4413dcb3e8acf6b0d4bb2dfe601c22dc';freeze=json.loads((ev/'2026-11-01-round43-win7-tls/source-freeze-first.json').read_text());expected={f['path'] for f in freeze['files']};changed={s[3:] for s in subprocess.check_output(['git','status','--porcelain'],cwd=w,text=True).splitlines()};assert changed==expected
for f in freeze['files']:assert sha(w/f['path'])==f['sha256'] and (w/f['path']).read_bytes()==(repo/f['path']).read_bytes()
subprocess.run(['git','worktree','remove','--force',str(w)],cwd=repo,check=True,capture_output=True);done(w);subprocess.run(['git','branch','-d','codex/win7-tls-expect-completion'],cwd=repo,check=True,capture_output=True)
for original,archive in [('fc-round43-win7-tls-expect-evidence','2026-11-01-round43-win7-tls'),('fc-round42-ci-evidence','2026-11-01-round42-ci')]:
 root=Path('/tmp')/original;guard(root);idx=json.loads((ev/archive/'INDEX.json').read_text());files={p.relative_to(root).as_posix():p for p in root.rglob('*') if p.is_file()};assert set(files)=={f['path'] for f in idx['files']}
 for f in idx['files']:assert files[f['path']].stat().st_size==f['bytes'] and sha(files[f['path']])==f['sha256']
 shutil.rmtree(root);done(root)
r.update(status='PASS',completedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),scope='Exact committed byte proof preceded cleanup; only original ZIPs, modern22EXE copies, merged R43 worktree/branch and finished evidence copies removed. Active R41 roots and its R42 web/package dependencies deliberately retained. No process killed or system configuration changed.');out.write_text(json.dumps(r,indent=2)+'\n');print(json.dumps({'status':r['status'],'removed':len(r['removed'])}))
