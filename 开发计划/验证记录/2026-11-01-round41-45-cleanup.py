"""Remove only completed, byte-proven, task-owned local working copies.

Never contacts Windows, signals a process, or accesses SSH key material.
Build caches are reproducible owned output, not archived original evidence.
"""
from pathlib import Path
import datetime
import hashlib
import json
import os
import shutil
import subprocess
import sys
import zipfile

root = Path('/workspace/共享/Eliora/未来教室计划')
reports = root / '开发计划/验证记录'
out = reports / '2026-11-01-round41-45-cleanup-first.json'
assert not out.exists(), 'Preserve original cleanup result; do not repeat blindly'
commit = sys.argv[1]
assert len(commit) == 40
r = {'status': 'PRECHECK', 'atUtc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
     'commit': commit, 'actions': [], 'scope': 'Exact committed originals and unchanged integrated sources precede removal. No process signals, Windows actions, SSH access or global cache removal. R47 and R46 remain active.'}
def save():
    out.write_text(json.dumps(r, indent=2) + '\n')
def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()
def git(*args):
    return subprocess.check_output(['git', *args], cwd=root)
def exact_dir(path):
    assert path.is_dir() and not path.is_symlink() and path.resolve() == path
def marker(path, value):
    exact_dir(path)
    assert (path / '.owner').read_text().strip() == value
def removal(path, basis):
    exact_dir(path)
    count = size = 0
    for p in path.rglob('*'):
        assert not p.is_symlink(), str(p)
        if p.is_file():
            count += 1
            size += p.stat().st_size
    shutil.rmtree(path)
    assert not path.exists()
    r['actions'].append({'path': str(path), 'files': count, 'bytes': size, 'basis': basis, 'removed': True})
    save()
