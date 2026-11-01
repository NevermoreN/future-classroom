#!/usr/bin/env python3
"""R79 committed local cleanup. Default inert; no Windows or private-key read.

Run only the committed copy in DEST. Explicit final SHA arguments are required.
Reads the intended repository's remote refs; never reads R80 working sources.
"""
import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import tarfile
import time

E=Path('/tmp/fc-round79-msi-maintenance')
MAIN=Path('/workspace/共享/Eliora/未来教室计划')
DEST=MAIN/'开发计划/验证记录/2026-11-02-round79-msi-maintenance'
RESULT=DEST/'local-cleanup-first.json'
SOURCE='c44906517fbc62233dca755d7225cea375264cbc'
CANDIDATE='52d6392552679e7cedb564cd52db08e6e2ebc04b'
WTS=((E/'package-preparation-first/checkout-261c58a-first','261c58aca8b0f63ff2cfb4dbd5e85c0d17770005'),(E/'repair-implementation-first/checkout-c449-first',CANDIDATE))
KEY=Path('/config/.local/share/future-classroom-test-access/win11-192.168.10.212')
ZIP_PATHS=[Path('/mnt/data')/n for n in ('FutureClassroom-0.1.0-public-desktop-dev.zip','FutureClassroom-0.1.0-continuous-accept-dev.zip','FutureClassroom-0.1.0-voice-capability-dev.zip')]
OPS=[]

def need(v,s):
 if not v:raise AssertionError(s)
def sha(b):return hashlib.sha256(b).hexdigest()
def canonical(n):
 need(isinstance(n,str) and n and '\\' not in n and '\0' not in n,'SAFE_RELATIVE_STRING')
 p=Path(n);need(not p.is_absolute() and p.as_posix()==n and all(x not in ('','.','..') for x in p.parts),'CANONICAL_RELATIVE_PATH');return p
def owned(n):
 p=E/canonical(n);need(not any(q.is_symlink() for q in (p,*p.parents) if q==E or E in q.parents),'NO_ALIAS_FOR_PROOF_FILE');return p
def fact(p):
 s=p.lstat();need(stat.S_ISREG(s.st_mode) and not p.is_symlink(),'REGULAR_PROOF_FILE');h=hashlib.sha256()
 with p.open('rb') as f:
  while True:
   b=f.read(1024*1024)
   if not b:break
   h.update(b)
 return dict(bytes=s.st_size,sha256=h.hexdigest())
def metadata(p):
 s=p.lstat();return dict(path=str(p),mode=oct(stat.S_IMODE(s.st_mode)),device=s.st_dev,inode=s.st_ino,bytes=s.st_size,mtimeNs=s.st_mtime_ns,ctimeNs=s.st_ctime_ns,uid=s.st_uid,gid=s.st_gid,regular=stat.S_ISREG(s.st_mode),directory=stat.S_ISDIR(s.st_mode),symlink=stat.S_ISLNK(s.st_mode))
def protected_metadata():
 k=metadata(KEY);need(k['regular'] and not k['symlink'] and k['mode']=='0o600','PERSISTENT_KEY_ONLY_0600_METADATA')
 # No key contents, hashes or command-line substitutions are ever requested.
 return dict(key=k,protected=[metadata(p) for p in ZIP_PATHS]+[metadata(MAIN/'target')])
def git(*tail,root=MAIN,seconds=120,allow=(0,),record_stdout=False):
 cmd=['git','-C',str(root),'-c','core.autocrlf=true',*tail]
 env=os.environ.copy();env.update(GIT_NO_LAZY_FETCH='1',GIT_TERMINAL_PROMPT='0');began=time.monotonic();p=subprocess.Popen(cmd,stdout=subprocess.PIPE,stderr=subprocess.PIPE,env=env)
 try:out,err=p.communicate(timeout=seconds)
 except subprocess.TimeoutExpired:
  OPS.append(dict(argv=cmd,pid=p.pid,status='UNKNOWN_ORIGINAL_GIT_CHILD_RETAIN_NO_TERMINATION',originalSeconds=seconds));raise AssertionError('GIT_CHILD_TIMEOUT_NO_FORCE_NO_KILL') from None
 row=dict(argv=cmd,actualExit=p.returncode,parentReaped=True,seconds=time.monotonic()-began,stdoutBytes=len(out),stdoutSha256=sha(out),stderrBytes=len(err),stderrSha256=sha(err))
 if record_stdout:row['stdout']=out.decode('utf-8','strict')
 OPS.append(row);need(p.returncode in allow,'ACTUAL_GIT_OPERATION_NOT_EXPECTED_RETURN');return out,p.returncode
