from pathlib import Path
import hashlib, json, subprocess, time
root = Path(__file__).resolve().parent
assert (root / '.owner').read_bytes() == b'future-classroom-owned-r72-archive-controller/v1\n'
command = ['python3', '-B', '/tmp/fc-round72-ci-evidence/archive-preparation-first/archive-evidence-parameterized-first.py',
    '--source-sha', '00120a026eea5dc4ff330f8526bf31169b373565',
    '--acceptance-report', 'independent-joint-first.json',
    '--expected-acceptance-sha256', '882e33ccb1b7e65f07c45c003d53378f9c1d7d460460695c779723e57eff7d2f',
    '--expected-acceptance-status', 'PASS_R72_MODERN_WIN11_WITH_WIN7_CI_FAILURE_PRESERVED',
    '--readonly-audit', 'windows-readonly-final-second.json',
    '--expected-readonly-sha256', 'e43eace48db68cbbe813d190655a8e6d497226542705fb1c5f078ba16fac42dd',
    '--root-controllers-terminal', '--execute']
report = root / 'archive-operation-first.json'
out, err = root / 'archive-operation-first.stdout.log', root / 'archive-operation-first.stderr.log'
assert not any(p.exists() for p in (report, out, err))
v = dict(status='RUNNING', command=command, scope='Exact R72 originals archive only; no deletion, Git or runtime certification.')
with report.open('x') as s: json.dump(v, s, indent=2)
t = time.monotonic()
with out.open('xb') as stdout, err.open('xb') as stderr:
    child = subprocess.Popen(command, stdout=stdout, stderr=stderr)
    code = child.wait()
v.update(status='PASS_COMMAND' if code == 0 else 'FAIL_COMMAND', actualExit=True, exitCode=code, elapsedSeconds=time.monotonic() - t)
for key, p in [('stdout', out), ('stderr', err)]:
    data = p.read_bytes()
    v[key] = dict(path=str(p), bytes=len(data), sha256=hashlib.sha256(data).hexdigest())
report.write_text(json.dumps(v, indent=2) + '\n')
print(json.dumps(v))
raise SystemExit(0 if code == 0 else 1)
