#!/usr/bin/env python3
"""R78 second committed cleanup: correct three protected ZIP names; default NOT_RUN.
No Windows/network call, process termination, sudo, global Git setting or key read.
"""
from pathlib import Path
import argparse,datetime,hashlib,json,os,re,stat,subprocess,tarfile,time
E=Path('/tmp/fc-round78-ci-evidence');D=E/'local-cleanup-preparation-first'
MAIN=Path('/workspace/共享/Eliora/未来教室计划');DEST=MAIN/'开发计划/验证记录/2026-11-01-round78-ci'
WT=E/'package-source-preparation-first/checkout-261c58a-first'
SOURCE='261c58aca8b0f63ff2cfb4dbd5e85c0d17770005'
KEY=Path('/config/.local/share/future-classroom-test-access/win11-192.168.10.212')
ZIP_PATHS=[Path('/mnt/data')/n for n in ('FutureClassroom-0.1.0-public-desktop-dev.zip','FutureClassroom-0.1.0-continuous-accept-dev.zip','FutureClassroom-0.1.0-voice-capability-dev.zip')]
RESULT=DEST/'local-cleanup-second.json'
SUCCESS='product-msi-runtime-second/report.json';FIRST='product-msi-runtime-first/report.json';FIRST_CLEANUP='root-first-product-scratch-cleanup-first.json'
SUCCESS_SHA='a636d1cc6a3f3f91a8c21d0aaf0dac43cbe93e856e9a66e5926af1ee3437d1f7'
FIRST_SHA='5126fb9b3c20f861ce7b3cf1f5ba7024508dfb17d827430e5da3cc33afd9545e'
def need(v,c):
 if not v:raise AssertionError(c)
def sha(b):return hashlib.sha256(b).hexdigest()
def canonical(n):
 p=Path(n);need(isinstance(n,str) and not p.is_absolute() and p.as_posix()==n and bool(p.parts) and all(s not in ('','.','..') for s in p.parts),'CANONICAL_RELATIVE_PATH');return p
def owned(p):
 p=Path(p);need(p.is_absolute() and p.resolve().is_relative_to(E) and not any(q.is_symlink() for q in (p,*p.parents) if q==E or E in q.parents),'OWNED_NONALIASED_INPUT');return p
def fact(p):
 need(stat.S_ISREG(p.lstat().st_mode) and not p.is_symlink(),'REGULAR_FILE');b=p.read_bytes();return dict(path=str(p),bytes=len(b),sha256=sha(b))
def metadata(p):
 s=p.lstat();return dict(path=str(p),mode=oct(stat.S_IMODE(s.st_mode)),regular=stat.S_ISREG(s.st_mode),directory=stat.S_ISDIR(s.st_mode),symlink=stat.S_ISLNK(s.st_mode),bytes=s.st_size,device=s.st_dev,inode=s.st_ino,mtimeNs=s.st_mtime_ns,uid=s.st_uid,gid=s.st_gid)
def key_metadata():
 m=metadata(KEY);need(m['regular'] and not m['symlink'] and m['mode']=='0o600','PERSISTENT_KEY_METADATA_ONLY_0600');return m
READ_OPERATIONS=[]
def git(*tail,root=MAIN,timeout=30):
 cmd=['git','-C',str(root),'-c','core.autocrlf=true',*tail];env=os.environ.copy();env.update(GIT_NO_LAZY_FETCH='1',GIT_TERMINAL_PROMPT='0')
 child=subprocess.Popen(cmd,stdout=subprocess.PIPE,stderr=subprocess.PIPE,env=env)
 try:out,err=child.communicate(timeout=timeout)
 except subprocess.TimeoutExpired:
  READ_OPERATIONS.append(dict(command=cmd,pid=child.pid,status='UNKNOWN_ORIGINAL_CHILD_DEADLINE_EXPIRED_NOT_TERMINATED',originalBudgetSeconds=timeout,actualExit=False))
  raise AssertionError('ORIGINAL_READONLY_GIT_CHILD_TIMEOUT_RETAIN_NO_FORCE') from None
 need(child.returncode==0,'READONLY_GIT_EXIT0_REQUIRED');return out
