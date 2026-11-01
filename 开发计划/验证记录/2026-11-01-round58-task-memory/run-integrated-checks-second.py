"""Owned sequential GNU compile/lint driver; no Windows or executable execution."""
from pathlib import Path
import datetime,hashlib,json,os,signal,subprocess,time
W=Path('/workspace/共享/Eliora/未来教室计划')
E=Path('/tmp/fc-round58-msvc-task-memory-evidence')
T=Path('/tmp/fc-round56-transactional-autorun-target')
REPORT=E/'integrated-checks-second.json'
sha=lambda b:hashlib.sha256(b).hexdigest()
def fact(p):
 b=p.read_bytes();return {'path':str(p),'bytes':len(b),'sha256':sha(b)}
def inputs():
 raw=subprocess.check_output(['git','ls-files','--cached','--others','--exclude-standard','-z'],cwd=W)
 names=sorted({p.decode() for p in raw.split(b'\0') if p})
 return {r:{k:v for k,v in fact(W/r).items() if k!='path'} for r in names if (r.startswith('crates/') and (r.endswith('.rs') or r.endswith('/Cargo.toml'))) or r in {'Cargo.toml','Cargo.lock','rust-toolchain.toml','.cargo/config.toml'}}
def group_members(group):
 members=[]
 for p in Path('/proc').iterdir():
  if not p.name.isdigit():continue
  try:
   s=(p/'stat').read_text();parts=s[s.rfind(')')+2:].split()
   if int(parts[2])==group and parts[0]!='Z':members.append(int(p.name))
  except (FileNotFoundError,PermissionError,ProcessLookupError):pass
 return members
def save():
 tmp=E/'integrated-checks-second.running.json';tmp.write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n');os.replace(tmp,REPORT)
assert not REPORT.exists()
BINDING=E/'main-integration-second.json'
assert sha(BINDING.read_bytes())=='ea74ee792c38975e1d70eb9a89e943c6ecf26d89b2b7bd0d84158765f3f6c9e8'
binding=json.loads(BINDING.read_text())
assert binding['sourceCount']==431 and len(binding['sourceInputs'])==431
assert binding['mainRoot']==str(W)
def extra_inputs():
 return {r:{k:v for k,v in fact(W/r).items() if k!='path'} for r in binding['r57ExtraInputs']}

