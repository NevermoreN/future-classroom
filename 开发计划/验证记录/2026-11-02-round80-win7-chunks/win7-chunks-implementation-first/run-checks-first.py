from pathlib import Path
import hashlib,json,os,subprocess,time
R=Path('/tmp/fc-round80-controlled-repair');W=R/'checkout-win7-chunks-first';D=Path(__file__).resolve().parent;T=R/'win7-chunks-check-target-first';assert not T.exists();T.mkdir();(T/'.owner').write_text('future-classroom-owned-win7-chunks-check/v1\n')
def inputs():
    return [dict(path=p.relative_to(W).as_posix(),bytes=p.stat().st_size,sha256=hashlib.sha256(p.read_bytes()).hexdigest()) for p in sorted(W.rglob('*')) if p.is_file() and (p.suffix=='.rs' or p.name in ('Cargo.toml','Cargo.lock','.cargo/config.toml'))]
report=dict(status='RUNNING',before=inputs(),commands=[]);env=os.environ.copy();env.update(CARGO_TARGET_DIR=str(T),CARGO_BUILD_JOBS='2',CARGO_TERM_COLOR='never');env.pop('FC_OWNED_TEST_SCOPE',None)
commands=[('gnu-strict-clippy',['cargo','clippy','--release','--locked','--offline','--workspace','--all-targets','--target','i686-pc-windows-gnu','--','-D','warnings']),('fmt',['cargo','fmt','--all','--','--check']),('architecture',['python3','scripts/check-architecture.py']),('diff',['git','diff','--check'])]
for label,argv in commands:
    start=time.monotonic()
    with (D/(label+'.stdout.log')).open('xb') as so,(D/(label+'.stderr.log')).open('xb') as se:
        child=subprocess.Popen(argv,cwd=W,env=env,stdout=so,stderr=se);code=child.wait(timeout=900)
    report['commands'].append(dict(label=label,argv=argv,actualExit=code,originalReaped=child.poll() is not None,seconds=time.monotonic()-start));(D/'checks-first.json').write_text(json.dumps(report,indent=2)+'\n');print(label,code,flush=True)
    if code:break
report['after']=inputs();report['sourceUnchanged']=report['before']==report['after'];report['status']='PASS_TWO_SOURCE_CHUNKS_STATIC_CHECKS' if len(report['commands'])==4 and all(c['actualExit']==0 for c in report['commands']) and report['sourceUnchanged'] else 'FAIL_RETAIN_ORIGINALS'
(D/'checks-first.json').write_text(json.dumps(report,indent=2)+'\n');print(report['status'],flush=True)
