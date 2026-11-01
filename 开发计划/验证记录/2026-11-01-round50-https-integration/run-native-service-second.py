from pathlib import Path
import datetime,hashlib,json,os,signal,subprocess,time

root=Path('/tmp/fc-round47-https-settings-worktree')
ev=Path('/tmp/fc-round50-https-integration-evidence')
host=Path('/tmp/fc-round47-https-settings-target')
cross=Path('/tmp/fc-round50-https-windows-target')
assert (host/'.owner').read_text()=='future-classroom-r47-core-host-target/v1\n'
assert (cross/'.owner').read_text()=='future-classroom-owned-r50-https-windows-target/v1\n'
out=ev/'native-service-second.json';assert not out.exists()
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def snapshot():
    names=set(subprocess.check_output(['git','ls-files','--cached','--others','--exclude-standard','-z'],cwd=root).decode().split('\0'))
    return {n:{'bytes':(root/n).stat().st_size,'sha256':sha(root/n)} for n in sorted(names) if n and ((n.startswith('crates/') and (n.endswith('.rs') or n.endswith('/Cargo.toml'))) or n in ('Cargo.toml','Cargo.lock','rust-toolchain.toml','.cargo/config.toml'))}
r={'status':'RUNNING','sourceBefore':snapshot(),'gitHeadBefore':subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip(),'commands':[],'jobs':2,'testThreads':2,'perCommandTimeoutSeconds':900,'scope':'Pre-UI diagnostic checks for integrated final R47 core plus frozen R48 native/service. No R49 UI, no normal program or Windows runtime claim; final full matrix remains required.'}
def save():out.write_text(json.dumps(r,indent=2)+'\n')
save()
env={**os.environ,'CARGO_BUILD_JOBS':'2','CARGO_TERM_COLOR':'never'}
env.pop('RUSTFLAGS',None);env.pop('CARGO_ENCODED_RUSTFLAGS',None);env.pop('CARGO_TARGET_DIR',None)
common=['--release','--locked','--offline']
commands=[
 ('fmt',['cargo','fmt','--all','--','--check']),
 ('architecture',['python3','scripts/check-architecture.py']),
 ('host-native-tests',['cargo','test','-p','fc-windows','--lib',*common,'--target-dir',str(host),'certificate_query_','--','--test-threads=2']),
 ('host-service-tests',['cargo','test','-p','fc-bootstrap','--lib',*common,'--target-dir',str(host),'https_certificate_','--','--test-threads=2']),
 ('host-strict',['cargo','clippy','-p','fc-windows','-p','fc-bootstrap','--all-targets',*common,'--target-dir',str(host),'--','-D','warnings']),
 ('windows-native-compile',['cargo','test','-p','fc-windows','--lib',*common,'--target','i686-pc-windows-gnu','--target-dir',str(cross/'stable'),'--no-run','--message-format=json']),
 ('windows-strict',['cargo','clippy','-p','fc-windows','-p','fc-bootstrap','--all-targets',*common,'--target','i686-pc-windows-gnu','--target-dir',str(cross/'stable'),'--','-D','warnings']),
]
for name,args in commands:
    a=ev/('native-service-second.'+name+'.stdout.log');b=ev/('native-service-second.'+name+'.stderr.log')
    print('NATIVE_SERVICE_STAGE_STARTED '+name,flush=True);start=time.monotonic()
    with a.open('xb') as stdout,b.open('xb') as stderr:
        child=subprocess.Popen(args,cwd=root,env=env,stdout=stdout,stderr=stderr,start_new_session=True)
        timed=False
        try:child.wait(timeout=900)
        except subprocess.TimeoutExpired:
            timed=True;os.killpg(child.pid,signal.SIGTERM)
            try:child.wait(timeout=5)
            except subprocess.TimeoutExpired:os.killpg(child.pid,signal.SIGKILL);child.wait()
    row={'name':name,'args':args,'exitCode':child.returncode,'pid':child.pid,'ownedProcessGroup':child.pid,'actualExit':True,'timedOut':timed,'elapsedSeconds':time.monotonic()-start}
    for label,p in [('stdout',a),('stderr',b)]:row[label]={'path':str(p),'bytes':p.stat().st_size,'sha256':sha(p)}
    r['commands'].append(row);r['sourceAfter']=snapshot();r['gitHeadAfter']=subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip();r['changedInputs']=[n for n in r['sourceBefore'].keys()|r['sourceAfter'].keys() if r['sourceBefore'].get(n)!=r['sourceAfter'].get(n)]
    save();print('NATIVE_SERVICE_STAGE_DONE '+name+' exit='+str(child.returncode),flush=True)
    if child.returncode or timed or r['changedInputs'] or r['gitHeadAfter']!=r['gitHeadBefore']:
        r['status']='FAIL';save();raise SystemExit(1)
r['status']='PASS_DIAGNOSTIC_ONLY_FINAL_UI_MATRIX_REQUIRED';save();print(r['status'],flush=True)
