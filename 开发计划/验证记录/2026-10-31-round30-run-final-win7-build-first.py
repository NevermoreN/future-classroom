from pathlib import Path
import subprocess,sys,os,json,hashlib,time
root=Path.cwd();ev=Path('/tmp/fc-round30-evidence')
command=['cargo','build','-Z','build-std=std,panic_abort','-p','fc-bootstrap','--bin','fc-bootstrap','--release','--locked','--target','i686-win7-windows-gnu','--target-dir','/tmp/fc-round30-final-win7-target','--message-format=json']
def digest(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def snapshot():
 paths=set(root.glob('crates/**/*.rs'))|set(root.glob('crates/**/Cargo.toml'))|{root/p for p in ['Cargo.toml','Cargo.lock','rust-toolchain.toml','.cargo/config.toml','scripts/link-win7-gnu.sh']}
 return {str(p.relative_to(root)):digest(p) for p in sorted(paths) if p.is_file()}
before=snapshot();start=time.monotonic();environment={**os.environ,'CARGO_BUILD_JOBS':'2','RUSTC_BOOTSTRAP':'1','CC_i686_win7_windows_gnu':'i686-w64-mingw32-gcc','AR_i686_win7_windows_gnu':'i686-w64-mingw32-ar'}
print('SUPPLEMENTAL_WIN7_GNU_BUILD_STARTED; formal Windows7/10/MSVC runtime NOT_VALIDATED',flush=True)
with (ev/'final-win7-build-first.jsonl').open('xb') as out,(ev/'final-win7-build-first.stderr.log').open('xb') as err:
 result=subprocess.run(command,env=environment,stdout=out,stderr=err)
after=snapshot();artifacts=[]
for l in (ev/'final-win7-build-first.jsonl').read_text().splitlines():
 try:r=json.loads(l)
 except ValueError:continue
 if r.get('reason')=='compiler-artifact' and r.get('executable'):
  p=Path(r['executable']);artifacts.append({'path':str(p),'sha256':digest(p),'bytes':p.stat().st_size,'target':r['target'],'fresh':r.get('fresh')})
report={'schemaVersion':1,'scope':'Supplemental Win7 GNU compile of current sources only; not formal MSVC or real Windows7/Windows10 evidence','command':command,'taskEnvironment':{k:environment[k] for k in ['CARGO_BUILD_JOBS','RUSTC_BOOTSTRAP','CC_i686_win7_windows_gnu','AR_i686_win7_windows_gnu']},'exitCode':result.returncode,'elapsedSeconds':time.monotonic()-start,'sourceBefore':before,'sourceAfter':after,'changedInputs':[p for p in set(before)|set(after) if before.get(p)!=after.get(p)],'artifacts':artifacts}
with (ev/'final-win7-build-first.json').open('x') as f:json.dump(report,f,indent=2);f.write('\n')
print(json.dumps({'exitCode':result.returncode,'elapsedSeconds':report['elapsedSeconds'],'changedInputs':report['changedInputs'],'artifacts':artifacts}),flush=True)
raise SystemExit(result.returncode)
