"""Archive only a terminal, explicitly approved HEAD/typed-voice stage.

Preparation is inert: no evidence copying or index mutation occurs without
--execute --terminal-confirmed and explicit final-audit/cleanup/result names.
The final reports must exist and pass before the first archive write. This is
stage evidence, never whole-Round30 or full-workspace acceptance.
"""
from pathlib import Path
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
SUMMARY = 'head-stage-summary.json'
MAPPING = 'head-stage-path-mapping.json'
TEST_RESULT = re.compile(r'test result: (ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;')


def digest(data):
    return hashlib.sha256(data).hexdigest()


def load(name):
    assert Path(name).name == name, 'terminal report must be a source-directory basename'
    return json.loads((SRC / name).read_text())


def archive_relative(path):
    return PREFIX + str(path.relative_to(SRC))


def verify_index(index):
    assert len({row['path'] for row in index['files']}) == len(index['files'])
    for row in index['files']:
        data = (DST / row['path']).read_bytes()
        assert len(data) == row['bytes'] and digest(data) == row['sha256'], row['path']


def checked_data(path):
    assert path.is_file() and not path.is_symlink(), str(path)
    assert path.suffix in {'.json', '.jsonl', '.log', '.py', '.pem', '.raw', '.stdout', '.stderr'}, str(path)
    assert not path.name.endswith('.control.json') and '.active' not in path.name.lower(), str(path)
    data = path.read_bytes()
    assert not re.search(rb'(?m)^-----BEGIN (?:RSA |EC |OPENSSH |ENCRYPTED )?PRIVATE KEY-----', data), str(path)
    return data


def verify_raw(report):
    raw = Path(report['rawPreservation']['directory'])
    assert raw.parent == SRC and raw.is_dir() and not raw.is_symlink()
    assert report['rawPreservation']['status'] == 'PASS'
    files = sorted(p for p in raw.rglob('*') if p.is_file())
    for name, expected in report['remoteRawEvidence'].items():
        data = (raw / name).read_bytes()
        assert len(data) == expected['bytes'] and digest(data) == expected['sha256'], name
    for name, row in report['rawPreservation']['files'].items():
        assert row['status'] == 'PASS'
        assert base64.b64decode(row['base64'], validate=True) == (raw / name).read_bytes(), name
    return raw, files


