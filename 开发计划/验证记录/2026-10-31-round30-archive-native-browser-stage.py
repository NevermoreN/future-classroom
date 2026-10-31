"""Explicit-allowlist archiver for the terminal native-browser stage.

No copy/index IO occurs without --execute. Execution is additionally blocked
until the terminal report schemas and final audit are actually reviewed here.
Never enumerate the whole evidence directory. Live *.control.json is excluded
before reading; historical .stop-*.json requires explicit expired approval.
"""
from pathlib import Path, PurePosixPath
import argparse
import base64
from email.utils import format_datetime, parsedate_to_datetime
import hashlib
import json
import re
import shutil

SRC = Path('/tmp/fc-round30-evidence')
DST = Path.cwd() / '开发计划/验证记录'
PREFIX = '2026-10-31-round30-'
SUMMARY = 'native-browser-stage-summary.json'
MAPPING = 'native-browser-stage-path-mapping.json'
PRIOR_COUNT = 319
PRIOR_SHA = 'fe007974a7ea5dd2c4fce783a2de3a3251530a93e3d2ff7f6a9f8970f8e694b3'
TERMINAL_SCHEMAS_REVIEWED = True
FAMILIES = ('native-browser-', 'core-native-browser-', 'core-msvc-exactsha-', 'service-observer-')
HELPERS = {'archive-native-browser-stage.py', 'prepare-native-browser-inputs.py',
           'audit-native-browser-windows.py', 'audit-native-browser-windows-first.py',
           'run-native-browser-tls-first.py', 'run-native-browser-tls-second.py',
           'run-native-browser-windows-workspace-compile.py', 'run-native-browser-windows-runtime-first.py',
           'freeze-native-browser-windows-workspace.py'}
SUFFIXES = {'.json', '.jsonl', '.log', '.py', '.mjs', '.pem', '.raw', '.stdout', '.stderr', '.txt'}


def digest(data):
    return hashlib.sha256(data).hexdigest()


def read_json(path):
    return json.loads(path.read_text())


def verify_index(index):
    assert len({row['path'] for row in index['files']}) == len(index['files'])
    for row in index['files']:
        data = (DST / row['path']).read_bytes()
        assert len(data) == row['bytes'] and digest(data) == row['sha256'], row['path']


def source_path(name):
    relative = PurePosixPath(name)
    assert not relative.is_absolute() and '..' not in relative.parts and '\\' not in name
    assert relative.as_posix() == name and relative.parts
    path = SRC / name
    for parent in [path, *path.parents]:
        if parent == SRC:
            break
        assert not parent.is_symlink(), str(parent)
    return path


def approved_family(name):
    return name.startswith(FAMILIES) or name in HELPERS


def forbidden_capability(path):
    # Never open a live control capability, even if somebody lists it explicitly.
    return path.name.lower().endswith('.control.json') or '.active' in path.name.lower()


def is_stop(path):
    return re.search(r'(?:^|\.)stop-[0-9a-f]{32}\.json$', path.name) is not None


def checked_data(path, expired_stops):
    assert path.is_file() and not path.is_symlink()
    assert not forbidden_capability(path), str(path)
    relative = str(path.relative_to(SRC))
    assert not is_stop(path) or relative in expired_stops, 'stop capability not explicitly expired: ' + relative
    assert path.suffix in SUFFIXES, str(path)
    data = path.read_bytes()
    assert not re.search(rb'(?m)^-----BEGIN (?:RSA |EC |OPENSSH |ENCRYPTED )?PRIVATE KEY-----', data), str(path)
    return data


def collect_explicit(manifest):
    """Enumerate only named files and named raw directories; no prefix glob."""
    files = manifest['files']
    directories = manifest['rawDirectories']
    expired = set(manifest.get('expiredStopCapabilities', []))
    assert len(files) == len(set(files)) and len(directories) == len(set(directories))
    selected = set()
    excluded = []
    for name in files:
        assert '/' not in name and approved_family(name), name
        path = source_path(name)
        if forbidden_capability(path):
            excluded.append({'path': str(path), 'reason': 'live control/active capability excluded before reading'})
            continue
        assert path.is_file()
        selected.add(path)
    for name in directories:
        assert '/' not in name and name.startswith(FAMILIES) and name.endswith('-raw'), name
        directory = source_path(name)
        assert directory.is_dir()
        for path in directory.rglob('*'):
            assert not path.is_symlink(), str(path)
            if not path.is_file():
                continue
            if forbidden_capability(path):
                excluded.append({'path': str(path), 'reason': 'live control/active capability excluded before reading'})
                continue
            if is_stop(path) and str(path.relative_to(SRC)) not in expired:
                excluded.append({'path': str(path), 'reason': 'stop capability lacks explicit terminal/expired approval; not read'})
                continue
            selected.add(path)
    for name in expired:
        path = source_path(name)
        assert is_stop(path) and path in selected, name
    return selected, excluded, expired


