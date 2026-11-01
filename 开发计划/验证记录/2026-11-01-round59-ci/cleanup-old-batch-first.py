from pathlib import Path
import os,json,hashlib,subprocess,shutil,stat,datetime
R=Path('/workspace/共享/Eliora/未来教室计划');E=Path('/tmp/fc-round59-ci-evidence');H=lambda b:hashlib.sha256(b).hexdigest();base='9ebe818bfc57c075e8b21419082dd44130b0e923'
commit=subprocess.check_output(['git','rev-parse','HEAD'],cwd=R,text=True).strip();proof=R/'开发计划/验证记录/2026-11-01-round55-58-committed-bytes-first.json';p=json.loads(proof.read_text());assert p['status']=='PASS_COMMITTED_GIT_BLOBS_MATCH_ALL_ORIGINAL_BYTES' and p['commit']==commit and p['count']==825
roots=[Path('/tmp')/x for x in ['fc-round55-ci-evidence','fc-round56-transactional-autorun-worktree','fc-round56-transactional-autorun-target','fc-round56-transactional-autorun-evidence','fc-round57-ci-inventory-worktree','fc-round57-ci-inventory-evidence','fc-round58-msvc-task-memory-worktree','fc-round58-msvc-task-memory-target','fc-round58-msvc-task-memory-evidence']]
assert all(x.exists() and not x.is_symlink() and x.parent==Path('/tmp') and x.resolve()==x for x in roots)
archive_pairs=[(roots[0],'2026-11-01-round55-ci'),(roots[3],'2026-11-01-round56-txr-experiment'),(roots[5],'2026-11-01-round57-ci-inventory'),(roots[8],'2026-11-01-round58-task-memory')]
guards=[]
for source,name in archive_pairs:
 d=R/'开发计划/验证记录'/name;idx=json.loads((d/'archive-index-first.json').read_text());excluded=json.loads((d/'archive-candidate-manifest-first.json').read_text())['excluded'] if name.endswith('round55-ci') else idx['excluded'];expected={x['path'] for x in idx['files']}|{x['path'] for x in excluded};actual={str(x.relative_to(source)) for x in source.rglob('*') if x.is_file()};assert actual==expected
 for row in idx['files']:
  b=(source/row['path']).read_bytes();assert len(b)==row['bytes'] and H(b)==row['sha256'] and b==(d/row['path']).read_bytes()
 for row in excluded:
  q=source/row['path'];assert q.stat().st_size==row['bytes']
  if q.name=='.owner':assert H(q.read_bytes())==row['sha256']
 guards.append({'root':str(source),'archive':str(d.relative_to(R)),'includedOriginalsVerified':len(idx['files']),'excludedWithOriginalIdentities':len(excluded),'sourcePathSetUnchanged':True})
assert (roots[2]/'.owner').read_text()=='future-classroom-owned-r56-transactional-autorun-target/v1\n'
assert (roots[7]/'.owner').read_bytes()==(roots[8]/'.owner').read_bytes()
worktrees=[(roots[1],'codex/transactional-autorun',R/'开发计划/验证记录/2026-11-01-round56-txr-experiment/withdrawn-source-first'),(roots[4],'codex/ci-inventory-capacity',R),(roots[6],'codex/msvc-task-memory',R)]
expected_paths={str(roots[1]):['crates/fc-bootstrap/src/application.rs','crates/fc-windows/src/system/autorun.rs','crates/fc-windows/src/system/autorun_tests.rs','crates/fc-windows/src/system/autorun_transaction.rs'],str(roots[4]):['scripts/windows-msvc-ci.ps1','scripts/check-ci-source-inventory.ps1','scripts/CI-SOURCE-INVENTORY.md'],str(roots[6]):['crates/fc-windows/Cargo.toml','crates/fc-windows/src/lib.rs','crates/fc-windows/src/native.rs','crates/fc-windows/src/materials/chooser.rs','crates/fc-windows/src/audio/device.rs','crates/fc-windows/src/com_task.rs','crates/fc-windows/src/com_task/tests.rs']}
for w,branch,reference in worktrees:
 assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=w,text=True).strip()==base
 assert subprocess.check_output(['git','branch','--show-current'],cwd=w,text=True).strip()==branch
 raw=subprocess.check_output(['git','status','--porcelain','-z','--untracked-files=all'],cwd=w);names={v[3:].decode() for v in raw.split(b'\0') if v};assert names==set(expected_paths[str(w)]),(str(w),sorted(names))
 for rel in names:assert (w/rel).read_bytes()==(reference/rel).read_bytes(),rel
 guards.append({'root':str(w),'branch':branch,'base':base,'allChangedPathsArchivedAndExact':len(names),'gitWorktreeRegistered':True})
