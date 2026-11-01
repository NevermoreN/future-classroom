"""Remove only completed R70/R71 owned roots after exact committed proof."""
from pathlib import Path
import base64, datetime, hashlib, json, os, shutil, stat, subprocess
MAIN = Path('/workspace/共享/Eliora/未来教室计划')
META = MAIN / '开发计划/验证记录'
ARCHIVE = META / '2026-11-01-round70-transfer-stop'
ROOT = Path('/tmp/fc-round70-ordinary-transfer-stop')
R71 = Path('/tmp/fc-round71-win7-item-order')
CONTROL = Path('/tmp/fc-round70-archive-controller')
INSTALLER = Path('/tmp/fc-next-installer-wiring-audit-20261101')
TRANSFER = Path('/tmp/fc-next-https-transfer-audit-backend-20261101')
ACTIVE = Path('/tmp/fc-round72-ci-evidence')
COMMIT = '9a6093ca09ab51b6c4c5bf4265b5f9e0b81c18c0'
INDEX_SHA = 'c22873724e3155854c498443e374f4f0afe6b574fe911e967cc9ce8e2b8d674f'
RECEIPT = META / '2026-11-01-round70-cleanup-first.json'
def sha(data): return hashlib.sha256(data).hexdigest()
def git(*args, data=None): return subprocess.check_output(['git', *args], cwd=MAIN, input=data)
def identity(path):
    assert path.is_file() and not path.is_symlink()
    data = path.read_bytes()
    return dict(path=str(path), bytes=len(data), sha256=sha(data))
def exact(left, right):
    assert not left.is_symlink() and not right.is_symlink()
    assert left.read_bytes() == right.read_bytes()
    return dict(original=identity(left), archived=identity(right))
