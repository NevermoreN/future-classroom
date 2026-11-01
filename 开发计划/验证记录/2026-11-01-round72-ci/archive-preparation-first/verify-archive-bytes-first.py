"""Independent R72 source/stored replay, prepared only; never deletes or commits."""
from pathlib import Path
import argparse, datetime, gzip, hashlib, json
SOURCE = Path('/tmp/fc-round72-ci-evidence')
ARCHIVE = Path('/workspace/共享/Eliora/未来教室计划/开发计划/验证记录/2026-11-01-round72-ci')
def need(ok, message):
    if not ok: raise AssertionError(message)
def sha(data): return hashlib.sha256(data).hexdigest()
def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--index-sha256', required=True)
    parser.add_argument('--receipt-sha256', required=True)
    parser.add_argument('--source-sha', required=True)
    parser.add_argument('--report', type=Path, required=True)
    args = parser.parse_args()
    control = Path('/tmp/fc-round72-archive-controller')
    need(args.report.parent.resolve() == control and not args.report.exists(), 'exclusive proof in separately owned controller required')
    need((control / '.owner').read_bytes() == b'future-classroom-owned-r72-archive-controller/v1\n', 'exact outside controller owner')
    need((SOURCE / '.owner').read_bytes() == b'future-classroom-owned-r72-ci-evidence/v1\n', 'exact R72 owner')
    data = (ARCHIVE / 'archive-index-first.json').read_bytes()
    need(sha(data) == args.index_sha256, 'exact final archive index')
    index = json.loads(data); need(index['status'] == 'ORIGINAL_BYTES_ARCHIVED' and index['sourceRoot'] == str(SOURCE), 'archive source/status')
    receipt = ARCHIVE / 'archive-verification-first.json'
    need(sha(receipt.read_bytes()) == args.receipt_sha256, 'exact archive verification receipt')
    r = json.loads(receipt.read_bytes()); need(r['sourceSha'] == args.source_sha and r['status'] == 'PASS_EXACT_ORIGINAL_AND_STORED_BYTES_AND_DECODED_CREDENTIAL_SCAN', 'source-bound archive verification')
    need(r['credentialScan']['status'] == 'PASS_NO_MATCHES' and not r['credentialScan']['findings'], 'original decoded credential scan passed')
    rows = index['files']; excluded = index['excludedCompiledPayloads']
    need(len({row['path'] for row in rows + excluded}) == len(rows) + len(excluded), 'unique complete indexed paths')
    replay = []; packed_count = 0
    for row in rows:
        original = SOURCE / row['path']; stored = ARCHIVE / row.get('archivePath', row['path'])
        need(not original.is_symlink() and not stored.is_symlink(), 'regular original/stored members')
        a = original.read_bytes(); b = stored.read_bytes()
        need(len(a) == row['bytes'] and sha(a) == row['sha256'], 'source original exact')
        need(len(b) == row.get('storedBytes', row['bytes']) and sha(b) == row.get('storedSha256', row['sha256']), 'stored original exact')
        if row.get('storage') == 'gzip_original_bytes': b = gzip.decompress(b); packed_count += 1
        need(a == b, 'independent decompressed/source byte equality')
        replay.append(dict(path=row['path'], bytes=len(a), sha256=sha(a), archivePath=row.get('archivePath', row['path'])))
    omitted = []
    for row in excluded:
        path = SOURCE / row['path']; need(not path.is_symlink(), 'regular excluded member')
        a = path.read_bytes(); need(len(a) == row['bytes'] and sha(a) == row['sha256'], 'excluded payload identity exact')
        omitted.append(dict(path=row['path'], bytes=len(a), sha256=sha(a)))
    # The future proof output is created only after complete exact membership
    # replay. Once emitted it is outside the immutable source root and must
    # be retained separately by the root's Git/cleanup controller.
    source_members = {p.relative_to(SOURCE).as_posix() for p in SOURCE.rglob('*') if p.is_file()}
    need(source_members == {row['path'] for row in rows + excluded}, 'source membership unchanged since archive')
    need({p.relative_to(ARCHIVE).as_posix() for p in ARCHIVE.rglob('*') if p.is_file()} == {row.get('archivePath', row['path']) for row in rows} | {'archive-index-first.json'}, 'exact archive membership')
    result = dict(status='PASS_R72_EXACT_SOURCE_STORED_AND_GZIP_REPLAY_ONLY', schemaVersion=1,
        recordedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(), sourceSha=args.source_sha,
        archiveIndexSha256=args.index_sha256, verificationReceiptSha256=args.receipt_sha256,
        files=replay, excludedPayloads=omitted, gzipFiles=packed_count, sourceRemoved=False,
        scope='Byte replay only. No new CI/runtime/OS acceptance, execution, Git mutation, cleanup or key-content access.',
        membershipQualification='This exclusive new replay proof was absent from the already archived source snapshot and is deliberately retained in the separately owned controller root.')
    with args.report.open('x') as stream: json.dump(result, stream, ensure_ascii=False, indent=2); stream.write('\n')
    print(json.dumps(dict(status=result['status'], fileCount=len(rows), excluded=len(excluded), reportSha256=sha(args.report.read_bytes()))))
if __name__ == '__main__': main()