save()
try:
    proof = json.loads((reports / '2026-11-01-round41-45-committed-proof.json').read_text())
    assert proof['status'] == 'PASS' and proof['commit'] == commit
    for row in proof['files']:
        data = git('show', commit + ':' + row['path'])
        assert len(data) == row['bytes'] and hashlib.sha256(data).hexdigest() == row['sha256']
    originals = {}
    for name in ['2026-11-01-round41-frontdoor-deadline', '2026-11-01-round45-ci-input-domains']:
        base = reports / name
        idx = json.loads((base / 'INDEX.json').read_text())
        path = Path(idx['sourceRoot'])
        exact_dir(path)
        expected = {row['path']: row for row in idx['files']}
        actual = {p.relative_to(path).as_posix() for p in path.rglob('*') if p.is_file()}
        assert actual == set(expected), (name, actual ^ set(expected))
        for n, row in expected.items():
            assert (path / n).stat().st_size == row['bytes'] and sha(path / n) == row['sha256'], n
            assert (base / n).read_bytes() == (path / n).read_bytes()
        originals[name] = (path, idx)
    # R41 did not originally have an owner marker. Its exact recorded source
    # root, complete indexed bytes and commit proof are the ownership basis.
    r['r41EvidenceOwnerMarker'] = 'NOT_PRESENT; complete original index and recorded sourceRoot used'
    e41, i41 = originals['2026-11-01-round41-frontdoor-deadline']
    e45, i45 = originals['2026-11-01-round45-ci-input-domains']
    marker(e45, 'future-classroom-r45-ci-input-domains-evidence/v1')
    trees = [
        (Path('/tmp/fc-round41-frontdoor-deadline-worktree'), 'codex/frontdoor-deadline-close', 9),
        (Path('/tmp/fc-round45-ci-input-domains-worktree'), 'codex/ci-input-domains', 1),
    ]
    registered = git('worktree', 'list', '--porcelain').decode()
    for path, branch, count in trees:
        exact_dir(path)
        assert 'worktree ' + str(path) + '\n' in registered
        assert subprocess.check_output(['git', 'branch', '--show-current'], cwd=path, text=True).strip() == branch
        assert not subprocess.check_output(['git', 'ls-files', '--others', '--exclude-standard', '-z'], cwd=path)
        names = subprocess.check_output(['git', 'diff', '--name-only', '-z'], cwd=path).decode().rstrip('\0').split('\0')
        assert len(names) == count
        for n in names:
            assert (path / n).read_bytes() == git('show', commit + ':' + n), n
    target = Path('/tmp/fc-round41-build-target')
    marker(target, 'future-classroom-owned-r41-build-target/v1')
    program_sets = []
    for suffix in ['second', 'third']:
        p = Path('/tmp/fc-round41-programs-' + suffix)
        exact_dir(p)
        build = json.loads((e41 / ('windows-integrated-' + suffix + '.json')).read_text())
        assert build['status'] == 'PASS'
        expected = {Path(a['path']).name: a for a in build['artifacts']}
        assert len(expected) == 22 and {x.name for x in p.iterdir()} == set(expected)
        for n, row in expected.items():
            assert (p/n).stat().st_size == row['bytes'] and sha(p/n) == row['sha256']
        program_sets.append(p)
    package = Path('/tmp/fc-round42-package')
    assets = Path('/tmp/fc-round42-web-assets')
    marker(package, 'future-classroom-owned-r42-package/v1')
    marker(assets, 'future-classroom-owned-r42-web-assets/v1')
    name = 'FutureClassroom-0.1.0-voice-capability-dev.zip'
    digest = '5e280a84e726f0366db3e0ea1bd704158e26ac1b3fdb4e542f397b48fc1706c0'
    assert {x.name for x in package.iterdir()} == {'.owner', name}
    retained = Path('/mnt/data') / name
    assert sha(package/name) == sha(retained) == digest
    with zipfile.ZipFile(retained) as z:
        local = [p for p in (assets/'dist').rglob('*') if p.is_file()]
        assert len(local) == 7
        assert {p.relative_to(assets).as_posix() for p in assets.rglob('*') if p.is_file()} == {'.owner', *(p.relative_to(assets).as_posix() for p in local)}
        for p in local:
            assert p.read_bytes() == z.read('web/' + p.relative_to(assets/'dist').as_posix())
    owned_paths = [x[0] for x in trees] + [e41, e45, target, *program_sets, package, assets]
    # Exclude only the actual verified current process/ancestor chain. A mere
    # path substring in any other live process is a conservative stop.
    ancestors = set()
    pid = os.getpid()
    while pid and pid not in ancestors:
        ancestors.add(pid)
        stat = Path('/proc') / str(pid) / 'stat'
        text = stat.read_text()
        pid = int(text[text.rfind(')')+2:].split()[1])
    references = []
    for p in Path('/proc').iterdir():
        if not p.name.isdigit() or int(p.name) in ancestors:
            continue
        try:
            command = (p/'cmdline').read_bytes().replace(b'\0', b' ').decode(errors='replace')
            cwd = os.readlink(p/'cwd')
        except (OSError, PermissionError):
            continue
        matched = [str(x) for x in owned_paths if str(x) in command or cwd == str(x) or cwd.startswith(str(x)+'/')]
        if matched:
            references.append({'pid': int(p.name), 'roots': matched})
    r['processReferences'] = references
    r['excludedOwnAncestorPids'] = sorted(ancestors)
    assert not references, 'Active references: no removal authorized yet'
    r['status'] = 'REMOVING'
    save()
    for path, branch, _ in trees:
        subprocess.run(['git', 'worktree', 'remove', '--force', str(path)], cwd=root, check=True, capture_output=True)
        assert not path.exists()
        subprocess.run(['git', 'branch', '-D', branch], cwd=root, check=True, capture_output=True)
        r['actions'].append({'path': str(path), 'branch': branch, 'removed': True, 'basis': 'Registered exact worktree/branch; all dirty bytes already in committed source; no untracked files'})
        save()
    for p in program_sets:
        removal(p, 'All22 original compiler artifact bytes match committed build report')
    removal(target, 'Exact owned build marker, original build runners, terminal tests/lints and no live reference; reproducible cache')
    removal(assets, 'All7 dist bytes equal retained exact development ZIP entries; owned marker')
    removal(package, 'Owned exact ZIP equals intentionally retained /mnt/data copy')
    removal(e45, 'All original bytes and owner marker match committed archive index')
    removal(e41, 'All original bytes match committed archive index; original absence of marker stated')
    r['status'] = 'PASS'
    r['retainedDevelopmentZipSha256'] = digest
    save()
    print(json.dumps({'status': 'PASS', 'removedRoots': len(r['actions']), 'signalsSent': 0}))
except Exception as error:
    r['status'] = 'FAIL_GUARD_OR_PARTIAL_CLEANUP'
    r['errorKind'] = type(error).__name__
    r['error'] = str(error)
    save()
    raise