def verify_raw_blob(value):
    data = base64.b64decode(value['base64'], validate=True)
    assert len(data) == value['bytes'] and digest(data) == value['sha256']
    return data


def verify_raw_report(report, selected):
    raw = source_path(Path(report['rawPreservation']['directory']).name)
    assert str(raw) == report['rawPreservation']['directory']
    assert report['rawPreservation']['status'] == 'PASS'
    files = sorted(path for path in raw.rglob('*') if path.is_file())
    assert set(files).issubset(selected)
    for name, row in report['remoteRawEvidence'].items():
        data = (raw / name).read_bytes()
        assert len(data) == row['bytes'] and digest(data) == row['sha256'], name
    for name, row in report['rawPreservation']['files'].items():
        assert row['status'] == 'PASS'
        data = (raw / name).read_bytes()
        assert base64.b64decode(row['base64'], validate=True) == data, name
        for key in ('remote', 'downloaded'):
            if key in row:
                assert len(data) == row[key]['bytes'] and digest(data) == row[key]['sha256'], name
    return {'directory': str(raw), 'files': len(files), 'hiddenFiles': sum(path.name.startswith('.') for path in files),
            'remoteInventoryFiles': len(report['remoteRawEvidence']), 'originalBytesBase64RemoteShaVerified': True}


def load_selected(name, selected):
    path = source_path(name)
    assert path in selected and not forbidden_capability(path), name
    return read_json(path)


