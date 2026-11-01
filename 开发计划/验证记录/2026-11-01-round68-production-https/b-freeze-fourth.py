from pathlib import Path
import subprocess,json,hashlib,datetime,difflib
r=Path('/tmp/fc-round68-production-https/worktree');ev=r.parent/'evidence'
prior=ev/'b-implementation-freeze-third.json';d=json.loads(prior.read_text());paths=[x['relativePath'] for x in d['source']]
cmd=['rustfmt','--check','--edition','2021','--config','skip_children=true']+paths
s=subprocess.run(cmd,cwd=r,capture_output=True,timeout=60)
for n,b in [('stdout.log',s.stdout),('stderr.log',s.stderr)]:
 with (ev/f'b-rustfmt-check-fourth.{n}').open('xb') as f:f.write(b)
assert s.returncode==0,s.stderr[-2000:]
def ident(b):return {'bytes':len(b),'sha256':hashlib.sha256(b).hexdigest()}
rows=[];delta=[];diff=[]
for rel in paths:
 b=(r/rel).read_bytes();out=ev/'b-source-after-fourth'/rel;out.parent.mkdir(parents=True,exist_ok=True)
 with out.open('xb') as f:f.write(b)
 row=next(x for x in d['source'] if x['relativePath']==rel);before=(ev/row['after']['path']).read_bytes()
 rows.append({'relativePath':rel,'before':row['before'],'after':{'path':str(out.relative_to(ev)),**ident(b)}})
 if before!=b:
  delta.append({'relativePath':rel,'third':ident(before),'fourth':ident(b)})
  diff.extend(difflib.unified_diff(before.decode().splitlines(True),b.decode().splitlines(True),fromfile='third/'+rel,tofile='fourth/'+rel))
assert len(delta)==2
with (ev/'b-third-to-fourth-minimal.diff').open('x') as f:f.writelines(diff)
report={'schema':1,'status':'FROZEN_CLIPPY_FIX_NOT_COMPILED_OR_EXECUTED','atUtc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'base':d['base'],'parentFreeze':{'file':prior.name,**ident(prior.read_bytes())},'source':rows,'changedSinceThird':delta,'minimalDiff':{'path':'b-third-to-fourth-minimal.diff',**ident((ev/'b-third-to-fourth-minimal.diff').read_bytes())},'rustfmtCheckExitCode':s.returncode,'cargoInvoked':False,'windowsInvoked':False,'changes':['Equivalent saturating_add(2) instead of checked_add(2).unwrap_or(MAX) in atomic negative epoch','Named CleanupPollResult<T> type alias for existing original-ticket/drained result; no new lint allow'], 'unchanged':['private P2 scope','budgets/capacities/listener/authorization behavior','all other sixteen B source files','A and C files']}
with (ev/'b-implementation-freeze-fourth.json').open('x') as f:json.dump(report,f,ensure_ascii=False,indent=2);f.write('\n')
print(json.dumps({'sourceFiles':len(rows),'deltaFiles':len(delta),'sha256':ident((ev/'b-implementation-freeze-fourth.json').read_bytes())['sha256'],'minimalDiffBytes':(ev/'b-third-to-fourth-minimal.diff').stat().st_size}))
