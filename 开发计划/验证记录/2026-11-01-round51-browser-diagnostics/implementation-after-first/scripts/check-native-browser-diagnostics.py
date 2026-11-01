#!/usr/bin/env python3
"""Pure R51 diagnostic models: no process, network, browser or Windows execution."""
import copy
import hashlib
import importlib.util
import json
from pathlib import Path
import sys
import time
from types import SimpleNamespace

sys.dont_write_bytecode = True
source = Path(__file__).resolve().with_name('windows-native-browser-observer.py')
spec = importlib.util.spec_from_file_location('fc_observer_diagnostics_pure', source)
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)
origin = 'https://localhost:12345'
path = '/fc/api/v1/content/homework/1/1/1/2026-11-01/visual'
manifest = {'schemaVersion': 1, 'apiVersion': 1, 'uiCompatibilityVersion': 1,
            'assets': [{'path': 'index.html', 'sizeBytes': 1, 'sha256': 'a' * 64}]}
run = {'origin': origin, 'generation': 1, 'maxMillis': 12000}
fact = {'requestOrdinal': 1, 'kind': 'visual', 'path': path, 'phase': 'visual-refresh-with-draft',
        'canceled': False, 'errorText': 'net::ERR_CONTENT_LENGTH_MISMATCH', 'blockedReason': 'NONE',
        'failureMillis': 90, 'requestMillis': 10, 'responseMillis': 80, 'extraInfoMillis': 85,
        'finishedMillis': None, 'bodyVerifiedMillis': None, 'responseReceived': True, 'finished': False,
        'bodyVerified': False, 'dataEvents': 2, 'dataBytes': 130, 'encodedBytes': 100, 'countsClamped': False}
record = {'method': 'GET', 'url': origin + path, 'status': 200, 'mime': 'application/json',
          'servedFromCache': False, 'fromDiskCache': False, 'fromServiceWorker': False,
          'fromPrefetchCache': False, 'wireStatus': 200, 'finished': False, 'bodyVerified': False, 'failure': fact}
value = {'schemaVersion': 1, 'status': 'FAIL', 'error': {'phase': fact['phase'], 'code': 'NETWORK_LOAD_FAILED'},
         'network': [record], 'wireResponses': [record]}
checks = []

def valid(candidate, kind='HOMEWORK'):
    return m.validate_network_failures(candidate, run, manifest, kind)

assert valid(value) == [fact]
checks.append('200/ExtraInfo200 partial-body first failure preserved without claiming body completion')
for error in ('net::ERR_ABORTED', 'net::ERR_CONNECTION_RESET', 'OTHER'):
    candidate = copy.deepcopy(value)
    candidate['network'][0]['failure'].update(errorText=error, canceled=True)
    assert valid(candidate)[0]['errorText'] == error
checks.append('aborted/reset/unknown remain failed diagnostics, never success')
candidate = copy.deepcopy(value)
candidate['network'][0]['failure'].update(countsClamped=True, dataEvents=65536, dataBytes=8 * 1024 * 1024,
                                         encodedBytes=8 * 1024 * 1024, failureMillis=None)
assert valid(candidate)[0]['countsClamped'] is True
checks.append('saturation explicitly labeled and missing clock remains unknown')
candidate = copy.deepcopy(value)
candidate['network'][0].update(finished=True, bodyVerified=True)
assert valid(candidate)[0]['finished'] is False
checks.append('fault-instant frozen facts survive a later already pending body-analysis completion')
assert valid({'status': 'PASS', 'network': [], 'wireResponses': []}) == []
checks.append('normal reports omit diagnostics')
mutations = [
    ('extra header field', lambda v: v['network'][0]['failure'].update(headers={'Cookie': 'PRIVATE'})),
    ('extra raw frame field', lambda v: v['network'][0]['failure'].update(rawCDP='PRIVATE')),
    ('extra request body field', lambda v: v['network'][0]['failure'].update(postData='PRIVATE')),
    ('unfiltered error', lambda v: v['network'][0]['failure'].update(errorText='net::ERR_ABORTED Cookie: PRIVATE')),
    ('linebreak error', lambda v: v['network'][0]['failure'].update(errorText='net::ERR_ABORTED\n')),
    ('overlong error', lambda v: v['network'][0]['failure'].update(errorText='net::ERR_' + 'X' * 81)),
    ('unknown blocked reason', lambda v: v['network'][0]['failure'].update(blockedReason='PRIVATE')),
    ('nonstring blocked reason', lambda v: v['network'][0]['failure'].update(blockedReason=[])),
    ('unknown phase', lambda v: v['network'][0]['failure'].update(phase='PRIVATE')),
    ('nonstring phase', lambda v: v['network'][0]['failure'].update(phase=[])),
    ('query in path', lambda v: v['network'][0]['failure'].update(path=path + '?Cookie=PRIVATE')),
    ('foreign approved-looking path', lambda v: v['network'][0].update(url='https://wrong.invalid' + path)),
    ('kind mismatch', lambda v: v['network'][0]['failure'].update(kind='asset')),
    ('unapproved API', lambda v: (v['network'][0]['failure'].update(path='/fc/api/v1/subject-visuals'),
                                v['network'][0].update(url=origin + '/fc/api/v1/subject-visuals'))),
    ('bool ordinal', lambda v: v['network'][0]['failure'].update(requestOrdinal=True)),
    ('wrong ordinal', lambda v: v['network'][0]['failure'].update(requestOrdinal=2)),
    ('bool canceled', lambda v: v['network'][0]['failure'].update(canceled=1)),
    ('bool counter', lambda v: v['network'][0]['failure'].update(dataBytes=True)),
    ('negative counter', lambda v: v['network'][0]['failure'].update(dataBytes=-1)),
    ('fraction counter', lambda v: v['network'][0]['failure'].update(dataEvents=1.5)),
    ('overflow events', lambda v: v['network'][0]['failure'].update(dataEvents=65537)),
    ('overflow bytes', lambda v: v['network'][0]['failure'].update(encodedBytes=8 * 1024 * 1024 + 1)),
    ('unlabeled clamp', lambda v: v['network'][0]['failure'].update(countsClamped=1)),
    ('bool time', lambda v: v['network'][0]['failure'].update(failureMillis=True)),
    ('NaN time', lambda v: v['network'][0]['failure'].update(failureMillis=float('nan'))),
    ('negative time', lambda v: v['network'][0]['failure'].update(requestMillis=-1)),
    ('out-of-bound time', lambda v: v['network'][0]['failure'].update(failureMillis=60001)),
    ('time after frozen failure', lambda v: v['network'][0]['failure'].update(responseMillis=91)),
    ('unobserved response claimed', lambda v: v['network'][0].update(status=None)),
    ('unobserved EOF claimed', lambda v: v['network'][0]['failure'].update(finished=True)),
    ('unverified body claimed', lambda v: v['network'][0]['failure'].update(bodyVerified=True)),
    ('null diagnostic', lambda v: v['network'][0].update(failure=None)),
    ('missing diagnostic field', lambda v: v['network'][0]['failure'].pop('canceled')),
    ('failure carried by PASS', lambda v: v.update(status='PASS')),
    ('different original failure', lambda v: v['error'].update(code='RUN_DEADLINE')),
    ('wrong record method', lambda v: v['network'][0].update(method='POST')),
]
for label, mutate in mutations:
    candidate = copy.deepcopy(value)
    mutate(candidate)
    try:
        valid(candidate)
    except RuntimeError as error:
        checks.append(label + ': rejected ' + str(error))
    else:
        raise AssertionError('accepted invalid diagnostic: ' + label)
