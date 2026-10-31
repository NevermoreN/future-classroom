"""Prepare owned browser-stage inputs; no build, remote activity or deletion."""
from pathlib import Path, PurePosixPath
from datetime import datetime, timezone
import hashlib
import json
import subprocess
import zipfile

ROOT = Path.cwd()
EVIDENCE = Path('/tmp/fc-round30-evidence')
DIST = Path('/tmp/fc-round30-web-verified-dist/dist')
ZIP = Path('/tmp/fc-round30-native-browser-web.zip')
NORMAL = Path('/tmp/fc-round30-windows-head-stage-snapshot/fc-bootstrap.exe')
EXPECTED_MANIFEST = '1dfa13904bbbdf661b182e055944c0b6ead4cd91f5c2618a6ee0b6fe9d1271ee'
EXPECTED_NORMAL = 'bbfa4b3818ed4aa15abdfb879e4d2f8339f73b36730b8e8e83b6d8d926574d6a'
STAMP = (2026, 10, 31, 0, 0, 0)
EXCLUDED_PARTS = {'.git', 'target', 'node_modules', '.cache', '.pytest_cache', '__pycache__',
                  'coverage', 'dist', '开发计划', '验证记录'}


def sha(data):
    return hashlib.sha256(data).hexdigest()


def metadata(path, data=None):
    if data is None:
        assert path.is_file() and not path.is_symlink(), str(path)
        data = path.read_bytes()
    return {'path': str(path), 'bytes': len(data), 'sha256': sha(data)}


def git_paths(*args):
    result = subprocess.run(['git', 'ls-files', '-z', *args], check=True, capture_output=True)
    return {p.decode('utf-8') for p in result.stdout.split(b'\0') if p}


def source_selected(name):
    path = PurePosixPath(name)
    if any(part in EXCLUDED_PARTS for part in path.parts):
        return False
    if path.suffix == '.rs':
        return True
    if name in ('Cargo.toml', 'Cargo.lock', '.cargo/config.toml', 'rust-toolchain.toml'):
        return True
    if len(path.parts) == 3 and path.parts[0] == 'crates' and path.parts[2] == 'Cargo.toml':
        return True
    if path.parts[0] == 'web':
        return path.suffix in {'.ts', '.tsx', '.js', '.mjs', '.cjs', '.json', '.html', '.css', '.svg'} or path.name == '.gitignore'
    return False


