"""Prepare final-stage archive only; wait for Root's local-cleanup terminal gate.

Explicit allowlist, original bytes/path preservation, old420 rows immutable.
The name final-stage does not imply all requirements, CI or release acceptance.
Without --execute no evidence/index IO occurs.
"""
from pathlib import Path, PurePosixPath
import argparse
import base64
from email.utils import format_datetime, parsedate_to_datetime
import hashlib
import json
import re
import shutil
import zipfile

SRC = Path('/tmp/fc-round30-evidence')
DST = Path.cwd() / '开发计划/验证记录'
PREFIX = '2026-10-31-round30-'
PRIOR_COUNT = 420
PRIOR_SHA = '88faff91b6064e21c4e54cd309495dfb213f7d74f4fa0ab900a89ac9d8bb973e'
SUMMARY = 'final-stage-summary.json'
MAPPING = 'final-stage-path-mapping.json'


def sha(data):
    return hashlib.sha256(data).hexdigest()


def path_for(name):
    relative = PurePosixPath(name)
    assert not relative.is_absolute() and '..' not in relative.parts and '\\' not in name
    assert relative.as_posix() == name
    path = SRC / name
    for component in [path, *path.parents]:
        if component == SRC:
            break
        assert not component.is_symlink()
    return path


def forbidden(path):
    return path.name.lower().endswith('.control.json') or '.active' in path.name.lower()


def data_for(path):
    assert path.is_file() and not path.is_symlink() and not forbidden(path)
    assert path.suffix in {'.json', '.jsonl', '.log', '.py', '.pem', '.raw', '.stdout', '.stderr', '.txt'}
    data = path.read_bytes()
    assert not re.search(rb'(?m)^-----BEGIN (?:RSA |EC |OPENSSH |ENCRYPTED )?PRIVATE KEY-----', data)
    return data


def blob(row):
    data = base64.b64decode(row['base64'], validate=True)
    assert len(data) == row['bytes'] and sha(data) == row['sha256']
    return data


def collect(manifest):
    selected = set()
    for name in manifest['files']:
        assert '/' not in name and (name.startswith(('final-', 'run-final-')) or name == 'archive-final-stage.py')
        path = path_for(name)
        assert path.is_file() and not forbidden(path)
        selected.add(path)
    for name in manifest['rawDirectories']:
        assert '/' not in name and name.startswith('final-') and name.endswith('-raw')
        directory = path_for(name)
        assert directory.is_dir()
        for path in directory.rglob('*'):
            assert not path.is_symlink()
            if path.is_file():
                assert not forbidden(path), 'final raw controls need explicit review, never read automatically'
                selected.add(path)
    assert len(manifest['files']) == len(set(manifest['files']))
    return selected


def load(name, selected):
    path = path_for(name)
    assert path in selected
    return json.loads(data_for(path))


