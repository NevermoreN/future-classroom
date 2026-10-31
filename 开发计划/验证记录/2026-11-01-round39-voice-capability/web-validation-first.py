from pathlib import Path
import subprocess,json,time,hashlib
root=Path('/tmp/fc-round39-voice-capability-worktree/web');ev=Path('/tmp/fc-round39-voice-capability-evidence');out=ev/'web-validation-first.json';assert not out.exists()
def inputs():
 files=list((root/'src').rglob('*'))+list((root/'tests').rglob('*'))+[root/'package.json',root/'package-lock.json',root/'vite.config.ts']
 return {str(p.relative_to(root)):{'bytes':p.stat().st_size,'sha256':hashlib.sha256(p.read_bytes()).hexdigest()} for p in files if p.is_file()}
r={'status':'RUNNING','scope':'R39 frozen teacher-Web source: npm locked install, full typecheck, all tests with two workers, static build/manifest budget. No native device or browser microphone runtime claim.','sourceBefore':inputs(),'commands':[]}
def save():out.write_text(json.dumps(r,indent=2)+'\n')
save()
for name,cmd in [('node-version',['node','--version']),('npm-version',['npm','--version']),('install',['npm','ci','--no-audit','--no-fund']),('typecheck',['npm','run','typecheck']),('test',['npm','test','--','--maxWorkers=2']),('build',['npm','run','build'])]:
 print('WEB_STAGE_STARTED '+name,flush=True);start=time.monotonic()
 with (ev/(name+'.stdout.log')).open('xb') as a,(ev/(name+'.stderr.log')).open('xb') as b:
  try:p=subprocess.run(cmd,cwd=root,stdout=a,stderr=b,timeout=900)
  except subprocess.TimeoutExpired:p=subprocess.CompletedProcess(cmd,124)
 r['commands'].append({'name':name,'command':cmd,'exitCode':p.returncode,'elapsedSeconds':time.monotonic()-start,'timeoutSeconds':900});r['sourceAfter']=inputs();r['changedInputs']=[n for n in set(r['sourceBefore'])|set(r['sourceAfter']) if r['sourceBefore'].get(n)!=r['sourceAfter'].get(n)];save();print('WEB_STAGE_DONE '+name+' '+str(p.returncode),flush=True)
 if p.returncode or r['changedInputs']:r['status']='FAIL';save();raise SystemExit(1)
r['status']='PASS';save();print('WEB_ALL_PASS',flush=True)