def git0(*tail,**kwargs):return git(*tail,**kwargs)[0]
def committed_file(commit,rel):
 canonical(rel);p=MAIN/rel
 need(not any(q.is_symlink() for q in (p,*p.parents) if q==MAIN or MAIN in q.parents),'COMMITTED_CURRENT_FILE_NONALIASED')
 a=fact(p);blob=git0('show',commit+':'+rel,seconds=180);need(len(blob)==a['bytes'] and sha(blob)==a['sha256'],'CURRENT_FILE_EQUALS_ACTUAL_COMMITTED_BLOB_'+rel)
 return dict(path=rel,**a,blob=git0('rev-parse',commit+':'+rel).decode().strip())
def process_gate():
 ignored={os.getpid()};pid=os.getpid()
 while pid>1:
  try:pid=int(Path('/proc',str(pid),'stat').read_text().rsplit(')',1)[1].split()[1])
  except (OSError,ValueError,IndexError):break
  ignored.add(pid)
 found=[]
 for p in Path('/proc').iterdir():
  if not p.name.isdigit() or int(p.name) in ignored:continue
  try:
   args=(p/'cmdline').read_bytes().split(b'\0')
   cwd=os.readlink(p/'cwd')
  except OSError:continue
  if any(str(E).encode() in x for x in args) or cwd==str(E) or cwd.startswith(str(E)+'/'):found.append(int(p.name))
 need(not found,'OTHER_PROCESS_REFERENCES_OWNED_E_'+str(found));return dict(status='PASS_NO_OTHER_LIVE_E_PROCESS_REFERENCE',count=0,signalsSent=0)
def mount_gate():
 points=[]
 for line in Path('/proc/self/mountinfo').read_text().splitlines():
  fields=line.split();need(len(fields)>5,'MOUNTINFO_COMPLETE');s=re.sub(r'\\([0-7]{3})',lambda m:chr(int(m.group(1),8)),fields[4]);p=Path(s)
  if p==E or E in p.parents:points.append(s)
 need(not points,'MOUNT_OR_BIND_MOUNT_INSIDE_E_REFUSED');return dict(status='PASS_NO_MOUNT_UNDER_E',count=0)
def repo_and_remote(args):
 need(git0('rev-parse','HEAD').decode().strip()==args.source_commit,'CURRENT_HEAD_EQUALS_EXPLICIT_FINAL_COMMIT')
 need(git0('status','--porcelain','-z','--untracked-files=all')==b'','MAIN_CLEAN_BEFORE_RECEIPT')
 origin=git0('config','--get','remote.origin.url').decode().strip()
 need(re.fullmatch(r'(?:https://github\.com/|git@github\.com:)NevermoreN/future-classroom(?:\.git)?',origin),'EXACT_OWN_REPOSITORY_REMOTE_NO_EMBEDDED_CREDENTIALS')
 raw=git0('ls-remote','--heads','origin','refs/heads/main','refs/heads/candidate/r79-damaged-install-preimage',seconds=60)
 rows={};
 for line in raw.decode('ascii').splitlines():
  oid,ref=line.split('\t');need(re.fullmatch('[0-9a-f]{40}',oid) and ref not in rows,'EXACT_REMOTE_REF_FACTS');rows[ref]=oid
 need(rows=={'refs/heads/main':args.expected_main_remote_sha,'refs/heads/candidate/r79-damaged-install-preimage':args.expected_candidate_remote_sha},'ACTUAL_REMOTE_MAIN_AND_ISOLATED_CANDIDATE_EXACT_EXPECTED_REFS')
 need(args.expected_main_remote_sha==args.source_commit and args.expected_candidate_remote_sha==CANDIDATE,'EXPLICIT_FINAL_COMMIT_AND_REVIEWED_CANDIDATE_SYNCHRONIZED')
 return dict(status='PASS_ACTUAL_READONLY_LS_REMOTE',refs=rows)
