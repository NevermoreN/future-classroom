"""Record terminal execution of the source-bound static gate, never run an EXE."""
from pathlib import Path
import argparse, datetime, hashlib, json, subprocess, time

root = Path(__file__).resolve().parent
p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--baseline', choices=['modern', 'win7'], required=True)
p.add_argument('--source-sha', required=True)
p.add_argument('--job-metadata', type=Path, required=True)
a = p.parse_args()
assert (root / '.owner').read_bytes() == b'future-classroom-owned-r72-ci-evidence/v1\n'
assert len(a.source_sha) == 40 and all(c in '0123456789abcdef' for c in a.source_sha)
assert a.job_metadata.resolve().parent == root and a.job_metadata.is_file()
stem = a.baseline + '-static-operation-first'
report = root / (stem + '.json')
stdout, stderr = root / (stem + '.stdout.log'), root / (stem + '.stderr.log')
assert not any(path.exists() for path in (report, stdout, stderr))
command = ['python3', '-B', str(root / 'prepare-ci-inputs-first.py'),
    '--baseline', a.baseline, '--source-sha', a.source_sha,
    '--run-metadata', str(root / (a.baseline + '-run-finalfirst.json')),
    '--job-metadata', str(a.job_metadata),
    '--artifact-metadata', str(root / (a.baseline + '-artifacts-first.json')),
    '--execute-static']
result = dict(status='RUNNING', sourceSha=a.source_sha, baseline=a.baseline,
    command=command, startedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),
    scope='Local static verification only; no product/test execution or network.')
with report.open('x') as stream:
    json.dump(result, stream, indent=2)
started = time.monotonic()
with stdout.open('xb') as out, stderr.open('xb') as err:
    child = subprocess.Popen(command, stdout=out, stderr=err)
    code = child.wait()
result.update(status='PASS_COMMAND' if code == 0 else 'FAIL_COMMAND',
    exitCode=code, actualExit=True, elapsedSeconds=time.monotonic() - started)
for key, path in [('stdout', stdout), ('stderr', stderr)]:
    data = path.read_bytes()
    result[key] = dict(path=path.name, bytes=len(data), sha256=hashlib.sha256(data).hexdigest())
report.write_text(json.dumps(result, indent=2) + '\n')
print(json.dumps(result))
raise SystemExit(0 if code == 0 else 1)
