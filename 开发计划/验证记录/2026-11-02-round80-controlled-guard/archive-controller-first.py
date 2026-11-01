#!/usr/bin/env python3
"""Finite, default-inert archive of root-frozen Rust guard component evidence."""
from pathlib import Path
import argparse,gzip,hashlib,io,json,os,stat,tarfile
MAIN=Path('/workspace/共享/Eliora/未来教室计划');R=Path('/tmp/fc-round80-controlled-repair')
D=MAIN/'开发计划/验证记录/2026-11-02-round80-controlled-guard'
MANIFEST_SHA='166f3b88f0bd7dd013c5d73fa71761aa9f9af0a85bdd231e017299515a39fde5'
BACKEND=MAIN/'开发计划/验证记录/2026-11-02-round79-msi-maintenance/evidence-originals-first.tar.gz'
MEMBER='archive-preparation-second/archive-second.py';SCANNER_SHA='da2906c450ea8c2ac7d18e358507023f519dbcc6faf1a9ac1abb1ae16af8c30f'
PROMOTE=['guard-native-runtime-first/report.json','guard-build-third/report.json','guard-clippy-second/report.json','guard-library-first/report.json','guard-library-first/export-check-first.json','guard-helper-first/report.json','guard-source-preservation-first/source-preservation-first.json','guard-candidate-sync-first.json','windows-readonly-guard-first.json']
def need(v,m):
 if not v:raise AssertionError(m)
def sha(b):return hashlib.sha256(b).hexdigest()
def encoded(j):return (json.dumps(j,ensure_ascii=False,indent=2)+'\n').encode()
def regular(p):
 need(p.is_relative_to(R) and p.resolve()==p,'EXACT_NO_ALIAS')
 for a in (p,*p.parents):
  if a==R or a.is_relative_to(R):need(not a.is_symlink(),'NO_LINK_ANCESTOR')
 need(not any(v in p.parts for v in ('target','target-first','deps','__pycache__','checkout-52d6392-first')),'NO_ACTIVE_W_OR_CACHE')
 need(stat.S_ISREG(p.lstat().st_mode) and p.stat().st_size<=16*1024*1024,'BOUNDED_REGULAR')
 return p