# One first failure may appear in both the network ledger and exact wire subset.
candidate = copy.deepcopy(value)
second = copy.deepcopy(record)
second['failure']['requestOrdinal'] = 2
candidate['network'].append(second)
try:
    valid(candidate)
except RuntimeError as error:
    checks.append('multiple first failures rejected: ' + str(error))
else:
    raise AssertionError('accepted more than one first failure')
for records in ([{}] * 129, 'PRIVATE'):
    try:
        valid({**value, 'network': records})
    except RuntimeError:
        checks.append('network cardinality/type rejected')
    else:
        raise AssertionError('unbounded network accepted')
candidate = copy.deepcopy(value)
candidate['wireResponses'][0] = copy.deepcopy(record)
candidate['wireResponses'][0]['failure']['errorText'] = 'OTHER'
try:
    valid(candidate)
except RuntimeError:
    checks.append('wire subset cannot replace frozen diagnostic')
else:
    raise AssertionError('different wire snapshot accepted')
try:
    valid(value, 'REMINDER')
except RuntimeError:
    checks.append('reminder still rejects homework visual')
else:
    raise AssertionError('reminder accepted visual')

# Run the actual coordinator failure path with ONLY in-memory capabilities.
# This proves diagnostics are checked/preserved before the existing nonzero-exit
# rejection, without launching Node/Edge or touching a report/control file.
marker = 'FutureClassroom-Test-TLS-' + 'a' * 32
observer = m.BrowserObserver(None, '/tmp/unused', '/tmp/unused-report.json', marker, manifest,
                             "script-src 'self'; connect-src 'self'", None, 'unused')
observer.prepared = True
observer._node_source_sha = 'b' * 64
observer.node_report = Path('/tmp/unused-node-report.json')
observer.node = SimpleNamespace(returncode=1, stdin=SimpleNamespace(close=lambda: None), wait=lambda timeout: None)
observer.node_logs = None
node_value = {**value, 'marker': marker, 'origin': origin, 'generation': 1, 'maxMillis': 12000,
              'contentKind': 'HOMEWORK', 'sourceSha256': 'b' * 64}
observer._send = lambda payload, deadline: None
observer._line = lambda *args: json.dumps({'type': 'RESULT', 'status': 'FAIL'})
observer._node_raw = lambda: None
observer._file = lambda path: (node_value, {'path': str(path), 'bytes': 1, 'sha256': 'c' * 64})
client = SimpleNamespace(deadline=time.monotonic() + 30, ready={'origin': origin, 'generation': 1},
                         cookie='__Host-FutureClassroom=' + 'd' * 64, csrf='e' * 64)
try:
    observer.run(client)
except RuntimeError as error:
    assert str(error) == 'NATIVE_BROWSER_RUN_FAILED'
else:
    raise AssertionError('nonzero Node became success')
assert observer.report['status'] == 'FAIL' and observer.result == {}
assert observer.report['networkFailureDiagnostics'] == [fact]
assert observer.report['nodeActualExitCode'] == 1
checks.append('actual coordinator control flow retains validated diagnostics before original exit1 failure; no positive result')
print(json.dumps({'status': 'PURE_LOCAL_MODEL_CHECKS_PASS', 'scope': __doc__, 'checks': checks,
                  'observerSourceSha256': hashlib.sha256(source.read_bytes()).hexdigest(),
                  'mockSourceSha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                  'python': sys.version, 'windows': 'NOT_RUN', 'browser': 'NOT_RUN', 'tls': 'NOT_RUN'}, indent=2))
