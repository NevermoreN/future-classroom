"""Preserve original download controller terminal output outside the ZIP."""
from pathlib import Path
import argparse
import datetime
import hashlib
import json
import subprocess
import time

ROOT = Path(__file__).resolve().parent
parser = argparse.ArgumentParser()
parser.add_argument('--baseline', choices=['modern', 'win7'], required=True)
parser.add_argument('--source-sha', required=True)
args, tail = parser.parse_known_args()
assert (ROOT/'.owner').read_bytes()==b'future-classroom-owned-r74-ci-evidence/v1\n'
assert len(args.source_sha)==40 and all(c in '0123456789abcdef' for c in args.source_sha)
report = ROOT / (args.baseline + '-download-operation-first.json')
stdout = ROOT / (args.baseline + '-download-operation-first.stdout.log')
stderr = ROOT / (args.baseline + '-download-operation-first.stderr.log')
assert not any(p.exists() for p in (report, stdout, stderr))
command = ['python3', '-B', str(ROOT / 'download-original.py'), '--baseline', args.baseline, '--source-sha', args.source_sha, *tail]
value = {'status': 'RUNNING', 'sourceSha': args.source_sha, 'command': command, 'originalBudgetSeconds': 1200,
         'startedUtc': datetime.datetime.now(datetime.timezone.utc).isoformat()}
with report.open('x') as stream:
    json.dump(value, stream, indent=2)
    stream.write('\n')
started = time.monotonic()
try:
    completed = subprocess.run(command, capture_output=True, timeout=1200)
    code, out, err = completed.returncode, completed.stdout, completed.stderr
except subprocess.TimeoutExpired as error:
    code, out, err = None, error.stdout or b'', error.stderr or b''
value.update(status='PASS' if code == 0 else 'FAIL', exitCode=code,
             elapsedSeconds=time.monotonic() - started, actualControllerReturned=True, actualExit=code is not None)
for key, path, data in [('stdout', stdout, out), ('stderr', stderr, err)]:
    with path.open('xb') as stream:
        stream.write(data)
    value[key] = {'path': str(path), 'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()}
report.write_text(json.dumps(value, indent=2) + '\n')
print(json.dumps(value))
print(out.decode('utf-8', 'replace'))
print(err.decode('utf-8', 'replace'))
raise SystemExit(0 if code == 0 else 1)
