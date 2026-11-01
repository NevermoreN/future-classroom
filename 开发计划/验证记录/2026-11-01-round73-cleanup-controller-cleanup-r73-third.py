#!/usr/bin/env python3
"""Remove exactly three reviewed, terminal R73 scratch roots; never generate approval."""
from pathlib import Path, PurePosixPath
import argparse
import datetime
import gzip
import hashlib
import json
import os
import re
import shutil
import stat
import subprocess

MAIN = Path('/workspace/共享/Eliora/未来教室计划')
META = MAIN / '开发计划/验证记录'
ARCHIVE = META / '2026-11-01-round73-installer-broker'
ROOT = Path('/tmp/fc-round73-installer-broker')
WORK = ROOT / 'worktree'
EVIDENCE = ROOT / 'evidence'
CONTROL = Path('/tmp/fc-round73-archive-controller')
SELF = Path('/tmp/fc-round73-cleanup-controller')
BASE = '00120a026eea5dc4ff330f8526bf31169b373565'
BRANCH = 'codex/installer-broker'
OWNERS = {
    ROOT: b'future-classroom-owned-r73-installer-broker/v1\n',
    CONTROL: b'future-classroom-owned-r73-archive-controller/v1\n',
    SELF: b'future-classroom-owned-r73-cleanup-controller/v1\n',
}
FROZEN_REPORTS = {
    'b-implementation-freeze-first.json': 'bb49743c651cf0e8940fc039a0c3f54885ab1f0ea76f5dda66a149ed60888c2e',
    'n-implementation-freeze-third.json': 'fbcdee53b97badcdb323e22cfb0649e851798d8756d5552833cf46f959c24131',
    'root-main-integrated-source-first.json': '9f5c25eb103e3b38aa9b5cbf356767b3d73d01adaab47186b14ab39ec18f68fe',
    'msi-tools-preparation-first.json': 'd955b2b42162c2f037ba2b1b94fa64e10dbda895cb0e8d66e09b888c0bd8c229',
    'msi-tools-input-first.json': 'f2cedba8e8cff6dc4bce034861d92efaf8604475b993c965d0472645365bec12',
}


def need(value, message):
    if not value:
        raise AssertionError(message)


def sha(data):
    return hashlib.sha256(data).hexdigest()


def regular(path):
    scope = next((root for root in (ROOT, CONTROL, SELF, MAIN) if path.is_relative_to(root)), None)
    need(scope is not None, 'unapproved read scope')
    parent = path.parent
    while parent != scope:
        need(parent != parent.parent and stat.S_ISDIR(parent.lstat().st_mode), 'linked file ancestor')
        parent = parent.parent
    need(stat.S_ISDIR(scope.lstat().st_mode), 'linked read root')
    need(stat.S_ISREG(path.lstat().st_mode), 'nonregular file: ' + str(path))
    # Never open/hash persistent test access or any unknown private-key payload.
    need(path.suffix.lower() not in {'.key', '.pfx', '.p12'}, 'private filename: ' + str(path))
    return path.read_bytes()


def relative(root, value):
    parsed = PurePosixPath(value)
    need(value and not parsed.is_absolute() and '..' not in parsed.parts
         and parsed.as_posix() == value and '\\' not in value, 'unsafe relative path')
    path = root.joinpath(*parsed.parts)
    # Inspect each ancestor without resolving through a symlink.
    parent = path.parent
    while parent != root:
        need(parent != parent.parent and stat.S_ISDIR(parent.lstat().st_mode), 'linked/missing ancestor')
        parent = parent.parent
    need(stat.S_ISDIR(root.lstat().st_mode), 'linked root')
    return path


def identity(path, row):
    data = regular(path)
    need(len(data) == row['bytes'] and sha(data) == row['sha256'], 'identity mismatch: ' + str(path))
    return data


def git(*args, data=None):
    return subprocess.check_output(['git', *args], cwd=MAIN, input=data)


def read_json(path, expected_sha=None):
    data = regular(path)
    if expected_sha:
        need(sha(data) == expected_sha, 'report identity: ' + str(path))
    return json.loads(data)