def validate_completed_browser_reports(manifest, selected):
    """Only already-terminal browser reports; no Windows runtime/livecap read."""
    raw_sets = []
    observations = []
    owner_reports = {}
    for stage in ('first', 'second'):
        stem = 'native-browser-tls-' + stage
        report = load_selected(stem + '.json', selected)
        node = load_selected(stem + '.json.browser-node.json', selected)
        owner_name = stem + '.json.browser-edge-owner.json'
        owner = load_selected(owner_name, selected)
        owner_reports[owner_name] = owner
        invocation = load_selected(stem + '-invocation.json', selected)
        assert report['cleanup'] == report['browser']['cleanup'] == owner['status'] == owner['cleanup'] == 'PASS'
        assert invocation['changedInputs'] == [] and invocation['sourceBefore'] == invocation['sourceAfter']
        assert report['exeSha256'] == invocation['exeSha256Before'] == invocation['exeSha256After']
        assert report['webBundleSha256'] == invocation['bundleSha256After']
        assert report['runnerSourceSha256'] == invocation['sourceAfter']['scripts/windows-tls-business-probe.py']['sha256']
        assert report['runnerSourceSha256'] == digest(source_path('native-browser-first-tested-windows-tls-business-probe.py').read_bytes())
        assert owner['sourceSha256'] == digest(source_path('native-browser-first-tested-windows-test-edge-owner.py').read_bytes())
        assert invocation['sourceAfter']['scripts/windows-native-browser-observer.py']['sha256'] == digest(source_path('native-browser-first-tested-windows-native-browser-observer.py').read_bytes())
        assert report['browser']['ownerActualExitCode'] == 0
        assert report['browser']['ownerPhysicalCleanup'] == 'PASS' and report['browser']['ownerProcessRetained'] is False
        outcome = owner['nativeOutcome']
        assert outcome['originalPrimaryExited'] is outcome['originalHandlesClosed'] is outcome['actualJobProcessesZero'] is True
        assert outcome['jobBoundBeforeResume'] is True and outcome['failure'] is None
        assert outcome['reason'] == 'capability-stop'
        assert owner['cleanupDetails']['rootAbsent'] is True and owner['cleanupDetails']['status'] == 'PASS'
        assert owner['existingBrowsers'] == owner['rootTrustStore'] == owner['sshConfiguration'] == 'UNTOUCHED'
        assert node['contextDisposed'] is node['cdpDisconnected'] is node['sessionCsrfMatched'] is True
        assert node['maxMillis'] == 12000 and node['observedRequestCount'] == node['observedResponseCount'] == 17
        assert node['wireResponseCount'] == len(node['wireResponses']) == 14 and node['cacheOnlyResponseCount'] == 3
        assert node['apiResponses'] == 9 and node['contentPosts'] == 1 and node['writeOutcome'] == 'COMMITTED'
        assert node['contentRevision'] == '1' and node['pageErrorCount'] == node['consoleErrorCount'] == 0
        assert len(node['network']) == 17 and all(item['finished'] and item['bodyVerified'] for item in node['network'])
        cached = [item for item in node['network'] if item['wireStatus'] is None]
        assert len(cached) == 3
        assert all(item['servedFromCache'] or item['fromDiskCache'] or item['fromServiceWorker'] or item['fromPrefetchCache'] for item in cached)
        api = [item for item in node['network'] if '/fc/api/v1/' in item['url']]
        assert len(api) == 9 and all(item['wireStatus'] == item['status'] == 200 for item in api)
        assert not any(item['servedFromCache'] or item['fromDiskCache'] or item['fromServiceWorker'] or item['fromPrefetchCache'] for item in api)
        assert [item['method'] for item in api].count('POST') == 1
        assert node['sourceSha256'] == invocation['sourceAfter']['scripts/windows-native-browser-harness.mjs']['sha256']
        harness_name = 'native-browser-' + stage + '-tested-windows-native-browser-harness.mjs'
        assert digest(source_path(harness_name).read_bytes()) == node['sourceSha256']
        for key, suffix in [('ownerReport', '.json.browser-edge-owner.json'), ('nodeReport', '.json.browser-node.json')]:
            binding = report['browser'][key]
            data = source_path(stem + suffix).read_bytes()
            assert len(data) == binding['bytes'] and digest(data) == binding['sha256']
        raw_sets.extend([verify_raw_report(report, selected), verify_raw_report(owner, selected)])
        if stage == 'first':
            assert report['status'] == node['status'] == 'FAIL'
            assert report['nativeReports'] == report['nativeProcessExits'] == []
            assert node['error']['code'] and report['error']['message'] == 'NATIVE_BROWSER_RUN_FAILED'
            assert invocation['exitCode'] != 0
        else:
            assert report['status'] == report['browser']['status'] == node['status'] == 'PASS'
            assert report['browserAcceptance'] == 'FINITE_NATIVE_CONTENT_UI_PASS'
            assert 0 <= node['elapsedMillis'] <= node['maxMillis'] and node['elapsedMillis'] == 778
            assert report['browser']['nodeActualExitCode'] == invocation['exitCode'] == 0
            http = [event for event in report['events'] if 'httpStatus' in event]
            assert len(http) == 45 and len(report['events']) == 47
            assert all(event['status'] == 'PASS' for event in report['events'])
            for event in http:
                date = event['responseDate']
                assert format_datetime(parsedate_to_datetime(date), usegmt=True) == date
                assert event['closeNotify'] == 'PASS: actual clean TLS EOF'
            native = report['native']
            assert native['heads'] == native['responses'] == len(http) + node['wireResponseCount'] == 59
            assert native['sharedWorkers'] == 2 and native['physicalNativeDrain'] is native['workerJoin'] is native['setupCleanup'] is True
            assert native['hubPending'] == native['downloadHubPending'] == native['quarantined'] == 0
            assert report['nativeProcessExit']['code'] == 0
        observations.append({'attempt': stage, 'report': stem + '.json', 'overallStatus': report['status'], 'cleanup': report['cleanup'],
                             'nodeStatus': node['status'], 'nodeError': node.get('error'), 'elapsedMillis': node.get('elapsedMillis'),
                             'maximumMillis': node['maxMillis'], 'requests': 17, 'wireResponses': 14, 'cacheOnlyResponses': 3,
                             'apiResponses': 9, 'contentPosts': 1, 'contentRevision': node['contentRevision'], 'writeOutcome': node['writeOutcome'],
                             'ownerProcessActualExitCode': report['browser']['ownerActualExitCode'],
                             'originalEdgePrimaryExitCode': outcome['originalPrimaryExitCode'], 'physicalOwnerCleanup': owner['cleanup'],
                             'browserVersion': node['browserVersion'], 'nodeChecks': node['checks'],
                             'rawHttpResponses': sum('httpStatus' in event for event in report['events']),
                             'rawCertificateNegativeEvents': sum('httpStatus' not in event for event in report['events']),
                             'nativeDebugLines': report.get('nativeDebugLines', []),
                             'nativeDrain': report.get('native', 'NOT_VALIDATED: native report absent on original failed attempt'),
                             'nodeSourceSha256': node['sourceSha256']})
    # Only after the exact original owner proves terminal cleanup can an expired
    # stop DTO be opened/archived. Live *.control.json is never selected/read.
    for relative in manifest.get('expiredStopCapabilities', []):
        path = source_path(relative)
        assert is_stop(path) and path in selected
        owner_name = re.sub(r'\.control\.json\.stop-[0-9a-f]{32}\.json$', '', path.name)
        assert owner_name in owner_reports, relative
        owner = owner_reports[owner_name]
        assert owner['status'] == owner['cleanup'] == 'PASS' and owner['cleanupDetails']['rootAbsent'] is True
    assert len(raw_sets) == 4 and all(row['files'] == 8 for row in raw_sets)
    return {'attempts': observations, 'rawSets': raw_sets,
            'scope': 'Finite Win11 Edge UI and raw TLS candidate only. First overall failure remains FAIL even though its content write/reload had completed. Cache-only observations are excluded from native wire totals.'}