def main():
    assert not RECEIPT.exists() and git('rev-parse', 'HEAD').decode().strip() == COMMIT
    assert not git('diff', '--cached', '--name-only', '-z')
    owned = [(ROOT, b'future-classroom-owned-r70-ordinary-transfer-stop/v1\n'),
             (R71, b'future-classroom-owned-r71-win7-item-order/v1\n'),
             (INSTALLER, b'future-classroom-owned-next-installer-wiring-audit/v1\n'),
             (TRANSFER, b'future-classroom-owned-next-https-transfer-audit/v1\n'),
             (CONTROL, b'future-classroom-owned-r70-archive-controller/v1\n')]
    for root, marker in owned:
        assert root.is_dir() and not root.is_symlink() and (root / '.owner').read_bytes() == marker
    assert (ACTIVE / '.owner').read_bytes() == b'future-classroom-owned-r72-ci-evidence/v1\n'
    proofs = {}
    for name in ('staged-bytes-first', 'committed-bytes-first', 'committed-bytes-second'):
        path = META / ('2026-11-01-round70-' + name + '.json')
        exact(path, CONTROL / (name + '.json'))
        proofs[name] = json.loads(path.read_bytes())
        assert proofs[name]['fileCount'] == 331 and proofs[name]['archiveFileCount'] == 321
    proof = proofs['committed-bytes-second']
    assert proof['verifiedCommit'] == COMMIT and proof['status'] == 'PASS_EXACT_COMMITTED_BYTES'
    assert all(p['files'] == proof['files'] for p in proofs.values())
    rows = proof['files']
    payload = git('cat-file', '--batch', data=''.join(r['gitBlob'] + '\n' for r in rows).encode())
    cursor = 0
    for row in rows:
        end = payload.index(b'\n', cursor); blob, kind, size = payload[cursor:end].split()
        size = int(size); cursor = end + 1; data = payload[cursor:cursor + size]; cursor += size
        assert payload[cursor:cursor + 1] == b'\n'; cursor += 1
        assert blob.decode() == row['gitBlob'] and kind == b'blob' and size == row['bytes'] and sha(data) == row['sha256']
        assert data == (MAIN / row['path']).read_bytes()
    assert cursor == len(payload)
    assert sha((ARCHIVE / 'archive-index-first.json').read_bytes()) == INDEX_SHA
    index = json.loads((ARCHIVE / 'archive-index-first.json').read_bytes())
    assert len(index['files']) == 319 and index['excludedPayloads'] == []
    expected_archive = {r['path'] for r in index['files']} | {'archive-index-first.json', index['verificationReceipt']['path']}
    assert {p.relative_to(ARCHIVE).as_posix() for p in ARCHIVE.rglob('*') if p.is_file()} == expected_archive
    assert {p.relative_to(ROOT / 'evidence').as_posix() for p in (ROOT / 'evidence').rglob('*') if p.is_file()} == {r['path'] for r in index['files']}
    source_proof = []
    for row in index['files']:
        original = ROOT / 'evidence' / row['path']; stored = ARCHIVE / row['path']
        pair = exact(original, stored)
        assert pair['original']['bytes'] == row['bytes'] and pair['original']['sha256'] == row['sha256']
        source_proof.append(pair)
    receipt_row = index['verificationReceipt']; actual = identity(ARCHIVE / receipt_row['path'])
    assert actual['bytes'] == receipt_row['bytes'] and actual['sha256'] == receipt_row['sha256']
    related = []
    for source, archived in [(R71 / 'evidence', ARCHIVE / 'round71-source-order'),
                              (INSTALLER, ARCHIVE / 'next-installer-readonly'),
                              (TRANSFER, ARCHIVE / 'prior-readonly-design')]:
        for path in sorted(source.rglob('*')):
            if path.is_file() and path.name != '.owner':
                related.append(exact(path, archived / path.relative_to(source)))
    frozen = json.loads((ARCHIVE / 'joint-source-freeze-eighth.json').read_bytes())
    source_paths = {r['path'] for r in frozen['files']}
    dirty_proof = []
    trees = [(ROOT / 'worktree', 'codex/ordinary-transfer-stop', source_paths),
             (R71 / 'worktree', 'codex/win7-item-order', {'crates/fc-windows/src/tls_frontdoor/mod.rs'})]
    by_hash = {}
    for row in index['files']: by_hash.setdefault(row['sha256'], []).append(row['path'])
    for tree, branch, expected in trees:
        assert git('-C', str(tree), 'branch', '--show-current').decode().strip() == branch
        assert not git('-C', str(tree), 'diff', '--cached', '--name-only', '-z')
        assert not git('-C', str(tree), 'ls-files', '--others', '--exclude-standard', '-z')
        assert not git('-C', str(tree), 'ls-files', '--others', '--ignored', '--exclude-standard', '-z')
        dirty = {p.decode() for p in git('-C', str(tree), 'diff', '--name-only', '-z').split(b'\0') if p}
        assert dirty == expected
        for path in sorted(dirty):
            original = tree / path; data = original.read_bytes(); matching = by_hash.get(sha(data), [])
            assert matching and data == (MAIN / path).read_bytes()
            match = next(p for p in matching if (ARCHIVE / p).read_bytes() == data)
            dirty_proof.append(exact(original, ARCHIVE / match))
    outside = []
    prefix = '2026-11-01-round70-archive-operation-first'
    for suffix in ('.json', '.stdout.log', '.stderr.log'):
        outside.append(exact(CONTROL / ('archive-operation-first' + suffix), META / (prefix + suffix)))
    expected_control = {'.owner', 'archive-owned-first.py', 'archive-operation-first.json', 'archive-operation-first.stdout.log',
        'archive-operation-first.stderr.log', 'staging-paths-first.nul', 'stage-first.json', 'stage-first.stdout.log',
        'stage-first.stderr.log', 'diff-check-first.json', 'diff-check-first.stdout.log', 'diff-check-first.stderr.log',
        'commit-first.json', 'commit-first.stdout.log', 'commit-first.stderr.log', 'staged-bytes-first.json',
        'committed-bytes-first.json', 'committed-bytes-second.json'}
    assert {p.relative_to(CONTROL).as_posix() for p in CONTROL.rglob('*') if p.is_file()} == expected_control
    assert (CONTROL / 'staging-paths-first.nul').read_bytes() == b''.join(r['path'].encode() + b'\0' for r in rows)
    assert json.loads((CONTROL / 'stage-first.json').read_bytes())['status'] == 'PASS'
    assert json.loads((CONTROL / 'commit-first.json').read_bytes())['commit'] == COMMIT
    assert not (CONTROL / 'diff-check-first.stdout.log').read_bytes() and not (CONTROL / 'diff-check-first.stderr.log').read_bytes()
    preserved = []
    for name in sorted(expected_control - {'.owner', 'staging-paths-first.nul', 'archive-operation-first.json',
        'archive-operation-first.stdout.log', 'archive-operation-first.stderr.log', 'staged-bytes-first.json',
        'committed-bytes-first.json', 'committed-bytes-second.json'}):
        path = CONTROL / name
        preserved.append(dict(**identity(path), base64=base64.b64encode(path.read_bytes()).decode()))
    active = []; needles = [os.fsencode(p) for p, _ in owned]
    for proc in Path('/proc').iterdir():
        if not proc.name.isdigit() or int(proc.name) in (os.getpid(), os.getppid()): continue
        try:
            args = (proc / 'cmdline').read_bytes().split(b'\0'); reasons = []
            if any(n in b' '.join(args) for n in needles): reasons.append('argv')
            try: cwd = os.fsencode(os.readlink(proc / 'cwd'))
            except (FileNotFoundError, PermissionError, ProcessLookupError): cwd = b''
            if any(cwd == n or cwd.startswith(n + b'/') for n in needles): reasons.append('cwd')
            try:
                for fd in (proc / 'fd').iterdir():
                    try: target = os.fsencode(os.readlink(fd))
                    except (FileNotFoundError, PermissionError, ProcessLookupError): continue
                    if any(target == n or target.startswith(n + b'/') for n in needles): reasons.append('open-fd'); break
            except (FileNotFoundError, PermissionError, ProcessLookupError): pass
            if reasons: active.append(dict(pid=int(proc.name), executable=Path(os.fsdecode(args[0])).name, reasons=reasons))
        except (FileNotFoundError, PermissionError, ProcessLookupError): continue
    assert not active, active
    inventory = []
    for root, marker in owned:
        count = size = dirs = links = 0
        for path in root.rglob('*'):
            mode = path.lstat().st_mode
            if stat.S_ISLNK(mode): links += 1
            elif stat.S_ISREG(mode): count += 1; size += path.stat().st_size
            elif stat.S_ISDIR(mode): dirs += 1
            else: raise AssertionError('nonregular owned member')
        inventory.append(dict(path=str(root), owner=marker.decode(), regularFiles=count, logicalBytes=size, directories=dirs, symlinksNotFollowed=links))
    # All original-byte and process assertions precede the first removal.
    actions = []
    for tree, branch, _ in trees:
        command = ['git', 'worktree', 'remove', '--force', str(tree)]
        completed = subprocess.run(command, cwd=MAIN, capture_output=True)
        actions.append(dict(command=command, exitCode=completed.returncode, stdoutBase64=base64.b64encode(completed.stdout).decode(), stderrBase64=base64.b64encode(completed.stderr).decode()))
        assert completed.returncode == 0 and not tree.exists()
        command = ['git', 'branch', '-D', branch]
        completed = subprocess.run(command, cwd=MAIN, capture_output=True)
        actions.append(dict(command=command, exitCode=completed.returncode, stdoutBase64=base64.b64encode(completed.stdout).decode(), stderrBase64=base64.b64encode(completed.stderr).decode()))
        assert completed.returncode == 0
    for root, _ in owned: shutil.rmtree(root); assert not root.exists()
    assert (ACTIVE / '.owner').read_bytes() == b'future-classroom-owned-r72-ci-evidence/v1\n'
    report = dict(schemaVersion=1, status='PASS_R70_R71_COMPLETED_OWNED_ROOTS_REMOVED_AFTER_EXACT_COMMIT_PROOF',
        recordedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(), verifiedCommit=COMMIT,
        archiveIndexSha256=INDEX_SHA, everyOriginalGitBlobReverified=331, sourceOriginalsVerified=source_proof,
        excludedPayloads=[], relatedOriginalsVerified=related, dirtyWorktreeSourcesArchived=dirty_proof,
        outsideControllerOriginalsVerified=outside, preservedTransientControllerOriginals=preserved,
        pathspec='Exact sorted331-path NUL vector matched committed proof; derivable from retained proof rows.',
        originalHeadAliasProof='First committed proof used HEAD; second binds the full commit explicitly. Both original reports retained.',
        removalInventory=inventory, worktreeBranchRemoval=actions, activeOwnedProcessesBeforeRemoval=active,
        physicalDiskBytesFreed='NOT_MEASURED', helperSha256=sha(Path(__file__).read_bytes()),
        scope='Only five exact owned completed roots removed. R72, main target/global caches/three developer ZIPs, Windows SSH service/public authorization and external private key preserved. No key content read/hash, no Windows/Cargo/CI operation. Historical failures remain committed; finite Win11 scope unchanged.')
    with RECEIPT.open('x') as stream: json.dump(report, stream, ensure_ascii=False, indent=2); stream.write('\n')
    print(json.dumps(dict(status=report['status'], removed=inventory, receiptSha256=sha(RECEIPT.read_bytes()))))
if __name__ == '__main__': main()
