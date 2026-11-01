"""Independent R73 exact original/stored replay. Prepared only by default.
No archive creation, deletion, Git mutation, execution or private-key access.
"""
from pathlib import Path, PurePosixPath
import argparse, datetime, gzip, hashlib, json, stat
CONTROL = Path('/tmp/fc-round73-archive-controller')
SOURCE = Path('/tmp/fc-round73-installer-broker/evidence')
ARCHIVE = Path('/workspace/共享/Eliora/未来教室计划/开发计划/验证记录/2026-11-01-round73-installer-broker')
BASE = '00120a026eea5dc4ff330f8526bf31169b373565'
PRODUCER = '858d3c62e4d3c4e6739a5763a77e26ccf368038a22d4ef150f5d284a080b5b07'
FREEZE = 'fbcdee53b97badcdb323e22cfb0649e851798d8756d5552833cf46f959c24131'
def need(ok, code):
    if not ok: raise AssertionError(code)
def sha(data): return hashlib.sha256(data).hexdigest()
def safe(root, name):
    rel = PurePosixPath(name)
    need(isinstance(name,str) and name == rel.as_posix() and not rel.is_absolute() and '..' not in rel.parts and bool(rel.parts), 'EXACT_RELATIVE_MEMBER')
    path = root / name
    need(not any(p.is_symlink() for p in [root, path, *path.parents] if p == root or root in p.parents), 'NO_MEMBER_OR_PARENT_SYMLINK')
    need(path.resolve().is_relative_to(root.resolve()) and stat.S_ISREG(path.lstat().st_mode), 'SCOPED_REGULAR_MEMBER')
    need(path.suffix.lower() not in {'.key','.pfx','.p12'}, 'PRIVATE_MATERIAL_NOT_READ')
    return path
def exact(root,row,stored=False):
    name=row.get('archivePath',row['path']) if stored else row['path'];data=safe(root,name).read_bytes()
    size=row.get('storedBytes',row['bytes']) if stored else row['bytes']
    digest=row.get('storedSha256',row['sha256']) if stored else row['sha256']
    need(len(data)==size and sha(data)==digest,'EXACT_MEMBER_IDENTITY')
    return data
def members(root):
    paths=list(root.rglob('*'));need(all(not p.is_symlink() and (p.is_dir() or stat.S_ISREG(p.lstat().st_mode)) for p in paths),'REGULAR_COMPLETE_MEMBERSHIP')
    return {p.relative_to(root).as_posix() for p in paths if p.is_file()}
