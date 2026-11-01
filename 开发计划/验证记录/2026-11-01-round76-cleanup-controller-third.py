#!/usr/bin/env python3
"""R76 local cleanup, default inert; requires actual committed exact archive."""
from pathlib import Path
import argparse,hashlib,json,os,re,stat,subprocess,sys,tarfile,time
MAIN=Path('/workspace/共享/Eliora/未来教室计划')
E=Path('/tmp/fc-round76-ci-evidence');C=Path('/tmp/fc-round76-cleanup-controller-second')
DEST=MAIN/'开发计划/验证记录/2026-11-01-round76-ci'
WT=E/'msi-checkout-preparation-first/checkout-3bad949-first'
BASE='3bad94949012e840cd1dda6a73a0ea1ddb067697'
KEY=Path('/config/.local/share/future-classroom-test-access/win11-192.168.10.212')
PRUNES={'tools','msi-checkout-preparation-first/checkout-3bad949-first','web-build-first/source','web-build-first/npm-cache','web-build-first/process-temp/node-compile-cache'}
SOURCES={'scripts/package-msi-candidate.py':'8ca2f818b481f5cd4a2c9fd4cee4f4d282c505f4f3ccb45ec28b0dc719757378','scripts/check-msi-candidate.py':'18dc6664d514858af926ca6cb5cd605b188ab5fb842d13d66661e0b16f4605df','packaging/MSI-CANDIDATE.md':'2e2e9d734986c037e71043f0c54ae18dfc36073e18f4a6b3643b48d18e8fd65e'}
def need(v,c):
 if not v:raise AssertionError(c)
def sha(b):return hashlib.sha256(b).hexdigest()
def regular(p):need(stat.S_ISREG(p.lstat().st_mode),'REGULAR_NO_SYMLINK '+str(p))
def identity(p):
 regular(p);h=hashlib.sha256();size=0
 with p.open('rb') as f:
  for b in iter(lambda:f.read(1024*1024),b''):h.update(b);size+=len(b)
 return dict(bytes=size,sha256=h.hexdigest())
def relative(p):
 need(isinstance(p,str) and p and '\\' not in p and ':' not in p and not p.startswith('/') and all(x not in ('','.','..') for x in p.split('/')),'SAFE_RELATIVE_PATH');return p
def exact(p,row):need(identity(p)=={k:row[k] for k in ('bytes','sha256')},'EXACT_BYTES '+str(p))
def no_alias(p):
 while p!=p.parent:need(not p.is_symlink(),'NO_ANCESTOR_SYMLINK '+str(p));p=p.parent

def inventory(root,prune=()):
 rows={};files=set();skipped=set();q=[root]
 while q:
  d=q.pop()
  with os.scandir(d) as entries:
   for x in entries:
    p=Path(x.path);rel=p.relative_to(root).as_posix();a=x.stat(follow_symlinks=False)
    kind='symlink' if stat.S_ISLNK(a.st_mode) else 'dir' if stat.S_ISDIR(a.st_mode) else 'file' if stat.S_ISREG(a.st_mode) else 'unknown'
    need(kind!='unknown','NO_SPECIAL_ENTRY '+rel)
    rows[rel]=(kind,a.st_size,a.st_mtime_ns,a.st_dev,a.st_ino,os.readlink(p) if kind=='symlink' else None)
    if rel in prune:need(kind=='dir','EXACT_REAL_PRUNED_DIRECTORY');skipped.add(rel);continue
    if kind=='dir':q.append(p)
    elif kind=='file':files.add(rel)
    elif prune:raise AssertionError('NO_UNPRUNED_SYMLINK '+rel)
 return rows,files,skipped

def related_processes():
 excluded={os.getpid()};pid=os.getpid()
 while pid>1:
  try:pid=int(next(x for x in Path('/proc',str(pid),'status').read_text().splitlines() if x.startswith('PPid:')).split()[1]);excluded.add(pid)
  except (FileNotFoundError,ProcessLookupError,StopIteration):break
 found=[]
 for d in Path('/proc').iterdir():
  if not d.name.isdecimal() or int(d.name) in excluded:continue
  try:
   cmd=(d/'cmdline').read_bytes();cwd=os.readlink(d/'cwd');exe=os.readlink(d/'exe')
   if str(E).encode() in cmd or cwd==str(E) or cwd.startswith(str(E)+'/') or exe.startswith(str(E)+'/'):found.append(int(d.name))
  except (FileNotFoundError,ProcessLookupError):continue
  except PermissionError:raise AssertionError('PROCESS_VISIBILITY_UNKNOWN '+d.name)
 return found

