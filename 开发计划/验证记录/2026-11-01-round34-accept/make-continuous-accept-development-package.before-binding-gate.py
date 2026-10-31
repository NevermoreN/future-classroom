#!/usr/bin/env python3
"""Prepare a reproducible GNU continuous-accept development ZIP only after exact original evidence.

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
EVIDENCE = Path('/tmp/fc-round34-accept-evidence')
REPORT = EVIDENCE / 'development-package-first.json'
NORMAL = Path('/tmp/fc-round34-accept-programs/fc-bootstrap.exe')
OLD = Path('/mnt/data/FutureClassroom-0.1.0-request-deadline-dev.zip')
OLD_SHA = '1acdec0c1506db0484c0a4ba8e2bfa7187ab26d485285128b6dd35ad9e114c69'
OLD_BYTES = 5960978
OUTPUT = Path('/mnt/data/FutureClassroom-0.1.0-continuous-accept-dev.zip')
WEB = Path('/tmp/fc-round34-web-dist')
REPACK = Path('/tmp/fc-round34-accept-repack')
BASE_GIT = '93414d5f26c829db552f8434433f6ad0684971f6'
EXPECTED_NORMAL_SHA = '33f89b7a3592d1c926171bdc855ba938a43eb7ded15741e0af6ddbafbc5613f9'
TOOLCHAIN = '1.97.1-x86_64-unknown-linux-gnu'
TARGET = 'i686-pc-windows-gnu'
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
MARKER = '.fc-round34-packaging-owner'

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


def source_manifest_gate(build):
    require(build['status'] == 'PASS' and build['changedInputs'] == [], 'GNU build or source immutability not PASS')
    require(build['gitHeadBefore'] == BASE_GIT and build['gitHeadAfter'] == BASE_GIT, 'Unexpected recorded build baseline Git reference')
    before, after = build['sourceBefore'], build['sourceAfter']
    require(isinstance(before, dict) and before == after and len(before) == 383, 'Original383 build input manifest changed/incomplete')
    current_paths = {p.relative_to(REPO).as_posix() for p in (REPO / 'crates').rglob('*.rs')}
    current_paths |= {p.relative_to(REPO).as_posix() for p in (REPO / 'crates').rglob('Cargo.toml')}
    current_paths |= {'Cargo.toml', 'Cargo.lock', '.cargo/config.toml', 'rust-toolchain.toml'}
    require(set(before) == current_paths, 'Current Rust/Cargo/config path inventory differs from original383 inputs')
    for name, expected in before.items():
        require(not PurePosixPath(name).is_absolute() and '..' not in PurePosixPath(name).parts and '\\' not in name, 'Invalid build source path')
        data = regular_bytes(REPO.joinpath(*PurePosixPath(name).parts), 64*MIB)
        require(len(data) == expected['bytes'] and sha(data) == expected['sha256'], 'Current build input changed from exact original build manifest')
    commands = build['commands']
    require(len({c['name'] for c in commands}) == len(commands), 'Duplicate GNU build command')
    require({c['name'] for c in commands} == {'rust-version','linker-version','fmt','architecture','compile','product','clippy'} and all(c['exitCode'] == 0 for c in commands), 'GNU compile/strict Clippy/preflight command gate failed')
    command_facts = []
    for command in commands:
        for name in ('stdout','stderr'):
            q = Path(command[name])
            require(q.is_relative_to(EVIDENCE), 'Build original log outside own evidence root')
            command_facts.append(fact(q, 16*MIB))
    version = next(c for c in commands if c['name'] == 'rust-version')
    version_text = regular_bytes(Path(version['stdout']), MIB).decode('utf-8')
    require(re.search(r'^release: 1\.97\.1$', version_text, re.M) and re.search(r'^host: x86_64-unknown-linux-gnu$', version_text, re.M), 'Original actual GNU compiler version/host differs')
    return {'baseGitReferenceBeforeAfter': BASE_GIT, 'bindingMode': 'Actual383-file bytes/SHA256 before and after build, rechecked at packaging; base Git SHA is context, not a claim that uncommitted Round34 Rust bytes belong to that commit.', 'sourceCount':383, 'sourceBefore':before, 'sourceAfter':after, 'originalCommandLogs':command_facts}


def continuous_gate(tls):
    require(tls['continuousAcceptEnabled'] is True and tls['status'] == 'PASS' and tls['cleanup'] == 'PASS', 'Finite continuous acceptance/runtime cleanup is not actual PASS')
    require(tls['strictP0'] == 'NOT_VALIDATED' and tls['productionListener'] == 'NOT_ENABLED', 'Original strict deadline/production gates upgraded')
    require(all(event['status'] == 'PASS' for event in tls['events']), 'Original TLS client event failed')
    expected_cases = {'continuous-idle-repost-status', 'continuous-after-retire-status'} | {f'continuous-original-{n}-exact-receipt' for n in range(1,5)} | {f'continuous-capacity-reject-{n}' for n in (1,2)}
    selected = [event for event in tls['events'] if event['case'] in expected_cases]
    require(len(selected) == 8 and {event['case'] for event in selected} == expected_cases, 'Eight continuous client evidence cases incomplete/duplicated')
    for event in selected:
        if event['case'].startswith('continuous-capacity-reject-'):
            require(event['tcpConnected'] is True and event['transportRejection'] in ('ConnectionResetError','SSLEOFError','SSLZeroReturnError'), 'Capacity close lacks actual connected transport EOF/reset')
        elif event['case'].startswith('continuous-original-'):
            require(event['exactOriginalReceipt'] is True and event['httpStatus'] in (200,201) and event['closeNotify'].startswith('PASS'), 'Original live JSON did not complete exact receipt/clean TLS EOF')
    progress = tls['nativeProgress']
    require(isinstance(progress,list) and 0 < len(progress) <= 512, 'Bounded native progress originals missing')
    by_sequence = {};last = 0
    for row in progress:
        require(row['marker'] == tls['marker'] and row['sequence'] > last and row['sequence'] not in by_sequence, 'Native progress identity/order mismatch')
        last = row['sequence'];by_sequence[last] = row
        require(0 <= row['acceptance']['bound_slots'] <= 4 and 0 <= row['acceptance']['ready_slots'] <= 4 and 0 <= row['acceptance']['native_pending'] <= 5 and 0 <= row['acceptance']['owned_sockets'] <= 6, 'Original native progress exceeds fixed ownership bounds')
    idle = tls['continuousIdle'];baseline=by_sequence[idle['baselineSequence']];reposted=by_sequence[idle['repostedSequence']]
    require(idle['noClientSeconds'] >= 5.05 and reposted['sequence'] > baseline['sequence'] and reposted['acceptance']['idle_returns'] > baseline['acceptance']['idle_returns'] and reposted['acceptance']['posts'] > baseline['acceptance']['posts'] and reposted['acceptance']['native_pending'] >= 1, 'Actual idle post return/repost evidence missing')
    capacity = tls['business']['continuousAcceptance']
    require(capacity['capacityCloses'] == 2 and capacity['sameOriginalReceipts'] == 4 and capacity['binaryUploadPermitsUsedByStage'] == 0, 'Continuous capacity stage counts differ')
    sequences = [capacity[k] for k in ('admittedSequence','capacityClosedSequence','retiredRearmedSequence','recoveredSequence')]
    require(sequences == sorted(set(sequences)), 'Continuous stage observation order is not strict')
    admitted,rejected,retired,recovered=[by_sequence[n] for n in sequences]
    def key(row):return row['generation'],row['slot'],row['serial']
    def intervals(row):return {key(entry):entry for entry in row['requests']}
    original={key(entry):entry for entry in capacity['originalIntervals']}
    require(len(original) == 4 and len({k[1] for k in original}) == 4 and all(entry['deadlineNs']-entry['admittedAtNs']==120_000_000_000 for entry in original.values()), 'Original four exact120-second interval identities incomplete')
    require(intervals(admitted)==original and admitted['acceptance']['bound_slots']==4 and admitted['pendingJobs']==0 and intervals(rejected)==original and rejected['acceptance']['bound_slots']==4 and rejected['acceptance']['capacity_rejects']==admitted['acceptance']['capacity_rejects']+2, 'Actual admission/reject barrier differs from four original requests')
    first=next(e for e in selected if e['case']=='continuous-original-1-exact-receipt')
    remaining={k:v for k,v in original.items() if k != key(first['originalInterval'])}
    require(len(remaining)==3 and intervals(retired)==remaining and retired['retired']>rejected['retired'] and retired['rearms']>rejected['rearms'] and retired['acceptance']['ready_slots']>=1 and retired['acceptance']['bound_slots']==3, 'Original actual retirement/rearm observation missing')
    require(intervals(recovered)==remaining and recovered['retired']>retired['retired'] and recovered['rearms']>retired['rearms'] and recovered['acceptance']['bound_slots']==3, 'Recovery GET disturbed other3 original intervals or lacked retirement')
    raw=tls['rawPreservation'];require(raw['status']=='PASS' and 'tls.stdout' in raw['files'], 'Original remote stdout preservation not PASS')
    raw_facts=[];raw_stdout=None
    for name,row in raw['files'].items():
        require(row['status']=='PASS' and row['remote']==row['downloaded'], 'Original remote artifact download identity mismatch')
        q=Path(row['artifact']);require(q.is_relative_to(EVIDENCE),'Original remote artifact outside own evidence root')
        data=regular_bytes(q,64*MIB)
        require(len(data)==row['remote']['bytes'] and sha(data)==row['remote']['sha256'],'Original remote evidence file bytes/SHA changed')
        raw_facts.append({'name':name,**fact(q,64*MIB)})
        if name=='tls.stdout':raw_stdout=data.decode('utf-8-sig')
    original_progress=[read_json(line[len('TLS_BUSINESS_PROGRESS '):].encode()) for line in raw_stdout.splitlines() if line.startswith('TLS_BUSINESS_PROGRESS ')]
    require(original_progress==progress,'Report native progress differs from complete original stdout')
    return {'idle':idle,'capacity':capacity,'continuousClientCases':selected,'nativeProgressCount':len(progress),'nativeProgressSequences':sequences,'originalRemoteFiles':raw_facts,'scope':'Finite real Schannel/shared-worker diagnostic only; not production listener, exact TCP5s, full120s wait or kernel resource certification.'}


def prepare(report):
    report['phase'] = 'original-evidence-gates'
    original_reports = {}
    for name in ('windows-affected-first.json','windows-native-first.json','windows-tls-continuous-first.json'):
        path = EVIDENCE / name
        data = regular_bytes(path, 32*MIB)
        original_reports[name] = read_json(data)
        report['inputReports'].append({'path': str(path), 'bytes': len(data), 'sha256': sha(data)})
    build=original_reports['windows-affected-first.json'];runtime=original_reports['windows-native-first.json'];tls=original_reports['windows-tls-continuous-first.json']
    report['buildInputBinding']=source_manifest_gate(build)
    require(runtime['status']=='PASS' and runtime['cleanup']=='PASS' and runtime['selfCheck']['exitCode']==0 and runtime['native']['exitCode']==0, 'Affected Win11/native runtime or cleanup not PASS')
    require(len(runtime['tests'])==1 and runtime['tests'][0]['exitCode']==0, 'Expected only one affected fc-windows test executable')
    tested=runtime['tests'][0]
    summary=[line for line in tested['output'] if re.fullmatch(r'test result: ok\. 177 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in [0-9.]+s',line)]
    require(len(summary)==1,'Actual177 affected tests summary missing; do not substitute a full-suite claim')
    artifacts=build['artifacts'];normal_rows=[a for a in artifacts if a['test'] is False];test_rows=[a for a in artifacts if a['test'] is True]
    require(len(normal_rows)==1 and len(test_rows)==1 and normal_rows[0]['path']==str(NORMAL),'Build artifacts do not identify exact GNU normal and affected test binary')
    normal=regular_bytes(NORMAL,128*MIB);normal_sha=sha(normal)
    require(normal_sha==EXPECTED_NORMAL_SHA==normal_rows[0]['sha256']==runtime['exeSha256']==tls['exeSha256'] and len(normal)==normal_rows[0]['bytes'],'GNU normal/build/runtime/TLS identity mismatch')
    require(tested['name']==Path(test_rows[0]['path']).name and tested['binarySha256']==test_rows[0]['sha256'],'Affected test executable identity differs from original GNU build')
    uploaded=[u for u in runtime['uploadedArtifacts'] if u['name']==NORMAL.name]
    require(len(uploaded)==1 and uploaded[0]['sha256']==normal_sha,'Uploaded native normal identity mismatch')
    require(tls['cleanup']=='PASS' and tls['cleanupDetails']['status']=='PASS' and tls['cleanupDetails']['rootAbsent'] is True and tls['cleanupDetails']['ownedProcessesRemaining']==0 and tls['cleanupDetails']['myDigestRestored'] is True and tls['cleanupDetails']['cngKeyExists'] is False and tls['ownedProcessCleanup']['status']=='PASS','TLS process/certificate/key/root cleanup incomplete')
    native=tls['native']
    require(native['physicalNativeDrain'] is True and native['sharedWorkers']==2 and native['workerJoin'] is True and native['hubPending']==0 and native['downloadHubPending']==0 and native['quarantined']==0 and native['setupCleanup'] is True,'TLS actual native/pool/hub/setup cleanup gate failed')
    report['continuousAcceptanceOriginals']=continuous_gate(tls)
    report['runtimeScope']={'testExecutables':1,'affectedTests':177,'hiddenNative':True,'fullWorkspaceSuite':'NOT_RUN_IN_THIS_STAGE; prior1313 suite is separate evidence, not claimed for this binary.','gnuOnly':True,'formalMsvcEvidence':'NOT_CLAIMED'}
    report['normalExecutable']={'path':str(NORMAL),'bytes':len(normal),'sha256':normal_sha,'buildAndTwoRuntimeReportsBindSameSha':True,'packagerExecutesExecutable':False}
    source_paths = ['scripts/package-probe.py', 'scripts/audit-pe.py', 'packaging/PROBE-README.txt', 'packaging/THIRD-PARTY-NOTICES.txt', 'LICENSE', 'web/THIRD-PARTY-NOTICES.txt', 'scripts/windows-probe.py', 'scripts/windows-tls-business-probe.py', 'scripts/windows-tls-probe.py']
    source_facts = {p: fact(REPO / p, 4*MIB) for p in source_paths}
    report['localSourceBindings'] = source_facts
    require(tls['runnerSourceSha256'] == source_facts['scripts/windows-tls-business-probe.py']['sha256'] and tls['ownershipHelperSourceSha256'] == source_facts['scripts/windows-tls-probe.py']['sha256'], 'Actual TLS runner/source helper changed from evidence')
    require(not OUTPUT.exists() and not OUTPUT.is_symlink() and not WEB.exists() and not WEB.is_symlink() and not REPACK.exists() and not REPACK.is_symlink(), 'Package output or temporary root already exists; refusing replacement')
    report['phase'] = 'frozen-original-web-package'
    old_data = regular_bytes(OLD, 128*MIB)
    require(len(old_data) == OLD_BYTES and sha(old_data) == OLD_SHA, 'Original Web source development ZIP bytes/SHA mismatch')
    require(tls['webBundleSha256'] == OLD_SHA, 'TLS static fixture did not use the exact frozen original Web package')
    _, old_members, old_member_facts, old_manifest = archive_members(OLD)
    report['reusedWebSource'] = {'path': str(OLD), 'bytes': len(old_data), 'sha256': OLD_SHA, 'memberCount': len(old_member_facts), 'webFiles': [f for f in old_member_facts if f['path'] in WEB_NAMES], 'extractionScope': 'Only these exact eight web members are copied; old GNU EXE and other native payloads are never extracted.'}
    require(regular_bytes(REPO / 'web/THIRD-PARTY-NOTICES.txt', MIB) == old_members['web/THIRD-PARTY-NOTICES.txt'], 'Current Web notice differs from frozen tested Web notice')
    token = 'round34-gnu-continuous-accept-development-package/' + uuid.uuid4().hex
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
    require(source_manifest_gate(build) == report['buildInputBinding'], '383 build inputs/logs changed during packaging')
    report['phase'] = 'independent-zip-and-payload-validation'
    first_data, members, rows, manifest = archive_members(OUTPUT)
    second_data, second_members, second_rows, second_manifest = archive_members(second)
    require(first_data == second_data and rows == second_rows and manifest == second_manifest, 'Independent same-basename repack not byte-identical')
    require(manifest['artifact_name'] == OUTPUT.name and manifest['target'] == TARGET and manifest['target_role'] == 'modern-GNU-development-only' and manifest['toolchain']['declared_build_toolchain'] == TOOLCHAIN, 'Package target/toolchain/name metadata mismatch')
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
        report = {'status': 'FAIL', 'phase': 'initial', 'scope': 'Exact383 Rust/Cargo input manifest and GNU build plus actual authorized Win11 affected177-test/hidden-native and finite continuous-accept TLS evidence, repackaged only as an unvalidated development probe. This is not the prior1313-test full suite, formal MSVC evidence, or an assertion that uncommitted Rust bytes already belong to the base Git commit. No EXE execution by this helper; no network/Windows/Git calls. Win7/Win10/complete product, strict TCP5s and release gates remain unchanged.', 'helperSource': fact(Path(__file__).resolve(), MIB), 'inputReports': [], 'commands': [], 'temporaryRootCleanup': [], 'temporaryPackagingRootsRemoved': False, 'error': None}
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