def all_dest_committed(commit):
 rel=DEST.relative_to(MAIN).as_posix();names=[x.decode() for x in git0('ls-tree','-r','--name-only','-z',commit,'--',rel).split(b'\0') if x]
 need(3<=len(names)<=1024,'FINITE_ARCHIVE_DEST_COMMITTED_FILE_COUNT')
 disk=[]
 for p in DEST.rglob('*'):
  need(not p.is_symlink(),'NO_DEST_SYMLINK');
  if p.is_file():disk.append(p.relative_to(MAIN).as_posix())
 need(set(names)==set(disk),'ALL_DEST_FILES_COMMITTED_NO_LATE_UNTRACKED_OMISSION')
 return [committed_file(commit,n) for n in names]
def archive_proof(args):
 index_path=DEST/args.index_name;archive=DEST/args.archive_name;verification_path=DEST/args.verification_name
 for p,pin in [(index_path,args.index_sha256),(archive,args.archive_sha256),(verification_path,args.verification_sha256)]:need(fact(p)['sha256']==pin,'EXPLICIT_FINAL_ARCHIVE_INDEX_VERIFY_SHA')
 index=json.loads(index_path.read_text());verify=json.loads(verification_path.read_text())
 need(index['status'].startswith('PASS_') and index['sourceRoot']==str(E) and index['sourceSha']==SOURCE and index['sourceDeletion'] is False,'R79_EXACT_INDEX_SOURCE_SCOPE')
 need(verify['status'].startswith('PASS_') and verify['sourceSha']==SOURCE and verify['indexSha256']==args.index_sha256 and verify['archiveSha256']==args.archive_sha256 and verify['sourceDeletion'] is False and verify['windowsContacted'] is False,'EXACT_VERIFICATION_BOUND_TO_FINAL_INDEX_ARCHIVE')
 scan=verify['credentialScan'];need(scan['status'].startswith('PASS_') and scan['findings']==[],'FINAL_DECODED_CREDENTIAL_SCAN_NO_FINDINGS')
 originals={r['path']:r for r in index['originals']};excluded={r['path']:r for r in index['excludedBinaries']}
 need(len(originals)==len(index['originals']) and len(excluded)==len(index['excludedBinaries'])==args.expected_excluded_count and not originals.keys()&excluded.keys(),'EXACT_UNIQUE_FINAL_ORIGINALS_AND_EXCLUDED_BINARY_COUNT')
 cf=index['archiveContainer'];need(fact(archive)=={k:cf[k] for k in ('bytes','sha256')},'ACTUAL_TAR_CONTAINER_IDENTITY')
 need(len(originals)==index['originalCount']==cf['memberCount']==verify['members'] and sum(r['bytes'] for r in originals.values())==index['originalBytes']==verify['originalBytes'],'FINAL_INDEX_TOTAL_CLOSURE')
 with tarfile.open(archive,'r:gz') as tar:
  members=tar.getmembers();need(len(members)==len(originals) and len({m.name for m in members})==len(members) and {m.name for m in members}==set(originals),'ALL_TAR_MEMBERS_EXACT_INDEX_SET')
  for m in members:
   canonical(m.name);need(m.isfile() and m.uid==m.gid==m.mtime==0 and m.mode==0o600 and m.uname==m.gname=='','DETERMINISTIC_REGULAR_TAR_ONLY')
   row=originals[m.name];p=owned(m.name);a=fact(p);stream=tar.extractfile(m);need(stream is not None,'TAR_MEMBER_STREAM');h=hashlib.sha256();total=0
   with p.open('rb') as current:
    while True:
     part=stream.read(1024*1024)
     if not part:break
     need(current.read(len(part))==part,'EVERY_ARCHIVED_ORIGINAL_CURRENT_BYTE_EXACT_'+m.name);h.update(part);total+=len(part)
    need(current.read(1)==b'','CURRENT_ORIGINAL_NO_EXTRA_TAIL')
   need(total==row['bytes']==a['bytes'] and h.hexdigest()==row['sha256']==a['sha256'],'EVERY_ARCHIVED_ORIGINAL_SHA_EXACT_'+m.name)
 for n,row in excluded.items():
  canonical(n);a=fact(owned(n));need(a=={k:row[k] for k in ('bytes','sha256')},'ALL_FINAL_EXCLUDED_BINARY_ACTUAL_SHA_'+n)
 for row in index.get('promoted',[]):
  n=row['path'];need(n in originals and fact(DEST/n)=={k:row[k] for k in ('bytes','sha256')} and (DEST/n).read_bytes()==owned(n).read_bytes(),'ALL_PROMOTED_BYTES_MATCH_ORIGINAL')
 acceptance='independent-finite-acceptance-first/acceptance-first.json';need(acceptance in originals,'FINITE_ACCEPTANCE_IS_ARCHIVED_ORIGINAL');a=json.loads(owned(acceptance).read_text())
 need(a['status']=='PASS_R79_FINITE_LOCAL_EVIDENCE_ACCEPTANCE_WIN11_ONLY_NOT_PRODUCTION_INTEGRATION' and a['sourceSha']==SOURCE and a['candidateSha']==CANDIDATE and a['archiveAdmissionEligible'] and not a['mayClaimProductionRepairReady'],'EXACT_R79_FINITE_ACCEPTANCE_NO_PRODUCTION_AUTHORITY')
 need('windows-readonly-final-second.json' in originals and originals['windows-readonly-final-second.json']['sha256']=='66bcac658e0a144af244a613702c8d8f37c6aab407fc39e3d19f4b71a85be4ad','ACTUAL_FINAL_WINDOWS_AUDIT_ARCHIVED')
 pres_name='repair-candidate-source-preservation-first/preservation-first.json';need(pres_name in originals,'CANDIDATE_PRESERVATION_ARCHIVED');pres=json.loads(owned(pres_name).read_text())
 need(pres['commit']==CANDIDATE and pres['parent']==SOURCE,'EXACT_REVIEWED_CANDIDATE_PARENT')
 for row in pres['files']:
  n='repair-candidate-source-preservation-first/source/'+row['path'];need(n in originals,'ALL_THREE_FULL_CANDIDATE_SOURCES_ARCHIVED');need(fact(owned(n))=={k:row[k] for k in ('bytes','sha256')},'PRESERVED_CANDIDATE_SOURCE_BYTES')
  blob=git0('show',CANDIDATE+':'+row['path']);need(blob==owned(n).read_bytes(),'EXACT_ACTUAL_CANDIDATE_COMMITTED_BLOB')
 patch='repair-candidate-source-preservation-first/candidate-second-complete.patch';need(patch in originals and fact(owned(patch))==pres['completePatch'],'COMPLETE_CANDIDATE_PATCH_ARCHIVED_EXACT')
 return index,originals,excluded,dict(status='PASS_FULL_COMMITTED_TAR_CURRENT_ORIGINAL_AND_BINARY_REPLAY',originals=len(originals),excludedBinaries=len(excluded),logicalOriginalBytes=index['originalBytes'],finiteAcceptance=a['status'])