def inventory(root):
    """lstat only; never traverse symlink directories or read their targets."""
    rows = []
    def visit(directory):
        for entry in sorted(os.scandir(directory), key=lambda x: x.name):
            path = Path(entry.path)
            mode = entry.stat(follow_symlinks=False).st_mode
            row = {'path': path.relative_to(root).as_posix()}
            if stat.S_ISLNK(mode):
                target = os.readlink(path)
                # Known Debian local links are allowed only when their terminal
                # target remains in the exact owned root. No link is followed.
                need(not os.path.isabs(target), 'absolute/outside link')
                lexical = Path(os.path.normpath(str(path.parent / target)))
                need(lexical.is_relative_to(root), 'outside link')
                row.update(kind='symlink', target=target)
            elif stat.S_ISDIR(mode):
                row['kind'] = 'directory'
                rows.append(row)
                visit(path)
                continue
            elif stat.S_ISREG(mode):
                data = regular(path)
                row.update(kind='file', bytes=len(data), sha256=sha(data))
            else:
                raise AssertionError('unknown filesystem object: ' + str(path))
            rows.append(row)
    visit(root)
    by_path = {row['path']: row for row in rows}
    for row in rows:
        if row['kind'] != 'symlink':
            continue
        path = root / row['path']
        target = Path(os.path.normpath(str(path.parent / row['target']))).relative_to(root).as_posix()
        seen = {row['path']}
        while by_path.get(target, {}).get('kind') == 'symlink':
            need(target not in seen, 'link cycle')
            seen.add(target)
            link = by_path[target]
            target = Path(os.path.normpath(str((root / target).parent / link['target']))).relative_to(root).as_posix()
        need(by_path.get(target, {}).get('kind') in {'file', 'directory'}, 'dangling/outside link')
    return sorted(rows, key=lambda row: row['path'])


def no_active_processes():
    ancestors = set()
    pid = os.getpid()
    while pid > 0 and pid not in ancestors:
        ancestors.add(pid)
        try:
            pid = int(Path(f'/proc/{pid}/stat').read_text().rsplit(')', 1)[1].split()[1])
        except FileNotFoundError:
            break
    needles = [os.fsencode(root) for root in OWNERS]
    def mentions(value):
        return any(value == n or value.startswith(n + b'/') for n in needles)
    active = []
    for proc in Path('/proc').iterdir():
        if not proc.name.isdigit() or int(proc.name) in ancestors:
            continue
        try:
            if proc.stat().st_uid != os.geteuid():
                continue
            args = (proc / 'cmdline').read_bytes()
            reasons = ['argv'] if any(n in args for n in needles) else []
            if mentions(os.fsencode(os.readlink(proc / 'cwd'))):
                reasons.append('cwd')
            for fd in (proc / 'fd').iterdir():
                try:
                    if mentions(os.fsencode(os.readlink(fd)).removesuffix(b' (deleted)')):
                        reasons.append('fd')
                        break
                except FileNotFoundError:
                    continue
            if reasons:
                active.append({'pid': int(proc.name), 'reasons': reasons})
        except (FileNotFoundError, ProcessLookupError):
            continue
        # Permission errors and malformed /proc metadata are not ignored.
    need(not active, 'active owned processes: ' + json.dumps(active))


