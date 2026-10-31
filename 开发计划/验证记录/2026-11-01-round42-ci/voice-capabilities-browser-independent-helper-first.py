from pathlib import Path
from urllib.parse import urlsplit
from collections import Counter
import ast, base64, datetime, hashlib, json, re

EV = Path('/tmp/fc-round42-ci-evidence')
ROOT = Path('/workspace/共享/Eliora/未来教室计划')
INPUT = EV / 'modern-msvc-voice-capabilities-browser-first.json'
OUT = EV / 'voice-capabilities-browser-independent-first.json'
SOURCE = 'bc2a86fb4413dcb3e8acf6b0d4bb2dfe601c22dc'
EXE = '6c82d3dfd5352849c4291bf06c2ed0228ceab9edbf368a06eb753dab2014d8ac'
sha = lambda b: hashlib.sha256(b).hexdigest()
assert not OUT.exists(), 'refuse existing report'
inputs = {}
report = {'schemaVersion': 1, 'status': 'RUNNING', 'sourceCommit': SOURCE, 'exeSha256': EXE,
          'scope': 'Independent read-only recomputation of completed R42 modern MSVC actual finite TLS/continuous/REMINDER browser evidence. Original runner, standalone Node/Edge-owner files and exact raw bytes are checked without executing tests or network. Final R39 feature plus false-positive correction/cleanup docs; NOT R41 deadline changes. No Cookie/CSRF/body/control capability copied.',
          'checks': [], 'inputs': []}

def check(name, condition):
    report['checks'].append({'name': name, 'status': 'PASS' if condition else 'FAIL'})
    if not condition:
        raise AssertionError(name)

def input_bytes(path):
    path = Path(path)
    check('input contained in exact evidence directory: ' + path.name,
          path.is_absolute() and path.is_relative_to(EV) and path.resolve().is_relative_to(EV))
    data = path.read_bytes()
    row = {'path': str(path), 'bytes': len(data), 'sha256': sha(data)}
    if str(path) in inputs:
        check('repeated input remains unchanged: ' + path.name, inputs[str(path)] == row)
    else:
        inputs[str(path)] = row
    return data

def json_input(path):
    return json.loads(input_bytes(path).decode('utf-8-sig'))

def encoded_stream(row, label):
    data = base64.b64decode(row['base64'], validate=True)
    check(label + ' exact complete retained bytes and SHA',
          row['bytes'] == len(data) and row['sha256'] == sha(data)
          and row.get('captureScope', 'COMPLETE') == 'COMPLETE'
          and row.get('observedBytes', len(data)) == len(data))
    return data

def raw_files(owner, label):
    preservation = owner['rawPreservation']
    check(label + ' original raw preservation PASS', preservation['status'] == 'PASS')
    result = {}
    for name, row in preservation['files'].items():
        data = input_bytes(row['artifact'])
        check(label + ' raw file accepted: ' + name, row['status'] == 'PASS')
        expected = row.get('downloaded', row)
        check(label + ' raw file byte hash: ' + name,
              expected['bytes'] == len(data) and expected['sha256'] == sha(data))
        if 'remote' in row:
            check(label + ' remote/download agreement: ' + name, row['remote'] == expected)
        if 'base64' in row:
            check(label + ' retained base64/file agreement: ' + name,
                  base64.b64decode(row['base64'], validate=True) == data)
        result[name] = data
    return result