def worktree_proof():
 registered=git0('worktree','list','--porcelain','-z');rows=[]
 for wt,commit in WTS:
  need(wt.is_dir() and not wt.is_symlink() and git0('rev-parse','HEAD',root=wt).decode().strip()==commit,'EXACT_OWNED_WORKTREE_HEAD')
  need(git0('status','--porcelain','-z','--untracked-files=all',root=wt)==b'','OWNED_WORKTREE_CLEAN_NORMAL_REMOVE_ONLY')
  need(registered.count(('worktree '+str(wt)).encode()+b'\0')==1,'EXACT_ONE_REGISTERED_WORKTREE')
  rows.append(dict(path=str(wt),head=commit,clean=True,forceAllowed=False))
 return rows
def worktree_paths(raw):
 return {field[len(b'worktree '):].decode('utf-8','strict') for field in raw.split(b'\0') if field.startswith(b'worktree ')}
def tree_plan(allowed,pruned,removed=()):
 device=E.lstat().st_dev;rows=[];found=set();count=0
 def in_prune(n):return any(n==p or n.startswith(p+'/') for p in pruned)
 def walk(d):
  nonlocal count
  for p in sorted(d.iterdir()):
   count+=1;need(count<=250000,'FINITE_FULL_OWNED_TREE_BOUND');n=p.relative_to(E).as_posix();s=p.lstat();need(s.st_dev==device and not os.path.ismount(p),'NO_MOUNT_CROSS_DEVICE_DELETE')
   row=dict(path=n,device=s.st_dev,inode=s.st_ino,size=s.st_size,mtimeNs=s.st_mtime_ns,ctimeNs=s.st_ctime_ns,nlink=s.st_nlink,mode=s.st_mode,uid=s.st_uid,gid=s.st_gid)
   if stat.S_ISLNK(s.st_mode):need(in_prune(n),'UNARCHIVED_UNPRUNED_SYMLINK');row['kind']='symlink';rows.append(row)
   elif stat.S_ISDIR(s.st_mode):walk(p);row['kind']='directory';rows.append(row)
   else:
    need(stat.S_ISREG(s.st_mode),'SPECIAL_FILE_REFUSED');need(n in allowed or in_prune(n),'UNKNOWN_UNARCHIVED_UNPRUNED_FILE_'+n);found.add(n);row['kind']='file';rows.append(row)
 walk(E)
 remaining={n for n in allowed if not any(n==p or n.startswith(p+'/') for p in removed)}
 need(remaining<=found,'ALL_EXPECTED_REMAINING_ORIGINALS_AND_BINARIES_PRESENT');return rows
