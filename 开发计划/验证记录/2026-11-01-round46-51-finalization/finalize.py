#!/usr/bin/env python3
"""Exact local proof, then separately requested cleanup of13 fixed owned roots.

No signals, privilege escalation, network, SSH, private-key/database reads,
global caches, repository target/debug, /mnt/data or new R52 roots are touched.
Process evidence is deliberately SAME_UID only; other UIDs are not certified.
"""
from pathlib import Path
import argparse,fcntl,hashlib,json,os,re,shutil,stat,subprocess,sys,time
sys.dont_write_bytecode=True
D=Path('/tmp/fc-round46-51-finalization-review');M=Path('/workspace/共享/Eliora/未来教室计划');CONFIG=D/'roots.json';UID=1026
LAST_INODES={}
PRIVATE={'.key','.pfx','.p12','.db','.sqlite','.sqlite3','.db-wal','.db-shm','.sqlite-wal','.sqlite-shm','.sqlite3-wal','.sqlite3-shm'}
sha=lambda b:hashlib.sha256(b).hexdigest()
def need(x,message):
 if not x:raise ValueError(message)
def safe_rel(s):
 p=Path(s);need(not p.is_absolute() and s and len(s)<4096 and '\n' not in s and '\0' not in s and '..' not in p.parts,'unsafe relative path');return p
def git(*args,cwd=M):return subprocess.check_output(['git','-c','core.fsmonitor=false','-c','core.untrackedCache=false',*args],cwd=cwd,timeout=60)
def opened(p,allow_binary=False):
 p=Path(p);need(p.is_absolute() and '..' not in p.parts,'absolute ordinary path required');need(p.suffix.lower() not in PRIVATE and not p.name.startswith(('id_rsa','id_ed25519')) and not p.name.endswith('.control.json'),'private/database/LIVE control read forbidden')
 fd=os.open('/',os.O_RDONLY|os.O_DIRECTORY)
 try:
  for part in p.parts[1:-1]:
   new=os.open(part,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW,dir_fd=fd);os.close(fd);fd=new
  file=os.open(p.name,os.O_RDONLY|os.O_NOFOLLOW,dir_fd=fd);info=os.fstat(file);need(stat.S_ISREG(info.st_mode),'not regular file')
  need(info.st_size<=128*1024*1024,'single file byte bound');return os.fdopen(file,'rb')
 finally:os.close(fd)
def bytes_(p):
 with opened(p) as f:return f.read()
def digest(p):
 h=hashlib.sha256();total=0
 with opened(p,True) as f:
  for b in iter(lambda:f.read(1024*1024),b''):h.update(b);total+=len(b)
 return {'bytes':total,'sha256':h.hexdigest()}
def load(p):return json.loads(bytes_(p).decode('utf-8-sig'))
def exact(p,row):
 got=digest(p);need(got['bytes']==row['bytes'] and got['sha256']==row['sha256'],'exact bytes/SHA mismatch: '+str(p));return got
def walk(root):
 root=Path(root);info=root.lstat();need(stat.S_ISDIR(info.st_mode) and info.st_uid==UID and not root.is_symlink(),'root owner/type mismatch: '+str(root))
 rows=[];inodes={};h=hashlib.sha256();paths=0
 def visit(p,rel):
  nonlocal paths
  s=p.lstat();paths+=1;need(paths<=500000,'root path count bound');need(not stat.S_ISLNK(s.st_mode),'symlink refused: '+str(p));need(stat.S_ISDIR(s.st_mode) or stat.S_ISREG(s.st_mode),'special file refused: '+str(p))
  need(s.st_uid==UID,'foreign owned entry refused: '+str(p));row=[rel,s.st_mode,s.st_dev,s.st_ino,s.st_size,s.st_mtime_ns,s.st_ctime_ns];h.update(json.dumps(row,separators=(',',':')).encode());h.update(b'\n');inodes[(s.st_dev,s.st_ino)]=str(root)
  if stat.S_ISREG(s.st_mode):
   need(p.suffix.lower() not in PRIVATE and not p.name.startswith(('id_rsa','id_ed25519')) and not p.name.endswith('.control.json'),'unknown private/database/LIVE file prevents deletion: '+str(p));rows.append(rel)
  else:
   with os.scandir(p) as it:children=sorted((x.name for x in it))
   for name in children:visit(p/name,name if not rel else rel+'/'+name)
 visit(root,'');return {'fingerprint':h.hexdigest(),'files':rows,'fileCount':len(rows),'uniqueInodes':len(inodes),'rootIdentity':{'dev':info.st_dev,'ino':info.st_ino,'uid':info.st_uid}},inodes