def main():
 a=argparse.ArgumentParser();a.add_argument('--execute',action='store_true');args=a.parse_args()
 if not args.execute:print(json.dumps({'status':'PREPARED_NOT_EXECUTED','Windows':False,'sourceDeletion':False}));return
 need(Path(__file__).resolve()==D/'archive-controller-first.py','EXACT_CONTROLLER')
 need({p.name for p in D.iterdir()}=={'archive-controller-first.py','source-manifest-first.json'},'FRESH_FINITE_DESTINATION')
 need((R/'.owner').read_bytes()==b'future-classroom-owned-r80-controlled-repair/v1\n','SOURCE_OWNER')
 mb=(D/'source-manifest-first.json').read_bytes();need(sha(mb)==MANIFEST_SHA,'FROZEN_MANIFEST');m=json.loads(mb);rows=m['originals'];excluded=m['excludedBinaryIdentities'];by={x['path']:x for x in rows+excluded}
 need(len(rows)==m['originalCount'] and sum(x['bytes'] for x in rows)==m['originalBytes'] and len(by)==len(rows)+len(excluded),'MANIFEST_CLOSURE')
 def original(row):
  b=regular(R/row['path']).read_bytes();need(len(b)==row['bytes'] and sha(b)==row['sha256'],'EXACT_ORIGINAL_'+row['path']);return b
 def report(rel):return json.loads(original(by[rel]))
 def enumerate_fixed():
  paths=[]
  def walk(p):
   for e in sorted(os.scandir(p),key=lambda e:e.name):
    need(not e.is_symlink(),'NO_FIXED_SCOPE_LINKS');q=Path(e.path)
    if e.is_dir(follow_symlinks=False):walk(q)
    else:paths.append(q.relative_to(R).as_posix())
  for rel in m['selectedDirectories']:need((R/rel).is_dir() and not (R/rel).is_symlink(),'EXACT_FIXED_DIRECTORY');walk(R/rel)
  paths+=m['selectedFiles'];need(len(paths)==len(set(paths)),'NO_DUPLICATE_FIXED_PATH');return sorted(paths)
 need(enumerate_fixed()==sorted(by),'EXACT_FIXED_SCOPE')
 with tarfile.open(BACKEND,'r:gz') as old:
  mem=old.getmember(MEMBER);need(mem.isfile() and mem.size==26272,'SCANNER_MEMBER');scanner=old.extractfile(mem).read()
 need(sha(scanner)==SCANNER_SHA,'EXACT_REVIEWED_SCANNER');ns={'__name__':'finite_guard_archive_scan'};exec(compile(scanner,'<frozen-r79-scanner>','exec'),ns)
 need(ns['TEXT_FORMS']==ns['TEXT_DERIVED_VIEWS']=={},'NO_CONTROL_EXCEPTIONS')
 stats={k:0 for k in ('textViews','utf16Views','cp936Views','clixmlDecodedViews','decodedBase64Views','jsonContainers','decodedJsonStringViews')};findings=[];jsonl_count=0
 def scan(b,label):
  nonlocal jsonl_count
  ns['credential_scan'](b,label,stats,findings)
  if label.endswith('.jsonl'):
   for i,line in enumerate(b.splitlines()):
    if not line.strip():continue
    need(i<250000,'JSONL_RECORD_BOUND');json.loads(line);jsonl_count+=1;ns['credential_scan'](line,label+'::jsonl/'+str(i+1),stats,findings)
  need(not findings,'CREDENTIAL_FINDING_PATH_KIND_ONLY_'+json.dumps(findings))
 # Identity-only producer paths under target/deps are never opened. The two fixed
 # downloaded/uploaded EXE copies are identity checked, never copied into the tar.
 for row in excluded:need(ns['binary_kind'](original(row))==row['kind'],'EXACT_BINARY_IDENTITY')
 compile_facts=[]
 for rel,expected in [('guard-build-first/report.json',101),('guard-build-second/report.json',0),('guard-build-third/report.json',0),('guard-clippy-first/report.json',101),('guard-clippy-second/report.json',0),('guard-library-first/report.json',0),('guard-helper-first/report.json',0)]:
  j=report(rel);need(j['actualExit']==expected and j['originalParentReaped'] and j['sourceUnchanged'] and j['sourcesBefore']==j['sourcesAfter'],'FIXED_COMPILER_FACT_'+rel)
  compile_facts.append({'path':rel,'status':j['status'],'actualExit':expected,'seconds':j['seconds'],'inputs':len(j['sourcesBefore']),'runtime':j['runtime'],'productionAdmission':j['productionAdmission']})
 source=report('guard-source-preservation-first/source-preservation-first.json');need(source['commit']==m['candidateCommit']=='d2dd16bb25ce5d9a8b7e61546a167a1dd61e5e96' and source['base']==m['parentCommit']=='b7e4c938ab8ff391e2ad6c1d681e074881118c0a' and source['actualCommitExit']==0 and source['gitBlobsMatch'] and source['worktreeClean'] and len(source['paths'])==10,'EXACT_CANDIDATE_COMMIT')
 for row in source['paths']:
  actual=by['guard-source-preservation-first/sources/'+row['path']];need(actual['bytes']==row['bytes'] and actual['sha256']==row['sha256'],'TEN_SOURCE_COPIES_MATCH_COMMIT_RECEIPT')
 sync=report('guard-candidate-sync-first.json');need(sync['actualExit']==0 and sync['originalParentReaped'] and sync['commit']==m['candidateCommit'] and sync['remoteReadback']['actualExit']==0 and sync['remoteReadback']['commit']==m['candidateCommit'],'ACTUAL_CANDIDATE_SYNC')
 revision=report('guard-revision-third/status.json');need(revision['status']=='NO_SOURCE_CHANGE_ALREADY_FIXED_BEFORE_FREEZE','DUPLICATE_PATCH_IS_NOT_COMPILER_FAILURE')
 export=report('guard-library-first/export-check-first.json');need(export['status']=='PASS_GNU_X86_UNDECORATED_TYPE1_EXPORT_ONLY' and export['objdumpActualExit']==0 and export['matchingExportRows']==['\t[   0] FcControlledInnerGuard'],'EXACT_UNDECORATED_EXPORT')
 runtime=report('guard-native-runtime-first/report.json');need(runtime['status']=='PASS_GUARD_COMPONENT_REGRESSION_NATIVE_TESTS_WIN11_ONLY' and runtime['expectedScopes']=={'controlled':6,'lifecycle':60} and runtime['cleanup']=='PASS_EXACT_DOWNLOADED_OWNED_SCRATCH_ONLY' and runtime['delete']['knownAbsent'] and runtime['ownedProcessReadback']['count']==0,'FINITE_WIN11_RUNTIME')
 need(len(runtime['native']['originalObjects'])==2 and all(x['Wait']==x['ExitCode']==0 and x['ExitKnown'] and x['CheckedClose'] for x in runtime['native']['originalObjects']),'ORIGINAL_PARENTS_RETURNED')
 need(runtime['native']['productionMsiAdapter']=='NOT_EXERCISED','NO_PRODUCTION_COUPLED_PROOF')
 for c in runtime['qualifiedCases']:
  need(c['outerTotals']['passed']==runtime['expectedScopes'][c['name']] and c['outerTotals']['failed']==c['outerTotals']['ignored']==0,'EXACT_PARENT_TOTALS')
 downloadroot=Path(runtime['windowsRoot']).name
 # Windows paths use backslashes even on this POSIX archive host.
 downloadroot=runtime['windowsRoot'].replace('\\','/').rsplit('/',1)[-1]
 need(len(runtime['inventory']['files'])==4,'FOUR_RUNTIME_DOWNLOADS')
 for row in runtime['inventory']['files']:
  rel='guard-native-runtime-first/download/'+downloadroot+'/'+row['path'];actual=by[rel];need(actual['bytes']==row['bytes'] and actual['sha256']==row['sha256'],'EXACT_DOWNLOADED_IDENTITY')
 for op in runtime['operations']:
  need(op['actualReturnCode']==0 and op['originalLocalReaped'] and not op['remoteOriginalReturnUnknown'] and not op['budgetExceeded'],'ORIGINAL_RUNTIME_TRANSPORT_RETURN')
  for key in ('stdout','stderr'):
   stream=op[key];rel=Path(stream['path']).relative_to(R).as_posix();row=by[rel];need(row['bytes']==stream['bytes'] and row['sha256']==stream['sha256'],'ORIGINAL_TRANSPORT_STREAM_IDENTITY')
 audit=report('windows-readonly-guard-first.json');need(audit['status']=='PASS' and audit['scratchDirectories']==audit['ownedProcesses']==audit['ownedTasks']==[] and audit['sshd']=='Running' and audit['persistentKeyMode']=='0o600' and audit['publicAuthorizationUnchanged'] and audit['projectRunUnchanged'],'FINAL_READONLY_AUDIT')
 facts={'schemaVersion':1,'status':'PASS_FINITE_RUST_GUARD_COMPONENT_BATCH_NOT_PRODUCTION_MSI_LINKAGE','candidateCommit':m['candidateCommit'],'parentCommit':m['parentCommit'],'candidateSync':sync,'compileFacts':compile_facts,'sourceFiles':10,'sourceCopiesMatchOriginalCommitReceipt':True,'libraryExport':export,'producerArtifactIdentitiesFromReportsOnly':m['producerArtifactIdentitiesFromReportsOnly'],'win11':{'runtimeReport':by['guard-native-runtime-first/report.json'],'scopes':runtime['expectedScopes'],'overlap':runtime['scopeOverlap'],'originalParents':runtime['native']['originalObjects'],'completeDownloadFiles':4,'ownedProcessesAfter':0,'scratchDeleted':runtime['delete']['knownAbsent'],'nativeMilliseconds':runtime['native']['elapsedMilliseconds'],'outerTransportSeconds':runtime['elapsedSeconds'],'audit':by['windows-readonly-guard-first.json']},'preservedHistory':['First compiler101: called unavailable Error::from_win32; retained with pre-fix source.','First strict Clippy101: unused diagnostic, retained with pre-fix source.','Original unexecuted preparation/NOT_RUN states remain unchanged.','Third revision rejected duplicate stale patch: NO_SOURCE_CHANGE_ALREADY_FIXED_BEFORE_FREEZE, not an additional compiler failure.'],'scope':'Private Rust Type1 guard, B/G bridge and zero-slot template components compiled, with pure parser and component runtime checks; normal helper compiled only. No real B/G/CA coupled MSI flow or public Setup route exercised.','productionMsiAdapter':'NOT_EXERCISED','windows7Runtime':'NOT_RUN','windows10Runtime':'NOT_RUN','actions':{'WindowsContacted':False,'Cargo':False,'worktreeOrTargetRead':False,'privateKeyContentRead':False,'sourceDeletion':False,'commitPush':False}}
 for row in rows:scan(original(row),'source:'+row['path'])
 scan(mb,'frozen-manifest');controller=Path(__file__).read_bytes();scan(controller,'archive-controller-source');fb=encoded(facts);scan(fb,'batch-facts')
 # Archive payload is created only after every selected text passes the scanner.
 archive=D/'evidence-originals-first.tar.gz'
 with archive.open('xb') as outer:
  with gzip.GzipFile(filename='',mode='wb',fileobj=outer,mtime=0,compresslevel=9) as zipped:
   with tarfile.open(fileobj=zipped,mode='w',format=tarfile.PAX_FORMAT) as tar:
    for row in rows:
     b=original(row);info=tarfile.TarInfo(row['path']);info.size=len(b);info.mode=0o600;info.uid=info.gid=info.mtime=0;info.uname=info.gname='';tar.addfile(info,io.BytesIO(b))
 with tarfile.open(archive,'r:gz') as tar:
  members=tar.getmembers();need([x.name for x in members]==[x['path'] for x in rows],'EXACT_TAR_MEMBER_SET')
  for member,row in zip(members,rows):
   need(member.isfile() and member.mode==0o600 and member.uid==member.gid==member.mtime==0 and member.uname==member.gname=='','DETERMINISTIC_REGULAR_TAR')
   b=tar.extractfile(member).read();need(b==original(row),'FULL_TAR_MEMBER_BYTE_REPLAY');scan(b,'stored-tar-gzip-decoded:'+row['path'])
 for rel in PROMOTE:
  b=original(by[rel]);p=D/rel;p.parent.mkdir(parents=True,exist_ok=True)
  with p.open('xb') as f:f.write(b)
  need(p.read_bytes()==b,'PROMOTED_BYTE_EXACT');scan(b,'promoted:'+rel)
 (D/'batch-facts-first.json').write_bytes(fb)
 cb=archive.read_bytes();index={'schemaVersion':1,'status':'PASS_FINITE_GUARD_COMPONENT_ORIGINALS_ARCHIVED','candidateCommit':m['candidateCommit'],'archive':{'path':archive.name,'bytes':len(cb),'sha256':sha(cb),'memberCount':len(rows)},'manifest':{'path':'source-manifest-first.json','bytes':len(mb),'sha256':MANIFEST_SHA},'facts':{'path':'batch-facts-first.json','bytes':len(fb),'sha256':sha(fb)},'originalCount':len(rows),'originalBytes':m['originalBytes'],'promoted':[by[x] for x in PROMOTE],'excludedBinaryIdentities':excluded,'producerArtifactsFromReportOnly':m['producerArtifactIdentitiesFromReportsOnly'],'allMembersReplayed':True,'sourceDeleted':False,'WindowsContacted':False,'worktreeOrTargetRead':False,'productionMsiAdapter':'NOT_EXERCISED'}
 ib=encoded(index);scan(ib,'archive-index');(D/'archive-index-first.json').write_bytes(ib)
 receipt={'schemaVersion':1,'status':'PASS_FULL_TAR_REPLAY_AND_BOUNDED_DECODED_CREDENTIAL_SCAN','candidateCommit':m['candidateCommit'],'archiveSha256':sha(cb),'archiveBytes':len(cb),'indexSha256':sha(ib),'originalCount':len(rows),'originalBytes':m['originalBytes'],'credentialScan':{'status':'PASS_NO_MATCHES','statistics':stats,'jsonlRecordsAlsoDecoded':jsonl_count,'patterns':list(ns['PATTERNS']),'findings':[],'coverage':'All selected text raw bytes, full stored-tar byte replay, nine promoted core reports, manifest/controller/facts/index. Strict UTF8/UTF16/CP936; CLIXML parsed text; JSON and JSONL strings; nested canonical generic/declared base64. Unknown controls rejected with no exceptions. Decoder depth12, JSON250000, decoded64MiB per original.','controlExceptions':[]},'scope':'Guard component build/export/helper and Win11 lifecycle60 includes controlled6. Real production B/G/CA coupled MSI NOT_EXERCISED; Win7/Win10 true runtime NOT_RUN. All original FAIL, pending NOT_RUN and duplicate patch NO_SOURCE_CHANGE retained.','sourceDeleted':False,'WindowsContacted':False,'Cargo':False,'worktreeOrTargetRead':False,'producerTargetBinariesRead':False,'privateKeyContentRead':False,'commitPush':False}
 rb=encoded(receipt);scan(rb,'scan-receipt');(D/'archive-verification-first.json').write_bytes(rb)
 need(enumerate_fixed()==sorted(by),'FINAL_FIXED_PATH_SET_UNCHANGED')
 for row in rows+excluded:original(row)
 expected={'source-manifest-first.json','archive-controller-first.py','batch-facts-first.json','evidence-originals-first.tar.gz','archive-index-first.json','archive-verification-first.json'}|set(PROMOTE)
 need({p.relative_to(D).as_posix() for p in D.rglob('*') if p.is_file()}==expected,'EXACT_FINITE_DESTINATION_FILE_SET')
 print(json.dumps({'status':'PASS','textMembers':len(rows),'originalBytes':m['originalBytes'],'binaryIdentitiesOnly':len(excluded),'producerArtifactIdentitiesOnly':len(m['producerArtifactIdentitiesFromReportsOnly']),'archiveBytes':len(cb),'archiveSha256':sha(cb),'indexSha256':sha(ib),'verificationSha256':sha(rb),'factsSha256':sha(fb),'destinationFiles':len(expected),'WindowsContacted':False,'sourceDeleted':False}))
if __name__=='__main__':main()