def supplement_proof(args):
 need(len(args.supplement)>=2,'EXPLICIT_LATE_OPERATION_AND_CONTROLLER_SUPPLEMENTS_REQUIRED')
 rows=[];late_sources={};dest_rel=DEST.relative_to(MAIN)
 for item in args.supplement:
  rel,pin=item.rsplit('=',1);canonical(rel);need(re.fullmatch('[0-9a-f]{64}',pin),'EXPLICIT_SUPPLEMENT_SHA256')
  need(rel.startswith(dest_rel.as_posix()+'/'),'SUPPLEMENT_MUST_BE_COMMITTED_ARCHIVE_DEST_FILE');need(fact(MAIN/rel)['sha256']==pin,'EXACT_EXPLICIT_LATE_SUPPLEMENT_SHA');proof=committed_file(args.source_commit,rel)
  # A late operation can be committed alongside the tar after the tar's source
  # inventory froze. Admit only the exact same relative E object, if it exists,
  # after matching its current bytes to this actual committed supplement blob.
  n=Path(rel).relative_to(dest_rel).as_posix();source=owned(n)
  try:source.lstat()
  except FileNotFoundError:proof['lateSource']='KNOWN_ABSENT_NO_EXTRA_E_PATH_ADMITTED'
  else:
   a=fact(source);need(a=={k:proof[k] for k in ('bytes','sha256')} and source.read_bytes()==(MAIN/rel).read_bytes(),'EXACT_LATE_E_SOURCE_EQUALS_COMMITTED_SUPPLEMENT')
   need(n not in late_sources,'UNIQUE_LATE_SOURCE_SUPPLEMENT');late_sources[n]=a;proof['lateSource']=dict(path=n,**a,authority='EXACT_COMMITTED_SUPPLEMENT_NOT_TAR_MEMBER')
  rows.append(proof)
 return rows,late_sources
def hardlink_groups(rows,pruned):
 groups={}
 for row in rows:
  if row['kind']=='file':groups.setdefault((row['device'],row['inode']),[]).append(row)
 result={}
 for key,members in groups.items():
  first=members[0];need(all(all(r[k]==first[k] for k in ('size','mtimeNs','ctimeNs','nlink','mode','uid','gid')) for r in members),'PLANNED_HARDLINK_GROUP_METADATA_IDENTICAL')
  need(first['nlink']>=len(members),'PLANNED_OWNED_LINK_COUNT_CANNOT_EXCEED_NATIVE_LINK_COUNT')
  # Only this round's known Cargo target, already pruned by the final archive,
  # may account for ctime changes caused by our own same-inode unlinks. This
  # never relaxes a source path, main target, global cache or another prune.
  cache_only=all(r['path'].startswith('repair-candidate-target-first/') and any(r['path']==p or r['path'].startswith(p+'/') for p in pruned) for r in members)
  result[key]=dict(unlinked=0,initialNlink=first['nlink'],expectedCtimeNs=first['ctimeNs'],allowOwnedCtime=cache_only and len(members)>1,paths=[r['path'] for r in members],ctimeAdjustments=0,observations=[])
 return result
