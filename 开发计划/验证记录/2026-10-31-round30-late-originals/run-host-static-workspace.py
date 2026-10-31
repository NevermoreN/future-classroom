from pathlib import Path
import subprocess,json,hashlib,os,time,re,sys
root=Path.cwd();ev=Path('/tmp/fc-round30-evidence');prefix=sys.argv[1] if len(sys.argv)>1 else 'root-host-workspace-static-first';env={**os.environ,'CARGO_BUILD_JOBS':'2'}
def snapshot():
 paths=list(root.glob('crates/**/*.rs'))+list(root.glob('crates/**/Cargo.toml'))+[root/x for x in ['Cargo.toml','Cargo.lock','rust-toolchain.toml','.cargo/config.toml'] if (root/x).is_file()]
 return {str(p.relative_to(root)):hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(paths) if p.is_file()}
before=snapshot();common=['cargo','test','--workspace','--release','--locked','--target-dir','/tmp/fc-round30-core-target'];results=[];artifacts=[]
for name,cmd in [('compile',common+['--no-run','--message-format=json']),('run',common+['--','--test-threads=2']),('clippy',['cargo','clippy','--workspace','--all-targets','--release','--locked','--target-dir','/tmp/fc-round30-core-target','--','-D','warnings'])]:
 start=time.monotonic();out=ev/(prefix+('.compile.jsonl' if name=='compile' else '.'+name+'.log'));err=ev/(prefix+'.compile.stderr.log') if name=='compile' else None
 with out.open('xb') as f:
  if err:
   with err.open('xb') as e:r=subprocess.run(cmd,env=env,stdout=f,stderr=e)
  else:r=subprocess.run(cmd,env=env,stdout=f,stderr=subprocess.STDOUT)
 results.append({'name':name,'command':cmd,'exitCode':r.returncode,'elapsedSeconds':time.monotonic()-start,'stdout':str(out),'stderr':str(err) if err else 'merged'})
 if name=='compile' and r.returncode==0:
  for line in out.read_text().splitlines():
   row=json.loads(line)
   if row.get('reason')=='compiler-artifact' and row.get('executable') and row.get('profile',{}).get('test'):
    p=Path(row['executable']);artifacts.append({'path':str(p),'target':row['target']['name'],'sha256BeforeRun':hashlib.sha256(p.read_bytes()).hexdigest()})
 print(json.dumps(results[-1]),flush=True)
 if r.returncode:break
after=snapshot()
for a in artifacts:a['sha256AfterRun']=hashlib.sha256(Path(a['path']).read_bytes()).hexdigest()
log=ev/(prefix+'.run.log');groups=[]
if log.exists():
 for line in log.read_text().splitlines():
  match=re.search(r'test result: (\w+)\. (\d+) passed; (\d+) failed; (\d+) ignored;',line)
  if match:groups.append({'status':match[1],'passed':int(match[2]),'failed':int(match[3]),'ignored':int(match[4])})
report={'scope':'Current static integration Rust full Host workspace phase, not final full goal acceptance','commands':results,'sourceBefore':before,'sourceAfter':after,'changedInputs':[p for p in set(before)|set(after) if before.get(p)!=after.get(p)],'artifacts':artifacts,'testGroups':groups,'totalPassed':sum(g['passed'] for g in groups)};(ev/(prefix+'.json')).write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n')
assert len(results)==3 and all(x['exitCode']==0 for x in results) and not report['changedInputs'] and all(a['sha256BeforeRun']==a['sha256AfterRun'] for a in artifacts)
print(json.dumps({'passed':report['totalPassed'],'groups':len(groups),'programs':len(artifacts)}),flush=True)
