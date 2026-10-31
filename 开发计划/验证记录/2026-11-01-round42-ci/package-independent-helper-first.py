from pathlib import Path, PurePosixPath
import datetime, hashlib, json, os, re, subprocess, tempfile, time, uuid, zipfile, zlib

ROOT = Path('/workspace/共享/Eliora/未来教室计划')
EV = Path('/tmp/fc-round42-ci-evidence')
CANDIDATE = Path('/tmp/fc-round42-package/FutureClassroom-0.1.0-voice-capability-dev.zip')
WEB = Path('/tmp/fc-round42-web-assets/dist')
EXE = Path('/tmp/fc-round42-modern-programs/fc-bootstrap.exe')
OUT = EV / 'package-independent-first.json'
SOURCE = 'bc2a86fb4413dcb3e8acf6b0d4bb2dfe601c22dc'
EXPECTED_EXE_SHA = '6c82d3dfd5352849c4291bf06c2ed0228ceab9edbf368a06eb753dab2014d8ac'
EXPECTED_EXE_BYTES = 10324480
sha = lambda b: hashlib.sha256(b).hexdigest()
now = lambda: datetime.datetime.now(datetime.timezone.utc).isoformat()

def unique(pairs):
    obj = {}
    for key, value in pairs:
        assert key not in obj, 'duplicate JSON key'
        obj[key] = value
    return obj

def parse(raw):
    return json.loads(raw.decode('utf-8'), object_pairs_hook=unique,
                      parse_constant=lambda _: (_ for _ in ()).throw(ValueError('nonfinite JSON')))

def record(path):
    data = path.read_bytes()
    return {'path': str(path), 'bytes': len(data), 'sha256': sha(data)}

def safe(name):
    pp = PurePosixPath(name)
    assert name and not pp.is_absolute() and '..' not in pp.parts
    assert '\\' not in name and '\x00' not in name and pp.as_posix() == name
    return pp

assert not OUT.exists()
for suffix in ['command.json', 'stdout.log', 'stderr.log']:
    assert not (EV / ('package-independent-repack-first.' + suffix)).exists()
report = {'schema': 'fc.package-independent.v1', 'status': 'RUNNING',
          'startedUtc': now(), 'sourceSha': SOURCE,
          'scope': 'Independent development-probe ZIP/manifest/source payload/CRC/SHA and exact-byte reproduction only. No executable, Windows, Cargo, Git mutation, publication or /mnt/data write. R42 source is final R39 capability feature plus later false-positive correction and cleanup docs, NOT R41 deadline code.',
          'runtime': 'NOT_RUN_BY_THIS_CHECK', 'candidate': record(CANDIDATE)}
OUT.write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n')

