from pathlib import Path
import datetime, hashlib, json, os, subprocess, sys, time

r = Path('/tmp/fc-round68-production-https')
e = r / 'evidence'
w = r / 'worktree'
freeze_name, name, *cmd = sys.argv[1:]
assert '/' not in freeze_name and freeze_name.endswith('.json')
assert name and all(c.isalnum() or c == '-' for c in name) and cmd
freeze = e / freeze_name
f = json.loads(freeze.read_text())
sha = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
def same():
    return all((w / x['path']).is_file() and sha(w / x['path']) == x['sha256']
               for x in f['workingFiles'])
assert same()
out, stdout, stderr = (e / (name + suffix) for suffix in ('.json', '.stdout.log', '.stderr.log'))
assert not any(p.exists() for p in (out, stdout, stderr))
d = {'status': 'RUNNING', 'command': cmd, 'cwd': str(w), 'sourceFreeze': freeze_name,
     'sourceFreezeSha256': sha(freeze), 'workingFileCount': len(f['workingFiles']),
     'originalBudgetSeconds': 900, 'startedUtc': datetime.datetime.now(datetime.timezone.utc).isoformat()}
out.open('x').write(json.dumps(d, indent=2) + '\n')
env = os.environ.copy()
env['CARGO_TARGET_DIR'] = str(r / 'target')
started = time.monotonic()
with stdout.open('xb') as a, stderr.open('xb') as b:
    try:
        code = subprocess.run(cmd, cwd=w, env=env, stdout=a, stderr=b, timeout=900).returncode
    except subprocess.TimeoutExpired:
        code = None
d.update(status='PASS' if code == 0 else 'FAIL_TIMEOUT' if code is None else 'FAIL',
         exitCode=code, elapsedSeconds=time.monotonic() - started, sourceUnchanged=same(),
         stdoutBytes=stdout.stat().st_size, stdoutSha256=sha(stdout),
         stderrBytes=stderr.stat().st_size, stderrSha256=sha(stderr))
out.write_text(json.dumps(d, indent=2) + '\n')
print(json.dumps(d), flush=True)
sys.exit(0 if code == 0 and d['sourceUnchanged'] else 1)
