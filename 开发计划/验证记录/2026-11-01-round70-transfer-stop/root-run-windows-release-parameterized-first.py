"""Root-only finite R70 release harness; invoke --execute after both builds PASS.
Preparation/AST validation performs no Windows/SSH or build operation.
"""
from pathlib import Path
import argparse
import datetime
import hashlib
import json
import struct
import subprocess
import sys
import time

ROOT = Path('/tmp/fc-round70-ordinary-transfer-stop')
EVIDENCE = ROOT / 'evidence'
WORKTREE = ROOT / 'worktree'
MAIN = Path('/workspace/共享/Eliora/未来教室计划')
FREEZE = None
FREEZE_SHA = None
BUILD_REPORTS = None
SCRIPT = MAIN / 'scripts/windows-probe.py'
TARGET = ROOT / 'target/i686-pc-windows-gnu/release'
NORMAL = TARGET / 'fc-bootstrap.exe'
METADATA = DEST = RECORD = STDOUT = STDERR = None


def sha(path):
    digest = hashlib.sha256()
    with path.open('rb') as stream:
        while chunk := stream.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def identity(path):
    return {'path': str(path), 'bytes': path.stat().st_size, 'sha256': sha(path)}


def save(path, value, exclusive=False):
    with path.open('x' if exclusive else 'w', encoding='utf-8') as stream:
        json.dump(value, stream, ensure_ascii=False, indent=2)
        stream.write('\n')


def source_current(frozen):
    return all((WORKTREE / row['path']).is_file()
               and sha(WORKTREE / row['path']) == row['sha256']
               for row in frozen['workingFiles'])


def compile_report(path):
    name = path.stem
    require(path.parent.resolve() == EVIDENCE and path.suffix == '.json' and not path.is_symlink(), 'build report outside exact R70 evidence root')
    require(path.is_file(), name + ' actual terminal report not yet present')
    report = json.loads(path.read_text())
    require(report.get('status') == 'PASS' and report.get('exitCode') == 0,
            name + ' has not actually passed; RUNNING is not completion')
    require(report.get('sourceUnchanged') is True
            and report.get('sourceFreezeSha256') == FREEZE_SHA
            and report.get('workingFileCount') == 595,
            name + ' source/freeze provenance mismatch')
    require(report.get('elapsedSeconds', 901) <= 900
            and report.get('originalBudgetSeconds') == 900,
            name + ' original build budget exceeded/mismatched')
    for kind in ('stdout', 'stderr'):
        raw = EVIDENCE / (name + '.' + kind + '.log')
        require(raw.is_file() and raw.stat().st_size == report.get(kind + 'Bytes')
                and sha(raw) == report.get(kind + 'Sha256'),
                name + ' complete original ' + kind + ' identity mismatch')
    return {'report': identity(path), 'stdout': identity(EVIDENCE / (name + '.stdout.log')),
            'stderr': identity(EVIDENCE / (name + '.stderr.log')),
            'elapsedSeconds': report['elapsedSeconds']}


def compiler_artifacts(path):
    result = []
    with path.open('rb') as stream:
        for number, raw in enumerate(stream, 1):
            try:
                row = json.loads(raw)
            except (ValueError, UnicodeError):
                continue
            if row.get('reason') == 'compiler-artifact' and row.get('executable'):
                result.append((number, raw, row))
    return result