def inspect(path):
    with zipfile.ZipFile(path) as z:
        infos = z.infolist()
        names = [i.filename for i in infos]
        assert len(names) == len(set(names)) == 13
        assert not any(i.is_dir() for i in infos) and z.testzip() is None
        manifest = parse(z.read('manifest.json'))
        assert manifest['schema_version'] == 1 and manifest['product'] == 'FutureClassroom'
        assert manifest['version'] == '0.1.0' and manifest['artifact_name'] == CANDIDATE.name
        assert manifest['artifact_type'] == 'development-probe' and manifest['status'] == 'NOT_VALIDATED'
        assert manifest['target'] == 'i686-pc-windows-msvc'
        assert manifest['target_role'] == 'modern-MSVC-development-only'
        assert manifest['toolchain']['declared_build_toolchain'] == '1.97.1-x86_64-pc-windows-msvc'
        assert manifest['sha256'] == EXPECTED_EXE_SHA
        assert not manifest['pe_audit']['structural_errors']
        assert manifest['gates']['release_tag'] == 'NOT_ALLOWED'
        assert all(v == 'NOT_VALIDATED' for k, v in manifest['gates'].items() if k != 'release_tag')
        entries = manifest['files']
        byname = {x['path']: x for x in entries}
        assert len(entries) == len(byname) == 12
        assert set(names) == set(byname) | {'manifest.json'}
        webraw = (WEB / 'fc-assets.json').read_bytes()
        assert len(webraw) <= 64 * 1024
        webmanifest = parse(webraw)
        assert set(webmanifest) == {'schemaVersion', 'apiVersion', 'uiCompatibilityVersion', 'assets'}
        for k in ['schemaVersion', 'apiVersion', 'uiCompatibilityVersion']:
            assert type(webmanifest[k]) is int and webmanifest[k] == 1
        assets = webmanifest['assets']
        assert 1 <= len(assets) <= 64
        aset = {}
        total = 0
        for item in assets:
            assert set(item) == {'path', 'sizeBytes', 'sha256'}
            name, size, digest = item['path'], item['sizeBytes'], item['sha256']
            assert isinstance(name, str) and (name == 'index.html' or re.fullmatch(r'assets/[A-Za-z0-9._-]+\.(js|css|svg|png|jpg|jpeg|webp)', name))
            assert name.lower() not in aset and type(size) is int and 1 <= size <= 5 * 1024 * 1024
            assert isinstance(digest, str) and re.fullmatch('[0-9a-f]{64}', digest)
            safe(name)
            data = (WEB / name).read_bytes()
            assert len(data) == size and sha(data) == digest
            aset[name.lower()] = item
            total += size
        assert 'index.html' in aset and total <= 5 * 1024 * 1024
        expected = {
            'fc-bootstrap.exe': EXE,
            'LICENSE': ROOT / 'LICENSE',
            'PROBE-README.txt': ROOT / 'packaging/PROBE-README.txt',
            'THIRD-PARTY-NOTICES.txt': ROOT / 'packaging/THIRD-PARTY-NOTICES.txt',
            'web/THIRD-PARTY-NOTICES.txt': ROOT / 'web/THIRD-PARTY-NOTICES.txt',
            'web/fc-assets.json': WEB / 'fc-assets.json',
        }
        expected.update({'web/' + item['path']: WEB / item['path'] for item in assets})
        assert set(expected) == set(byname)
        frontend = manifest['frontend']
        assert frontend['included'] is True and frontend['status'] == 'PARTIAL_NOT_VALIDATED'
        assert frontend['manifest_path'] == 'web/fc-assets.json' and frontend['manifest_sha256'] == sha(webraw)
        assert frontend['asset_count'] == len(assets) == 6 and frontend['asset_bytes'] == total
        assert frontend['third_party_notices'] == 'web/THIRD-PARTY-NOTICES.txt'
        members = []
        for info in infos:
            name = info.filename
            safe(name)
            assert ((info.external_attr >> 16) & 0o170000) != 0o120000
            data = z.read(info)
            assert len(data) == info.file_size and zlib.crc32(data) & 0xffffffff == info.CRC
            item = {'path': name, 'bytes': len(data), 'sha256': sha(data),
                    'crc32': f'{info.CRC:08x}', 'crcVerified': True}
            if name != 'manifest.json':
                assert byname[name]['size'] == len(data) and byname[name]['sha256'] == sha(data)
                assert expected[name].read_bytes() == data
                item['source'] = str(expected[name])
                item['manifestAndExactSourceMatched'] = True
            if name == 'fc-bootstrap.exe':
                assert len(data) == EXPECTED_EXE_BYTES and sha(data) == EXPECTED_EXE_SHA
            members.append(item)
        return {'manifest': manifest, 'members': members, 'assetCount': len(assets),
                'assetBytes': total, 'manifestBytes': len(z.read('manifest.json')),
                'manifestSha256': sha(z.read('manifest.json'))}

