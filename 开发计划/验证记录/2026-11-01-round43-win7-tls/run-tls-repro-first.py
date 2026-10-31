from pathlib import Path
import datetime, hashlib, json, os, re, shutil, subprocess, time, uuid
base=Path('/tmp/fc-round43-win7-tls-expect-evidence')
out=base/'tls-repro-first.json';assert not out.exists()
root=Path('/tmp')/('fc-round43-tls-repro-'+uuid.uuid4().hex);root.mkdir();marker='future-classroom-owned-r43-tls-repro/v1\n'
(root/'.owner').write_text(marker);(root/'src').mkdir()
manifest='[package]\nname = "fc-r43-tls-repro"\nversion = "0.0.0"\nedition = "2021"\nrust-version = "1.97.1"\n\n[workspace]\n'
(root/'Cargo.toml').write_text(manifest)
attribute='#[cfg_attr(all(clippy, target_os = "windows", target_vendor = "win7", target_env = "msvc"), expect(clippy::missing_const_for_thread_local, reason = "nightly-2026-10-29 rebuilt Win7 std wraps this const initializer in a non-const function"))]'
block='''    use std::{cell::RefCell, sync::{Arc, atomic::{AtomicBool, Ordering}}};
    pub struct TlsExit(pub Arc<AtomicBool>);
    impl Drop for TlsExit { fn drop(&mut self) { self.0.store(true, Ordering::Release); } }
    std::thread_local! {
        static EXIT: RefCell<Option<TlsExit>> = const { RefCell::new(None) };
    }
    pub fn touch() -> bool { EXIT.with(|slot| slot.borrow().is_none()) }
'''
original='pub mod reset {\n'+block+'}\npub mod runtime {\n'+block+'}\npub mod phase {\n    use std::cell::Cell;\n    std::thread_local! {\n        static PHASE: Cell<&\'static str> = const { Cell::new("startup") };\n    }\n    pub fn touch() -> &\'static str { PHASE.with(|slot| slot.get()) }\n}\n'
fixed=original.replace('        static EXIT:', '        '+attribute+'\n        static EXIT:').replace('        static PHASE:', '        '+attribute+'\n        static PHASE:')
assert fixed.count(attribute)==3
sentinel=fixed+'\nstd::thread_local! { static UNANNOTATED: std::cell::Cell<bool> = std::cell::Cell::new(false); }\npub fn sentinel() -> bool { UNANNOTATED.with(|slot| slot.get()) }\n'
sha=lambda b:hashlib.sha256(b).hexdigest()
report={'status':'RUNNING','scope':'Dependency-free three const-TLS prototypes; exact fixed-nightly Win7 build-std strict Clippy, no EXE/native execution or whole-workspace build','ownerRoot':str(root),'ownerMarker':marker,'jobs':2,'totalBudgetSeconds':900,'cases':[],'versions':[],'cleanup':{'status':'PENDING'},'startedUtc':datetime.datetime.now(datetime.timezone.utc).isoformat()}
start=time.monotonic();limit=start+900
command=['cargo','+nightly-2026-10-29','clippy','-Z','build-std','--offline','--release','--target','i686-win7-windows-msvc','--','-D','warnings']
env={**os.environ,'CARGO_BUILD_JOBS':'2','CARGO_INCREMENTAL':'0','CARGO_TERM_COLOR':'never','CARGO_TARGET_DIR':str(root/'target')}
for name in ['RUSTFLAGS','CARGO_ENCODED_RUSTFLAGS','RUSTC_WRAPPER','RUSTC_WORKSPACE_WRAPPER']:env.pop(name,None)
def save():out.write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n')
save()
try:
 for name,argv in [('rustc',['rustc','+nightly-2026-10-29','-vV']),('cargo',['cargo','+nightly-2026-10-29','-V']),('clippy',['cargo','+nightly-2026-10-29','clippy','-V'])]:
  p=subprocess.run(argv,cwd=root,env=env,capture_output=True,timeout=min(15,max(0.01,limit-time.monotonic())))
  facts=[]
  for suffix,data in [('stdout.log',p.stdout),('stderr.log',p.stderr)]:
   f=base/('tls-repro-version-'+name+'.'+suffix)
   with f.open('xb') as w:w.write(data)
   facts.append({'path':str(f),'bytes':len(data),'sha256':sha(data)})
  report['versions'].append({'argv':argv,'exitCode':p.returncode,'originals':facts});save();assert p.returncode==0
 for name,source,expected,diagnostics in [('original-three',original,101,3),('scoped-three',fixed,0,0),('strict-sentinel',sentinel,101,1)]:
  data=source.encode();archive=base/('tls-repro-'+name+'.rs')
  with archive.open('xb') as w:w.write(data)
  (root/'src/lib.rs').write_bytes(data)
  case={'name':name,'command':command,'source':{'path':str(archive),'bytes':len(data),'sha256':sha(data)},'expectedExitCode':expected,'expectedMissingConstCount':diagnostics,'timeoutSeconds':max(0.01,limit-time.monotonic())}
  stdout=base/('tls-repro-'+name+'.stdout.log');stderr=base/('tls-repro-'+name+'.stderr.log')
  began=time.monotonic()
  with stdout.open('xb') as a,stderr.open('xb') as b:
   p=subprocess.run(command,cwd=root,env=env,stdout=a,stderr=b,timeout=case['timeoutSeconds'])
  s=stderr.read_text(errors='replace');count=len(re.findall(r'^error: initializer for `thread_local` value can be made `const`',s,re.M))
  case.update({'exitCode':p.returncode,'elapsedSeconds':time.monotonic()-began,'missingConstCount':count,'originals':[{'path':str(f),'bytes':f.stat().st_size,'sha256':sha(f.read_bytes())} for f in [stdout,stderr]],'passed':p.returncode==expected and count==diagnostics})
  if name=='strict-sentinel':case['passed']=case['passed'] and 'UNANNOTATED' in s
  report['cases'].append(case);save();print(name,p.returncode,count,case['passed'],flush=True)
  assert case['passed'],name
 report['status']='PASS_EXACT_NIGHTLY_WIN7_CLIPPY_RED_GREEN_SENTINEL_NO_RUNTIME'