def validate_known_reports(manifest, selected):
    build = load('final-win7-build-first.json', selected)
    pe = load('final-win7-freeze-and-pe-first.json', selected)
    tls = load('final-win7-tls-on-win11-first.json', selected)
    invocation = load('final-win7-tls-first-invocation.json', selected)
    package = load('final-modern-package-first.json', selected)
    audit = load('final-windows-after.json', selected)
    readonly = load('final-readonly-checks-first.json', selected)
    metadata = load('final-packaging-metadata-update.json', selected)
    actions = load('final-github-actions-readonly-preflight.json', selected)
    assert build['exitCode'] == pe['exitCode'] == invocation['exitCode'] == 0
    assert build['sourceBefore'] == build['sourceAfter'] and build['changedInputs'] == invocation['changedInputs'] == []
    assert len(build['artifacts']) == 1
    assert build['artifacts'][0]['sha256'] == pe['sha256'] == tls['exeSha256'] == invocation['exeSha256After'] == '67c5cbf7d27995e843f2a9ebc8e488ce48dc5e585b71bed045c8d66115f21e1b'
    assert pe['compileReportSha256'] == sha(path_for('final-win7-build-first.json').read_bytes())
    assert pe['parsed']['structural_errors'] == pe['parsed']['risks'] == []
    assert pe['parsed']['status'] == 'NOT_VALIDATED'
    assert tls['status'] == tls['cleanup'] == 'PASS' and tls['win7'] == tls['win10'] == 'NOT_VALIDATED'
    assert tls['fixture']['platform']['build'] == '26300'
    http = [event for event in tls['events'] if 'httpStatus' in event]
    assert len(http) == 45 and len(tls['events']) == 47 and all(event['status'] == 'PASS' for event in tls['events'])
    for event in http:
        assert event['closeNotify'] == 'PASS: actual clean TLS EOF'
        assert format_datetime(parsedate_to_datetime(event['responseDate']), usegmt=True) == event['responseDate']
    native = tls['native']
    assert native['heads'] == native['responses'] == 45 and native['sharedWorkers'] == 2
    assert native['physicalNativeDrain'] is native['workerJoin'] is native['setupCleanup'] is True
    assert native['hubPending'] == native['downloadHubPending'] == native['quarantined'] == 0
    raw = Path(tls['rawPreservation']['directory'])
    assert raw.parent == SRC and tls['rawPreservation']['status'] == 'PASS'
    raw_files = sorted(path for path in raw.rglob('*') if path.is_file())
    assert len(raw_files) == 8 and set(raw_files).issubset(selected)
    for name, row in tls['remoteRawEvidence'].items():
        data = (raw / name).read_bytes()
        assert len(data) == row['bytes'] and sha(data) == row['sha256']
    for name, row in tls['rawPreservation']['files'].items():
        assert row['status'] == 'PASS' and base64.b64decode(row['base64'], validate=True) == (raw / name).read_bytes()
    for stream in invocation['streams'].values():
        assert Path(stream['path']).read_bytes() == blob(stream)
    assert package['status'] == package['crc'] == package['allPayloadBytesAndSha256'] == 'PASS'
    assert package['sourceBefore'] == package['sourceAfter'] and package['changedInputs'] == []
    assert len(package['sourceBefore']) == 460
    assert package['members'] == 13 and package['payloads'] == 12 and package['independentSameBasenameRepack'] == 'BYTE_IDENTICAL'
    assert package['manifest']['status'] == 'NOT_VALIDATED'
    assert all(value == 'NOT_VALIDATED' for key, value in package['manifest']['gates'].items() if key != 'release_tag')
    assert package['manifest']['gates']['release_tag'] == 'NOT_ALLOWED'
    package_data = Path(package['path']).read_bytes()
    assert len(package_data) == package['bytes'] == 5942644 and sha(package_data) == package['sha256'] == invocation['webBundleSha256After'] == '03f1f4ca2fa098688dedad32c5e3a274a21baba313ad2d1ff29c089b141caad6'
    with zipfile.ZipFile(package['path']) as archive:
        assert len(archive.infolist()) == 13 and archive.testzip() is None
        for row in package['manifest']['files']:
            data = archive.read(row['path'])
            assert len(data) == row['size'] and sha(data) == row['sha256']
    assert all(run['exitCode'] == 0 for run in package['runs'] + readonly['runs'])
    for run in package['runs'] + readonly['runs']:
        for stream in ('stdout', 'stderr'):
            blob(run[stream])
    assert metadata['status'] == 'METADATA_ONLY_LOCAL_AST_AND_DIFF_PASS'
    assert actions['httpStatus'] == 200 and actions['repo'] == 'NevermoreN/future-classroom'
    assert audit['status'] == 'PASS' and audit['ownedProcesses'] == audit['ownedTasks'] == audit['scratchDirectories'] == []
    assert audit['publicAuthorizationUnchanged'] is audit['projectRunUnchanged'] is True
    assert audit['sshd'] == 'Running' and audit['persistentKeyMode'] == '0o600'
    assert sha(path_for('final-windows-after.json').read_bytes()) == '28197c76649c9825d3116116f40b9f25aac7b151d0663e9696f9efbefe3acae3'
    for stream in ('stdout', 'stderr'):
        blob(audit['ssh'][stream])
    return {'supplementalWin7Gnu': {'compileExitCode': 0, 'elapsedSeconds': build['elapsedSeconds'], 'exeSha256': pe['sha256'],
                                   'peStructuralErrors': 0, 'peRiskTriggers': 0, 'scope': build['scope'],
                                   'actualRuntime': 'SAME Windows11 build26300 only; not actual Windows7 or Windows10'},
            'supplementalTls': {'httpResponses': 45, 'certificateNegatives': 2, 'native': native, 'rawFiles': len(raw_files),
                                'rawHiddenFiles': sum(path.name.startswith('.') for path in raw_files),
                                'originalBytesBase64RemoteShaVerified': True},
            'modernDevelopmentPackage': {'path': package['path'], 'bytes': package['bytes'], 'sha256': package['sha256'],
                                          'members': 13, 'payloads': 12, 'independentRepack': package['independentSameBasenameRepack'],
                                          'observedInputFiles': 460, 'originalChangedInputs': [], 'status': package['manifest']['status'],
                                          'gates': package['manifest']['gates'],
                                          'scope': 'Development ZIP only. The 460-input before/after observation is not a retrospective expansion of an earlier compiler capture.'},
            'readonlyChecks': {'scope': readonly['scope'], 'runs': [{'name': run['name'], 'exitCode': run['exitCode']} for run in readonly['runs']],
                               'rustNotices': readonly['runs'][0]['parsed'],
                               'modernPe': {'status': readonly['runs'][1]['parsed']['status'],
                                            'structuralErrors': len(readonly['runs'][1]['parsed']['structural_errors']),
                                            'riskTriggers': len(readonly['runs'][1]['parsed']['risks'])}},
            'metadataUpdate': {'status': metadata['status'], 'scope': metadata['evidenceScope']},
            'githubActions': {'scope': actions['scope'], 'httpStatus': 200, 'ciDispatch': 'NOT_RUN'},
            'finalWindowsAudit': audit}


