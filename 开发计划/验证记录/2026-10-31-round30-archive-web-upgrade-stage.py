"""Archive completed pre-HTTP-correction stage evidence without rewriting originals."""
from pathlib import Path
import base64
import hashlib
import json
import re
import shutil

SRC = Path('/tmp/fc-round30-evidence')
DST = Path.cwd() / '开发计划/验证记录'
PREFIX = '2026-10-31-round30-'
INDEX = DST / (PREFIX + 'stage-progress-index.json')
SUMMARY_NAME = 'web-upgrade-stage-summary.json'
MAPPING_NAME = 'web-upgrade-stage-path-mapping.json'
PREFIXES = ('web-upgrade-', 'browser-upgrade-', 'windows-edge-cache-',
            'windows-web-upgrade-', 'windows-runtime-fixture-', 'root-host-workspace-static-')
EXPLICIT = {'runtime-fixture-fmt-first.log', 'core-web-version-refresh-source-freeze.json',
            'windows-edge-read-only-inventory.json', 'prepare-upgrade-browser-fixture.py',
            'windows-edge-cache-owner.py'}


def sha(data):
    return hashlib.sha256(data).hexdigest()


def read(name):
    return json.loads((SRC / name).read_text())


def archive_name(path):
    return PREFIX + str(path.relative_to(SRC))


def safe_file(path):
    assert path.is_file() and not path.is_symlink(), str(path)
    assert path.suffix.lower() in {'.json', '.jsonl', '.log', '.js', '.py', '.pem', '.raw', '.stdout', '.stderr'}, str(path)
    assert not path.name.endswith('.control.json') and '.active' not in path.name.lower(), str(path)
    data = path.read_bytes()
    assert not re.search(rb'(?m)^-----BEGIN (?:RSA |EC |OPENSSH |ENCRYPTED )?PRIVATE KEY-----', data), str(path)
    return data


original_index = INDEX.read_bytes()
index = json.loads(original_index)
existing = {row['path']: row for row in index['files']}
assert len(existing) == len(index['files'])
for row in existing.values():
    data = (DST / row['path']).read_bytes()
    assert len(data) == row['bytes'] and sha(data) == row['sha256'], row['path']
prior_count = len(existing)
prior_bytes = sum(row['bytes'] for row in existing.values())

# Completed, independently recorded retirement makes old fixture control tokens
# historical evidence only. No still-active .control.json file is archived.
for name in ('browser-upgrade-fixture-cleanup.json', 'browser-upgrade-edge-fixture-cleanup.json'):
    assert read(name)['status'] == 'PASS'
assert read('browser-upgrade-fixture-cleanup.json')['ownedOriginalProcessExited'] is True
assert read('browser-upgrade-edge-fixture-cleanup.json')['originalFixtureProcessExited'] is True
assert read('browser-upgrade-edge-fixture-cleanup.json')['localStopCapabilityDeleted'] is True
assert read('browser-upgrade-edge-fixture-address-verification.json')['status'] == 'PASS'

selected = set()
excluded = []
for path in SRC.iterdir():
    if not (path.name.startswith(PREFIXES) or path.name in EXPLICIT):
        continue
    if path.name in (SUMMARY_NAME, MAPPING_NAME):
        continue
    if path.is_dir():
        assert path.name.endswith('-raw') and not path.is_symlink(), str(path)
        for child in path.rglob('*'):
            if child.is_file():
                selected.add(child)
            else:
                assert child.is_dir() and not child.is_symlink(), str(child)
    elif path.name.endswith('.control.json') or '.active' in path.name.lower():
        excluded.append({'original': str(path), 'reason': 'active control capability is not archival evidence'})
    else:
        selected.add(path)
# Preserve the archive procedure itself as evidence, without editing repo code.
selected.add(Path(__file__).resolve())

