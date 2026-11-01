#!/usr/bin/env python3
"""Archive terminal, original nonbinary evidence; never read private control/key files."""
from pathlib import Path
import argparse, hashlib, json, runpy, stat

MAIN = Path('/workspace/共享/Eliora/未来教室计划')
SCANNER = MAIN / '开发计划/验证记录/2026-11-01-round46-msvc/archive-r46.py'
MODULE = runpy.run_path(str(SCANNER))
sha = lambda b: hashlib.sha256(b).hexdigest()

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--round', choices=['52', '53', '54'], required=True)
    parser.add_argument('--seal', action='store_true')
    args = parser.parse_args()
    names = {'52': 'ci', '53': 'msi', '54': 'win7-loader'}
    source = Path('/tmp/fc-round' + args.round + '-' + names[args.round] + '-evidence')
    destination = MAIN / ('开发计划/验证记录/2026-11-01-round' + args.round + '-' + names[args.round])
    output = source.parent / ('fc-round' + args.round + '-archive-' + ('sealed' if args.seal else 'preparation') + '-first.json')
    assert not output.exists() and not destination.exists()
    assert source.is_dir() and not source.is_symlink()
    # Bind the established scanner verbatim; its E global is used only to prove
    # that retired synthetic stop capabilities belong to their terminal owner.
    MODULE['expired_stop'].__globals__['E'] = source
    scan = MODULE['Scanner']()
    rows, excluded = [], []
    allowed = MODULE['ALLOWED'] | {'.wxs', '.xml', '.idt'}
    binary = MODULE['BIN_EXT'] | {'.msi', '.cab', '.deb'}
    for path in sorted(source.rglob('*')):
        info = path.lstat()
        if stat.S_ISDIR(info.st_mode):
            continue
        rel = path.relative_to(source).as_posix()
        assert stat.S_ISREG(info.st_mode) and not path.is_symlink(), rel
        if MODULE['sensitive_path'](path):
            excluded.append({'path': rel, 'reason': 'PRIVATE_CONTROL_OR_KEY_NEVER_READ_OR_HASHED'})
            continue
        duplicate = args.round == '53' and any(part in {'payload', 'extracted'} for part in path.relative_to(source).parts)
        if path.suffix.lower() in binary or 'programs' in path.relative_to(source).parts or '__pycache__' in path.parts or duplicate:
            excluded.append({'path': rel, 'bytes': info.st_size, 'reason': 'BINARY_OR_DUPLICATE_PAYLOAD; original reports retain exact hashes and provenance'})
            continue
        if path.suffix.lower() not in allowed and path.name not in {'.owner', 'LICENSE'}:
            scan.finding('UNKNOWN_FILE_TYPE', rel)
            continue
        assert info.st_size <= 64 * 1024 * 1024, rel
        data = path.read_bytes()
        assert not data.startswith((b'MZ', b'PK\x03\x04', bytes.fromhex('d0cf11e0a1b11ae1'))), rel
        encoding = 'utf-16' if data.startswith((b'\xff\xfe', b'\xfe\xff')) else 'utf-8-sig'
        try:
            text = data.decode(encoding)
        except UnicodeError:
            scan.finding('UNCLASSIFIED_NON_TEXT', rel)
            continue
        obj, synthetic = None, False
        if path.suffix == '.json':
            try:
                obj = json.loads(text)
            except ValueError:
                scan.finding('INVALID_JSON', rel)
            if isinstance(obj, dict):
                cleanup = obj.get('cleanupDetails', {})
                synthetic = (str(obj.get('credentialScope', '')).startswith('Disposable synthetic probe account only')
                             and obj.get('cleanup') == 'PASS' and cleanup.get('rootAbsent') is True
                             and cleanup.get('ownedProcessesRemaining') == 0)
        retired = MODULE['expired_stop'](rel)
        scan.text(text, rel, retired=retired, synthetic=synthetic)
        if obj is not None:
            scan.walk(obj, rel, retired=retired, synthetic=synthetic)
        elif path.suffix == '.jsonl':
            for index, line in enumerate(text.splitlines(), 1):
                if line.strip():
                    try:
                        scan.walk(json.loads(line), rel + ':' + str(index), retired=retired, synthetic=synthetic)
                    except ValueError:
                        scan.finding('INVALID_JSONL', rel + ':' + str(index))
        rows.append({'path': rel, 'bytes': len(data), 'sha256': sha(data)})
    assert len(rows) < 10000 and sum(r['bytes'] for r in rows) <= 512 * 1024 * 1024
    report = {'status': 'FAIL_REVIEW_REQUIRED' if scan.blocked else 'PREPARED_ORIGINAL_TEXT_ONLY',
              'source': str(source), 'destination': str(destination), 'files': rows, 'excluded': excluded,
              'scannerSha256': sha(SCANNER.read_bytes()), 'findings': scan.blocked,
              'annotations': scan.annotations, 'decoded': scan.decoded, 'archivePerformed': False}
    if args.seal and not scan.blocked:
        # Caller first verifies every consumer is terminal; this tool does not
        # stop processes, install software, invoke Windows or delete anything.
        destination.mkdir()
        for row in rows:
            data = (source / row['path']).read_bytes()
            assert len(data) == row['bytes'] and sha(data) == row['sha256']
            target = destination / row['path']
            target.parent.mkdir(parents=True, exist_ok=True)
            with target.open('xb') as stream:
                stream.write(data)
            assert target.read_bytes() == data
        (destination / 'INDEX.json').write_text(json.dumps({'source': str(source), 'files': rows, 'excluded': excluded}, ensure_ascii=False, indent=2) + '\n')
        (destination / 'INDEX.md').write_text('Original nonbinary evidence; binaries and private controls excluded.\n' + str(len(rows)) + ' exact byte/SHA-256 checked files. Failed original runs remain unchanged.\n')
        report.update(status='PASS_EXACT_ORIGINAL_TEXT_ARCHIVE', archivePerformed=True)
    with output.open('x') as stream:
        json.dump(report, stream, ensure_ascii=False, indent=2)
        stream.write('\n')
    print(json.dumps({'status': report['status'], 'files': len(rows), 'bytes': sum(r['bytes'] for r in rows), 'findings': scan.blocked, 'report': str(output)}))
    return 1 if scan.blocked else 0

if __name__ == '__main__':
    raise SystemExit(main())
