from pathlib import Path
import datetime, hashlib, json, os, signal, subprocess, time
R=Path('/tmp/fc-round75-installer-maintenance');W=R/'worktree';E=R/'evidence';O=E/'n-typecheck-first.json'
assert not O.exists()
def sha(p): return hashlib.sha256(p.read_bytes()).hexdigest()
def inputs():
 names=set()
 for command in [['git','diff','--name-only','-z','HEAD'],['git','ls-files','--others','--exclude-standard','-z']]: names.update(s.decode() for s in subprocess.check_output(command,cwd=W).split(b'\0') if s)
 names.add('Cargo.lock')
 return [dict(path=n,bytes=(W/n).stat().st_size,sha256=sha(W/n)) for n in sorted(names) if (W/n).is_file()]
command=['rustup','run','1.97.1','cargo','check','--locked','--offline','--target','i686-pc-windows-gnu','--target-dir',str(R/'target'),'-p','fc-windows','--all-targets']
v=dict(status='RUNNING',scope='GNU all-target typecheck only, no target runtime/Windows',startedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),command=command,originalBudgetSeconds=900,sourceBefore=inputs());O.write_text(json.dumps(v,indent=2)+'\n')
out=E/'n-typecheck-first.stdout.log';err=E/'n-typecheck-first.stderr.log';begin=time.monotonic()
with out.open('xb') as stdout,err.open('xb') as stderr:
 child=subprocess.Popen(command,cwd=W,stdout=stdout,stderr=stderr,start_new_session=True);v['pid']=child.pid;O.write_text(json.dumps(v,indent=2)+'\n')
 try: code=child.wait(timeout=900);v['timedOut']=False
 except subprocess.TimeoutExpired:
  v['timedOut']=True;os.killpg(child.pid,signal.SIGTERM)
  try: code=child.wait(timeout=10)
  except subprocess.TimeoutExpired: os.killpg(child.pid,signal.SIGKILL);code=child.wait(timeout=10)
v.update(exitCode=code,actualExit=True,elapsedSeconds=time.monotonic()-begin,sourceAfter=inputs(),stdout=dict(path=out.name,sha256=sha(out)),stderr=dict(path=err.name,sha256=sha(err)));v['sourceUnchanged']=v['sourceBefore']==v['sourceAfter'];v['status']='PASS_TYPECHECK_NOT_RUNTIME' if code==0 and not v['timedOut'] and v['sourceUnchanged'] else 'FAIL';O.write_text(json.dumps(v,indent=2)+'\n');print(json.dumps(dict(status=v['status'],exitCode=code,elapsedSeconds=v['elapsedSeconds'],sourceUnchanged=v['sourceUnchanged'],reportSha256=sha(O))),flush=True);raise SystemExit(0 if v['status'].startswith('PASS') else 1)
