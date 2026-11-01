"""Root-only finite R68 release harness; invoke --execute after both builds PASS.
Preparation/AST validation performs no Windows/SSH or build operation.
"""
from pathlib import Path
import datetime
import hashlib
import json
import struct
import subprocess
import sys
import time

ROOT = Path('/tmp/fc-round68-production-https')
EVIDENCE = ROOT / 'evidence'
WORKTREE = ROOT / 'worktree'
MAIN = Path('/workspace/共享/Eliora/未来教室计划')
FREEZE = EVIDENCE / 'joint-implementation-freeze-fourth.json'
FREEZE_SHA = 'fb957f0dfb80bc840507d2a737ba13f5aa09e436848bbe4e9d352a50df1cd196'
SCRIPT = MAIN / 'scripts/windows-probe.py'
TARGET = ROOT / 'target/i686-pc-windows-gnu/release'
NORMAL = TARGET / 'fc-bootstrap.exe'
METADATA = EVIDENCE / 'joint-release-tests-first.stdout.log'
DEST = EVIDENCE / 'windows-release-first.json'
RECORD = EVIDENCE / 'windows-release-operation-first.json'
STDOUT = EVIDENCE / 'windows-release-first.stdout.log'
STDERR = EVIDENCE / 'windows-release-first.stderr.log'


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


def compile_report(name):
    path = EVIDENCE / (name + '.json')
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
            and path.resolve().is_relative_to(TARGET.resolve()), 'artifact escaped exact R68 target')
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
    require(sys.argv[1:] == ['--execute'], 'Use --execute only after root observes both original build reports PASS')
    require(not any(path.exists() for path in (RECORD, DEST, STDOUT, STDERR)),
            'first operation/raw/result already exists; preserve it and resume original operation')
    require(FREEZE.is_file() and sha(FREEZE) == FREEZE_SHA, 'R68 fourth source freeze mismatch')
    frozen = json.loads(FREEZE.read_text())
    require(len(frozen['workingFiles']) == 595 and source_current(frozen),
            'R68 exact 595 frozen working inputs changed')
    require(sha(SCRIPT) == sha(WORKTREE / 'scripts/windows-probe.py'),
            'main runner differs from frozen worktree input')
    builds = {name: compile_report(name) for name in
              ('joint-clippy-fourth', 'joint-release-tests-first', 'joint-release-product-first')}
    require(builds['joint-release-tests-first']['stdout']['sha256'] == sha(METADATA),
            'test metadata differs from actual producer stdout')
    tests = [(number, raw, row) for number, raw, row in compiler_artifacts(METADATA)
             if row.get('profile', {}).get('test') is True]
    require(len(tests) == 21 and len({row['executable'] for _, _, row in tests}) == 21,
            'expected actual 21 distinct harness artifacts from complete Cargo JSON')
    product_log = EVIDENCE / 'joint-release-product-first.stdout.log'
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
             'scope': 'R68 A3+B4+C5 configured-runtime joint source; actual Win11/WOW64 GNU release harnesses plus hidden public desktop. Ordinary HTTPS authenticated wire, visual and repair are separate operations; no Win7/10 runtime or campus proof.'}
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
