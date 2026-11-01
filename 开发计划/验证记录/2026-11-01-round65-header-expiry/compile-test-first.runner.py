from pathlib import Path
import os,json,hashlib,subprocess,time,signal,datetime
ROOT=Path('/tmp/fc-round65-header-expiry/worktree');EV=ROOT.parent/'evidence';TARGET=ROOT.parent/'target'
def sha(data):return hashlib.sha256(data).hexdigest()
def write(name,obj):
 with (EV/name).open('x',encoding='utf-8') as f:json.dump(obj,f,ensure_ascii=False,indent=2);f.write('\n')
def snapshot():
 paths={p for p in (ROOT/'crates').rglob('*') if p.is_file() and not p.is_symlink() and (p.suffix=='.rs' or p.name=='Cargo.toml')}
 for n in ['Cargo.toml','Cargo.lock','rust-toolchain.toml','.cargo/config.toml','build.rs']:
  p=ROOT/n
  if p.is_file():paths.add(p)
 entries=[]
 for p in sorted(paths):
  b=p.read_bytes();entries.append({'path':str(p.relative_to(ROOT)),'bytes':len(b),'sha256':sha(b)})
 return {'scope':'All crate Rust/Cargo.toml plus root Cargo/lock/toolchain/config/build.rs, same capture as check-first','count':len(entries),'canonicalSha256':sha(json.dumps(entries,ensure_ascii=False,sort_keys=True,separators=(',',':')).encode()),'files':entries}
freeze=EV/'implementation-freeze-first.json';assert sha(freeze.read_bytes())=='4fb7c4d38ad99e66da3f5949fe590af7c055eb01365c905f5e4bc1c6af2c08a3'
for item in json.loads(freeze.read_bytes())['source']:
 b=(ROOT/item['relativePath']).read_bytes();assert len(b)==item['after']['bytes'] and sha(b)==item['after']['sha256']
before=snapshot();write('compile-test-first.source-before.json',before)
cmd=['cargo','test','--locked','--offline','-p','fc-windows','--target','i686-pc-windows-gnu','--no-run','--message-format=json']
env=os.environ.copy();env['CARGO_TARGET_DIR']=str(TARGET);env['CARGO_BUILD_JOBS']='2'
started=datetime.datetime.now(datetime.timezone.utc).isoformat();start=time.monotonic();timeout=False
with (EV/'compile-test-first.stdout.log').open('xb') as out,(EV/'compile-test-first.stderr.log').open('xb') as err:
 p=subprocess.Popen(cmd,cwd=ROOT,env=env,stdout=out,stderr=err,start_new_session=True)
 write('compile-test-first.invocation.json',{'command':cmd,'cwd':str(ROOT),'originalPid':p.pid,'startedUtc':started,'budgetSeconds':900,'environmentOverrides':{'CARGO_TARGET_DIR':str(TARGET),'CARGO_BUILD_JOBS':'2'},'runnerSha256':sha(Path(__file__).read_bytes())})
 try:exit_code=p.wait(timeout=900)
 except subprocess.TimeoutExpired:
  timeout=True;os.killpg(p.pid,signal.SIGKILL);exit_code=p.wait()
elapsed=time.monotonic()-start
after=snapshot();write('compile-test-first.source-after.json',after)
a={x['path']:x for x in before['files']};b={x['path']:x for x in after['files']};changed=[n for n in sorted(set(a)|set(b)) if a.get(n)!=b.get(n)]
artifacts=[];messages=[];parse_errors=[]
for n,line in enumerate((EV/'compile-test-first.stdout.log').read_bytes().splitlines(),1):
 try:r=json.loads(line)
 except Exception:parse_errors.append(n);continue
 if r.get('reason')=='compiler-message':messages.append(r)
 if r.get('reason')=='compiler-artifact' and r.get('target',{}).get('name')=='fc_windows' and r.get('profile',{}).get('test') and r.get('executable'):
  q=Path(r['executable']);data=q.read_bytes();artifacts.append({'path':str(q),'bytes':len(data),'sha256':sha(data),'target':r['target'],'profile':r['profile'],'filenames':r['filenames'],'packageId':r['package_id']})
streams={}
for s in ['stdout','stderr']:
 q=EV/f'compile-test-first.{s}.log';data=q.read_bytes();streams[s]={'path':str(q),'bytes':len(data),'sha256':sha(data)}
report={'status':'PASS_ADAPTER_TEST_COMPILE_LINK_ONLY' if exit_code==0 and not timeout and not changed and len(artifacts)==1 and not parse_errors else 'FAIL_ADAPTER_TEST_COMPILE_LINK','command':cmd,'cwd':str(ROOT),'targetDirectory':str(TARGET),'originalPid':p.pid,'actualExit':True,'exitCode':exit_code,'timedOut':timeout,'elapsedSeconds':elapsed,'budgetSeconds':900,'startedUtc':started,'finishedUtc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'sourceBeforeCanonicalSha256':before['canonicalSha256'],'sourceAfterCanonicalSha256':after['canonicalSha256'],'inputCount':before['count'],'changedInputs':changed,'freezeSha256':sha(freeze.read_bytes()),'artifacts':artifacts,'compilerMessageCount':len(messages),'parseErrorLines':parse_errors,'streams':streams,'scope':'One offline fc-windows test build/link. EXE never executed; no all-workspace normal product, profile/RUSTFLAGS change, test runtime, Clippy, Windows, CI, network, strict5s acceptance.'}
write('compile-test-first.json',report);print(json.dumps(report,ensure_ascii=False),flush=True)