def source_name(path):return owned(Path(path) if Path(path).is_absolute() else E/canonical(str(path))).relative_to(E).as_posix()
def json_bytes(b):return json.loads(b.decode('utf-8-sig'))
def selected(names):return [n for n in names if n and (n.startswith(('crates/','scripts/','web/','.cargo/')) or n in ('Cargo.toml','Cargo.lock'))]
def process_gate():
 exclude={os.getpid()};p=os.getpid()
 while p>1:
  try:p=int(Path('/proc',str(p),'stat').read_text().rsplit(')',1)[1].split()[1])
  except (OSError,ValueError,IndexError):break
  exclude.add(p)
 live=[]
 for p in Path('/proc').iterdir():
  if not p.name.isdigit() or int(p.name) in exclude:continue
  try:args=(p/'cmdline').read_bytes().split(b'\0')
  except OSError:continue
  if any(str(E).encode() in arg for arg in args):live.append(int(p.name))
 need(not live,'OTHER_LIVE_OWNED_ROOT_COMMANDS_'+str(live));return dict(status='PASS',otherOwnedRootProcessReferences=0,signalCalls=0)
def committed_file(commit,relative):
 canonical(relative);p=MAIN/relative;need(not any(q.is_symlink() for q in (p,*p.parents) if q==MAIN or MAIN in q.parents),'NONALIASED_MAIN_FILE')
 f=fact(p);blob=git('show',commit+':'+relative,timeout=120);need(p.read_bytes()==blob,'CURRENT_BYTES_EQUAL_C1_BLOB_'+relative);return dict(path=relative,bytes=f['bytes'],sha256=f['sha256'],gitBlobOidSha1=git('rev-parse',commit+':'+relative).decode().strip())
def changed_blob_proof(commit):
 fields=git('diff-tree','--no-commit-id','--name-status','--no-renames','-z','-r',commit).split(b'\0');fields=[f.decode() for f in fields if f];need(len(fields)%2==0,'EXACT_CHANGED_PATH_STREAM');rows=[]
 for i in range(0,len(fields),2):
  kind,n=fields[i:i+2];canonical(n)
  if kind=='D':need(not (MAIN/n).exists() and not (MAIN/n).is_symlink(),'COMMITTED_DELETION_ABSENT');rows.append(dict(path=n,status=kind))
  else:need(kind in ('A','M','T'),'REVIEW_UNEXPECTED_GIT_CHANGE_KIND');rows.append(dict(status=kind,**committed_file(commit,n)))
 need(rows,'C1_HAS_REVIEWABLE_CHANGED_PATHS');return rows

