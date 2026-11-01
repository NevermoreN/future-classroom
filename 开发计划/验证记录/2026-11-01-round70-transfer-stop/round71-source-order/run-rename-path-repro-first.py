"""Root-only explicit execution; preparation by reviewer does not run Windows."""
import argparse
import base64
import datetime
import hashlib
import json
import pathlib
import signal
import subprocess
import time

parser = argparse.ArgumentParser()
parser.add_argument('--execute', action='store_true')
parser.add_argument('--host', default='future-classroom-win11')
parser.add_argument('--budget-seconds', type=int, default=90)
args = parser.parse_args()
if not args.execute:
    parser.error('root must explicitly pass --execute')
if not 15 <= args.budget_seconds <= 120:
    parser.error('budget must be15..120 seconds')
evidence = pathlib.Path(__file__).resolve().parent
source = evidence / 'rename-path-repro-first.ps1'
payload = source.read_bytes()
receipt = evidence / 'rename-path-repro-operation-first.json'
out = evidence / 'rename-path-repro-first.stdout.log'
err = evidence / 'rename-path-repro-first.stderr.log'
lock = evidence / 'rename-path-repro-execution-first.lock'
with lock.open('x') as handle:
    handle.write('single explicit root execution; never auto-retry\n')

def now():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()

wrapper = ("$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';"
           "[Console]::InputEncoding=[Text.UTF8Encoding]::new($false);"
           "[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false);"
           "$s=[Console]::In.ReadToEnd();&([scriptblock]::Create($s))")
encoded = base64.b64encode(wrapper.encode('utf-16-le')).decode('ascii')
remote = ('"%SystemRoot%\\SysWOW64\\WindowsPowerShell\\v1.0\\powershell.exe" '
          '-NoProfile -NonInteractive -EncodedCommand ' + encoded)
command = ['ssh', '-o', 'BatchMode=yes', '-o', 'ConnectTimeout=8', args.host, remote]
record = {'status': 'RUNNING', 'startedUtc': now(), 'command': command,
          'budgetSeconds': args.budget_seconds, 'script': {'path': source.name,
          'bytes': len(payload), 'sha256': hashlib.sha256(payload).hexdigest()},
          'scope': 'One own synchronous native fixture; no registry policy/network change. '
                   'Local SSH timeout does not prove original Windows process exited.'}

def save():
    receipt.write_text(json.dumps(record, indent=2) + '\n')

save()
started = time.monotonic()
with out.open('xb') as stdout, err.open('xb') as stderr:
    child = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=stdout,
                             stderr=stderr, start_new_session=True)
    record['controllerPid'] = child.pid
    save()
    try:
        child.communicate(input=payload, timeout=args.budget_seconds)
        code = child.returncode
        timed = False
    except subprocess.TimeoutExpired:
        timed = True
        child.send_signal(signal.SIGTERM)
        try:
            child.wait(timeout=5)
        except subprocess.TimeoutExpired:
            child.kill()
            child.wait()
        code = child.returncode
record.update(exitCode=code, timedOut=timed, endedUtc=now(),
              elapsedSeconds=time.monotonic()-started,
              status='TIMEOUT_REMOTE_OUTCOME_UNKNOWN' if timed else 'COMMAND_RETURNED')
for key, path in [('stdout', out), ('stderr', err)]:
    data = path.read_bytes()
    record[key] = {'path': path.name, 'bytes': len(data),
                   'sha256': hashlib.sha256(data).hexdigest()}
record['scriptUnchanged'] = source.read_bytes() == payload
if not timed and code == 0:
    try:
        result = json.loads(out.read_text(encoding='utf-8-sig').strip())
        record['result'] = result
        record['status'] = 'OBSERVED' if result.get('cleanup') == 'PASS' else 'FAIL_OR_RETAINED'
    except (ValueError, UnicodeError):
        record['status'] = 'FAIL_RESULT_NOT_JSON'
save()
print(json.dumps({'status': record['status'], 'exitCode': code,
                  'elapsedSeconds': record['elapsedSeconds'], 'receipt': str(receipt)}))