class Blobs:
 def __init__(self,commit):
  self.commit=commit;self.p=subprocess.Popen(['git','cat-file','--batch'],cwd=M,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE);self.cache={}
 def inspect(self,path,local=None):
  safe_rel(path);self.p.stdin.write((self.commit+':'+path+'\n').encode());self.p.stdin.flush();head=self.p.stdout.readline(4096).decode().strip().split();need(len(head)==3 and head[1]=='blob','missing committed blob: '+path);size=int(head[2]);need(size<=128*1024*1024,'Git blob bound');h=hashlib.sha256();remaining=size;f=opened(local) if local else None
  try:
   while remaining:
    b=self.p.stdout.read(min(1024*1024,remaining));need(b,'short Git blob');remaining-=len(b);h.update(b)
    if f:need(f.read(len(b))==b,'local bytes differ from committed blob: '+path)
   if f:need(not f.read(1),'local bytes extend committed blob: '+path)
   need(self.p.stdout.read(1)==b'\n','Git batch separator');out={'gitPath':path,'gitBlob':head[0],'bytes':size,'sha256':h.hexdigest()};self.cache[path]=out;return out
  finally:
   if f:f.close()
 def close(self):
  self.p.stdin.close();self.p.stdout.close();self.p.wait(timeout=30);need(self.p.returncode==0 and not self.p.stderr.read(),'Git batch failed');self.p.stderr.close()
def tree(commit):
 raw=git('ls-tree','-r','-z',commit);need(len(raw)<32*1024*1024,'Git tree metadata bound');rows={}
 for record in raw.split(b'\0'):
  if not record:continue
  left,path=record.split(b'\t',1);mode,kind,oid=left.decode().split();name=path.decode();safe_rel(name);need(kind=='blob' and mode in {'100644','100755'},'unexpected Git tree object');rows[name]=oid
 return rows
def compare_files(a,b):
 with opened(a) as x,opened(b) as y:
  while True:
   u=x.read(1024*1024);v=y.read(1024*1024);need(u==v,'archive/source byte comparison failed')
   if not u:return

def proc_refs(roots,inodes,groups):
 references=[];unseen=[];inspected=[];active_groups=[]
 for entry in sorted(Path('/proc').iterdir(),key=lambda p:int(p.name) if p.name.isdigit() else -1):
  if not entry.name.isdigit():continue
  pid=int(entry.name);owner=None
  try:
   owner=entry.stat().st_uid
   if owner!=UID:unseen.append({'pid':pid,'reason':'OTHER_UID_NOT_INSPECTED'});continue
   raw=(entry/'stat').read_text();parts=raw[raw.rfind(')')+2:].split();state=parts[0];start=parts[19];pgrp=int(parts[2])
   if state=='Z':inspected.append({'pid':pid,'state':'Z','scope':'Zombie has released userspace files/address space'});continue
   if pgrp in groups:active_groups.append({'pid':pid,'group':pgrp,'scope':'Original owned group number still present; conservatively blocks even possible PID reuse'})
   matched=set()
   def match(path,key=None):
    path=path.removesuffix(' (deleted)')
    for root in roots:
     if path==root or path.startswith(root+'/'):matched.add(root)
    if key in inodes:matched.add(inodes[key])
   for name in ['cwd','exe']:
    try:match(os.readlink(entry/name))
    except FileNotFoundError:
     if not entry.exists():raise ProcessLookupError()
     raise
   try:
    descriptors=list((entry/'fd').iterdir());need(len(descriptors)<=65536,'process fd enumeration bound')
    for fd in descriptors:
     try:
      target=os.readlink(fd);s=fd.stat();match(target,(s.st_dev,s.st_ino))
     except FileNotFoundError:continue
    with (entry/'maps').open('rb') as f:maps=f.read(16*1024*1024+1)
    need(len(maps)<=16*1024*1024,'process maps bound')
    for line in maps.decode(errors='replace').splitlines():
     fields=line.split(None,5)
     if len(fields)<5:continue
     dev=fields[3].split(':');key=(os.makedev(int(dev[0],16),int(dev[1],16)),int(fields[4]));path=fields[5] if len(fields)>5 else '';path=re.sub(r'\\([0-7]{3})',lambda m:chr(int(m[1],8)),path);match(path,key)
   except FileNotFoundError:
    if not entry.exists():raise ProcessLookupError()
    raise
   check=(entry/'stat').read_text();need(check[check.rfind(')')+2:].split()[19]==start,'PID identity changed while inspecting')
   inspected.append({'pid':pid,'startTicks':start,'cwdExeFdMaps':'INSPECTED'})
   if matched:references.append({'pid':pid,'roots':sorted(matched)})
  except (FileNotFoundError,ProcessLookupError):
   if owner==UID and entry.exists():
    try:
     current=(entry/'stat').read_text();zombie=current[current.rfind(')')+2:].split()[0]=='Z'
    except OSError:zombie=False
    if not zombie:references.append({'pid':pid,'reason':'LIVE_OWN_UID_PROCESS_RESOURCE_UNAVAILABLE_FOR_INSPECTION'})
  except (OSError,ValueError) as error:
   if entry.exists():
    if owner==UID:references.append({'pid':pid,'reason':'OWN_UID_PROCESS_COULD_NOT_BE_FULLY_INSPECTED','errorType':type(error).__name__})
    else:unseen.append({'pid':pid,'reason':'UID_OR_OTHER_UID_PROCESS_NOT_INSPECTED','errorType':type(error).__name__})
 return {'scope':'SAME_UID_1026_ONLY_NOT_GLOBAL_SYSTEM_PROOF','inspected':inspected,'otherUidNotInspected':unseen,'references':references,'archivedOwnedGroupsStillPresent':active_groups,'signalsSent':0}