def archive_proof(commit):
 relbase=DEST.relative_to(MAIN).as_posix();bindings=[committed_file(commit,relbase+'/'+n) for n in ('evidence-originals-first.tar.gz','archive-index-first.json','archive-verification-first.json')]
 ib=(DEST/'archive-index-first.json').read_bytes();index=json_bytes(ib);verify=json_bytes((DEST/'archive-verification-first.json').read_bytes());archive=DEST/'evidence-originals-first.tar.gz'
 need(index['status']=='PASS_ARCHIVED_BYTES_NOT_NEW_RUNTIME' and index['sourceRoot']==str(E) and index['sourceSha']==SOURCE and index['sourceDeletion'] is False,'EXACT_R78_ARCHIVE_INDEX')
 need(verify['status']=='PASS_EXACT_ORIGINAL_TAR_REPLAY_AND_DECODED_CREDENTIAL_SCAN' and verify['sourceSha']==SOURCE and verify['indexSha256']==sha(ib) and verify['archiveSha256']==sha(archive.read_bytes()) and verify['sourceDeletion'] is False and verify['windowsContacted'] is False,'EXACT_COMMITTED_ARCHIVE_VERIFICATION')
 need(verify['credentialScan']['status']=='PASS_NO_MATCHES' and verify['credentialScan']['findings']==[],'ORIGINAL_DECODED_SCAN_NO_FINDINGS')
 cf=index['archiveContainer'];af=fact(archive);need(cf['bytes']==af['bytes'] and cf['sha256']==af['sha256']==verify['archiveSha256'],'ACTUAL_TAR_CONTAINER_IDENTITY')
 originals={r['path']:r for r in index['originals']};excluded={r['path']:r for r in index['excludedBinaries']};need(len(originals)==len(index['originals']) and len(excluded)==len(index['excludedBinaries']) and not set(originals)&set(excluded),'UNIQUE_ARCHIVED_AND_EXCLUDED_MEMBERS')
 need(len(originals)==index['originalCount']==verify['members']==cf['memberCount'] and sum(r['bytes'] for r in originals.values())==index['originalBytes']==verify['originalBytes'],'ARCHIVE_COUNT_AND_BYTES_CLOSURE')
 with tarfile.open(archive,'r:gz') as t:
  members=t.getmembers();need(len(members)==len(originals) and len({m.name for m in members})==len(members) and {m.name for m in members}==set(originals),'FULL_TAR_MEMBERSHIP')
  for m in members:
   canonical(m.name);need(m.isfile() and m.uid==m.gid==m.mtime==0 and m.mode==0o600 and m.uname==m.gname=='','REGULAR_DETERMINISTIC_TAR_MEMBERS');row=originals[m.name];p=owned(E/m.name);f=fact(p);b=t.extractfile(m).read();need(f['bytes']==row['bytes']==len(b) and f['sha256']==row['sha256']==sha(b) and p.read_bytes()==b,'EVERY_TEXT_ORIGINAL_EXACT_REPLAY_'+m.name)
 for n,row in excluded.items():
  canonical(n);f=fact(owned(E/n));need(f['bytes']==row['bytes'] and f['sha256']==row['sha256'],'EVERY_EXCLUDED_BINARY_IDENTITY_'+n)
 for row in index['promoted']:
  n=row['path'];need(n in originals,'PROMOTED_IS_ORIGINAL');b=(DEST/n).read_bytes();need(b==(E/n).read_bytes() and len(b)==row['bytes'] and sha(b)==row['sha256'],'EXACT_COMMITTED_PROMOTED_BYTES');committed_file(commit,relbase+'/'+n)
 ev=index['evidence'];need(len(ev['ciPEs'])==48 and len(ev['toolPackages'])==7,'ACTUAL_48_PE_7_DEB_IDENTITIES')
 for row in ev['ciPEs']+ev['toolPackages']:need(row=={k:excluded[row['path']][k] for k in ('path','bytes','sha256')},'EXCLUDED_ORIGINAL_CI_AND_PACKAGE_EXACT_IDENTITIES')
 def original_json(n):
  n=source_name(n);need(n in originals,'REPORT_IS_COMMITTED_TAR_ORIGINAL_'+n);return json_bytes((E/n).read_bytes())
 acceptance=original_json(ev['acceptance']['path']);need(acceptance['status']==ev['acceptanceStatus'] and acceptance['status'].startswith('PASS_R78_') and acceptance['sourceSha']==SOURCE,'FINAL_FINITE_ACTUAL_ACCEPTANCE')
 need(originals['windows-readonly-final-first.json']['sha256']=='c62f735c28fd60ab5aaea6053453ce41a32ba6545f6245bcb14ef840a1a7a23d','EXACT_ACTUAL_FINAL_READONLY_SHA')
 final=original_json(ev['readonlyAudit']['path']);need(final['status']=='PASS' and final.get('completedAtUtc') and final['scratchDirectories']==final['ownedProcesses']==final['ownedTasks']==[] and final['sshd']=='Running' and final['publicAuthorizationUnchanged'] is True and final['projectRunUnchanged'] is True and final['integrityStatus']=='PASS','FINAL_WINDOWS_AUDIT_CLEANUP_AND_AUTHORIZATION')
 for name,pin in [(SUCCESS,SUCCESS_SHA),(FIRST,FIRST_SHA)]:need(originals[name]['sha256']==pin,'ORIGINAL_MSI_REPORT_EXPLICIT_SHA')
 success=original_json(SUCCESS);first=original_json(FIRST);fc=original_json(FIRST_CLEANUP)
 need(success['status']=='PASS_PRODUCT_MSI_RUNTIME_CAPTURE_AND_EXACT_SCRATCH_CLEANUP_WIN11_ONLY' and success['cleanup']=='PASS_EXACT_DOWNLOADED_OWNED_SCRATCH_ONLY' and success['cleanupActual']['status']=='REMOVED_EXACT_PRODUCT_RUNTIME_ROOT' and success['cleanupActual']['knownAbsent'] is True and success['sourceUnchanged'] is True and success['controllerReturned'] is True and success['phaseParentExit']==0,'ACTUAL_SECOND_PRODUCT_MSI_AND_SCRATCH_CLEANUP')
 phases=success['phaseResult'];need(phases['status']=='PASS_THREE_PHASE_FOUR_ROLE_AUTOMATIC_IMAGE_CLEANUP_AND_ORIGINAL_GATE_WIN11_ONLY' and phases['cleanup']=='PASS_FORMAL_UNINSTALL_AND_ORIGINAL_GATE_RETIRED' and len(phases['phases'])==3 and [r['name'] for r in phases['phases']]==['fresh','reapply','uninstall'],'THREE_ACTUAL_PRODUCT_PHASES')
 for phase in phases['phases']:
  c=phase['client'];o=phase['ownerProof'];h=phase['helperImageCleanup'];need(c['actualExit'] is True and c['exitCode']==0 and c['timedOut'] is False and o['complete'] is True and o['knownControlsReturned'] is True and h['status']=='PASS_PRECISE_HELPER_IMAGE_AUTOMATIC_ABSENCE' and h['allCapturedImagesKnownAbsent'] is True and h['brokerImageKnownAbsent'] is True and h['mutations'] is False,'ACTUAL_PHASE_ORIGINAL_CONTROLS_AND_PRODUCT_IMAGE_CLEANUP')
 need(first['status']=='FAIL_PRODUCT_PHASE_OR_COVERAGE_RETAINED' and first['cleanup']=='RETAIN_LIVE_PENDING_UNKNOWN_INSTALLATION_OR_INCOMPLETE_PROOF','FIRST_FAIL_RETAINED_VERBATIM')
 need(fc['status']=='PASS' and fc['actualExit'] is True and fc['exitCode']==0 and fc['originalReportSha256']==FIRST_SHA and fc['originalReportStillExact'] is True and fc['actual']['status']=='PASS_DOWNLOADED_ENDED_NONCE_SCRATCH_ONLY_ORIGINAL_MSI_FAIL_RETAINED' and fc['actual']['knownRootAbsent'] is True and fc['actual']['ownedProcesses']==0 and fc['actual']['systemCacheOrRegistryMutation'] is False and fc['deleteReceipt']['knownAbsent'] is True,'FIRST_STRICT_SCRATCH_CLEANUP_SEPARATE_FROM_FAIL')
 system_name='root-second-msi-system-paths-first.json';need(originals[system_name]['sha256']=='fe383d0b8f0bb95de0e2f8309aa21eea0140def165c6bbb892c74ae096472772','EXACT_ACTUAL_SECOND_SYSTEM_PATHS_SHA');system=original_json(system_name)
 need(system['status']=='PASS' and system['mutations']=='NONE' and system['runtimeReportSha256']==SUCCESS_SHA and system['ssh']['actualExit'] is True and system['ssh']['exitCode']==0 and system['observed']['mutations']=='NONE' and len(system['observed']['facts'])==11 and all(row['status']=='KNOWN_ABSENT' and row['attributes']==4294967295 and row['win32Error']==2 for row in system['observed']['facts']),'ACTUAL_EXACT_SECOND_MSI_SYSTEM_PATH_ABSENCE')
 return index,originals,excluded,dict(secondPreciseSystemPaths='PASS_11_KNOWN_ABSENT_READONLY',committedArchiveBindings=bindings,originalCount=len(originals),excludedCount=len(excluded),fullTextReplay='PASS',allExcludedIdentities='PASS',acceptedPEs=48,exactToolPackages=7,finalAcceptance=acceptance['status'],finalReadonly='PASS',secondMsi=success['status'],firstMsiFailureRetained=first['status'],firstStrictCleanup=fc['status'])

