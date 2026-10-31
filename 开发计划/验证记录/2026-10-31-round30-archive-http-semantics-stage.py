"""Explicit, terminal-gated archive of the HTTP-semantics stage, not final acceptance.

Without --execute this helper performs no evidence/index IO. Execute only after
Root confirms original runtime, audit and cleanup writers have actually ended.
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
SUMMARY = 'http-semantics-stage-summary.json'
MAPPING = 'http-semantics-stage-path-mapping.json'
EXPLICIT = {'tls-runner-empty-status-selftest.json', 'freeze-http-semantics-windows.py',
            'run-http-semantics-win-compile.py'}


def digest(data):
    return hashlib.sha256(data).hexdigest()


def load(name):
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


def execute():
    # Read only terminal reports first. No source log is touched and no copying
    # begins unless all original actual-runtime/final-audit/cleanup gates pass.
    runtime = load('http-semantics-windows-runtime-first.json')
    audit = load('http-semantics-windows-final-after.json')
    intermediate = load('http-semantics-intermediate-local-cleanup.json')
    obsolete = load('http-semantics-obsolete-local-snapshots-cleanup.json')
    win_clippy = load('http-semantics-windows-clippy-result.json')
    assert runtime['status'] == runtime['cleanup'] == 'PASS'
    assert audit['status'] == 'PASS' and audit['ownedProcesses'] == audit['ownedTasks'] == audit['scratchDirectories'] == []
    assert audit['sshd'] == 'Running' and audit['persistentKeyMode'] == '0o600'
    assert audit['publicAuthorizationUnchanged'] is True and audit['projectRunUnchanged'] is True
    assert intermediate['status'] == obsolete['status'] == win_clippy['status'] == 'PASS'
    assert intermediate['temporaryWebZipAbsent'] and intermediate['individualNormalSnapshotAbsent']
    assert win_clippy['exitCode'] == 0 and win_clippy['sourceStillMatchesSuccessfulWorkspaceCompile'] is True
    assert digest(Path(win_clippy['log']).read_bytes()) == win_clippy['logSha256']

    host = load('http-semantics-host-workspace-first.json')
    compile_report = load('http-semantics-windows-workspace-first.json')
    snapshot = load('http-semantics-windows-snapshot.json')
    normal = load('http-semantics-normal-first-snapshot.json')
    tls = load('http-semantics-tls-first.json')
    source_freeze = load('http-semantics-source-freeze.json')
    raw_verification = load('http-semantics-tls-original-bytes-verification.json')
    rejection = load('http-semantics-windows-runtime-first-cli-rejection.json')
    assert host['changedInputs'] == compile_report['changedInputs'] == []
    assert host['sourceAfter'] == compile_report['sourceAfter'], 'Host/Windows sourceAfter mismatch'
    assert host['sourceBefore'] == host['sourceAfter'] and compile_report['sourceBefore'] == compile_report['sourceAfter']
    assert all(command['exitCode'] == 0 for command in host['commands'])
    assert len(host['testGroups']) == 32 and len(host['artifacts']) == 20
    assert host['totalPassed'] == sum(group['passed'] for group in host['testGroups']) == 848
    assert all(group['failed'] == group['ignored'] == 0 for group in host['testGroups'])
    assert all(row['sha256BeforeRun'] == row['sha256AfterRun'] for row in host['artifacts'])
    assert compile_report['exitCode'] == 0
    assert digest((SRC / 'http-semantics-windows-workspace-first.json').read_bytes()) == snapshot['compileReportSha256']
    correlated = []
    for path, expected in source_freeze['files'].items():
        if path in host['sourceAfter']:
            assert host['sourceAfter'][path] == expected['sha256'], path
            correlated.append(path)

    # Independently compare the early normal metadata, completed full snapshot,
    # actual still-retained local frozen files, actual uploaded/runtime binaries
    # and the EXE used by the completed TLS observation. Do not archive EXEs.
    normal_rows = [row for row in snapshot['files'] if not row['test']]
    test_rows = [row for row in snapshot['files'] if row['test']]
    assert len(normal_rows) == 1 and len(test_rows) == len(runtime['tests']) == 20
    assert normal_rows[0]['sha256'] == normal['sha256'] == runtime['exeSha256'] == tls['exeSha256']
    assert normal_rows[0]['bytes'] == normal['bytes']
    for row in snapshot['files']:
        data = Path(row['frozen']).read_bytes()
        assert len(data) == row['bytes'] and digest(data) == row['sha256'], row['frozen']
    expected_tests = {Path(row['frozen']).name: row for row in test_rows}
    uploaded = {row['name']: row['sha256'] for row in runtime['uploadedArtifacts']}
    assert len(expected_tests) == 20 and uploaded['fc-bootstrap.exe'] == normal['sha256']
    program_results = []
    pattern = re.compile(r'test result: (ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;')
    for test in runtime['tests']:
        assert test['exitCode'] == 0 and test['name'] in expected_tests
        assert test['binarySha256'] == expected_tests[test['name']]['sha256'] == uploaded[test['name']]
        matches = [pattern.search(line) for line in test['output']]
        matches = [match for match in matches if match]
        assert len(matches) == 1 and matches[0][1] == 'ok', test['name']
        passed, failed, ignored = (int(matches[0][number]) for number in (2, 3, 4))
        assert failed == ignored == 0
        program_results.append({'name': test['name'], 'binarySha256': test['binarySha256'],
                                'passed': passed, 'failed': failed, 'ignored': ignored, 'exitCode': test['exitCode']})
    assert sum(row['passed'] for row in program_results) == 1278

    tested_runner = SRC / 'http-semantics-tls-runner-tested.py'
    assert digest(tested_runner.read_bytes()) == tls['runnerSourceSha256']
    assert tls['status'] == tls['cleanup'] == raw_verification['status'] == 'PASS'
    assert digest((SRC / 'http-semantics-tls-first.json').read_bytes()) == raw_verification['reportSha256']
    http = [event for event in tls['events'] if 'httpStatus' in event]
    assert len(http) == 38 and len(tls['events']) == 40 and all(event['status'] == 'PASS' for event in tls['events'])
    for event in http:
        date = event['responseDate']
        assert format_datetime(parsedate_to_datetime(date), usegmt=True) == date
        assert event['closeNotify'] == 'PASS: actual clean TLS EOF'
    conditional = [event for event in http if event['httpStatus'] == 304]
    assert len(conditional) == 2 and all(event['responseBodyBytes'] == 0 and
        event['responseFraming'] == 'status-defined empty; CL/TE omitted by diagnostic contract' for event in conditional)
    allow_cases = {'static-wrong-method': 'GET', 'api-wrong-method-status': 'GET',
                   'api-wrong-method-pairing/requests': 'POST', 'api-wrong-method-voice/broadcasts': 'GET, POST'}
    assert {event['case'] for event in http if event['httpStatus'] == 405} == set(allow_cases)
    assert rejection['exitCode'] == 2
    raw_dir = Path(tls['rawPreservation']['directory'])
    assert raw_dir.parent == SRC and not raw_dir.is_symlink()
    raw_files = sorted(file for file in raw_dir.rglob('*') if file.is_file())
    for name, expected in tls['remoteRawEvidence'].items():
        data = (raw_dir / name).read_bytes()
        assert len(data) == expected['bytes'] and digest(data) == expected['sha256']
    for name, row in tls['rawPreservation']['files'].items():
        assert row['status'] == 'PASS'
        assert base64.b64decode(row['base64'], validate=True) == (raw_dir / name).read_bytes()

    index_path = DST / (PREFIX + 'stage-progress-index.json')
    original_index = index_path.read_bytes()
    index = json.loads(original_index)
    verify_index(index)
    existing = {row['path']: row for row in index['files']}
    initial_count = len(index['files'])
    selected = {Path(__file__).resolve()}
    excluded = []
    for file in SRC.iterdir():
        if not (file.name.startswith('http-semantics-') or file.name in EXPLICIT):
            continue
        if file.name in (SUMMARY, MAPPING):
            continue
        if file.is_dir():
            if not file.name.endswith('-raw'):
                excluded.append({'path': str(file), 'reason': 'not a raw evidence directory; no build/cache payloads'})
                continue
            assert not file.is_symlink()
            selected.update(path for path in file.rglob('*') if path.is_file())
        elif file.name.endswith('.control.json') or '.active' in file.name.lower():
            excluded.append({'path': str(file), 'reason': 'active control capability'})
        else:
            selected.add(file)
    assert set(raw_files).issubset(selected)
    summary = {
        'schemaVersion': 1,
        'scope': 'Completed HTTP-semantics stage only; not final Round30 source freeze, production listener or full-goal acceptance.',
        'preservation': 'Original reports, failed CLI observation, complete logs and raw hidden files remain byte-identical; original internal paths are unchanged. Separate mapping resolves archived paths.',
        'priorIndex': {'verifiedFiles': initial_count, 'sha256BeforeAppend': digest(original_index)},
        'sourceBinding': {'hostAndWindowsSourceAfterExactlyEqual': True, 'capturedFiles': len(host['sourceAfter']),
            'bothChangedInputs': [], 'narrowFreezePathsMatched': correlated, 'originalKnownGaps': source_freeze['knownGaps']},
        'host': {'report': PREFIX + 'http-semantics-host-workspace-first.json', 'passed': host['totalPassed'],
            'groups': len(host['testGroups']), 'programs': len(host['artifacts']),
            'commands': [{'name': command['name'], 'exitCode': command['exitCode']} for command in host['commands']],
            'scope': 'Host runtime plus Host strict Clippy only; original source/artifact bindings retained.'},
        'windowsCompile': {'report': PREFIX + 'http-semantics-windows-workspace-first.json',
            'exitCode': compile_report['exitCode'], 'elapsedSeconds': compile_report['elapsedSeconds'],
            'changedInputs': compile_report['changedInputs'], 'reportedArtifacts': len(compile_report['artifacts'])},
        'windowsStrictClippy': win_clippy,
        'windowsRuntime': {'report': PREFIX + 'http-semantics-windows-runtime-first.json',
            'status': runtime['status'], 'cleanup': runtime['cleanup'], 'platform': runtime['platform'],
            'programs': program_results, 'passed': sum(row['passed'] for row in program_results),
            'failed': sum(row['failed'] for row in program_results), 'ignored': sum(row['ignored'] for row in program_results),
            'normalSha256': normal['sha256'], 'independentNormalAndFullSnapshotAndTlsAndRuntimeMatch': True,
            'all21FrozenLocalFilesActuallyRehashed': True, 'selfCheck': runtime['selfCheck'], 'native': runtime['native']},
        'nativeTls': {'report': PREFIX + 'http-semantics-tls-first.json', 'status': tls['status'],
            'cleanup': tls['cleanup'], 'httpResponses': len(http), 'certificateRejections': len(tls['events']) - len(http),
            'exeSha256': tls['exeSha256'], 'runnerSourceSha256': tls['runnerSourceSha256'],
            'conditional304': [{'case': event['case'], 'wireBodyBytes': event['responseBodyBytes'],
                                'framing': event['responseFraming']} for event in conditional],
            'uniqueCanonicalUtcDatesCheckedByTestedRunner': len(http),
            'allowAssertionsByExactTestedRunner': allow_cases,
            'allowEvidenceScope': 'The exact archived runner asserted unique Allow headers for these 405 cases. Original event schema records statuses but does not expose a separate Allow-value field.',
            'native': tls['native'], 'business': tls['business'],
            'raw': {'directory': PREFIX + raw_dir.name, 'remoteInventoryFiles': len(tls['remoteRawEvidence']),
                    'allLocalFilesIncludingMetadata': len(raw_files), 'hiddenFiles': sum(file.name.startswith('.') for file in raw_files),
                    'allRemoteBytesShaAndPreservedBase64Verified': True}},
        'cliRejection': {'report': PREFIX + 'http-semantics-windows-runtime-first-cli-rejection.json',
            'exitCode': rejection['exitCode'], 'scope': rejection['scope'],
            'note': 'Original alias CLI rejection occurred locally before remote work. Subsequently corrected host argument is recorded by actual runtime evidence; no runner edit is inferred.'},
        'cleanup': {'finalWindowsAudit': audit, 'intermediateLocal': intermediate, 'obsoleteSnapshots': obsolete,
            'scope': 'These are exactly the recorded cleanup scopes. Nine obsolete local snapshots and temporary ZIP/duplicate individual normal were removed earlier; current20-program snapshot, two Cargo caches, verified web dist and evidence are retained for continued Round30. No final-round cleanup is claimed.',
            'archiverActions': 'Local copy/hash/index only. Original evidence and current frozen EXEs are not deleted. No process, service, port, trust store, Run key, SSH authorization or private key is mutated.'},
        'remainingBoundaries': source_freeze['knownGaps'] + [
            'No physical Seewo/visible desktop/audio playback acceptance is supplied by this phase.',
            'Earlier Chromium6/33 used routed APIs/cache disabled; earlier WinEdge2/13 used true cache but Python fixture304 still emitted CL0. Those historical browser reports remain unchanged and are not retrospectively corrected by this native HTTP phase.',
            'This is independent stage evidence, not a final whole-workspace freeze or release package.'],
        'priorBrowserStageSummary': PREFIX + 'web-upgrade-stage-summary.json',
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
        return {'original': str(path), 'archived': str(Path('开发计划/验证记录') / relative), **{key: row[key] for key in ('bytes', 'sha256')}}

    for path in sorted(selected):
        mappings.append(copy(path))
    mapping_source = SRC / MAPPING
    mapping_source.write_text(json.dumps({'schemaVersion': 1, 'scope': 'Original JSON paths are unchanged; every mapping resolves to identical bytes.',
        'files': mappings, 'mappingSelf': 'Separately included in the SHA/byte index to avoid recursive self-hashing.'}, ensure_ascii=False, indent=2) + '\n')
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
        'runtimePrograms': len(program_results), 'runtimePassed': sum(row['passed'] for row in program_results),
        'rawFiles': len(raw_files), 'rawDotfiles': sum(file.name.startswith('.') for file in raw_files)}, ensure_ascii=False, indent=2))


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--execute', action='store_true', help='Root has confirmed all original stage writers terminal; archive only after complete terminal reports pass')
    args = parser.parse_args()
    if args.execute:
        execute()
    else:
        print('PREPARED_ONLY: no evidence/index read or write; requires explicit --execute after terminal-stage confirmation')
