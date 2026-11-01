#!/usr/bin/env python3
"""Prepare by default. Root supplies --commit40 --committed-proof --outer-copy-map
--controllers-reaped --report ... --execute after committing source+archive.
Proof: {commit:40hex,files:[{path:mainRelative,bytes,sha256}]}; outer map:
{files:[{source:exactOuterTopFile,destination:mainRelative,bytes,sha256}]}.
Proof may itself be an uncommitted main JSON; the map and this script must be
committed/proved. No Windows/network/kill. Only the two exact R75 roots removed.
"""
from pathlib import Path, PurePosixPath
import argparse, gzip, hashlib, json, os, re, runpy, shutil, stat, subprocess, sys
sys.dont_write_bytecode = True
MAIN=Path('/workspace/共享/Eliora/未来教室计划')
ROOT=Path('/tmp/fc-round75-installer-maintenance'); OUTER=Path('/tmp/fc-round75-archive-controller')
ARCH=MAIN/'开发计划/验证记录/2026-11-01-round75-installer-maintenance'; W=ROOT/'worktree'
BASE='2dfb2e7e07ace64c61abf66c9a423aa815de7110'; BRANCH='codex/installer-maintenance'
J_SHA='d5d22e44e81e8f15c9ffb956d89a81f56be29d4e63a02e2dba7a1ab4b47e9590'
PRODUCER_SHA='c2097456ca15a57699c6e297f5f0cf772b42ef1397c368a51733405831fc6414'
PRUNED={'worktree','target','tools'}
def need(value,code):
    if not value: raise AssertionError(code)
def sha(data): return hashlib.sha256(data).hexdigest()
def rel(name):
    p=PurePosixPath(name);need(name==p.as_posix() and not p.is_absolute() and '..' not in p.parts,'RELATIVE_PATH');return p

def member(base,name):
    p=base/str(rel(name));cursor=p
    while cursor!=base:
        need(not cursor.is_symlink(),'SYMLINK_MEMBER');cursor=cursor.parent
    need(not base.is_symlink(),'SYMLINK_ROOT');return p

def data(path):
    need(stat.S_ISREG(path.lstat().st_mode) and path.suffix.lower() not in {'.key','.pfx','.p12'},'REGULAR_NONPRIVATE_INPUT')
    return path.read_bytes()
def identity(path,size,digest):
    b=data(path);need(len(b)==size and sha(b)==digest,'ORIGINAL_BYTES_IDENTITY');return b

def inventory(base,pruned=False,allow_links=False):
    rows={};counts={'regularFiles':0,'logicalBytes':0,'directories':0,'symlinksUnlinkedOnly':0}
    def walk(directory):
        for p in sorted(directory.iterdir()):
            if pruned and directory==base and p.name in PRUNED: continue
            s=p.lstat();name=p.relative_to(base).as_posix()
            if stat.S_ISLNK(s.st_mode):
                need(allow_links and not pruned,'UNEXPECTED_SYMLINK');counts['symlinksUnlinkedOnly']+=1
            elif stat.S_ISDIR(s.st_mode):counts['directories']+=1;walk(p)
            else:
                need(stat.S_ISREG(s.st_mode),'SPECIAL_MEMBER');rows[name]=s.st_size
                counts['regularFiles']+=1;counts['logicalBytes']+=s.st_size
    walk(base);return rows,counts

def git(*args):return subprocess.check_output(['git','-C',str(MAIN),*args],stderr=subprocess.PIPE,timeout=60)
def processes_clear():
    rows={}
    for line in subprocess.check_output(['ps','-eo','pid=,ppid=,args='],timeout=15).decode().splitlines():
        parts=line.strip().split(None,2)
        if len(parts)==3:rows[int(parts[0])]=(int(parts[1]),parts[2])
    excluded=set();p=os.getpid()
    while p and p not in excluded:excluded.add(p);p=rows.get(p,(0,''))[0]
    active=[pid for pid,(_,cmd) in rows.items() if pid not in excluded and any(str(r) in cmd for r in (ROOT,OUTER))]
    need(not active,'RELATED_PROCESSES_STILL_ACTIVE')