def worktree_proof(commit,index,originals):
 p=E/'package-source-preparation-first/actual-checkout-first.json';need(p.relative_to(E).as_posix() in originals,'CHECKOUT_PROOF_ARCHIVED');proof=json_bytes(p.read_bytes())
 need(proof['status']=='PASS_ACTUAL_CI_CHECKOUT_BYTES_EXACT' and proof['sourceSha']==SOURCE and proof['actualExit'] is True and proof['exitCode']==0 and proof['detached'] is True and proof['checkoutClean'] is True and proof['checkout']==str(WT),'ACTUAL_ORIGINAL_CHECKOUT_PROOF')
 need(git('rev-parse','HEAD',root=WT).decode().strip()==SOURCE,'EXACT_WT_SOURCE_HEAD')
 head_file=Path(git('rev-parse','--git-path','HEAD',root=WT).decode().strip());head_file=head_file if head_file.is_absolute() else WT/head_file;need(head_file.read_bytes()==(SOURCE+'\n').encode(),'WT_ACTUAL_DETACHED_HEAD_BYTES')
 need(git('status','--porcelain','-z','--untracked-files=all',root=WT)==b'','WT_EXACT_CLEAN_BEFORE_NORMAL_REMOVE')
 names=selected(git('ls-files','-z',root=WT).decode().split('\0'));need(len(names)==544 and proof['selectedCount']==len(proof['selectedInputs'])==544,'ACTUAL_544_SELECTED_INPUT_DOMAIN')
 byproof={source_name(r['path']):r for r in proof['selectedInputs']};need(len(byproof)==544 and {WT.relative_to(E).as_posix()+'/'+n for n in names}==set(byproof),'WHOLE_SELECTED_DOMAIN_MEMBERSHIP')
 raw=owned(Path(proof['admission']['raw']));before=raw/'input-sha256.json';after=raw/'input-sha256-after.json';need(before.read_bytes()==after.read_bytes() and before.relative_to(E).as_posix() in originals and after.relative_to(E).as_posix() in originals,'ORIGINAL_CI_INPUTS_BEFORE_AFTER_IN_COMMITTED_TAR')
 inv=json_bytes(before.read_bytes());by={r['path']:r for r in inv};need(len(by)==len(inv),'UNIQUE_ORIGINAL_CI_INPUTS')
 rows=[]
 for n in names:
  w=fact(owned(WT/n));r=by[n];need((w['bytes'],w['sha256'])==(r['bytes'],r['sha256']) and w==byproof[(WT/n).relative_to(E).as_posix()],'RAW_WT_CRLF_BYTES_EXACT_CI_'+n)
  actual=committed_file(commit,n);old=git('show',SOURCE+':'+n);oid=git('rev-parse',SOURCE+':'+n).decode().strip();need(actual['gitBlobOidSha1']==oid and actual['sha256']==sha(old),'C1_MAIN_LF_BLOB_EQUALS_ORIGINAL_SOURCE_'+n)
  rows.append(dict(path=n,rawCheckoutBytes=w['bytes'],rawCheckoutSha256=w['sha256'],gitBlobBytes=actual['bytes'],gitBlobContentSha256=actual['sha256'],gitBlobOidSha1=oid))
 registered=git('worktree','list','--porcelain','-z');needle=('worktree '+str(WT)).encode()+b'\0';need(registered.count(needle)==1,'ONE_EXACT_REGISTERED_OWNED_WORKTREE')
 return dict(status='PASS_RAW_CI_CHECKOUT_AND_COMMITTED_LF_BLOBS_SEPARATELY',checkout=str(WT),sourceSha=SOURCE,selectedCount=544,inputs=rows,worktreesBefore=registered.decode().split('\0'))

