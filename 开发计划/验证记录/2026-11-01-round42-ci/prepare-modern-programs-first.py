from pathlib import Path, PurePosixPath, PureWindowsPath
import datetime, hashlib, json, re, runpy, struct, subprocess, zipfile, zlib

ROOT = Path('/workspace/共享/Eliora/未来教室计划')
EV = Path('/tmp/fc-round42-ci-evidence')
BASE = EV / 'modern-artifact-raw'
DEST = Path('/tmp/fc-round42-modern-programs')
ZIP = Path('/tmp/fc-round42-modern-original.zip')
OUT = EV / 'modern-programs-independent-first.json'
MEMBER_OUT = EV / 'modern-original-member-ledger-first.json'
METADATA = EV / 'modern-test-metadata-first.jsonl'
SOURCE = 'bc2a86fb4413dcb3e8acf6b0d4bb2dfe601c22dc'
RUN = 41244281175
ARTIFACT = 13058962672
EXPECTED_BYTES = 40772166
EXPECTED_SHA = '93638e76769f5d3910c4e3e9a713a93bedc318c55ebc8e78f24ff42ac4ff265a'
sha = lambda data: hashlib.sha256(data).hexdigest()
load = lambda data: json.loads(data.decode('utf-8-sig'))
for p in [BASE, DEST, OUT, MEMBER_OUT, METADATA]:
    assert not p.exists(), 'refuse existing output'
BASE.mkdir()
DEST.mkdir()
report = {
    'schema': 'fc.modern-programs-independent.v1', 'status': 'RUNNING',
    'recordedAtUtc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
    'sourceSha': SOURCE, 'runId': RUN, 'artifactId': ARTIFACT,
    'scope': 'Exact original R42 modern MSVC artifact: final R39 capability feature, Win7 MSVC thread-local false-positive correction and cleanup documentation. NOT R41 new frontdoor deadline code. Independent ZIP/CRC/producer ledger, complete exact source checkout comparison, PE32/static CRT audit. No executable, Cargo, Windows, remote or Git mutation.',
    'runtime': 'NOT_RUN', 'artifacts': [], 'members': [], 'inputs': [],
}

def save(path, obj):
    path.write_text(json.dumps(obj, ensure_ascii=False, indent=2) + '\n')

def safe_path(name):
    pp = PurePosixPath(name)
    assert name and not pp.is_absolute() and '..' not in pp.parts
    assert '\\' not in name and '\x00' not in name
    assert pp.as_posix() == name
    return pp

