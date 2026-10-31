from pathlib import Path
import subprocess,os,json,time,hashlib
base=Path('/tmp/fc-round40-ci-evidence');root=Path('/tmp/fc-round40-threadlocal-repro');assert (root/'.owner').read_text()=='future-classroom-owned-r40-threadlocal-repro/v1\n'
original=json.loads((base/'thread-local-repro-first.json').read_text())['source'];assert hashlib.sha256(original.encode()).hexdigest()=='bd0c616fe1f902c8f04fc532225f058ca31b950eb9266ce1a47b04ce10713eda'
env={**os.environ,'CARGO_BUILD_JOBS':'2','CARGO_TERM_COLOR':'never'}
for name in ['RUSTFLAGS','CARGO_ENCODED_RUSTFLAGS']:env.pop(name,None)
cmd=['cargo','+nightly-2026-10-29','clippy','-Z','build-std','--offline','--release','--target','i686-win7-windows-msvc','--','-D','warnings']
attribute='#[cfg_attr(all(clippy, target_os = "windows", target_vendor = "win7", target_env = "msvc"), expect(clippy::missing_const_for_thread_local, reason = "fixed nightly rebuilt Win7 std wraps even const TLS initializers in a non-const function"))]'
fixed=original.replace('static VALUE:',attribute+' static VALUE:')
sentinel=fixed+'std::thread_local! { static UNANNOTATED: Cell<bool> = Cell::new(false); }\npub fn sentinel() -> bool { UNANNOTATED.with(|value| value.get()) }\n'
r={'scope':'Narrow per-static Win7-only expectation regression: original const initializer unchanged, global strict warnings remain; independent unannotated non-const initializer must still be rejected. No target runtime.','command':cmd,'timeoutSeconds':900,'status':'RUNNING','cases':[]};out=base/'thread-local-repro-msvc-only.json';assert not out.exists();out.write_text(json.dumps(r,indent=2)+'\n')
for name,source,expected in [('msvc-scoped-expect',fixed,0),('msvc-strict-sentinel',sentinel,101)]:
 (root/'src/lib.rs').write_text(source);(base/('thread-local-'+name+'.rs')).write_text(source);start=time.monotonic()
 with (base/('thread-local-'+name+'.stdout.log')).open('xb') as a,(base/('thread-local-'+name+'.stderr.log')).open('xb') as b:
  p=subprocess.run(cmd,cwd=root,env=env,stdout=a,stderr=b,timeout=900)
 stderr=(base/('thread-local-'+name+'.stderr.log')).read_text();ok=p.returncode==expected
 if name=='msvc-strict-sentinel':ok=ok and 'missing_const_for_thread_local' in stderr and 'UNANNOTATED' in stderr
 r['cases'].append({'name':name,'source':source,'sourceSha256':hashlib.sha256(source.encode()).hexdigest(),'exitCode':p.returncode,'expectedExitCode':expected,'passed':ok,'elapsedSeconds':time.monotonic()-start});out.write_text(json.dumps(r,indent=2)+'\n');print(name,p.returncode,ok,flush=True)
 if not ok:r['status']='FAIL';out.write_text(json.dumps(r,indent=2)+'\n');raise SystemExit(1)
r['status']='PASS';out.write_text(json.dumps(r,indent=2)+'\n');print('SCOPED_EXPECT_AND_STRICT_SENTINEL_PASS',flush=True)