def plan_tree(originals,excluded,pruned):
 allowed=set(originals)|set(excluded);prunes={r['path'] for r in pruned};need(len(prunes)==len(pruned),'UNIQUE_PRUNED_ROOTS');found=set();rows=[];count=0
 def inside(n):return any(n==p or n.startswith(p+'/') for p in prunes)
 def walk(p):
  nonlocal count
  for f in sorted(p.iterdir()):
   count+=1;need(count<=200000,'FULL_GENERATED_CLEANUP_ENTRY_BOUND');n=f.relative_to(E).as_posix();s=f.lstat();need(s.st_dev==E.lstat().st_dev,'NO_CROSS_FILESYSTEM_OWNED_TREE_DELETION')
   if stat.S_ISLNK(s.st_mode):need(inside(n),'UNARCHIVED_UNPRUNED_SYMLINK');rows.append(dict(path=n,kind='symlink',bytes=0));continue
   if stat.S_ISDIR(s.st_mode):walk(f);rows.append(dict(path=n,kind='directory',bytes=0));continue
   need(stat.S_ISREG(s.st_mode),'UNKNOWN_SPECIAL_FILE_NO_CLEANUP')
   if not inside(n):need(n in allowed,'UNKNOWN_UNARCHIVED_FILE_NO_CLEANUP_'+n);found.add(n)
   rows.append(dict(path=n,kind='file',bytes=s.st_size))
 walk(E);need(found==allowed,'ALL_NONPRUNED_ORIGINALS_AND_EXCLUDED_FILES_CLOSED');return rows

