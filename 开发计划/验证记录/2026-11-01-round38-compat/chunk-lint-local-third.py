from pathlib import Path
import subprocess,json,hashlib,os,time
root=Path.cwd();ev=Path('/tmp/fc-round37-ci-evidence');out=ev/'chunk-lint-local-third.json';assert not out.exists() and root==Path('/tmp/fc-round38-win7-worktree')
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def inputs():
 files=list((root/'crates').rglob('*.rs'))+list((root/'crates').rglob('Cargo.toml'))+[p for p in (root/'.cargo').rglob('*') if p.is_file()]+[root/n for n in ['Cargo.toml','Cargo.lock','rust-toolchain.toml']]
 return {p.relative_to(root).as_posix():{'bytes':p.stat().st_size,'sha256':sha(p)} for p in sorted(set(files))}
r={'status':'RUNNING','scope':'R38 frozen source: strict fixed-nightly whole-workspace Windows GNU cross lint, then same full workspace fixed stable release Host tests and strict lint; jobs2/tests2/original900s each unchanged; owned TMPDIR isolated. Original slow debug run intentionally cancelled and recorded separately. Not formal Win7 MSVC/Win10/visible Seewo runtime evidence.','sourceBefore':inputs(),'commands':[]}
def save():out.write_text(json.dumps(r,indent=2)+'\n')
env={**os.environ,'CARGO_BUILD_JOBS':'2','RUST_TEST_THREADS':'2','CARGO_TERM_COLOR':'never','TMPDIR':'/dev/shm/fc-round38-release-tests','TMP':'/dev/shm/fc-round38-release-tests','TEMP':'/dev/shm/fc-round38-release-tests'};env.pop('RUSTFLAGS',None);env.pop('CARGO_ENCODED_RUSTFLAGS',None);save()
commands=[('nightly-version',['rustc','+nightly-2026-10-29','-vV']),('stable-version',['rustc','+1.97.1','-vV']),('fmt',['cargo','fmt','--all','--','--check']),('architecture',['python3','-B','scripts/check-architecture.py']),('nightly-windows-clippy',['cargo','+nightly-2026-10-29','clippy','--workspace','--all-targets','--release','--locked','--target','i686-pc-windows-gnu','--target-dir','/tmp/fc-round38-lint-target','--','-D','warnings']),('host-test',['cargo','+1.97.1','test','--workspace','--all-targets','--release','--locked','--target-dir','/tmp/fc-round38-host-target','--','--test-threads=2']),('stable-host-clippy',['cargo','+1.97.1','clippy','--workspace','--all-targets','--release','--locked','--target-dir','/tmp/fc-round38-host-target','--','-D','warnings'])]
for name,command in commands:
 print('LOCAL_STAGE_STARTED '+name,flush=True);start=time.monotonic();stdout=ev/('chunk-lint-local-third.'+name+'.stdout.log');stderr=ev/('chunk-lint-local-third.'+name+'.stderr.log')
 with stdout.open('xb') as a,stderr.open('xb') as b:
  try:p=subprocess.run(command,cwd=root,env=env,stdout=a,stderr=b,timeout=900)
  except subprocess.TimeoutExpired:p=subprocess.CompletedProcess(command,124);r['timedOutStage']=name
 r['commands'].append({'name':name,'command':command,'exitCode':p.returncode,'elapsedSeconds':time.monotonic()-start,'timeoutSeconds':900,'stdout':str(stdout),'stderr':str(stderr)})
 r['sourceAfter']=inputs();r['changedInputs']=[n for n in set(r['sourceBefore'])|set(r['sourceAfter']) if r['sourceBefore'].get(n)!=r['sourceAfter'].get(n)];save();print('LOCAL_STAGE_DONE '+name+' '+str(p.returncode),flush=True)
 if p.returncode or r['changedInputs']:r['status']='FAIL';save();raise SystemExit(1)
r['status']='PASS';save();print('LOCAL_ALL_PASS',flush=True)