try:
    r = json_input(INPUT)
    check('original finite runner terminal and cleanup PASS', r['status'] == r['cleanup'] == 'PASS')
    check('exact successful modern original executable SHA', r['exeSha256'] == EXE)
    check('finite REMINDER continuous mode and unchanged gate limits',
          r['browserContentKind'] == 'REMINDER' and r['continuousAcceptEnabled'] is True
          and r['strictP0'] == 'NOT_VALIDATED' and r['productionListener'] == 'NOT_ENABLED'
          and r['forwardTimingIsNativeDeadlineEvidence'] is False
          and r['physicalPlayback'] == 'NOT_EXERCISED')
    b = r['browser']
    node_path = Path(b['nodeReportPath'])
    n = json_input(node_path)
    node_meta = inputs[str(node_path)]
    check('standalone Node original byte hash binding',
          b['nodeReport']['bytes'] == node_meta['bytes'] and b['nodeReport']['sha256'] == node_meta['sha256'])
    check('standalone Node equals actual embedded result', b['result'] == n)
    check('Node actual same service and committed finite REMINDER context',
          n['status'] == 'PASS' and n['contentKind'] == 'REMINDER'
          and n['marker'] == r['marker'] and n['generation'] == r['ready'][0]['generation']
          and n['origin'] == r['ready'][0]['origin'] and n['maxMillis'] <= 12000
          and n['elapsedMillis'] <= n['maxMillis'])
    source_files = {
        'observer': ROOT / 'scripts/windows-native-browser-observer.py',
        'edgeOwner': ROOT / 'scripts/windows-test-edge-owner.py',
        'nodeHarness': ROOT / 'scripts/windows-native-browser-harness.mjs',
        'tlsOwnershipHelpers': ROOT / 'scripts/windows-tls-probe.py',
    }
    for key, path in source_files.items():
        check('actual observer source hash: ' + key, sha(path.read_bytes()) == b['sourceSha256'][key])
    runner = ROOT / 'scripts/windows-tls-business-probe.py'
    check('actual runner source hash', sha(runner.read_bytes()) == r['runnerSourceSha256'])
    check('Node report source bound to frozen actual harness', n['sourceSha256'] == b['sourceSha256']['nodeHarness'])
    csp = None
    for statement in ast.parse(runner.read_text()).body:
        if isinstance(statement, ast.Assign) and any(isinstance(t, ast.Name) and t.id == 'CSP' for t in statement.targets):
            csp = ast.literal_eval(statement.value)
    check('literal strict CSP provenance', isinstance(csp, str) and sha(csp.encode()) == n['expectedCspSha256'])
    web_manifest = json.loads((Path('/tmp/fc-round42-web-assets/dist') / 'fc-assets.json').read_text())
    canonical = json.dumps(web_manifest, separators=(',', ':'), ensure_ascii=False).encode()
    check('Node exact admitted asset manifest hash', sha(canonical) == n['assetManifestCanonicalSha256'])
    by_asset = {x['path']: x for x in web_manifest['assets']}
    for asset in n['assets']:
        expected = by_asset.get(asset['path'])
        check('actual visited browser asset exact identity: ' + asset['path'], expected is not None
              and asset['verified'] is True and asset['bytes'] == expected['sizeBytes']
              and asset['sha256'] == expected['sha256'])
    network = n['network']
    check('all browser observations finished with verified bodies', all(
        x['finished'] is True and x['bodyVerified'] is True for x in network))
    flags = ['servedFromCache', 'fromDiskCache', 'fromServiceWorker', 'fromPrefetchCache']
    wire = [x for x in network if x['wireStatus'] is not None
            and (x['wireStatus'] == 304 or not any(x[f] for f in flags))]
    check('wire responses independently exclude cached200 and retain actual304',
          wire == n['wireResponses'] and len(wire) == n['wireResponseCount'] == b['wireResponseCount'])
    check('cache-only observations remain separately accounted',
          len(network) == n['observedResponseCount'] == n['observedRequestCount']
          and n['cacheOnlyResponseCount'] == len(network) - len(wire))
    origin = n['origin']
    for row in network:
        parsed = urlsplit(row['url'])
        check('browser observation exact same HTTPS origin',
              parsed.scheme == 'https' and parsed.netloc == urlsplit(origin).netloc
              and not parsed.query and not parsed.fragment and not parsed.username and not parsed.password)
        if not parsed.path.startswith('/fc/api/'):
            check('browser static observation exact strict CSP', row.get('csp') == csp)
    api = [x for x in network if urlsplit(x['url']).path.startswith('/fc/api/')]
    content_paths = {urlsplit(x['url']).path for x in api if re.fullmatch(
        r'/fc/api/v1/content/reminders/1/[0-9]{4}-[0-9]{2}-[0-9]{2}', urlsplit(x['url']).path)}
    check('one exact actual class-day reminder route', len(content_paths) == 1)
    content_path = next(iter(content_paths))
    content_date = content_path.rsplit('/', 1)[1]
    datetime.date.fromisoformat(content_date)
    route_counts = Counter((x['method'], urlsplit(x['url']).path) for x in api)
    expected_routes = Counter({('GET', '/fc/api/v1/status'): 2,
                              ('GET', '/fc/api/v1/session'): 2,
                              ('GET', '/fc/api/v1/content/reminders/scopes/0'): 2,
                              ('GET', content_path): 2, ('POST', content_path): 1})
    check('derive exact nine actual API operations', route_counts == expected_routes and len(api) == n['apiResponses'] == 9)
    check('all API observations actual200 without any cache flags', all(
        x['status'] == x['wireStatus'] == 200 and not any(x[f] for f in flags) for x in api))
    check('exact one POST and no alternate browser write',
          sum(x['method'] == 'POST' for x in network) == n['contentPosts'] == 1
          and type(n['contentPosts']) is int)
    operations = {'statusGets': 2, 'sessionGets': 2, 'scopeGets': 2,
                  'contentGets': 2, 'contentPosts': 1, 'visualGets': 0}
    check('declared exact operations match independently derived ledger', n['apiOperations'] == operations)
    check('REMINDER has no visual route/read/observation',
          not any('/visual' in urlsplit(x['url']).path or '/homework' in urlsplit(x['url']).path for x in network)
          and n['visualReads'] == 0 and n['visualObservations'] == [])
    check('two instrumented actual bottom shortcut clicks', type(n['reminderShortcutClicks']) is int and n['reminderShortcutClicks'] == 2)
    dom = n['reminderObservations']
    check('three exact DOM save/reload stages and revision sequence',
          [x['stage'] for x in dom] == ['initial', 'after-save', 'after-reload']
          and [x['revision'] for x in dom] == ['0', '1', '1'])
    bools = ['titleMatches', 'dateMatches', 'expiryMatches', 'draftMatches', 'noVisualPreview', 'bottomShortcutAvailable']
    expected_key = {'kind': 'REMINDER', 'classId': '1', 'date': content_date}
    check('actual reminder DOM key/date/expiry/draft and no preview observations', all(
        x['key'] == expected_key and set(x) == {'stage', 'key', 'revision', *bools}
        and all(x[k] is True for k in bools) for x in dom))
    check('actual committed receipt CSRF matches with no browser page errors',
          n['writeOutcome'] == 'COMMITTED' and n['contentRevision'] == '1'
          and n['sessionCsrfMatched'] is True and n['pageErrorCount'] == n['consoleErrorCount'] == 0)
    stdout = encoded_stream(b['nodeRaw']['stdout'], 'Node stdout')
    encoded_stream(b['nodeRaw']['stderr'], 'Node stderr')
    messages = [json.loads(x) for x in stdout.decode().splitlines()]
    check('actual Node READY then RESULT with matching report',
          [x['type'] for x in messages] == ['READY', 'RESULT']
          and all(x['marker'] == r['marker'] for x in messages)
          and messages[1]['status'] == 'PASS' and messages[1]['report'] == str(node_path)
          and messages[1]['wireResponseCount'] == len(wire) and messages[1]['contentPosts'] == 1
          and messages[1]['contextDisposed'] is True)
    check('original Node actualexit0 and context disposal/CDP disconnect acknowledged',
          b['nodeActualExitCode'] == 0 and n['contextDisposed'] is True and n['cdpDisconnected'] is True)
    native_raw = raw_files(r, 'native')
    lines = native_raw['tls.stdout'].decode().splitlines()
    passes = [json.loads(x.split(' ', 1)[1]) for x in lines if x.startswith('TLS_BUSINESS_PASS ')]
    progress = [json.loads(x.split(' ', 1)[1]) for x in lines if x.startswith('TLS_BUSINESS_PROGRESS ')]
    check('one raw native PASS receipt identical to all runner copies',
          len(passes) == 1 and passes[0] == r['native'] and r['nativeReports'] == passes)
    check('raw native progress sequence exactly matches retained parsed observations', progress == r['nativeProgress'])
    final = passes[0]
    check('original two shared workers actually joined and hubs/quarantine drained',
          final['physicalNativeDrain'] is True and final['sharedWorkers'] == 2
          and final['workerJoin'] is True and final['setupCleanup'] is True
          and final['hubPending'] == final['downloadHubPending'] == final['quarantined'] == 0)
    last = progress[-1]
    check('raw final native sockets IOCP jobs and request owners returned',
          last['acceptance']['native_pending'] == last['acceptance']['owned_sockets'] == 0
          and last['jsonBodyRecords'] == last['pendingJobs'] == 0 and last['requests'] == [])
    debug = [x for x in lines if x.startswith('TLS_BUSINESS_REPORT ')]
    check('raw final native debug reports real drain and no retained owners', len(debug) == 1 and all(
        re.search(pattern, debug[0]) is not None for pattern in [
            r'active_records: 0,', r'pending_jobs: 0,', r'cleanup_pending: 0,',
            r'retained_cleanup: 0,', r'native_drained: true,', r'stopping: true,']))
    http = [x for x in r['events'] if type(x.get('httpStatus')) is int]
    non_http = [x for x in r['events'] if type(x.get('httpStatus')) is not int]
    check('all original client observations successful and digest bound',
          all(x['status'] == 'PASS' for x in r['events'])
          and sha(json.dumps(r['events'], sort_keys=True, separators=(',', ':')).encode()) == r['clientEventsSha256'])
    check('all actual raw HTTP records use TLS12 exact fixture and clean close_notify', all(
        x['tlsVersion'] == 'TLSv1.2' and x['serverHostname'] == 'localhost'
        and x['peerDerSha256'] == http[0]['peerDerSha256']
        and x['closeNotify'] == 'PASS: actual clean TLS EOF' for x in http))
    check('four nonHTTP observations are actual capacity EOF and certificate rejections',
          {x['case'] for x in non_http} == {'continuous-capacity-reject-1', 'continuous-capacity-reject-2',
                                                      'wrong-ca-rejected', 'wrong-hostname-rejected'}
          and all(x['transportRejection'] in ['EOF', 'ConnectionResetError', 'SSLEOFError']
                  for x in non_http if x['case'].startswith('continuous-capacity'))
          and all(x['rejectionType'] == 'SSLCertVerificationError'
                  for x in non_http if x['case'].startswith('wrong-')))
    check('dynamic exact native response conservation rawHTTP plus browser wire',
          final['heads'] == final['responses'] == len(http) + len(wire))
    capabilities = [x for x in http if x.get('route') == 'voice/capabilities']
    observation = r['voiceCapabilitiesObservation']
    check('three actual exact capabilities GETs ordered401200200',
          len(capabilities) == 3 and [x['httpStatus'] for x in capabilities] == [401, 200, 200]
          and [x['case'] for x in capabilities] == ['voice-capabilities-no-cookie',
              'voice-capabilities-authenticated', 'voice-capabilities-authenticated-repeat']
          and all(x['method'] == 'GET' and x['requestBodyBytes'] == 0 for x in capabilities))
    check('two actual authenticated capabilities responses exact body digest stable',
          capabilities[1]['responseBodyBytes'] == capabilities[2]['responseBodyBytes']
          and capabilities[1]['responseBodySha256'] == capabilities[2]['responseBodySha256'])
    check('capability observation remains UNVERIFIED and query/cache not inferred',
          observation['status'] == 'PASS' and observation['additionalWireRequests'] == len(capabilities)
          and observation['unauthenticatedStatus'] == 401 and observation['authenticatedStatuses'] == [200, 200]
          and observation['deviceAvailability'] == 'UNVERIFIED'
          and observation['nativeQueryCount'] == 'NOT_OBSERVED'
          and observation['cacheBehavior'] == 'NOT_DETERMINED_BY_GET'
          and observation['stableFieldsExcludingServerNow'] == 'MATCH')
    # Bind the recorded successful runner to its exact twenty-field validator;
    # full response bodies are intentionally not retained, so do not pretend to
    # independently reparse the original actual payload from this evidence.
    expected_keys = None
    for statement in ast.parse(runner.read_text()).body:
        if isinstance(statement, ast.FunctionDef) and statement.name == 'validate_voice_capabilities':
            for node in statement.body:
                if isinstance(node, ast.Assign) and any(isinstance(t, ast.Name) and t.id == 'keys' for t in node.targets):
                    expected_keys = ast.literal_eval(node.value)
    check('actual unchanged runner exact20field validator source bound', isinstance(expected_keys, set) and len(expected_keys) == 20)
    continuous = r['business']['continuousAcceptance']
    check('continuous report retains exact four unique original request intervals',
          len(continuous['originalIntervals']) == 4
          and len({(x['generation'], x['slot'], x['serial']) for x in continuous['originalIntervals']}) == 4
          and all(x['deadlineNs'] - x['admittedAtNs'] == 120_000_000_000 for x in continuous['originalIntervals'])
          and continuous['capacityCloses'] == 2 and continuous['sameOriginalReceipts'] == 4
          and continuous['binaryUploadPermitsUsedByStage'] == 0)
    check('continuous idle observation has actual application return/repost after5.05s',
          r['continuousIdle']['noClientSeconds'] >= 5.05 and r['continuousIdle']['idleReturns'] >= 1
          and r['continuousIdle']['repostedSequence'] > r['continuousIdle']['baselineSequence'])
    check('native actual original process exit0 and tunnel cleanup',
          r['nativeProcessExit'] == {'code': 0} and r['nativeProcessExits'] == [{'code': 0}]
          and r['ownedTunnelCleanup'] == 'PASS' and r['ownedProcessCleanup']['status'] == 'PASS'
          and r['ownedProcessCleanup']['ownedProcessesRemaining'] == 0)
    owner_path = Path(b['ownerReportPath'])
    owner = json_input(owner_path)
    owner_meta = inputs[str(owner_path)]
    check('standalone original Edge owner report exact byte hash',
          b['ownerReport']['bytes'] == owner_meta['bytes'] and b['ownerReport']['sha256'] == owner_meta['sha256'])
    check('original Edge owner and observer supervisor actualexit0/cleanup PASS',
          owner['status'] == owner['cleanup'] == b['status'] == b['cleanup'] == b['ownerPhysicalCleanup'] == 'PASS'
          and b['ownerActualExitCode'] == 0 and b['errors'] == [] and b['ownerProcessRetained'] is False)
    edge_raw = raw_files(owner, 'Edge owner')
    outcome = json.loads(edge_raw['.edge-outcome.json'].decode('utf-8-sig'))
    process = json.loads(edge_raw['.edge-process.json'].decode('utf-8-sig'))
    assigned = json.loads(edge_raw['.edge-assigned.json'].decode('utf-8-sig'))
    check('original Edge physical outcome equals raw retained original', owner['nativeOutcome'] == outcome)
    check('Edge suspended assigned-before-resume and exact owner identity',
          process['createdSuspended'] is True and assigned['assignedBeforeResume'] is True
          and assigned['breakawayAllowed'] is False and assigned['killOnJobClose'] is True
          and process['pid'] == assigned['pid'] and process['startTicks'] == assigned['startTicks']
          and process['marker'] == assigned['marker'] == owner['marker'])
    check('Edge actual primary exited all job processes zero handles closed',
          outcome['originalPrimaryExited'] is True and outcome['actualJobProcessesZero'] is True
          and outcome['originalHandlesClosed'] is True and outcome['jobBoundBeforeResume'] is True
          and outcome['failure'] is None and outcome['marker'] == owner['marker'])
    check('Edge owned profile/root deleted with other browsers and OS settings untouched',
          owner['cleanupDetails']['status'] == 'PASS' and owner['cleanupDetails']['rootAbsent'] is True
          and owner['cleanupDetails']['actualJobProcessesZero'] is True
          and owner['cleanupDetails']['existingBrowsers'] == 'UNTOUCHED'
          and owner['cleanupDetails']['sshConfiguration'] == 'UNTOUCHED'
          and owner['cleanupDetails']['rootTrustStore'] == 'UNTOUCHED')
    for kind in ['stdout', 'stderr']:
        encoded_stream(b['ownerSupervisorRaw'][kind], 'original Edge supervisor ' + kind)
    check('owned native fixture directory CNG certificate and settings cleanup',
          r['cleanupDetails']['status'] == 'PASS' and r['cleanupDetails']['rootAbsent'] is True
          and r['cleanupDetails']['cngKeyExists'] is False and r['cleanupDetails']['myDigestRestored'] is True
          and r['cleanupDetails']['ownedProcessesRemaining'] == 0
          and all(r['cleanupDetails'][x] == 'UNTOUCHED' for x in ['sshConfiguration', 'rootTrustStore', 'firewall', 'httpSys']))
    check('all original independent inline remote cleanup commands exited0', all(
        x['exitCode'] == 0 for x in r['remoteControlRunsRaw'] + owner['remoteControlRunsRaw']))
    check('all retained original input bytes unchanged through review', all(
        Path(x['path']).stat().st_size == x['bytes'] and sha(Path(x['path']).read_bytes()) == x['sha256']
        for x in inputs.values()))
    report.update(status='PASS', contentKind='REMINDER',
        counts={'clientEvents': len(r['events']), 'clientHttpResponses': len(http),
                'clientNonHttpObservations': len(non_http), 'capabilityGets': len(capabilities),
                'capabilityStatuses': [x['httpStatus'] for x in capabilities],
                'browserObservedResponses': len(network), 'browserWireResponses': len(wire),
                'browserCacheOnlyResponses': len(network) - len(wire),
                'apiResponses': len(api), 'apiOperations': operations, 'posts': n['contentPosts'],
                'visualRequests': 0, 'visualReads': n['visualReads'],
                'shortcutClicks': n['reminderShortcutClicks'], 'domStages': len(dom),
                'domRevisions': [x['revision'] for x in dom],
                'nativeHeads': final['heads'], 'nativeResponses': final['responses'],
                'sharedWorkers': final['sharedWorkers'], 'hubPending': final['hubPending'],
                'downloadHubPending': final['downloadHubPending'], 'quarantined': final['quarantined']},
        nativeFinalReceipt=final, lastNativeProgress=last,
        capabilityObservation=observation, capabilityGetEvidence=[{k:x[k] for k in [
            'case', 'method', 'route', 'httpStatus', 'requestBodyBytes', 'responseBodyBytes',
            'responseBodySha256', 'responseWireBytes', 'responseWireSha256', 'closeNotify']} for x in capabilities],
        capabilityValidator={'disclosedFieldCount': len(expected_keys), 'fieldNames': sorted(expected_keys),
            'scope': 'Exact actually successful runner source validator; original response bodies intentionally omitted, not independently reparsed from absent bodies.'},
        edgePhysicalOutcome={k:outcome[k] for k in ['originalPrimaryExited', 'originalPrimaryExitCode',
            'actualJobProcessesZero', 'originalHandlesClosed', 'jobBoundBeforeResume', 'reason', 'failure']},
        inputs=list(inputs.values()), helper={'path': str(Path(__file__)), 'bytes': Path(__file__).stat().st_size,
                                             'sha256': sha(Path(__file__).read_bytes())},
        limitations=['No tests/network/Windows process were executed by this independent checker.',
            'UNVERIFIED is the actual observed capability value; three GETs do not reveal native-query count, cache hits/misses or audibility. No microphone, playback or volume action inferred.',
            'Reminder DOM/shortcut observations are frozen actual harness observations cross-checked with exact real network routes; no retained raw CDP-frame claim.',
            'Edge primary exit code is recorded faithfully (1 after capability-stop); original owner supervisor actualexit0 and job0/handles/profile cleanup establish completed ownership.',
            'R42 exact6c82 source is final R39 feature plus false-positive fix/cleanup docs, not R41 new frontdoor deadline code.',
            'Original strict firstTCP5s gate remains FAIL/not validated, production listener remains disabled, Win7/Win10/visibleSeewo/mobile/campusTLS/physical sound remain unvalidated.',
            'Full Rust runtime suite and final overall machine cleanup audit are separate evidence; no extrapolation from this finite browser check.'])
except BaseException as error:
    report.update(status='FAIL', errorKind=type(error).__name__,
                  failedCheck=str(error) if isinstance(error, AssertionError) else 'Original evidence invariant or shape failed',
                  inputs=list(inputs.values()))
with OUT.open('x') as f:
    f.write(json.dumps(report, ensure_ascii=False, indent=2) + '\n')
print(json.dumps({'status': report['status'], 'checks': len(report['checks']), 'counts': report.get('counts'),
                  'report': {'path': str(OUT), 'bytes': OUT.stat().st_size, 'sha256': sha(OUT.read_bytes())}}, ensure_ascii=False))
raise SystemExit(0 if report['status'] == 'PASS' else 1)