def validate_terminal(manifest, selected):
    facts = validate_known_reports(manifest, selected)
    assert manifest['allOriginalWritersTerminal'] is True
    assert manifest['terminalReports']['localCleanup'] and manifest['terminalReports']['finalChecksAndBinding']
    cleanup = load(manifest['terminalReports']['localCleanup'], selected)
    binding = load(manifest['terminalReports']['finalChecksAndBinding'], selected)
    tail = load('final-tail-checks.json', selected)
    extras = load('final-web-extra-originals.json', selected)
    assert cleanup['status'] == binding['status'] == tail['status'] == 'PASS'
    assert binding['inputCount'] == len(binding['inputs']) == 460
    canonical = json.dumps(binding['inputs'], ensure_ascii=False, sort_keys=True, separators=(',', ':')).encode()
    assert sha(canonical) == binding['canonicalSha256'] == '6439992139b75a544646e50a5fc13899fbe07f5fc7672ac9ad05378fbc53304e'
    assert binding['rustInputCommonFieldsUnchangedFromFullHostAndWindowsBuilds'] is binding['oldCaptureLimitationsRetained'] is True
    for name, row in binding['inputs'].items():
        data = Path(name).read_bytes()
        assert len(data) == row['bytes'] and sha(data) == row['sha256'], name
    assert binding['artifactCount'] == len(binding['artifacts']) == 42
    # Artifacts are now deleted. Correlate the actual pre-deletion observation
    # against their original compiled/frozen/runtime identities; never pretend
    # to rehash an absent path. The modern ZIP still contains actual bbfa bytes.
    host = json.loads((SRC / 'native-browser-host-workspace-first.json').read_text())
    frozen = json.loads((SRC / 'native-browser-windows-workspace-frozen.json').read_text())
    expected = {row['path']: row['sha256BeforeRun'] for row in host['artifacts']}
    expected.update({row['frozen']: row['sha256'] for row in frozen['programs']})
    expected[frozen['normal']['path']] = frozen['normal']['sha256']
    pe = load('final-win7-freeze-and-pe-first.json', selected)
    expected[pe['exe']] = pe['sha256']
    assert len(expected) == 42
    assert {row['path']: row['sha256'] for row in binding['artifacts']} == expected
    assert len(cleanup['deleted']) == 6 and cleanup['totalFiles'] == sum(row['files'] for row in cleanup['deleted']) == 4002
    assert cleanup['totalBytes'] == sum(row['bytes'] for row in cleanup['deleted']) == 1890453910
    for row in cleanup['deleted']:
        assert row['status'] == 'REMOVED' and not Path(row['path']).exists()
    for row in binding['artifacts']:
        assert not Path(row['path']).exists()
        assert any(Path(row['path']).is_relative_to(Path(root['path'])) for root in cleanup['deleted'])
    assert cleanup['persistentSshAuthorization'] == cleanup['persistentPrivateKey'] == cleanup['sourceWorktree'] == 'UNTOUCHED'
    assert cleanup['retainedDevelopmentZip']['sha256'] == facts['modernDevelopmentPackage']['sha256']
    assert tail['source460UnchangedFromTestedArtifactObservation'] is True
    assert tail['githubSync'] == tail['msvcCi'] == 'NOT_YET_RUN' and tail['fullGoal'] == 'ACTIVE_NOT_COMPLETE'
    for check in tail['checks']:
        assert check['exitCode'] == 0
        blob(check['stdout']); blob(check['stderr'])
    attributes = tail['additionalGitAttributeSource']
    data = Path(attributes['path']).read_bytes()
    assert len(data) == attributes['bytes'] and sha(data) == attributes['sha256']
    assert set(extras['files']) == {'build-budget.json', 'THIRD-PARTY-NOTICES.txt'}
    for row in extras['files'].values():
        blob(row)
    facts.update({'finalInputObservation': {'inputCount': 460, 'canonicalSha256': binding['canonicalSha256'],
                                            'oldCompilerCaptureLimitsPreserved': True,
                                            'scope': binding['scope'], 'current460FilesActuallyRehashed': True},
                  'artifactDeletionBinding': {'originalObservedArtifacts': 42, 'compiledFrozenIdentitiesMatched': 42,
                                               'absentArtifactPathsNotRehashedAfterDeletion': True,
                                               'modernExeBytesActuallyVerifiedInsideRetainedZip': True},
                  'localCleanup': cleanup, 'tailChecks': tail,
                  'webExtraOriginals': {'files': list(extras['files']), 'base64BytesAndShaVerified': True}})
    return facts