def main():
 p=argparse.ArgumentParser(description=__doc__);p.add_argument('--execute',action='store_true');p.add_argument('--source-commit');a=p.parse_args()
 if not a.execute:print(json.dumps(dict(status='NOT_RUN_PREPARATION_ONLY',sourceRoot=str(E),receipt=str(RESULT),sourceDeletion=False,Windows='NOT_CONNECTED')));return 0
 need(not RESULT.exists(),'FRESH_CLEANUP_RECEIPT_ONLY');need(DEST.is_dir() and not DEST.is_symlink(),'EXISTING_MAIN_ARCHIVE_DESTINATION')
 value=dict(status='RUNNING',sourceCommit=a.source_commit,originalCompiledSourceSha=SOURCE,startedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),scope='Only owned R78 local worktree/tests/build/cache after exact committed archive/source proof; no Windows/network call, process termination, global install/cache or private-key contents.',removedFiles=0,removedSymlinks=0,removedDirectories=0,logicalRegularFileBytesRemoved=0,sourceDeletionStarted=False,worktreeRemoval=None)
 try:
  need(re.fullmatch('[0-9a-f]{40}',a.source_commit or ''),'FULL_C1_GIT_SHA');need(E.is_dir() and not E.is_symlink() and (E/'.owner').read_bytes()==b'future-classroom-owned-r78-ci-evidence/v1\n','EXACT_R78_OWNER')
  need((D/'.owner').read_bytes()==b'future-classroom-owned-r78-local-cleanup-preparation/v1\n' and Path(__file__).resolve()==DEST/'local-cleanup-second.py','EXACT_COMMITTED_RECOVERY_CONTROLLER_LOCATION')
  need(sha((D/'cleanup-first.py').read_bytes())=='5aa9888c6a1b12adcabbf0e9e68ec5bf7062f8f99ccc47303aa8f318d3eabb68','ORIGINAL_ARCHIVED_CONTROLLER_UNCHANGED')
  need(sha((DEST/'local-cleanup-first.json').read_bytes())=='4d8ad878e26d1c978f64c0e2a7f29b6f72e5c82266d2d1ac3d5bcf6168e4d1fb','ORIGINAL_FIRST_CLEANUP_FAILURE_UNCHANGED')
  need(git('rev-parse','HEAD').decode().strip()==a.source_commit and git('status','--porcelain','-z','--untracked-files=all')==b'','CURRENT_C1_HEAD_AND_CLEAN_MAIN')
  key_before=key_metadata();protected_before=[metadata(q) for q in ZIP_PATHS]+[metadata(MAIN/'target')];value['persistentKeyMetadataBefore']=key_before;value['protectedLocalMetadataBefore']=protected_before
  value['changedGitBlobProof']=changed_blob_proof(a.source_commit);index,originals,excluded,archive=archive_proof(a.source_commit);value['archiveProof']=archive;value['worktreeProof']=worktree_proof(a.source_commit,index,originals);value['processBefore']=process_gate();tree=plan_tree(originals,excluded,index['prunedGeneratedTrees']);value['plannedOwnedEntries']=len(tree);value['plannedLogicalRegularBytes']=sum(r['bytes'] for r in tree if r['kind']=='file')
  need((E/'tools/.owner').read_bytes()==b'future-classroom-owned-r78-msi-tools/v1\n','EXACT_OWNED_TOOLS_MARKER');value['processBeforeMutation']=process_gate()
  value['sourceDeletionStarted']=True;value['worktreeRemovalStarted']=True
  cmd=['git','-C',str(MAIN),'-c','core.autocrlf=true','worktree','remove',str(WT)];start=time.monotonic();env=os.environ.copy();env.update(GIT_NO_LAZY_FETCH='1',GIT_TERMINAL_PROMPT='0');child=subprocess.Popen(cmd,stdout=subprocess.PIPE,stderr=subprocess.PIPE,env=env)
  value['worktreeRemoval']=dict(command=cmd,pid=child.pid,originalBudgetSeconds=180,actualExit=False,status='RUNNING_ORIGINAL_GIT_CHILD',forceUsed=False)
  try:out,err=child.communicate(timeout=180)
  except subprocess.TimeoutExpired:
   value['worktreeRemoval'].update(status='UNKNOWN_ORIGINAL_GIT_CHILD_DEADLINE_EXPIRED_NOT_TERMINATED',elapsedSeconds=time.monotonic()-start)
   raise AssertionError('ORIGINAL_WORKTREE_REMOVE_TIMEOUT_RETAIN_REMAINING_NO_PROCESS_TERMINATION') from None
  value['worktreeRemoval'].update(status='ACTUAL_ORIGINAL_CHILD_RETURNED',actualExit=True,parentReaped=True,exitCode=child.returncode,elapsedSeconds=time.monotonic()-start,stdout=out.decode('utf-8','strict'),stderr=err.decode('utf-8','strict'))
  need(child.returncode==0 and not WT.exists() and not WT.is_symlink(),'NORMAL_EXACT_WORKTREE_REMOVE_EXIT0')
  need(('worktree '+str(WT)).encode()+b'\0' not in git('worktree','list','--porcelain','-z'),'OWNED_WORKTREE_REGISTRATION_REMOVED')
  # Worktree deletion above is performed by Git, and is accounted separately.
  value['sourceDeletionStarted']=True;value['worktreeLogicalRegularBytesRemoved']=sum(r['bytes'] for r in tree if r['kind']=='file' and r['path'].startswith(WT.relative_to(E).as_posix()+'/'))
  remainder=plan_tree(originals,excluded,index['prunedGeneratedTrees'])
  for row in remainder:
   n=row['path'];f=E/n
   if n=='.owner':continue
   if row['kind']=='directory':need(f.is_dir() and not f.is_symlink(),'DELETE_REAL_DIRECTORY_ONLY');f.rmdir();value['removedDirectories']+=1
   elif row['kind']=='symlink':need(f.is_symlink(),'UNLINK_ORIGINAL_LINK_ONLY');f.unlink();value['removedSymlinks']+=1
   else:need(stat.S_ISREG(f.lstat().st_mode) and not f.is_symlink(),'UNLINK_REGULAR_FILE_ONLY');f.unlink();value['removedFiles']+=1;value['logicalRegularFileBytesRemoved']+=row['bytes']
  need(set(p.name for p in E.iterdir())=={'.owner'},'ONLY_OWNER_MARKER_REMAINS');owner=E/'.owner';owner_bytes=owner.stat().st_size;owner.unlink();value['removedFiles']+=1;value['logicalRegularFileBytesRemoved']+=owner_bytes;E.rmdir();value['removedDirectories']+=1
  need(not E.exists() and not E.is_symlink(),'OWNED_LOCAL_ROOT_ABSENT');need(git('rev-parse','HEAD').decode().strip()==a.source_commit and git('status','--porcelain','-z','--untracked-files=all')==b'','C1_MAIN_REMAINS_CLEAN_BEFORE_NEW_RECEIPT')
  value['persistentKeyMetadataAfter']=key_metadata();value['protectedLocalMetadataAfter']=[metadata(q) for q in ZIP_PATHS]+[metadata(MAIN/'target')];need(value['persistentKeyMetadataAfter']==key_before and value['protectedLocalMetadataAfter']==protected_before,'PERSISTENT_KEY_AND_INTENDED_ARTIFACTS_METADATA_UNCHANGED')
  value.update(status='PASS_EXACT_COMMITTED_EVIDENCE_AND_OWNED_R78_LOCAL_CLEANUP',ownedRootKnownAbsent=True,worktreeKnownAbsent=True,privateKeyContentsRead=False,windowsContacted=False,processesTerminated=0,physicalStorageReclaimed='NOT_MEASURED_LOGICAL_COUNTS_ONLY')
 except Exception as error:value.update(status='FAIL_RETAIN_REMAINING_ORIGINALS_NO_FORCE',errorType=type(error).__name__,reason=str(error) if isinstance(error,AssertionError) else 'PROOF_OR_OWNED_CLEANUP_OPERATION_FAILED',ownedRootKnownAbsent=not E.exists())
 value['readonlyGitDeadlineExceptions']=READ_OPERATIONS
 value['completedUtc']=datetime.datetime.now(datetime.timezone.utc).isoformat()
 with RESULT.open('x') as f:json.dump(value,f,ensure_ascii=False,indent=2);f.write('\n')
 print(json.dumps(dict(status=value['status'],receipt=str(RESULT),sourceRemoved=value.get('ownedRootKnownAbsent'),removedFiles=value['removedFiles'],removedSymlinks=value['removedSymlinks'],logicalRegularFileBytesRemoved=value['logicalRegularFileBytesRemoved'])));return 0 if value['status'].startswith('PASS_') else 1
if __name__=='__main__':raise SystemExit(main())