active=[]
for q in Path('/proc').iterdir():
 if not q.name.isdigit() or int(q.name) in {os.getpid(),os.getppid()}:continue
 try:
  cmd=(q/'cmdline').read_bytes().replace(b'\0',b' ').decode(errors='replace');cwd=os.readlink(q/'cwd')
  if any(str(x) in cmd or cwd==str(x) or cwd.startswith(str(x)+'/') for x in roots):active.append(int(q.name))
 except (FileNotFoundError,PermissionError,ProcessLookupError):pass
assert not active,('active owned consumers',active)
facts=[]
for root in roots:
 files=[q for q in root.rglob('*') if q.is_file() and not q.is_symlink()];facts.append({'root':str(root),'regularFiles':len(files),'logicalBytes':sum(q.stat().st_size for q in files),'ownerMarkerSha256':H((root/'.owner').read_bytes()) if (root/'.owner').exists() else None,'removed':False})
report=R/'开发计划/验证记录/2026-11-01-round55-58-local-cleanup-first.json';assert not report.exists()
record={'status':'RUNNING','sourceCommit':commit,'proofSha256':H(proof.read_bytes()),'startedUtc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'ownershipGuards':guards,'activeOwnedConsumers':active,'roots':facts,'branchRemoval':[],'scope':'Only nine finished owned /tmp roots. Worktree changes fully archived; no cleanup of project main target, global caches, persistent SSH authorization/private key or intentional development ZIPs.'}
def save():report.write_text(json.dumps(record,ensure_ascii=False,indent=2)+'\n')
save()
try:
 for w,branch,reference in worktrees:
  subprocess.run(['git','worktree','remove','--force',str(w)],cwd=R,check=True,capture_output=True);next(x for x in facts if x['root']==str(w))['removed']=not w.exists();save()
 for root in roots:
  if not root.exists():continue
  shutil.rmtree(root);next(x for x in facts if x['root']==str(root))['removed']=not root.exists();save()
 for w,branch,reference in worktrees:
  result=subprocess.run(['git','branch','-d','--',branch],cwd=R,capture_output=True,text=True,check=True);record['branchRemoval'].append({'branch':branch,'exitCode':result.returncode,'stdout':result.stdout,'stderr':result.stderr});save()
 assert all(x['removed'] for x in facts)
 key=Path('/config/.local/share/future-classroom-test-access/win11-192.168.10.212');mode=oct(stat.S_IMODE(key.stat().st_mode));assert mode=='0o600'
 record['preserved']={'persistentPrivateKey':{'path':str(key),'mode':mode,'contentsRead':False},'mainTargetDebugExists':(R/'target/debug').exists(),'intentionalDevelopmentZipMetadata':[{'path':str(q),'bytes':q.stat().st_size} for q in [Path('/mnt/data/FutureClassroom-0.1.0-public-desktop-dev.zip'),Path('/mnt/data/FutureClassroom-0.1.0-continuous-accept-dev.zip'),Path('/mnt/data/FutureClassroom-0.1.0-voice-capability-dev.zip')]],'newActiveR59EvidencePreserved':E.exists(),'WindowsServiceAndAuthorization':'Unchanged per last actual read-only Windows audit; no Windows mutation or SSH test in this local cleanup.'}
 record['status']='PASS_ALL9_FINISHED_OWNED_ROOTS_AND3_BRANCHES_REMOVED';record['removedLogicalBytes']=sum(x['logicalBytes'] for x in facts);record['completedUtc']=datetime.datetime.now(datetime.timezone.utc).isoformat();save()
except BaseException as error:
 record['status']='FAIL_PARTIAL_LOCAL_CLEANUP';record['error']={'kind':type(error).__name__,'message':str(error)[:600]};save();raise
print(json.dumps({'status':record['status'],'removedRoots':sum(x['removed'] for x in facts),'logicalBytes':record.get('removedLogicalBytes'),'branches':len(record['branchRemoval']),'reportSha256':H(report.read_bytes())}),flush=True)