owned = None
try:
    report['inspection'] = inspect(CANDIDATE)
    # The notices/licenses are byte-identical to the exact successful CI Git source,
    # independently of the current main filesystem copy used by the packager.
    originals = []
    for relative in ['LICENSE', 'packaging/THIRD-PARTY-NOTICES.txt', 'web/THIRD-PARTY-NOTICES.txt']:
        data = (ROOT / relative).read_bytes()
        blob = subprocess.check_output(['git', 'cat-file', 'blob', SOURCE + ':' + relative], cwd=ROOT)
        assert data == blob
        originals.append({'path': relative, 'bytes': len(data), 'sha256': sha(data),
                          'exactPinnedGitBlobMatched': True})
    report['licenseSourceProof'] = originals
    packager = ROOT / 'scripts/package-probe.py'
    before = record(packager)
    token = 'fc-round42-independent-repack-' + uuid.uuid4().hex
    owned = Path(tempfile.mkdtemp(prefix='fc-round42-independent-repack-', dir='/tmp'))
    os.chmod(owned, 0o700)
    owner = owned / '.owner'
    with owner.open('x') as f:
        f.write(token + '\n')
    duplicate = owned / CANDIDATE.name
    command = ['python3', '-B', str(packager), '--exe', str(EXE), '--output', str(duplicate),
               '--target', 'i686-pc-windows-msvc', '--toolchain', '1.97.1-x86_64-pc-windows-msvc',
               '--web-dist', str(WEB)]
    stdout_path = EV / 'package-independent-repack-first.stdout.log'
    stderr_path = EV / 'package-independent-repack-first.stderr.log'
    command_path = EV / 'package-independent-repack-first.command.json'
    command_report = {'status': 'RUNNING', 'command': command, 'cwd': str(ROOT),
                      'startedUtc': now(), 'stdout': str(stdout_path), 'stderr': str(stderr_path),
                      'budgetSeconds': 60, 'ownedDirectory': str(owned)}
    with command_path.open('x') as f:
        f.write(json.dumps(command_report, indent=2) + '\n')
    started = time.monotonic()
    with stdout_path.open('xb') as sout, stderr_path.open('xb') as serr:
        process = subprocess.run(command, cwd=ROOT, stdout=sout, stderr=serr, timeout=60)
    command_report.update(status='PASS' if process.returncode == 0 else 'FAIL',
                          completedUtc=now(), elapsedSeconds=time.monotonic() - started,
                          exitCode=process.returncode, stdoutEvidence=record(stdout_path),
                          stderrEvidence=record(stderr_path))
    command_path.write_text(json.dumps(command_report, indent=2) + '\n')
    assert process.returncode == 0
    assert record(packager) == before
    duplicate_inspection = inspect(duplicate)
    assert duplicate_inspection == report['inspection']
    original_bytes = CANDIDATE.read_bytes()
    duplicate_bytes = duplicate.read_bytes()
    assert original_bytes == duplicate_bytes
    report['repack'] = {'status': 'PASS', 'sameBasename': duplicate.name == CANDIDATE.name,
                        'exactFullZipBytesIdentical': True, 'duplicate': record(duplicate),
                        'packager': before, 'commandEvidence': record(command_path),
                        'stdoutEvidence': record(stdout_path), 'stderrEvidence': record(stderr_path),
                        'duplicateInspection': duplicate_inspection}
    assert owner.read_text() == token + '\n'
    assert set(owned.iterdir()) == {owner, duplicate}
    duplicate.unlink()
    owner.unlink()
    owned.rmdir()
    assert not owned.exists()
    report['repack']['ownedDuplicateDirectoryActuallyRemoved'] = True
    report['candidatePreserved'] = CANDIDATE.exists() and CANDIDATE.read_bytes() == original_bytes
    assert report['candidatePreserved']
    report.update(status='PASS', completedUtc=now(), memberCount=13, payloadCount=12,
                  bytes=len(original_bytes), sha256=sha(original_bytes),
                  helper=record(Path(__file__)),
                  limitations=['Development-probe package only; no release/tag/deployment or ordinary listener enablement.',
                               'No Windows EXE or browser executed by this package audit. Parent-owned runtime evidence is independent and still outside this report.',
                               'No Win7/Win10/Seewo full-product acceptance or strict first-TCP five-second gate claim.'])
except BaseException as error:
    report.update(status='FAIL', errorKind=type(error).__name__, completedUtc=now(),
                  errorMessage='Independent package invariant failed; original candidate and failure evidence preserved.',
                  retainedOwnedDirectory=str(owned) if owned is not None and owned.exists() else None)
OUT.write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n')
print(json.dumps({'status': report['status'], 'bytes': report.get('bytes'),
                  'sha256': report.get('sha256'), 'memberCount': report.get('memberCount'),
                  'payloadCount': report.get('payloadCount'), 'report': record(OUT)}, ensure_ascii=False))
raise SystemExit(0 if report['status'] == 'PASS' else 1)