def execute(args):
    assert args.terminal_confirmed, 'Root must first confirm ALL original writers are terminal'
    assert args.final_audit and args.cleanup_report and args.clippy_result
    # No source log is consumed while preparing this helper. Terminal JSON is
    # parsed here only after explicit execution authorization.
    runtime = load('head-windows-runtime-first.json')
    tls = load('head-tls-fixed-first.json')
    tls_verification = load('head-tls-fixed-original-verification.json')
    local_checks = load('head-final-local-checks.json')
    audit = load(args.final_audit)
    cleanups = {name: load(name) for name in args.cleanup_report}
    clippy = {name: load(name) for name in args.clippy_result}
    assert runtime['status'] == runtime['cleanup'] == tls['status'] == tls['cleanup'] == 'PASS'
    assert audit['status'] == 'PASS'
    assert audit['ownedProcesses'] == audit['ownedTasks'] == audit['scratchDirectories'] == []
    assert audit['sshd'] == 'Running' and audit['persistentKeyMode'] == '0o600'
    assert audit['publicAuthorizationUnchanged'] is True and audit['projectRunUnchanged'] is True
    assert all(report['status'] == 'PASS' for report in cleanups.values())
    for name, report in clippy.items():
        assert report['status'] == 'PASS' and report['sourcesStillMatchFreeze'] is True, name
        assert len(report['checks']) == 2
        for check in report['checks']:
            assert check['exitCode'] == 0 and check['exitCodeSource']
            assert digest(Path(check['log']).read_bytes()) == check['logSha256'], name
    assert local_checks['status'] == 'PASS'
    assert local_checks['testedRustCargoSourcesStillMatch'] is local_checks['testedRunnerStillMatches'] is True
    assert {row['name'] for row in local_checks['checks']} == {'architecture', 'fmt', 'diff'}
    for check in local_checks['checks']:
        assert check['exitCode'] == 0
        assert digest(Path(check['log']).read_bytes()) == check['logSha256']

    freeze = load('head-source-freeze.json')
    host = load('head-host-libraries-result.json')
    compile_report = load('head-windows-libraries-first.json')
    normal_report = load('head-normal-first.json')
    snapshot = load('head-windows-libraries-snapshot.json')
    binding = load('head-stage-program-binding.json')
    assert host['status'] == 'PASS' and host['exitCode'] == 0 and host['sourcesMatchFreezeAfter'] is True
    assert len(host['testGroups']) == 2
    assert sum(group['passed'] for group in host['testGroups']) == host['passed'] == 111
    assert all(group['status'] == 'ok' and group['failed'] == group['ignored'] == 0 for group in host['testGroups'])
    for report in [compile_report, normal_report]:
        assert report['exitCode'] == 0 and report['changedInputs'] == []
        assert report['sourceBefore'] == report['sourceAfter']
    for name, expected in freeze['rustCargoSources'].items():
        assert compile_report['sourceAfter'][name] == expected, name
    normal_missing_freeze = sorted(set(freeze['rustCargoSources']) - set(normal_report['sourceAfter']))
    # The original normal compiler wrapper captured Rust plus root Cargo inputs,
    # but omitted the twelve crate manifests. Preserve that original limitation;
    # the separate three-library compiler did capture all freeze fields.
    expected_manifest_omissions = sorted(str(path) for path in Path('crates').glob('*/Cargo.toml'))
    assert normal_missing_freeze == expected_manifest_omissions
    for name, expected in freeze['rustCargoSources'].items():
        if name in normal_report['sourceAfter']:
            assert normal_report['sourceAfter'][name] == expected, name
    common_sources = set(compile_report['sourceAfter']) & set(normal_report['sourceAfter'])
    assert all(compile_report['sourceAfter'][name] == normal_report['sourceAfter'][name] for name in common_sources)
    assert digest((SRC / 'head-windows-libraries-first.json').read_bytes()) == snapshot['compileReportSha256']
    assert binding['inputsStillMatch'] is True
    normal = binding['normal']
    assert len(normal_report['artifacts']) == 1
    assert normal['sha256'] == normal_report['artifacts'][0]['sha256'] == runtime['exeSha256'] == tls['exeSha256']
    assert normal['bytes'] == normal_report['artifacts'][0]['bytes']
    expected_tests = {Path(row['path']).name: row for row in snapshot['files']}
    assert len(expected_tests) == len(runtime['tests']) == 3
    for row in [normal, *snapshot['files']]:
        data = Path(row['path']).read_bytes()
        assert len(data) == row['bytes'] and digest(data) == row['sha256'], row['path']
    uploaded = {row['name']: row['sha256'] for row in runtime['uploadedArtifacts']}
    assert uploaded['fc-bootstrap.exe'] == normal['sha256']
    programs = []
    for test in runtime['tests']:
        assert test['exitCode'] == 0
        assert test['binarySha256'] == uploaded[test['name']] == expected_tests[test['name']]['sha256']
        results = [TEST_RESULT.search(line) for line in test['output']]
        results = [match for match in results if match]
        assert len(results) == 1 and results[0][1] == 'ok', test['name']
        passed, failed, ignored = (int(results[0][n]) for n in (2, 3, 4))
        assert failed == ignored == 0
        programs.append({'name': test['name'], 'binarySha256': test['binarySha256'],
                         'passed': passed, 'failed': failed, 'ignored': ignored, 'exitCode': test['exitCode']})
    assert sum(row['passed'] for row in programs) == 526
    assert {expected_tests[row['name']]['target']: row['passed'] for row in programs} == {
        'fc_protocol': 80, 'fc_windows': 156, 'fc_bootstrap': 290}
    assert runtime['selfCheck']['exitCode'] == runtime['native']['exitCode'] == 0

    tested_runner = SRC / 'head-tls-runner-tested.py'
    assert digest(tested_runner.read_bytes()) == tls['runnerSourceSha256'] == freeze['runnerSha256']
    http = [event for event in tls['events'] if 'httpStatus' in event]
    assert len(http) == 45 and len(tls['events']) == 47 and all(event['status'] == 'PASS' for event in tls['events'])
    assert {event['case'] for event in tls['events'] if 'httpStatus' not in event} == {'wrong-ca-rejected', 'wrong-hostname-rejected'}
    for event in http:
        date = event['responseDate']
        assert format_datetime(parsedate_to_datetime(date), usegmt=True) == date
        assert event['closeNotify'] == 'PASS: actual clean TLS EOF'
    bodyless = [event for event in http if event.get('method') == 'HEAD' or event['httpStatus'] in (204, 304)]
    assert all(event['responseBodyBytes'] == 0 for event in bodyless)
    heads = [event for event in http if event['method'] == 'HEAD']
    assert [(event['case'], event['httpStatus']) for event in heads] == [
        ('static-head-method', 405), ('static-head-host', 403), ('api-head-get', 405),
        ('api-head-post', 405), ('api-head-shared', 405), ('api-head-unknown', 404), ('api-head-host', 403)]
    assert all(event['responseFraming'] == 'HEAD response; CL/TE omitted and zero wire body' for event in heads)
    allow_cases = {'static-head-method': 'GET', 'api-head-get': 'GET', 'api-head-post': 'POST',
                   'api-head-shared': 'GET, POST', 'static-wrong-method': 'GET', 'api-wrong-method-status': 'GET',
                   'api-wrong-method-pairing/requests': 'POST', 'api-wrong-method-voice/broadcasts': 'GET, POST'}
    assert {event['case']: event['responseAllow'] for event in http if event['httpStatus'] == 405} == allow_cases
    assert len(tls['static']['assets']) == 6 and tls['static']['status'] == 'PASS'
    assert tls['business']['exactRetryReceipt'] is True and tls['business']['downloadClientRetainedBodyBytes'] == 0
    assert tls['business']['materialSha256'] == tls['business']['downloadSha256']
    assert tls['nativeProcessExit']['code'] == 0 and len(tls['nativeReports']) == 1
    native = tls['native']
    assert native['physicalNativeDrain'] is native['workerJoin'] is native['setupCleanup'] is True
    assert native['sharedWorkers'] == 2 and native['hubPending'] == native['downloadHubPending'] == native['quarantined'] == 0
    assert native['heads'] == native['responses'] == 45 and native['peakRecords'] == 5
    assert tls_verification['status'] == 'PASS'
    assert tls_verification['reportSha256'] == digest((SRC / 'head-tls-fixed-first.json').read_bytes())
    assert tls_verification['httpRequests'] == 45 and tls_verification['headRequests'] == 7 and tls_verification['certificateNegatives'] == 2
    final_raw, final_raw_files = verify_raw(tls)
    for row in tls_verification['originalFiles']:
        data = (final_raw / row['name']).read_bytes()
        assert len(data) == row['bytes'] and digest(data) == row['sha256']
    debug_fields = {}
    assert len(tls['nativeDebugLines']) == 1
    for name, value in re.findall(r'(\w+): (\d+|true|false)', tls['nativeDebugLines'][0]):
        debug_fields[name] = value == 'true' if value in ('true', 'false') else int(value)
    assert debug_fields['closed'] == debug_fields['retired'] == 83 and debug_fields['rearms'] == 79
    assert debug_fields['stale_events'] == debug_fields['active_records'] == debug_fields['pending_jobs'] == 0
    assert debug_fields['cleanup_pending'] == debug_fields['retained_cleanup'] == 0
    assert debug_fields['native_drained'] is debug_fields['stopping'] is True
    assert debug_fields['owned_bytes'] == 1575756

    baseline = load('head-tls-baseline-first.json')
    red = load('head-tls-baseline-verification.json')
    assert baseline['status'] == 'FAIL' and baseline['cleanup'] == 'PASS'
    assert red['status'] == 'EXPECTED_BASELINE_REJECTION_PASS' and red['cleanup'] == 'PASS'
    assert red['originalReportSha256'] == digest((SRC / 'head-tls-baseline-first.json').read_bytes())
    assert red['frozenPreviousCandidateSha256'] == baseline['exeSha256'] != normal['sha256']
    assert red['actualNativeDrain'] == 'NOT_VALIDATED_ON_THIS_FAILED_CLIENT_RUN'
    assert len(baseline['events']) == 1 and baseline['events'][0]['case'] == 'static-head-method'
    assert baseline['events'][0]['status'] == 'FAIL'
    assert baseline['nativeReports'] == []
    assert digest((SRC / 'head-tls-runner-baseline-tested.py').read_bytes()) == baseline['runnerSourceSha256']
    assert (SRC / 'head-tls-runner-baseline-tested.py').read_bytes() == tested_runner.read_bytes()
    baseline_raw, baseline_raw_files = verify_raw(baseline)

    # The previous snapshot was already removed by Root. Correlate its recorded
    # deletion inventory to the archived original manifest; never claim a new
    # hash of a now-absent old binary. Only the current four EXEs are rehashed.
    cleanup = cleanups['head-local-cleanup.json']
    assert cleanup['obsoleteDirectoryAbsent'] is cleanup['webZipAbsent'] is True
    assert not Path(cleanup['obsoleteDirectory']).exists()
    assert len(cleanup['obsoleteFiles']) == 22
    assert sum(row['bytes'] for row in cleanup['obsoleteFiles']) == cleanup['removedSnapshotBytes'] == 99618676
    prior_snapshot = load('http-semantics-windows-snapshot.json')
    removed = {row['name']: row for row in cleanup['obsoleteFiles']}
    assert len(prior_snapshot['files']) == 21
    for row in prior_snapshot['files']:
        actual = removed[Path(row['frozen']).name]
        assert actual['bytes'] == row['bytes'] and actual['sha256'] == row['sha256']

    index_path = DST / (PREFIX + 'stage-progress-index.json')
    original_index = index_path.read_bytes()
    index = json.loads(original_index)
    verify_index(index)
    initial_count = len(index['files'])
    existing = {row['path']: row for row in index['files']}
    selected = {Path(__file__).resolve()}
    selected.update(SRC / name for name in [args.final_audit, *args.cleanup_report, *args.clippy_result])
    excluded = []
    for path in SRC.iterdir():
        if not path.name.startswith('head-') or path.name in (SUMMARY, MAPPING):
            continue
        if path.is_dir():
            if not path.name.endswith('-raw'):
                excluded.append({'path': str(path), 'reason': 'not a raw evidence directory; no caches/build payload'})
                continue
            assert not path.is_symlink()
            selected.update(file for file in path.rglob('*') if file.is_file())
        elif path.name.endswith('.control.json') or '.active' in path.name.lower():
            excluded.append({'path': str(path), 'reason': 'active control capability'})
        else:
            selected.add(path)
    assert set(final_raw_files + baseline_raw_files).issubset(selected)
    # Validate all candidate bytes before the first archive write. Historical
    # inactive synthetic raw DTOs remain byte-identical, never rewritten.
    for path in selected:
        data = checked_data(path)
        destination = DST / archive_relative(path)
        if destination.exists():
            assert destination.read_bytes() == data, 'refuse differing existing archive: ' + str(destination)

    summary = {
        'schemaVersion': 1,
        'scope': 'Completed HEAD/typed-voice-method stage only: two Host libraries, three actual Windows libraries and separate explicit candidate TLS diagnostic. Not full-workspace/final Round30 acceptance.',
        'preservation': 'Original JSON/logs/raw bytes (including hidden files and ExpectedRed) remain unchanged; original internal paths are preserved via a separate mapping.',
        'priorIndex': {'verifiedFiles': initial_count, 'sha256BeforeAppend': digest(original_index)},
        'sourceBinding': {'freeze': PREFIX + 'head-source-freeze.json', 'changedPaths': list(freeze['changedSincePreviousVerifiedHttpPhase']),
                          'freezeFilesMatchLibraryCompileReport': True, 'normalCapturedFreezeFieldsMatch': True,
                          'normalOriginalCaptureOmittedCrateManifests': normal_missing_freeze,
                          'commonCompileSourcePaths': len(common_sources),
                          'bothChangedInputs': [], 'normalTlsRuntimeSnapshotSha256': normal['sha256']},
        'host': host,
        'windowsCompile': {'libraries': {'elapsedSeconds': compile_report['elapsedSeconds'], 'exitCode': compile_report['exitCode'], 'programs': len(snapshot['files'])},
                           'normal': {'elapsedSeconds': normal_report['elapsedSeconds'], 'exitCode': normal_report['exitCode'], 'sha256': normal['sha256']}},
        'strictClippy': clippy,
        'finalLocalChecks': local_checks,
        'windowsRuntime': {'report': PREFIX + 'head-windows-runtime-first.json', 'status': runtime['status'], 'cleanup': runtime['cleanup'],
                           'platform': runtime['platform'], 'programs': programs, 'passed': sum(row['passed'] for row in programs),
                           'failed': sum(row['failed'] for row in programs), 'ignored': sum(row['ignored'] for row in programs),
                           'selfCheck': runtime['selfCheck'], 'native': runtime['native'], 'allFourFrozenLocalFilesActuallyRehashed': True},
        'nativeTls': {'report': PREFIX + 'head-tls-fixed-first.json', 'status': tls['status'], 'cleanup': tls['cleanup'], 'httpResponses': len(http),
                      'certificateRejections': len(tls['events']) - len(http), 'runnerSourceSha256': tls['runnerSourceSha256'],
                      'redBlueRunnerBytesIdentical': True, 'canonicalUtcDateResponsesChecked': len(http),
                      'headCases': [{'case': event['case'], 'httpStatus': event['httpStatus'], 'responseAllow': event.get('responseAllow'),
                                     'wireBodyBytes': event['responseBodyBytes'], 'framing': event['responseFraming']} for event in heads],
                      'all405ExactAllowAssertions': allow_cases,
                      'native': tls['native'], 'business': tls['business'], 'static': tls['static'],
                      'nativeDebugReportParsed': debug_fields,
                      'ownedBytesScope': 'Retained application allocation accounting from the original candidate report, not observed process working-set or peak memory.',
                      'bodylessResponses': [{'case': event['case'], 'method': event.get('method'), 'status': event['httpStatus'],
                                             'wireBodyBytes': event['responseBodyBytes'], 'framing': event.get('responseFraming')} for event in bodyless],
                      'raw': {'directory': PREFIX + final_raw.name, 'files': len(final_raw_files),
                              'dotfiles': sum(p.name.startswith('.') for p in final_raw_files), 'base64RemoteShaAndLocalBytesVerified': True}},
        'expectedBaselineRed': {'report': PREFIX + 'head-tls-baseline-first.json', 'verification': red,
                                'originalStatus': baseline['status'], 'originalCleanup': baseline['cleanup'], 'oldExeSha256': baseline['exeSha256'],
                                'eventsObserved': len(baseline['events']), 'remainingCases': 'NOT_RUN after first HEAD Content-Length rejection',
                                'actualNativeDrain': 'NOT_VALIDATED; native report absent on original failed-client run',
                                'scope': 'Previous frozen candidate rejected by new stricter diagnostic HEAD contract. This is expected before-fix evidence, not a failed execution of the new candidate.',
                                'headContentLengthScope': 'The diagnostic contract chooses to omit Content-Length for these HEAD responses. HTTP generally permits a representative Content-Length on HEAD; the baseline CL rejection alone does not establish a general RFC violation.',
                                'raw': {'directory': PREFIX + baseline_raw.name, 'files': len(baseline_raw_files),
                                        'dotfiles': sum(p.name.startswith('.') for p in baseline_raw_files)}},
        'cleanup': {'finalWindowsAudit': audit, 'localReports': cleanups,
                    'obsoleteSnapshotComparison': {'recordedDeletedFiles': len(removed), 'recordedDeletedBytes': cleanup['removedSnapshotBytes'],
                                                   'binaryEntriesMatchedPriorArchivedManifest': len(prior_snapshot['files']),
                                                   'oldNormalActuallyRehashedThisPhase': False, 'currentFourExesActuallyRehashed': True},
                    'retainedScope': cleanup['retainedForContinuingRound30'],
                    'archiverActions': 'Only copy/hash/index. Original evidence and current frozen binaries are retained; no remote process/service/trust/firewall/Run/SSH authorization/private-key mutation.'},
        'remainingBoundaries': ['Strict native TCP first-five-second gate has not passed; no production listener enablement is claimed.',
                                'HEAD currently returns the existing method/boundary errors; no GET-equivalent HEAD200 endpoint support was added.',
                                'The legacy HTTP.sys implementation is not covered by the new native HEAD evidence.',
                                'Windows 7/10 actual-runtime and MSVC evidence remain separate; this stage supplies actual Windows 11 only.',
                                'A real browser against the native TLS candidate remains unvalidated; these are raw TLS client observations.',
                                'No physical Seewo/visible desktop/audio playback acceptance is supplied.',
                                'Prior Chromium6/33 used API routing/cache disabled; prior WinEdge2/13 true-cache Python fixture304 and old35HTTP still used CL0. Those original historical reports are unchanged.',
                                'Original full-workspace Host848/Windows1278 evidence predates this source freeze; this stage tests only affected libraries and finite TLS, not all20 new-source executables.'],
        'priorHttpSemanticsSummary': PREFIX + 'http-semantics-stage-summary.json',
        'syntheticRawCapabilityScope': 'Only historical already-retired test roots/ports and synthetic fixtures may be retained verbatim; no persistent SSH credential/private key or live control capability is archived.',
        'excludedCandidates': excluded,
    }
    summary_source = SRC / SUMMARY
    summary_source.write_text(json.dumps(summary, ensure_ascii=False, indent=2) + '\n')
    selected.add(summary_source)
    mappings = []
    added = []

    def copy(path):
        data = checked_data(path)
        relative = archive_relative(path)
        destination = DST / relative
        row = {'path': relative, 'bytes': len(data), 'sha256': digest(data)}
        if destination.exists():
            assert destination.read_bytes() == data, 'refuse differing existing archive: ' + relative
        else:
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(path, destination)
        assert path.read_bytes() == destination.read_bytes() == data
        if relative in existing:
            assert existing[relative] == row
        else:
            index['files'].append(row)
            existing[relative] = row
            added.append(relative)
        return {'original': str(path), 'archived': str(Path('开发计划/验证记录') / relative),
                'bytes': row['bytes'], 'sha256': row['sha256']}

    for path in sorted(selected):
        mappings.append(copy(path))
    mapping_source = SRC / MAPPING
    mapping_source.write_text(json.dumps({'schemaVersion': 1, 'scope': 'Original paths unchanged; every mapping resolves to identical bytes.',
                                         'files': mappings, 'mappingSelf': 'Separately indexed to avoid recursive self-hashing.'}, ensure_ascii=False, indent=2) + '\n')
    copy(mapping_source)
    assert index_path.read_bytes() == original_index, 'index changed concurrently; refuse overwrite'
    index_path.write_text(json.dumps(index, ensure_ascii=False, indent=2) + '\n')
    verify_index(index)
    for row in mappings:
        assert Path(row['original']).read_bytes() == Path(row['archived']).read_bytes()
    print(json.dumps({'status': 'ARCHIVE_BYTES_SHA_VERIFIED', 'priorFiles': initial_count, 'addedFiles': len(added),
                      'indexedFiles': len(index['files']), 'indexedBytes': sum(row['bytes'] for row in index['files']),
                      'indexSha256': digest(index_path.read_bytes()),
                      'indexedFileManifestSha256': digest(json.dumps(index['files'], ensure_ascii=False, sort_keys=True, separators=(',', ':')).encode()),
                      'summary': str(DST / archive_relative(summary_source)), 'mapping': str(DST / archive_relative(mapping_source)),
                      'runtimePrograms': len(programs), 'runtimePassed': sum(row['passed'] for row in programs),
                      'tlsHttpResponses': len(http), 'baselineRawFiles': len(baseline_raw_files), 'finalRawFiles': len(final_raw_files)}, ensure_ascii=False, indent=2))


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--execute', action='store_true')
    parser.add_argument('--terminal-confirmed', action='store_true')
    parser.add_argument('--final-audit', help='Explicit terminal audit JSON basename, confirmed by Root')
    parser.add_argument('--cleanup-report', action='append', default=[], help='Explicit completed cleanup JSON basename')
    parser.add_argument('--clippy-result', action='append', default=[], help='Explicit terminal strict-Clippy result JSON basename')
    args = parser.parse_args()
    if args.execute:
        execute(args)
    else:
        print('PREPARED_ONLY: no evidence copying/index IO; await final runtime/TLS/audit/cleanup schemas and explicit terminal confirmation')
