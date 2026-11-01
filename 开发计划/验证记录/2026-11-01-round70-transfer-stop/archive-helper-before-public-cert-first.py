#!/usr/bin/env python3
"""Archive final R70 evidence only, after root's explicit terminal/PASS gate.

Preserves original bytes, including historical FAILs. No deletion, source edits,
Git, Cargo, Windows, network, compression, or access to other temporary rounds.
Credential patterns/UTF16/nested-JSON base64 scanning follow archived R68's
archive-evidence-third.py. No R68 expired-secret or public-context exceptions
are inherited into this different evidence scope.
"""
from pathlib import Path
import argparse
import base64
import datetime
import hashlib
import json
import re
import stat

SOURCE = Path('/tmp/fc-round70-ordinary-transfer-stop/evidence')
DEST = Path('/workspace/共享/Eliora/未来教室计划/开发计划/验证记录/2026-11-01-round70-transfer-stop')
OWNER = b'future-classroom-owned-r70-ordinary-transfer-stop/v1\n'
EXCLUDED_SUFFIXES = {
    '.exe', '.zip', '.pdb', '.dll', '.lib', '.rlib', '.rmeta', '.pyc',
    '.obj', '.o', '.a', '.so', '.dylib', '.wasm', '.7z', '.tar', '.gz',
    '.db', '.sqlite', '.sqlite3', '.cache', '.key', '.pem', '.pfx', '.p12',
}
PATTERNS = {
    'private-key-pem': re.compile(r'-----BEGIN (?:OPENSSH |RSA |EC |DSA |ENCRYPTED )?PRIVATE KEY-----\s+[A-Za-z0-9+/=\s]{32,}-----END (?:OPENSSH |RSA |EC |DSA |ENCRYPTED )?PRIVATE KEY-----'),
    'github-token': re.compile(r'(?<![A-Za-z0-9])(?:gh[pousr]_[A-Za-z0-9]{30,}|github_pat_[A-Za-z0-9_]{30,})(?![A-Za-z0-9])'),
    'openai-secret': re.compile(r'(?<![A-Za-z0-9])sk-(?:proj-|svcacct-)?[A-Za-z0-9_-]{24,}'),
    'aws-access-key': re.compile(r'(?<![A-Z0-9])(?:AKIA|ASIA)[A-Z0-9]{16}(?![A-Z0-9])'),
    'bearer-secret': re.compile(r'(?i)authorization\s*[=:]\s*[\x22\x27]?Bearer\s+[A-Za-z0-9._~-]{24,}'),
    'basic-secret': re.compile(r'(?i)authorization\s*[=:]\s*[\x22\x27]?Basic\s+[A-Za-z0-9+/]{16,}={0,2}'),
    'signed-download-url': re.compile(r'(?i)https?://[^\s\x22\x27<>]{5,}[?&](?:sig|signature|x-amz-signature)=[A-Za-z0-9%_+/-]{20,}'),
    'session-cookie-bearer': re.compile(r'__Host-FutureClassroom=[0-9a-fA-F]{64}'),
    'json-issued-web-secret': re.compile(r'(?i)[\x22\x27](?:csrf|pollSecret|sessionCookie|accessToken|refreshToken)[\x22\x27]\s*:\s*[\x22\x27][A-Za-z0-9_./+=-]{32,}[\x22\x27]'),
    'json-password-value': re.compile(r'(?i)[\x22\x27]password[\x22\x27]\s*:\s*[\x22\x27][^\x22\x27\n]{12,128}[\x22\x27]'),
}
PAIRING = re.compile(r'(?i)\\?[\x22\x27]pairingCode\\?[\x22\x27]\s*:\s*\\?[\x22\x27]([0-9]{8})\\?[\x22\x27]')


def need(value, message):
    if not value:
        raise AssertionError(message)


def sha(data):
    return hashlib.sha256(data).hexdigest()


def membership():
    need(SOURCE.is_dir() and not SOURCE.is_symlink(), 'regular owned evidence root required')
    paths = []
    for path in SOURCE.rglob('*'):
        mode = path.lstat().st_mode
        need(stat.S_ISDIR(mode) or stat.S_ISREG(mode), 'nonregular evidence entry: ' + str(path.relative_to(SOURCE)))
        if stat.S_ISREG(mode):
            paths.append(path)
    return sorted(paths)


def excluded(path):
    return (path.suffix.lower() in EXCLUDED_SUFFIXES
            or path.name.lower().endswith(('.db-wal', '.db-shm', '.sqlite-wal', '.sqlite-shm'))
            or any(part in ('__pycache__', 'node_modules', '.npm-cache', 'target', '.cache')
                   for part in path.relative_to(SOURCE).parts))


