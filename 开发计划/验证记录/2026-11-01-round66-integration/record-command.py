from pathlib import Path
import os,sys,subprocess,time,json,hashlib,datetime
root=Path('/tmp/fc-round66-integration');e=root/'evidence';w=Path('/tmp/fc-round63-installer-owner/worktree')
name=sys.argv[1];cmd=sys.argv[2:];assert name and all(c.isalnum() or c=='-' for c in name) and cmd
out=e/(name+'.json');a=e/(name+'.stdout.log');b=e/(name+'.stderr.log');assert not any(p.exists() for p in [out,a,b])
freeze=e/'implementation-freeze-first.json';f=json.loads(freeze.read_text())
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
for row in f['allRustCargoInputs']:assert sha(w/row['path'])==row['sha256']
r={'status':'RUNNING','command':cmd,'cwd':str(w),'sourceFreezeSha256':sha(freeze),'originalBudgetSeconds':900,'startedUtc':datetime.datetime.now(datetime.timezone.utc).isoformat()};out.write_text(json.dumps(r,indent=2)+'\n');t=time.monotonic();env=os.environ.copy();env['CARGO_TARGET_DIR']='/tmp/fc-round63-installer-owner/target'
with a.open('xb') as x,b.open('xb') as y:
 try:ret=subprocess.run(cmd,cwd=w,env=env,stdout=x,stderr=y,timeout=900).returncode
 except subprocess.TimeoutExpired:ret=None
r.update(status='PASS' if ret==0 else ('FAIL_TIMEOUT' if ret is None else 'FAIL'),exitCode=ret,elapsedSeconds=time.monotonic()-t,stdoutBytes=a.stat().st_size,stdoutSha256=sha(a),stderrBytes=b.stat().st_size,stderrSha256=sha(b),sourceUnchanged=all(sha(w/row['path'])==row['sha256'] for row in f['allRustCargoInputs']))
out.write_text(json.dumps(r,indent=2)+'\n');print(json.dumps(r),flush=True);sys.exit(0 if ret==0 and r['sourceUnchanged'] else 1)
