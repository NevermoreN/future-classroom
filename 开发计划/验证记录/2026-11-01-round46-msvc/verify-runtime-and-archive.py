#!/usr/bin/env python3
"""Local-only R46 original artifact/runtime audit. Never invokes programs/SSH."""
import argparse
import binascii
import copy
import datetime
import hashlib
import json
from pathlib import Path, PurePosixPath
import re
import stat
import struct
import sys
import zipfile

EV = Path('/tmp/fc-round46-ci-evidence')
SOURCE = 'e23bd538d828aeed4378a88f69cf81cb85a4389f'
MAX_MEMBER = 128 * 1024 * 1024
MAX_TOTAL = 512 * 1024 * 1024
SUMMARY = re.compile(r'^test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out; finished in ([0-9.]+)s$', re.M)

def require(value, message):
    if not value:
        raise ValueError(message)

def digest(path):
    h = hashlib.sha256()
    with Path(path).open('rb') as f:
        for block in iter(lambda: f.read(1024 * 1024), b''):
            h.update(block)
    return h.hexdigest()

def fact(path):
    p = Path(path)
    require(p.is_file() and not p.is_symlink(), 'required regular evidence file missing')
    return {'path': str(p), 'bytes': p.stat().st_size, 'sha256': digest(p)}

def same_file(path, expected):
    actual = fact(path)
    require(actual['bytes'] == expected['bytes'] and actual['sha256'] == expected['sha256'], 'evidence bytes/SHA mismatch: ' + Path(path).name)
    return actual

def read_json(path):
    return json.loads(Path(path).read_text(encoding='utf-8'))

def safe_name(name):
    p = PurePosixPath(name)
    require(bool(name) and not name.endswith('/') and '\\' not in name and ':' not in name and not p.is_absolute() and all(v not in ('', '.', '..') for v in name.split('/')), 'unsafe ZIP member')

def validate_summary(result):
    require(type(result.get('exitCode')) is int and result['exitCode'] == 0, 'libtest exit is not zero')
    require(result.get('timeoutSeconds') == 60, 'original per-test 60s budget changed')
    output = result.get('output')
    require(isinstance(output, list) and all(isinstance(v, str) for v in output), 'libtest output missing or malformed')
    text = '\n'.join(output)
    matches = list(SUMMARY.finditer(text))
    require(len(matches) == 1, 'expected exactly one complete libtest summary')
    passed, failed, ignored, measured, filtered = map(int, matches[0].groups()[:5])
    require((failed, ignored, measured, filtered) == (0, 0, 0, 0), 'libtest has failed/ignored/measured/filtered tests')
    starts = re.findall(r'^running (\d+) tests?$', text, re.M)
    require(starts == [str(passed)], 'libtest announced count disagrees with summary')
    require(not re.search(r'^test .* \.\.\. (?:FAILED|ignored)\s*$', text, re.M), 'libtest output contains failed/ignored case')
    return {'passed': passed, 'failed': failed, 'ignored': ignored, 'filtered': filtered, 'outputUtf8Bytes': len(text.encode()), 'outputUtf8Sha256': hashlib.sha256(text.encode()).hexdigest(), 'summary': matches[0].group(0)}

