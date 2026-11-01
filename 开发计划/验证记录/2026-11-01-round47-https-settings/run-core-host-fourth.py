#!/usr/bin/env python3
"""R47 sequential owned Host checks. No Windows/CI/network operations."""
import os,sys,json,subprocess,pathlib,hashlib,time,datetime,signal
ROOT=pathlib.Path('/tmp/fc-round47-https-settings-worktree'); EV=pathlib.Path('/tmp/fc-round47-https-settings-evidence');TARGET=pathlib.Path('/tmp/fc-round47-https-settings-target')
OWNER='future-classroom-r47-core-host-target/v1\n'
if not TARGET.is_dir() or (TARGET/'.owner').read_text()!=OWNER:raise RuntimeError('owned target mismatch')
def snapshot():
    raw=subprocess.check_output(['git','ls-files','--cached','--others','--exclude-standard','-z'],cwd=ROOT)
    names=set(raw.decode().split('\0')); paths=[]
    for name in sorted(names):
        if name and ((name.startswith('crates/') and (name.endswith('.rs') or name.endswith('/Cargo.toml'))) or name in ('Cargo.toml','Cargo.lock','rust-toolchain.toml','.cargo/config.toml')):
            data=(ROOT/name).read_bytes();paths.append({'path':name,'bytes':len(data),'sha256':hashlib.sha256(data).hexdigest()})
    return paths
before=snapshot(); source_changes=subprocess.check_output(['git','diff','--name-only','-z'],cwd=ROOT).decode().split('\0')
source_changes=[x for x in source_changes if x]
source_changes+=subprocess.check_output(['git','ls-files','--others','--exclude-standard','-z'],cwd=ROOT).decode().split('\0');source_changes=sorted(set(filter(None,source_changes)))
for name in source_changes:
    dest=EV/'implementation-after-fourth'/name;dest.parent.mkdir(parents=True,exist_ok=True);dest.write_bytes((ROOT/name).read_bytes())
(EV/'implementation-fourth.diff').write_bytes(subprocess.check_output(['git','diff'],cwd=ROOT))
with (EV/'core-source-freeze-fourth.json').open('x') as f:json.dump({'status':'FROZEN_TESTS_NOT_RUN','base':'ab5c403fa4f2cd90a19e3be52d7307ea408465d3','files':[x for x in before if x['path'] in source_changes],'all_compile_input_count':len(before),'scope':'Actual Rust/crate manifests plus root toolchain/Cargo configuration; not entire Git evidence history or compiler external dependencies.','modified_files':source_changes},f,indent=2);f.write('\n')
commands=[('fmt',['cargo','fmt','--all','--','--check']),('architecture',['python3','scripts/check-architecture.py']),('targeted-https',['cargo','test','--release','--locked','--offline','-p','fc-domain','-p','fc-ports','-p','fc-storage','-p','fc-bootstrap','--lib','https_settings','--','--test-threads=2']),('affected-host-libs',['cargo','test','--release','--locked','--offline','-p','fc-domain','-p','fc-ports','-p','fc-storage','-p','fc-bootstrap','--lib','--','--test-threads=2']),('affected-host-clippy',['cargo','clippy','--release','--locked','--offline','-p','fc-domain','-p','fc-ports','-p','fc-storage','-p','fc-bootstrap','--all-targets','--','-D','warnings'])]
env=os.environ.copy();env.update(CARGO_TARGET_DIR=str(TARGET),CARGO_BUILD_JOBS='2',CARGO_TERM_COLOR='never')
report={'status':'RUNNING','sourceBefore':before,'commands':[],'startedUtc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'environment':{'CARGO_TARGET_DIR':str(TARGET),'CARGO_BUILD_JOBS':'2','CARGO_TERM_COLOR':'never'},'timeoutSecondsPerCommand':900,'runtime':'HOST_ONLY_NOT_WINDOWS'}
try:
    for name,argv in commands:
        start=time.monotonic();out=EV/f'core-host-fourth-{name}.stdout.log';err=EV/f'core-host-fourth-{name}.stderr.log'
        with out.open('xb') as stdout,err.open('xb') as stderr:
            child=subprocess.Popen(argv,cwd=ROOT,env=env,stdout=stdout,stderr=stderr,start_new_session=True)
            facts={'name':name,'argv':argv,'pid':child.pid,'ownedProcessGroup':child.pid,'timeoutSeconds':900,'startedUtc':datetime.datetime.now(datetime.timezone.utc).isoformat()}
            try: child.wait(timeout=900)
            except subprocess.TimeoutExpired:
                facts['timeout']=True;os.killpg(child.pid,signal.SIGTERM)
                try:child.wait(timeout=5)
                except subprocess.TimeoutExpired:os.killpg(child.pid,signal.SIGKILL);child.wait()
        facts.update(exitCode=child.returncode,durationSeconds=time.monotonic()-start,actualExit=True)
        for key,path in [('stdout',out),('stderr',err)]:
            data=path.read_bytes();facts[key]={'path':str(path),'bytes':len(data),'sha256':hashlib.sha256(data).hexdigest()}
        report['commands'].append(facts)
        print(json.dumps({'name':name,'exitCode':child.returncode,'durationSeconds':facts['durationSeconds']},ensure_ascii=False),flush=True)
        if child.returncode or facts.get('timeout'):break
    report['status']='PASS' if len(report['commands'])==len(commands) and all(x['exitCode']==0 and not x.get('timeout') for x in report['commands']) else 'FAIL'
finally:
    report['sourceAfter']=snapshot();report['changedInputs']=[(a['path'],b['path']) for a,b in zip(before,report['sourceAfter']) if a!=b]
    if report['sourceAfter']!=before:report['status']='FAIL_SOURCE_CHANGED'
    report['finishedUtc']=datetime.datetime.now(datetime.timezone.utc).isoformat()
    report['targetRetainedForParentIntegration']=str(TARGET);report['targetOwnerUnchanged']=(TARGET/'.owner').read_text()==OWNER
    with (EV/'core-host-fourth.json').open('x') as f:json.dump(report,f,ensure_ascii=False,indent=2);f.write('\n')
print(json.dumps({'status':report['status'],'commands':len(report['commands']),'inputs':len(before),'changedInputs':report['changedInputs'],'report':str(EV/'core-host-fourth.json')},ensure_ascii=False),flush=True)
sys.exit(0 if report['status']=='PASS' else 1)
