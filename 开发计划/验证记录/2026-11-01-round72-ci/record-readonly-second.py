"""Capture actual final read-only audit exit and original streams."""
from pathlib import Path
import hashlib, json, subprocess, time
root = Path(__file__).resolve().parent
assert (root / '.owner').read_bytes() == b'future-classroom-owned-r72-ci-evidence/v1\n'
report = root / 'windows-readonly-operation-second.json'
stdout, stderr = root / 'windows-readonly-operation-second.stdout.log', root / 'windows-readonly-operation-second.stderr.log'
assert not any(p.exists() for p in (report, stdout, stderr))
command = ['python3', '-B', str(root / 'audit-windows-readonly-first.py'),
    '--report', str(root / 'windows-readonly-final-second.json'),
    '--baseline', '/workspace/共享/Eliora/未来教室计划/开发计划/验证记录/2026-11-01-round70-transfer-stop/windows-readonly-final-first.json']
v = dict(status='RUNNING', command=command, scope='Read-only final owned-residue, Run and persistent SSH audit; no mutation.')
with report.open('x') as s: json.dump(v, s, indent=2)
t = time.monotonic()
with stdout.open('xb') as out, stderr.open('xb') as err:
    c = subprocess.Popen(command, stdout=out, stderr=err)
    code = c.wait()
v.update(status='PASS_COMMAND' if code == 0 else 'FAIL_COMMAND', exitCode=code, actualExit=True, elapsedSeconds=time.monotonic() - t)
for key, path in [('stdout', stdout), ('stderr', stderr)]:
    data = path.read_bytes()
    v[key] = dict(path=str(path), bytes=len(data), sha256=hashlib.sha256(data).hexdigest())
report.write_text(json.dumps(v, indent=2) + '\n')
print(json.dumps(v))
raise SystemExit(0 if code == 0 else 1)
