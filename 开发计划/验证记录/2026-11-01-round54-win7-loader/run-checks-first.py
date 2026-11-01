from pathlib import Path
import datetime,hashlib,json,os,signal,subprocess,time
W=Path('/tmp/fc-round54-win7-loader-worktree');E=Path('/tmp/fc-round54-win7-loader-evidence');T=Path('/tmp/fc-round54-win7-loader-target');REPORT=E/'checks-first.json';LIMIT=16*1024*1024
sha=lambda b:hashlib.sha256(b).hexdigest()
def inputs():
 data=subprocess.check_output(['git','ls-files','--cached','--others','--exclude-standard','-z'],cwd=W);paths=[s for s in data.decode().split('\0') if s and (s in {'Cargo.toml','Cargo.lock','rust-toolchain.toml','.cargo/config.toml'} or s.startswith('crates/') and (s.endswith('.rs') or s.endswith('/Cargo.toml')))];return {p:{'bytes':len((b:=(W/p).read_bytes())),'sha256':sha(b)} for p in sorted(paths)}
def live_group(group):
 found=[]
 for p in Path('/proc').iterdir():
  if not p.name.isdigit():continue
  try:
   text=(p/'stat').read_text();fields=text[text.rfind(')')+2:].split()
   if int(fields[2])==group and fields[0]!='Z':found.append(int(p.name))
  except (OSError,ValueError):pass
 return found
report={'status':'RUNNING','scope':'R54 isolated3-file Win7 loader import delta; local GNU crosscompile and exact compiler lint evidence only. No Windows/SSH/CI/EXE execution. Native-only semantic cases are compiled here and reserved for actual Windows runs.','jobs':2,'testThreads':2,'perCommandTimeoutSeconds':900,'sourceBefore':inputs(),'gitHeadBefore':subprocess.check_output(['git','rev-parse','HEAD'],cwd=W).decode().strip(),'commands':[]}
assert not REPORT.exists()
def save():REPORT.write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n')
env=os.environ.copy();env['CARGO_BUILD_JOBS']='2';env['RUST_TEST_THREADS']='2';env['CARGO_TARGET_DIR']=str(T/'stable');env['RUSTFLAGS']='-C target-feature=+crt-static';env.pop('CARGO_ENCODED_RUSTFLAGS',None)
report['compilerEnvironment']={'CARGO_BUILD_JOBS':'2','RUST_TEST_THREADS':'2','RUSTFLAGS':env['RUSTFLAGS'],'CARGO_ENCODED_RUSTFLAGS':'ABSENT','scope':'Only child process environment; no system/project config mutation.'}
base=['--workspace','--all-targets','--release','--locked','--offline','--target','i686-pc-windows-gnu']
commands=[('versions-stable',['rustup','run','1.97.1','rustc','--version','--verbose']),('versions-nightly',['rustup','run','nightly-2026-10-29','rustc','--version','--verbose']),('fmt',['cargo','+1.97.1','fmt','--all','--','--check']),('architecture',['python3','scripts/check-architecture.py']),('diffcheck',['git','diff','--check']),('windows-compile',['cargo','+1.97.1','test',*base,'--target-dir',str(T/'stable'),'--no-run','--message-format=json']),('windows-product',['cargo','+1.97.1','build','-p','fc-bootstrap','--bin','fc-bootstrap','--release','--locked','--offline','--target','i686-pc-windows-gnu','--target-dir',str(T/'stable'),'--message-format=json']),('windows-stable-strict',['cargo','+1.97.1','clippy',*base,'--target-dir',str(T/'stable'),'--','-D','warnings']),('windows-nightly-strict',['cargo','+nightly-2026-10-29','clippy',*base,'--target-dir',str(T/'nightly'),'--','-D','warnings'])]
save()
try:
 for name,args in commands:
  stdout=E/(name+'-first.stdout.log');stderr=E/(name+'-first.stderr.log');started=time.monotonic();row={'name':name,'args':args,'timeoutSeconds':900,'status':'RUNNING','startedUtc':datetime.datetime.now(datetime.timezone.utc).isoformat()};report['commands'].append(row);save();print(json.dumps({'phase':name,'status':'RUNNING'}),flush=True)
  with stdout.open('xb') as out,stderr.open('xb') as err:
   process=subprocess.Popen(args,cwd=W,env=env,stdout=out,stderr=err,start_new_session=True);group=process.pid;row['ownedProcessGroup']=group;row['timedOut']=False;row['logBoundExceeded']=False;save()
   while process.poll() is None:
    if time.monotonic()-started>=900 or stdout.stat().st_size>LIMIT or stderr.stat().st_size>LIMIT:
     row['timedOut']=time.monotonic()-started>=900;row['logBoundExceeded']=stdout.stat().st_size>LIMIT or stderr.stat().st_size>LIMIT;os.killpg(group,signal.SIGTERM)
     try:process.wait(timeout=10)
     except subprocess.TimeoutExpired:os.killpg(group,signal.SIGKILL);process.wait(timeout=10)
     break
    time.sleep(.25)
   row['exitCode']=process.wait();row['actualExit']=True
  remaining=live_group(group)
  if remaining:
   os.killpg(group,signal.SIGTERM)
   for _ in range(40):
    if not live_group(group):break
    time.sleep(.25)
  row['liveOwnedGroupRemaining']=live_group(group);row['elapsedSeconds']=time.monotonic()-started
  for kind,p in [('stdout',stdout),('stderr',stderr)]:row[kind]={'path':str(p),'bytes':p.stat().st_size,'sha256':sha(p.read_bytes())}
  row['status']='PASS' if row['exitCode']==0 and not row['timedOut'] and not row['logBoundExceeded'] and not row['liveOwnedGroupRemaining'] else 'FAIL';save();print(json.dumps({'phase':name,'status':row['status'],'exitCode':row['exitCode'],'elapsedSeconds':round(row['elapsedSeconds'],2)}),flush=True)
  if row['status']!='PASS':raise RuntimeError('original command failed: '+name)
 report['status']='PASS_COMMANDS_ONLY_PE_AUDIT_PENDING_WINDOWS_NOT_RUN'
except BaseException as error:report.update(status='FAIL',error={'type':type(error).__name__,'message':str(error)[:300]})
report['sourceAfter']=inputs();report['gitHeadAfter']=subprocess.check_output(['git','rev-parse','HEAD'],cwd=W).decode().strip();report['changedInputs']=[p for p in set(report['sourceBefore'])|set(report['sourceAfter']) if report['sourceBefore'].get(p)!=report['sourceAfter'].get(p)]
if report['changedInputs'] or report['gitHeadBefore']!=report['gitHeadAfter']:report['status']='FAIL_SOURCE_CHANGED_DURING_VALIDATION'
save();print(json.dumps({'status':report['status'],'report':str(REPORT),'sha256':sha(REPORT.read_bytes()),'sourceCount':len(report['sourceAfter'])}),flush=True)
raise SystemExit(0 if report['status'].startswith('PASS') else 1)
