import datetime, hashlib, json, os, pathlib, signal, subprocess, time
root=pathlib.Path('/tmp/fc-round68-production-https');ev=root/'evidence';wt=root/'native-check-worktree'
manifest=ev/'native-A-source-freeze-first.json';m=json.loads(manifest.read_text())
receipt=ev/'native-A-check-gnu-first.json'
def now(): return datetime.datetime.now(datetime.timezone.utc).isoformat()
def source():
    rows=[]
    for r in m['files']:
        b=(wt/r['path']).read_bytes();rows.append({'path':r['path'],'bytes':len(b),'sha256':hashlib.sha256(b).hexdigest()})
    return rows
before=source()
assert all(a['sha256']==b['sha256'] and a['bytes']==b['bytes'] for a,b in zip(before,m['files']))
command=['cargo','check','--locked','--offline','-p','fc-windows','--all-targets','--target','i686-pc-windows-gnu']
env=os.environ.copy();env['CARGO_TARGET_DIR']=str(root/'native-check-target')
out=ev/'native-A-check-gnu-first.stdout.log';err=ev/'native-A-check-gnu-first.stderr.log'
r={'status':'RUNNING','command':command,'cwd':str(wt),'targetDirectory':env['CARGO_TARGET_DIR'],'budgetSeconds':900,'startedUtc':now(),'sourceManifestSha256':hashlib.sha256(manifest.read_bytes()).hexdigest(),'baseCommit':m['baseCommit'],'sourceScope':'Exact detached base commit plus these twelve A-domain overlay files; no B/C overlay; package-scoped fc-windows all-targets GNU typecheck, not whole workspace or actual Windows execution.','sourceBefore':before}
receipt.write_text(json.dumps(r,indent=2)+'\n')
start=time.monotonic()
with out.open('wb') as stdout,err.open('wb') as stderr:
    child=subprocess.Popen(command,cwd=wt,env=env,stdout=stdout,stderr=stderr,start_new_session=True)
    r['cargoPid']=child.pid;receipt.write_text(json.dumps(r,indent=2)+'\n')
    try: code=child.wait(timeout=900);timed=False
    except subprocess.TimeoutExpired:
        timed=True;os.killpg(child.pid,signal.SIGKILL);code=child.wait()
r.update(status='TIMEOUT' if timed else ('PASS' if code==0 else 'FAIL'),exitCode=code,timedOut=timed,endedUtc=now(),elapsedSeconds=time.monotonic()-start,sourceAfter=source())
r['sourceUnchanged']=r['sourceBefore']==r['sourceAfter']
for label,p in [('stdout',out),('stderr',err)]:
    b=p.read_bytes();r[label]={'path':p.name,'bytes':len(b),'sha256':hashlib.sha256(b).hexdigest()}
receipt.write_text(json.dumps(r,indent=2)+'\n')
print(json.dumps({'status':r['status'],'exitCode':code,'elapsedSeconds':r['elapsedSeconds'],'sourceUnchanged':r['sourceUnchanged'],'receipt':str(receipt)}),flush=True)
