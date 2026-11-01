#!/usr/bin/env python3
"""Verify committed original bytes before deleting only this batch's owned roots."""
from pathlib import Path
import hashlib, json, shutil, stat, subprocess

ROOT = Path('/workspace/共享/Eliora/未来教室计划')
RECORDS = ROOT / '开发计划/验证记录'
sha = lambda data: hashlib.sha256(data).hexdigest()
git = lambda *args: subprocess.check_output(['git', *args], cwd=ROOT)
head = git('rev-parse', 'HEAD').decode().strip()
assert git('rev-parse', '6d3c369').decode().strip() == '6d3c369efcf285fc6ffbca2e8c39cb936d77f393'
proof_path = RECORDS / '2026-11-01-round52-54-committed-bytes-first.json'
cleanup_path = RECORDS / '2026-11-01-round52-54-local-cleanup-first.json'
assert not proof_path.exists() and not cleanup_path.exists()
prefixes = ['开发计划/验证记录/2026-11-01-round' + n + '-' + label + '/' for n, label in [('52', 'ci'), ('53', 'msi'), ('54', 'win7-loader')]]
archive_reports = {'开发计划/验证记录/2026-11-01-round' + n + '-archive-first.json' for n in ['52', '53', '54']}
source_paths = git('diff-tree', '--no-commit-id', '--name-only', '-r', '-z', '6d3c369').decode().strip('\0').split('\0')
rows = []
process = subprocess.Popen(['git', 'cat-file', '--batch'], cwd=ROOT, stdin=subprocess.PIPE, stdout=subprocess.PIPE)
for entry in git('ls-files', '--stage', '-z').split(b'\0'):
    if not entry:
        continue
    info, raw_path = entry.split(b'\t', 1)
    mode, oid, stage = info.split()
    path = raw_path.decode()
    if not (path.startswith(tuple(prefixes)) or path in archive_reports or path in source_paths):
        continue
    assert stage == b'0' and mode in {b'100644', b'100755'}
    process.stdin.write(oid + b'\n')
    process.stdin.flush()
    actual_oid, kind, length = process.stdout.readline().split()
    assert actual_oid == oid and kind == b'blob'
    length = int(length)
    data = process.stdout.read(length)
    assert len(data) == length and process.stdout.read(1) == b'\n'
    local = ROOT / path
    assert local.is_file() and not local.is_symlink() and local.read_bytes() == data
    rows.append({'path': path, 'gitBlob': oid.decode(), 'bytes': length, 'sha256': sha(data)})
process.stdin.close()
assert process.wait(timeout=30) == 0
assert len(rows) == 1042  # 1030 archived originals/indices/reports plus 12 source/docs.
proof_path.write_text(json.dumps({'status': 'PASS_COMMITTED_GIT_BLOBS_EXACT_BYTES', 'commit': head,
                                 'sourceCommit': '6d3c369efcf285fc6ffbca2e8c39cb936d77f393', 'files': rows}, ensure_ascii=False, indent=2) + '\n')

trees = [
    (Path('/tmp/fc-round53-msi-candidate-worktree'), 'codex/msi-candidate', {
        'packaging/PROBE-README.txt', 'scripts/package-probe.py', 'scripts/windows-test-edge-owner.py',
        'packaging/MSI-CANDIDATE.md', 'scripts/check-msi-candidate.py', 'scripts/package-msi-candidate.py'}),
    (Path('/tmp/fc-round54-win7-loader-worktree'), 'codex/win7-loader-imports', {
        'crates/fc-windows/src/desktop.rs', 'crates/fc-windows/src/workbench/color_picker.rs', 'crates/fc-windows/src/audio/device.rs'}),
]
roots = [(Path('/tmp/fc-round' + n + '-' + label), 'future-classroom-owned-r' + n + '-' + label + '/v1\n')
         for n, label in [('52', 'ci-evidence'), ('53', 'msi-evidence'), ('53', 'msi-tools'), ('54', 'win7-loader-evidence'), ('54', 'win7-loader-target')]]
