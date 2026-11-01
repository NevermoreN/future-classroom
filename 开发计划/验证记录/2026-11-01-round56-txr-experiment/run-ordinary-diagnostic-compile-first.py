"""One GNU libtest diagnostic compile, not a product rebuild or Windows execution."""
from pathlib import Path
import datetime,hashlib,json,os,signal,subprocess,time
W=Path('/workspace/共享/Eliora/未来教室计划');E=Path('/tmp/fc-round56-transactional-autorun-evidence');T=Path('/tmp/fc-round56-transactional-autorun-target');OUT=E/'ordinary-diagnostic-compile-first.json';ROOT=E/'main-integration-first.json';DIAG=Path('/tmp/fc-round58-msvc-task-memory-evidence/diagnostic-main-integration-first.json');NORMAL=T/'stable/i686-pc-windows-gnu/release/fc-bootstrap.exe';CHANGED='crates/fc-windows/src/com_task/tests.rs';OTHER='crates/fc-windows/src/system/autorun_tests.rs';sha=lambda b:hashlib.sha256(b).hexdigest()
def fact(p):
 b=p.read_bytes();return {'path':str(p),'bytes':len(b),'sha256':sha(b)}
def inputs():
 names=subprocess.check_output(['git','ls-files','--cached','--others','--exclude-standard','-z'],cwd=W).split(b'\0');paths=sorted({b.decode() for b in names if b})
 return {p:{k:v for k,v in fact(W/p).items() if k!='path'} for p in paths if (p.startswith('crates/') and (p.endswith('.rs') or p.endswith('/Cargo.toml'))) or p in {'Cargo.toml','Cargo.lock','rust-toolchain.toml','.cargo/config.toml'}}
def extras(binding):return {p:{k:v for k,v in fact(W/p).items() if k!='path'} for p in binding['r57ExtraInputs']}
def group_members(g):
 result=[]
 for p in Path('/proc').iterdir():
  if not p.name.isdigit():continue
  try:
   s=(p/'stat').read_text();r=s[s.rfind(')')+2:].split()
   if int(r[2])==g and r[0]!='Z':result.append(int(p.name))
  except (FileNotFoundError,PermissionError,ProcessLookupError):pass
 return result
def save():
 p=E/'ordinary-diagnostic-compile-first.running.json';p.write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n');os.replace(p,OUT)