def proof(commit,config):
 need(re.fullmatch('[0-9a-f]{40}',commit) and git('rev-parse','HEAD').decode().strip()==commit,'explicit current committed HEAD required');need(os.getuid()==UID,'run as exact owner UID1026; no automatic sudo');need(sha(bytes_(D/'.owner'))==config['toolOwnerSha256'],'tool owner mismatch')
 git_tree=tree(commit);batch=Blobs(commit);archives=[];bysha={};allroots=[];inode_map={};rootfacts={};groups=set()
 try:
  for binding in config['archives']:
   archive=Path(binding['archive']);index_raw=bytes_(archive/'INDEX.json');need(sha(index_raw)==binding['indexSha256'],'archive INDEX changed');idx=json.loads(index_raw);source=Path(binding['source']);need(idx['sourceRoot']==str(source),'sourceRoot changed');owner=binding['owner'];exact(owner['path'],owner)
   astate,_=walk(archive);expected={x['path'] for x in idx['files']}|{'INDEX.json','INDEX.md'};need(set(astate['files'])==expected,'archive membership differs from INDEX');prefix=archive.relative_to(M).as_posix();need({x for x in git_tree if x.startswith(prefix+'/')}=={prefix+'/'+x for x in expected},'committed archive membership differs')
   members=[]
   for rel in sorted(expected):
    row=batch.inspect(prefix+'/'+rel,archive/rel);need(row['gitBlob']==git_tree[prefix+'/'+rel],'Git tree/blob mismatch');members.append(row)
    if rel not in {'INDEX.json','INDEX.md'}:
     want=next(x for x in idx['files'] if x['path']==rel);need(row['bytes']==want['bytes'] and row['sha256']==want['sha256'],'archive source ledger mismatch');bysha.setdefault((row['bytes'],row['sha256']),[]).append(str(archive/rel))
   state,inos=walk(source);expected_source={x['path'] for x in idx['files']}|{x['path'] for x in idx.get('excluded',[]) if x.get('type')!='excluded-directory'};need(set(state['files'])==expected_source,'evidence root has unindexed or missing originals');known={}
   if 'round46-msvc' in str(archive):
    for tag in ['modern','win7']:
     audit=load(archive/(tag+'-original-independent-first.json'));need(audit['status']=='PASS_STATIC_ONLY_RUNTIME_NOT_RUN','R46 artifact audit notPASS')
     for row in audit['executables']:known[row['localPath']]=row['producer']
     known[audit['zip']['path']]=audit['zip']
   for row in idx['files']:
    exact(source/row['path'],row);compare_files(source/row['path'],archive/row['path'])
    if row['path'].endswith('.json'):
     def collect(x):
      if isinstance(x,dict):
       if type(x.get('ownedProcessGroup')) is int and x['ownedProcessGroup']>1:groups.add(x['ownedProcessGroup'])
       for v in x.values():
        if isinstance(v,(dict,list)):collect(v)
      elif isinstance(x,list):
       for v in x:
        if isinstance(v,(dict,list)):collect(v)
     collect(load(archive/row['path']))
   for row in idx.get('excluded',[]):
    p=source/row['path'];need(str(p) in known,'unproven excluded file or subtree: '+str(p));exact(p,known[str(p)])
   rootfacts[str(source)]=state;inode_map.update(inos);allroots.append(str(source));archives.append({'path':str(archive),'source':str(source),'members':members,'sourceOriginals':len(idx['files']),'excludedBinaryShaVerified':len(known)})
  registrations=git('worktree','list','--porcelain').decode();worktrees=[]
  for binding in config['worktrees']:
   p=Path(binding['path']);need('worktree '+str(p)+'\n' in registrations,'worktree not registered');need(git('branch','--show-current',cwd=p).decode().strip()==binding['branch'] and git('rev-parse','HEAD',cwd=p).decode().strip()==binding['baseHead'],'worktree original branch/base changed');g=digest(p/'.git');need(g['bytes']==binding['gitFileBytes'] and g['sha256']==binding['gitFileSha256'],'worktree .git ownership changed')
   for item in git('ls-files','-v','-z',cwd=p).decode().split('\0'):
    if item:need(item[0]=='H','assume-unchanged/skip-worktree/nonordinary index refused')
   tracked=set(git('ls-files','-z',cwd=p).decode().rstrip('\0').split('\0'));untracked=set(x for x in git('ls-files','--others','--exclude-standard','-z',cwd=p).decode().split('\0') if x);ignored=set(x for x in git('ls-files','--others','--ignored','--exclude-standard','-z',cwd=p).decode().split('\0') if x)
   dirty=set(x for x in git('diff','HEAD','--no-renames','--name-only','-z',cwd=p).decode().split('\0') if x)|untracked|ignored
   state,inos=walk(p);need(set(state['files'])==tracked|untracked|ignored|{'.git'},'unclassified worktree files or deletions');rows=[]
   for rel in sorted(dirty):
    safe_rel(rel);file=p/rel;need(file.suffix.lower() in {'.rs','.toml','.lock','.py','.ps1','.mjs','.yml','.yaml','.md','.txt','.json'} or rel in {'.gitignore','.gitattributes'},'unknown modified/untracked file type');got=digest(file);hits=bysha.get((got['bytes'],got['sha256']),[]);head=None
    if rel in git_tree:
     candidate=batch.inspect(rel)
     if candidate['sha256']==got['sha256'] and candidate['bytes']==got['bytes']:head=batch.inspect(rel,file)
    need(head or hits,'modified/untracked bytes absent from committed HEAD and all exact archives: '+str(file))
    if hits:compare_files(file,Path(hits[0]))
    rows.append({'path':rel,**got,'headExact':head,'archiveExact':hits})
   rootfacts[str(p)]=state;inode_map.update(inos);allroots.append(str(p));worktrees.append({**binding,'modifiedUntrackedIgnored':rows,'ignoredFiles':len(ignored),'status':'ALL_PRESENT_DIRTY_BYTES_TRACEABLE'})
  compiled=load(M/'开发计划/验证记录/2026-11-01-round50-https-integration/compiled-programs-preliminary-first.json');outputs=[]
  for binding in config['outputs']:
   p=Path(binding['path']);owner=digest(p/'.owner');need(owner['bytes']==binding['ownerBytes'] and owner['sha256']==binding['ownerSha256'],'output owner mismatch');need(sorted(x.name for x in p.iterdir())==binding['topMembers'],'output top-level membership changed');state,inos=walk(p)
   if binding['kind']=='programs':
    wants={Path(x['path']).name:x for x in compiled['artifacts']};need(set(state['files'])==set(wants)|{'.owner'},'programs root contains unknown files')
    for name,row in wants.items():exact(p/name,row)
   rootfacts[str(p)]=state;inode_map.update(inos);allroots.append(str(p));outputs.append({**binding,'basis':'Exact22 compiler artifacts SHA' if binding['kind']=='programs' else 'Exact original owned cache marker/topology; all entries metadata-only reproducible build outputs, no private/DB/symlink tolerated'})
 finally:batch.close()
 processes=proc_refs(allroots,inode_map,groups);need(not processes['references'] and not processes['archivedOwnedGroupsStillPresent'],'owned UID process or original task group still references roots / could not inspect')
 for root,want in rootfacts.items():need(walk(Path(root))[0]==want,'root metadata/membership changed during proof')
 need(len(allroots)==13 and len(set(allroots))==13,'exact13-root allowlist');LAST_INODES.clear();LAST_INODES.update(inode_map);stamp=sha(json.dumps({'commit':commit,'roots':rootfacts},sort_keys=True,separators=(',',':')).encode())
 return {'status':'PASS_COMMITTED_BYTES_AND_SAME_UID_PROCESS_PROOF','commit':commit,'scope':'Only UID1026 physical references checked. Other UIDs explicitly not inspected; no full-system no-reference assertion. No signals or cleanup in proof mode.','archives':archives,'archiveMembers':sum(len(x['members']) for x in archives),'sourceOriginals':sum(x['sourceOriginals'] for x in archives),'worktrees':worktrees,'outputs':outputs,'roots':rootfacts,'processes':processes,'archivedOwnedProcessGroups':sorted(groups),'validationDigest':stamp}