save(OUT, report)
try:
    rawzip = ZIP.read_bytes()
    assert len(rawzip) == EXPECTED_BYTES and sha(rawzip) == EXPECTED_SHA
    report['zip'] = {'path': str(ZIP), 'bytes': len(rawzip), 'sha256': sha(rawzip)}
    del rawzip
    audit_script = ROOT / 'scripts/audit-pe.py'
    audit = runpy.run_path(str(audit_script))['audit_pe']
    with zipfile.ZipFile(ZIP) as z:
        infos = z.infolist()
        names = [x.filename for x in infos]
        assert len(names) == len(set(names)) == 192
        assert not any(x.is_dir() for x in infos)
        assert z.testzip() is None
        producer = load(z.read('artifact-sha256.json'))
        ledger = {x['path']: x for x in producer}
        assert len(ledger) == len(producer) == 191
        assert set(names) == set(ledger) | {'artifact-sha256.json'}
        rows = load(z.read('executables.json'))
        exes = {x['relative_path']: x for x in rows}
        assert len(rows) == len(exes) == 22
        assert sum(x['role'] == 'tests' for x in rows) == 21
        assert all(x['executed'] is False for x in rows)
        assert {x for x in names if x.lower().endswith('.exe')} == set(exes)
        assert sum(x.file_size for x in infos) <= 512 * 1024 * 1024
        for info in infos:
            name = info.filename
            pp = safe_path(name)
            assert ((info.external_attr >> 16) & 0o170000) != 0o120000
            assert info.file_size <= 128 * 1024 * 1024
            data = z.read(info)
            digest = sha(data)
            assert len(data) == info.file_size
            assert zlib.crc32(data) & 0xffffffff == info.CRC
            if name in ledger:
                assert ledger[name]['bytes'] == len(data) and ledger[name]['sha256'] == digest
            member = {'path': name, 'bytes': len(data), 'sha256': digest,
                      'zipCrc32': f'{info.CRC:08x}', 'crcChecked': True,
                      'producerLedgerMatched': name in ledger,
                      'nonbinaryRawOriginalPreserved': name not in exes}
            if name not in exes:
                assert not data.startswith(b'MZ'), 'unlisted native binary'
                path = BASE.joinpath(*pp.parts)
                path.parent.mkdir(parents=True, exist_ok=True)
                with path.open('xb') as f:
                    f.write(data)
                assert path.read_bytes() == data
            else:
                row = exes[name]
                assert row['bytes'] == len(data) and row['sha256'] == digest
                assert re.fullmatch(r'[A-Za-z0-9_-]+\.exe', pp.name)
                path = DEST / pp.name
                assert not path.exists()
                with path.open('xb') as f:
                    f.write(data)
                assert path.read_bytes() == data
                assert data[:2] == b'MZ'
                peoff = struct.unpack_from('<I', data, 0x3c)[0]
                assert data[peoff:peoff+4] == b'PE\x00\x00'
                machine = struct.unpack_from('<H', data, peoff + 4)[0]
                optional = peoff + 24
                magic = struct.unpack_from('<H', data, optional)[0]
                subsystem_version = list(struct.unpack_from('<HH', data, optional + 48))
                subsystem = struct.unpack_from('<H', data, optional + 68)[0]
                assert machine == 0x14c and magic == 0x10b and subsystem == 3
                pe = audit(path)
                assert not pe['structural_errors']
                dlls = [d.lower() for d in pe['imports']]
                assert dlls
                forbidden = [d for d in dlls if d.startswith((
                    'vcruntime', 'msvcp', 'api-ms-win-crt', 'libgcc_s_',
                    'libstdc++', 'libwinpthread')) or d == 'ucrtbase.dll'
                    or re.fullmatch(r'msvcr[0-9]+\.dll', d)]
                assert not forbidden
                report['artifacts'].append({**row, 'localPath': str(path),
                    'peHeader': {'machine': hex(machine), 'optionalMagic': hex(magic),
                                 'subsystem': subsystem, 'subsystemVersion': subsystem_version},
                    'forbiddenDynamicRuntimeImports': forbidden, 'independentPeAudit': pe})
            report['members'].append(member)
    assert len([p for p in BASE.rglob('*') if p.is_file()]) == 170
    assert len([p for p in DEST.iterdir() if p.is_file()]) == 22
    result = load((BASE / 'result.json').read_bytes())
    start = load((BASE / 'run-start.json').read_bytes())
    cleanup = load((BASE / 'cleanup.json').read_bytes())
    assert result['source_sha'] == result['dispatch_source_sha'] == SOURCE
    assert all(start[k] == SOURCE for k in ['expected_source_sha', 'dispatch_source_sha', 'workflow_sha'])
    assert result['status'] == 'COMPILE_CHECKS_PASSED_RUNTIME_NOT_RUN'
    assert result['baseline'] == start['baseline'] == 'Modern'
    assert result['target'] == start['target'] == 'i686-pc-windows-msvc'
    assert result['toolchain'] == start['toolchain'] == '1.97.1-x86_64-pc-windows-msvc'
    assert start['rustflags'] == '-C target-feature=+crt-static'
    assert result['executable_count'] == 22
    assert all(result[k] == 'NOT_RUN' for k in ['windows7', 'windows10', 'windows11', 'seewo_device'])
    assert result['input_manifest_sha256'] == sha((BASE / 'input-sha256.json').read_bytes())
    commands = result['commands']
    assert len(commands) == 39 and len({x['name'] for x in commands}) == len(commands)
    command_logs = []
    for command in commands:
        assert command['passed'] is True and command['exit_code'] == 0
        assert command['parent_exited'] is True
        assert command['timed_out_or_pipe_stalled'] is False and command['log_truncated'] is False
        logs = []
        for key in ['stdout', 'stderr']:
            relative = command[key]
            safe_path(relative)
            assert relative in ledger
            data = (BASE / relative).read_bytes()
            assert len(data) == ledger[relative]['bytes'] and sha(data) == ledger[relative]['sha256']
            logs.append({'path': relative, 'bytes': len(data), 'sha256': sha(data)})
        command_logs.append({'command': command, 'originalLogs': logs})
    assert (BASE / 'logs/source-sha.stdout.log').read_text(encoding='utf-8-sig').strip() == SOURCE
    for suffix in ['before', 'after']:
        assert not (BASE / f'logs/source-clean-{suffix}.stdout.log').read_text(encoding='utf-8-sig').strip()
    assert cleanup['status'] == 'OWNED_BUILD_ROOTS_REMOVED'
    assert {x['root'] for x in cleanup['roots']} == {'target', 'scratch', 'cargo', 'rustup'}
    assert all(x['removed'] is True for x in cleanup['roots'])
    metadata = []
    byname = {PurePosixPath(x['relative_path']).name: x for x in rows if x['role'] == 'tests'}
    for line in (BASE / 'logs/workspace-test-compile.stdout.log').read_text(encoding='utf-8-sig').splitlines():
        try:
            row = json.loads(line)
        except json.JSONDecodeError:
            continue
        if row.get('reason') != 'compiler-artifact' or not row.get('executable') or not row.get('profile', {}).get('test'):
            continue
        name = PureWindowsPath(row['executable']).name
        assert name in byname
        row['executable'] = str(DEST / name)
        metadata.append(row)
    assert len(metadata) == len(byname) == 21
    assert len({x['executable'] for x in metadata}) == 21
    with METADATA.open('x') as f:
        f.write(''.join(json.dumps(x) + '\n' for x in metadata))
    inputs = load((BASE / 'input-sha256.json').read_bytes())
    paths = {p.decode('utf-8') for p in subprocess.check_output(
        ['git', 'ls-tree', '-r', '--name-only', '-z', SOURCE], cwd=ROOT).split(b'\x00') if p}
    assert len({x['path'] for x in inputs}) == len(inputs)
    assert {x['path'] for x in inputs} == paths
    proc = subprocess.Popen(['git', 'cat-file', '--batch'], cwd=ROOT,
                            stdin=subprocess.PIPE, stdout=subprocess.PIPE)
    direct = crlf = 0
    try:
        for item in inputs:
            path = item['path']
            assert '\n' not in path and '\r' not in path and '\x00' not in path
            proc.stdin.write((SOURCE + ':' + path + '\n').encode('utf-8'))
            proc.stdin.flush()
            header = proc.stdout.readline().decode().strip().split()
            assert len(header) == 3 and header[1] == 'blob'
            body = proc.stdout.read(int(header[2]))
            assert len(body) == int(header[2]) and proc.stdout.read(1) == b'\n'
            gitbytes, gitsha = len(body), sha(body)
            if gitbytes == item['bytes'] and gitsha == item['sha256']:
                direct += 1
                mode = 'EXACT_GIT_BLOB'
            else:
                assert b'\r' not in body and b'\x00' not in body
                converted = body.replace(b'\n', b'\r\n')
                assert len(converted) == item['bytes'] and sha(converted) == item['sha256']
                crlf += 1
                mode = 'EXACT_LF_TO_CRLF_CHECKOUT'
            report['inputs'].append({**item, 'gitBlobBytes': gitbytes,
                                    'gitBlobSha256': gitsha, 'checkoutMatch': mode})
    finally:
        proc.stdin.close()
        assert proc.wait() == 0
    report.update(status='PASS', testExecutables=21, totalExecutables=22,
        completeCiInputSet=len(inputs), directGitBlobMatches=direct,
        exactWindowsCrlfCheckoutMatches=crlf,
        sourceComparison='Complete pinned Git tree; exact Git blob or exact full LF-to-CRLF checkout only. No loose normalization or changed original files.',
        counts={'zipMembers': 192, 'producerLedgerRows': 191, 'nonbinaryRawFiles': 170,
                'commands': len(commands), 'testExecutables': 21, 'productExecutables': 1,
                'inputs': len(inputs), 'directGitBlobMatches': direct, 'exactLfToCrlfMatches': crlf},
        ciTerminal={'originalResult': result, 'originalStart': start,
                    'originalCleanup': cleanup, 'commandOriginalLogBindings': command_logs},
        producerLedger=producer,
        auditTool={'path': str(audit_script), 'bytes': audit_script.stat().st_size,
                   'sha256': sha(audit_script.read_bytes())},
        verifierSource={'path': str(Path(__file__)), 'sha256': sha(Path(__file__).read_bytes())},
        metadata={'path': str(METADATA), 'bytes': METADATA.stat().st_size,
                  'sha256': sha(METADATA.read_bytes()), 'records': 21},
        limitations=['Static imports and PE headers do not certify target-OS runtime, dynamic loads or driver behavior.',
                     'No Win7 subsystem version requirement is imposed on the Modern baseline.',
                     'This artifact is R39 final feature and subsequent false-positive correction/cleanup docs, not R41 deadline changes.',
                     'Windows runtime and classroom acceptance remain separate and NOT_RUN here.'])
    save(MEMBER_OUT, {'status': 'PASS', 'sourceSha': SOURCE, 'runId': RUN, 'artifactId': ARTIFACT,
                      'zip': report['zip'], 'counts': report['counts'], 'members': report['members'],
                      'producerLedger': producer, 'ciTerminal': report['ciTerminal']})
except BaseException as error:
    report.update(status='FAIL', errorKind=type(error).__name__,
                  errorMessage='Independent exact-artifact invariant failed; originals and partial outputs retained.')
    save(OUT, report)
    print(json.dumps({'status': report['status'], 'errorKind': type(error).__name__, 'report': str(OUT)}))
    raise SystemExit(1)
save(OUT, report)
normal = [x for x in report['artifacts'] if x['role'] != 'tests']
assert len(normal) == 1
print(json.dumps({'status': report['status'], 'counts': report['counts'],
                  'normal': {k: normal[0][k] for k in ['localPath', 'bytes', 'sha256']},
                  'report': {'path': str(OUT), 'bytes': OUT.stat().st_size,
                             'sha256': sha(OUT.read_bytes())}}, ensure_ascii=False))