def verify_index(index):
    assert len({row['path'] for row in index['files']}) == len(index['files'])
    for row in index['files']:
        data = (DST / row['path']).read_bytes()
        assert len(data) == row['bytes'] and sha(data) == row['sha256']


def execute(args):
    assert args.terminal_confirmed and args.allowlist
    allowlist_path = path_for(args.allowlist)
    manifest = json.loads(data_for(allowlist_path))
    assert manifest['allOriginalWritersTerminal'] is True
    selected = collect(manifest)
    facts = validate_terminal(manifest, selected)
    index_path = DST / (PREFIX + 'stage-progress-index.json')
    original = index_path.read_bytes()
    index = json.loads(original)
    assert len(index['files']) == PRIOR_COUNT and sha(original) == PRIOR_SHA
    verify_index(index)
    prior_rows = list(index['files'])
    existing = {row['path']: row for row in prior_rows}
    payloads = {}
    for path in sorted(selected):
        data = data_for(path)
        destination = DST / (PREFIX + str(path.relative_to(SRC)))
        assert not destination.exists() or destination.read_bytes() == data
        payloads[path] = data
    summary = {'schemaVersion': 1, 'scope': 'Round30 closeout evidence only, not all project requirements or a release/CI/MSVC/three-OS acceptance.',
               'preservation': 'Original reports, stdout/stderr, raw hidden files and original JSON paths unchanged. Separate mapping resolves identical archived bytes.',
               'priorIndex': {'files': len(prior_rows), 'sha256': sha(original)}, 'facts': facts,
               'repositoryPromotion': manifest['repositoryPromotion'],
               'remainingBoundaries': ['Persistent production listener and strict first TCP five-second requirement remain unaccepted.',
                                        'Formal Windows7/Windows10/MSVC, visible Seewo/PPT/touch/DPI, installation lifecycle and long-run gates remain unaccepted.',
                                        'Git synchronization and exact-SHA MSVC CI have not yet actually occurred; only subsequent original evidence can establish them.']}
    summary_path = path_for(SUMMARY)
    with summary_path.open('x') as output:
        output.write(json.dumps(summary, ensure_ascii=False, indent=2) + '\n')
    payloads[summary_path] = data_for(summary_path)
    mappings = []
    added = []

    def copy(path, data):
        relative = PREFIX + str(path.relative_to(SRC))
        target = DST / relative
        row = {'path': relative, 'bytes': len(data), 'sha256': sha(data)}
        assert path.read_bytes() == data
        if target.exists():
            assert target.read_bytes() == data
        else:
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(path, target)
        assert target.read_bytes() == data
        if relative in existing:
            assert existing[relative] == row
        else:
            index['files'].append(row)
            existing[relative] = row
            added.append(relative)
        return {'original': str(path), 'archived': str(Path('开发计划/验证记录') / relative), 'bytes': row['bytes'], 'sha256': row['sha256']}

    for path, data in sorted(payloads.items()):
        mappings.append(copy(path, data))
    mapping_path = path_for(MAPPING)
    with mapping_path.open('x') as output:
        output.write(json.dumps({'schemaVersion': 1, 'files': mappings, 'mappingSelf': 'Separately indexed; no recursive self-hash.'}, ensure_ascii=False, indent=2) + '\n')
    copy(mapping_path, data_for(mapping_path))
    assert index['files'][:PRIOR_COUNT] == prior_rows and index_path.read_bytes() == original
    index_path.write_text(json.dumps(index, ensure_ascii=False, indent=2) + '\n')
    verify_index(index)
    print(json.dumps({'status': 'ARCHIVE_BYTES_SHA_VERIFIED', 'priorFiles': PRIOR_COUNT, 'addedFiles': len(added),
                      'indexedFiles': len(index['files']), 'indexedBytes': sum(row['bytes'] for row in index['files']),
                      'indexSha256': sha(index_path.read_bytes()),
                      'manifestSha256': sha(json.dumps(index['files'], ensure_ascii=False, sort_keys=True, separators=(',', ':')).encode()),
                      'summary': str(DST / (PREFIX + SUMMARY)), 'mapping': str(DST / (PREFIX + MAPPING))}, ensure_ascii=False, indent=2))


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--execute', action='store_true')
    parser.add_argument('--terminal-confirmed', action='store_true')
    parser.add_argument('--allowlist')
    args = parser.parse_args()
    if args.execute:
        execute(args)
    else:
        print('PREPARED_ONLY: await actual local-cleanup/final-check binding; no copy/index IO')
