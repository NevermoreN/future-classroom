from pathlib import Path
import datetime,hashlib,json,os,signal,subprocess,time,uuid,shutil,stat
E=Path('/tmp/fc-round34-accept-evidence');R=Path('/tmp/fc-round34-clippy-repro');REPORT=E/'clippy-repro-first.json';T='nightly-2026-10-29'
def sha(b):return hashlib.sha256(b).hexdigest()
def fact(p):
 b=p.read_bytes();return {'path':str(p),'bytes':len(b),'sha256':sha(b)}
def fresh(p,b):
 with p.open('xb') as f:f.write(b)
assert not REPORT.exists() and not R.exists() and not R.is_symlink()
report={'status':'RUNNING','scope':'Local fixed-nightly Linux host, dependency-free no_std library, Tier3 i686-win7-windows-gnu; diagnostic core-only build-std. No Windows/CI/product changes, executable runtime, or network.','startedUtc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'commands':[],'cleanup':'NOT_RUN','envWhitelist':{'CARGO_NET_OFFLINE':'true','CARGO_BUILD_JOBS':'2','RUSTFLAGS':'-C target-feature=+crt-static','CARGO_ENCODED_RUSTFLAGS':'ABSENT'},'root':str(R)}
with REPORT.open('x') as reportfile:
 def save():
  reportfile.seek(0);json.dump(report,reportfile,indent=2);reportfile.write('\n');reportfile.truncate();reportfile.flush()
 token=uuid.uuid4().hex;R.mkdir();fresh(R/'.owner',token.encode());identity=R.stat();report['rootIdentity']={'device':identity.st_dev,'inode':identity.st_ino,'markerSha256':sha(token.encode())}
 (R/'src').mkdir()
 fresh(R/'Cargo.toml',b'[package]\nname = "fc-clippy-build-std-repro"\nversion = "0.0.0"\nedition = "2021"\n\n[lib]\npath = "src/lib.rs"\n\n[workspace]\n')
 fresh(R/'src/lib.rs',b'#![no_std]\n\npub fn identity(value: u32) -> u32 { value }\n')
 report['fixture']=[dict(fact(p),utf8=p.read_text()) for p in [R/'Cargo.toml',R/'src/lib.rs']]
 env=os.environ.copy();env['CARGO_NET_OFFLINE']='true';env['CARGO_BUILD_JOBS']='2';env['CARGO_TERM_COLOR']='never';env['RUSTFLAGS']='-C target-feature=+crt-static';env.pop('CARGO_ENCODED_RUSTFLAGS',None)
 def run(name,args,timeout,target=None):
  out=E/(name+'.stdout.log');err=E/(name+'.stderr.log');assert not out.exists() and not err.exists()
  e=env.copy()
  if target:e['CARGO_TARGET_DIR']=str(R/target)
  row={'name':name,'argv':args,'cwd':str(R),'timeoutSeconds':timeout,'targetDirectory':str(R/target) if target else None,'exitCode':None,'timedOut':False,'startedUtc':datetime.datetime.now(datetime.timezone.utc).isoformat()};report['commands'].append(row);save();start=time.monotonic()
  with out.open('xb') as o,err.open('xb') as x:
   child=subprocess.Popen(args,cwd=R,env=e,stdout=o,stderr=x,start_new_session=True)
   try:row['exitCode']=child.wait(timeout=timeout)
   except subprocess.TimeoutExpired:
    row['timedOut']=True;os.killpg(child.pid,signal.SIGKILL);row['exitCode']=child.wait(timeout=10)
  row.update(elapsedSeconds=time.monotonic()-start,stdout=fact(out),stderr=fact(err));save();print(json.dumps({'name':name,'exitCode':row['exitCode'],'elapsedSeconds':row['elapsedSeconds'],'timedOut':row['timedOut']}),flush=True)
  return row
 try:
  for name,args in [('clippy-repro-rustc-version',['rustc','+'+T,'-vV']),('clippy-repro-cargo-version',['cargo','+'+T,'-vV']),('clippy-repro-clippy-version',['cargo','+'+T,'clippy','-V'])]:
   row=run(name,args,20);assert row['exitCode']==0
  common=['--offline','-vv','--lib','--target','i686-win7-windows-gnu','--','-D','warnings']
  first=run('clippy-repro-global-first',['cargo','+'+T,'-Z','build-std=core','clippy']+common,180,'target-global')
  second=run('clippy-repro-after-command-first',['cargo','+'+T,'clippy','-Z','build-std=core']+common,180,'target-after-command')
  a=Path(first['stderr']['path']).read_text();b=Path(second['stderr']['path']).read_text()
  report['observed']={'globalExit':first['exitCode'],'globalMissingCore':('E0463' in a and ("can't find crate for `core`" in a or 'could not find' in a)),'afterCommandExit':second['exitCode'],'afterCommandCoreCompilation':('Compiling core v' in b or 'Checking core v' in b),'independentTargets':True,'bothOffline':True,'bothOriginalBudgets180Seconds':True,'warningsDeniedBoth':True}
  report['status']='CAUSAL_REPRODUCED' if first['exitCode']!=0 and report['observed']['globalMissingCore'] and second['exitCode']==0 and not first['timedOut'] and not second['timedOut'] else 'REPRO_INCONCLUSIVE'
 except Exception as ex:
  report['status']='FAIL';report['error']={'kind':type(ex).__name__,'message':str(ex)}
 finally:
  report['finishedUtc']=datetime.datetime.now(datetime.timezone.utc).isoformat();save()
  # Only this marker/inode-bound diagnostic tree; retain all original text logs
  # and complete fixture text in evidence, not compiled/cache leftovers.
  try:
   now=R.lstat();assert stat.S_ISDIR(now.st_mode) and not R.is_symlink() and now.st_dev==identity.st_dev and now.st_ino==identity.st_ino and (R/'.owner').read_bytes()==token.encode()
   entries=list(R.rglob('*'));assert not any(p.is_symlink() for p in entries)
   report['cleanupFileCount']=sum(p.is_file() for p in entries);report['cleanupLogicalBytes']=sum(p.stat().st_size for p in entries if p.is_file());shutil.rmtree(R);assert not R.exists();report['cleanup']='PASS_OWNED_REPRO_ROOT_REMOVED'
  except Exception as ex:
   report['cleanup']='FAIL_RETAINED';report['cleanupError']={'kind':type(ex).__name__,'message':str(ex)}
  save()
print(json.dumps({'status':report['status'],'cleanup':report['cleanup'],'report':str(REPORT),'observed':report.get('observed')}),flush=True)
raise SystemExit(0 if report['status']=='CAUSAL_REPRODUCED' and report['cleanup'].startswith('PASS') else 1)