raw_counts = {}
for name in ('windows-edge-cache-owner-first.json', 'windows-web-upgrade-tls-first.json'):
    report = read(name)
    assert report['status'] == report['cleanup'] == 'PASS'
    raw = Path(report['rawPreservation']['directory'])
    assert raw.parent == SRC and raw.is_dir()
    files = [file for file in raw.rglob('*') if file.is_file()]
    assert set(files).issubset(selected)
    for leaf, expected in report['remoteRawEvidence'].items():
        data = (raw / leaf).read_bytes()
        assert len(data) == expected['bytes'] and sha(data) == expected['sha256'], leaf
    for leaf, preserved in report['rawPreservation']['files'].items():
        assert preserved['status'] == 'PASS'
        data = (raw / leaf).read_bytes()
        assert base64.b64decode(preserved['base64'], validate=True) == data, leaf
    raw_counts[str(raw.relative_to(SRC))] = {
        'files': len(files), 'dotfiles': sum(file.name.startswith('.') for file in files),
        'originalRemoteInventoryEntries': len(report['remoteRawEvidence']),
        'allFilesIncludingUnlistedMetadataPreserved': True,
    }

web = read('web-upgrade-first.json')
vitest = read('web-upgrade-first-vitest.json')
chromium = read('browser-upgrade-second-result.json')
edge = read('windows-edge-cache-first-result.json')
native = read('windows-web-upgrade-tls-first.json')
host_first = read('root-host-workspace-static-first.json')
host_second = read('root-host-workspace-static-second.json')
runtime = read('windows-runtime-fixture-stage-first.json')
http_events = [event for event in native['events'] if 'httpStatus' in event]
assert native['runnerSourceSha256'] == sha((SRC / 'web-upgrade-tls-runner-tested.py').read_bytes())
assert chromium['checkCount'] == len(chromium['checks']) == 33 and len(chromium['cases']) == 6
assert edge['checkCount'] == len(edge['checks']) == 13 and len(edge['cases']) == 2
assert len(http_events) == 35 and len(native['events']) == 37
assert all(event['status'] == 'PASS' for event in native['events'])
assert host_second['totalPassed'] == sum(group['passed'] for group in host_second['testGroups']) == 837
runtime_results = [line for test in runtime['tests'] for line in test['output'] if 'test result:' in line]
assert len(runtime_results) == 1 and re.search(r'\b33 passed; 0 failed;', runtime_results[0])
summary = {
    'schemaVersion': 1,
    'scope': 'Completed Web version-refresh/retired-assets stage evidence, BEFORE subsequent HTTP 304/Date/Allow corrections. Not final Round30 source or full-goal acceptance.',
    'originalReports': 'Original JSON/log/JS/Python/PEM bytes are unchanged, including absolute artifact paths, failures and all raw dotfiles; use the separate original-to-archive mapping.',
    'index': str(INDEX.relative_to(Path.cwd())),
    'priorIndexVerified': {'files': prior_count, 'bytes': prior_bytes, 'sha256BeforeAppend': sha(original_index)},
    'web': {
        'report': archive_name(SRC / 'web-upgrade-first.json'),
        'commands': [{'command': command['command'], 'exitCode': command['exitCode']} for command in web['commands']],
        'changedInputs': web['changedInputs'], 'generatedNoticesMatch': web['generatedNoticesMatch'],
        'vitest': {key: vitest[key] for key in ('success', 'numTotalTests', 'numPassedTests', 'numFailedTests', 'numTotalTestSuites', 'numPassedTestSuites')},
        'buildBudget': web['buildBudget'],
    },
    'chromiumUpgrade': {
        'report': archive_name(SRC / 'browser-upgrade-second-result.json'),
        'status': chromium['status'], 'browserVersion': chromium['browserVersion'],
        'checks': chromium['checkCount'], 'cases': [case['case'] for case in chromium['cases']],
        'originalScope': chromium['scope'],
        'limits': 'Six cases/33 checks use intercepted API routes; browser HTTP cache is disabled by that routed harness. This is DOM/resource-failure recovery and synthetic capture evidence, not genuine browser-cache acceptance or Rust-native browser TLS.',
    },
    'windowsEdgeCache': {
        'report': archive_name(SRC / 'windows-edge-cache-first-result.json'),
        'ownerReport': archive_name(SRC / 'windows-edge-cache-owner-first.json'),
        'status': edge['status'], 'browserVersion': edge['browserVersion'], 'checks': edge['checkCount'],
        'cases': [case['case'] for case in edge['cases']], 'originalScope': edge['scope'],
        'limits': 'Two cases/13 checks use actual Windows11 Edge HTTP cache, no request routing/cache disabling, synthetic API and Python static HTTPS fixture. The archived Python fixture emitted Content-Length: 0 on 304; these historical browser results DO NOT validate the later corrected RFC response encoder or Rust-native browser TLS.',
        'fixture304Source': [archive_name(SRC / 'browser-upgrade-server.py'), archive_name(SRC / 'browser-upgrade-server-stage1.py')],
    },
    'nativeTlsHistoricalContract': {
        'report': archive_name(SRC / 'windows-web-upgrade-tls-first.json'),
        'status': native['status'], 'cleanup': native['cleanup'], 'httpResponses': len(http_events),
        'certificateNegativeCases': len(native['events']) - len(http_events),
        'conditionalResponses': [{'case': event['case'], 'status': event['httpStatus'], 'bodyBytes': event['responseBodyBytes']} for event in http_events if event['httpStatus'] == 304],
        'exeSha256': native['exeSha256'], 'runnerSourceSha256': native['runnerSourceSha256'],
        'archivedTestedRunner': archive_name(SRC / 'web-upgrade-tls-runner-tested.py'),
        'native': native['native'],
        'limits': 'This original 35-HTTP + two certificate-negative PASS used the old mandatory-Content-Length diagnostic: its two 304 responses were accepted under CL:0. Preserve it as OLD CONTRACT evidence only; it is not evidence for the subsequent omitted-CL 304/204, Date or Allow corrections.',
        'originalBoundaries': {key: native[key] for key in ('strictP0', 'productionListener', 'browserAcceptance', 'win7', 'win10', 'physicalPlayback')},
    },
    'hostWorkspace': {
        'firstReport': archive_name(SRC / 'root-host-workspace-static-first.json'),
        'firstTotalPassed': host_first['totalPassed'],
        'firstFailedGroups': [group for group in host_first['testGroups'] if group['failed']],
        'firstFailure': 'Original run log records worker::tests::deadline_output_drop_retains_running_slot_and_delays_terminal_result: unexpected held-output rejection Busy; original exit 101 remains preserved.',
        'secondReport': archive_name(SRC / 'root-host-workspace-static-second.json'),
        'secondTotalPassed': host_second['totalPassed'],
        'secondCommands': [{'name': command['name'], 'exitCode': command['exitCode']} for command in host_second['commands']],
        'changedInputs': host_second['changedInputs'],
        'limits': '837 Host tests and this stage clippy passed before the new 304/Date/Allow edits; these are not final-current-source validation or Windows runtime acceptance.',
    },
    'windowsRuntime': {
        'report': archive_name(SRC / 'windows-runtime-fixture-stage-first.json'),
        'status': runtime['status'], 'cleanup': runtime['cleanup'], 'platform': runtime['platform'],
        'testResults': runtime_results, 'uploadedArtifacts': runtime['uploadedArtifacts'],
        'selfCheck': runtime['selfCheck'], 'native': runtime['native'],
        'limits': '33 actual runtime tests were run on Windows11 build 26300 with the captured prior artifacts, before new 304/Date/Allow edits. Offscreen native lifecycle is not visible desktop evidence. Win7/Win10 and physical Seewo validation remain separate.',
    },
    'rawDirectories': raw_counts,
    'failedAttempts': {
        'browserFirst': {'report': archive_name(SRC / 'browser-upgrade-first-result.json'), 'status': read('browser-upgrade-first-result.json')['status'], 'error': read('browser-upgrade-first-result.json')['error']},
        'browserCacheAttempt': {'report': archive_name(SRC / 'browser-upgrade-cache-first-result.json'), 'status': read('browser-upgrade-cache-first-result.json')['status'], 'error': read('browser-upgrade-cache-first-result.json')['error']},
        'preservation': 'All selected first/second/third tool outputs, logs, compile diagnostics and raw files are copied completely, without replacing a failed attempt by a later PASS.',
    },
    'cleanupEvidence': {
        'originalReports': [archive_name(SRC / name) for name in (
            'browser-upgrade-browser-cleanup.json', 'browser-upgrade-fixture-cleanup.json',
            'browser-upgrade-edge-fixture-cleanup.json', 'browser-upgrade-edge-fixture-address-verification.json',
            'windows-edge-cache-all-temp-and-ports-final.json', 'windows-web-upgrade-and-runtime-final-after.json',
            'web-upgrade-local-build-cleanup.json')],
        'archiveOperation': 'Local copy/hash/index only; no process, service, port, certificate, SSH authorization or source evidence was mutated or deleted.',
        'historicalControlMetadata': 'Original synthetic fixture control tokens/PID/CDP metadata are archived verbatim only after original server/root/ports and owned browser job retirement were recorded. They are expired test capabilities, not persistent credentials. No active control capability file is archived.',
        'excludedPayloads': 'No private key, active control, ZIP/package, executable/build output, verified-dist payload, node_modules, npm/browser cache or user data is copied.',
        'retainedOutsideRepository': 'All original /tmp evidence remains for this continuing round; only the reproducible archive helper and summary/mapping were generated here. The verified dist remains a separately reported staging resource, not an archived artifact.',
        'persistentWindowsSsh': 'Preserved; refer to original final-after/cleanup reports, no new remote audit was run by this archiver.',
    },
    'excludedCandidates': excluded,
}
summary_source = SRC / SUMMARY_NAME
summary_source.write_text(json.dumps(summary, ensure_ascii=False, indent=2) + '\n')
selected.add(summary_source)