def regular_guard(row,current,group):
 need(all(current[k]==row[k] for k in ('device','inode','size','mtimeNs','mode','uid','gid')),'REGULAR_OBJECT_CONTENT_OR_OTHER_METADATA_CHANGED')
 count=group['unlinked'];need(current['nlink']==row['nlink']-count,'NATIVE_NLINK_EQUALS_EXACT_OWNED_UNLINK_COUNT')
 if count==0:need(current['ctimeNs']==row['ctimeNs'],'FIRST_OR_SINGLE_LINK_ORIGINAL_CTIME_UNCHANGED')
 else:
  need(group['allowOwnedCtime'],'OWNED_CTIME_ADJUSTMENT_ONLY_EXACT_PRUNED_CARGO_CACHE')
  need(current['ctimeNs']==group['expectedCtimeNs'],'NEXT_LINK_MATCHES_ACTUAL_POST_OWN_UNLINK_CTIME')
def native_regular(p):
 s=p.lstat();need(stat.S_ISREG(s.st_mode),'NATIVE_REMAINING_HARDLINK_IS_REGULAR')
 return dict(device=s.st_dev,inode=s.st_ino,size=s.st_size,mtimeNs=s.st_mtime_ns,ctimeNs=s.st_ctime_ns,nlink=s.st_nlink,mode=s.st_mode,uid=s.st_uid,gid=s.st_gid)
def account_owned_unlink(row,group,remaining_rows):
 group['unlinked']+=1
 future=[r for r in remaining_rows if r['kind']=='file' and (r['device'],r['inode'])==(row['device'],row['inode'])]
 if future and group['allowOwnedCtime']:
  observed=native_regular(E/future[0]['path'])
  need(all(observed[k]==row[k] for k in ('device','inode','size','mtimeNs','mode','uid','gid')) and observed['nlink']==group['initialNlink']-group['unlinked'],'POST_OWN_UNLINK_SAME_CACHE_OBJECT_EXACT_NLINK_AND_METADATA')
  if observed['ctimeNs']!=group['expectedCtimeNs']:group['ctimeAdjustments']+=1
  group['expectedCtimeNs']=observed['ctimeNs'];group['observations'].append(dict(afterOwnedUnlinkCount=group['unlinked'],actualRemainingNlink=observed['nlink'],actualRemainingCtimeNs=observed['ctimeNs']))