assert not OUT.exists()
binding=json.loads(ROOT.read_text());diag=json.loads(DIAG.read_text());assert fact(ROOT)['sha256']=='67fb56bdfa73cd70fb6fee8a999a14542b2463ffe382d1b9bb7cef1bc64d6bdc'
report={'status':'RUNNING','scope':'Only targetedfc_windows GNU diagnostic libtest compile. ExactlyR56+R58testdiagnostics differ; other430source andR57extras unchanged. NoWindows/tests/product/lints/CI execution.','mainBinding':fact(ROOT),'diagnosticBinding':fact(DIAG),'r56DiagnosticFreeze':fact(E/'ordinary-interleaving-diagnostic-freeze-first.json'),'sourceBefore':inputs(),'extraInputsBefore':extras(binding),'normalBefore':fact(NORMAL),'command':None}
save()
try:
 delta=[p for p in sorted(set(binding['sourceInputs'])|set(report['sourceBefore'])) if binding['sourceInputs'].get(p)!=report['sourceBefore'].get(p)]
 assert len(report['sourceBefore'])==432 and delta==sorted([CHANGED,OTHER]), 'unexpected actualsource change'
 assert diag['path']==CHANGED and diag['afterSha256']=='049252f97272e067cebd22c5ee8c8dced328e4ea02f2753c6f8a9e4bada75232' and report['sourceBefore'][CHANGED]=={'bytes':diag['bytes'],'sha256':diag['afterSha256']}
 assert report['sourceBefore'][OTHER]=={'bytes':32940,'sha256':'200425fdc63f1babae06db93274f3101fc404732fe6c7523d054961cf36b0266'}, 'R56 diagnostic candidate mismatch'
 assert report['extraInputsBefore']==binding['r57ExtraInputs'],'R57 changed'
 assert report['normalBefore']['sha256']=='040f5dd3096c32e8fe2597698c64015db1f4db49e56b4db0d7a910a0ec0fc117','normal before changed'
 args=['cargo','+1.97.1','test','-p','fc-windows','--lib','--release','--locked','--offline','--target','i686-pc-windows-gnu','--target-dir',str(T/'stable'),'--no-run','--message-format=json'];env=os.environ.copy();env.update(CARGO_BUILD_JOBS='2',RUST_TEST_THREADS='2',RUSTFLAGS='-C target-feature=+crt-static',CARGO_TARGET_DIR=str(T/'stable'));env.pop('CARGO_ENCODED_RUSTFLAGS',None)
 row={'argv':args,'status':'RUNNING','timeoutSeconds':900,'logBoundBytes':16*1024*1024,'timedOut':False,'logBoundExceeded':False,'startedUtc':datetime.datetime.now(datetime.timezone.utc).isoformat()};report['command']=row;started=time.monotonic();out=E/'ordinary-diagnostic-compile-first.stdout.log';err=E/'ordinary-diagnostic-compile-first.stderr.log'
 with out.open('xb') as so,err.open('xb') as se:
  child=subprocess.Popen(args,cwd=W,env=env,stdout=so,stderr=se,start_new_session=True);row['ownedProcessGroup']=child.pid;save();print(json.dumps({'phase':'ordinary-diagnostic-compile','status':'RUNNING','ownedGroup':child.pid}),flush=True)
  while child.poll() is None:
   row['timedOut']=time.monotonic()-started>=900;row['logBoundExceeded']=out.stat().st_size>16*1024*1024 or err.stat().st_size>16*1024*1024
   if row['timedOut'] or row['logBoundExceeded']:
    os.killpg(child.pid,signal.SIGTERM)
    try:child.wait(timeout=2)
    except subprocess.TimeoutExpired:os.killpg(child.pid,signal.SIGKILL);child.wait()
    break
   time.sleep(.2)
  code=child.wait()
 if group_members(child.pid):
  os.killpg(child.pid,signal.SIGTERM);time.sleep(.2)
  if group_members(child.pid):os.killpg(child.pid,signal.SIGKILL);time.sleep(.2)
 row.update(exitCode=code,actualExit=True,elapsedSeconds=time.monotonic()-started,stdout=fact(out),stderr=fact(err),liveOwnedGroupRemaining=group_members(child.pid));row['status']='PASS' if code==0 and not row['timedOut'] and not row['logBoundExceeded'] and not row['liveOwnedGroupRemaining'] else 'FAIL';assert row['status']=='PASS','original diagnostic compile failed'
 records=[]
 for line in out.read_text().splitlines():
  if not line.strip():continue
  r=json.loads(line)
  if r.get('reason')=='compiler-artifact' and r.get('executable') and r['profile']['test'] and r['target']['name']=='fc_windows':records.append((line,r))
 assert len(records)==1,'one unique fc_windows test artifact required'
 line,record=records[0];metadata=E/'ordinary-diagnostic-windows-test-metadata-first.jsonl';metadata.open('x').write(line+'\n');exe=Path(record['executable']);assert exe.is_relative_to(T/'stable') and not exe.is_symlink();report.update(status='PASS_TEST_ONLY_DIAGNOSTIC_COMPILED_NOT_EXECUTED',executable=fact(exe),metadata=fact(metadata),compilerRecord=record)
except BaseException as error:report.update(status='FAIL',error={'type':type(error).__name__,'message':str(error)[:500]})
finally:
 report['sourceAfter']=inputs();report['extraInputsAfter']=extras(binding);report['normalAfter']=fact(NORMAL);report['changedInputs']=[k for k in sorted(set(report['sourceBefore'])|set(report['sourceAfter'])) if report['sourceBefore'].get(k)!=report['sourceAfter'].get(k)];report['changedExtraInputs']=[k for k in binding['r57ExtraInputs'] if report['extraInputsBefore'].get(k)!=report['extraInputsAfter'].get(k)]
 if report['changedInputs'] or report['changedExtraInputs'] or report['normalBefore']!=report['normalAfter']:report['status']='FAIL_INPUT_OR_NORMAL_CHANGED'
 save()
print(json.dumps({'status':report['status'],'report':str(OUT),'sha256':sha(OUT.read_bytes()),'executable':report.get('executable'),'metadata':report.get('metadata'),'error':report.get('error')},ensure_ascii=False),flush=True)
raise SystemExit(0 if report['status'].startswith('PASS') else 1)
