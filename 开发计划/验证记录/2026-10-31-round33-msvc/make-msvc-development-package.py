#!/usr/bin/env python3
"""Prepare a reproducible MSVC development ZIP only after exact original evidence.

Local packaging only when explicitly invoked by the coordinator. Never executes
an EXE, accesses Windows/network/Git, or upgrades release/compatibility gates.
Existing paths are rejected; any failure leaves originals and partial work.
"""
from __future__ import annotations
import binascii
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import signal
import stat
import subprocess
import sys
import time
import uuid
import zipfile

REPO = Path('/workspace/共享/Eliora/未来教室计划')
EVIDENCE = Path('/tmp/fc-round33-msvc-evidence')
REPORT = EVIDENCE / 'development-package-first.json'
NORMAL = Path('/tmp/fc-round33-msvc-programs/fc-bootstrap.exe')
OLD = Path('/mnt/data/FutureClassroom-0.1.0-request-deadline-dev.zip')
OLD_SHA = '1acdec0c1506db0484c0a4ba8e2bfa7187ab26d485285128b6dd35ad9e114c69'
OLD_BYTES = 5960978
OUTPUT = Path('/mnt/data/FutureClassroom-0.1.0-msvc-request-deadline-dev.zip')
WEB = Path('/tmp/fc-round33-web-dist')
REPACK = Path('/tmp/fc-round33-msvc-repack')
SOURCE = '9d42f7cc75c7d2545caa18366d4a6d047aedaa99'
TOOLCHAIN = '1.97.1-x86_64-pc-windows-msvc'
TARGET = 'i686-pc-windows-msvc'
MIB = 1024 * 1024
WEB_NAMES = {
    'web/THIRD-PARTY-NOTICES.txt', 'web/assets/favicon-CXuYoSfB.svg',
    'web/assets/index-C3GlEF2q.js', 'web/assets/index-DFFVdc8-.css',
    'web/assets/pcm-worklet-B7E4LRoP.js', 'web/assets/wav-worker-LSywWbpJ.js',
    'web/fc-assets.json', 'web/index.html',
}
NATIVE_NAMES = {'fc-bootstrap.exe', 'LICENSE', 'PROBE-README.txt', 'THIRD-PARTY-NOTICES.txt'}
ALL_NAMES = WEB_NAMES | NATIVE_NAMES | {'manifest.json'}
GATES = {name: 'NOT_VALIDATED' for name in ('Win7_SP1_x86', 'Windows_10', 'Windows_11_complete_product', 'formal_MSVC_baseline', 'real_machine_acceptance')}
GATES['release_tag'] = 'NOT_ALLOWED'
MARKER = '.fc-round33-packaging-owner'

class Rejected(Exception):
    pass

def require(ok, detail):
    if not ok:
        raise Rejected(detail)

def sha(data):
    return hashlib.sha256(data).hexdigest()

def regular_bytes(path, limit):
    info = path.lstat()
    require(stat.S_ISREG(info.st_mode) and not path.is_symlink() and 0 <= info.st_size <= limit, 'Expected bounded regular input file')
    descriptor = os.open(path, os.O_RDONLY | getattr(os, 'O_NOFOLLOW', 0))
    with os.fdopen(descriptor, 'rb') as stream:
        opened = os.fstat(stream.fileno())
        require(stat.S_ISREG(opened.st_mode) and opened.st_dev == info.st_dev and opened.st_ino == info.st_ino, 'Input identity changed while opening')
        data = stream.read(limit + 1)
        require(len(data) == info.st_size and len(data) <= limit, 'Input size changed or exceeded bound')
    return data

def fact(path, limit=128*MIB):
    data = regular_bytes(path, limit)
    return {'path': str(path), 'bytes': len(data), 'sha256': sha(data)}

def read_json(data):
    def unique(pairs):
        out = {}
        for key, value in pairs:
            require(key not in out, 'Duplicate JSON field')
            out[key] = value
        return out
    return json.loads(data.decode('utf-8-sig'), object_pairs_hook=unique)

