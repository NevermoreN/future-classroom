from pathlib import Path
import datetime,hashlib,json,os,signal,struct,subprocess,time
R=Path('/tmp/fc-round73-installer-broker');W=R/'worktree';E=R/'evidence';O=E/'n-release-first.json';assert not O.exists()
F=E/'n-implementation-freeze-second.json';f=json.loads(F.read_text())
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
for row in f['source']:assert sha(W/row['path'])==row['sha256']
def inputs():
 names=set()
 for c in [['git','diff','--name-only','-z','HEAD'],['git','ls-files','--others','--exclude-standard','-z']]:names.update(s.decode() for s in subprocess.check_output(c,cwd=W).split(b'\0') if s)
 names.add('Cargo.lock')
 return [dict(path=n,bytes=(W/n).stat().st_size,sha256=sha(W/n)) for n in sorted(names) if (W/n).is_file()]
v=dict(status='RUNNING',scope='Fixed nightly strictClippy, stable1.97.1 target release code generation and PE inspection only. No Windows/target-EXE execution, CI dispatch, network or main mutation. Cargo may run its ordinary host build scripts.',originalPerCommandBudgetSeconds=900,sourceBefore=inputs(),freezeSha256=sha(F),startedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),steps=[])
def save():O.write_text(json.dumps(v,indent=2)+'\n')
save();start=time.monotonic();common=['--locked','--offline','--target','i686-pc-windows-gnu','--target-dir',str(R/'target')]
commands=[('nightly-workspace-strict-clippy',['rustup','run','nightly-2026-10-29','cargo','clippy',*common,'--workspace','--all-targets','--','-D','warnings']),('stable-release-tests',['rustup','run','1.97.1','cargo','test',*common,'--release','--workspace','--all-targets','--no-run','--message-format=json']),('stable-release-bins',['rustup','run','1.97.1','cargo','build',*common,'--release','-p','fc-bootstrap','--bins','--message-format=json'])]
for name,command in commands:
 out=E/('n-'+name+'-first.stdout.log');err=E/('n-'+name+'-first.stderr.log');assert not out.exists() and not err.exists()
 step=dict(name=name,command=command,status='RUNNING');v['steps'].append(step);save();begin=time.monotonic()
 with out.open('xb') as stdout,err.open('xb') as stderr:
  child=subprocess.Popen(command,cwd=W,stdout=stdout,stderr=stderr,start_new_session=True);step['pid']=child.pid;save()
  try:code=child.wait(timeout=900);step['timedOut']=False
  except subprocess.TimeoutExpired:
   step['timedOut']=True;os.killpg(child.pid,signal.SIGTERM)
   try:code=child.wait(timeout=10)
   except subprocess.TimeoutExpired:os.killpg(child.pid,signal.SIGKILL);code=child.wait(timeout=10)
 step.update(status='PASS' if code==0 and not step['timedOut'] else 'FAIL',exitCode=code,actualExit=True,elapsedSeconds=time.monotonic()-begin,stdout=dict(path=out.name,sha256=sha(out),bytes=out.stat().st_size),stderr=dict(path=err.name,sha256=sha(err),bytes=err.stat().st_size));save()
 if step['status']!='PASS':break
v.update(sourceAfter=inputs(),elapsedSeconds=time.monotonic()-start);v['sourceUnchanged']=v['sourceBefore']==v['sourceAfter'];v['status']='PASS_CODEGEN_NOT_RUNTIME' if len(v['steps'])==3 and all(s['status']=='PASS' for s in v['steps']) and v['sourceUnchanged'] else 'FAIL'
def artifacts(name,test):
 rows=[]
 for line in (E/('n-'+name+'-first.stdout.log')).read_text().splitlines():
  item=json.loads(line)
  if item.get('reason')=='compiler-artifact' and item.get('executable') and item.get('profile',{}).get('test') is test:
   p=Path(item['executable']);assert p.resolve().is_relative_to((R/'target').resolve()) and p.is_file()
   b=p.read_bytes();assert b[:2]==b'MZ';offset=struct.unpack_from('<I',b,0x3c)[0];assert b[offset:offset+4]==b'PE\0\0';machine=struct.unpack_from('<H',b,offset+4)[0];magic=struct.unpack_from('<H',b,offset+24)[0];subsystem=struct.unpack_from('<H',b,offset+24+68)[0]
   assert machine==0x14c and magic==0x10b and subsystem==(3 if test else 2)
   rows.append(dict(target=item['target']['name'],kind=item['target']['kind'],packageId=item['package_id'],manifestPath=item['manifest_path'],profile=item['profile'],path=str(p),bytes=len(b),sha256=sha(p),peMachine=machine,peOptionalMagic=magic,peSubsystem=subsystem))
 return rows
if v['status']=='PASS_CODEGEN_NOT_RUNTIME':
 try:
  tests=artifacts('stable-release-tests',True);bins=artifacts('stable-release-bins',False)
  assert len(tests)==22 and len({r['path'] for r in tests})==22
  assert len(bins)==2 and {r['target'] for r in bins}=={'fc-bootstrap','fc-installer-lifecycle'}
  for row in bins:
   assert '/fc-bootstrap/Cargo.toml' in row['manifestPath'] and row['profile']['test'] is False
   out=E/('n-normal-'+row['target']+'-objdump-first.stdout.log');err=E/('n-normal-'+row['target']+'-objdump-first.stderr.log');assert not out.exists() and not err.exists()
   command=['i686-w64-mingw32-objdump','-p',row['path']]
   with out.open('xb') as stdout,err.open('xb') as stderr:completed=subprocess.run(command,stdout=stdout,stderr=stderr,timeout=25,check=True)
   import re
   assert re.search(r'^Subsystem\s+00000002\s+\(Windows GUI\)',out.read_text(),re.M)
   row['independentObjdump']=dict(command=command,exitCode=completed.returncode,stdout=dict(path=out.name,sha256=sha(out)),stderr=dict(path=err.name,sha256=sha(err)))
  v.update(testHarnessCount=len(tests),testExecutables=tests,normalExecutableCount=len(bins),normalExecutables=bins)
 except Exception as error:v.update(status='FAIL_ARTIFACT_INSPECTION',artifactError=type(error).__name__)
save();print(json.dumps(dict(status=v['status'],steps=[{k:s[k] for k in ['name','exitCode','actualExit','elapsedSeconds']} for s in v['steps']],sourceUnchanged=v['sourceUnchanged'],testHarnessCount=v.get('testHarnessCount'),normalExecutableCount=v.get('normalExecutableCount'),reportSha256=sha(O))),flush=True)
raise SystemExit(0 if v['status']=='PASS_CODEGEN_NOT_RUNTIME' else 1)
