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
args, tail = parser.parse_known_args()
report = ROOT / (args.baseline + '-download-operation-first.json')
stdout = ROOT / (args.baseline + '-download-operation-first.stdout.log')
stderr = ROOT / (args.baseline + '-download-operation-first.stderr.log')
assert not any(p.exists() for p in (report, stdout, stderr))
command = ['python3', '-B', str(ROOT / 'download-original.py'), '--baseline', args.baseline, *tail]
value = {'status': 'RUNNING', 'command': command, 'originalBudgetSeconds': 1200,
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
             elapsedSeconds=time.monotonic() - started, actualControllerReturned=True)
for key, path, data in [('stdout', stdout, out), ('stderr', stderr, err)]:
    with path.open('xb') as stream:
        stream.write(data)
    value[key] = {'path': path.name, 'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()}
report.write_text(json.dumps(value, indent=2) + '\n')
print(json.dumps(value))
print(out.decode('utf-8', 'replace'))
print(err.decode('utf-8', 'replace'))
raise SystemExit(0 if code == 0 else 1)
