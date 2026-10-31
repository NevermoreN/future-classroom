#!/usr/bin/env python3
"""Pure synthetic report validation only: no browser, sockets, subprocess or remote."""
import copy
import hashlib
import importlib.util
import json
from pathlib import Path
import sys
sys.dont_write_bytecode = True
source = Path('/tmp/fc-round35-visuals-worktree/scripts/windows-native-browser-observer.py')
spec = importlib.util.spec_from_file_location('fc_observer_visual_pure', source)
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)
marker = 'FutureClassroom-Test-TLS-' + 'a' * 32
origin = 'https://localhost:12345'
owner = object.__new__(m.BrowserObserver)
owner.marker, owner._node_source_sha = marker, 'b' * 64
owner.manifest = {'schemaVersion': 1, 'apiVersion': 1, 'uiCompatibilityVersion': 1, 'assets': []}
owner.expected_csp, owner.node_report = "script-src 'self'; connect-src 'self'", Path('/tmp/synthetic-node-report.json')
run = {'generation': 1, 'origin': origin, 'maxMillis': 12000}
base = '/fc/api/v1/content/homework/1/1/1/2026-11-01'
paths = [('GET', '/fc/api/v1/status')] * 2 + [('GET', '/fc/api/v1/session')] * 2 + [('GET', '/fc/api/v1/content/homework/scopes/0')] * 2
paths += [('GET', base)] * 2 + [('POST', base)] + [('GET', base + '/visual')] * 4
records = [{'method': method, 'url': origin + path, 'status': 200, 'mime': 'application/json', 'servedFromCache': False,
            'fromDiskCache': False, 'fromServiceWorker': False, 'fromPrefetchCache': False, 'wireStatus': 200,
            'finished': True, 'bodyVerified': True} for method, path in paths]
visual = {'subjectRevision': '2', 'displayRevision': '4', 'rulesVersion': 1, 'effectiveRgb': '#C98282', 'mode': 'FILLED',
          'homeworkFillPercent': 0, 'courseFillPercent': 20, 'homeworkComponentOpacityPercent': 50, 'courseComponentOpacityPercent': 100,
          'homeworkVisible': True, 'courseVisible': False}
def pill(kind):
    return {'mode': 'FILLED', 'fillPercent': str(visual[kind + 'FillPercent']), 'effectiveRgb': '#C98282',
            'visible': str(visual[kind + 'Visible']).lower(), 'componentOpacity': visual[kind + 'ComponentOpacityPercent'] / 100,
            'pillOpacity': 1, 'effectiveOpacity': visual[kind + 'ComponentOpacityPercent'] / 100,
            'backgroundColor': 'rgba(201, 130, 130, 0)' if kind == 'homework' else 'rgba(201, 130, 130, 0.2)',
            'borderWidth': 0, 'borderStyle': 'none', 'textAlign': 'center', 'textMatches': True, 'textColorUnchanged': True}
dom = {'subjectRevision': '2', 'displayRevision': '4', 'rulesVersion': '1', 'homework': pill('homework'), 'course': pill('course'),
       'refreshOpacity': 1, 'helpOpacity': 1, 'draftMatches': True, 'bottomReminderAvailable': True}
observations = [{'stage': stage, 'readNumber': index + 1, 'responseBodySha256': 'c' * 64,
                 'visual': copy.deepcopy(visual), 'dom': copy.deepcopy(dom), 'draftPreserved': True}
                for index, stage in enumerate(('initial', 'draft-refresh', 'after-save', 'after-reload'))]
value = {'schemaVersion': 1, 'status': 'PASS', 'marker': marker, 'generation': 1, 'origin': origin, 'maxMillis': 12000,
         'sourceSha256': 'b' * 64, 'assetManifestCanonicalSha256': m.digest(m.encoded(owner.manifest)),
         'expectedCspSha256': m.digest(owner.expected_csp.encode()), 'contextDisposed': True, 'cdpDisconnected': True,
         'sessionCsrfMatched': True, 'writeOutcome': 'COMMITTED', 'contentPosts': 1, 'network': records,
         'wireResponseCount': len(records), 'wireResponses': records, 'observedRequestCount': len(records),
         'observedResponseCount': len(records), 'apiResponses': 13, 'cacheOnlyResponseCount': 0, 'apiOperations': dict(m.API_OPERATIONS),
         'pageErrorCount': 0, 'consoleErrorCount': 0, 'assets': [], 'visualReads': 4, 'visualObservations': observations}
message = {'type': 'RESULT', 'status': 'PASS', 'marker': marker, 'wireResponseCount': len(records),
           'contentPosts': 1, 'contextDisposed': True, 'report': str(owner.node_report)}
checks = []
owner._validate_result(value, message, run)
checks.append('synthetic exact 13-operation ledger and Filled0/20 CSS accepted; not IO/browser evidence')
outlined = copy.deepcopy(value)
for observation in outlined['visualObservations']:
    observation['visual']['mode'] = 'OUTLINED'
    for kind in ('homework', 'course'):
        observation['dom'][kind].update(mode='OUTLINED', backgroundColor='rgba(0, 0, 0, 0)', borderWidth=1, borderStyle='solid')
owner._validate_result(outlined, message, run)
checks.append('synthetic Outlined transparent background and independent whole opacity accepted')
mutations = [
    ('missing visual', lambda v: v.update(visualReads=3)),
    ('forged ninth-only total', lambda v: v.update(apiResponses=9)),
    ('bool operation count', lambda v: v['apiOperations'].update(contentPosts=True)),
    ('cache disguised as wire', lambda v: v['network'][-1].update(fromDiskCache=True)),
    ('different visual scope', lambda v: v['network'][-1].update(url=origin+base.replace('/1/1/1/', '/1/1/2/')+'/visual')),
    ('zero fill acquired border', lambda v: v['visualObservations'][1]['dom']['homework'].update(borderWidth=1, borderStyle='solid')),
    ('text faded with fill', lambda v: v['visualObservations'][1]['dom']['course'].update(pillOpacity=0.2)),
    ('controls faded', lambda v: v['visualObservations'][1]['dom'].update(refreshOpacity=0.5)),
    ('draft changed', lambda v: v['visualObservations'][1]['dom'].update(draftMatches=False)),
    ('stage reordered', lambda v: v['visualObservations'][1].update(stage='after-save')),
]
for label, mutate in mutations:
    invalid = copy.deepcopy(value)
    mutate(invalid)
    try:
        owner._validate_result(invalid, message, run)
    except RuntimeError as error:
        checks.append(label + ': rejected ' + str(error))
    else:
        raise AssertionError('accepted invalid synthetic model: ' + label)
print(json.dumps({'status': 'PURE_LOCAL_MODEL_CHECKS_PASS', 'scope': __doc__, 'checks': checks,
                  'observerSourceSha256': hashlib.sha256(source.read_bytes()).hexdigest(),
                  'mockSourceSha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                  'python': sys.version, 'windows': 'NOT_RUN', 'browser': 'NOT_RUN', 'tls': 'NOT_RUN'}, indent=2))
