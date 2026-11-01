"""Read every changed Git blob, including ignored raw logs, and compare bytes."""
from pathlib import Path
import argparse
import datetime
import gzip
import hashlib
import json
import subprocess

MAIN = Path('/workspace/共享/Eliora/未来教室计划')
ARCHIVE = MAIN / '开发计划/验证记录/2026-11-01-round69-ci'
BASE = '8bc1c9812df765865a54988a23ff3973d04884fc'
CONTROL = Path('/tmp/fc-round69-archive-controller')
INDEX_SHA = 'ed374e6aea90508343c4c3a3d6c99e9e72882655301780d29242238e65c28efd'

def git(*args, data=None):
    return subprocess.check_output(['git', *args], input=data, cwd=MAIN)

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--commit')
    parser.add_argument('--report', type=Path, required=True)
    args = parser.parse_args()
    assert not args.report.exists()
    if args.commit:
        assert git('rev-parse', args.commit + '^').decode().strip() == BASE
    else:
        assert git('rev-parse', 'HEAD').decode().strip() == BASE
    scope = args.commit or 'STAGED_INDEX'
    command = ['diff', '--name-only', '-z', BASE, args.commit] if args.commit else ['diff', '--cached', '--name-only', '-z']
    changed = {p.decode('utf-8') for p in git(*command).split(b'\0') if p}
    assert (CONTROL / '.owner').read_bytes() == b'future-classroom-owned-r69-archive-controller/v1\n'
    assert args.report.parent.resolve() == CONTROL and not args.report.is_symlink()
    index_path = ARCHIVE / 'archive-index-first.json'
    assert hashlib.sha256(index_path.read_bytes()).hexdigest() == INDEX_SHA
    index = json.loads(index_path.read_bytes())
    assert index['status'] == 'ORIGINAL_BYTES_ARCHIVED' and index['originalFiles'] == len(index['files']) == 508
    membership = {row.get('archivePath', row['path']) for row in index['files']} | {'archive-index-first.json'}
    assert len(membership) == 509
    actual_archive = {p.relative_to(ARCHIVE).as_posix() for p in ARCHIVE.rglob('*') if p.is_file()}
    assert actual_archive == membership
    for row in index['files']:
        path = ARCHIVE / row.get('archivePath', row['path'])
        assert not path.is_symlink()
        data = path.read_bytes()
        assert len(data) == row.get('storedBytes', row['bytes'])
        assert hashlib.sha256(data).hexdigest() == row.get('storedSha256', row['sha256'])
        original = gzip.decompress(data) if row.get('storage') == 'gzip_original_bytes' else data
        assert len(original) == row['bytes'] and hashlib.sha256(original).hexdigest() == row['sha256']
    runtime = json.loads((ARCHIVE / 'independent-runtime-first.json').read_bytes())
    assert runtime['status'] == 'PASS_R69_MODERN_WIN11_REGRESSION_WITH_WIN7_CI_FAILURE_RETAINED' and runtime['sourceSha'] == BASE
    assert runtime['modern']['status'] == 'PASS_ACTUAL_MODERN_CI_AND_FINITE_WIN11_RUNTIME_ONLY'
    assert runtime['win7']['status'] == 'ACTUAL_CI_FAIL_RETAINED_NOT_RUNTIME_APPROVED' and runtime['win7']['runtime'] == 'NOT_RUN_NOT_APPROVED'
    operation_prefix = '开发计划/验证记录/2026-11-01-round69-archive-operation-first'
    operation = json.loads((MAIN / (operation_prefix + '.json')).read_bytes())
    assert operation['status'] == 'PASS' and operation['exitCode'] == 0
    for stream in ('stdout', 'stderr'):
        row = operation[stream]
        actual = (MAIN / (operation_prefix + '.' + stream + '.log')).read_bytes()
        assert len(actual) == row['bytes'] and hashlib.sha256(actual).hexdigest() == row['sha256']
        assert actual == Path(row['path']).read_bytes()
    assert (MAIN / (operation_prefix + '.json')).read_bytes() == (CONTROL / 'operation-first.json').read_bytes()
    archive_paths = {p.relative_to(MAIN).as_posix() for p in ARCHIVE.rglob('*') if p.is_file()}
    expected = {'.gitattributes', '开发计划/10-实施进度与验证记录.md', Path(__file__).relative_to(MAIN).as_posix()} | archive_paths | {operation_prefix + suffix for suffix in ('.json', '.stdout.log', '.stderr.log')}
    assert len(expected) == 515 and len(archive_paths) == 509
    assert changed == expected, {'missing': sorted(expected - changed), 'unexpected': sorted(changed - expected)}
    tree = {}
    if args.commit:
        for row in git('ls-tree', '-r', '-z', '--full-tree', args.commit).split(b'\0'):
            if not row:
                continue
            header, path = row.split(b'\t', 1)
            mode, kind, blob = header.split()
            if path.decode('utf-8') in changed:
                assert kind == b'blob' and mode == b'100644'
                tree[path.decode('utf-8')] = blob.decode('ascii')
    else:
        for row in git('ls-files', '--stage', '-z').split(b'\0'):
            if not row:
                continue
            header, path = row.split(b'\t', 1)
            mode, blob, stage = header.split()
            if path.decode('utf-8') in changed:
                assert stage == b'0' and mode == b'100644'
                tree[path.decode('utf-8')] = blob.decode('ascii')
    assert set(tree) == expected
    paths = sorted(tree)
    payload = git('cat-file', '--batch', data=''.join(tree[p] + '\n' for p in paths).encode('ascii'))
    cursor = 0
    rows = []
    for path in paths:
        end = payload.index(b'\n', cursor)
        blob, kind, size = payload[cursor:end].split()
        assert blob.decode() == tree[path] and kind == b'blob'
        size = int(size)
        cursor = end + 1
        data = payload[cursor:cursor + size]
        cursor += size
        assert payload[cursor:cursor + 1] == b'\n'
        cursor += 1
        assert data == (MAIN / path).read_bytes(), path
        rows.append({'path': path, 'bytes': size, 'sha256': hashlib.sha256(data).hexdigest(), 'gitBlob': tree[path]})
    assert cursor == len(payload)
    report = {'status': 'PASS_EXACT_COMMITTED_BYTES' if args.commit else 'PASS_EXACT_STAGED_BYTES',
              'recordedUtc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
              'baseCommit': BASE, 'verifiedCommit': scope, 'files': rows,
              'fileCount': len(rows), 'archiveFileCount': len(archive_paths),
              'totalBytes': sum(r['bytes'] for r in rows),
              'archiveIndexSha256': INDEX_SHA,
              'scope': 'R69 exact515 membership:509 archived original/stored files,2 metadata,3 actual archive-controller records and this helper. Every Git blob equals reviewed working bytes, including ignored logs; modern CI/Win11 PASS and original Win7 CI failure/NOT_RUN remain separate. No private key read, push or cleanup.',
              'verifierSha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest()}
    with args.report.open('x') as out:
        json.dump(report, out, ensure_ascii=False, indent=2)
        out.write('\n')
    print(report['status'], report['fileCount'], report['totalBytes'])

if __name__ == '__main__':
    main()