def validate_local_models(selected):
    before = load_selected('core-native-browser-cache-settle-before.json', selected)
    after = load_selected('core-native-browser-cache-settle-after.json', selected)
    assert before['expectedFailure'] is True and before['exitCode'] == 1 and before['remote'] == 'NOT_RUN'
    assert after['status'] == 'LOCAL_REGRESSION_PASSED_WINDOWS_RETRY_NOT_RUN'
    assert after['firstWindowsFailureUnmodified'] is True and after['remote'] == after['windowsRetry'] == 'NOT_RUN'
    assert digest(source_path('core-native-browser-cache-settle-before.json').read_bytes()) == after['beforeLocalReportSha256']
    first_node = source_path('native-browser-tls-first.json.browser-node.json').read_bytes()
    assert digest(first_node) == before['preservedFirstWindowsFailureSha256'] == after['preservedFirstWindowsFailureSha256']
    assert len(base64.b64decode(after['sourceBase64'], validate=True)) == after['sourceBytes']
    assert digest(base64.b64decode(after['sourceBase64'], validate=True)) == after['sourceSha256']
    assert all(check['exitCode'] == 0 for check in after['checks'])
    source_bindings = {}
    for phase in ('before', 'after'):
        report = load_selected('core-msvc-exactsha-' + phase + '-source.json', selected)
        source_bindings[phase] = report['files']
        for name, row in report['files'].items():
            assert len(base64.b64decode(row['base64'], validate=True)) == row['bytes']
            assert digest(base64.b64decode(row['base64'], validate=True)) == row['sha256'], name
    static = load_selected('core-msvc-exactsha-static-final.json', selected)
    assert static['ciDispatch'] == static['toolchainInstall'] == static['cargo'] == 'NOT_RUN'
    assert static['status'] == 'LOCAL_STATIC_AND_CONTRACT_MODEL_PASS_POWERSHELL_AST_PENDING'
    assert all(row['status'] == 'PASS' for row in static['negativeAndPositiveCases'])
    ast = load_selected('native-browser-msvc-exactsha-powershell-ast.json', selected)
    assert ast['exitCode'] == 0 and ast['parsed']['status'] == 'PASS' and ast['parsed']['errors'] == []
    assert ast['sourceSha256Before'] == ast['sourceSha256After'] == source_bindings['after']['scripts/windows-msvc-ci.ps1']['sha256']
    assert digest(base64.b64decode(ast['stdinUtf8Base64'], validate=True)) == ast['sourceSha256Before']
    for stream in ('stdout', 'stderr'):
        verify_raw_blob(ast[stream])
    for name in ('.github/workflows/windows-msvc.yml', 'scripts/windows-msvc-ci.ps1'):
        assert static['afterSourceSha256'][name] == source_bindings['after'][name]['sha256']
    service_name = 'service-observer-local-checks-d9790467996d402f95573ee1d393fca3.json'
    service = load_selected(service_name, selected)
    assert digest(source_path(service_name).read_bytes()) == '0236126cb097c9b42adda4ce1df05f817bea5990ea1e75224150f067ee07420b'
    assert service['status'] == 'PASS' and service['process']['exitCode'] == 0 and service['source']['unchanged'] is True
    assert service['source']['beforeSha256'] == service['source']['afterSha256']
    script = service['checkScript']['source'].encode('utf-8')
    assert len(script) == service['checkScript']['utf8Bytes'] and digest(script) == service['checkScript']['sha256']
    for stream in ('stdout', 'stderr'):
        verify_raw_blob(service['process'][stream])
    return {'cacheSettle': {'localBeforeExpectedFailure': True, 'localAfterStatusOriginal': after['status'],
                            'originalFirstWindowsFailurePreserved': True, 'scope': 'Local model/regression evidence; separate actual second Windows browser report proves its own PASS.'},
            'observerMock': {'report': service_name, 'status': service['status'], 'scope': service['scope']},
            'msvcExactSha': {'statusOriginal': static['status'], 'contractModelScope': static['contractModelScope'],
                             'windowsReadOnlyAst': ast['parsed'], 'ciDispatch': 'NOT_RUN', 'toolchainInstall': 'NOT_RUN',
                             'scope': 'Required independent source_sha and fail-closed SHA checks have local static/model evidence plus actual Windows AST parse only. No CI run/build/runtime or main-branch promotion is inferred.'}}


