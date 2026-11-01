"""Read every changed Git blob, including ignored raw logs, and compare bytes."""
from pathlib import Path
import argparse
import datetime
import hashlib
import json
import subprocess

MAIN = Path('/workspace/共享/Eliora/未来教室计划')
ARCHIVE = MAIN / '开发计划/验证记录/2026-11-01-round68-production-https'
BASE = '452b41984443d5cacb8b34219c92b64f0287c2ae'

def git(*args, data=None):
    return subprocess.check_output(['git', *args], input=data, cwd=MAIN)

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--commit')
    parser.add_argument('--report', type=Path, required=True)
    args = parser.parse_args()
    assert not args.report.exists()
    scope = args.commit or 'STAGED_INDEX'
    command = ['diff', '--name-only', '-z', BASE, args.commit] if args.commit else ['diff', '--cached', '--name-only', '-z']
    changed = {p.decode('utf-8') for p in git(*command).split(b'\0') if p}
    frozen = json.loads((ARCHIVE / 'joint-implementation-freeze-seventh.json').read_bytes())
    archive_paths = {p.relative_to(MAIN).as_posix() for p in ARCHIVE.rglob('*') if p.is_file()}
    expected = {r['path'] for r in frozen['files']} | {'.gitattributes', '开发计划/10-实施进度与验证记录.md'} | archive_paths
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
              'scope': 'Exact membership and every original Git blob equal to reviewed working bytes, including ignored logs; no private key read.',
              'verifierSha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest()}
    with args.report.open('x') as out:
        json.dump(report, out, ensure_ascii=False, indent=2)
        out.write('\n')
    print(report['status'], report['fileCount'], report['totalBytes'])

if __name__ == '__main__':
    main()
