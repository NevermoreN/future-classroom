from pathlib import Path
import json,os,stat,hashlib,time
p=Path('/tmp/FutureClassroom-Business-23836-1793493138680888507-0');ev=Path('/tmp/fc-round50-https-integration-evidence');out=ev/'host-timeout-scratch-cleanup-first.json';assert not out.exists();oldpid=23836;group=16878;source=Path('/tmp/fc-round47-https-settings-worktree/crates/fc-bootstrap/tests/voice_storage.rs');prior=ev/'full-matrix-second.json';d=json.loads(prior.read_text());cmd=d['commands'][-1];assert cmd['name']=='host-tests' and cmd['timedOut'] and cmd['actualExit'] and cmd['ownedProcessGroup']==group and cmd['exitCode']==-15
r={'status':'FAIL','scope':'One exact generated synthetic SQLite scratch from killed owned Host voice_storage test; no generic TEMP sweep and no signals. No DB contents read or archived.','root':str(p),'observedOldTestPid':oldpid,'oldCargoGroup':group,'originalActualTimeout':cmd,'source':{'path':str(source),'sha256':hashlib.sha256(source.read_bytes()).hexdigest()},'ownership':'voice_storage.rs Directory::new exact FutureClassroom-Business-<processid>-<SystemTime nanos>-<counter>; old testPID23836 recorded by root ps during owned originalCargo group. Original process and all exact-path references must be gone.','removed':[]}
try:
 assert '"FutureClassroom-Business-{}-{}-{}"' in source.read_text();assert not Path('/proc/'+str(oldpid)).exists();st=p.lstat();assert stat.S_ISDIR(st.st_mode) and st.st_uid==os.getuid() and not p.is_symlink();assert int(p.name.split('-')[-2])<prior.stat().st_mtime_ns
 names={x.name for x in p.iterdir()};assert names=={'state.sqlite3','state.sqlite3-wal','state.sqlite3-shm'};items=[]
 for q in p.iterdir():
  v=q.lstat();assert stat.S_ISREG(v.st_mode) and v.st_nlink==1 and v.st_uid==os.getuid();items.append({'name':q.name,'inode':v.st_ino,'device':v.st_dev,'bytes':v.st_size})
 refs=[];group_live=[]
 for process in Path('/proc').iterdir():
  if not process.name.isdigit():continue
  try:
   raw=(process/'stat').read_text();fields=raw[raw.rindex(')')+2:].split()
   if int(fields[2])==group:group_live.append(int(process.name))
   for q in [process/'cwd',process/'exe',*list((process/'fd').iterdir())]:
    try:t=os.readlink(q)
    except (FileNotFoundError,PermissionError,OSError):continue
    if t==str(p) or t.startswith(str(p)+'/'):refs.append({'pid':int(process.name),'kind':q.parent.name if q.parent.name=='fd' else q.name})
  except (FileNotFoundError,ProcessLookupError,PermissionError,OSError,ValueError):continue
 r.update(ownedLeafMetadata=items,oldGroupLivePids=group_live,exactPathReferences=refs);assert not refs and not group_live
 for item in items:
  q=p/item['name'];v=q.lstat();assert (v.st_dev,v.st_ino)==(item['device'],item['inode']) and stat.S_ISREG(v.st_mode);q.unlink();r['removed'].append(item['name'])
 assert (p.lstat().st_dev,p.lstat().st_ino)==(st.st_dev,st.st_ino);p.rmdir();assert not p.exists();r['status']='PASS_EXACT_OWNED_TIMEOUT_SCRATCH_REMOVED'
except Exception as e:r['error']={'type':type(e).__name__,'message':str(e)}
with out.open('x') as f:json.dump(r,f,indent=2);f.write('\n')
print(r['status']);raise SystemExit(0 if r['status'].startswith('PASS_') else 1)