def validate_runtime(audit, report, command, expected_tests, metadata):
    require(command.get('status') == 'PASS_COMMAND' and command.get('exitCode') == 0 and command.get('actualExit') is True and command.get('scriptUnchanged') is True, 'runtime command not actually successful and exited')
    require(command.get('sourceSha') == SOURCE and command.get('inputNormalSha256') == audit['normalExecutable']['producer']['sha256'] and command.get('metadataSha256') == metadata['sha256'], 'runtime command source/program/metadata mismatch')
    require(report.get('status') == 'PASS' and report.get('cleanup') == 'PASS' and report.get('win7') == 'NOT_VALIDATED', 'runtime/cleanup not PASS or OS scope upgraded')
    require(report.get('testTimeoutSeconds') == 60 and 'timedOutTest' not in report and 'error' not in report, 'runtime budget/error/timeout mismatch')
    require(report.get('exeSha256') == audit['normalExecutable']['producer']['sha256'], 'normal runtime EXE SHA mismatch')
    platform = report.get('platform', {})
    require(isinstance(platform.get('caption'), str) and 'Windows 11' in platform['caption'] and str(platform.get('build', '')).isdigit() and int(platform['build']) >= 22000, 'actual platform is not expected Win11')
    tests = report.get('tests')
    require(isinstance(tests, list) and len(tests) == 21, 'runtime must contain all 21 test executables')
    require([v.get('name') for v in tests] == list(expected_tests), 'runtime test order/names differ from exact metadata')
    upload = report.get('uploadedArtifacts')
    require(isinstance(upload, list) and len(upload) == 22 and len({v.get('name') for v in upload}) == 22, 'uploaded22 identity ledger malformed')
    expected_upload = {Path(audit['normalExecutable']['localPath']).name: audit['normalExecutable']['producer']['sha256'], **{k: v['sha256'] for k, v in expected_tests.items()}}
    require({v.get('name'): v.get('sha256') for v in upload} == expected_upload, 'uploaded artifacts differ from original artifact22')
    summaries = []
    for test in tests:
        expected = expected_tests[test['name']]
        require(test.get('binarySha256') == expected['sha256'], 'executed test SHA differs from original artifact')
        summaries.append({'name': test['name'], 'sha256': test['binarySha256'], **validate_summary(test)})
    for key in ('selfCheck', 'system'):
        result = report.get(key)
        require(isinstance(result, dict) and result.get('exitCode') == 0 and isinstance(result.get('output'), list) and all(isinstance(v, str) for v in result['output']), 'normal self/system probe incomplete')
    native = report.get('native', {})
    require(native.get('exitCode') == 0 and native.get('hostExitCode') == 0 and type(native.get('sessionId')) is int and native['sessionId'] > 0, 'hidden native/host probes did not actually pass in interactive session')
    require(isinstance(native.get('output'), list) and isinstance(native.get('hostOutput'), list), 'hidden native/host outputs missing')
    return {'status': 'PASS_ACTUAL_WIN11_ONLY', 'testExecutables': 21, 'passedTests': sum(x['passed'] for x in summaries), 'tests': summaries, 'platform': platform, 'cleanup': 'PASS', 'formalWin7Os': 'NOT_VALIDATED', 'normalSha256': report['exeSha256']}