def remove_nonfollowing(root):
 files=0;logical=0;links=0;dirs=0
 for d,ds,fs in os.walk(root,topdown=False,followlinks=False):
  for name in fs+ds:
   p=Path(d)/name;a=p.lstat()
   if stat.S_ISLNK(a.st_mode):p.unlink();links+=1
   elif stat.S_ISREG(a.st_mode):p.unlink();files+=1;logical+=a.st_size
   elif stat.S_ISDIR(a.st_mode):p.rmdir();dirs+=1
   else:raise AssertionError('UNKNOWN_DELETE_KIND')
 root.rmdir();return dict(regularFiles=files,logicalBytes=logical,symlinks=links,directories=dirs+1)

def main():
 ap=argparse.ArgumentParser(description=__doc__)
 for name in ('commit','index','expected-index-sha256','archive','expected-archive-sha256','controller-committed-path'):ap.add_argument('--'+name)
 ap.add_argument('--all-controllers-reaped',action='store_true');ap.add_argument('--execute',action='store_true');a=ap.parse_args()
 if not a.execute:print(json.dumps(dict(status='PREPARED_NO_ACTIONS',targets=[str(E),str(WT)],needs='Actual committed index/tar/controller pins and all controllers terminal')));return 0
 report=MAIN/'开发计划/验证记录/2026-11-01-round76-cleanup-second.json';need(not report.exists(),'FRESH_CLEANUP_RECEIPT')
 r=dict(status='FAIL_NO_DELETION',operations=[],rootRemoved=False,worktreeRemoved=False,scope='Only R76 E and its exact registered detached worktree; no Windows/private-key content/cache/main-target/intent ZIP operations');start=time.monotonic()
 def command(argv):
  cp=subprocess.run(argv,cwd=MAIN,capture_output=True,timeout=120);r['operations'].append(dict(command=argv,actualExit=True,exitCode=cp.returncode,stdout={'bytes':len(cp.stdout),'sha256':sha(cp.stdout)},stderr={'bytes':len(cp.stderr),'sha256':sha(cp.stderr),'text':cp.stderr.decode('utf8','strict')}));need(cp.returncode==0,'COMMAND_EXIT0');return cp.stdout
 def blob(path,expected):
  relative(path);data=command(['git','show',a.commit+':'+path]);need(dict(bytes=len(data),sha256=sha(data))==expected,'COMMITTED_GIT_BLOB '+path);exact(MAIN/path,expected)
 try:
  need(a.all_controllers_reaped and re.fullmatch('[0-9a-f]{40}',a.commit or ''),'EXPLICIT_TERMINAL_COMMIT')
  for value in (a.expected_index_sha256,a.expected_archive_sha256):need(re.fullmatch('[0-9a-f]{64}',value or ''),'EXPLICIT_ARCHIVE_SHA')
  need(Path(__file__).absolute().parent==C and C.is_dir() and not C.is_symlink(),'EXACT_REAL_CONTROLLER_ROOT');regular(C/'.owner');regular(E/'.owner')
  need((C/'.owner').read_bytes()==b'future-classroom-owned-r76-cleanup-controller-second/v1\n','OWNED_CONTROLLER')
  need(E.is_dir() and not E.is_symlink() and (E/'.owner').read_bytes()==b'future-classroom-owned-r76-ci-evidence/v1\n','OWNED_E_ROOT')
  no_alias(E);no_alias(C);baseline_inventory,_,_=inventory(E);index=Path(a.index);archive=Path(a.archive)
  need(index==DEST/'archive-index-first.json' and archive==DEST/'evidence-originals-first.tar.gz','EXACT_MAIN_ARCHIVE_PATHS');no_alias(index);no_alias(archive)
  need(identity(index)['sha256']==a.expected_index_sha256 and identity(archive)['sha256']==a.expected_archive_sha256,'EXPLICIT_ACTUAL_ARCHIVE_PINS')
  j=json.loads(index.read_text());need(j['schemaVersion']==3 and j['sourceRoot']==str(E) and j['status']=='ORIGINAL_BYTES_ARCHIVED_IN_GZIP_TAR','EXACT_INDEX_SCHEMA3')
  container=j['archiveContainer'];need(container['path']==archive.name,'EXACT_CONTAINER_NAME');exact(archive,container)
  command(['git','merge-base','--is-ancestor',a.commit,'HEAD'])
  archived={index.name,archive.name};raw={};standalone=[]
  for row in j['files']:
   path=relative(row['path']);need(path not in raw,'UNIQUE_RAW_MEMBER');raw[path]=row;exact(E/path,row)
   if row.get('storage')=='tar_gzip_original_bytes':need(row['archivePath']==archive.name and row['tarMember']==path,'EXACT_TAR_MEMBER_PATH')
   else:need(row.get('storage')=='exact_original_bytes' and row['archivePath']==path and path=='archive-verification-first.json','ONLY_EXACT_STANDALONE_RECEIPT');exact(DEST/path,row);standalone.append(path);archived.add(path)
  wanted={p for p,row in raw.items() if row.get('storage')=='tar_gzip_original_bytes'};seen=set()
  with archive.open('rb') as f:header=f.read(10)
  need(header[:3]==b'\x1f\x8b\x08' and header[3]==0 and header[4:8]==b'\0'*4,'DETERMINISTIC_GZIP_HEADER')
  with tarfile.open(archive,'r:gz') as t:
   for member in t:
    p=relative(member.name);need(p in wanted and p not in seen and member.isreg() and not member.issym() and not member.islnk(),'EXACT_REGULAR_TAR_MEMBER');seen.add(p)
    need(member.uid==member.gid==member.mtime==0 and member.mode==0o600 and member.uname==member.gname=='','FIXED_TAR_METADATA')
    row=raw[p];need(member.size==row['bytes'],'TAR_MEMBER_SIZE');f=t.extractfile(member);data=f.read();need(len(data)==row['bytes'] and sha(data)==row['sha256'] and data==(E/p).read_bytes(),'TAR_ORIGINAL_SOURCE_REPLAY')
  need(seen==wanted and len(seen)==container['memberCount'] and len(raw)==j['originalFiles'],'COMPLETE_TAR_SET')
  for row in j['promotedOriginals']:
   p=relative(row['path']);need(p in wanted and row['archivePath']==p,'PROMOTED_SAME_ORIGINAL');exact(DEST/p,row);need(identity(E/p)==identity(DEST/p),'PROMOTED_SOURCE_BYTE_EQUAL');archived.add(p)
  archive_inventory,actual_archive,_=inventory(DEST);need(all(x[0] in ('file','dir') for x in archive_inventory.values()),'NO_ARCHIVE_SYMLINK');need(actual_archive==archived and len(archived)==j['archiveFileCount'],'COMPLETE_MAIN_ARCHIVE_MEMBERSHIP')
  for rel in archived:
   no_alias(DEST/rel);blob((DEST/rel).relative_to(MAIN).as_posix(),identity(DEST/rel))
  for rel,digest in SOURCES.items():need(identity(MAIN/rel)['sha256']==digest,'FROZEN_B5_MAIN_SOURCE');blob(rel,identity(MAIN/rel))
  own=Path(__file__).absolute();regular(own);ownrel=relative(a.controller_committed_path);no_alias(MAIN/ownrel);regular(MAIN/ownrel);need((MAIN/ownrel).read_bytes()==own.read_bytes(),'EXACT_COMMITTED_CONTROLLER_COPY');blob(ownrel,identity(own))
  pruned={relative(x['path']) for x in j['prunedSubtrees']};need(pruned==PRUNES,'ONLY_FIVE_OWNED_PRUNED_SUBTREES')
  for p,marker in [('tools',b'future-classroom-owned-r76-msi-tools/v1\n'),('web-build-first',b'future-classroom-owned-r76-web-build/v1\n'),('msi-checkout-preparation-first',b'future-classroom-owned-r76-msi-checkout-preparation/v1\n')]:regular(E/p/'.owner');need((E/p/'.owner').read_bytes()==marker,'PRUNED_SUBTREE_OWNER')
  excluded={relative(x['path']):x for x in j['excludedCompiledPayloads']};need(len(excluded)==len(j['excludedCompiledPayloads']) and not (set(raw)&set(excluded)),'UNIQUE_DISJOINT_EXCLUDED')
  for p,row in excluded.items():exact(E/p,row)
  for field,count in [('explicitPrunedPackageIdentities',7),('acceptedCiPeIdentities',48)]:
   need(len(j[field])==count,'EXPECTED_EXPLICIT_PAYLOAD_COUNT')
   for row in j[field]:p=relative(row['path']);exact(E/p,row);need(field!='acceptedCiPeIdentities' or p in excluded,'CI_PE_MUST_BE_EXCLUDED')
  _,source_files,skipped=inventory(E,pruned);need(source_files==set(raw)|set(excluded) and skipped==pruned,'COMPLETE_ORIGINAL_UNPRUNED_SOURCE_MEMBERSHIP')
  need((E/'archive-verification-first.json').read_bytes()==(DEST/'archive-verification-first.json').read_bytes(),'ORIGINAL_VERIFICATION_RECEIPT')
  registration=command(['git','worktree','list','--porcelain','-z']);records=registration.split(b'\0\0');matches=[x.split(b'\0') for x in records if x.startswith(('worktree '+str(WT)).encode()+b'\0')]
  need(len(matches)==1 and ('HEAD '+BASE).encode() in matches[0] and b'detached' in matches[0],'EXACT_REGISTERED_DETACHED_WORKTREE')
  need(command(['git','-C',str(WT),'rev-parse','HEAD']).decode().strip()==BASE and command(['git','-c','core.autocrlf=true','-C',str(WT),'status','--porcelain=v1','-z','--untracked-files=all'])==b'','OWNED_WORKTREE_CLEAN_BASE')
  key=KEY.lstat();need(stat.S_ISREG(key.st_mode) and stat.S_IMODE(key.st_mode)==0o600,'PERSISTENT_SSH_PRIVATEKEY_METADATA_0600_ONLY');r['preservedPrivateKeyMetadata']={'path':str(KEY),'mode':'0600','contentsRead':False}
  need(not related_processes(),'NO_ACTIVE_RELATED_PROCESS');snapshot,_,_=inventory(E);need(snapshot==baseline_inventory,'NO_DELTA_DURING_ALL_ARCHIVE_SOURCE_CHECKS');need(not related_processes(),'NO_ACTIVE_PROCESS_BEFORE_ACTION')
  need(inventory(E)[0]==snapshot,'NO_SOURCE_DELTA_BEFORE_ACTION');wt_prefix=WT.relative_to(E).as_posix();wt_entries={k:v for k,v in snapshot.items() if k==wt_prefix or k.startswith(wt_prefix+'/')}
  r['status']='FAIL_AFTER_CLEANUP_ACTION_STARTED';command(['git','worktree','remove',str(WT)]);need(not WT.exists(),'WORKTREE_REMOVED_NORMALLY');r['worktreeRemoved']=True
  after,_,_=inventory(E);expected={k:v for k,v in snapshot.items() if k not in wt_entries};need(set(after)==set(expected),'ONLY_EXACT_WORKTREE_REMOVAL_MEMBERSHIP_DELTA');parent=WT.parent.relative_to(E).as_posix()
  for key,value in after.items():need(value==expected[key] if key!=parent else all(value[i]==expected[key][i] for i in (0,3,4,5)),'ONLY_OWN_PARENT_DIRECTORY_METADATA_CHANGED')
  need(not related_processes(),'NO_ACTIVE_PROCESS_BEFORE_ROOT_REMOVAL');removed=remove_nonfollowing(E);r.update(status='PASS_COMMITTED_R76_EXACT_LOCAL_CLEANUP',rootRemoved=not E.exists(),remainingRootRemoved=removed,worktreeOriginalRegularFiles=sum(x[0]=='file' for x in wt_entries.values()),worktreeOriginalLogicalBytes=sum(x[1] for x in wt_entries.values() if x[0]=='file'),worktreeOriginalSymlinks=sum(x[0]=='symlink' for x in wt_entries.values()),physicalSpaceReclaimed='NOT_CLAIMED')
 except Exception as e:r.update(errorType=type(e).__name__,reason=str(e) if isinstance(e,AssertionError) else 'LOCAL_CHECK_OR_REMOVAL_ERROR')
 if r['rootRemoved']:r['totalRegularFilesRemoved']=r['remainingRootRemoved']['regularFiles']+r['worktreeOriginalRegularFiles'];r['totalLogicalBytesRemoved']=r['remainingRootRemoved']['logicalBytes']+r['worktreeOriginalLogicalBytes'];r['totalSymlinksRemovedNonfollowing']=r['remainingRootRemoved']['symlinks']+r['worktreeOriginalSymlinks']
 r.update(elapsedSeconds=time.monotonic()-start,commit=a.commit,indexSha256=a.expected_index_sha256,archiveSha256=a.expected_archive_sha256,controllerSourceSha256=sha(Path(__file__).read_bytes()))
 with report.open('x') as f:json.dump(r,f,ensure_ascii=False,indent=2);f.write('\n')
 print(json.dumps({'status':r['status'],'report':str(report),'rootRemoved':r['rootRemoved']}));return 0 if r['status']=='PASS_COMMITTED_R76_EXACT_LOCAL_CLEANUP' and r['rootRemoved'] else 1
if __name__=='__main__':raise SystemExit(main())