def main():
 p=argparse.ArgumentParser(description=__doc__);p.add_argument('--execute',action='store_true');p.add_argument('--source-commit');p.add_argument('--index-name',default='archive-index-first.json');p.add_argument('--archive-name',default='evidence-originals-first.tar.gz');p.add_argument('--verification-name',default='archive-verification-first.json');p.add_argument('--index-sha256');p.add_argument('--archive-sha256');p.add_argument('--verification-sha256');p.add_argument('--expected-excluded-count',type=int);p.add_argument('--expected-main-remote-sha');p.add_argument('--expected-candidate-remote-sha');p.add_argument('--supplement',action='append',default=[]);a=p.parse_args()
 if not a.execute:
  print(json.dumps(dict(status='NOT_RUN_PREPARATION_ONLY',sourceRoot=str(E),destination=str(DEST),receipt=str(RESULT),sourceDeletion=False,Windows='NOT_CONTACTED',R80Sources='NOT_READ',remote='NOT_QUERIED')));return 0
 need(not RESULT.exists(),'FRESH_RESULT_ONLY')
 value=dict(status='RUNNING_LOCAL_R79_CLEANUP',sourceCommit=a.source_commit,sourceSha=SOURCE,candidateSha=CANDIDATE,sourceDeletionStarted=False,removedFiles=0,removedSymlinks=0,removedDirectories=0,logicalRegularBytesRemovedDirectly=0,logicalRegularBytesRemovedByGit=0,Windows='NOT_CONTACTED',R80SourceContents='NOT_READ_OR_HASHED',privateKeyContentsRead=False,physicalReclaimedBytes='NOT_MEASURED_LOGICAL_COUNTS_ONLY',worktreeRemovals=[])
 try:
  need(DEST.is_dir() and not DEST.is_symlink() and Path(__file__).resolve()==DEST/'local-cleanup-first.py','RUN_ONLY_ACTUAL_COMMITTED_DEST_CONTROLLER_COPY')
  for n in (a.index_name,a.archive_name,a.verification_name):need(len(canonical(n).parts)==1,'FLAT_FINAL_ARCHIVE_NAME')
  need(re.fullmatch('[0-9a-f]{40}',a.source_commit or '') and re.fullmatch('[0-9a-f]{40}',a.expected_main_remote_sha or '') and re.fullmatch('[0-9a-f]{40}',a.expected_candidate_remote_sha or ''),'EXPLICIT_FINAL_COMMIT_AND_REMOTE_REFS_REQUIRED')
  for pin in (a.index_sha256,a.archive_sha256,a.verification_sha256):need(re.fullmatch('[0-9a-f]{64}',pin or ''),'EXPLICIT_FINAL_CONTENT_SHA256_REQUIRED')
  need(a.expected_excluded_count is not None and a.expected_excluded_count>0,'EXPLICIT_FINAL_BINARY_COUNT_FROM_INDEX')
  need(E.is_dir() and not E.is_symlink() and (E/'.owner').read_bytes()==b'future-classroom-owned-r79-msi-maintenance/v1\n','EXACT_OWNED_R79_ROOT_MARKER')
  value['mountBefore']=mount_gate();value['processBefore']=process_gate();value['protectedBefore']=protected_metadata();value['remoteProof']=repo_and_remote(a)
  value['committedDestinationProof']=all_dest_committed(a.source_commit);value['lateSupplementProof'],late_sources=supplement_proof(a);value['lateSourceAllowed']=late_sources
  value['registeredWorktreePathsBefore']=sorted(worktree_paths(git0('worktree','list','--porcelain','-z')))
  index,originals,excluded,proof=archive_proof(a);value['archiveProof']=proof;value['worktreeProof']=worktree_proof()
  pruned=[r['path'] for r in index['prunedGeneratedTrees']];need(len(set(pruned))==len(pruned),'UNIQUE_FINAL_GENERATED_PRUNE_ROOTS')
  for n in pruned:canonical(n)
  allowed=set(originals)|set(excluded)|set(late_sources);tree=tree_plan(allowed,pruned);value['plannedEntries']=len(tree);value['plannedLogicalRegularBytes']=sum(r['size'] for r in tree if r['kind']=='file');value['processImmediatelyBeforeMutation']=process_gate();mount_gate()
  # Original processes must be terminal. No forced worktree removal, no signal.
  removed=[];value['sourceDeletionStarted']=True
  for wt,commit in WTS:
   need(git0('rev-parse','HEAD',root=wt).decode().strip()==commit and git0('status','--porcelain','-z','--untracked-files=all',root=wt)==b'','RECHECK_OWNED_WT_CLEAN_BEFORE_NORMAL_REMOVE')
   git0('worktree','remove',str(wt),seconds=180,record_stdout=True)
   need(not wt.exists() and not wt.is_symlink(),'NORMAL_WORKTREE_REMOVE_ACTUAL_ABSENCE');rel=wt.relative_to(E).as_posix();removed.append(rel);value['worktreeRemovals'].append(dict(path=str(wt),head=commit,status='ACTUAL_NORMAL_GIT_REMOVE_EXIT0',forceUsed=False));value['logicalRegularBytesRemovedByGit']+=sum(r['size'] for r in tree if r['kind']=='file' and (r['path']==rel or r['path'].startswith(rel+'/')))
  remaining=tree_plan(allowed,pruned,removed);groups=hardlink_groups(remaining,pruned);rows_by_path={r['path']:r for r in remaining if r['kind']=='file'}
  value['hardlinkAccounting']=[dict(device=k[0],inode=k[1],progress=g) for k,g in groups.items() if g['initialNlink']>1];value['ownedHardlinkUnlinkCount']=0
  process_gate();mount_gate()
  for index,row in enumerate(remaining):
   n=row['path'];q=E/n
   if n=='.owner':continue
   s=q.lstat();need(s.st_dev==row['device'] and s.st_ino==row['inode'],'SAME_PLANNED_OWNED_OBJECT_BEFORE_UNLINK')
   if row['kind']=='directory':need(stat.S_ISDIR(s.st_mode) and not q.is_symlink(),'REAL_EMPTY_DIRECTORY_ONLY');q.rmdir();value['removedDirectories']+=1
   elif row['kind']=='symlink':need(stat.S_ISLNK(s.st_mode),'UNLINK_OWNED_SYMLINK_ITSELF_ONLY');q.unlink();value['removedSymlinks']+=1
   else:
    group=groups[(row['device'],row['inode'])];regular_guard(row,native_regular(q),group);q.unlink();value['removedFiles']+=1;value['logicalRegularBytesRemovedDirectly']+=row['size']
    if group['initialNlink']>1:value['ownedHardlinkUnlinkCount']+=1
    future_same_inode=[rows_by_path[n] for n in group['paths'][group['unlinked']+1:]]
    account_owned_unlink(row,group,future_same_inode)
  value['expectedOwnedHardlinkCtimeAdjustments']=sum(g['ctimeAdjustments'] for g in groups.values())
  need({q.name for q in E.iterdir()}=={'.owner'},'ONLY_OWN_MARKER_REMAINS');owner=E/'.owner';size=owner.lstat().st_size;need(owner.read_bytes()==b'future-classroom-owned-r79-msi-maintenance/v1\n','ORIGINAL_OWNER_MARKER_LAST');owner.unlink();value['removedFiles']+=1;value['logicalRegularBytesRemovedDirectly']+=size;E.rmdir();value['removedDirectories']+=1
  need(not E.exists() and not E.is_symlink(),'EXACT_OWNED_ROOT_ABSENT')
  registered=git0('worktree','list','--porcelain','-z');need(all(('worktree '+str(wt)).encode()+b'\0' not in registered for wt,_ in WTS),'ONLY_TWO_OWNED_REGISTRATIONS_REMOVED')
  value['registeredWorktreePathsAfter']=sorted(worktree_paths(registered));need(set(value['registeredWorktreePathsAfter'])==set(value['registeredWorktreePathsBefore'])-{str(wt) for wt,_ in WTS},'EVERY_OTHER_WORKTREE_REGISTRATION_PRESERVED_WITHOUT_SOURCE_READ')
  need(git0('rev-parse','HEAD').decode().strip()==a.source_commit and git0('status','--porcelain','-z','--untracked-files=all')==b'','MAIN_REMAINS_COMMITTED_CLEAN_BEFORE_NEW_RECEIPT')
  value['protectedAfter']=protected_metadata();need(value['protectedAfter']==value['protectedBefore'],'PERSISTENT_KEY_METADATA_AND_THREE_INTENDED_ZIPS_MAIN_TARGET_UNCHANGED')
  value.update(status='PASS_EXACT_COMMITTED_R79_EVIDENCE_AND_OWNED_LOCAL_CLEANUP',ownedRootKnownAbsent=True,processesTerminated=0,globalCachesTouched=False,R80WorktreeTouched=False)
 except Exception as error:value.update(status='FAIL_RETAIN_REMAINING_ORIGINALS_NO_FORCE',errorType=type(error).__name__,reason=str(error) if isinstance(error,AssertionError) else 'PROOF_OR_OWNED_CLEANUP_OPERATION_FAILED',ownedRootKnownAbsent=not E.exists())
 value['gitOperations']=OPS;value['completedUtc']=datetime.datetime.now(datetime.timezone.utc).isoformat()
 with RESULT.open('x') as f:json.dump(value,f,ensure_ascii=False,indent=2);f.write('\n')
 print(json.dumps(dict(status=value['status'],receipt=str(RESULT),ownedRootKnownAbsent=value.get('ownedRootKnownAbsent'),logicalRegularBytesRemovedDirectly=value['logicalRegularBytesRemovedDirectly'],logicalRegularBytesRemovedByGit=value['logicalRegularBytesRemovedByGit'])));return 0 if value['status'].startswith('PASS_') else 1

if __name__=='__main__':raise SystemExit(main())