except Exception as error:
 report['status']='FAIL';report['error']={'type':type(error).__name__,'message':str(error)}
finally:
 # Preserve all tiny input files and Cargo-generated text before removing only
 # the marker-bound scratch. Evidence original files are never deleted.
 if root.exists():
  assert root.parent==Path('/tmp') and root.name.startswith('fc-round43-tls-repro-') and not root.is_symlink()
  assert (root/'.owner').read_text()==marker
  report['scratchInputs']=[]
  for f in [root/'.owner',root/'Cargo.toml',root/'Cargo.lock',root/'src/lib.rs']:
   if f.exists():
    data=f.read_bytes();dest=base/('tls-repro-final-'+f.name.replace('.','_')+'.txt')
    with dest.open('xb') as w:w.write(data)
    report['scratchInputs'].append({'originalPath':str(f),'path':str(dest),'bytes':len(data),'sha256':sha(data)})
  shutil.rmtree(root)
  report['cleanup']={'status':'PASS' if not root.exists() else 'FAIL','root':str(root),'existsAfter':root.exists(),'scope':'Only exact owned marker-bound scratch, including target; no product files/global compiler/cache/SSH changes'}
 report['elapsedSeconds']=time.monotonic()-start;report['completedUtc']=datetime.datetime.now(datetime.timezone.utc).isoformat();save()
print(report['status'],report['cleanup']['status'],flush=True)
raise SystemExit(0 if report['status'].startswith('PASS') and report['cleanup']['status']=='PASS' else 1)