def artifact(number, raw, row, expected_subsystem):
    path = Path(row['executable'])
    require(path.is_absolute() and path.suffix.lower() == '.exe'
            and path.resolve().is_relative_to(TARGET.resolve()), 'artifact escaped exact R70 target')
    require(path.is_file() and str(path) in row.get('filenames', []),
            'actual Cargo artifact file missing or not named by its original producer')
    with path.open('rb') as stream:
        head = stream.read(64)
        require(head[:2] == b'MZ' and len(head) == 64, 'artifact is not PE')
        offset = struct.unpack_from('<I', head, 60)[0]
        stream.seek(offset)
        pe = stream.read(96)
    require(pe[:4] == b'PE\0\0' and struct.unpack_from('<H', pe, 4)[0] == 0x14c
            and struct.unpack_from('<H', pe, 24)[0] == 0x10b,
            'artifact is not actual i686 PE32')
    require(struct.unpack_from('<H', pe, 92)[0] == expected_subsystem,
            'normal/test subsystem mismatch')
    return {**identity(path), 'cargoMetadataLine': number,
            'cargoMetadataLineSha256': hashlib.sha256(raw).hexdigest(),
            'cargoTarget': row['target']['name'], 'cargoPackageId': row.get('package_id'),
            'cargoProfileTest': row['profile']['test'], 'peMachine': 'I386',
            'peMagic': 'PE32', 'peSubsystem': expected_subsystem}


