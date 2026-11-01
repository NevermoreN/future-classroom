"""Remove only the completed, byte-proven R68 owned local test root."""
from pathlib import Path
import datetime
import hashlib
import json
import os
import shutil
import stat
import subprocess

MAIN = Path('/workspace/共享/Eliora/未来教室计划')
ROOT = Path('/tmp/fc-round68-production-https')
ARCHIVE = MAIN / '开发计划/验证记录/2026-11-01-round68-production-https'
RECEIPT = MAIN / '开发计划/验证记录/2026-11-01-round68-cleanup-first.json'

def sha(data):
    return hashlib.sha256(data).hexdigest()

def main():
    assert not RECEIPT.exists()
    assert not ROOT.is_symlink() and (ROOT / '.owner').read_bytes() == b'future-classroom-owned-r68-production-https/v1\n'
    proof_path = MAIN / '开发计划/验证记录/2026-11-01-round68-committed-bytes-first.json'
    proof = json.loads(proof_path.read_bytes())
    assert proof['status'] == 'PASS_EXACT_COMMITTED_BYTES'
    assert proof['verifiedCommit'] == '02332c3ec3e20a496f35213228f3ec141f63327a'
    assert proof['fileCount'] == 1230 and proof['archiveFileCount'] == 1181
    frozen = json.loads((ARCHIVE / 'joint-implementation-freeze-seventh.json').read_bytes())
    for row in frozen['files']:
        for directory in (MAIN, ROOT / 'worktree'):
            data = (directory / row['path']).read_bytes()
            assert len(data) == row['bytes'] and sha(data) == row['sha256']
    # Independently check that every dirty check-clone file is already retained
    # by exact content in the committed archive before any worktree is removed.
    archived = {}
    for path in ARCHIVE.rglob('*'):
        if path.is_file():
            archived.setdefault(sha(path.read_bytes()), path.relative_to(ARCHIVE).as_posix())
    worktrees = []
    for name in ('native-check-worktree', 'native-check-worktree-second', 'native-check-worktree-third', 'worktree'):
        directory = ROOT / name
        rows = set(subprocess.check_output(['git', 'diff', '--name-only', '-z'], cwd=directory).split(b'\0'))
        rows |= set(subprocess.check_output(['git', 'ls-files', '--others', '--exclude-standard', '-z'], cwd=directory).split(b'\0'))
        preserved = []
        for row in sorted(rows - {b''}):
            path = directory / row.decode('utf-8')
            assert path.is_file() and not path.is_symlink()
            data = path.read_bytes()
            digest = sha(data)
            assert digest in archived, str(path)
            preserved.append({'path': row.decode('utf-8'), 'bytes': len(data), 'sha256': digest, 'archiveMember': archived[digest]})
        assert len(preserved) == (47 if name == 'worktree' else 12)
        worktrees.append({'path': str(directory), 'changedFilesRetained': preserved})
    # The failed archive is a duplicate temporary output, not an original source.
    failed = ROOT / 'failed-archive-second'
    failed_count = 0
    for path in failed.rglob('*'):
        if path.is_file():
            data = path.read_bytes()
            assert sha(data) in archived
            failed_count += 1
    assert failed_count == 1175
    # All root-owned controllers have returned. Refuse removal if another test,
    # compiler or transport is still using this root. No process is terminated.
    active = []
    for proc in Path('/proc').iterdir():
        if not proc.name.isdigit() or int(proc.name) in (os.getpid(), os.getppid()):
            continue
        try:
            args = (proc / 'cmdline').read_bytes().split(b'\0')
        except (FileNotFoundError, PermissionError, ProcessLookupError):
            continue
        if not args or str(ROOT).encode() not in b' '.join(args):
            continue
        executable = Path(os.fsdecode(args[0])).name
        if executable.startswith(('python', 'cargo', 'rustc', 'rustfmt', 'clippy', 'ssh', 'scp', 'wine', 'node', 'npm')):
            active.append({'pid': int(proc.name), 'executable': executable})
    assert not active, active
    file_count = logical_bytes = symlink_count = 0
    for path in ROOT.rglob('*'):
        metadata = path.lstat()
        if stat.S_ISREG(metadata.st_mode):
            file_count += 1
            logical_bytes += metadata.st_size
        elif stat.S_ISLNK(metadata.st_mode):
            symlink_count += 1
    for row in worktrees:
        subprocess.run(['git', 'worktree', 'remove', '--force', row['path']], cwd=MAIN, check=True)
    subprocess.run(['git', 'branch', '-d', 'codex/production-https'], cwd=MAIN, check=True, capture_output=True)
    shutil.rmtree(ROOT)
    assert not ROOT.exists()
    report = {'status': 'PASS_COMPLETED_OWNED_ROOT_REMOVED',
              'recordedUtc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
              'committedBytesProofSha256': sha(proof_path.read_bytes()),
              'verifiedCommit': proof['verifiedCommit'], 'removedRoot': str(ROOT),
              'removedRegularFiles': file_count, 'removedLogicalFileBytes': logical_bytes,
              'removedSymlinks': symlink_count, 'physicalDiskBytesFreed': 'NOT_MEASURED',
              'worktrees': worktrees, 'duplicateFailedArchiveMembersRetained': failed_count,
              'activeOwnedProcessesBeforeRemoval': active,
              'scope': 'Local completed R68 root only. No remote Windows mutation or process termination. Persistent SSH service/public keys/private key, main target/global caches, intentional dev ZIPs and active R69/R70 roots remain untouched.',
              'helperSha256': sha(Path(__file__).read_bytes())}
    with RECEIPT.open('x') as out:
        json.dump(report, out, ensure_ascii=False, indent=2)
        out.write('\n')
    print(report['status'], file_count, logical_bytes)

if __name__ == '__main__':
    main()