for path, branch, expected in trees:
    assert path.is_dir() and not path.is_symlink()
    assert subprocess.check_output(['git', '-C', str(path), 'branch', '--show-current']).decode().strip() == branch
    assert subprocess.check_output(['git', '-C', str(path), 'rev-parse', 'HEAD']).decode().strip() == 'f6963919f100618fc482b1bff1a42dcbc571e610'
    changed = set(subprocess.check_output(['git', '-C', str(path), 'diff', '--name-only', '-z']).decode().strip('\0').split('\0'))
    untracked = set(subprocess.check_output(['git', '-C', str(path), 'ls-files', '--others', '--exclude-standard', '-z']).decode().strip('\0').split('\0')) - {''}
    assert changed | untracked == expected
    for rel in expected:
        assert (path / rel).read_bytes() == (ROOT / rel).read_bytes()
for path, marker in roots:
    assert path.is_dir() and not path.is_symlink() and (path / '.owner').read_text() == marker

def counts(root):
    files, size = 0, 0
    for path in root.rglob('*'):
        info = path.lstat()
        if stat.S_ISREG(info.st_mode):
            files += 1
            size += info.st_size
    return {'files': files, 'bytes': size}

# Each relevant process was reaped before this script: R52 runtime/remaining
# sessions56512/33780; R53 MSI62059; R54 compiler90702, Windows23491 and audit63486.
# No process termination, key read, remote call or system configuration mutation.
removed = []
for path, branch, expected in trees:
    row = {'path': str(path), 'kind': 'owned-worktree', **counts(path)}
    subprocess.run(['git', 'worktree', 'remove', '--force', str(path)], cwd=ROOT, check=True)
    subprocess.run(['git', 'branch', '-D', branch], cwd=ROOT, check=True, stdout=subprocess.DEVNULL)
    assert not path.exists()
    row.update(absent=True, branchRemoved=True)
    removed.append(row)
for path, marker in roots:
    row = {'path': str(path), 'kind': 'owned-root', **counts(path)}
    shutil.rmtree(path)
    assert not path.exists()
    row['absent'] = True
    removed.append(row)

# These reports were emitted outside their inventoried source to avoid recursive
# self-inclusion; sealed originals already match the committed archive reports.
outside = []
for n in ['52', '53', '54']:
    sealed = Path('/tmp/fc-round' + n + '-archive-sealed-first.json')
    assert sealed.read_bytes() == (RECORDS / ('2026-11-01-round' + n + '-archive-first.json')).read_bytes()
    sealed.unlink()
    outside.append(str(sealed))
    prepared = Path('/tmp/fc-round' + n + '-archive-preparation-first.json')
    if prepared.exists():
        # Preparations only inventory unchanged originals; sealed reports carry
        # the same file/SHA ledger and supersede their not-yet-copied status.
        d = json.loads(prepared.read_text())
        final = json.loads((RECORDS / ('2026-11-01-round' + n + '-archive-first.json')).read_text())
        assert d['files'] == final['files'] and d['status'] == 'PREPARED_ORIGINAL_TEXT_ONLY'
        prepared.unlink()
        outside.append(str(prepared))

assert Path('/tmp/fc-round55-ci-evidence/.owner').read_text() == 'future-classroom-owned-r55-ci-evidence/v1\n'
assert stat.S_IMODE(Path('/config/.local/share/future-classroom-test-access/win11-192.168.10.212').stat().st_mode) == 0o600
cleanup_path.write_text(json.dumps({'status': 'PASS_OWNED_LOCAL_BATCH_CLEANUP', 'commitProved': head,
                                   'proofSha256': sha(proof_path.read_bytes()), 'removed': removed,
                                   'outsideReportsRemoved': outside, 'helperSha256': sha(Path(__file__).read_bytes()),
                                   'preserved': ['main target/debug and global developer caches', 'persistent SSH private key and authorization/service',
                                                 'three intentional /mnt/data development ZIPs', 'new active R55 CI evidence root'],
                                   'windows': 'Separate final R54 read-only audit already PASS; no new remote mutation or observation by this local cleanup'},
                                  ensure_ascii=False, indent=2) + '\n')
print(json.dumps({'status': 'PASS', 'committedFilesProved': len(rows), 'ownedRootsRemoved': len(removed),
                  'logicalBytesRemoved': sum(x['bytes'] for x in removed), 'outsideReportsRemoved': len(outside)}))