def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--index-sha256',required=True);p.add_argument('--receipt-sha256',required=True)
    p.add_argument('--source-base-git',required=True);p.add_argument('--report',type=Path,required=True);p.add_argument('--execute',action='store_true');a=p.parse_args()
    need(a.source_base_git==BASE,'GIT_BASE_ONLY_NOT_CANDIDATE_SHA')
    if not a.execute: print(json.dumps(dict(status='PREPARED_REPLAY_NOT_EXECUTED',sourceBaseGit=BASE,candidateProducerSha256=PRODUCER,sourcePayloadRead=False)));return
    need(a.report.parent.resolve()==CONTROL and not a.report.exists(),'EXCLUSIVE_OUTSIDE_PROOF')
    need((CONTROL/'.owner').read_bytes()==b'future-classroom-owned-r73-archive-controller/v1\n','EXACT_CONTROLLER_OWNER')
    need((SOURCE/'.owner').read_bytes()==b'future-classroom-owned-r73-installer-broker-evidence/v1\n','EXACT_EVIDENCE_OWNER')
    index_bytes=safe(ARCHIVE,'archive-index-first.json').read_bytes();need(sha(index_bytes)==a.index_sha256,'EXACT_FINAL_INDEX');index=json.loads(index_bytes)
    receipt_bytes=safe(ARCHIVE,'archive-verification-first.json').read_bytes();need(sha(receipt_bytes)==a.receipt_sha256,'EXACT_FINAL_RECEIPT');receipt=json.loads(receipt_bytes)
    need(index['status']=='ORIGINAL_BYTES_ARCHIVED' and index['sourceRoot']==str(SOURCE),'EXACT_INDEX_SCOPE')
    need(receipt['status']=='PASS_EXACT_ORIGINAL_AND_STORED_BYTES_AND_DECODED_CREDENTIAL_SCAN' and receipt['sourceRoot']==str(SOURCE) and receipt['archiveRoot']==str(ARCHIVE),'EXACT_RECEIPT_SCOPE')
    need(receipt['sourceBaseGit']==BASE and receipt['executedCandidateProducerSha256']==PRODUCER and receipt['candidateSourceFreezeSha256']==FREEZE and 'sourceSha' not in receipt,'DISTINCT_BASE_AND_EXECUTED_CANDIDATE')
    need(receipt['credentialScan']['status']=='PASS_NO_MATCHES' and receipt['credentialScan']['findings']==[],'RETAINED_DECODED_CREDENTIAL_SCAN_PASSED')
    need(sha(safe(ARCHIVE,'n-release-third.json').read_bytes())==PRODUCER and sha(safe(ARCHIVE,'n-implementation-freeze-third.json').read_bytes())==FREEZE,'EXACT_ARCHIVED_CANDIDATE_BINDING')
    producer=json.loads(safe(ARCHIVE,'n-release-third.json').read_bytes());need(producer['status']=='PASS_CODEGEN_NOT_RUNTIME' and producer['sourceBefore']==producer['sourceAfter'] and producer['sourceUnchanged'] is True and producer['freezeSha256']==FREEZE,'EXACT_CANDIDATE_FROZEN_SOURCE')
    rows=index['files'];excluded=index['excludedCompiledPayloads'];need(len({r['path'] for r in rows+excluded})==len(rows)+len(excluded),'UNIQUE_ALL_SOURCE_MEMBERS')
    need(len({r.get('archivePath',r['path']) for r in rows})==len(rows),'UNIQUE_ALL_STORED_MEMBERS')
    replay=[];packed=0
    for row in rows:
        original=exact(SOURCE,row);stored=exact(ARCHIVE,row,True)
        need(row.get('storage') in (None,'gzip_original_bytes'),'CLOSED_STORAGE_MODE')
        if row.get('storage')=='gzip_original_bytes':stored=gzip.decompress(stored);packed+=1
        need(stored==original,'INDEPENDENT_DECOMPRESSED_BYTE_EQUALITY')
        replay.append(dict(path=row['path'],bytes=len(original),sha256=sha(original),archivePath=row.get('archivePath',row['path'])))
    omitted=[]
    for row in excluded:
        data=exact(SOURCE,row);omitted.append(dict(path=row['path'],bytes=len(data),sha256=sha(data)))
    need(members(SOURCE)=={r['path'] for r in rows+excluded},'COMPLETE_IMMUTABLE_SOURCE_MEMBERSHIP')
    need(members(ARCHIVE)=={r.get('archivePath',r['path']) for r in rows}|{'archive-index-first.json'},'COMPLETE_ARCHIVE_MEMBERSHIP')
    need(index['verificationReceipt']==next(r for r in rows if r['path']=='archive-verification-first.json'),'EXACT_INDEXED_RECEIPT')
    need(safe(ARCHIVE,'archive-index-first.json').read_bytes()==index_bytes and safe(ARCHIVE,'archive-verification-first.json').read_bytes()==receipt_bytes,'CONTROL_ORIGINALS_UNCHANGED')
    result=dict(status='PASS_R73_EXACT_SOURCE_STORED_AND_GZIP_REPLAY_ONLY',recordedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),sourceBaseGit=BASE,executedCandidateProducerSha256=PRODUCER,candidateSourceFreezeSha256=FREEZE,archiveIndexSha256=a.index_sha256,verificationReceiptSha256=a.receipt_sha256,files=replay,excludedPayloads=omitted,gzipFiles=packed,sourceRemoved=False,scope='Byte replay only; no new runtime/OS/MSI/CI acceptance. Git base is not the uncommitted candidate source identity. No key-content access or mutation.',membershipQualification='Exclusive proof is outside immutable evidence membership.')
    with a.report.open('x') as f:json.dump(result,f,ensure_ascii=False,indent=2);f.write('\n')
    print(json.dumps(dict(status=result['status'],fileCount=len(rows),excluded=len(excluded),reportSha256=sha(a.report.read_bytes()))))
if __name__=='__main__':main()