def main():
 ap=argparse.ArgumentParser(description=__doc__);ap.add_argument('mode',choices=['proof','cleanup']);ap.add_argument('--commit',required=True);ap.add_argument('--report',type=Path,required=True);ap.add_argument('--proof',type=Path);ap.add_argument('--confirm-thirteen-owned-roots',action='store_true');a=ap.parse_args();need(a.report.parent==D and not a.report.exists(),'fresh report only inside finalization root');config=load(CONFIG);need(config['ownerUid']==UID,'configuration owner changed');report={'status':'FAIL','mode':a.mode,'actions':[],'signalsSent':0,'mainSourceModified':False,'cleanupPerformed':False,'scope':'Exact13 owned roots only; no main files/global target/debug/SSH/R52/mntdata deletion.'}
 with (D/'run.lock').open('a') as lock:
  fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
  try:
   current=proof(a.commit,config);report['proof']=current
   if a.mode=='proof':report['status']=current['status']
   else:
    need(a.confirm_thirteen_owned_roots and a.proof and a.proof.parent==D,'explicit exact13 cleanup confirmation and original proof required');old=load(a.proof);need(old.get('status')=='PASS_COMMITTED_BYTES_AND_SAME_UID_PROCESS_PROOF' and old['proof']['validationDigest']==current['validationDigest'],'original proof no longer matches currentHEAD/roots')
    roots=list(current['roots']);worktree_paths={x['path'] for x in config['worktrees']};ordered=[x['path'] for x in config['worktrees']]+[x['path'] for x in config['outputs']]+[x['source'] for x in config['archives']]
    need(set(ordered)==set(roots) and shutil.rmtree.avoids_symlink_attacks,'exact allowed removal set and fd-safe removal required')
    for root in ordered:
     p=Path(root);need(walk(p)[0]==current['roots'][root],'root changed immediately before cleanup');references=proc_refs(ordered,LAST_INODES,set());need(not references['references'],'sameUID new physical reference prevents remaining cleanup')
     if root in worktree_paths:subprocess.run(['git','worktree','remove','--force',root],cwd=M,check=True,capture_output=True,timeout=120)
     else:shutil.rmtree(p)
     need(not p.exists(),'owned root still exists after removal');report['actions'].append({'path':root,'removed':True,'basis':'Exact committed byte proof/owner/metadata and fresh sameUID negative physical-reference check'})
    report.update(status='PASS_THIRTEEN_OWNED_ROOTS_REMOVED_SAME_UID_SCOPE',cleanupPerformed=True,branchRefs='RETAINED_NO_BRANCH_DELETE',scopeBoundary='Other UID processes not certified. No signals, privilege escalation, global cache/target/debug, main source, SSH, R52 or /mnt/data changes.')
  except Exception as error:report.update(error={'type':type(error).__name__,'message':str(error)[:1000]},partialCleanup=bool(report['actions']))
  report['toolSha256']=sha(bytes_(Path(__file__)));report['configSha256']=sha(bytes_(CONFIG))
  with a.report.open('x') as f:json.dump(report,f,ensure_ascii=False,indent=2);f.write('\n')
 print(json.dumps({'status':report['status'],'report':str(a.report),'sha256':sha(bytes_(a.report)),'removed':len(report['actions']),'error':report.get('error')},ensure_ascii=False));return 0 if report['status'].startswith('PASS') else 1
if __name__=='__main__':raise SystemExit(main())
