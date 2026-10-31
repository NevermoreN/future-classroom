from pathlib import Path
import json,hashlib,shutil,stat
ev=Path('/tmp/fc-round31-device-evidence');out=ev/'local-build-and-test-cleanup.json';assert not out.exists()
binding=json.loads((ev/'final-source-and-runtime-binding.json').read_bytes());assert binding['status']=='PASS_CURRENT_FEATURE_BOUND_TO_ACTUAL_WIN11_GNU'
roots=[Path('/tmp/fc-round31-device-target'),Path('/tmp/fc-round31-device-programs')]
for proc in Path('/proc').iterdir():
    if not proc.name.isdigit():continue
    try:executable=(proc/'exe').readlink()
    except (PermissionError,FileNotFoundError,ProcessLookupError):continue
    assert not any(str(executable).startswith(str(root)+'/') for root in roots), 'An owned executable is still physically live'
programs={Path(x['path']) for x in binding['programs']}
assert {p for p in roots[1].rglob('*') if p.is_file()}==programs and len(programs)==22
for row in binding['programs']:
    p=Path(row['path']);assert not p.is_symlink();data=p.read_bytes();assert len(data)==row['bytes'] and hashlib.sha256(data).hexdigest()==row['sha256']
package=Path(binding['developmentPackage']['path']);assert hashlib.sha256(package.read_bytes()).hexdigest()==binding['developmentPackage']['sha256']
records=[]
for root in roots:
    assert root.is_dir() and not root.is_symlink() and root.resolve()==root
    files=[p for p in root.rglob('*') if not stat.S_ISDIR(p.lstat().st_mode)]
    records.append({'root':str(root),'files':len(files),'logicalBytes':sum(p.lstat().st_size for p in files)})
for root in roots:shutil.rmtree(root);assert not root.exists()
assert not Path('/tmp/fc-round31-web-dist').exists() and not Path('/tmp/fc-round31-repack').exists()
r={'status':'PASS','scope':'All own completed Host/GNU build caches and frozen test-program snapshots removed only after physical process termination, source/runtime binding and deterministic current development ZIP verification. Source worktree and original public evidence remain for integration; persistent Windows SSH authorization/key and unrelated caches untouched.','removed':records,'totalFiles':sum(x['files'] for x in records),'totalLogicalBytes':sum(x['logicalBytes'] for x in records),'retainedDevelopmentPackage':binding['developmentPackage'],'retainedPackageShaVerifiedAfterCleanup':hashlib.sha256(package.read_bytes()).hexdigest()==binding['developmentPackage']['sha256'],'windowsAudit':'windows-after.json PASS with no owned activity','sourceBinding':'final-source-and-runtime-binding.json','activeUserEnvironment':'UNTOUCHED'}
out.write_text(json.dumps(r,indent=2)+'\n');print(json.dumps({k:r[k] for k in ['status','totalFiles','totalLogicalBytes','removed','retainedPackageShaVerifiedAfterCleanup']}),flush=True)