def main():
    global FREEZE, FREEZE_SHA, BUILD_REPORTS, METADATA, DEST, RECORD, STDOUT, STDERR
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--execute', action='store_true')
    parser.add_argument('--freeze', type=Path, required=True)
    parser.add_argument('--freeze-sha256', required=True)
    parser.add_argument('--clippy-report', type=Path, required=True)
    parser.add_argument('--tests-report', type=Path, required=True)
    parser.add_argument('--product-report', type=Path, required=True)
    parser.add_argument('--label', default='windows-release-full-first')
    args = parser.parse_args()
    # No source/build/PE read before root's explicit execution switch.
    require(args.execute, 'Use --execute only after root observes the new frozen build reports PASS')
    require(args.label and all(c.isascii() and (c.isalnum() or c == '-') for c in args.label), 'fresh safe operation label')
    require(len(args.freeze_sha256) == 64 and all(c in '0123456789abcdef' for c in args.freeze_sha256), 'exact freeze SHA required')
    FREEZE = args.freeze.resolve()
    require(FREEZE.parent == EVIDENCE and not args.freeze.is_symlink(), 'freeze outside exact R70 evidence root')
    FREEZE_SHA = args.freeze_sha256
    BUILD_REPORTS = {'clippy': args.clippy_report.resolve(), 'tests': args.tests_report.resolve(), 'product': args.product_report.resolve()}
    require(len(set(BUILD_REPORTS.values())) == 3, 'three distinct actual build reports required')
    METADATA = BUILD_REPORTS['tests'].with_suffix('.stdout.log')
    DEST = EVIDENCE / (args.label + '.json')
    RECORD = EVIDENCE / (args.label + '-operation.json')
    STDOUT = EVIDENCE / (args.label + '.stdout.log')
    STDERR = EVIDENCE / (args.label + '.stderr.log')
    require((ROOT / '.owner').read_bytes() == b'future-classroom-owned-r70-ordinary-transfer-stop/v1\n', 'exact owned R70 root marker')
    require(not any(path.exists() for path in (RECORD, DEST, STDOUT, STDERR)),
            'first operation/raw/result already exists; preserve it and resume original operation')
    require(FREEZE.is_file() and sha(FREEZE) == FREEZE_SHA, 'R70 supplied frozen source identity mismatch')
    frozen = json.loads(FREEZE.read_text())
    require(len(frozen['workingFiles']) == 595 and source_current(frozen),
            'R70 exact 595 frozen working inputs changed')
    require(sha(SCRIPT) == sha(WORKTREE / 'scripts/windows-probe.py'),
            'main runner differs from frozen worktree input')
    builds = {kind: compile_report(path) for kind, path in BUILD_REPORTS.items()}
    require(builds['tests']['stdout']['sha256'] == sha(METADATA),
            'test metadata differs from actual producer stdout')
    tests = [(number, raw, row) for number, raw, row in compiler_artifacts(METADATA)
             if row.get('profile', {}).get('test') is True]
    require(len(tests) == 21 and len({row['executable'] for _, _, row in tests}) == 21,
            'expected actual 21 distinct harness artifacts from complete Cargo JSON')
    product_log = BUILD_REPORTS['product'].with_suffix('.stdout.log')
    products = [(number, raw, row) for number, raw, row in compiler_artifacts(product_log)
                if row.get('target', {}).get('name') == 'fc-bootstrap'
                and row.get('profile', {}).get('test') is False]
    require(len(products) == 1 and Path(products[0][2]['executable']) == NORMAL,
            'normal product must be exact actual non-test fc-bootstrap Cargo artifact')
    normal = artifact(*products[0], expected_subsystem=2)
    harnesses = [artifact(*item, expected_subsystem=3) for item in tests]
    command = ['python3', '-B', str(SCRIPT), '--host', 'elior@192.168.10.212',
               '--identity', '/config/.local/share/future-classroom-test-access/win11-192.168.10.212',
               '--exe', str(NORMAL), '--test-metadata', str(METADATA),
               '--test-timeout-seconds', '900', '--native-seconds', '1',
               '--system-host', '--public-desktop', '--report', str(DEST)]
    state = {'status': 'RUNNING', 'startedUtc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
             'command': command, 'sourceFreeze': FREEZE.name, 'sourceFreezeSha256': FREEZE_SHA,
             'workingFileCount': 595, 'script': identity(SCRIPT), 'metadata': identity(METADATA),
             'buildProvenance': builds, 'normal': normal, 'tests': harnesses,
             'expectedHarnessCount': len(harnesses), 'testTotal': 'COUNT_FROM_ACTUAL_RAW_RESULTS_ONLY',
             'scope': 'R70 supplied joint source/freeze, including later root-owned production long-path publication correction when present; actual Win11/WOW64 GNU release harnesses plus hidden public desktop only. Aggregate case total comes from actual results, never hardcoded1542. Ordinary31 material-stop/reuse, visual/repair and other flows are separate; no Win7/10 OS or campus proof.'}
    save(RECORD, state, exclusive=True)
    started = time.monotonic()
    try:
        with STDOUT.open('xb') as a, STDERR.open('xb') as b:
            child = subprocess.Popen(command, stdout=a, stderr=b, cwd=MAIN)
            state['controllerPid'] = child.pid
            save(RECORD, state)
            code = child.wait()
    except BaseException as error:
        state.update(status='OBSERVATION_INTERRUPTED_NOT_COMPLETION',
                     exceptionKind=type(error).__name__, elapsedSeconds=time.monotonic() - started)
        save(RECORD, state)
        raise
    state.update(status='PASS_COMMAND' if code == 0 else 'FAIL_COMMAND', exitCode=code,
                 elapsedSeconds=time.monotonic() - started, sourceUnchanged=source_current(frozen),
                 scriptUnchanged=sha(SCRIPT) == state['script']['sha256'],
                 metadataUnchanged=sha(METADATA) == state['metadata']['sha256'],
                 artifactsUnchanged=all(sha(Path(row['path'])) == row['sha256'] for row in [normal] + harnesses),
                 stdout=identity(STDOUT), stderr=identity(STDERR))
    if DEST.is_file():
        try:
            result = json.loads(DEST.read_text())
            state.update(resultStatus=result.get('status'), cleanup=result.get('cleanup'), result=identity(DEST))
        except (ValueError, UnicodeError):
            state['resultStatus'] = 'INVALID_JSON'
    valid = (code == 0 and state.get('resultStatus') == 'PASS' and state.get('cleanup') == 'PASS'
             and all(state[key] for key in ('sourceUnchanged', 'scriptUnchanged', 'metadataUnchanged', 'artifactsUnchanged')))
    if not valid:
        state['status'] = 'FAIL_COMMAND_OR_VALIDATION'
    save(RECORD, state)
    print(json.dumps({key: state.get(key) for key in ('status', 'exitCode', 'elapsedSeconds', 'resultStatus', 'cleanup', 'sourceUnchanged')}), flush=True)
    return 0 if valid else 1


if __name__ == '__main__':
    sys.exit(main())
