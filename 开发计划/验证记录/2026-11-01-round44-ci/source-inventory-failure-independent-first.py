from pathlib import Path,PurePosixPath
import datetime,hashlib,json,re,subprocess,zipfile,zlib

ROOT=Path('/workspace/共享/Eliora/未来教室计划');BASE=Path('/tmp/fc-round44-ci-evidence')
OUT=BASE/'source-inventory-failure-independent-first.json'
TREE_OUT=BASE/'source-git-tree-metadata-first.json'
SOURCE='e0d97689d6848d971bb9f751cc5f014c6a98882b'
sha=lambda b:hashlib.sha256(b).hexdigest()
load=lambda b:json.loads(b.decode('utf-8-sig'))
assert not OUT.exists() and not TREE_OUT.exists()
r={'schemaVersion':1,'status':'RUNNING','sourceSha':SOURCE,'scope':'Read-only independent original R44 two CI early failures and exact Git tree metadata size diagnosis. No workflow dispatch/rerun, source mutation, Cargo, Windows or executable run. A successful artifact check does not mark either failed CI successful.','checks':[],'artifacts':[]}
def check(name,ok):
 r['checks'].append({'name':name,'status':'PASS' if ok else 'FAIL'})
 assert ok,name

def fact(path):
 data=path.read_bytes();return {'path':str(path),'bytes':len(data),'sha256':sha(data)}
