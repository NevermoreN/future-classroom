"""R73 exact staged/committed blob proof. Does not stage, commit or delete.
Explicit reviewed source and outside-controller paths; complete archive indexed.
Prepared only by default. No executable or network operation.
"""
from pathlib import Path, PurePosixPath
import argparse, hashlib, json, re, stat, subprocess
REPO=Path('/workspace/共享/Eliora/未来教室计划')
CONTROL=Path('/tmp/fc-round73-archive-controller')
PREFIX='开发计划/验证记录/2026-11-01-round73-installer-broker/'
PRODUCER='858d3c62e4d3c4e6739a5763a77e26ccf368038a22d4ef150f5d284a080b5b07'
FREEZE='fbcdee53b97badcdb323e22cfb0649e851798d8756d5552833cf46f959c24131'
SOURCE_BASE='00120a026eea5dc4ff330f8526bf31169b373565'
DOCS={'.gitattributes','AGENTS.md','开发计划/10-实施进度与验证记录.md','开发计划/11-Windows兼容矩阵与验收.md'}
def need(ok,code):
    if not ok:raise AssertionError(code)
def sha(data):return hashlib.sha256(data).hexdigest()
def git(*words):return subprocess.check_output(['git',*words],cwd=REPO)
def relative(name):
    p=PurePosixPath(name);need(isinstance(name,str) and name==p.as_posix() and not p.is_absolute() and '..' not in p.parts and bool(p.parts),'EXACT_REPO_RELATIVE_PATH');return name
def regular(name):
    path=REPO/relative(name);need(not any(p.is_symlink() for p in [path,*path.parents] if p==REPO or REPO in p.parents) and stat.S_ISREG(path.lstat().st_mode),'REGULAR_WORKING_PATH')
    need(path.suffix.lower() not in {'.key','.pfx','.p12'},'PRIVATE_CONTENT_FORBIDDEN');return path