mappings = []
added = []
for path in sorted(selected):
    data = safe_file(path)
    relative = archive_name(path)
    target = DST / relative
    row = {'path': relative, 'bytes': len(data), 'sha256': sha(data)}
    if target.exists():
        assert target.read_bytes() == data, 'refuse differing existing archive: ' + relative
    else:
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(path, target)
    assert path.read_bytes() == target.read_bytes() == data
    if relative in existing:
        assert existing[relative] == row
    else:
        index['files'].append(row)
        existing[relative] = row
        added.append(relative)
    mappings.append({'original': str(path), 'archived': str(Path('开发计划/验证记录') / relative),
                     'bytes': len(data), 'sha256': sha(data)})

mapping = {'schemaVersion': 1, 'scope': 'Original paths remain unchanged inside all copied reports; this mapping resolves them to byte-identical archived files.',
           'files': mappings, 'rawDirectories': raw_counts,
           'mappingSelf': 'This mapping is separately byte/SHA-indexed; it does not recursively hash itself.'}
mapping_source = SRC / MAPPING_NAME
mapping_source.write_text(json.dumps(mapping, ensure_ascii=False, indent=2) + '\n')
data = safe_file(mapping_source)
relative = archive_name(mapping_source)
row = {'path': relative, 'bytes': len(data), 'sha256': sha(data)}
target = DST / relative
if target.exists():
    assert target.read_bytes() == data, 'refuse differing existing mapping'
else:
    shutil.copyfile(mapping_source, target)
assert target.read_bytes() == data
if relative in existing:
    assert existing[relative] == row
else:
    index['files'].append(row)
    existing[relative] = row
    added.append(relative)
assert INDEX.read_bytes() == original_index, 'index changed concurrently; do not overwrite'
INDEX.write_text(json.dumps(index, ensure_ascii=False, indent=2) + '\n')
for row in index['files']:
    data = (DST / row['path']).read_bytes()
    assert len(data) == row['bytes'] and sha(data) == row['sha256'], row['path']
for row in mappings:
    assert Path(row['original']).read_bytes() == Path(row['archived']).read_bytes()
print(json.dumps({'priorVerifiedFiles': prior_count, 'addedFiles': len(added), 'indexedFiles': len(index['files']),
                  'indexedBytes': sum(row['bytes'] for row in index['files']),
                  'rawDirectories': raw_counts,
                  'summary': str(DST / archive_name(summary_source)),
                  'mapping': str(DST / archive_name(mapping_source)),
                  'originalEvidenceMutated': False}, ensure_ascii=False, indent=2))