def validate_terminal_reports(manifest, selected):
    # Root confirmed every original writer terminal; all validators below use
    # actual report schemas and preserve original failed attempts.
    assert TERMINAL_SCHEMAS_REVIEWED, 'PREPARED ONLY: actual terminal browser/TLS/audit/cleanup schema review is still pending'
    browser = validate_completed_browser_reports(manifest, selected)
    local = validate_local_models(selected)
    names = manifest['terminalReports']
    assert all(isinstance(name, str) and name for name in names.values()), 'Actual final report names remain pending'
    runtime = load_selected(names['windowsRuntime'], selected)
    runtime_result = load_selected(names['windowsRuntimeResult'], selected)
    runtime_invocation = load_selected(names['windowsRuntimeInvocation'], selected)
    audit = load_selected(names['windowsFinalAudit'], selected)
    cleanup = load_selected(names['localCleanup'], selected)
    assert runtime['status'] == runtime['cleanup'] == audit['status'] == cleanup['status'] == 'PASS'
    assert audit['ownedProcesses'] == audit['ownedTasks'] == audit['scratchDirectories'] == []
    assert audit['projectRunUnchanged'] is audit['publicAuthorizationUnchanged'] is True
    assert audit['sshd'] == 'Running' and audit['persistentKeyMode'] == '0o600'
    assert digest(source_path(names['windowsFinalAudit']).read_bytes()) == 'dd50b53074cd653dfb70c294a744db3aaffc8ef9ac3a267442ea7b811df357d3'
    assert runtime_invocation['exitCode'] == 0 and runtime_invocation['inputChanges'] == runtime_invocation['artifactChanges'] == []
    assert runtime_invocation['runnerSourceBefore'] == runtime_invocation['runnerSourceAfter']
    for stream in runtime_invocation['streams'].values():
        data = Path(stream['path']).read_bytes()
        assert len(data) == stream['bytes'] and digest(data) == stream['sha256']
    for audit_name in ['native-browser-windows-before.json', 'native-browser-windows-before-retry1.json',
                       'native-browser-windows-after-first-failure.json', names['windowsFinalAudit']]:
        observation = load_selected(audit_name, selected)
        for stream in ('stdout', 'stderr'):
            verify_raw_blob(observation['ssh'][stream])
        source = observation['powershell']['fullSource']
        assert digest(source.encode()) == observation['powershell']['sourceSha256Utf8']
        assert digest(source.encode('utf-16-le')) == observation['powershell']['sourceSha256Utf16Le']
        helper = 'audit-native-browser-windows-first.py' if audit_name == 'native-browser-windows-before.json' else 'audit-native-browser-windows.py'
        assert digest(source_path(helper).read_bytes()) == observation['auditSource']['sha256']
        if audit_name == 'native-browser-windows-before.json':
            assert observation['status'] == 'FAIL' and observation['ssh']['exitCode'] == 1
        else:
            assert observation['status'] == 'PASS' and observation['ssh']['exitCode'] == 0
            assert observation['projectRunUnchanged'] is observation['publicAuthorizationUnchanged'] is True
    host = load_selected('native-browser-host-workspace-first.json', selected)
    compile_report = load_selected('native-browser-windows-workspace-first.json', selected)
    frozen = load_selected('native-browser-windows-workspace-frozen.json', selected)
    assert host['changedInputs'] == compile_report['changedInputs'] == []
    assert host['sourceBefore'] == host['sourceAfter'] == compile_report['sourceBefore'] == compile_report['sourceAfter']
    assert all(command['exitCode'] == 0 for command in host['commands'])
    assert len(host['artifacts']) == 20 and len(host['testGroups']) == 32
    assert host['totalPassed'] == sum(row['passed'] for row in host['testGroups']) == 852
    assert all(row['failed'] == row['ignored'] == 0 for row in host['testGroups'])
    assert compile_report['exitCode'] == 0 and len(frozen['programs']) == len(runtime['tests']) == 20
    assert frozen['compileReport']['sha256'] == digest(source_path('native-browser-windows-workspace-first.json').read_bytes())
    assert runtime_invocation['frozenBindingSha256'] == digest(source_path('native-browser-windows-workspace-frozen.json').read_bytes())
    assert frozen['sourceBefore'] == frozen['sourceAfter'] == compile_report['sourceAfter']
    assert all(row['sha256BeforeRun'] == row['sha256AfterRun'] for row in host['artifacts'])
    for row in frozen['programs']:
        data = Path(row['frozen']).read_bytes()
        assert len(data) == row['bytes'] and digest(data) == row['sha256']
    normal_bytes = Path(frozen['normal']['path']).read_bytes()
    assert digest(normal_bytes) == frozen['normal']['sha256'] == runtime['exeSha256']
    assert digest(normal_bytes) == 'bbfa4b3818ed4aa15abdfb879e4d2f8339f73b36730b8e8e83b6d8d926574d6a'
    programs = {Path(row['frozen']).name: row for row in frozen['programs']}
    uploaded = {row['name']: row['sha256'] for row in runtime['uploadedArtifacts']}
    results = []
    for test in runtime['tests']:
        assert test['exitCode'] == 0 and test['binarySha256'] == programs[test['name']]['sha256'] == uploaded[test['name']]
        matches = [re.search(r'test result: (ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;', line) for line in test['output']]
        matches = [match for match in matches if match]
        assert len(matches) == 1 and matches[0][1] == 'ok'
        passed, failed, ignored = (int(matches[0][n]) for n in (2, 3, 4))
        assert failed == ignored == 0
        results.append({'name': test['name'], 'sha256': test['binarySha256'], 'passed': passed, 'failed': failed, 'ignored': ignored})
    assert runtime_result['status'] == runtime_result['cleanup'] == 'PASS'
    assert runtime_result['runtimeSha256'] == digest(source_path(names['windowsRuntime']).read_bytes())
    assert sum(row['passed'] for row in results) == runtime_result['totalPassed'] == 1287
    assert runtime_result['programs'] == len(results) == 20
    assert runtime_result['groups'] == results
    raw_verification = load_selected(names['rawVerification'], selected)
    for row in raw_verification['runs']:
        data = Path(row['path']).read_bytes()
        assert digest(data) == row['sha256']
        original = json.loads(data)
        assert original['status'] == row['status'] and original['cleanup'] == row['cleanup'] == 'PASS'
        assert row['tlsFiles'] == 7 and row['edgeFiles'] == 8
    checks = load_selected(names['formatArchitecture'], selected)
    assert all(check['exitCode'] == 0 for check in checks['checks'])
    win_clippy = load_selected(names['windowsClippyResult'], selected)
    assert win_clippy['exitCode'] == win_clippy['originalToolResult']['exit_code'] == 0
    assert win_clippy['inputComparisonAtRecording']['currentRustAndCargoInputsEqualCompileSnapshot'] is True
    assert win_clippy['inputComparisonAtRecording']['compileReportSha256'] == digest(source_path('native-browser-windows-workspace-first.json').read_bytes())
    clippy_data = Path(win_clippy['log']['path']).read_bytes()
    assert len(clippy_data) == win_clippy['log']['bytes'] and digest(clippy_data) == win_clippy['log']['sha256']
    binding = load_selected('native-browser-input-binding.json', selected)
    snapshot = load_selected('native-browser-input-source-snapshot.json', selected)
    assert binding['status'] == 'INPUT_BYTES_SHA_VERIFIED'
    assert digest(source_path('native-browser-input-source-snapshot.json').read_bytes()) == binding['sourceSnapshot']['sha256']
    assert snapshot['crateCargoManifests'] == 12 and snapshot['fileCount'] == len(snapshot['files']) == 434
    assert binding['oldHeadFreezeMissingPaths'] == binding['oldHeadFreezeChangedPaths'] == []
    assert len(cleanup['deleted']) == 6 and sum(row['bytes'] for row in cleanup['deleted']) == cleanup['totalBytes'] == 29497011
    for row in cleanup['deleted']:
        assert not Path(row['path']).exists(), row['path']
        if 'sameBytesRetainedAt' in row:
            data = Path(row['sameBytesRetainedAt']).read_bytes()
            assert len(data) == row['bytes'] and digest(data) == row['sha256']
        else:
            assert row['sha256'] == binding['zip']['sha256'] and row['bytes'] == binding['zip']['bytes']
    # No guessed runtime total: sum actual terminal test results only.
    return {'browser': browser, 'localChecks': local, 'host': {'passed': host['totalPassed'], 'groups': 32, 'programs': 20},
            'windowsCompile': {'exitCode': compile_report['exitCode'], 'elapsedSeconds': compile_report['elapsedSeconds']},
            'windowsRuntime': {'programs': results, 'passed': sum(row['passed'] for row in results), 'cleanup': runtime['cleanup'],
                               'all20FrozenExesActuallyRehashed': True, 'normalActuallyRehashedAtNewSnapshot': frozen['normal'],
                               'oldNormalNotRebuiltByThisTestCompilation': True},
            'inputSnapshot': {'files': snapshot['fileCount'], 'rustFiles': snapshot['rustFiles'], 'webFiles': snapshot['webFiles'],
                              'crateManifests': 12, 'normalOriginalCaptureStillOmitted12Manifests': True},
            'formatArchitecture': checks, 'windowsStrictClippy': win_clippy,
            'finalWindowsAudit': audit, 'localCleanup': cleanup,
            'remainingBoundaries': ['Not final Round30/all-goal acceptance; main has not yet been synchronized.',
                                     'Strict native first-five-second gate, Win7/Win10 actual runtime, MSVC CI and physical Seewo acceptance remain unvalidated.',
                                     'Native browser evidence covers finite Win11 Edge synthetic homework only; no pairing/logout/microphone/playback UI acceptance is inferred.']}


