from pathlib import Path
import hashlib,json,os,signal,subprocess,time,shutil
root=Path('/tmp/fc-round47-https-settings-worktree');ev=Path('/tmp/fc-round50-https-integration-evidence');host=Path('/tmp/fc-round47-https-settings-target');cross=Path('/tmp/fc-round50-https-windows-target')
assert (host/'.owner').read_text()=='future-classroom-r47-core-host-target/v1\n'
assert (cross/'.owner').read_text()=='future-classroom-owned-r50-https-windows-target/v1\n'
assert json.loads((ev/'native-service-second.json').read_text())['status']=='FAIL'
assert (ev/'ui-integration-first.json').exists()
report=ev/'full-matrix-second.json';assert not report.exists()
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def snapshot():
 names=set(subprocess.check_output(['git','ls-files','--cached','--others','--exclude-standard','-z'],cwd=root).decode().split('\0'))
 return {n:{'bytes':(root/n).stat().st_size,'sha256':sha(root/n)} for n in sorted(names) if n and ((n.startswith('crates/') and (n.endswith('.rs') or n.endswith('/Cargo.toml'))) or n in ('Cargo.toml','Cargo.lock','rust-toolchain.toml','.cargo/config.toml'))}
r={'status':'RUNNING','scope':'Final R47 corefourth, R48 native/service2 with two minimal R50 compiler-order corrections, R49 UI9. Whole GNU workspace/all-target test compile+normal build+stable/nightly strict lint and whole Host all-target test execution. Does not claim Windows runtime or MSVC for this source.','jobs':2,'testThreads':2,'perCommandTimeoutSeconds':900,'sourceBefore':snapshot(),'gitHeadBefore':subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip(),'commands':[]}
def save():report.write_text(json.dumps(r,indent=2)+'\n')
save();env={**os.environ,'CARGO_BUILD_JOBS':'2','CARGO_TERM_COLOR':'never'}
for n in ('RUSTFLAGS','CARGO_ENCODED_RUSTFLAGS','CARGO_TARGET_DIR'):env.pop(n,None)
common=['--release','--locked','--offline'];target=['--target','i686-pc-windows-gnu','--target-dir',str(cross/'stable')]
commands=[('fmt',['cargo','fmt','--all','--','--check']),('architecture',['python3','scripts/check-architecture.py']),('compiler',['rustc','-vV']),('linker',['i686-w64-mingw32-gcc','--version']),('windows-compile',['cargo','test','--workspace','--all-targets',*common,*target,'--no-run','--message-format=json']),('windows-product',['cargo','build','-p','fc-bootstrap','--bin','fc-bootstrap',*common,*target,'--message-format=json']),('windows-strict',['cargo','clippy','--workspace','--all-targets',*common,*target,'--','-D','warnings']),('windows-nightly-strict',['cargo','+nightly-2026-10-29','clippy','--workspace','--all-targets',*common,'--target','i686-pc-windows-gnu','--target-dir',str(cross/'nightly'),'--','-D','warnings']),('host-tests',['cargo','test','--workspace','--all-targets',*common,'--target-dir',str(host),'--','--test-threads=2']),('host-strict',['cargo','clippy','--workspace','--all-targets',*common,'--target-dir',str(host),'--','-D','warnings'])]
for name,args in commands:
 a=ev/('full-matrix-second.'+name+'.stdout.log');b=ev/('full-matrix-second.'+name+'.stderr.log');print('FULL_MATRIX_STARTED '+name,flush=True);start=time.monotonic()
 with a.open('xb') as out,b.open('xb') as err:
  child=subprocess.Popen(args,cwd=root,env=env,stdout=out,stderr=err,start_new_session=True);timed=False
  try:child.wait(timeout=900)
  except subprocess.TimeoutExpired:
   timed=True;os.killpg(child.pid,signal.SIGTERM)
   try:child.wait(timeout=5)
   except subprocess.TimeoutExpired:os.killpg(child.pid,signal.SIGKILL);child.wait()
 row={'name':name,'args':args,'exitCode':child.returncode,'actualExit':True,'ownedProcessGroup':child.pid,'timedOut':timed,'elapsedSeconds':time.monotonic()-start}
 for key,p in [('stdout',a),('stderr',b)]:row[key]={'path':str(p),'bytes':p.stat().st_size,'sha256':sha(p)}
 r['commands'].append(row);r['sourceAfter']=snapshot();r['gitHeadAfter']=subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip();r['changedInputs']=[n for n in r['sourceBefore'].keys()|r['sourceAfter'].keys() if r['sourceBefore'].get(n)!=r['sourceAfter'].get(n)];save();print('FULL_MATRIX_DONE '+name+' exit='+str(child.returncode),flush=True)
 if child.returncode or timed or r['changedInputs'] or r['gitHeadAfter']!=r['gitHeadBefore']:r['status']='FAIL';save();raise SystemExit(1)
programs=Path('/tmp/fc-round50-https-programs-second');assert not programs.exists();programs.mkdir();(programs/'.owner').write_text('future-classroom-owned-r50-https-programs-second/v1\n');r['artifacts']=[];metadata=[]
for name in ('windows-compile','windows-product'):
 for line in (ev/('full-matrix-second.'+name+'.stdout.log')).read_text().splitlines():
  try:item=json.loads(line)
  except ValueError:continue
  if item.get('reason')!='compiler-artifact' or not item.get('executable'):continue
  test=item.get('profile',{}).get('test',False)
  if name=='windows-compile' and not test:continue
  if name=='windows-product' and (test or item['target']['name']!='fc-bootstrap'):continue
  p=Path(item['executable']);assert p.suffix=='.exe';dest=programs/p.name;assert not dest.exists();shutil.copyfile(p,dest);assert sha(p)==sha(dest)
  r['artifacts'].append({'path':str(dest),'originalPath':str(p),'bytes':dest.stat().st_size,'sha256':sha(dest),'test':test,'target':item['target']})
  if test:item['executable']=str(dest);metadata.append(item)
assert len(metadata)==21 and len(r['artifacts'])==22
with (ev/'windows-test-metadata-second.jsonl').open('x') as f:
 for item in metadata:f.write(json.dumps(item)+'\n')
r['status']='PASS_HOST_AND_CROSS_ONLY_RUNTIME_REQUIRED';save();print(r['status'],flush=True)
