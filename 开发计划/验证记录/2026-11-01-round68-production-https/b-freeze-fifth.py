from pathlib import Path
import subprocess,json,hashlib,datetime,difflib
r=Path('/tmp/fc-round68-production-https/worktree');ev=r.parent/'evidence'
prior=ev/'b-implementation-freeze-fourth.json';d=json.loads(prior.read_text());added=['crates/fc-bootstrap/src/backend/account_management_tests.rs','crates/fc-bootstrap/src/backend/profiles/tests.rs'];paths=[x['relativePath'] for x in d['source']]+added
assert len(paths)==len(set(paths))==20
cmd=['rustfmt','--check','--edition','2021','--config','skip_children=true']+paths
s=subprocess.run(cmd,cwd=r,capture_output=True,timeout=60)
for n,b in [('stdout.log',s.stdout),('stderr.log',s.stderr)]:
 with (ev/f'b-rustfmt-check-fifth.{n}').open('xb') as f:f.write(b)
assert s.returncode==0,s.stderr[-2000:]
def ident(b):return {'bytes':len(b),'sha256':hashlib.sha256(b).hexdigest()}
rows=[];delta=[];diff=[]
for rel in paths:
 b=(r/rel).read_bytes();out=ev/'b-source-after-fifth'/rel;out.parent.mkdir(parents=True,exist_ok=True)
 with out.open('xb') as f:f.write(b)
 old=next((x for x in d['source'] if x['relativePath']==rel),None)
 if old:before=(ev/old['after']['path']).read_bytes();base=old['before']
 else:
  before=(ev/'b-before-fifth'/rel).read_bytes();rawbase=subprocess.check_output(['git','show',f'{d["base"]}:{rel}'],cwd=r);assert before==rawbase;base=ident(rawbase)
 rows.append({'relativePath':rel,'before':base,'after':{'path':str(out.relative_to(ev)),**ident(b)}})
 if before!=b:
  delta.append({'relativePath':rel,'fourthOrUnchangedGitBase':ident(before),'fifth':ident(b)})
  diff.extend(difflib.unified_diff(before.decode().splitlines(True),b.decode().splitlines(True),fromfile='fourth-or-base/'+rel,tofile='fifth/'+rel))
assert {x['relativePath'] for x in delta}==set(added)
with (ev/'b-fourth-to-fifth-minimal.diff').open('x') as f:f.writelines(diff)
report={'schema':1,'status':'FROZEN_FIXTURE_ONLY_NOT_COMPILED_OR_EXECUTED','atUtc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'base':d['base'],'parentFreeze':{'file':prior.name,**ident(prior.read_bytes())},'sourceFileCount':20,'source':rows,'changedSinceFourthOrUnchangedBase':delta,'minimalDiff':{'path':'b-fourth-to-fifth-minimal.diff',**ident((ev/'b-fourth-to-fifth-minimal.diff').read_bytes())},'rustfmtCheckExitCode':s.returncode,'cargoInvoked':False,'windowsInvoked':False,'changes':['Account-management/profile component seeds complete first password change using their existing trusted Local fixture lease and real guarded credential transaction.','Each of the three account seed operations reacquires a fresh local global-admin fixture lease; no use of the session revoked by the preceding local password-change login.'],'preserved':['All eighteen B4 source files byte-identical','Production Web negative execution gate and native readiness unchanged','Later account/Web/role/scope/revocation/profile CAS/reopen assertions unchanged','All actual full-second FAIL reports/raw/operation preserved'],'authorization':'Root explicitly authorized these two additional test-fixture files after read-only B-fixture-origin-review-first.json.'}
with (ev/'b-implementation-freeze-fifth.json').open('x') as f:json.dump(report,f,ensure_ascii=False,indent=2);f.write('\n')
print(json.dumps({'sourceFiles':len(rows),'deltaFiles':len(delta),'sha256':ident((ev/'b-implementation-freeze-fifth.json').read_bytes())['sha256'],'minimalDiffBytes':(ev/'b-fourth-to-fifth-minimal.diff').stat().st_size}))