def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--base',required=True);p.add_argument('--commit');p.add_argument('--source-paths',type=Path,required=True);p.add_argument('--outer-path',action='append',default=[]);p.add_argument('--index-sha256',required=True);p.add_argument('--receipt-sha256',required=True);p.add_argument('--report',type=Path,required=True);p.add_argument('--execute',action='store_true');a=p.parse_args()
    need(re.fullmatch('[0-9a-f]{40}',a.base),'EXPLICIT_ACTUAL_MAIN_BASE')
    if not a.execute:print(json.dumps(dict(status='PREPARED_GIT_PROOF_NOT_EXECUTED',sourcePayloadRead=False,gitMutation=False)));return
    need((CONTROL/'.owner').read_bytes()==b'future-classroom-owned-r73-archive-controller/v1\n','EXACT_CONTROLLER_OWNER')
    need(a.report.parent.resolve()==CONTROL and not a.report.exists(),'EXCLUSIVE_OUTSIDE_PROOF')
    archive=REPO/PREFIX;ib=regular(PREFIX+'archive-index-first.json').read_bytes();rb=regular(PREFIX+'archive-verification-first.json').read_bytes()
    need(sha(ib)==a.index_sha256 and sha(rb)==a.receipt_sha256,'EXACT_REVIEWED_ARCHIVE_CONTROLS');index=json.loads(ib);receipt=json.loads(rb)
    need(index['status']=='ORIGINAL_BYTES_ARCHIVED' and index['sourceRoot']=='/tmp/fc-round73-installer-broker/evidence' and receipt['status']=='PASS_EXACT_ORIGINAL_AND_STORED_BYTES_AND_DECODED_CREDENTIAL_SCAN' and receipt['sourceBaseGit']==SOURCE_BASE and receipt['executedCandidateProducerSha256']==PRODUCER and receipt['candidateSourceFreezeSha256']==FREEZE and 'sourceSha' not in receipt,'CANDIDATE_BOUND_ARCHIVE_NOT_GIT_BASE_AS_SOURCE')
    need(receipt['credentialScan']['status']=='PASS_NO_MATCHES' and receipt['credentialScan']['findings']==[],'ARCHIVE_DECODED_SCAN_PASSED')
    pb=regular(PREFIX+'n-release-third.json').read_bytes();fb=regular(PREFIX+'n-implementation-freeze-third.json').read_bytes();need(sha(pb)==PRODUCER and sha(fb)==FREEZE,'EXACT_ARCHIVED_PRODUCER_FREEZE');producer=json.loads(pb)
    need(producer['status']=='PASS_CODEGEN_NOT_RUNTIME' and producer['sourceBefore']==producer['sourceAfter'] and producer['sourceUnchanged'] is True and len(producer['sourceBefore'])==16,'FROZEN_CANDIDATE16')
    need(a.source_paths.resolve()==CONTROL/'candidate-source-paths-first.json' and not a.source_paths.is_symlink(),'EXPLICIT_OWNED_SOURCE_LIST_ONLY')
    candidate={r['path']:r for r in producer['sourceBefore']};source_list=json.loads(a.source_paths.read_bytes());need(isinstance(source_list,list) and len(source_list)==len(set(source_list))==15 and set(source_list)==set(candidate)-{'Cargo.lock'},'EXPLICIT_EXACT15_CANDIDATE_SOURCE_PATHS')
    for name,row in candidate.items():
        data=regular(name).read_bytes();need(len(data)==row['bytes'] and sha(data)==row['sha256'],'CURRENT_SOURCE_AND_CARGO_LOCK_MATCH_PRODUCER')
    outer=set(a.outer_path);need(len(outer)==len(a.outer_path) and all(re.fullmatch(r'开发计划/验证记录/2026-11-01-round73-[A-Za-z0-9_.-]+\.(?:py|json|log|diff)',relative(n)) for n in outer),'EXPLICIT_CLOSED_R73_OUTER_CONTROLLER_ALLOWLIST')
    archived={PREFIX+relative(r.get('archivePath',r['path'])) for r in index['files']}|{PREFIX+'archive-index-first.json'}
    actual_archive={p.relative_to(REPO).as_posix() for p in archive.rglob('*') if p.is_file()};need(actual_archive==archived,'COMPLETE_EXACT_INDEXED_ARCHIVE')
    need(len(archived)==len(index['files'])+1,'UNIQUE_INDEXED_STORED_PATHS')
    for row in index['files']:
        data=regular(PREFIX+row.get('archivePath',row['path'])).read_bytes()
        need(len(data)==row.get('storedBytes',row['bytes']) and sha(data)==row.get('storedSha256',row['sha256']),'EVERY_STORED_MEMBER_MATCHES_REVIEWED_INDEX')
    expected=set(source_list)|DOCS|archived|outer
    need(git('rev-parse',a.base+'^{commit}').decode().strip()==a.base,'EXACT_MAIN_BASE_COMMIT')
    if a.commit:
        need(re.fullmatch('[0-9a-f]{40}',a.commit) and git('rev-parse',a.commit+'^{commit}').decode().strip()==a.commit,'EXACT_COMMITTED_ID')
        need(git('rev-list','--parents','-n','1',a.commit).decode().split()==[a.commit,a.base],'SINGLE_EXPECTED_COMMIT_PARENT')
        changed=git('diff-tree','--no-commit-id','--name-status','--no-renames','-r','-z',a.commit)
        spec_prefix=a.commit+':'
    else:
        need(git('rev-parse','HEAD').decode().strip()==a.base,'STAGED_EXPECTED_HEAD')
        changed=git('diff','--cached','--name-status','--no-renames','-z');spec_prefix=':'
    fields=changed.decode().split('\0');need(fields[-1]=='','NUL_DELIMITED_GIT_PATHS');fields=fields[:-1];need(len(fields)%2==0,'NAME_STATUS_PAIRS')
    changes=list(zip(fields[::2],fields[1::2]));need(all(s in ('A','M') for s,n in changes),'ADDITIONS_MODIFICATIONS_ONLY')
    paths=[n for s,n in changes];need(len(paths)==len(set(paths)) and set(paths)==expected,'EXACT_REVIEWED_CHANGESET_NO_EXTRA_OR_MISSING')
    need(git('show',a.base+':Cargo.lock')==regular('Cargo.lock').read_bytes()==git('show',spec_prefix+'Cargo.lock'),'CARGO_LOCK_GUARD_UNCHANGED_AND_NOT_STAGED_CHANGE')
    if not a.commit:subprocess.run(['git','diff','--cached','--check'],cwd=REPO,check=True,capture_output=True)
    rows=[]
    for name in sorted(paths):
        working=regular(name).read_bytes();spec=spec_prefix+name;blob=git('show',spec);need(working==blob,'EVERY_REVIEWED_WORKING_BYTE_EQUALS_GIT_BLOB')
        rows.append(dict(path=name,bytes=len(blob),sha256=sha(blob),gitBlobOid=git('rev-parse',spec).decode().strip()))
    result=dict(status='PASS_EXACT_COMMITTED_BLOBS' if a.commit else 'PASS_EXACT_STAGED_BLOBS',commit=a.commit,parentGitBase=a.base,candidateGitBaseOnly=SOURCE_BASE,executedCandidateProducerSha256=PRODUCER,candidateSourceFreezeSha256=FREEZE,archiveIndexSha256=a.index_sha256,verificationReceiptSha256=a.receipt_sha256,files=rows,fileCount=len(rows),bytes=sum(r['bytes'] for r in rows),cargoLock='UNCHANGED_GUARD_ONLY_NOT_CHANGED_PATH',scope='Exactly15 frozen candidate source files, four reviewed documentation/attributes paths, complete R73 archive, and explicitly supplied R73 outside controller/proof paths. No staging/commit/network/Windows/Cargo/key-content access or acceptance expansion.')
    with a.report.open('x') as f:json.dump(result,f,ensure_ascii=False,indent=2);f.write('\n')
    print(json.dumps({k:result[k] for k in ('status','commit','fileCount','bytes')}))
if __name__=='__main__':main()