def identity(path):
    need(stat.S_ISREG(path.lstat().st_mode), 'regular original required')
    data = path.read_bytes()
    return data, {'path': path.relative_to(SOURCE).as_posix(), 'bytes': len(data), 'sha256': sha(data)}


def write_new(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open('xb') as output:
        output.write(data)
    need(path.read_bytes() == data, 'archive write readback mismatch: ' + str(path.relative_to(DEST)))


def scanner():
    findings = []
    synthetic = []
    stats = {'textViews': 0, 'utf16Views': 0, 'jsonContainers': 0, 'decodedRawPayloads': 0}

    def scan(data, label, depth=0):
        need(depth <= 12, 'credential decoder depth bound: ' + label)
        views = []
        try:
            views.append(('utf8', data.decode('utf-8-sig')))
        except UnicodeError:
            pass
        if data.startswith((b'\xff\xfe', b'\xfe\xff')) or (len(data) > 4 and data.count(b'\0') > len(data) // 8):
            for encoding in ('utf-16-le', 'utf-16-be'):
                try:
                    views.append((encoding, data.decode(encoding).lstrip('\ufeff')))
                    stats['utf16Views'] += 1
                except UnicodeError:
                    pass
        need(bool(views), 'unrecognized retained evidence encoding: ' + label)
        for encoding, text in views:
            stats['textViews'] += 1
            for kind, pattern in PATTERNS.items():
                for _ in pattern.finditer(text):
                    findings.append({'path': label, 'kind': kind, 'view': encoding})
            for match in PAIRING.finditer(text):
                original = label.split(':', 1)[-1].split('::', 1)[0]
                if match.group(1) == '00000000' and original.endswith(('.py', '.rs', '.patch', '.diff')):
                    synthetic.append({'path': label, 'kind': 'LITERAL_SYNTHETIC_ZERO_TEST_VECTOR', 'view': encoding, 'valueOmitted': True})
                else:
                    findings.append({'path': label, 'kind': 'UNSCOPED_POTENTIAL_PAIRING_SHORT_CODE', 'view': encoding})
            try:
                value = json.loads(text)
            except (ValueError, RecursionError):
                continue
            stats['jsonContainers'] += 1
            budget = [0]

            def walk(item, trail):
                budget[0] += 1
                need(budget[0] <= 250000, 'JSON credential traversal bound: ' + label)
                if isinstance(item, dict):
                    for key, value in item.items():
                        key = str(key)
                        if isinstance(value, str) and (key.lower() in ('base64', 'encodedcommand', 'stdoutbase64', 'stderrbase64') or key.lower().endswith('base64')):
                            try:
                                decoded = base64.b64decode(value, validate=True)
                            except (ValueError, TypeError):
                                continue
                            need(len(decoded) <= 32 * 1024 * 1024, 'decoded credential view bound: ' + label)
                            stats['decodedRawPayloads'] += 1
                            scan(decoded, label + '::' + trail + '/' + key, depth + 1)
                        elif isinstance(value, str) and value[:1] in ('{', '['):
                            try:
                                nested = json.loads(value)
                            except (ValueError, RecursionError):
                                continue
                            walk(nested, trail + '/' + key)
                        elif isinstance(value, (dict, list)):
                            walk(value, trail + '/' + key)
                elif isinstance(item, list):
                    for index, value in enumerate(item):
                        walk(value, trail + '/' + str(index))

            walk(value, '')

    return scan, findings, synthetic, stats


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--acceptance-report', required=True, help='relative final independent PASS JSON under this R70 evidence root')
    parser.add_argument('--expected-acceptance-sha256', required=True)
    parser.add_argument('--root-controllers-terminal', action='store_true', required=True,
                        help='root explicitly confirms all R70 writers/controllers have reached terminal state and been reaped')
    args = parser.parse_args()
    need(re.fullmatch('[0-9a-f]{64}', args.expected_acceptance_sha256), 'exact lowercase SHA256 required')
    owner = SOURCE.parent / '.owner'
    need(owner.is_file() and not owner.is_symlink() and owner.read_bytes() == OWNER, 'exact R70 owner marker required')
    need(not DEST.exists() and not DEST.is_symlink(), 'fresh archive destination required')
    report = SOURCE / args.acceptance_report
    need(report.is_relative_to(SOURCE) and '..' not in Path(args.acceptance_report).parts
         and not Path(args.acceptance_report).is_absolute(), 'acceptance must be relative to owned evidence')
    need(report.resolve().is_relative_to(SOURCE.resolve()), 'acceptance cannot escape owned evidence')
    accepted, acceptance_row = identity(report)
    need(sha(accepted) == args.expected_acceptance_sha256, 'root-confirmed independent acceptance bytes required')
    acceptance = json.loads(accepted.decode('utf-8-sig'))
    need(isinstance(acceptance, dict) and isinstance(acceptance.get('status'), str)
         and (acceptance['status'] == 'PASS' or acceptance['status'].startswith('PASS_')),
         'final independent acceptance must actually PASS')
    originals = membership()
    rows, omitted = [], []
    scan, findings, synthetic, stats = scanner()
    # Scan all retained source bytes before creating the destination. A finding
    # fails closed, reports only location/category, and never modifies originals.
    for path in originals:
        data, row = identity(path)
        if excluded(path):
            row['reason'] = 'Binary, archive, database, key or build-cache payload excluded; original bytes are not committed.'
            omitted.append(row)
        else:
            scan(data, 'source:' + row['path'])
            rows.append(row)
    need(not findings, 'credential findings by path/category only: ' + json.dumps(findings))
    need(membership() == originals, 'source membership changed before copy')
    DEST.mkdir(parents=True)
    for row in rows:
        data, current = identity(SOURCE / row['path'])
        need(current == row, 'original changed after scan: ' + row['path'])
        write_new(DEST / row['path'], data)
    # Independent reread and scan of every stored counterpart, not only digest
    # comparison against a previous read. No gzip or derived source substitution.
    for row in rows:
        data, current = identity(SOURCE / row['path'])
        stored = (DEST / row['path']).read_bytes()
        need(current == row and stored == data, 'source/archive bytes differ: ' + row['path'])
        scan(stored, 'stored:' + row['path'])
    for row in omitted:
        _, current = identity(SOURCE / row['path'])
        need(current == {key: row[key] for key in ('path', 'bytes', 'sha256')}, 'excluded original changed')
    need(not findings, 'stored credential findings by path/category only: ' + json.dumps(findings))
    need(membership() == originals, 'source membership changed during archive')
    receipt = {
        'schemaVersion': 1, 'status': 'PASS_EXACT_ORIGINAL_BYTES_AND_DECODED_CREDENTIAL_SCAN',
        'recordedUtc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
        'sourceRoot': str(SOURCE), 'archiveRoot': str(DEST),
        'acceptance': dict(acceptance_row, status=acceptance['status']),
        'rootControllersTerminalExplicitlyConfirmed': args.root_controllers_terminal,
        'originalFiles': len(rows), 'originalBytes': sum(row['bytes'] for row in rows),
        'credentialScan': {'status': 'PASS', 'patterns': list(PATTERNS), 'statistics': stats,
                           'findings': [], 'syntheticZeroTestVectorOccurrences': synthetic,
                           'coverage': 'Each retained original and independently reopened stored counterpart; UTF8/actual UTF16 views and nested JSON base64/encodedCommand decoded payloads. No expired-credential exception inherited.'},
        'historicalFailures': 'All retained FAIL reports, diagnostic before/after originals, separate C# three-case successes and final four-source records remain exact bytes. No earlier FAIL is promoted to PASS.',
        'scope': 'R70 evidence archive only; runtime/platform claims remain those of the SHA-bound independent acceptance.',
        'sourceRemoved': False, 'compression': 'NONE',
    }
    receipt_bytes = (json.dumps(receipt, ensure_ascii=False, indent=2) + '\n').encode()
    scan(receipt_bytes, 'new-verification-receipt')
    need(not findings, 'credential finding in derived receipt')
    write_new(DEST / 'archive-verification-first.json', receipt_bytes)
    index = {'schemaVersion': 1, 'status': 'ORIGINAL_BYTES_ARCHIVED', 'sourceRoot': str(SOURCE),
             'files': rows, 'excludedPayloads': omitted,
             'verificationReceipt': {'path': 'archive-verification-first.json', 'bytes': len(receipt_bytes), 'sha256': sha(receipt_bytes)}}
    index_bytes = (json.dumps(index, ensure_ascii=False, indent=2) + '\n').encode()
    scan(index_bytes, 'new-archive-index')
    need(not findings, 'credential finding in derived archive index')
    write_new(DEST / 'archive-index-first.json', index_bytes)
    for row in rows:
        data, current = identity(SOURCE / row['path'])
        need(current == row and (DEST / row['path']).read_bytes() == data, 'final exact bytes check: ' + row['path'])
    need(membership() == originals, 'final source membership changed')
    need({p.relative_to(DEST).as_posix() for p in DEST.rglob('*') if p.is_file()}
         == {row['path'] for row in rows} | {'archive-index-first.json', 'archive-verification-first.json'},
         'final archive membership mismatch')
    print(json.dumps({'status': 'PASS', 'originalFiles': len(rows), 'originalBytes': receipt['originalBytes'],
                      'excludedPayloads': len(omitted), 'indexSha256': sha(index_bytes),
                      'verificationReceiptSha256': sha(receipt_bytes), 'credentialScan': 'PASS',
                      'decodedRawPayloads': stats['decodedRawPayloads'], 'sourceRemoved': False}))


if __name__ == '__main__':
    main()