try:
 for label,run,job,artifact,expected_bytes,expected_sha in [
  ('modern',41247806786,124674777677,13059558491,58920,'4c0055131d38f4426f0be0689e54987411276ed27fcd5c85b279484aaaec7d7a'),
  ('win7',41247836092,124674867418,13060600090,58994,'12de62c0fa852df729fa52594d795c75e89a3b375713b81549f37e63671c00b5')]:
  path=BASE/(label+'-original-first.zip');raw=path.read_bytes()
  check(label+' exact original GitHub producer ZIP bytes SHA',len(raw)==expected_bytes and sha(raw)==expected_sha)
  dest=BASE/(label+'-artifact-raw');check(label+' refuse overwrite raw originals',not dest.exists());dest.mkdir()
  members=[]
  with zipfile.ZipFile(path) as z:
   infos=z.infolist();names=[x.filename for x in infos]
   check(label+' exact18 unique ordinary ZIP members CRC',len(names)==len(set(names))==18 and not any(x.is_dir() for x in infos) and z.testzip() is None)
   producer=load(z.read('artifact-sha256.json'));ledger={x['path']:x for x in producer}
   check(label+' exact17 producer ledger complete set',len(ledger)==len(producer)==17 and set(names)==set(ledger)|{'artifact-sha256.json'})
   for info in infos:
    name=info.filename;pp=PurePosixPath(name)
    check(label+' member safe regular '+name,not pp.is_absolute() and '..' not in pp.parts and '\\' not in name and pp.as_posix()==name and ((info.external_attr>>16)&0o170000)!=0o120000)
    data=z.read(info);digest=sha(data)
    check(label+' member CRC bytes '+name,len(data)==info.file_size and zlib.crc32(data)&0xffffffff==info.CRC)
    if name in ledger:check(label+' original producer size SHA '+name,len(data)==ledger[name]['bytes'] and digest==ledger[name]['sha256'])
    target=dest.joinpath(*pp.parts);target.parent.mkdir(exist_ok=True,parents=True)
    with target.open('xb') as f:f.write(data)
    members.append({'path':name,'bytes':len(data),'sha256':digest,'crc32':f'{info.CRC:08x}','producerLedgerMatched':name in ledger})
  result=load((dest/'result.json').read_bytes());failure=load((dest/'failure.json').read_bytes());cleanup=load((dest/'cleanup.json').read_bytes());start=load((dest/'run-start.json').read_bytes())
  check(label+' source exact original CI binding',result['source_sha']==result['dispatch_source_sha']==SOURCE and start['expected_source_sha']==start['dispatch_source_sha']==start['workflow_sha']==SOURCE)
  check(label+' original CI remains failed before compiler admission',result['status']=='FAILED_OR_INCOMPLETE' and result['executable_count']==0 and result['input_manifest_sha256'] is None and failure['message']=='Source inventory size/reparse bound exceeded.')
  commands=result['commands']
  check(label+' only3 source commands no compiler/lint/runtime command',[x['name'] for x in commands]==['source-sha','source-clean-before','source-input-paths'])
  check(label+'3 source processes actualexit0 no timeout/truncation',all(x['exit_code']==0 and x['passed'] is True and x['parent_exited'] is True and x['timed_out_or_pipe_stalled'] is False and x['log_truncated'] is False for x in commands))
  check(label+' exact source stdout and clean checkout',(dest/'logs/source-sha.stdout.log').read_text(encoding='utf-8-sig').strip()==SOURCE and not (dest/'logs/source-clean-before.stdout.log').read_bytes())
  check(label+' cleanup all four original roots removed',cleanup['status']=='OWNED_BUILD_ROOTS_REMOVED' and {x['root'] for x in cleanup['roots']}=={'target','scratch','cargo','rustup'} and all(x['removed'] is True for x in cleanup['roots']))
  job_log=BASE/(label+'-job-logs-decoded-first.log');text=job_log.read_text(encoding='utf-8-sig')
  check(label+' actual decoded job log same error source and upload success',SOURCE in text and 'Compilation evidence failed: Source inventory size/reparse bound exceeded.' in text and 'Process completed with exit code 1.' in text and 'successfully uploaded! Final size is '+str(expected_bytes)+' bytes. Artifact ID is '+str(artifact) in text)
  r['artifacts'].append({'baseline':label,'runId':run,'jobId':job,'artifactId':artifact,'zip':fact(path),'jobDecodedLog':fact(job_log),'pluginLogOriginal':fact(BASE/(label+'-job-logs-plugin-first.json')),'pluginArtifactMetadata':fact(BASE/(label+'-artifacts-first.json')),'memberCount':len(infos),'producerLedgerCount':len(producer),'members':members,'originalFailure':failure,'originalResult':result,'originalCleanup':cleanup,'originalStart':start,'commandOriginalLogBindings':[{'command':x,'stdout':fact(dest/x['stdout']),'stderr':fact(dest/x['stderr'])} for x in commands]})
 rawtree=subprocess.check_output(['git','ls-tree','-rlz',SOURCE],cwd=ROOT);rows=[]
 for item in rawtree.split(b'\0'):
  if not item:continue
  meta,path=item.split(b'\t',1);mode,kind,oid,size=meta.decode().split()
  rows.append({'path':path.decode('utf-8'),'gitMode':mode,'gitObjectType':kind,'gitBlobObjectId':oid,'gitBlobBytes':int(size)})
 paths={x['path'] for x in rows};check('exact6940 unique Git source metadata rows',len(rows)==len(paths)==6940)
 check('all exact Git tree objects are regular files no repository symlink/submodule',all(x['gitMode'] in ['100644','100755'] and x['gitObjectType']=='blob' for x in rows))
 for label in ['modern','win7']:
  data=(BASE/(label+'-artifact-raw/logs/source-input-paths.stdout.log')).read_bytes()
  logged=[x.decode('utf-8') for x in data.split(b'\0') if x]
  check(label+' exact full original CI ls-files set matches pinned Git tree',len(logged)==len(set(logged))==len(rows) and set(logged)==paths)
 prefix='开发计划/验证记录/'
 archive=[x for x in rows if x['path'].startswith(prefix)];other=[x for x in rows if not x['path'].startswith(prefix)]
 total=sum(x['gitBlobBytes'] for x in rows);arch_total=sum(x['gitBlobBytes'] for x in archive);other_total=sum(x['gitBlobBytes'] for x in other);max_file=max(x['gitBlobBytes'] for x in rows)
 check('original raw Git bytes already exceed256MiB',total>256*1024*1024)
 check('original count and maximum raw file below independent original bounds',len(rows)<10000 and max_file<64*1024*1024)
 check('remaining550 tracked files fit unchanged64MiB256MiB domain bounds',len(other)==550 and other_total==20723460 and max(x['gitBlobBytes'] for x in other)<64*1024*1024 and other_total<256*1024*1024)
 tree_id=subprocess.check_output(['git','rev-parse',SOURCE+'^{tree}'],cwd=ROOT).decode().strip()
 metadata={'schemaVersion':1,'sourceSha':SOURCE,'gitTreeObjectId':tree_id,'objectFormat':'sha1','scope':'Full original Git tree/blob metadata only. gitBlobBytes and SHA1 object IDs are from the pinned Git object database, not Windows checkout file SHA256. This file SHA256 binds this metadata document only. No complete archived checkout-content SHA256 claim.','rows':rows}
 with TREE_OUT.open('x') as f:f.write(json.dumps(metadata,ensure_ascii=False,indent=2)+'\n')
 pinned=subprocess.check_output(['git','show',SOURCE+':scripts/windows-msvc-ci.ps1'],cwd=ROOT)
 pinned_path=BASE/'windows-msvc-ci.pinned-source-first.ps1'
 with pinned_path.open('xb') as f:f.write(pinned)
 source_text=pinned.decode()
 check('pinned Hash-Inputs exact original source bounds',"$Paths.Count -gt 10000" in source_text and "$file.Length -gt 64MB -or $total -gt 256MB" in source_text and "ReparsePoint) -ne 0) { throw 'Source inventory size/reparse bound exceeded.'" in source_text)
 r.update(status='PASS_ORIGINAL_FAILURE_DIAGNOSIS_BOTH_CI_REMAIN_FAILED',gitTreeMetadata=fact(TREE_OUT),pinnedCiSource=fact(pinned_path),sourceFacts={'files':len(rows),'gitBlobTotalBytes':total,'originalTotalBoundBytes':256*1024*1024,'rawGitBytesOverBound':total-256*1024*1024,'maximumGitBlobBytes':max_file,'originalPerFileBoundBytes':64*1024*1024,'archivePrefix':prefix,'archiveFiles':len(archive),'archiveGitBlobBytes':arch_total,'archiveShareOfRawGitBytes':arch_total/total,'outsideArchiveFiles':len(other),'outsideArchiveGitBlobBytes':other_total,'outsideArchiveMaximumGitBlobBytes':max(x['gitBlobBytes'] for x in other),'largestOriginalGitBlobs':sorted(rows,key=lambda x:x['gitBlobBytes'],reverse=True)[:12]},
 diagnosis={'observedFailure':'Both actual CI runs fail in Hash-Inputs before toolchain/compiler steps, after three successful source commands; same generic size/reparse exception. No Rust or Clippy compilation result exists in either run.','independentlyEstablishedCause':'Pinned full Git blob byte inventory is 286613906B, already18178450B over unchanged256MiB; growing preserved verification archives account for265890446B. The all-tracked-source size policy no longer admits this repository snapshot.','notRecorded':'Failure records omit triggering relative path, current file Length, cumulative total and FileAttributes. Cannot reconstruct first triggering Windows-sorted path, exact Windows checkout total or categorically exclude an earlier reparse/other filesystem condition from these original logs. Git regular modes and6.14MB raw maximum are repository metadata, not a fresh Windows attribute audit.','checkoutCaveat':'Raw Git sizes are not labeled actual Windows checkout sizes. Archived -text rules and earlier exact CRLF proofs are retained; no loose normalization or fabricated whole-checkout SHA ledger is used.'},
 proposedDesign={'status':'READ_ONLY_PROPOSAL_NOT_IMPLEMENTED','boundsUnchanged':{'perContentHashedFileBytes':64*1024*1024,'hashedDomainTotalBytes':256*1024*1024,'fileCount':10000,'jobs':2,'stageBudgetSeconds':900},'fullGitMetadataDomain':'Keep the complete pinned commit/tree and every tracked path, mode, blob object ID and raw Git blob size as a separate bounded metadata ledger. Hash that ledger document; never label its object IDs or metadata digest raw checkout SHA256. Preserve complete original archive INDEX/ledger proofs separately.','contentHashedDomain':'Declare a versioned conservative tracked-non-evidence domain: all tracked files outside literal normalized prefix 开发计划/验证记录/. Current pinned tree has550 such files/20723460 raw Git bytes. This retains Rust/crates/tests/build.rs/Cargo/lock/config/workflows/scripts/frontend/packaging/licenses/current plans/README instead of only assuming410 Rust files exhaust compiler input. Real Windows files are still read under original64/256/reparse checks before AND after compile; actual domain totals/SHA computed on that checkout.','partitionProof':'Require unique complete full-tree metadata set equals the disjoint union of declared content-hashed and archive-metadata domains, no missing/unclassified path, no rooted/escape/reparse aliases. Continue exact expected commit/workflow binding and full Git clean-before/after. Keep full6940 rows bounded; do not silently relax10000.','buildClosure':'Review and enforce that Cargo/build.rs/include_bytes/include_str/packaging verification cannot consume excluded archived files as build input. If any archive path is referenced as build/test input, add it explicitly to the content-hashed domain with existing limits or fail. All source/attribute/build configuration that defines selection must itself be included.','schemaAndClaims':'Emit explicit schema/version/domain labels for before/after content SHA256 and full-Git-object metadata. Result must say SHA256 of declared non-evidence checkout domain plus full historical Git-object metadata, NOT SHA256-read every Git checkout file. Record actual first failing path/length/total/reparse bool in any later bounded failure, avoiding signed URLs or environment dumps.','negativeChecks':['Unclassified or alias path rejected','A changed compiled/config/script/Web input changes before/after SHA or rejects','Excluded archive metadata change changes full-tree metadata identity but is not claimed raw SHA validation','A referenced archived file cannot silently enter compile without content-domain inclusion','File64MiB+1/total256MiB+1/real reparse remain rejected','All previous jobs2/900s/source-clean/exact-source and cleanup invariants retained']},
 limitations=['This is diagnosis and design only; no helper/source/workflow fix applied and neither failed CI rerun.','No Windows executable, PE artifact or runtime evidence exists in these two early-failure artifacts.','Original failure/no input manifest remain preserved, no extrapolation from R42/R41 successes.'],helper=fact(Path(__file__)))
except BaseException as error:r.update(status='FAIL_INDEPENDENT_DIAGNOSIS',errorKind=type(error).__name__,failedCheck=str(error) if isinstance(error,AssertionError) else 'Original evidence invariant or shape failed')
with OUT.open('x') as f:f.write(json.dumps(r,ensure_ascii=False,indent=2)+'\n')
print(json.dumps({'status':r['status'],'checks':len(r['checks']),'sourceFacts':r.get('sourceFacts'),'report':fact(OUT)},ensure_ascii=False))
raise SystemExit(0 if r['status']=='PASS_ORIGINAL_FAILURE_DIAGNOSIS_BOTH_CI_REMAIN_FAILED' else 1)