def verify_committed(proof, commit):
    need(proof.get('status') in {'PASS_EXACT_COMMITTED_BLOBS', 'PASS_EXACT_COMMITTED_BYTES'}, 'unaccepted commit proof')
    need(proof.get('commit', proof.get('verifiedCommit')) == commit, 'commit proof mismatch')
    need(git('rev-parse', 'HEAD').decode().strip() == commit, 'main moved')
    need(not git('diff', '--cached', '--name-only', '-z'), 'main staged delta')
    rows = proof['files']
    by_path = {row['path']: row for row in rows}
    need(len(rows) == len(by_path), 'duplicate committed proof path')
    tree = {}
    for entry in git('ls-tree', '-r', '-z', commit).split(b'\0'):
        if entry:
            metadata, name = entry.split(b'\t', 1)
            _, kind, oid = metadata.split()
            if kind == b'blob':
                tree[name.decode()] = oid.decode()
    oids = sorted({r.get('gitBlobOid', r.get('gitBlob')) for r in rows})
    need(all(re.fullmatch(r'[0-9a-f]{40}', oid or '') for oid in oids), 'invalid blob OID')
    raw = git('cat-file', '--batch', data=('\n'.join(oids) + '\n').encode())
    blobs = {}; offset = 0
    for oid in oids:
        end = raw.index(b'\n', offset)
        actual, kind, size = raw[offset:end].decode().split()
        need(actual == oid and kind == 'blob', 'not an exact committed blob')
        start = end + 1; size = int(size)
        blobs[oid] = raw[start:start + size]
        need(raw[start + size:start + size + 1] == b'\n', 'batch blob boundary')
        offset = start + size + 1
    need(offset == len(raw), 'trailing batch bytes')
    for path, row in by_path.items():
        oid = row.get('gitBlobOid', row.get('gitBlob'))
        need(tree.get(path) == oid, 'blob not at exact commit path')
        data = blobs[oid]
        need(len(data) == row['bytes'] and sha(data) == row['sha256'], 'committed blob identity')
        need(regular(relative(MAIN, path)) == data, 'main differs from committed bytes')
    return by_path


def verify_sources(committed):
    reports = {name: read_json(EVIDENCE / name, digest) for name, digest in FROZEN_REPORTS.items()}
    b = reports['b-implementation-freeze-first.json']['ownedSource']
    n = reports['n-implementation-freeze-third.json']['source']
    integrated = reports['root-main-integrated-source-first.json']
    changed = [r for r in integrated['copiedGuardInputs'] if r['changedFromBase']]
    need(len(changed) == 15 and integrated['changedSourceCount'] == 15, 'exact fifteen integrated sources')
    by_path = {r['path']: r for r in changed}
    need(set(by_path) <= committed.keys(), 'fifteen integrated source blobs not in committed proof')
    need(len(b) == len(n) == 7 and len({r['path'] for r in b + n}) == 14, 'B7/N3 boundaries')
    for row in b + n:
        need(by_path[row['path']]['sha256'] == row['sha256'], 'freeze/main integration disagreement')
    need(set(by_path) - {r['path'] for r in b + n} == {'crates/fc-bootstrap/src/application/web_editor.rs'}, 'unknown fifteenth source')
    for row in changed:
        need(identity(relative(WORK, row['path']), row) == identity(relative(MAIN, row['path']), row), 'main/work source differs')
    build = integrated['currentBuildRelatedSources']
    need(len(build) == integrated['currentBuildRelatedSourceCount'] == 548, 'original build guard count')
    for row in build:
        need(identity(relative(WORK, row['path']), row) == identity(relative(MAIN, row['path']), row), 'build input differs')
    need(git('-C', str(WORK), 'rev-parse', 'HEAD').decode().strip() == BASE, 'worktree base changed')
    need(git('rev-parse', 'refs/heads/' + BRANCH).decode().strip() == BASE, 'branch moved')
    need(git('-C', str(WORK), 'branch', '--show-current').decode().strip() == BRANCH, 'wrong branch')
    need(not git('-C', str(WORK), 'diff', '--cached', '--name-only', '-z'), 'staged worktree delta')
    dirty = git('-C', str(WORK), 'diff', '--name-only', '-z') + git('-C', str(WORK), 'ls-files', '--others', '--exclude-standard', '-z')
    need({p.decode() for p in dirty.split(b'\0') if p} == set(by_path), 'unknown worktree delta')
    need(not git('-C', str(WORK), 'ls-files', '--others', '--ignored', '--exclude-standard', '-z'), 'unknown ignored worktree data')
    return reports


