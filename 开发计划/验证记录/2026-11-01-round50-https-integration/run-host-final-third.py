from pathlib import Path
import json,hashlib,subprocess,os,signal,time,datetime
root=Path('/tmp/fc-round47-https-settings-worktree');ev=Path('/tmp/fc-round50-https-integration-evidence');host=Path('/tmp/fc-round47-https-settings-target');prior=ev/'full-matrix-second.json';p=json.loads(prior.read_text());assert p['status']=='FAIL' and p['commands'][-1]['name']=='host-tests' and p['commands'][-1]['timedOut'] and p['commands'][-1]['actualExit'];assert (host/'.owner').read_text()=='future-classroom-r47-core-host-target/v1\n'
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def snapshot():
 names=set(subprocess.check_output(['git','ls-files','--cached','--others','--exclude-standard','-z'],cwd=root).decode().split('\0'))
 return {n:{'bytes':(root/n).stat().st_size,'sha256':sha(root/n)} for n in sorted(names) if n and ((n.startswith('crates/') and (n.endswith('.rs') or n.endswith('/Cargo.toml'))) or n in ('Cargo.toml','Cargo.lock','rust-toolchain.toml','.cargo/config.toml'))}
before=snapshot();assert before==p['sourceBefore'];report=ev/'host-final-third.json';assert not report.exists();r={'status':'RUNNING','sourceBefore':before,'sourceHead':subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip(),'priorFullMatrix':{'path':str(prior),'bytes':prior.stat().st_size,'sha256':sha(prior)},'priorOriginalFailure':'Complete compile + tests hit original900s; original FAIL retained. No source, concurrency or per-command900s limit changed. Cached complete Host workspace tests and strictClippy now separate commands. No GNU test/normal rebuild needed.','jobs':2,'testThreads':2,'perCommandTimeoutSeconds':900,'commands':[],'tmpDirectoryNamesBefore':sorted(v.name for v in Path('/tmp').iterdir() if v.is_dir() and (v.name.startswith('fc-') or v.name.startswith('future-classroom')))}
def save():report.write_text(json.dumps(r,indent=2)+'\n')
save();env={**os.environ,'CARGO_BUILD_JOBS':'2','CARGO_TERM_COLOR':'never'}
for n in ('RUSTFLAGS','CARGO_ENCODED_RUSTFLAGS','CARGO_TARGET_DIR'):env.pop(n,None)
common=['--workspace','--all-targets','--release','--locked','--offline','--target-dir',str(host)]
for name,args in [('tests',['cargo','test',*common,'--','--test-threads=2']),('strict',['cargo','clippy',*common,'--','-D','warnings'])]:
 print('HOST_FINAL_STARTED '+name,flush=True);start=time.monotonic();a=ev/('host-final-third.'+name+'.stdout.log');b=ev/('host-final-third.'+name+'.stderr.log')
 with a.open('xb') as out,b.open('xb') as err:
  child=subprocess.Popen(args,cwd=root,env=env,stdout=out,stderr=err,start_new_session=True);timed=False
  try:child.wait(timeout=900)
  except subprocess.TimeoutExpired:
   timed=True;os.killpg(child.pid,signal.SIGTERM)
   try:child.wait(timeout=5)
   except subprocess.TimeoutExpired:os.killpg(child.pid,signal.SIGKILL);child.wait()
 row={'name':name,'args':args,'actualExit':True,'exitCode':child.returncode,'pid':child.pid,'ownedProcessGroup':child.pid,'timedOut':timed,'elapsedSeconds':time.monotonic()-start}
 for key,q in [('stdout',a),('stderr',b)]:row[key]={'path':str(q),'bytes':q.stat().st_size,'sha256':sha(q)}
 r['commands'].append(row);r['sourceAfter']=snapshot();r['changedInputs']=[n for n in before.keys()|r['sourceAfter'].keys() if before.get(n)!=r['sourceAfter'].get(n)];r['tmpDirectoryNamesAfter']=sorted(v.name for v in Path('/tmp').iterdir() if v.is_dir() and (v.name.startswith('fc-') or v.name.startswith('future-classroom')));save();print('HOST_FINAL_DONE '+name+' exit='+str(child.returncode),flush=True)
 if child.returncode or timed or r['changedInputs']:r['status']='FAIL';save();raise SystemExit(1)
r['status']='PASS_ALL_HOST_TARGET_TESTS_AND_STRICT';save();print(r['status'],flush=True)
