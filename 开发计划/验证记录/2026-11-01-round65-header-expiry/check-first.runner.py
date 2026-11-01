from pathlib import Path
import json,hashlib,os,subprocess,time,signal,datetime
ROOT=Path('/tmp/fc-round65-header-expiry/worktree')
EV=ROOT.parent/'evidence'
TARGET=ROOT.parent/'target'
def sha(data):return hashlib.sha256(data).hexdigest()
def write(name, obj):
 path=EV/name
 with path.open('x',encoding='utf-8') as out:json.dump(obj,out,ensure_ascii=False,indent=2);out.write('\n')
def manifest():
 paths=set()
 for path in (ROOT/'crates').rglob('*'):
  if path.is_file() and not path.is_symlink() and (path.suffix=='.rs' or path.name=='Cargo.toml'):paths.add(path)
 for name in ['Cargo.toml','Cargo.lock','rust-toolchain.toml','.cargo/config.toml','build.rs']:
  path=ROOT/name
  if path.is_file():paths.add(path)
 entries=[]
 for path in sorted(paths):
  data=path.read_bytes();entries.append({'path':str(path.relative_to(ROOT)),'bytes':len(data),'sha256':sha(data)})
 canonical=json.dumps(entries,ensure_ascii=False,sort_keys=True,separators=(',',':')).encode()
 return {'scope':'All crate .rs/Cargo.toml plus root Cargo/lock/toolchain/config/build.rs; excludes target/evidence','count':len(entries),'canonicalSha256':sha(canonical),'files':entries}
freeze=EV/'implementation-freeze-first.json'
assert sha(freeze.read_bytes())=='4fb7c4d38ad99e66da3f5949fe590af7c055eb01365c905f5e4bc1c6af2c08a3'
frozen=json.loads(freeze.read_bytes())
for item in frozen['source']:
 data=(ROOT/item['relativePath']).read_bytes();assert len(data)==item['after']['bytes'] and sha(data)==item['after']['sha256']
before=manifest();write('check-first.source-before.json',before)
command=['cargo','check','--locked','--offline','--workspace','--all-targets','--target','i686-pc-windows-gnu']
env=os.environ.copy();env['CARGO_TARGET_DIR']=str(TARGET);env['CARGO_BUILD_JOBS']='2'
start=time.monotonic();started=datetime.datetime.now(datetime.timezone.utc).isoformat();timed_out=False
with (EV/'check-first.stdout.log').open('xb') as stdout,(EV/'check-first.stderr.log').open('xb') as stderr:
 process=subprocess.Popen(command,cwd=ROOT,env=env,stdout=stdout,stderr=stderr,start_new_session=True)
 write('check-first.invocation.json',{'command':command,'cwd':str(ROOT),'environmentOverrides':{'CARGO_TARGET_DIR':str(TARGET),'CARGO_BUILD_JOBS':'2'},'deadlineSeconds':900,'startedUtc':started,'originalPid':process.pid,'runnerSha256':sha(Path(__file__).read_bytes())})
 try:code=process.wait(timeout=900)
 except subprocess.TimeoutExpired:
  timed_out=True;os.killpg(process.pid,signal.SIGKILL);code=process.wait()
elapsed=time.monotonic()-start
after=manifest();write('check-first.source-after.json',after)
old={r['path']:r for r in before['files']};new={r['path']:r for r in after['files']}
changed=[p for p in sorted(set(old)|set(new)) if old.get(p)!=new.get(p)]
streams={}
for name in ['stdout','stderr']:
 path=EV/f'check-first.{name}.log';data=path.read_bytes();streams[name]={'path':str(path),'bytes':len(data),'sha256':sha(data)}
report={'status':'PASS_CARGO_CHECK_ONLY' if code==0 and not timed_out and not changed else 'FAIL_CARGO_CHECK','command':command,'cwd':str(ROOT),'targetDirectory':str(TARGET),'originalPid':process.pid,'actualExit':True,'exitCode':code,'timedOut':timed_out,'budgetSeconds':900,'elapsedSeconds':elapsed,'startedUtc':started,'finishedUtc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'sourceBeforeCanonicalSha256':before['canonicalSha256'],'sourceAfterCanonicalSha256':after['canonicalSha256'],'inputCount':before['count'],'changedInputs':changed,'freezeSha256':sha(freeze.read_bytes()),'streams':streams,'scope':'One offline type/borrow check; no tests/Clippy/Windows/CI/network/runtime/strict5s acceptance. Separate owned target only.'}
write('check-first.json',report)
print(json.dumps(report,ensure_ascii=False),flush=True)
