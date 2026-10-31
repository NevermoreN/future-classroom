from pathlib import Path
import subprocess,json,hashlib,time,os,shutil

root=Path('/tmp/fc-round31-device-worktree')
ev=Path('/tmp/fc-round31-device-evidence')
out=ev/'windows-workspace-first.json'
assert not out.exists()
assert json.loads((ev/'host-workspace-third.json').read_bytes())['status']=='PASS', 'Wait for actual Host termination and success'
target='/tmp/fc-round31-device-target'
programs=Path('/tmp/fc-round31-device-programs')
assert not programs.exists()
def digest(p): return hashlib.sha256(p.read_bytes()).hexdigest()
def snapshot():
    files=list((root/'crates').rglob('*'))+list((root/'.cargo').rglob('*'))+[root/n for n in ['Cargo.toml','Cargo.lock','rust-toolchain.toml']]
    return {p.relative_to(root).as_posix():{'bytes':p.stat().st_size,'sha256':digest(p)} for p in sorted(set(files)) if p.is_file() and '__pycache__' not in p.parts}
r={'scope':'Modern i686 GNU cross compilation and strict Clippy only; actual Windows runtime evidence is separate; no Win7/Win10/MSVC certification claim','gitHeadBefore':subprocess.check_output(['git','rev-parse','HEAD'],cwd=root).decode().strip(),'sourceBefore':snapshot(),'commands':[],'status':'RUNNING'}
def save(): out.write_text(json.dumps(r,indent=2)+'\n')
save()
environment={**os.environ,'CARGO_BUILD_JOBS':'2','CARGO_TERM_COLOR':'never'}
commands=[
 ('rust-version',['rustc','-vV']),
 ('linker-version',['i686-w64-mingw32-gcc','--version']),
 ('compile',['cargo','test','--workspace','--all-targets','--release','--locked','--target','i686-pc-windows-gnu','--target-dir',target,'--no-run','--message-format=json']),
 ('product',['cargo','build','-p','fc-bootstrap','--bin','fc-bootstrap','--release','--locked','--target','i686-pc-windows-gnu','--target-dir',target,'--message-format=json']),
 ('clippy',['cargo','clippy','--workspace','--all-targets','--release','--locked','--target','i686-pc-windows-gnu','--target-dir',target,'--','-D','warnings']),
]
for name,command in commands:
    start=time.monotonic();print('WINDOWS_BUILD_STAGE_STARTED '+name,flush=True)
    stdout=ev/('windows-workspace-first.'+name+'.stdout.log');stderr=ev/('windows-workspace-first.'+name+'.stderr.log')
    with stdout.open('xb') as a,stderr.open('xb') as b:
        result=subprocess.run(command,cwd=root,env=environment,stdout=a,stderr=b)
    r['commands'].append({'name':name,'command':command,'exitCode':result.returncode,'elapsedSeconds':time.monotonic()-start,'stdout':str(stdout),'stderr':str(stderr)})
    r['sourceAfter']=snapshot()
    r['gitHeadAfter']=subprocess.check_output(['git','rev-parse','HEAD'],cwd=root).decode().strip()
    r['changedInputs']=[p for p in set(r['sourceBefore'])|set(r['sourceAfter']) if r['sourceBefore'].get(p)!=r['sourceAfter'].get(p)]
    save();print('WINDOWS_BUILD_STAGE_DONE '+name+' exit='+str(result.returncode),flush=True)
    if result.returncode or r['changedInputs'] or r['gitHeadAfter']!=r['gitHeadBefore']:
        r['status']='FAIL';save();raise SystemExit(1)

programs.mkdir()
r['artifacts']=[]
metadata=[]
for name in ['compile','product']:
    for line in (ev/('windows-workspace-first.'+name+'.stdout.log')).read_text().splitlines():
        try: row=json.loads(line)
        except json.JSONDecodeError: continue
        if row.get('reason')!='compiler-artifact' or not row.get('executable'): continue
        if name=='compile' and not row.get('profile',{}).get('test'): continue
        if name=='product' and (row.get('profile',{}).get('test') or row.get('target',{}).get('name')!='fc-bootstrap'): continue
        source=Path(row['executable']);assert source.suffix.lower()=='.exe'
        copied=programs/source.name;assert not copied.exists()
        shutil.copyfile(source,copied);assert digest(copied)==digest(source)
        r['artifacts'].append({'path':str(copied),'originalPath':str(source),'bytes':copied.stat().st_size,'sha256':digest(copied),'test':row.get('profile',{}).get('test',False),'target':row['target']})
        if row.get('profile',{}).get('test'):
            row['executable']=str(copied);metadata.append(row)
assert len(metadata)==21, 'Inspect actual all-targets workspace artifact set before remote execution'
assert sum(not x['test'] for x in r['artifacts'])==1
with (ev/'windows-test-metadata.jsonl').open('x') as f:
    for row in metadata: f.write(json.dumps(row)+'\n')
r['metadataScope']='Derived runtime path mapping only; original complete Cargo stdout retained separately'
r['status']='PASS';save()
print(json.dumps({'status':'PASS','testExecutables':len(metadata),'totalExecutables':len(r['artifacts'])}),flush=True)