def execute(args):
    assert args.terminal_confirmed and args.allowlist
    manifest_path = source_path(args.allowlist)
    assert manifest_path.name.startswith('native-browser-') and manifest_path.suffix == '.json'
    manifest = read_json(manifest_path)
    assert manifest['allOriginalWritersTerminal'] is True
    selected, excluded, expired = collect_explicit(manifest)
    assert Path(__file__).resolve() in selected and manifest_path in selected
    # Validate all original terminal reports before the first archive write.
    facts = validate_terminal_reports(manifest, selected)
    index_path = DST / (PREFIX + 'stage-progress-index.json')
    original_index = index_path.read_bytes()
    index = json.loads(original_index)
    assert len(index['files']) == PRIOR_COUNT and digest(original_index) == PRIOR_SHA
    verify_index(index)
    prior_rows = list(index['files'])
    existing = {row['path']: row for row in index['files']}
    # Preflight every selected byte and target before writing anything.
    payloads = {}
    for path in sorted(selected):
        data = checked_data(path, expired)
        relative = PREFIX + str(path.relative_to(SRC))
        destination = DST / relative
        if destination.exists():
            assert destination.read_bytes() == data, 'refuse differing existing archive: ' + relative
        payloads[path] = data
    summary = {
        'schemaVersion': 1,
        'scope': 'Completed explicit native-browser candidate stage only; no production listener, full Round30, P0, Win7/10 or physical Seewo acceptance is inferred.',
        'preservation': 'All selected original reports/failed attempts/raw bytes including hidden files remain unchanged. Original internal paths remain unchanged; path mapping resolves archived copies.',
        'priorIndex': {'verifiedFiles': len(prior_rows), 'sha256BeforeAppend': digest(original_index)},
        'factsFromOriginalTerminalReports': facts,
        'allowlist': {'path': str(manifest_path), 'sha256': digest(manifest_path.read_bytes()),
                      'files': manifest['files'], 'rawDirectories': manifest['rawDirectories']},
        'expiredStopCapabilities': sorted(expired),
        'excludedCapabilities': excluded,
        'archiverScope': 'Local evidence copy/hash/index only. No original deletion, remote action, process/browser/task/service/trust/Run/SSH authorization mutation or private-key read.',
        'sourceCoverageLimit': 'New browser input observation includes all 12 crate manifests. It does not retroactively expand the old normal compiler capture or prove a new current-web-source build.',
        'repositoryPromotion': manifest['mainPromotion'],
        'priorHeadSummary': PREFIX + 'head-stage-summary.json',
    }
    summary_path = SRC / SUMMARY
    assert not summary_path.exists(), 'summary must be a fresh stage report'
    summary_path.write_text(json.dumps(summary, ensure_ascii=False, indent=2) + '\n')
    payloads[summary_path] = checked_data(summary_path, expired)
    mappings = []
    added = []

    def copy(path, data):
        relative = PREFIX + str(path.relative_to(SRC))
        destination = DST / relative
        row = {'path': relative, 'bytes': len(data), 'sha256': digest(data)}
        assert path.read_bytes() == data, 'source changed after terminal preflight: ' + str(path)
        if destination.exists():
            assert destination.read_bytes() == data, 'refuse differing existing archive: ' + relative
        else:
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(path, destination)
        assert destination.read_bytes() == data
        if relative in existing:
            assert existing[relative] == row
        else:
            index['files'].append(row)
            existing[relative] = row
            added.append(relative)
        return {'original': str(path), 'archived': str(Path('开发计划/验证记录') / relative),
                'bytes': row['bytes'], 'sha256': row['sha256']}

    for path, data in sorted(payloads.items()):
        mappings.append(copy(path, data))
    mapping_path = SRC / MAPPING
    with mapping_path.open('x') as output:
        output.write(json.dumps({'schemaVersion': 1, 'scope': 'Original paths unchanged; mapped payloads are byte-identical.',
                                 'files': mappings, 'mappingSelf': 'Separately indexed to avoid recursive self-hashing.'}, ensure_ascii=False, indent=2) + '\n')
    copy(mapping_path, checked_data(mapping_path, expired))
    assert index['files'][:len(prior_rows)] == prior_rows
    assert index_path.read_bytes() == original_index, 'index changed concurrently; refuse overwrite'
    index_path.write_text(json.dumps(index, ensure_ascii=False, indent=2) + '\n')
    verify_index(index)
    for row in mappings:
        assert Path(row['original']).read_bytes() == Path(row['archived']).read_bytes()
    print(json.dumps({'status': 'ARCHIVE_BYTES_SHA_VERIFIED', 'priorFiles': len(prior_rows), 'addedFiles': len(added),
                      'indexedFiles': len(index['files']), 'indexedBytes': sum(row['bytes'] for row in index['files']),
                      'indexSha256': digest(index_path.read_bytes()),
                      'indexedFileManifestSha256': digest(json.dumps(index['files'], ensure_ascii=False, sort_keys=True, separators=(',', ':')).encode()),
                      'summary': str(DST / (PREFIX + SUMMARY)), 'mapping': str(DST / (PREFIX + MAPPING)),
                      'excludedCapabilities': excluded}, ensure_ascii=False, indent=2))


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--execute', action='store_true')
    parser.add_argument('--terminal-confirmed', action='store_true')
    parser.add_argument('--allowlist', help='Explicit native-browser-*.json basename naming individual files and raw directories')
    args = parser.parse_args()
    if args.execute:
        execute(args)
    else:
        print('PREPARED_ONLY: explicit allowlist and actual terminal-schema review pending; no evidence copying/index IO')
