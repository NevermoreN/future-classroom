from pathlib import Path
import json,hashlib,subprocess,stat,shutil,datetime
repo=Path.cwd();base=Path('/tmp/fc-round34-accept-evidence');out=base/'local-build-and-test-cleanup.json'
target=Path('/tmp/fc-round34-accept-target');programs=Path('/tmp/fc-round34-accept-programs');worktree=Path('/tmp/fc-round34-accept-worktree')
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
def check(v,m):
 if not v:raise AssertionError(m)
def real(p):
 s=p.lstat();check(stat.S_ISDIR(s.st_mode) and not p.is_symlink() and p.resolve()==p,'Expected exact real owned root');return {'device':s.st_dev,'inode':s.st_ino}
def files(p):
 rows=[]
 for q in p.rglob('*'):
  s=q.lstat();check(not q.is_symlink() and (stat.S_ISREG(s.st_mode) or stat.S_ISDIR(s.st_mode)),'Unexpected aliased/special owned-root entry')
  if stat.S_ISREG(s.st_mode):rows.append({'path':q.relative_to(p).as_posix(),'bytes':s.st_size})
 return rows
def command(argv):
 p=subprocess.run(argv,capture_output=True,text=True,timeout=45);r['commands'].append({'command':argv,'exitCode':p.returncode,'stdout':p.stdout,'stderr':p.stderr});check(p.returncode==0,'Exact owned worktree Git operation failed');return p.stdout
with out.open('x') as stream:
 r={'status':'FAIL','createdUtc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'scope':'Remove only this round owned cross-build target, two executable snapshots, integrated source-only worktree and its base-only branch. Evidence working directory is retained until archive/Git blob proof; final development ZIP and ongoing tools/SSH are retained. Logical lengths are not physical filesystem reclaim measurements.','commands':[],'removed':[]}
 try:
  binding=json.loads((base/'final-source-and-runtime-binding.json').read_text());package=json.loads((base/'development-package-first.json').read_text())
  check(binding['status']=='PASS_AFFECTED_WINDOWS_SOURCE_AND_RUNTIME_BOUND' and package['status']=='PASS_DEVELOPMENT_PACKAGE_ONLY' and package['temporaryPackagingRootsRemoved'],'Validation/package not complete')
  r['preexistingRootTarget']=real(repo/'target')
  check(sha(Path(package['package']['path']))==package['package']['sha256'],'Retained dev package identity differs')
  expected={Path(x['path']).name:x for x in binding['programs']}
  check({p.name for p in programs.iterdir()}==set(expected),'Executable snapshot root contains unexpected entries')
  for name,row in expected.items():
   check(sha(programs/name)==row['sha256']==sha(Path(row['originalPath'])),'Original/tested executable identity changed')
  expected_changed={'crates/fc-bootstrap/src/application/http_business_probe.rs','crates/fc-bootstrap/src/web_transfer/network.rs','crates/fc-windows/src/tls_frontdoor/mod.rs','crates/fc-windows/src/tls_frontdoor/native.rs','crates/fc-windows/src/tls_frontdoor/native/accept.rs','crates/fc-windows/src/tls_frontdoor/native/stream.rs','crates/fc-windows/src/tls_frontdoor/native/stream/tests.rs','scripts/windows-tls-business-probe.py'}
  check(command(['git','-C',str(worktree),'rev-parse','HEAD']).strip()==binding['gitBaseReference'],'Worktree head differs')
  tracked=set(command(['git','-C',str(worktree),'diff','--name-only','-z']).strip('\x00').split('\x00'))
  untracked=set(command(['git','-C',str(worktree),'ls-files','--others','--exclude-standard','-z']).strip('\x00').split('\x00'))
  check(tracked|untracked==expected_changed,'Source-only worktree change inventory differs')
  check(not command(['git','-C',str(worktree),'ls-files','--others','--ignored','--exclude-standard','-z']),'Unexpected ignored worktree files')
  r['integratedWorktreeFiles']={n:{'bytes':(worktree/n).stat().st_size,'sha256':sha(worktree/n)} for n in sorted(expected_changed)}
  check(all((worktree/n).read_bytes()==(repo/n).read_bytes() for n in expected_changed),'A worktree source is not integrated exactly in root')
  for p in [target,programs]:
   identity=real(p);rows=files(p);check(real(p)==identity,'Owned root identity changed before deletion')
   shutil.rmtree(p);check(not p.exists(),'Owned root remains')
   r['removed'].append({'path':str(p),'identity':identity,'fileCount':len(rows),'logicalBytes':sum(x['bytes'] for x in rows),'removed':True})
  identity=real(worktree);rows=files(worktree);check(real(worktree)==identity,'Worktree identity changed')
  command(['git','worktree','remove','--force',str(worktree)]);check(not worktree.exists(),'Owned worktree remains')
  command(['git','branch','-d','codex/continuous-accept-owner'])
  r['removed'].append({'path':str(worktree),'identity':identity,'fileCount':len(rows),'logicalBytes':sum(x['bytes'] for x in rows),'removed':True,'branchDeleted':'codex/continuous-accept-owner','sourceCopiesAllIntegratedInRoot':True})
  check(real(repo/'target')==r['preexistingRootTarget'],'Preexisting root target changed identity')
  key=Path('/config/.local/share/future-classroom-test-access/win11-192.168.10.212');check(stat.S_IMODE(key.stat().st_mode)==0o600,'Persistent private key mode differs')
  r.update(status='PASS',retained={'developmentPackage':package['package'],'preexistingRootTarget':'UNTOUCHED','fixedNightlyClippyAndRustfmt':'CONTINUING_DEVELOPMENT_TOOLS','persistentSshKey':{'path':str(key),'contentsRead':False,'mode':'0600'}})
 except Exception as e:r['error']={'kind':type(e).__name__,'detail':str(e)}
 finally:stream.write(json.dumps(r,ensure_ascii=False,indent=2)+'\n')
print(json.dumps({'status':r['status'],'error':r.get('error'),'removed':r['removed']},ensure_ascii=False));raise SystemExit(0 if r['status']=='PASS' else 1)
