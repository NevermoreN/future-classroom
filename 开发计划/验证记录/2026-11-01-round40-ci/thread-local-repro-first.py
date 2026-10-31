from pathlib import Path
import subprocess,os,json,time,hashlib
base=Path('/tmp/fc-round40-ci-evidence');root=Path('/tmp/fc-round40-threadlocal-repro');assert not root.exists();root.mkdir()
(root/'.owner').write_text('future-classroom-owned-r40-threadlocal-repro/v1\n')
(root/'src').mkdir();(root/'Cargo.toml').write_text('[package]\nname="fc-threadlocal-repro"\nversion="0.0.0"\nedition="2021"\n')
source='use std::cell::Cell;\nstd::thread_local! { static VALUE: Cell<bool> = const { Cell::new(false) }; }\npub fn read() -> bool { VALUE.with(|value| value.get()) }\n';(root/'src/lib.rs').write_text(source)
env={**os.environ,'CARGO_BUILD_JOBS':'2','CARGO_TERM_COLOR':'never'}
for name in ['RUSTFLAGS','CARGO_ENCODED_RUSTFLAGS']:env.pop(name,None)
cmd=['cargo','+nightly-2026-10-29','clippy','-Z','build-std','--offline','--release','--target','i686-win7-windows-msvc','--','-D','warnings']
r={'scope':'Minimal existing-const thread_local initializer against exact fixed nightly and rebuilt Win7 std, no target execution or product changes.','command':cmd,'source':source,'sourceSha256':hashlib.sha256(source.encode()).hexdigest(),'timeoutSeconds':900,'status':'RUNNING'};out=base/'thread-local-repro-first.json'
assert not out.exists();out.write_text(json.dumps(r,indent=2)+'\n');start=time.monotonic()
with (base/'thread-local-repro-first.stdout.log').open('xb') as a,(base/'thread-local-repro-first.stderr.log').open('xb') as b:
 try:p=subprocess.run(cmd,cwd=root,env=env,stdout=a,stderr=b,timeout=900);r.update(exitCode=p.returncode,status='TERMINAL')
 except subprocess.TimeoutExpired:r.update(exitCode=124,status='TIMEOUT')
r['elapsedSeconds']=time.monotonic()-start;out.write_text(json.dumps(r,indent=2)+'\n');print(json.dumps(r),flush=True)
