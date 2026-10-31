from pathlib import Path
import hashlib, json, os, subprocess, time

root = Path('/tmp/fc-round41-frontdoor-deadline-worktree')
ev = Path('/tmp/fc-round41-frontdoor-deadline-evidence')
out = ev / 'host-targeted-first.json'
assert not out.exists()
baseline = json.loads((ev / 'windows-integrated-third.json').read_text())
def snapshot():
    paths = list((root/'crates').rglob('*.rs')) + list((root/'crates').rglob('Cargo.toml')) + list((root/'.cargo').rglob('*')) + [root/n for n in ('Cargo.toml','Cargo.lock','rust-toolchain.toml')]
    return {p.relative_to(root).as_posix(): {'bytes': p.stat().st_size, 'sha256': hashlib.sha256(p.read_bytes()).hexdigest()} for p in sorted(set(paths)) if p.is_file()}
r = {'scope': 'Portable fc-windows affected module; tests and strict Clippy. Windows-only code separately compiled and executed in the third GNU matrix. No Win7/Win10 runtime claim.', 'status': 'RUNNING', 'sourceBefore': snapshot(), 'commands': [], 'jobs': 2, 'testThreads': 2}
assert r['sourceBefore'] == baseline['sourceAfter']
r['gitHeadBefore'] = subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip()
def save(): out.write_text(json.dumps(r,indent=2)+'\n')
save()
env = {**os.environ, 'CARGO_BUILD_JOBS':'2', 'CARGO_TERM_COLOR':'never'}
for label, args in (
    ('tests', ['cargo','test','-p','fc-windows','--all-targets','--release','--locked','--target-dir','/tmp/fc-round41-build-target/host','--','--test-threads=2']),
    ('clippy', ['cargo','clippy','-p','fc-windows','--all-targets','--release','--locked','--target-dir','/tmp/fc-round41-build-target/host','--','-D','warnings']),
):
    stdout = ev / f'host-targeted-first.{label}.stdout.log'
    stderr = ev / f'host-targeted-first.{label}.stderr.log'
    print('HOST_STAGE_STARTED '+label,flush=True)
    start = time.monotonic()
    try:
        with stdout.open('xb') as a, stderr.open('xb') as b:
            result = subprocess.run(args,cwd=root,env=env,stdout=a,stderr=b,timeout=900)
        code = result.returncode
    except subprocess.TimeoutExpired:
        code = 124
        r['timedOutStage'] = label
    r['commands'].append({'name':label,'command':args,'exitCode':code,'elapsedSeconds':time.monotonic()-start,'stdout':str(stdout),'stderr':str(stderr)})
    r['sourceAfter'] = snapshot()
    r['gitHeadAfter'] = subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip()
    r['changedInputs'] = [p for p in r['sourceBefore'].keys()|r['sourceAfter'].keys() if r['sourceBefore'].get(p)!=r['sourceAfter'].get(p)]
    save()
    print('HOST_STAGE_DONE '+label+' exit='+str(code),flush=True)
    if code or r['changedInputs'] or r['gitHeadBefore'] != r['gitHeadAfter']:
        r['status']='FAIL';save();raise SystemExit(1)
r['status']='PASS';save();print('HOST_TARGETED_PASS',flush=True)