def verify_tools(reports):
    tools = ROOT / 'tools'
    need(regular(tools / '.owner') == b'future-classroom-owned-r73-msi-tools/v1\n', 'tools owner')
    report = reports['msi-tools-preparation-first.json']
    packages = reports['msi-tools-input-first.json']['packages']
    need(report['status'] == 'PASS_EXACT_PACKAGES_LOCAL_EXTRACTION_ELF_BINDING_AND_SCOPED_TOOL_LOADING_ONLY', 'tools preparation not passed')
    need(len(report['packages']) == len(packages) == 7, 'exact seven packages')
    need({r['name'] for r in report['packages']} == {r['name'] for r in packages}, 'package set differs')
    for row in report['packages']:
        original = next(r for r in packages if r['name'] == row['name'])
        need(all(row[k] == original[k] for k in ('version', 'bytes', 'sha256')), 'package provenance differs')
        path = Path(row['download']['path'])
        need(path.parent == tools / 'packages', 'package outside owned root')
        identity(path, row)
        download = read_json(EVIDENCE / ('msi-tools-download-' + row['name'] + '-first.json'))
        need(download['status'] == 'PASS_EXACT_PUBLISHED_BYTES_AND_SHA256' and download['actualExit']
             and download['exitCode'] == 0 and not download['controllerTimedOut'], 'download not terminal/pass')
        need(download['downloaded'] == row['download'], 'download identity mismatch')
        extraction = next(c for c in report['commands'] if c['name'] == 'extract-' + row['name'])
        need(extraction['actualExit'] and extraction['exitCode'] == 0 and not extraction['controllerTimedOut'], 'extraction not terminal/pass')
        need(extraction['command'] == ['/usr/bin/dpkg-deb', '-x', str(path), str(tools / 'root')], 'extraction escaped root')
        for log in (extraction['stdout'], extraction['stderr']):
            path = Path(log['path'])
            need(path.is_relative_to(EVIDENCE), 'tool log outside evidence')
            identity(path, log)
    identity(Path(report['wixlWrapper']['path']), report['wixlWrapper'])


