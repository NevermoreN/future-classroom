"""Delete only two completed R69 owned local roots after exact committed proof."""
from pathlib import Path
import base64,datetime,gzip,hashlib,json,os,shutil,stat,subprocess
MAIN=Path('/workspace/共享/Eliora/未来教室计划')
ROOT=Path('/tmp/fc-round69-ci-evidence')
CONTROL=Path('/tmp/fc-round69-archive-controller')
ARCHIVE=MAIN/'开发计划/验证记录/2026-11-01-round69-ci'
META=MAIN/'开发计划/验证记录'
RECEIPT=META/'2026-11-01-round69-cleanup-first.json'
COMMIT='c5e4a8050a4ca4c31a0106c52e193e547d130fb2'
INDEX_SHA='ed374e6aea90508343c4c3a3d6c99e9e72882655301780d29242238e65c28efd'
def sha(d):return hashlib.sha256(d).hexdigest()
def git(*args,data=None):return subprocess.check_output(['git',*args],input=data,cwd=MAIN)
def identity(p):
 assert p.is_file() and not p.is_symlink()
 d=p.read_bytes();return dict(path=str(p),bytes=len(d),sha256=sha(d))
def main():
 assert not RECEIPT.exists() and git('rev-parse','HEAD').decode().strip()==COMMIT
 assert not git('diff','--cached','--name-only','-z')
 for root,marker in [(ROOT,b'future-classroom-owned-r69-ci-evidence/v1\n'),(CONTROL,b'future-classroom-owned-r69-archive-controller/v1\n')]:
  assert root.is_dir() and not root.is_symlink() and (root/'.owner').read_bytes()==marker
 proof_paths={k:META/('2026-11-01-round69-'+k+'-bytes-first.json') for k in ('staged','committed')}
 proofs={k:json.loads(p.read_bytes()) for k,p in proof_paths.items()}
 for kind,value in proofs.items():
  assert value['status']=='PASS_EXACT_'+kind.upper()+'_BYTES' and value['fileCount']==515 and value['archiveFileCount']==509
  assert proof_paths[kind].read_bytes()==(CONTROL/(kind+'-bytes-first.json')).read_bytes()
 assert proofs['committed']['verifiedCommit']==COMMIT
 assert proofs['staged']['files']==proofs['committed']['files']
 # Re-read every original first-commit Git blob, including every ignored raw log.
 rows=proofs['committed']['files'];payload=git('cat-file','--batch',data=''.join(row['gitBlob']+'\n' for row in rows).encode());cursor=0
 for row in rows:
  end=payload.index(b'\n',cursor);blob,kind,size=payload[cursor:end].split();size=int(size);cursor=end+1;data=payload[cursor:cursor+size];cursor+=size
  assert payload[cursor:cursor+1]==b'\n';cursor+=1
  assert blob.decode()==row['gitBlob'] and kind==b'blob' and size==row['bytes'] and sha(data)==row['sha256']
  assert data==(MAIN/row['path']).read_bytes()
 assert cursor==len(payload)
 index_path=ARCHIVE/'archive-index-first.json';assert sha(index_path.read_bytes())==INDEX_SHA
 index=json.loads(index_path.read_bytes());assert index['originalFiles']==len(index['files'])==508 and len(index['excludedCompiledPayloads'])==24
 assert len({p for p in ARCHIVE.rglob('*') if p.is_file()})==509
 source_proof=[];excluded=[];gzip_count=0
 for row in index['files']:
  source=ROOT/row['path'];stored=ARCHIVE/row.get('archivePath',row['path']);a=source.read_bytes();b=stored.read_bytes()
  assert len(a)==row['bytes'] and sha(a)==row['sha256']
  assert len(b)==row.get('storedBytes',row['bytes']) and sha(b)==row.get('storedSha256',row['sha256'])
  if row.get('storage')=='gzip_original_bytes':b=gzip.decompress(b);gzip_count+=1
  assert a==b
  source_proof.append(dict(path=row['path'],bytes=len(a),sha256=sha(a),archivePath=row.get('archivePath',row['path'])))
 for row in index['excludedCompiledPayloads']:
  data=(ROOT/row['path']).read_bytes();assert len(data)==row['bytes'] and sha(data)==row['sha256']
  excluded.append(dict(path=row['path'],bytes=len(data),sha256=sha(data),reason='Original compiled EXE or downloaded CI ZIP; exact identity retained, deliberately not committed or executed during cleanup.'))
 assert gzip_count==22
 assert {p.relative_to(ROOT).as_posix() for p in ROOT.rglob('*') if p.is_file()}=={row['path'] for row in index['files']+index['excludedCompiledPayloads']}
 prefix='2026-11-01-round69-archive-operation-first';outside_originals=[]
 for local,suffix in [('operation-first.json','.json'),('operation-first.stdout.log','.stdout.log'),('operation-first.stderr.log','.stderr.log')]:
  original=CONTROL/local;committed=META/(prefix+suffix);data=original.read_bytes();assert data==committed.read_bytes()
  row=next(v for v in rows if v['path']==committed.relative_to(MAIN).as_posix());assert len(data)==row['bytes'] and sha(data)==row['sha256']
  outside_originals.append(identity(original))
 expected_control={'.owner','operation-first.json','operation-first.stdout.log','operation-first.stderr.log','staging-paths-first.nul',
  'stage-first.json','stage-first.stdout.log','stage-first.stderr.log','diff-check-first.stdout.log','diff-check-first.stderr.log',
  'commit-first.json','commit-first.stdout.log','commit-first.stderr.log','staged-bytes-first.json','committed-bytes-first.json'}
 assert {p.relative_to(CONTROL).as_posix() for p in CONTROL.rglob('*') if p.is_file()}==expected_control
 assert (CONTROL/'staging-paths-first.nul').read_bytes()==b''.join(row['path'].encode()+b'\0' for row in rows)
 # Preserve the small local staging/commit controller originals exactly inside
 # this receipt; pathspec is derivable from the already retained515 proof rows.
 transient=[]
 for name in sorted(expected_control-{'operation-first.json','operation-first.stdout.log','operation-first.stderr.log','staged-bytes-first.json','committed-bytes-first.json','staging-paths-first.nul','.owner'}):
  p=CONTROL/name;data=p.read_bytes();transient.append(dict(**identity(p),base64=base64.b64encode(data).decode()))
 assert json.loads((CONTROL/'stage-first.json').read_bytes())['status']=='PASS'
 assert json.loads((CONTROL/'commit-first.json').read_bytes())['commit']==COMMIT
 assert not (CONTROL/'diff-check-first.stdout.log').read_bytes() and not (CONTROL/'diff-check-first.stderr.log').read_bytes()
 # Refuse any related live process by argv, cwd or open-fd metadata. No process
 # is killed and no private-key contents or process environments are read.
 active=[];needles=[str(ROOT).encode(),str(CONTROL).encode()]
 for proc in Path('/proc').iterdir():
  if not proc.name.isdigit() or int(proc.name) in (os.getpid(),os.getppid()):continue
  try:
   args=(proc/'cmdline').read_bytes().split(b'\0');reasons=[]
   if any(n in b' '.join(args) for n in needles):reasons.append('argv')
   try:cwd=os.fsencode(os.readlink(proc/'cwd'))
   except (FileNotFoundError,PermissionError,ProcessLookupError):cwd=b''
   if any(cwd==n or cwd.startswith(n+b'/') for n in needles):reasons.append('cwd')
   try:
    for fd in (proc/'fd').iterdir():
     try:target=os.fsencode(os.readlink(fd))
     except (FileNotFoundError,PermissionError,ProcessLookupError):continue
     if any(target==n or target.startswith(n+b'/') for n in needles):reasons.append('open-fd');break
   except (FileNotFoundError,PermissionError,ProcessLookupError):pass
   if reasons:active.append(dict(pid=int(proc.name),executable=Path(os.fsdecode(args[0])).name if args else '',reasons=reasons))
  except (FileNotFoundError,PermissionError,ProcessLookupError):continue
 assert not active,active
 removed=[]
 for root in (ROOT,CONTROL):
  count=size=directories=0
  for p in root.rglob('*'):
   mode=p.lstat().st_mode;assert not stat.S_ISLNK(mode),'symlink in owned root'
   if stat.S_ISREG(mode):count+=1;size+=p.stat().st_size
   elif stat.S_ISDIR(mode):directories+=1
   else:raise AssertionError('nonregular owned member')
  removed.append(dict(path=str(root),regularFiles=count,logicalBytes=size,directories=directories))
 assert removed[0]['regularFiles']==532 and removed[1]['regularFiles']==15
 for root in (ROOT,CONTROL):shutil.rmtree(root);assert not root.exists()
 report=dict(schemaVersion=1,status='PASS_R69_COMPLETED_OWNED_ROOTS_REMOVED_AFTER_EXACT_COMMIT_PROOF',recordedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),
  verifiedCommit=COMMIT,archiveIndexSha256=INDEX_SHA,committedBytesProof=identity(proof_paths['committed']),stagedBytesProof=identity(proof_paths['staged']),
  everyOriginalGitBlobReverified=515,sourceOriginalsVerified=source_proof,excludedCompiledAndZipVerified=excluded,gzipDecodedExactFiles=gzip_count,
  outsideArchivedOriginalsVerified=outside_originals,localTransientControllerOriginals=transient,
  pathspecQualification='Exact sorted515-path NUL vector matched retained committed proof; temporary pathspec deleted with owned controller, derivable without loss from proof rows.',
  removed=removed,activeOwnedProcessesBeforeRemoval=active,physicalDiskBytesFreed='NOT_MEASURED',helperSha256=sha(Path(__file__).read_bytes()),
  scope='Only completed R69 local CI evidence and archive-controller roots removed. Main committed originals/proofs/Win7 failure retained. No Windows mutation, process termination, R70/R71/other-root/cache/SSH-service/publickey/privatekey action. No failed Win7 EXE approval or execution.')
 with RECEIPT.open('x') as out:json.dump(report,out,ensure_ascii=False,indent=2);out.write('\n')
 print(json.dumps(dict(status=report['status'],removed=removed,receiptSha256=sha(RECEIPT.read_bytes()))))
if __name__=='__main__':main()
