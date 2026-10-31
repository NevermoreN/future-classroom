from pathlib import Path
import sys,subprocess,time,json,hashlib,datetime

base=Path('/tmp/fc-round42-ci-evidence');name=sys.argv[1];command=sys.argv[2:]
assert name and all(c.isalnum() or c=='-' for c in name) and command
out=base/(name+'.command.json');stdout=base/(name+'.stdout.log');stderr=base/(name+'.stderr.log')
assert not any(p.exists() for p in [out,stdout,stderr])
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
r={'status':'RUNNING','command':command,'cwd':str(Path.cwd()),'startedUtc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'stdout':str(stdout),'stderr':str(stderr)}
out.write_text(json.dumps(r,indent=2)+'\n');start=time.monotonic()
with stdout.open('xb') as a,stderr.open('xb') as b:result=subprocess.run(command,stdout=a,stderr=b)
r.update(status='PASS' if result.returncode==0 else 'FAIL',exitCode=result.returncode,elapsedSeconds=time.monotonic()-start,completedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),stdoutBytes=stdout.stat().st_size,stdoutSha256=sha(stdout),stderrBytes=stderr.stat().st_size,stderrSha256=sha(stderr))
out.write_text(json.dumps(r,indent=2)+'\n')
print(json.dumps({k:r[k] for k in ['status','exitCode','elapsedSeconds','stdoutBytes','stderrBytes']}),flush=True)
raise SystemExit(result.returncode)
