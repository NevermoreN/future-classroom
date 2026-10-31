from pathlib import Path
import shutil,json,hashlib,stat
repo=Path.cwd();dest=repo/'开发计划/验证记录';idx=dest/'2026-10-31-round29-evidence-index.json';j=json.loads(idx.read_text())
for r in j['records']:
 a=repo/r['archivedPath'];b=Path(r['originalPath']);assert a.read_bytes()==b.read_bytes() and hashlib.sha256(a.read_bytes()).hexdigest()==r['sha256']
roots=[Path('/tmp')/s for s in ['fc-round29-host','fc-round29-win','fc-round29-win7','fc-round29-core-target','fc-round29-http1-editor-target','fc-round29-response-target','fc-round29-web-dist','fc-round29-repack','fc-round29-evidence']]
assert set(Path('/tmp').glob('fc-round29-*'))<=set(roots)
rows=[]
for root in roots:
 if not root.exists():rows.append({'path':str(root),'state':'already absent, agent-owned response target cleaned after scoped tests','logicalBytes':0});continue
 assert root.is_dir() and not root.is_symlink();files=[x for x in root.rglob('*') if x.is_file() and not x.is_symlink()];rows.append({'path':str(root),'files':len(files),'logicalBytes':sum(x.stat().st_size for x in files)})
for root in roots:
 if root.exists():shutil.rmtree(root)
assert not list(Path('/tmp').glob('fc-round29-*'))
key=Path('/config/.local/share/future-classroom-test-access/win11-192.168.10.212');assert stat.S_IMODE(key.stat().st_mode)==0o600
pkg=Path('/mnt/data/FutureClassroom-0.1.0-http1-tls-dev.zip');assert hashlib.sha256(pkg.read_bytes()).hexdigest()=='49d497d1a6a03a0d5d13ab638a1765fb07ffab2e6823a926ef848a5a34c20912'
r={'status':'PASS','scope':'only this round own build/test/scratch roots, original evidence byte-verified before deletion','roots':rows,'removedLogicalBytes':sum(x['logicalBytes'] for x in rows),'remainingOwnedTemporaryRoots':0,'persistentSshPrivateKeyMode':'0600; retained outside repository','developmentPackage':'retained unchanged','preexistingPackagesWorkspaceNodeModulesGlobalCaches':'retained','remoteScope':'see windows-after and both TLS cleanup reports; persistent sshd/public authorization unchanged'};out=dest/'2026-10-31-round29-cleanup.json';out.write_text(json.dumps(r,ensure_ascii=False,indent=2)+'\n');data=out.read_bytes();j['records'].append({'originalPath':str(out),'archivedPath':str(out.relative_to(repo)),'sizeBytes':len(data),'sha256':hashlib.sha256(data).hexdigest(),'note':'generated after actual cleanup'});idx.write_text(json.dumps(j,ensure_ascii=False,indent=2)+'\n');print(json.dumps({'status':'PASS','removedLogicalBytes':r['removedLogicalBytes'],'remainingOwnedTemporaryRoots':0,'archivedRecords':len(j['records'])}))