def audit_original(tag):
    audit_path = EV / f'{tag}-original-independent-first.json'
    a = read_json(audit_path)
    require(a['status'] == 'PASS_STATIC_ONLY_RUNTIME_NOT_RUN' and a['sourceSha'] == SOURCE and a['runtime'] == 'NOT_RUN', 'original independent audit source/status mismatch')
    require(a['artifactIntegrity'] == 'PASS_WHOLE_ZIP_CRC_AND_COMPLETE_PRODUCER_LEDGER' and not a['phaseFailures'] and not a['ciCommandFailures'] and a['helperUnchanged'] is True, 'original static audit gates failed')
    require(a['counts']['executables'] == 22 and a['counts']['tests'] == 21 and a['counts']['product'] == 1, 'original EXE counts changed')
    require(a['originalCleanup']['status'] == 'OWNED_BUILD_ROOTS_REMOVED' and all(v['removed'] is True for v in a['originalCleanup']['roots']), 'CI original cleanup failed')
    require(all(v['exit_code'] == 0 and v['passed'] is True and v['parent_exited'] is True and v['timed_out_or_pipe_stalled'] is False for v in a['originalCommands']), 'original compile commands failed')
    zfact = same_file(a['zip']['path'], a['zip'])
    root = EV / f'{tag}-artifact-original'
    expected = {v['path']: v for v in a['members']}
    require(len(expected) == len(a['members']), 'duplicate audited ZIP path')
    exes = {v['producer']['relative_path']: v for v in a['executables']}
    require(len(exes) == 22 and len({Path(v['localPath']).name.casefold() for v in exes.values()}) == 22, '22 EXE identities/basenames not unique')
    mappings, binary_facts = [], []
    with zipfile.ZipFile(a['zip']['path']) as z:
        entries = z.infolist()
        require(len(entries) == len(expected) and len({v.filename.casefold() for v in entries}) == len(entries), 'ZIP counts/case duplicate mismatch')
        require(sum(v.file_size for v in entries) <= MAX_TOTAL, 'ZIP logical bytes exceed bounded original audit')
        for entry in entries:
            safe_name(entry.filename)
            require(not stat.S_ISLNK(entry.external_attr >> 16) and entry.file_size <= MAX_MEMBER and entry.filename in expected, 'ZIP member bounds/type mismatch')
            h = hashlib.sha256(); crc = 0; length = 0
            with z.open(entry) as f:
                for block in iter(lambda: f.read(1024 * 1024), b''):
                    h.update(block); crc = binascii.crc32(block, crc); length += len(block)
            seen = {'bytes': length, 'sha256': h.hexdigest(), 'crc32': f'{crc & 0xffffffff:08x}'}
            wanted = expected[entry.filename]
            require(all(seen[k] == wanted[k] for k in seen) and crc & 0xffffffff == entry.CRC, 'ZIP whole member CRC/SHA differs from audit')
            if entry.filename in exes:
                item = exes[entry.filename]
                f = same_file(item['localPath'], item['producer'])
                require(f['sha256'] == seen['sha256'] and f['bytes'] == seen['bytes'], 'localEXE != ZIP original')
                with Path(item['localPath']).open('rb') as p:
                    header = p.read(4096)
                require(header[:2] == b'MZ' and len(header) >= 64, 'EXE DOS header missing')
                off = struct.unpack_from('<I', header, 60)[0]
                require(off + 26 <= len(header) and header[off:off + 4] == b'PE\0\0' and struct.unpack_from('<H', header, off + 4)[0] == 0x14c and struct.unpack_from('<H', header, off + 24)[0] == 0x10b, 'EXE not actual PE32/i386')
                require(not item['independentPe']['structural_errors'] and not any(re.fullmatch(r'(?:vcruntime\d+(?:_\d+)?|msvcp\d+(?:_\d+)?|ucrtbase|msvcrt|api-ms-win-crt-.*)\.dll', dll, re.I) for dll in item['independentPe']['imports']), 'original PE imports/structure gate no longer valid')
                inspection = same_file(item['rawInspection']['path'], item['rawInspection'])
                binary_facts.append({'zipMember': entry.filename, **f, 'originalZip': zfact['sha256'], 'crc32': seen['crc32'], 'objdumpOriginal': inspection, 'executedByThisHelper': False})
            else:
                local = same_file(root / 'raw' / entry.filename, seen)
                mappings.append({'zipMember': entry.filename, 'zipSha256': zfact['sha256'], 'crc32': seen['crc32'], 'rawFile': local, 'archiveRelativePath': f'{tag}/original-artifact/{entry.filename}'})
    ledger = read_json(root / 'raw/artifact-sha256.json')
    require(isinstance(ledger, list) and len(ledger) == len(expected) - 1 and len({v['path'] for v in ledger}) == len(ledger), 'producer all-member ledger count/duplicates changed')
    require({v['path'] for v in ledger} == set(expected) - {'artifact-sha256.json'}, 'producer ledger coverage mismatch')
    require(all(v['bytes'] == expected[v['path']]['bytes'] and v['sha256'] == expected[v['path']]['sha256'] for v in ledger), 'producer ledger disagrees with full ZIP audit')
    metadata = same_file(a['testMetadata']['path'], a['testMetadata'])
    rows = [json.loads(line) for line in Path(metadata['path']).read_text().splitlines()]
    require(len(rows) == 21 and all(v.get('reason') == 'compiler-artifact' and v.get('profile', {}).get('test') is True for v in rows), 'metadata test21 exact selection mismatch')
    tests = {}
    for row in rows:
        path = row['executable']; matching = [v for v in exes.values() if v['localPath'] == path and v['producer']['role'] == 'tests']
        require(len(matching) == 1 and Path(path).name not in tests, 'metadata not bijective with original tests')
        tests[Path(path).name] = matching[0]['producer']
    require(len(tests) == 21, '21 unique tests missing')
    # Original auditor retained raw per-EXE objdump stdout, but version output
    # only has a digest/first line in its JSON. Do not invent a raw version file.
    derived = [metadata, fact(audit_path)] + [v['objdumpOriginal'] for v in binary_facts]
    return a, tests, metadata, {'tag': tag, 'status': 'PASS_ORIGINAL_STATIC_BINDING_ONLY', 'sourceSha': SOURCE, 'zip': zfact, 'members': len(expected), 'nonbinaryOriginalMappings': mappings, 'binariesNotForGit': binary_facts, 'derivedNonbinaryEvidence': derived, 'originalObjdumpVersionScope': 'Original audit retains version first line/bytes/SHA only; complete raw version stdout was not retained separately and is not reconstructed here.', 'archiveScope': 'Only nonbinary raw originals and derived text/JSON/objdump. EXEs/ZIP binaries stay outside Git; no new archive or extraction performed.'}