def verify(a,result):
    need(re.fullmatch('[0-9a-f]{40}',a.commit40 or '') and a.controllers_reaped,'EXPLICIT_COMMIT_AND_REAPED_CONFIRMATION')
    need(git('rev-parse','HEAD').decode().strip()==a.commit40,'MAIN_HEAD_EXACT_COMMIT')
    need(ROOT.is_dir() and not ROOT.is_symlink() and OUTER.is_dir() and not OUTER.is_symlink(),'EXACT_REAL_OWNED_ROOTS')
    need(data(ROOT/'.owner')==b'future-classroom-owned-r75-installer-maintenance/v1\n','ROOT_OWNER')
    need(data(OUTER/'.owner')==b'future-classroom-owned-r75-archive-controller/v1\n','OUTER_OWNER')
    proofpath=member(MAIN,str(a.committed_proof.relative_to(MAIN)));proof=json.loads(data(proofpath))
    need(proof['commit']==a.commit40,'PROOF_COMMIT');proved={}
    for row in proof['files']:
        name=str(rel(row['path']));need(name not in proved,'DUPLICATE_PROOF_PATH')
        b=identity(member(MAIN,name),row['bytes'],row['sha256'])
        need(git('show',a.commit40+':'+name)==b,'COMMITTED_GIT_BLOB_BYTES');proved[name]=row
    changed=set([x.decode() for x in git('diff-tree','--no-commit-id','--name-only','-z','-r',a.commit40).split(b'\0') if x])
    need(changed==set(proved),'PROOF_ALL_COMMIT_CHANGED_FILES_EXACT')
    required={Path(__file__).relative_to(MAIN).as_posix()}
    mapname=a.outer_copy_map.relative_to(MAIN).as_posix();required.add(mapname)
    mapbytes=data(member(MAIN,mapname));need(sha(mapbytes)=='9d854785e07d65918bda9f2a159065cb4c5c5fa6bd9c55c811e08b37df52f9ec','EXACT_ROOT_OUTER_COPY_MAP');copy_map=json.loads(mapbytes)['files']
    indexpath=ARCH/'archive-index-first.json';index=json.loads(data(indexpath))
    need(index['status']=='ORIGINAL_BYTES_ARCHIVED' and index['sourceRoot']==str(ROOT) and len(index['files'])==index['originalFiles']==1517,'FINAL_INDEX_SCOPE_COUNT')
    rows={r['path']:r for r in index['files']};omitted={r['path']:r for r in index['excludedCompiledPayloads']}
    need(len(rows)==1517 and len(omitted)==56 and len(index['explicitFinalProducerTargetPeIdentities'])==24,'EXACT_ARCHIVE_AND_EXCLUDED_COUNTS')
    need(set(index['prunedInfrastructureSubtrees'])==PRUNED,'EXACT_PRUNED_SUBTREES')
    required.add(indexpath.relative_to(MAIN).as_posix())
    archived={r.get('archivePath',r['path']) for r in rows.values()}|{'archive-index-first.json'}
    actual_archive,_=inventory(ARCH);need(set(actual_archive)==archived,'COMPLETE_MAIN_ARCHIVE_MEMBERSHIP')
    for name,row in rows.items():
        original=identity(member(ROOT,name),row['bytes'],row['sha256'])
        stored=member(ARCH,row.get('archivePath',name));required.add(stored.relative_to(MAIN).as_posix())
        b=identity(stored,row.get('storedBytes',row['bytes']),row.get('storedSha256',row['sha256']))
        need((gzip.decompress(b) if row.get('storage')=='gzip_original_bytes' else b)==original,'STORED_OR_DECOMPRESSED_ORIGINAL')
    for name,row in omitted.items():identity(member(ROOT,name),row['bytes'],row['sha256'])
    for row in index['explicitFinalProducerTargetPeIdentities']:
        need(row['path'].startswith('target/') and row['producerSha256']==PRODUCER_SHA,'FINAL_PE_SCOPE');identity(member(ROOT,row['path']),row['bytes'],row['sha256'])
    unpruned,_=inventory(ROOT,True);need(set(unpruned)==set(rows)|set(omitted),'UNPRUNED_ORIGINAL_MEMBERSHIP')
    helper=ARCH/'evidence/archive-preparation-third/archive-evidence-third.py'
    # Only committed code is imported; no main() call and no pycache writes.
    need(helper.relative_to(MAIN).as_posix() in proved,'COMMITTED_ARCHIVE_VERIFIER')
    h=runpy.run_path(str(helper));containers=index['reviewedBinaryContainerOmissions']
    need(len(containers)==3,'EXACT_THREE_OMISSIONS')
    first=next(iter(containers.values()))
    need(h['reviewed_binary_containers'](ROOT/first['manifestPath'],first['manifestSha256'])==containers,'EXACT_REVIEWED_OMISSION_SEMANTICS')
    joint=json.loads(identity(ROOT/'evidence/root-joint-source-freeze-fourth.json',len(data(ROOT/'evidence/root-joint-source-freeze-fourth.json')),J_SHA))
    producer=json.loads(identity(ROOT/'evidence/n-release-third.json',len(data(ROOT/'evidence/n-release-third.json')),PRODUCER_SHA))
    source={r['path']:r for r in joint['sources']};need(len(source)==8 and source=={r['path']:r for r in producer['sourceBefore']}=={r['path']:r for r in producer['sourceAfter']},'JOINT_PRODUCER_EIGHT_SOURCES')
    seven=set(source)-{'Cargo.lock'};need(len(seven)==7,'SEVEN_CHANGED_SOURCE_FILES')
    for name,row in source.items():
        b=identity(member(MAIN,name),row['bytes'],row['sha256']);need(git('show',a.commit40+':'+name)==b,'MAIN_SOURCE_GIT_BLOB')
        need(identity(member(W,name),row['bytes'],row['sha256'])==b,'WORKTREE_SOURCE_EXACT')
    required.update(seven)
    need(subprocess.check_output(['git','-C',str(W),'rev-parse','HEAD'],timeout=15).decode().strip()==BASE,'WORKTREE_BASE')
    need(subprocess.check_output(['git','-C',str(W),'branch','--show-current'],timeout=15).decode().strip()==BRANCH,'WORKTREE_BRANCH')
    dirty=subprocess.check_output(['git','-C',str(W),'status','--porcelain','-z','--untracked-files=all'],timeout=30).split(b'\0')
    dirty=[r.decode() for r in dirty if r];need({r[3:] for r in dirty}==seven and all(r[:3] in (' M ','?? ') for r in dirty),'ONLY_EXACT_SEVEN_DIRTY_SOURCE')
    need(git('rev-parse','refs/heads/'+BRANCH).decode().strip()==BASE,'BRANCH_STILL_BASE')
    top={p.name for p in OUTER.iterdir()};expected_top={'.owner','first-partial-archive','second-partial-archive','independent-byte-replay-third.json','independent-byte-replay-fourth.json'}|{f'{stem}-{n}.{suffix}' for n in ('first','second','third') for stem,suffix in [('archive','stdout.log'),('archive','stderr.log'),('archive-operation','json'),('command','json')]}
    need(top==expected_top,'OUTER_EXACT_TOP_MEMBERS')
    mapped=set()
    for row in copy_map:
        src=Path(row['source']);need(src.parent==OUTER and src.name in top and src.is_file() and src.name not in mapped,'EXACT_OUTER_FILE_MAP')
        b=identity(src,row['bytes'],row['sha256']);dest=str(rel(row['destination']));required.add(dest)
        need(identity(member(MAIN,dest),row['bytes'],row['sha256'])==b,'OUTER_COPIED_ORIGINAL');mapped.add(src.name)
    need(mapped==top-{'first-partial-archive','second-partial-archive'},'ALL_OUTER_TOP_FILES_COPIED')
    for n,exitcode in [('first',1),('second',1),('third',0)]:
        op=json.loads(data(OUTER/f'archive-operation-{n}.json'));need(op['actualParentReturned'] is True and op['exitCode']==exitcode,'ARCHIVE_CONTROLLER_TERMINAL')
    for n in ('first','second'):
        partial=OUTER/f'{n}-partial-archive';inv=json.loads(data(ARCH/f'evidence/archive-attempt-{n}/partial-archive-inventory-{n}.json'))
        need(inv['retainedPartialPath']==str(partial) and inv['status']=='PASS_FAILED_PARTIAL_MOVED_AND_ORIGINALS_PRESERVED_NOT_FINAL_ARCHIVE','PARTIAL_SCOPE')
        files=inv['files'];actual,_=inventory(partial);need(set(actual)=={r['archivePath'] for r in files} and len(files)==inv['storedFileCount'],'PARTIAL_COMPLETE_MEMBERSHIP')
        for row in files:
            b=identity(member(partial,row['archivePath']),row['storedBytes'],row['storedSha256'])
            original=identity(member(ROOT,row['originalPath']),row['originalBytes'],row['originalSha256'])
            need((gzip.decompress(b) if row['archivePath']!=row['originalPath'] else b)==original,'PARTIAL_STILL_ORIGINAL_BYTES')
    need(required<=set(proved),'ALL_REQUIRED_ARCHIVE_SOURCE_CONTROLLER_COPIES_COMMITTED')
    replay=json.loads(data(OUTER/'independent-byte-replay-fourth.json'));need(replay['status']=='PASS_INDEPENDENT_R75_EXACT_BYTES_AND_BINARY_OMISSION_REPLAY_ONLY' and replay['indexSha256']==sha(data(indexpath)),'FINAL_INDEPENDENT_BYTE_REPLAY_PASS')
    processes_clear();need(shutil.rmtree.avoids_symlink_attacks,'FD_SAFE_NONFOLLOW_REMOVAL_REQUIRED')
    _,root_counts=inventory(ROOT,allow_links=True);_,outer_counts=inventory(OUTER,allow_links=True)
    result.update(verifiedCommit=a.commit40,provedFiles=len(proved),archiveOriginalFiles=1517,excluded56AndFinal24Verified=True,omissionThreeVerified=True,worktreeSevenPreserved=True,plannedRemoval={'root':root_counts,'outer':outer_counts},measurement='regular logical file bytes; no physical disk-space claim',proofSha256=sha(data(proofpath)),outerCopyMapSha256=sha(data(member(MAIN,mapname))))
    return root_counts,outer_counts

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--commit40');p.add_argument('--committed-proof',type=Path);p.add_argument('--outer-copy-map',type=Path);p.add_argument('--controllers-reaped',action='store_true');p.add_argument('--execute',action='store_true');p.add_argument('--report',type=Path);a=p.parse_args()
    if not a.execute:print(json.dumps({'status':'PREPARED_NO_ACTIONS','roots':[str(ROOT),str(OUTER)],'actualCleanup':'NOT_RUN'}));return 0
    need(a.report and a.committed_proof and a.outer_copy_map,'EXPLICIT_MAIN_INPUTS_AND_REPORT')
    report=member(MAIN,a.report.relative_to(MAIN).as_posix());need(report.parent==MAIN/'开发计划/验证记录' and not report.exists(),'FRESH_MAIN_VALIDATION_REPORT')
    result={'status':'NOT_RUN_BLOCKED','commands':[],'rootsRemoved':[]}
    try:
        verify(a,result)
        def mutate(command):
            child=subprocess.Popen(command,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
            try:out,err=child.communicate(timeout=120)
            except subprocess.TimeoutExpired:
                # Never kill a process as a cleanup shortcut. Retain roots and
                # report the live original PID for root inspection.
                result['commands'].append({'command':command,'pid':child.pid,'actualExit':False,'timedOut':True});raise AssertionError('ORIGINAL_GIT_COMMAND_NOT_RETURNED')
            result['commands'].append({'command':command,'pid':child.pid,'actualExit':True,'exitCode':child.returncode,'stdout':out.decode(errors='replace'),'stderr':err.decode(errors='replace')});need(child.returncode==0,'GIT_CLEANUP_COMMAND_FAILED')
        processes_clear();mutate(['git','-C',str(MAIN),'worktree','remove','--force',str(W)])
        need(not W.exists(),'WORKTREE_REMAINS');need(git('rev-parse','refs/heads/'+BRANCH).decode().strip()==BASE,'BRANCH_MOVED_RETAIN_ROOT')
        mutate(['git','-C',str(MAIN),'branch','-D',BRANCH]);processes_clear()
        for owned in (ROOT,OUTER):
            shutil.rmtree(owned);need(not owned.exists(),'OWNED_ROOT_REMAINS');result['rootsRemoved'].append(str(owned))
        result.update(status='PASS_EXACT_R75_ROOTS_REMOVED',actualRemoved=result['plannedRemoval'],r76Untouched=True,sshAuthorizationUntouched=True)
    except Exception as error:result.update(status='FAIL_PARTIAL_CLEANUP' if result['rootsRemoved'] or result['commands'] else 'NOT_RUN_BLOCKED',errorType=type(error).__name__,errorCode=str(error) if isinstance(error,AssertionError) else 'LOCAL_VERIFICATION_OR_REMOVAL_ERROR')
    with report.open('x') as stream:json.dump(result,stream,indent=2);stream.write('\n')
    print(json.dumps({'status':result['status'],'report':str(report),'rootsRemoved':result['rootsRemoved']}));return 0 if result['status']=='PASS_EXACT_R75_ROOTS_REMOVED' else 1
if __name__=='__main__':raise SystemExit(main())