def main():
    assert ROOT.joinpath('Cargo.toml').is_file(), 'run from repository root'
    manifest_bytes = (DIST / 'fc-assets.json').read_bytes()
    assert sha(manifest_bytes) == EXPECTED_MANIFEST
    manifest = json.loads(manifest_bytes)
    assert {key: manifest[key] for key in ('schemaVersion', 'apiVersion', 'uiCompatibilityVersion')} == {
        'schemaVersion': 1, 'apiVersion': 1, 'uiCompatibilityVersion': 1}
    assert len(manifest['assets']) == 6
    payloads = [('web/fc-assets.json', manifest_bytes)]
    assets = []
    seen = set()
    for row in manifest['assets']:
        relative = PurePosixPath(row['path'])
        assert not relative.is_absolute() and '..' not in relative.parts and '\\' not in row['path']
        assert relative.as_posix() == row['path'] and row['path'] not in seen
        seen.add(row['path'])
        path = DIST / row['path']
        assert path.is_file() and not path.is_symlink()
        data = path.read_bytes()
        assert len(data) == row['sizeBytes'] and sha(data) == row['sha256'], row['path']
        assets.append({'path': row['path'], 'bytes': len(data), 'sha256': sha(data)})
        payloads.append(('web/' + row['path'], data))
    assert sorted(path.relative_to(DIST).as_posix() for path in DIST.rglob('*') if path.is_file()) == sorted(['fc-assets.json', *seen])
    normal = metadata(NORMAL)
    assert normal['sha256'] == EXPECTED_NORMAL

    tracked = git_paths('--cached')
    untracked = git_paths('--others', '--exclude-standard')
    paths = sorted(name for name in tracked | untracked if source_selected(name))
    rows = []
    for name in paths:
        row = metadata(ROOT / name)
        row.update({'repositoryPath': name, 'gitState': 'tracked' if name in tracked else 'untracked'})
        rows.append(row)
    crate_manifests = [row for row in rows if row['repositoryPath'].startswith('crates/') and row['repositoryPath'].endswith('/Cargo.toml')]
    assert len(crate_manifests) == 12
    assert any(row['repositoryPath'] == 'crates/fc-bootstrap/build.rs' for row in rows)
    previous = json.loads((EVIDENCE / 'head-source-freeze.json').read_text())
    current = {row['repositoryPath']: row for row in rows}
    previous_missing = sorted(set(previous['rustCargoSources']) - set(current))
    previous_changed = sorted(name for name, expected in previous['rustCargoSources'].items()
                              if name in current and current[name]['sha256'] != expected)
    # Capture new source state honestly; do not imply rebuild or silently require
    # equality with old normal-compile input coverage.
    snapshot = {
        'schemaVersion': 1,
        'scope': 'Current browser-stage repository input observation only, not a new build or retroactive extension of old compiler capture coverage.',
        'observedAtUtc': datetime.now(timezone.utc).isoformat(),
        'repositoryRoot': str(ROOT),
        'selection': 'Git tracked plus nonignored untracked Rust; root Cargo.toml/Cargo.lock/.cargo/config.toml/rust-toolchain.toml; all 12 crate Cargo.toml and build.rs; web source/tests/scripts/assets/HTML/CSS/JSON/config/lock and .gitignore.',
        'excluded': sorted(EXCLUDED_PARTS),
        'privateKeysAndEvidenceAndBuildPayloads': 'Not selected. ZIP payloads and EXEs are independently hashed, not embedded in this source snapshot.',
        'files': rows,
        'fileCount': len(rows),
        'trackedFiles': sum(row['gitState'] == 'tracked' for row in rows),
        'untrackedFiles': sum(row['gitState'] == 'untracked' for row in rows),
        'rustFiles': sum(row['repositoryPath'].endswith('.rs') for row in rows),
        'crateCargoManifests': len(crate_manifests),
        'webFiles': sum(row['repositoryPath'].startswith('web/') for row in rows),
        'totalBytes': sum(row['bytes'] for row in rows),
        'manifestSha256': sha(json.dumps(rows, ensure_ascii=False, sort_keys=True, separators=(',', ':')).encode()),
        'comparisonToPreviousHeadFreeze': {'path': str(EVIDENCE / 'head-source-freeze.json'),
                                          'sha256': sha((EVIDENCE / 'head-source-freeze.json').read_bytes()),
                                          'missingPaths': previous_missing, 'changedPaths': previous_changed},
        'oldNormalCompileCaptureLimitation': 'The earlier normal compiler capture omitted 12 crate Cargo.toml files. This new observation includes them; it does not prove they were captured by that earlier build. Verified bundle hashing does not itself prove current web source was rebuilt.',
        'oldNormal': normal,
        'sourceStability': 'Every selected source file was reread after capture; sizes and SHA256 had to remain identical before publishing the report.'}

    # Never truncate an earlier owned package. Creation is exclusive, all archive
    # members have deterministic timestamp/order/mode and explicit deflate level.
    with zipfile.ZipFile(ZIP, 'x', compression=zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
        for member, data in sorted(payloads):
            info = zipfile.ZipInfo(member, STAMP)
            info.compress_type = zipfile.ZIP_DEFLATED
            info.create_system = 3
            info.external_attr = 0o100644 << 16
            archive.writestr(info, data, compress_type=zipfile.ZIP_DEFLATED, compresslevel=9)
    members = []
    with zipfile.ZipFile(ZIP) as archive:
        assert len(archive.infolist()) == 7 and archive.testzip() is None
        assert [info.filename for info in archive.infolist()] == sorted(name for name, _ in payloads)
        originals = dict(payloads)
        for info in archive.infolist():
            data = archive.read(info.filename)
            assert data == originals[info.filename] and info.date_time == STAMP and info.compress_type == zipfile.ZIP_DEFLATED
            members.append({'path': info.filename, 'bytes': len(data), 'sha256': sha(data),
                            'compressedBytes': info.compress_size, 'timestamp': list(info.date_time), 'compression': 'DEFLATE'})
    for row in rows:
        data = Path(row['path']).read_bytes()
        assert len(data) == row['bytes'] and sha(data) == row['sha256'], row['repositoryPath']
    assert metadata(NORMAL) == normal
    source_snapshot = EVIDENCE / 'native-browser-input-source-snapshot.json'
    with source_snapshot.open('x') as output:
        output.write(json.dumps(snapshot, ensure_ascii=False, indent=2) + '\n')
    binding = {
        'schemaVersion': 1, 'status': 'INPUT_BYTES_SHA_VERIFIED',
        'scope': 'Local preparation only; no browser, TLS, Windows runtime, build or release acceptance is inferred.',
        'verifiedDist': str(DIST), 'manifest': {'path': str(DIST / 'fc-assets.json'), 'bytes': len(manifest_bytes), 'sha256': EXPECTED_MANIFEST},
        'assets': assets, 'payloadBytes': sum(row['bytes'] for row in assets),
        'zip': {**metadata(ZIP), 'entries': 7, 'members': members},
        'oldNormal': normal, 'sourceSnapshot': metadata(source_snapshot),
        'sourceSnapshotFiles': len(rows), 'oldHeadFreezeMissingPaths': previous_missing, 'oldHeadFreezeChangedPaths': previous_changed,
        'actions': 'Created only this helper, the specified new ZIP and two JSON reports. No product/runner edits, remote actions, deletions or changes to old artifacts.'}
    report = EVIDENCE / 'native-browser-input-binding.json'
    with report.open('x') as output:
        output.write(json.dumps(binding, ensure_ascii=False, indent=2) + '\n')
    print(json.dumps({'status': binding['status'], 'zip': binding['zip']['path'], 'zipBytes': binding['zip']['bytes'],
                      'zipSha256': binding['zip']['sha256'], 'memberCount': len(members), 'manifestSha256': EXPECTED_MANIFEST,
                      'normalSha256': normal['sha256'], 'sourceSnapshot': str(source_snapshot), 'sourceSnapshotSha256': metadata(source_snapshot)['sha256'],
                      'sourceFiles': len(rows), 'sourceRustFiles': snapshot['rustFiles'], 'sourceWebFiles': snapshot['webFiles'],
                      'sourceUntrackedFiles': snapshot['untrackedFiles'], 'sourceCrateManifests': len(crate_manifests),
                      'oldHeadFreezeMissingPaths': previous_missing, 'oldHeadFreezeChangedPaths': previous_changed,
                      'binding': str(report), 'bindingSha256': metadata(report)['sha256']}, ensure_ascii=False, indent=2))


if __name__ == '__main__':
    main()