def model_tests():
    result = {'exitCode': 0, 'timeoutSeconds': 60, 'output': ['running 2 tests', 'test one ... ok', 'test two ... ok', '', 'test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s']}
    require(validate_summary(result)['passed'] == 2, 'valid summary model rejected')
    cases = []
    for label, transform in [
        ('nonzero_exit', lambda v: v.update(exitCode=1)),
        ('changed_budget', lambda v: v.update(timeoutSeconds=61)),
        ('missing_output', lambda v: v.pop('output')),
        ('missing_summary', lambda v: v['output'].pop()),
        ('duplicate_summary', lambda v: v['output'].append(v['output'][-1])),
        ('failed', lambda v: v['output'].__setitem__(-1, v['output'][-1].replace('0 failed', '1 failed'))),
        ('ignored', lambda v: v['output'].__setitem__(-1, v['output'][-1].replace('0 ignored', '1 ignored'))),
        ('filtered', lambda v: v['output'].__setitem__(-1, v['output'][-1].replace('0 filtered', '1 filtered'))),
        ('wrong_announced_count', lambda v: v['output'].__setitem__(0, 'running 3 tests')),
    ]:
        bad = copy.deepcopy(result); transform(bad)
        try: validate_summary(bad)
        except ValueError: cases.append({'case': label, 'rejected': True})
        else: raise ValueError('negative summary model accepted: ' + label)
    return {'status': 'PASS_LOCAL_PURE_SUMMARY_MODELS_ONLY', 'positive': 1, 'negative': cases, 'scope': 'No runtime/artifact/native/Cargo execution; pure malformed-summary decisions only.'}

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--report', required=True, type=Path)
    parser.add_argument('--prepare-only', action='store_true')
    parser.add_argument('--model-selftest', action='store_true')
    args = parser.parse_args()
    require(not args.report.exists(), 'fresh report required; overwrite forbidden')
    report = {'status': 'FAIL', 'scope': 'Read-only local evidence verification; no executable, SSH, Cargo, download, CI or Git writes.', 'helper': fact(Path(__file__)), 'createdUtc': datetime.datetime.now(datetime.timezone.utc).isoformat()}
    try:
        if args.model_selftest:
            report.update(model_tests())
        else:
            results = []
            for tag in ('modern', 'win7'):
                audit, tests, metadata, original = audit_original(tag)
                if args.prepare_only:
                    original['runtime'] = 'NOT_REVIEWED_ACTIVE_OR_PENDING'
                else:
                    runtime_path = EV / f'{tag}-msvc-runtime-first.json'
                    command_path = EV / f'{tag}-msvc-runtime-first.command.json'
                    runtime = read_json(runtime_path); command = read_json(command_path)
                    original['runtime'] = validate_runtime(audit, runtime, command, tests, metadata)
                    original['runtimeReportOriginal'] = fact(runtime_path)
                    original['runtimeCommandOriginal'] = fact(command_path)
                    original['runtimeStdout'] = same_file(EV / f'{tag}-msvc-runtime-first.stdout.log', command['stdout'])
                    original['runtimeStderr'] = same_file(EV / f'{tag}-msvc-runtime-first.stderr.log', command['stderr'])
                results.append(original)
            report.update(status='PREPARED_RUNTIME_PENDING' if args.prepare_only else 'PASS_BOTH_ORIGINAL_ARTIFACTS_AND_ACTUAL_WIN11_RUNTIME', suites=results, limitations=['Win7-target EXEs on Win11 are Win11 supplemental execution, not actual Win7 SP1 proof.', 'No actual Win10/visible Seewo/72h/production TCP+SSPI gate upgrade.', 'No cleanup or archive operation is performed by this helper; active Windows roots are never touched.'])
    except Exception as error:
        # All messages are bounded local labels/names, never signed URLs/keys.
        report['error'] = {'type': type(error).__name__, 'message': str(error)[:1000]}
    report['helperUnchanged'] = digest(Path(__file__)) == report['helper']['sha256']
    with args.report.open('x', encoding='utf-8') as f:
        json.dump(report, f, ensure_ascii=False, indent=2); f.write('\n')
    print(json.dumps({'status': report['status'], 'report': str(args.report), 'reportSha256': digest(args.report)}, ensure_ascii=False))
    return 0 if report['status'].startswith(('PASS_', 'PREPARED_')) and report['helperUnchanged'] else 1

if __name__ == '__main__':
    sys.exit(main())
