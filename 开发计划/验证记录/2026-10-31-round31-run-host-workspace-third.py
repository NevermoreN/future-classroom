from pathlib import Path
import subprocess,json,hashlib,time,os,re
root=Path('/tmp/fc-round31-device-worktree');ev=Path('/tmp/fc-round31-device-evidence');out=ev/'host-workspace-third.json';assert not out.exists()
def digest(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def snapshot():
 files=list((root/'crates').rglob('*'))+list((root/'.cargo').rglob('*'))+[root/n for n in ['Cargo.toml','Cargo.lock','rust-toolchain.toml']]
 return {p.relative_to(root).as_posix():{'bytes':p.stat().st_size,'sha256':digest(p)} for p in sorted(set(files)) if p.is_file() and '__pycache__' not in p.parts}
r={'scope':'Current isolated Round31 Rust/Cargo full Host compile/run/strict Clippy; no Windows or native-device evidence','sourceBefore':snapshot(),'commands':[],'status':'RUNNING'}
def save():out.write_text(json.dumps(r,indent=2)+'\n')
save();environment={**os.environ,'CARGO_BUILD_JOBS':'2','CARGO_TERM_COLOR':'never'};target='/tmp/fc-round31-device-target'
commands=[('compile',['cargo','test','--workspace','--release','--locked','--target-dir',target,'--no-run','--message-format=json']),('run',['cargo','test','--workspace','--release','--locked','--target-dir',target,'--','--test-threads=2']),('clippy',['cargo','clippy','--workspace','--all-targets','--release','--locked','--target-dir',target,'--','-D','warnings'])]
for name,command in commands:
 start=time.monotonic();print('HOST_STAGE_STARTED '+name,flush=True)
 stdout=ev/('host-workspace-third.'+name+'.stdout.log');stderr=ev/('host-workspace-third.'+name+'.stderr.log')
 with stdout.open('xb') as a,stderr.open('xb') as b: result=subprocess.run(command,cwd=root,env=environment,stdout=a,stderr=b)
 step={'name':name,'command':command,'exitCode':result.returncode,'elapsedSeconds':time.monotonic()-start,'stdout':str(stdout),'stderr':str(stderr)};r['commands'].append(step);r['sourceAfter']=snapshot();r['changedInputs']=[p for p in set(r['sourceBefore'])|set(r['sourceAfter']) if r['sourceBefore'].get(p)!=r['sourceAfter'].get(p)];save();print('HOST_STAGE_DONE '+name+' exit='+str(result.returncode),flush=True)
 if result.returncode or r['changedInputs']:r['status']='FAIL';save();raise SystemExit(1)
 if name=='compile':
  artifacts=[]
  for line in stdout.read_text().splitlines():
   try:row=json.loads(line)
   except json.JSONDecodeError:continue
   if row.get('reason')=='compiler-artifact' and row.get('executable') and row.get('profile',{}).get('test'):
    p=Path(row['executable']);artifacts.append({'path':str(p),'bytes':p.stat().st_size,'sha256':digest(p)})
  r['artifacts']=artifacts;save()
 if name=='run':
  data=stdout.read_text()+stderr.read_text();groups=re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;',data);r['testGroups']=[{'passed':int(a),'failed':int(b),'ignored':int(c)} for a,b,c in groups];r['totalPassed']=sum(x['passed'] for x in r['testGroups']);save()
r['status']='PASS';save();print(json.dumps({'status':'PASS','totalPassed':r.get('totalPassed'),'artifacts':len(r.get('artifacts',[]))}),flush=True)