def verify_archive(index, committed):
    need(index['status'] == 'ORIGINAL_BYTES_ARCHIVED' and Path(index['sourceRoot']) == EVIDENCE, 'archive identity')
    omitted = index['excludedCompiledPayloads']
    need(len(omitted) == 24 and all(r['path'].startswith('producer-n2-compiled-originals-first/') and r['path'].endswith('.exe') for r in omitted), 'unexpected exclusions')
    need(regular(EVIDENCE / '.owner') == b'future-classroom-owned-r73-installer-broker-evidence/v1\n', 'evidence owner')
    expected = {r['path'] for r in index['files'] + omitted}
    actual = {r['path'] for r in inventory(EVIDENCE) if r['kind'] == 'file'}
    need(actual == expected, 'unknown evidence delta')
    for row in index['files']:
        original = identity(relative(EVIDENCE, row['path']), row)
        stored = relative(ARCHIVE, row.get('archivePath', row['path']))
        need(stored.relative_to(MAIN).as_posix() in committed, 'archive not committed')
        data = regular(stored)
        if row.get('storage') == 'gzip_original_bytes':
            need(len(data) == row['storedBytes'] and sha(data) == row['storedSha256'], 'gzip stored identity')
            data = gzip.decompress(data)
        need(data == original, 'source/archive original bytes differ')
    retention = read_json(EVIDENCE / 'producer-n2-compiled-originals-first/retention-first.json')
    need(len(retention['files']) == 24, 'N2 retained binary count')
    retained = {str(Path(r['retainedPath']).relative_to(EVIDENCE)): r for r in retention['files']}
    need(set(retained) == {r['path'] for r in omitted}, 'N2 exclusion set')
    for row in omitted:
        data = identity(relative(EVIDENCE, row['path']), row)
        need(data.startswith(b'MZ') and retained[row['path']]['sha256'] == row['sha256']
             and retained[row['path']]['bytes'] == row['bytes'], 'N2 original PE identity')
    receipt = index['verificationReceipt']
    identity(relative(ARCHIVE, receipt['path']), receipt)
    expected_archive = {r.get('archivePath', r['path']) for r in index['files']} | {'archive-index-first.json', receipt['path']}
    need({r['path'] for r in inventory(ARCHIVE) if r['kind'] == 'file'} == expected_archive, 'unknown archive delta')
    return len(index['files'])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--plan', type=Path, required=True)
    parser.add_argument('--expected-plan-sha256', required=True)
    parser.add_argument('--committed-proof', type=Path, required=True)
    parser.add_argument('--expected-proof-sha256', required=True)
    parser.add_argument('--commit', required=True)
    parser.add_argument('--execute', action='store_true')
    args = parser.parse_args()
    for value in (args.expected_plan_sha256, args.expected_proof_sha256):
        need(re.fullmatch(r'[0-9a-f]{64}', value), 'explicit SHA256 required')
    need(re.fullmatch(r'[0-9a-f]{40}', args.commit), 'explicit full commit required')
    if not args.execute:
        print(json.dumps({'status': 'PREPARED_ONLY_NOT_EXECUTED', 'removalPerformed': False}))
        return
    for path in (args.plan, args.committed_proof):
        need(path.is_relative_to(META), 'approval/proof must be in repository records')
    plan = read_json(args.plan, args.expected_plan_sha256)
    need(plan['status'] == 'ROOT_REVIEWED_TERMINAL_R73_CLEANUP_PLAN' and plan['rootConfirmedAllControllersTerminal'] is True, 'root final review missing')
    committed = verify_committed(read_json(args.committed_proof, args.expected_proof_sha256), args.commit)
    need(args.plan.relative_to(MAIN).as_posix() in committed, 'plan not committed')
    roots = {Path(r['path']): r for r in plan['roots']}
    need(set(roots) == set(OWNERS), 'exact three owned roots required')
    for root, marker in OWNERS.items():
        need(stat.S_ISDIR(root.lstat().st_mode) and regular(root / '.owner') == marker, 'owner mismatch')
        need(inventory(root) == roots[root]['entries'], 'unknown final inventory delta: ' + str(root))
    host = read_json(EVIDENCE / 'n-host-first.json')
    need(host['scratchRoot'] == str(ROOT / 'host-scratch-first')
         and host['scratchCleanup'] == 'PASS_ONLY_EXACT_OWNED_MARKER_AND_EMPTY_ROOT_REMOVED', 'Host scratch cleanup not proved')
    need(not os.path.lexists(ROOT / 'host-scratch-first'), 'Host scratch unexpectedly remains')
    need({r['path'].split('/')[0] for r in roots[ROOT]['entries']} == {'.owner', 'evidence', 'worktree', 'target', 'tools'}, 'unknown root child')
    for terminal in plan['terminalReports']:
        path = relative(EVIDENCE, terminal['path'])
        value = read_json(path, terminal['sha256'])
        need(value.get('status') == terminal['status'], 'terminal status differs from original')
        if terminal['path'] == 'n-host-first.json' and terminal['status'] == 'FAIL':
            need(terminal['sha256'] == 'da0715d485985841af18f274ba28a6db9425f0edcc596877c392d206f89db036', 'only exact original Host failure is qualified')
            need(value['sourceUnchanged'] is True and value['sourceBefore'] == value['sourceAfter']
                 and value['testHarnessCount'] == 22 and value['totalPassed'] == 989
                 and value['scratchCleanup'] == 'PASS_ONLY_EXACT_OWNED_MARKER_AND_EMPTY_ROOT_REMOVED', 'Host failure shape changed')
            need(len(value['steps']) == 2, 'Host original two steps required')
            for step, expected in zip(value['steps'], [('tests', 'PASS', 0), ('strict-clippy', 'FAIL', 101)]):
                need((step['name'], step['status'], step['exitCode']) == expected
                     and step['actualExit'] is True and step['timedOut'] is False, 'Host original actual exits not proved')
            attribution = plan['hostFailureAttribution']
            need(attribution['path'] == 'n-host-qualification-first.json'
                 and attribution['sha256'] == 'c2883954cb93bea75730c95a42b56430fe1759b162b078ab1648ce60089ad6d1', 'exact readonly Host attribution required')
            qualified = read_json(relative(EVIDENCE, attribution['path']), attribution['sha256'])
            need(qualified['status'] == 'HOST_TEST_PASS_HOST_STRICT_CLIPPY_FAIL_PREEXISTING_WINDOWS_CALLER_CFG'
                 and qualified['originalReport'] == {'path': terminal['path'], 'sha256': terminal['sha256']}
                 and qualified['sourceUnchangedSixteenInputs'] is True
                 and qualified['scratchRootAbsent'] is True
                 and len(qualified['primaryDiagnostics']) == 6 and len(qualified['diagnosticFiles']) == 4, 'Host attribution does not match original six diagnostics')
            producer = read_json(EVIDENCE / 'n-host-producer-terminal-first.json',
                                 '35043829f5a53c286c8c9564e696865de2c8438fb755043208882eb5a23c9779')
            need(producer['status'] == 'FAIL_HOST_CLIPPY_CONTROLLER_ACTUAL_EXIT1_REAPED'
                 and producer['report'] == qualified['originalReport']
                 and qualified['actualParentTerminal'] == {'path': 'n-host-producer-terminal-first.json', 'sha256': '35043829f5a53c286c8c9564e696865de2c8438fb755043208882eb5a23c9779'}
                 and producer['actualToolTerminal']['exitCode'] == 1
                 and producer['actualToolTerminal']['runningSessionReturned'] is False, 'Host original producer not physically terminal')
        else:
            need(terminal['status'].startswith('PASS')
                 and not any(x in terminal['status'] for x in ('RUNNING', 'PREPARED', 'PENDING', 'INTERRUPTED')), 'nonterminal final controller')
    need(any(row['path'] == 'n-host-first.json' and row['status'] == 'FAIL' for row in plan['terminalReports']), 'original Host FAIL must be honestly listed and qualified')
    need(len({row['path'] for row in plan['terminalReports']}) >= 3, 'finite acceptance, build, host terminal reports required')
    reports = verify_sources(committed)
    verify_tools(reports)
    archive_files = verify_archive(read_json(ARCHIVE / 'archive-index-first.json', plan['archiveIndexSha256']), committed)
    copies = plan['controllerCopies']
    for control in (CONTROL, SELF):
        expected = {r['path'] for r in roots[control]['entries'] if r['kind'] == 'file'}
        mapping = {r['source']: r['archived'] for r in copies if Path(r['source']).is_relative_to(control)}
        need(set(mapping) == {str(control / p) for p in expected}, 'controller originals not fully mapped')
        for original, archived in mapping.items():
            need(archived in committed and regular(Path(original)) == regular(relative(MAIN, archived)), 'controller copy not committed/exact')
    no_active_processes()
    # Recheck final reviewed membership before the first mutation. No self-made
    # inventory is automatically accepted. No process termination is attempted.
    for root in OWNERS:
        need(inventory(root) == roots[root]['entries'], 'late unknown delta')
    receipt = META / '2026-11-01-round73-cleanup-first.json'
    need(not receipt.exists(), 'receipt already exists')
    actions = []
    status = 'FAIL_PARTIAL_CLEANUP_RETAIN_REMAINDER'
    try:
        for command in (['git', 'worktree', 'remove', '--force', str(WORK)], ['git', 'branch', '-D', BRANCH]):
            result = subprocess.run(command, cwd=MAIN, capture_output=True, timeout=120)
            actions.append({'command': command, 'exitCode': result.returncode,
                            'stdout': result.stdout.decode('utf-8', 'backslashreplace'), 'stderr': result.stderr.decode('utf-8', 'backslashreplace')})
            need(result.returncode == 0, 'Git cleanup failed; retain all other roots')
        for root in (ROOT, CONTROL, SELF):
            need(regular(root / '.owner') == OWNERS[root], 'owner changed before removal')
            shutil.rmtree(root)  # symlinks are unlinked; never followed.
            need(not root.exists(), 'owned root remains')
            actions.append({'removedRoot': str(root)})
        status = 'PASS_R73_TERMINAL_OWNED_ROOTS_REMOVED_AFTER_EXACT_COMMITTED_PROOF'
    finally:
        with receipt.open('x') as stream:
            json.dump({'schemaVersion': 1, 'status': status, 'atUtc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
                       'commit': args.commit, 'planSha256': args.expected_plan_sha256,
                       'committedProofSha256': args.expected_proof_sha256, 'archiveOriginalsReplayed': archive_files,
                       'excludedOriginalPeIdentitiesVerified': 24, 'integratedSourcesVerified': 15, 'buildInputsVerified': 548,
                       'actions': actions, 'preserved': 'Main target/global caches/three development ZIPs/Windows SSH service and authorization/external private key; no key content read.',
                       'physicalDiskFreed': 'NOT_MEASURED'}, stream, ensure_ascii=False, indent=2)
            stream.write('\n')
    print(json.dumps({'status': status, 'receipt': str(receipt)}))


if __name__ == '__main__':
    main()
