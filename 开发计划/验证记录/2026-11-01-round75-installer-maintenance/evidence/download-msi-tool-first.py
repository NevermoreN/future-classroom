#!/usr/bin/env python3
"""One exact R75 tool package transfer; no install or global cache use."""
from pathlib import Path
import datetime
import hashlib
import json
import subprocess
import sys
import time

ROOT = Path('/tmp/fc-round75-installer-maintenance')
EVIDENCE = ROOT / 'evidence'
config = json.loads((EVIDENCE / 'msi-tools-input-first.json').read_text())
name = sys.argv[1]
items = [item for item in config['packages'] if item['name'] == name]
assert len(items) == 1
item = items[0]
assert (ROOT / 'tools/.owner').read_bytes() == b'future-classroom-owned-r75-msi-tools/v1\n'
assert item['transferBase'] == 'https://mirrors.edge.kernel.org/ubuntu/'
relative = item['officialRepositoryPath']
assert relative.startswith('pool/') and '..' not in relative.split('/')
destination = ROOT / 'tools/packages' / Path(relative).name
assert not destination.exists()
command = ['curl', '--disable', '--fail', '--location', '--max-redirs', '3',
           '--proto', '=https', '--proto-redir', '=https', '--tlsv1.2',
           '--connect-timeout', '15', '--max-time', '120',
           '--max-filesize', str(item['bytes']), '--output', str(destination),
           item['transferBase'] + relative]
started = time.monotonic()
report = {'schemaVersion': 1, 'status': 'FAIL', 'package': item['name'],
          'expectedBytes': item['bytes'], 'expectedSha256': item['sha256'],
          'command': command, 'startedUtc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
          'deadlineSeconds': 120, 'connectionDeadlineSeconds': 15,
          'candidateMsiProduced': False, 'windowsInvoked': False}
try:
    stdout = EVIDENCE / ('msi-tools-download-' + name + '-first.stdout.log')
    stderr = EVIDENCE / ('msi-tools-download-' + name + '-first.stderr.log')
    with stdout.open('xb') as out, stderr.open('xb') as err:
        process = subprocess.Popen(command, stdin=subprocess.DEVNULL, stdout=out, stderr=err)
        try:
            code = process.wait(timeout=125)
            report['controllerTimedOut'] = False
        except subprocess.TimeoutExpired:
            process.kill()
            code = process.wait(timeout=5)
            report['controllerTimedOut'] = True
    report.update(actualExit=True, parentReaped=True, pid=process.pid, exitCode=code, elapsedSeconds=time.monotonic()-started)
    report['logs'] = []
    for path in (stdout, stderr):
        data = path.read_bytes()
        assert len(data) <= 16 * 1024 * 1024
        report['logs'].append({'path': path.name, 'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()})
    if destination.is_file():
        data = destination.read_bytes()
        report['downloaded'] = {'path': str(destination), 'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()}
    assert code == 0 and not report['controllerTimedOut']
    assert report['downloaded']['bytes'] == item['bytes'] and report['downloaded']['sha256'] == item['sha256']
    report['status'] = 'PASS_EXACT_PUBLISHED_BYTES_AND_SHA256'
finally:
    with (EVIDENCE / ('msi-tools-download-' + name + '-first.json')).open('x') as output:
        json.dump(report, output, ensure_ascii=False, indent=2)
        output.write('\n')
print(json.dumps({'package': name, 'status': report['status'], 'exitCode': report.get('exitCode'), 'elapsedSeconds': report.get('elapsedSeconds')}))
