from pathlib import Path
import subprocess,json,hashlib,datetime,difflib
r=Path('/tmp/fc-round68-production-https/worktree');ev=r.parent/'evidence'
prior=ev/'b-implementation-freeze-second.json';d=json.loads(prior.read_text())
paths=[x['relativePath'] for x in d['source']]+['crates/fc-bootstrap/src/web_transfer/work/tests.rs']
cmd=['rustfmt','--check','--edition','2021','--config','skip_children=true']+paths
s=subprocess.run(cmd,cwd=r,capture_output=True,timeout=60)
for n,b in [('stdout.log',s.stdout),('stderr.log',s.stderr)]:
 with (ev/f'b-rustfmt-check-third.{n}').open('xb') as f:f.write(b)
assert s.returncode==0,s.stderr[-2000:]
def ident(b):return {'bytes':len(b),'sha256':hashlib.sha256(b).hexdigest()}
rows=[];delta=[];diff=[]
for rel in paths:
 b=(r/rel).read_bytes();out=ev/'b-source-after-third'/rel;out.parent.mkdir(parents=True,exist_ok=True)
 with out.open('xb') as f:f.write(b)
 row=next((x for x in d['source'] if x['relativePath']==rel),None)
 if row:before=(ev/row['after']['path']).read_bytes()
 else:before=subprocess.check_output(['git','show',f'{d["base"]}:{rel}'],cwd=r)
 base=subprocess.run(['git','show',f'{d["base"]}:{rel}'],cwd=r,capture_output=True)
 rows.append({'relativePath':rel,'before':None if base.returncode else ident(base.stdout),'after':{'path':str(out.relative_to(ev)),**ident(b)}})
 if before!=b:
  delta.append({'relativePath':rel,'secondOrGitBase':ident(before),'third':ident(b)})
  diff.extend(difflib.unified_diff(before.decode().splitlines(True),b.decode().splitlines(True),fromfile='second/'+rel,tofile='third/'+rel))
with (ev/'b-second-to-third-minimal.diff').open('x') as f:f.writelines(diff)
report={'schema':1,'status':'FROZEN_NOT_COMPILED_OR_EXECUTED','atUtc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'base':d['base'],'parentFreeze':{'file':prior.name,**ident(prior.read_bytes())},'source':rows,'changedSinceSecondOrGitBase':delta,'minimalDiff':{'path':'b-second-to-third-minimal.diff',**ident((ev/'b-second-to-third-minimal.diff').read_bytes())},'rustfmtCheckExitCode':s.returncode,'cargoInvoked':False,'windowsInvoked':False,'p1Fix':['Exact monotonic runtime_claim AtomicU64 independent of negative admission epoch; exact CAS release','physical_returned idempotent; old returned Drop never releases a replacement claim','quiescent/stale RuntimeOwner stop guarded; internal stop acts through exact original reservation','new regressions: old returned delayed Drop/repeated return during replacement Starting and Stopping; old negative stop during replacement Running'], 'compileFixes':['include! inner doc comments become ordinary comments','SessionLease from public fc_application identity import','VoiceService via public backend re-export','work test explicitly rejects unexpected Cleanup completion without weakening correlation'], 'notProven':['Rust typecheck/link','Windows execution','C/A integration behavior','Actual ordinary HTTPS probe and cleanup']}
with (ev/'b-implementation-freeze-third.json').open('x') as f:json.dump(report,f,ensure_ascii=False,indent=2);f.write('\n')
print(json.dumps({'sourceFiles':len(rows),'deltaFiles':len(delta),'sha256':ident((ev/'b-implementation-freeze-third.json').read_bytes())['sha256'],'minimalDiffBytes':(ev/'b-second-to-third-minimal.diff').stat().st_size}))
