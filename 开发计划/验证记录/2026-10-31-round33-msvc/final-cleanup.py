from pathlib import Path
import json,hashlib,subprocess,shutil,stat,datetime

repo=Path.cwd();base=Path('/tmp/fc-round33-msvc-evidence');d=repo/'开发计划/验证记录';out=Path('/mnt/data/FutureClassroom-round33-sync-and-cleanup.json');assert not out.exists()
sha=lambda b:hashlib.sha256(b).hexdigest()
load=lambda p:json.loads(p.read_bytes())
head=subprocess.check_output(['git','rev-parse','HEAD']).decode().strip();assert not subprocess.check_output(['git','status','--porcelain'])
assert subprocess.check_output(['git','ls-remote','origin','refs/heads/main']).decode().split()[0]==head
summary=load(d/'2026-10-31-round33-msvc-stage-summary.json');rows=summary['sourceMapping'];assert {str(p) for p in base.rglob('*') if p.is_file()}=={r['source'] for r in rows}
for row in rows:
 p=Path(row['source']);assert p.is_file() and not p.is_symlink();data=p.read_bytes();assert len(data)==row['bytes'] and sha(data)==row['sha256'];assert (d/row['path']).read_bytes()==data;assert subprocess.check_output(['git','show','HEAD:开发计划/验证记录/'+row['path']])==data
v=load(base/'artifact-verification-first.json');programs=Path('/tmp/fc-round33-msvc-programs');bins=v['binaryFacts'];assert {str(p) for p in programs.iterdir()}=={r['path'] for r in bins}
for row in bins:
 p=Path(row['path']);assert p.is_file() and not p.is_symlink();data=p.read_bytes();assert len(data)==row['bytes'] and sha(data)==row['sha256']
zip_reports=['artifact-download-first.json','artifact-download-resume-first.json','artifact-download-resume-second.json','artifact-download-resume-third.json'];zips=[]
for n in zip_reports:
 r=load(base/n);p=Path(r['path']);assert p.parent==Path('/tmp') and p.name in ['fc-round33-msvc-original.zip','fc-round33-msvc-resumed.zip','fc-round33-msvc-resumed-second.zip','fc-round33-msvc-resumed-third.zip'];assert p.is_file() and not p.is_symlink();data=p.read_bytes();assert len(data)==r['bytes'] and sha(data)==r['sha256'];zips.append({'path':str(p),'bytes':len(data),'sha256':sha(data),'report':n})
assert len({r['path'] for r in zips})==4
w=Path('/tmp/fc-round33-win7-worktree');assert w.is_dir() and not w.is_symlink();assert subprocess.check_output(['git','-C',str(w),'branch','--show-current']).decode().strip()=='codex/win7-msvc-baseline';assert subprocess.check_output(['git','-C',str(w),'rev-parse','HEAD']).decode().strip()=='57d7ca0f1308aa8ea83a81af28addef9abef4af6'
status=subprocess.check_output(['git','-C',str(w),'status','--porcelain','-z']).split(b'\0');assert {s[3:].decode() for s in status if s}=={'scripts/windows-msvc-ci.ps1','.github/workflows/windows-win7-msvc.yml'}
for n in ['scripts/windows-msvc-ci.ps1','.github/workflows/windows-win7-msvc.yml']:assert (w/n).read_bytes()==subprocess.check_output(['git','show','HEAD:'+n])==(repo/n).read_bytes()
key=Path('/config/.local/share/future-classroom-test-access/win11-192.168.10.212');assert key.is_file() and stat.S_IMODE(key.stat().st_mode)==0o600
pkg=summary['developmentPackage'];assert sha(Path(pkg['path']).read_bytes())==pkg['sha256'];assert summary['windowsFinalAudit']['status']=='PASS'
ci=[]
for n in ['windows-msvc','windows-win7-msvc']:
 p=Path('/mnt/data')/('FutureClassroom-round33-'+n+'-dispatch.json');r=load(p);assert r['sourceSha']==head and r['status']=='RUN_IDENTIFIED' and sum(c['method']=='POST' for c in r['calls'])==1;ci.append({'path':str(p),'sha256':sha(p.read_bytes()),'runId':r['runId'],'sourceSha':head,'runStatus':r['runStatus'],'conclusion':r.get('conclusion')})
report={'status':'RUNNING','createdUtc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'main':head,'remoteMain':head,'goal':'ACTIVE: 按照计划完成全部开发。','scope':'Only exact original already-Git-archived Round33 working evidence,22 exact tested binaries,4 exact owned download files and isolated worktree/branch. No user Windows resources, persistent SSH authorization, unrelated caches or planning drafts touched.','sourceMapping':rows,'programs':bins,'downloads':zips,'developmentPackage':pkg,'ciDispatches':ci,'persistentPrivateKey':{'path':str(key),'mode':'0o600','contentsRead':False,'retained':True},'retainedDevelopmentTools':'Installed rustfmt on fixed nightly plus existing compiler/rust-src retained as ongoing development tooling; no local Cargo build target created by Round33. Preexisting main target is unrelated and retained.','ownCleanupSource':Path(__file__).read_text(),'removed':[]}
def save():
 with out.open('w') as f:f.write(json.dumps(report,ensure_ascii=False,indent=2)+'\n')
with out.open('x') as f:f.write(json.dumps(report,ensure_ascii=False,indent=2)+'\n')
try:
 for row in zips:Path(row['path']).unlink();assert not Path(row['path']).exists();report['removed'].append(row);save()
 assert programs.resolve()==programs;shutil.rmtree(programs);assert not programs.exists();report['removed'].append({'path':str(programs),'files':len(bins),'logicalBytes':sum(r['bytes'] for r in bins)});save()
 subprocess.run(['git','worktree','remove','--force',str(w)],check=True,capture_output=True);assert not w.exists();subprocess.run(['git','branch','-D','codex/win7-msvc-baseline'],check=True,capture_output=True);report['removed'].append({'path':str(w),'branch':'codex/win7-msvc-baseline'});save()
 assert base.resolve()==base;shutil.rmtree(base);assert not base.exists();report['removed'].append({'path':str(base),'files':len(rows),'logicalBytes':sum(r['bytes'] for r in rows)});save()
 assert not Path('/tmp/fc-round33-web-dist').exists() and not Path('/tmp/fc-round33-msvc-repack').exists();assert key.is_file() and stat.S_IMODE(key.stat().st_mode)==0o600
 assert not subprocess.check_output(['git','status','--porcelain']);assert subprocess.check_output(['git','ls-remote','origin','refs/heads/main']).decode().split()[0]==head
 report.update(status='PASS',completedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),workingTree='CLEAN',localOwnedBuildAndTestResidue='ABSENT',persistentSshAuthorization='RETAINED')
except Exception as e:report.update(status='FAIL',errorKind=type(e).__name__);save();raise
save();print(json.dumps({'status':report['status'],'main':head,'workingTree':'CLEAN','ci':ci,'cleanupReport':str(out),'persistentSshAuthorization':'RETAINED'},ensure_ascii=False),flush=True)
