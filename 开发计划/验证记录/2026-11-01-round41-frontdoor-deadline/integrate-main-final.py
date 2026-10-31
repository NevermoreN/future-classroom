from pathlib import Path
import hashlib,json,subprocess,shutil

root=Path('/workspace/共享/Eliora/未来教室计划')
work=Path('/tmp/fc-round41-frontdoor-deadline-worktree')
ev=Path('/tmp/fc-round41-frontdoor-deadline-evidence')
out=ev/'main-final-integration-first.json'
assert not out.exists()
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
files=subprocess.check_output(['git','diff','--name-only','-z'],cwd=work).decode().rstrip('\0').split('\0')
assert len(files)==9 and len(set(files))==9
build=json.loads((ev/'windows-integrated-third.json').read_text())
assert build['status']=='PASS' and build['sourceBefore']==build['sourceAfter']
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=root)==subprocess.check_output(['git','rev-parse','HEAD'],cwd=work)
assert sha(root/'scripts/windows-msvc-ci.ps1')=='ed26c9932325d377bbbf4eaf63aad29d75fc0cc8ad77561b1121f3389024c418'
for n in files:
    assert subprocess.check_output(['git','show','HEAD:'+n],cwd=root)==(root/n).read_bytes(),n
before={n:{'bytes':(root/n).stat().st_size,'sha256':sha(root/n)} for n in files}
for n in files:
    dest=ev/'main-final-integration-before'/n
    dest.parent.mkdir(parents=True,exist_ok=True)
    shutil.copyfile(root/n,dest)
for n in files:shutil.copyfile(work/n,root/n)
after={n:{'bytes':(root/n).stat().st_size,'sha256':sha(root/n)} for n in files}
for n in files:assert (root/n).read_bytes()==(work/n).read_bytes(),n
actual={n:{'bytes':(root/n).stat().st_size,'sha256':sha(root/n)} for n in build['sourceAfter']}
assert actual==build['sourceAfter']
assert after['scripts/windows-tls-business-probe.py']['sha256']=='0cb59751f0bd1e79dd86de9eb26ac780e264a969d74cbdbe074be988210c7208'
assert after['scripts/windows-test-edge-owner.py']['sha256']=='e0930f19dc6fabf490774b2bd354605fb285d195dbaae32fdffd1e41f4f775f5'
assert sha(root/'scripts/windows-msvc-ci.ps1')=='ed26c9932325d377bbbf4eaf63aad29d75fc0cc8ad77561b1121f3389024c418'
r={'status':'PASS','scope':'Exact nine reviewed R41 files integrated into main. All 410 Rust build inputs match the final third GNU compilation and actual b1f Win11 runtime. Separate R45 CI input-domain source preserved. This is source identity, not an additional runtime claim.','filesBefore':before,'filesAfter':after,'rustInputs':len(actual),'sourceBuildReport':{'path':str(ev/'windows-integrated-third.json'),'sha256':sha(ev/'windows-integrated-third.json')},'gitHead':subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip()}
out.write_text(json.dumps(r,indent=2)+'\n')
print(json.dumps({'status':'PASS','files':len(files),'rustInputs':len(actual)}))
