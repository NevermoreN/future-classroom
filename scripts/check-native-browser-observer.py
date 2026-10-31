#!/usr/bin/env python3
"""Pure synthetic report validation only: no browser, sockets, subprocess or remote."""
import copy
import hashlib
import importlib.util
import json
from pathlib import Path
import sys
sys.dont_write_bytecode = True
source = Path(__file__).resolve().with_name('windows-native-browser-observer.py')
spec = importlib.util.spec_from_file_location('fc_observer_visual_pure', source)
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)
marker = 'FutureClassroom-Test-TLS-' + 'a' * 32
origin = 'https://localhost:12345'
owner = object.__new__(m.BrowserObserver)
owner.marker, owner._node_source_sha, owner.content_kind = marker, 'b' * 64, 'HOMEWORK'
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
value = {'contentKind': 'HOMEWORK', 'reminderShortcutClicks': 0, 'reminderObservations': [], 'schemaVersion': 1, 'status': 'PASS', 'marker': marker, 'generation': 1, 'origin': origin, 'maxMillis': 12000,
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

# The reminder journey is independently selected before reading a report. It
# cannot opt itself into the cheaper ledger or reuse a homework visual result.
reminder = copy.deepcopy(value)
reminder.update(contentKind='REMINDER', reminderShortcutClicks=2, visualReads=0, visualObservations=[],
                apiResponses=9, apiOperations=dict(m.REMINDER_OPERATIONS))
reminder_records = reminder['network'][:9]
for record in reminder_records:
    record['url'] = record['url'].replace('/content/homework/scopes/', '/content/reminders/scopes/').replace(
        '/content/homework/1/1/1/', '/content/reminders/1/')
reminder.update(network=reminder_records, wireResponses=reminder_records, wireResponseCount=9,
                observedRequestCount=9, observedResponseCount=9)
reminder_message = {**message, 'wireResponseCount': 9}
reminder['reminderObservations'] = [dict(stage=stage, key={'kind':'REMINDER','classId':'1','date':'2026-11-01'}, revision=('0','1','1')[i], titleMatches=True, dateMatches=True, expiryMatches=True, draftMatches=True, noVisualPreview=True, bottomShortcutAvailable=True) for i, stage in enumerate(('initial','after-save','after-reload'))]
owner.content_kind = 'REMINDER'
owner._validate_result(reminder, reminder_message, run)
checks.append('independent exact reminder 9-operation ledger with two bottom-shortcut clicks and zero visuals accepted')
reminder_mutations = [
    ('missing reminder DOM', lambda v: v.update(reminderObservations=[])),
    ('wrong reminder DOM key', lambda v: v['reminderObservations'][1]['key'].update(classId='2')),
    ('reminder DOM adds owner', lambda v: v['reminderObservations'][1]['key'].update(ownerTeacherId='1')),
    ('wrong reminder date', lambda v: v['reminderObservations'][1]['key'].update(date='2026-02-30')),
    ('wrong saved revision', lambda v: v['reminderObservations'][1].update(revision='0')),
    ('reminder visual preview shown', lambda v: v['reminderObservations'][1].update(noVisualPreview=False)),
    ('expiry text mismatch', lambda v: v['reminderObservations'][1].update(expiryMatches=False)),
    ('draft text mismatch', lambda v: v['reminderObservations'][1].update(draftMatches=False)),
    ('reported homework kind', lambda v: v.update(contentKind='HOMEWORK')),
    ('shortcut bool', lambda v: v.update(reminderShortcutClicks=True)),
    ('only one shortcut click', lambda v: v.update(reminderShortcutClicks=1)),
    ('reported visual read', lambda v: v.update(visualReads=1)),
    ('hidden visual observations', lambda v: v.update(visualObservations=observations[:1])),
    ('homework route on reminder', lambda v: v['network'][-1].update(url=origin + base)),
    ('unrequested reminder visual route', lambda v: v['network'][-1].update(url=origin + '/fc/api/v1/content/reminders/1/2026-11-01/visual')),
    ('foreign origin', lambda v: v['network'][-1].update(url='https://wrong.invalid/fc/api/v1/content/reminders/1/2026-11-01')),
    ('second slot', lambda v: v['network'][-1].update(url=origin + '/fc/api/v1/content/reminders/2/2026-11-01')),
    ('second write', lambda v: v['network'][0].update(method='POST')),
    ('unfinished body', lambda v: v['network'][-1].update(finished=False)),
    ('unverified body', lambda v: v['network'][-1].update(bodyVerified=False)),
    ('boolean content-post count', lambda v: v.update(contentPosts=True)),
    ('cache disguised as API wire response', lambda v: v['network'][-1].update(fromDiskCache=True)),
]
for label, mutate in reminder_mutations:
    invalid = copy.deepcopy(reminder); mutate(invalid)
    try:
        owner._validate_result(invalid, reminder_message, run)
    except RuntimeError as error:
        checks.append(label + ': rejected ' + str(error))
    else:
        raise AssertionError('accepted invalid synthetic reminder report: ' + label)
# Same isolated request type cannot silently accept another journey's report.
owner.content_kind = 'HOMEWORK'
try:
    owner._validate_result(reminder, reminder_message, run)
except RuntimeError:
    checks.append('externally selected homework journey rejects reminder report')
else:
    raise AssertionError('reminder report changed observer-selected journey')
print(json.dumps({'status': 'PURE_LOCAL_MODEL_CHECKS_PASS', 'scope': __doc__, 'checks': checks,
                  'observerSourceSha256': hashlib.sha256(source.read_bytes()).hexdigest(),
                  'mockSourceSha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                  'python': sys.version, 'windows': 'NOT_RUN', 'browser': 'NOT_RUN', 'tls': 'NOT_RUN'}, indent=2))