assert (E/'.owner').read_text()=='future-classroom Round58 task-memory editor owned 2026-11-01\n'
assert (T/'.owner').read_text()=='future-classroom-owned-r56-transactional-autorun-target/v1\n'
env=os.environ.copy();env.update(CARGO_BUILD_JOBS='2',RUST_TEST_THREADS='2',RUSTFLAGS='-C target-feature=+crt-static',CARGO_TARGET_DIR=str(T/'stable'));env.pop('CARGO_ENCODED_RUSTFLAGS',None)
target=['--release','--locked','--offline','--target','i686-pc-windows-gnu','--target-dir',str(T/'stable')]
commands=[
 ('fmt',['cargo','+1.97.1','fmt','--all','--','--check']),
 ('architecture',['python3','scripts/check-architecture.py']),
 ('diffcheck',['git','diff','--check']),
 ('windows-fc-windows-compile',['cargo','+1.97.1','test','-p','fc-windows','--lib',*target,'--no-run','--message-format=json']),
 ('windows-product',['cargo','+1.97.1','build','-p','fc-bootstrap','--bin','fc-bootstrap',*target,'--message-format=json']),
 ('windows-stable-strict',['cargo','+1.97.1','clippy','--workspace','--all-targets',*target,'--','-D','warnings']),
 ('windows-nightly-strict',['cargo','+nightly-2026-10-29','clippy','--workspace','--all-targets',*['--release','--locked','--offline','--target','i686-pc-windows-gnu','--target-dir',str(T/'nightly')],'--','-D','warnings']),
]
report={'status':'RUNNING','worktree':str(W),'target':str(T),'base':subprocess.check_output(['git','rev-parse','HEAD'],cwd=W,text=True).strip(),'sourceBefore':inputs(),'commands':[],'scope':'Compile/lint only. No Windows/SSH/CI/EXE execution. Original900spercommand,16MiBperstream,jobs2,testthreads2.'}
report['integrationBinding']=fact(BINDING)
report['extraInputsBefore']=extra_inputs()
report['originalToolchainVersionsReport']=fact(Path('/tmp/fc-round56-transactional-autorun-evidence/checks-first.json'))
report['withdrawalProof']=binding['r56Withdrawal']
report['qualifiedScope']='R58 secondfreeze+R57 only. R56 was rejected and withdrawn; noR56 CAS/runtime success claim.'
save()
try:
 assert report['sourceBefore']==binding['sourceInputs'], 'actual431-source set/bytes differs from final R56withdrawn integration binding'
 assert report['extraInputsBefore']==binding['r57ExtraInputs'], 'R57 extra input mismatch'
 for name,args in commands:
  out=E/f'integrated-{name}-second.stdout.log';err=E/f'integrated-{name}-second.stderr.log';started=time.monotonic();row={'name':name,'argv':args,'status':'RUNNING','timeoutSeconds':900,'logBoundBytes':16*1024*1024,'startedUtc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'timedOut':False,'logBoundExceeded':False};report['commands'].append(row)
  with out.open('xb') as so,err.open('xb') as se:
   child=subprocess.Popen(args,cwd=W,env=env,stdout=so,stderr=se,start_new_session=True);row['ownedProcessGroup']=child.pid;save();print(json.dumps({'phase':name,'status':'RUNNING','ownedGroup':child.pid}),flush=True)
   while child.poll() is None:
    row['timedOut']=time.monotonic()-started>=900
    row['logBoundExceeded']=out.stat().st_size>16*1024*1024 or err.stat().st_size>16*1024*1024
    if row['timedOut'] or row['logBoundExceeded']:
     os.killpg(child.pid,signal.SIGTERM)
     try:child.wait(timeout=2)
     except subprocess.TimeoutExpired:os.killpg(child.pid,signal.SIGKILL);child.wait()
     break
    time.sleep(.2)
   code=child.wait()
  remaining=group_members(child.pid)
  if remaining:
   os.killpg(child.pid,signal.SIGTERM);time.sleep(.2)
   if group_members(child.pid):os.killpg(child.pid,signal.SIGKILL);time.sleep(.2)
  row.update(exitCode=code,actualExit=True,elapsedSeconds=time.monotonic()-started,stdout=fact(out),stderr=fact(err),liveOwnedGroupRemaining=group_members(child.pid))
  row['status']='PASS' if code==0 and not row['timedOut'] and not row['logBoundExceeded'] and not row['liveOwnedGroupRemaining'] else 'FAIL';save();print(json.dumps({'phase':name,'status':row['status'],'exitCode':code,'elapsedSeconds':round(row['elapsedSeconds'],2)}),flush=True)
  if row['status']!='PASS':raise RuntimeError('original command failed: '+name)
 report['status']='PASS_INTEGRATED_COMPILE_STRICT_LINT_ONLY_PE_AND_RUNTIME_NOT_RUN'
except BaseException as error:
 report.update(status='FAIL',error={'type':type(error).__name__,'message':str(error)[:500]})
finally:
 report['sourceAfter']=inputs();report['changedInputs']=[k for k in sorted(set(report['sourceBefore'])|set(report['sourceAfter'])) if report['sourceBefore'].get(k)!=report['sourceAfter'].get(k)]
 report['extraInputsAfter']=extra_inputs()
 report['changedExtraInputs']=[k for k in binding['r57ExtraInputs'] if report['extraInputsBefore'].get(k)!=report['extraInputsAfter'].get(k)]
 report['bindingAfter']=fact(BINDING)
 if report['changedInputs'] or report['changedExtraInputs'] or report['bindingAfter']!=report['integrationBinding']:report['status']='FAIL_SOURCE_CHANGED'
 save()
print(json.dumps({'status':report['status'],'report':str(REPORT),'sha256':sha(REPORT.read_bytes()),'sourceCount':len(report['sourceBefore'])}),flush=True)
raise SystemExit(0 if report['status'].startswith('PASS') else 1)