def new_bytes(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open('xb') as stream:
        stream.write(data)

def archive_members(path):
    data = regular_bytes(path, 128*MIB)
    members, facts, total = {}, [], 0
    with zipfile.ZipFile(io.BytesIO(data)) as archive:
        infos = archive.infolist()
        require(len(infos) == 13 and {i.filename for i in infos} == ALL_NAMES, 'Development ZIP must contain exact13 members')
        require(len({i.filename.casefold() for i in infos}) == 13, 'Duplicate/case-aliased ZIP member')
        for info in infos:
            name = info.filename
            require(info.orig_filename == name and not info.is_dir() and not info.flag_bits & 1, 'Invalid/encrypted ZIP member')
            mode = (info.external_attr >> 16) & 0xffff
            require(stat.S_IFMT(mode) in (0, stat.S_IFREG), 'ZIP symlink/special file rejected')
            require(0 <= info.file_size <= 128*MIB, 'ZIP member size bound')
            total += info.file_size
            require(total <= 160*MIB, 'Development ZIP logical total bound')
            content = archive.read(info)  # zipfile verifies original CRC at EOF.
            require(len(content) == info.file_size and binascii.crc32(content) & 0xffffffff == info.CRC, 'Member bytes/CRC mismatch')
            require(name not in members, 'Duplicate ZIP member')
            members[name] = content
            facts.append({'path': name, 'bytes': len(content), 'sha256': sha(content), 'crc32': f'{info.CRC:08x}'})
    manifest = read_json(members['manifest.json'])
    require(manifest['status'] == 'NOT_VALIDATED' and manifest['artifact_type'] == 'development-probe', 'Development status upgraded')
    require(manifest['features']['complete_application'] == 'NOT_IMPLEMENTED' and manifest['gates'] == GATES, 'Full application/release/compatibility gates upgraded')
    entries = manifest['files']
    require(isinstance(entries, list) and len(entries) == 12, 'Manifest must describe12 payloads')
    seen = set()
    for entry in entries:
        name = entry['path']
        require(name in members and name != 'manifest.json' and name not in seen, 'Invalid/duplicate payload ledger member')
        seen.add(name)
        require(entry['size'] == len(members[name]) and entry['sha256'] == sha(members[name]), 'Payload manifest bytes/SHA mismatch')
    require(seen == ALL_NAMES - {'manifest.json'}, 'Payload manifest coverage incomplete')
    frontend = manifest['frontend']
    require(frontend['included'] is True and frontend['status'] == 'PARTIAL_NOT_VALIDATED' and frontend['asset_count'] == 6, 'Frontend scope upgraded/missing')
    require(frontend['manifest_sha256'] == sha(members['web/fc-assets.json']), 'Static manifest identity mismatch')
    static = read_json(members['web/fc-assets.json'])
    require(all(type(static[key]) is int and static[key] == 1 for key in ('schemaVersion', 'apiVersion', 'uiCompatibilityVersion')), 'Static version changed')
    declared = set()
    for asset in static['assets']:
        name = 'web/' + asset['path']
        require(name in WEB_NAMES and name not in declared, 'Static manifest path outside frozen allowlist')
        declared.add(name)
        require(asset['sizeBytes'] == len(members[name]) and asset['sha256'] == sha(members[name]), 'Static asset bytes/SHA mismatch')
    require(declared | {'web/fc-assets.json', 'web/THIRD-PARTY-NOTICES.txt'} == WEB_NAMES, 'Static manifest does not cover exact frozen8 Web members')
    return data, members, facts, manifest

def create_owned_root(path, token):
    require(not path.exists() and not path.is_symlink(), 'Temporary packaging root already exists')
    path.mkdir(exist_ok=False)
    new_bytes(path / MARKER, token.encode('ascii'))
    info = path.lstat()
    return {'path': str(path), 'device': info.st_dev, 'inode': info.st_ino, 'markerSha256': sha(token.encode('ascii'))}

def remove_owned_root(path, identity, expected_files, token):
    require(path in (WEB, REPACK) and str(path) == identity['path'], 'Cleanup path outside exact two owned roots')
    info = path.lstat()
    require(stat.S_ISDIR(info.st_mode) and not path.is_symlink() and info.st_dev == identity['device'] and info.st_ino == identity['inode'], 'Cleanup root identity changed')
    require(regular_bytes(path / MARKER, 128) == token.encode('ascii'), 'Cleanup capability marker mismatch')
    files, directories = {}, []
    for item in path.rglob('*'):
        info = item.lstat()
        require(not item.is_symlink() and (stat.S_ISREG(info.st_mode) or stat.S_ISDIR(info.st_mode)), 'Cleanup contains aliased/special entry')
        if stat.S_ISREG(info.st_mode):
            relative = item.relative_to(path).as_posix()
            files[relative] = fact(item)
        else:
            directories.append(item)
    require(set(files) == set(expected_files) | {MARKER}, 'Cleanup would remove unexpected owned-root content')
    for relative, expected in expected_files.items():
        require(files[relative]['sha256'] == expected['sha256'] and files[relative]['bytes'] == expected['bytes'], 'Cleanup file changed after verification')
    for relative in sorted(files):
        if relative != MARKER:
            (path / relative).unlink()
    for directory in sorted(directories, key=lambda p: len(p.parts), reverse=True):
        directory.rmdir()
    (path / MARKER).unlink()
    path.rmdir()
    require(not path.exists() and not path.is_symlink(), 'Owned temporary packaging root remains')
    return {'path': str(path), 'removed': True, 'removedFileCount': len(files), 'originalIdentity': identity}

def logged_package(output, label, report, overall_deadline):
    command = [sys.executable, '-B', 'scripts/package-probe.py', '--exe', str(NORMAL), '--output', str(output), '--target', TARGET, '--toolchain', TOOLCHAIN, '--web-dist', str(WEB)]
    stdout_path = EVIDENCE / f'development-package-first.{label}.stdout.log'
    stderr_path = EVIDENCE / f'development-package-first.{label}.stderr.log'
    require(not stdout_path.exists() and not stderr_path.exists(), 'Original packaging log path exists')
    remaining = min(180.0, overall_deadline - time.monotonic())
    require(remaining > 0, 'Original local packaging budget exhausted')
    record = {'command': command, 'cwd': str(REPO), 'label': label, 'timeoutSeconds': remaining, 'exitCode': None, 'timedOut': False}
    report['commands'].append(record)
    child = None
    start = time.monotonic()
    # Private session permits exact whole-group cleanup of this packager and its
    # own objdump/rustc children on timeout; no unrelated process-name matching.
    with stdout_path.open('xb') as out, stderr_path.open('xb') as err:
        try:
            child = subprocess.Popen(command, cwd=REPO, stdout=out, stderr=err, start_new_session=True)
            record['pid'] = child.pid
            record['exitCode'] = child.wait(timeout=remaining)
        except subprocess.TimeoutExpired:
            record['timedOut'] = True
            if child is not None:
                os.killpg(child.pid, signal.SIGKILL)
                record['exitCode'] = child.wait(timeout=10)
        finally:
            record['durationSeconds'] = time.monotonic() - start
    record['stdout'] = fact(stdout_path, 16*MIB)
    record['stderr'] = fact(stderr_path, 16*MIB)
    require(not record['timedOut'] and record['exitCode'] == 0, 'Packaging subcommand failed or timed out; original logs retained')


def prepare(report):
    report['phase'] = 'original-evidence-gates'
    original_reports = {}
    for name in ('artifact-verification-first.json', 'windows-msvc-full-first.json', 'windows-msvc-tls-first.json'):
        path = EVIDENCE / name
        data = regular_bytes(path, 32*MIB)
        original_reports[name] = read_json(data)
        report['inputReports'].append({'path': str(path), 'bytes': len(data), 'sha256': sha(data)})
    verified = original_reports['artifact-verification-first.json']
    full = original_reports['windows-msvc-full-first.json']
    tls = original_reports['windows-msvc-tls-first.json']
    require(isinstance(verified['status'], str) and verified['status'].startswith('PASS_') and verified['sourceSha'] == SOURCE, 'Exact original MSVC artifact not verified')
    require(full['status'] == 'PASS' and full['cleanup'] == 'PASS' and tls['status'] == 'PASS' and tls['cleanup'] == 'PASS', 'Windows runtime/TLS status or cleanup not PASS')
    require(tls['cleanupDetails']['status'] == 'PASS' and tls['cleanupDetails']['rootAbsent'] is True and tls['cleanupDetails']['ownedProcessesRemaining'] == 0 and tls['cleanupDetails']['myDigestRestored'] is True and tls['cleanupDetails']['cngKeyExists'] is False, 'TLS process/certificate/key/root cleanup incomplete')
    require(tls['ownedProcessCleanup']['status'] == 'PASS', 'TLS actual owned process stop not proved')
    native = tls['native']
    require(native['physicalNativeDrain'] is True and native['sharedWorkers'] == 2 and native['workerJoin'] is True and native['hubPending'] == 0 and native['downloadHubPending'] == 0 and native['quarantined'] == 0 and native['setupCleanup'] is True, 'TLS actual native/pool/hub/setup cleanup gate failed')
    normal_rows = [row for row in verified['binaryFacts'] if row['role'] == 'product']
    require(len(normal_rows) == 1 and normal_rows[0]['path'] == str(NORMAL) and normal_rows[0]['executed'] is False, 'Verified normal EXE path/compile-only flag mismatch')
    normal = regular_bytes(NORMAL, 128*MIB)
    normal_sha = sha(normal)
    require(normal_rows[0]['sha256'] == normal_sha and normal_rows[0]['bytes'] == len(normal) and full['exeSha256'] == normal_sha == tls['exeSha256'], 'Normal EXE/verifier/two runtime reports have different identities')
    uploaded = [row for row in full['uploadedArtifacts'] if row['name'] == NORMAL.name]
    require(len(uploaded) == 1 and uploaded[0]['sha256'] == normal_sha, 'Full runtime uploaded normal EXE identity mismatch')
    require(len(full['tests']) == 21 and all(t['exitCode'] == 0 for t in full['tests']), 'Full runtime must contain21 successful harnesses')
    report['normalExecutable'] = {'path': str(NORMAL), 'bytes': len(normal), 'sha256': normal_sha, 'verifierExecutionFlag': False, 'windowsRuntimeReportsBindSameSha': True, 'packagerExecutesExecutable': False}
    source_paths = ['scripts/package-probe.py', 'scripts/audit-pe.py', 'packaging/PROBE-README.txt', 'packaging/THIRD-PARTY-NOTICES.txt', 'LICENSE', 'web/THIRD-PARTY-NOTICES.txt', 'scripts/windows-probe.py', 'scripts/windows-tls-business-probe.py']
    source_facts = {p: fact(REPO / p, 4*MIB) for p in source_paths}
    report['localSourceBindings'] = source_facts
    require(not OUTPUT.exists() and not OUTPUT.is_symlink() and not WEB.exists() and not WEB.is_symlink() and not REPACK.exists() and not REPACK.is_symlink(), 'Package output or temporary root already exists; refusing replacement')
    report['phase'] = 'frozen-original-web-package'
    old_data = regular_bytes(OLD, 128*MIB)
    require(len(old_data) == OLD_BYTES and sha(old_data) == OLD_SHA, 'Original Web source development ZIP bytes/SHA mismatch')
    require(tls['webBundleSha256'] == OLD_SHA, 'TLS static fixture did not use the exact frozen original Web package')
    _, old_members, old_member_facts, old_manifest = archive_members(OLD)
    report['reusedWebSource'] = {'path': str(OLD), 'bytes': len(old_data), 'sha256': OLD_SHA, 'memberCount': len(old_member_facts), 'webFiles': [f for f in old_member_facts if f['path'] in WEB_NAMES], 'extractionScope': 'Only these exact eight web members are copied; old GNU EXE and other native payloads are never extracted.'}
    require(regular_bytes(REPO / 'web/THIRD-PARTY-NOTICES.txt', MIB) == old_members['web/THIRD-PARTY-NOTICES.txt'], 'Current Web notice differs from frozen tested Web notice')
    token = 'round33-msvc-development-package/' + uuid.uuid4().hex
    roots = {}
    report['ownedTemporaryRoots'] = roots
    for path in (WEB, REPACK):
        roots[str(path)] = create_owned_root(path, token)
    web_files = {}
    for name in sorted(WEB_NAMES):
        relative = name[len('web/'):]
        require(PurePosixPath(relative).as_posix() == relative and '..' not in PurePosixPath(relative).parts, 'Unsafe frozen Web relative path')
        new_bytes(WEB.joinpath(*PurePosixPath(relative).parts), old_members[name])
        web_files[relative] = {'bytes': len(old_members[name]), 'sha256': sha(old_members[name])}
    report['phase'] = 'two-independent-package-commands'
    deadline = time.monotonic() + 420.0
    logged_package(OUTPUT, 'package', report, deadline)
    second = REPACK / OUTPUT.name
    logged_package(second, 'repack', report, deadline)
    for path, before in source_facts.items():
        require(fact(REPO / path, 4*MIB) == before, 'Packager or fixed source/notice changed during packaging')
    require(regular_bytes(NORMAL, 128*MIB) == normal, 'Normal EXE changed during packaging')
    report['phase'] = 'independent-zip-and-payload-validation'
    first_data, members, rows, manifest = archive_members(OUTPUT)
    second_data, second_members, second_rows, second_manifest = archive_members(second)
    require(first_data == second_data and rows == second_rows and manifest == second_manifest, 'Independent same-basename repack not byte-identical')
    require(manifest['artifact_name'] == OUTPUT.name and manifest['target'] == TARGET and manifest['target_role'] == 'modern-MSVC-development-only' and manifest['toolchain']['declared_build_toolchain'] == TOOLCHAIN, 'Package target/toolchain/name metadata mismatch')
    require(manifest['sha256'] == normal_sha and members['fc-bootstrap.exe'] == normal, 'Packaged normal EXE differs from exact tested normal executable')
    require(all(members[name] == old_members[name] == second_members[name] for name in WEB_NAMES), 'One of8 tested Web members changed')
    expected_native = {'LICENSE': REPO / 'LICENSE', 'PROBE-README.txt': REPO / 'packaging/PROBE-README.txt', 'THIRD-PARTY-NOTICES.txt': REPO / 'packaging/THIRD-PARTY-NOTICES.txt'}
    require(all(members[name] == regular_bytes(path, 4*MIB) for name, path in expected_native.items()), 'Fixed native notice/README bytes differ from current packager inputs')
    require('--tls-business-probe <40hex-current-user-MY-thumbprint> <FutureClassroom-Test-TLS-32hex>' in manifest['features']['declared_probe_commands'], 'Finite TLS business command missing from development metadata')
    report.update({'package': {'path': str(OUTPUT), 'bytes': len(first_data), 'sha256': sha(first_data)}, 'archiveMembers': rows, 'manifest': manifest, 'repackByteIdentical': True, 'repack': {'path': str(second), 'bytes': len(second_data), 'sha256': sha(second_data)}, 'unchangedWebMemberCount': 8, 'payloadCount': 12, 'packageChecks': 'PASS; cleanup not yet performed'})
    report['phase'] = 'exact-owned-temporary-root-cleanup'
    report['temporaryRootCleanup'].append(remove_owned_root(WEB, roots[str(WEB)], web_files, token))
    repack_expected = {OUTPUT.name: {'bytes': len(second_data), 'sha256': sha(second_data)}}
    report['temporaryRootCleanup'].append(remove_owned_root(REPACK, roots[str(REPACK)], repack_expected, token))
    require(not WEB.exists() and not REPACK.exists() and OUTPUT.is_file(), 'Post-cleanup final paths do not match scope')
    report['temporaryPackagingRootsRemoved'] = True
    report['status'] = 'PASS_DEVELOPMENT_PACKAGE_ONLY'
    report['phase'] = 'complete'


def main():
    # Reserve original first-report filename before doing anything else. Never
    # overwrite an earlier failed/successful report or package/temporary roots.
    with REPORT.open('xb') as output:
        report = {'status': 'FAIL', 'phase': 'initial', 'scope': 'Exact original modern MSVC CI artifact plus actual authorized Win11 full-suite/finite TLS evidence, repackaged only as an unvalidated development probe. No EXE execution by this helper; no network/Windows/Git calls. Win7/Win10/complete product, strict TCP5s and release gates remain unchanged.', 'helperSource': fact(Path(__file__).resolve(), MIB), 'inputReports': [], 'commands': [], 'temporaryRootCleanup': [], 'temporaryPackagingRootsRemoved': False, 'error': None}
        try:
            prepare(report)
        except Exception as error:
            report['error'] = {'kind': type(error).__name__, 'detail': str(error) if isinstance(error, Rejected) else 'Local preparation/packaging exception; originals retained, external exception text omitted.'}
            report['failurePolicy'] = 'No automatic cleanup/retry/overwrite on failure. Preserve owned temporary roots, any output left by the existing packager, and raw subcommand logs for coordinator review. Existing package-probe.py may itself remove its own incomplete ZIP on a packaging error; this helper performs no failure cleanup.'
        finally:
            output.write((json.dumps(report, ensure_ascii=False, indent=2) + '\n').encode('utf-8'))
    print(json.dumps({'status': report['status'], 'report': str(REPORT), 'package': report.get('package'), 'temporaryPackagingRootsRemoved': report['temporaryPackagingRootsRemoved'], 'errorKind': (report['error'] or {}).get('kind')}), flush=True)
    return 0 if report['status'] == 'PASS_DEVELOPMENT_PACKAGE_ONLY' else 1

if __name__ == '__main__':
    raise SystemExit(main())
