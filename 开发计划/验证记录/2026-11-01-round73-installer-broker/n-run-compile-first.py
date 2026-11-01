from pathlib import Path
import datetime,hashlib,json,os,signal,subprocess,time
R=Path('/tmp/fc-round73-installer-broker');W=R/'worktree';E=R/'evidence'
O=E/'n-compile-first.json';assert not O.exists()
F=E/'n-implementation-freeze-first.json';f=json.loads(F.read_text())
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
for row in f['source']:assert sha(W/row['path'])==row['sha256']
rootfix=W/'crates/fc-bootstrap/src/application/web_editor.rs'
assert sha(rootfix)=='541c7743fff130df55d36675cdf4c57a13218db2b7ebc21a531239eeffa2f627'
def inputs():
 names=set()
 for command in [['git','diff','--name-only','-z','HEAD'],['git','ls-files','--others','--exclude-standard','-z']]:
  names.update(v.decode() for v in subprocess.check_output(command,cwd=W).split(b'\0') if v)
 names.add('Cargo.lock')
 return [dict(path=n,bytes=(W/n).stat().st_size,sha256=sha(W/n)) for n in sorted(names) if (W/n).is_file()]
v=dict(status='RUNNING',scope='GNU target typecheck and strictClippy only; no linker/runtime/Windows/CI dispatch/network/main mutation.',originalBudgetSeconds=900,sourceBefore=inputs(),freezeSha256=sha(F),startedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),steps=[])
def save():O.write_text(json.dumps(v,indent=2)+'\n')
save();start=time.monotonic();deadline=start+900
commands=[('helper-typecheck',['cargo','check','--locked','--offline','-p','fc-bootstrap','--bin','fc-installer-lifecycle','--target','i686-pc-windows-gnu','--target-dir',str(R/'target')]),('workspace-strict-clippy',['cargo','clippy','--locked','--offline','--workspace','--all-targets','--target','i686-pc-windows-gnu','--target-dir',str(R/'target'),'--','-D','warnings']),('architecture',['python3','scripts/check-architecture.py']),('diff-check',['git','diff','--check'])]
for name,command in commands:
 out=E/('n-'+name+'-first.stdout.log');err=E/('n-'+name+'-first.stderr.log');assert not out.exists() and not err.exists()
 step=dict(name=name,command=command,status='RUNNING');v['steps'].append(step);save();begin=time.monotonic()
 with out.open('xb') as stdout,err.open('xb') as stderr:
  child=subprocess.Popen(command,cwd=W,stdout=stdout,stderr=stderr,start_new_session=True)
  step['pid']=child.pid;save()
  try:code=child.wait(timeout=max(.1,deadline-time.monotonic()));step['timedOut']=False
  except subprocess.TimeoutExpired:
   step['timedOut']=True;os.killpg(child.pid,signal.SIGTERM)
   try:code=child.wait(timeout=10)
   except subprocess.TimeoutExpired:os.killpg(child.pid,signal.SIGKILL);code=child.wait(timeout=10)
 step.update(status='PASS' if code==0 and not step['timedOut'] else 'FAIL',exitCode=code,actualExit=True,elapsedSeconds=time.monotonic()-begin,stdout=dict(path=out.name,sha256=sha(out),bytes=out.stat().st_size),stderr=dict(path=err.name,sha256=sha(err),bytes=err.stat().st_size));save()
 if step['status']!='PASS':break
v.update(sourceAfter=inputs(),elapsedSeconds=time.monotonic()-start)
v['sourceUnchanged']=v['sourceBefore']==v['sourceAfter'];v['status']='PASS_TYPECHECK_CLIPPY_ONLY_NOT_RUNTIME' if len(v['steps'])==len(commands) and all(s['status']=='PASS' for s in v['steps']) and v['sourceUnchanged'] else 'FAIL'
save();print(json.dumps(dict(status=v['status'],steps=[{k:s[k] for k in ['name','exitCode','actualExit','elapsedSeconds']} for s in v['steps']],sourceUnchanged=v['sourceUnchanged'],sourceCount=len(v['sourceBefore']),reportSha256=sha(O))),flush=True)
raise SystemExit(0 if v['status']=='PASS_TYPECHECK_CLIPPY_ONLY_NOT_RUNTIME' else 1)
