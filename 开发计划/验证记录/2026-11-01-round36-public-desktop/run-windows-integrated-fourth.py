from pathlib import Path
import subprocess,json,hashlib,time,os,shutil

root=Path.cwd();ev=Path('/tmp/fc-round36-public-desktop-evidence')
out=ev/'windows-integrated-fourth.json';target='/tmp/fc-round35-visuals-target'
programs=Path('/tmp/fc-round36-public-desktop-programs-fourth')
assert not out.exists() and Path(target).exists() and not programs.exists()
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def snapshot():
    files=list((root/'crates').rglob('*.rs'))+list((root/'crates').rglob('Cargo.toml'))+list((root/'.cargo').rglob('*'))+[root/n for n in ['Cargo.toml','Cargo.lock','rust-toolchain.toml']]
    return {p.relative_to(root).as_posix():{'bytes':p.stat().st_size,'sha256':sha(p)} for p in sorted(set(files)) if p.is_file()}
r={'scope':'Whole-workspace Windows x86 tests compiled, actual product cross build, then strict whole-workspace Clippy. Source frozen throughout. Cross compilation alone is not runtime/MSVC/Win7/Win10 evidence.','gitHeadBefore':subprocess.check_output(['git','rev-parse','HEAD']).decode().strip(),'sourceBefore':snapshot(),'commands':[],'status':'RUNNING'}
def save():out.write_text(json.dumps(r,indent=2)+'\n')
environment={**os.environ,'CARGO_BUILD_JOBS':'2','CARGO_TERM_COLOR':'never'}
environment.pop('RUSTFLAGS',None);environment.pop('CARGO_ENCODED_RUSTFLAGS',None)
r['compilerFlagSource']='Child process flags unset; exact checked-in target rustflags supply static CRT. No global environment mutation.'
save()
commands=[
 ('rust-version',['rustc','-vV']),
 ('linker-version',['i686-w64-mingw32-gcc','--version']),
 ('fmt',['cargo','fmt','--all','--','--check']),
 ('architecture',['python3','scripts/check-architecture.py']),
 ('compile',['cargo','test','--workspace','--all-targets','--release','--locked','--target','i686-pc-windows-gnu','--target-dir',target,'--no-run','--message-format=json']),
 ('product',['cargo','build','-p','fc-bootstrap','--bin','fc-bootstrap','--release','--locked','--target','i686-pc-windows-gnu','--target-dir',target,'--message-format=json']),
 ('clippy',['cargo','clippy','--workspace','--all-targets','--release','--locked','--target','i686-pc-windows-gnu','--target-dir',target,'--','-D','warnings']),
]
for name,command in commands:
    print('AFFECTED_BUILD_STAGE_STARTED '+name,flush=True);start=time.monotonic()
    stdout=ev/('windows-integrated-fourth.'+name+'.stdout.log');stderr=ev/('windows-integrated-fourth.'+name+'.stderr.log')
    try:
        with stdout.open('xb') as a,stderr.open('xb') as b:result=subprocess.run(command,cwd=root,env=environment,stdout=a,stderr=b,timeout=900)
    except subprocess.TimeoutExpired:
        result=subprocess.CompletedProcess(command,124)
        r['timedOutStage']=name
        r['timeoutSeconds']=900
    r['commands'].append({'name':name,'command':command,'exitCode':result.returncode,'elapsedSeconds':time.monotonic()-start,'stdout':str(stdout),'stderr':str(stderr)})
    r['sourceAfter']=snapshot();r['gitHeadAfter']=subprocess.check_output(['git','rev-parse','HEAD']).decode().strip()
    r['changedInputs']=[p for p in set(r['sourceBefore'])|set(r['sourceAfter']) if r['sourceBefore'].get(p)!=r['sourceAfter'].get(p)]
    save();print('AFFECTED_BUILD_STAGE_DONE '+name+' exit='+str(result.returncode),flush=True)
    if result.returncode or r['changedInputs'] or r['gitHeadAfter']!=r['gitHeadBefore']:
        r['status']='FAIL';save();raise SystemExit(1)
programs.mkdir();r['artifacts']=[];metadata=[]
for name in ['compile','product']:
    for line in (ev/('windows-integrated-fourth.'+name+'.stdout.log')).read_text().splitlines():
        try:row=json.loads(line)
        except json.JSONDecodeError:continue
        if row.get('reason')!='compiler-artifact' or not row.get('executable'):continue
        if name=='compile' and not row.get('profile',{}).get('test'):continue
        if name=='product' and (row.get('profile',{}).get('test') or row.get('target',{}).get('name')!='fc-bootstrap'):continue
        source=Path(row['executable']);assert source.suffix.lower()=='.exe';dest=programs/source.name;assert not dest.exists()
        shutil.copyfile(source,dest);assert sha(source)==sha(dest)
        r['artifacts'].append({'path':str(dest),'originalPath':str(source),'bytes':dest.stat().st_size,'sha256':sha(dest),'test':row.get('profile',{}).get('test',False),'target':row['target']})
        if row.get('profile',{}).get('test'):row['executable']=str(dest);metadata.append(row)
assert len(metadata)==21 and len(r['artifacts'])==22
with (ev/'windows-test-metadata-fourth.jsonl').open('x') as f:
    for row in metadata:f.write(json.dumps(row)+'\n')
r['status']='PASS';save();print(json.dumps({'status':'PASS','testExecutables':len(metadata),'totalExecutables':len(r['artifacts'])}),flush=True)
