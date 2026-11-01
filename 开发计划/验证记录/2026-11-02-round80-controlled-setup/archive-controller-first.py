#!/usr/bin/env python3
"""Explicit finite setup evidence archive; default inert. Never Windows/W/target/cleanup."""
from pathlib import Path
import argparse,gzip,hashlib,io,json,os,stat,tarfile
R=Path('/tmp/fc-round80-controlled-repair');M=Path('/workspace/共享/Eliora/未来教室计划');D=M/'开发计划/验证记录/2026-11-02-round80-controlled-setup'
MANIFEST_SHA='9488aa02aab3311914124438ff460a952b242f1681a1bcb5f432973a4ceebffd'
BACKEND=M/'开发计划/验证记录/2026-11-02-round79-msi-maintenance/evidence-originals-first.tar.gz';MEMBER='archive-preparation-second/archive-second.py';BACKEND_SHA='da2906c450ea8c2ac7d18e358507023f519dbcc6faf1a9ac1abb1ae16af8c30f'
PROMOTE=['setup-native-runtime-first/report.json','setup-replay-native-runtime-first/report.json','controlled-product-producer-first/evidence.json','controlled-product-package-first/report.json','controlled-product-runtime-first/report.json','controlled-product-failure-review-first/review-first.json','setup-replay-source-preservation-first/source-preservation-first.json','setup-replay-candidate-sync-first.json','guard-setup-build-second/report.json','guard-setup-clippy-first/report.json','guard-setup-replay-build-first/report.json','guard-setup-replay-clippy-first/report.json','root-controlled-product-runtime-operation-first.json','root-setup-native-runtime-operation-first.json','root-setup-replay-native-runtime-operation-first.json']
def need(v,m):
 if not v:raise AssertionError(m)
def sha(b):return hashlib.sha256(b).hexdigest()
def raw(j):return (json.dumps(j,ensure_ascii=False,indent=2)+'\n').encode()
def regular(p):
 need(p.is_absolute() and p.is_relative_to(R) and p.resolve()==p,'EXACT_NO_ALIAS_ORIGINAL')
 for q in (p,*p.parents):
  if q==R or q.is_relative_to(R):need(not q.is_symlink(),'NO_SYMLINK_ANCESTOR')
 need(stat.S_ISREG(p.lstat().st_mode),'REGULAR_ORIGINAL');need(not any(v in p.parts for v in ('target','target-first','node_modules','deps','checkout-52d6392-first','__pycache__')),'NO_WORKTREE_TARGET_CACHE')
 return p

def main():
 a=argparse.ArgumentParser();a.add_argument('--execute',action='store_true');args=a.parse_args()
 if not args.execute:print(json.dumps({'status':'PREPARED_NOT_EXECUTED','WindowsContacted':False,'sourceDeletion':False}));return
 need(Path(__file__).resolve()==D/'archive-controller-first.py','EXACT_CONTROLLER');need({x.name for x in D.iterdir()}=={'archive-controller-first.py','source-manifest-first.json','text-forms-first.json','archive-selector-first.py'},'NEW_FINITE_DESTINATION')
 need((R/'.owner').read_bytes()==b'future-classroom-owned-r80-controlled-repair/v1\n','R80_OWNER')
 mb=(D/'source-manifest-first.json').read_bytes();need(sha(mb)==MANIFEST_SHA,'FROZEN_MANIFEST');m=json.loads(mb);rows=m['originals'];excluded=m['excludedBinaryIdentities'];artifacts=m['producerArtifactsIdentityOnly'];by={x['path']:x for x in rows+excluded+artifacts}
 need(len(rows)==m['originalCount']==376 and sum(x['bytes'] for x in rows)==m['originalBytes']==13278445 and len(by)==len(rows)+len(excluded)+len(artifacts),'FROZEN_CLOSURE')
 def original(row):
  b=regular(R/row['path']).read_bytes();need(len(b)==row['bytes'] and sha(b)==row['sha256'],'EXACT_SOURCE_BYTES_'+row['path']);return b
 def report(rel):return json.loads(original(by[rel]))
 # Re-enumerate only the explicit allowed scope using the frozen local selector.
 # It has no top-level execution when imported, and never sees active root trees.
 selector=D/'archive-selector-first.py';selector_bytes=selector.read_bytes();need(sha(selector_bytes)=='01e3d58a7b554bee18ba374da6135a8c0a66b2479eda46478492b9c1ed31f93d','FROZEN_EXPLICIT_SELECTOR');selector_ns={'__name__':'finite_selector_only'};exec(compile(selector_bytes,'<explicit-finite-selector>','exec'),selector_ns)
 expected,links,pruned=selector_ns['collect']();need(expected==sorted(by) and links==m['symlinksNotFollowed'] and pruned==m['excludedDerivedPackageTrees'],'EXACT_SELECTED_MEMBERSHIP')
 with tarfile.open(BACKEND,'r:gz') as t:
  member=t.getmember(MEMBER);need(member.isfile() and member.size==26272,'EXACT_SCANNER_MEMBER');scanner=t.extractfile(member).read()
 need(sha(scanner)==BACKEND_SHA,'EXACT_REVIEWED_SCANNER');ns={'__name__':'frozen_setup_archive_scanner'};exec(compile(scanner,'<frozen-r79-scanner>','exec'),ns)
 form_bytes=(D/'text-forms-first.json').read_bytes();forms=json.loads(form_bytes);need(forms['status']=='EXACT_THREE_PATH_BYTES_SHA_BOUND_STRUCTURAL_TEXT_FORMS' and len(forms['files'])==3,'THREE_EXACT_STRUCTURAL_TEXT_FORMS')
 need(len({x['path'] for x in forms['files']})==3 and {x['path'] for x in forms['files']}=={x['path'] for x in m['controlDiagnostics']},'NO_ADDED_CONTROL_PATH')
 for f in forms['files']:
  row=by[f['path']];need(f['bytes']==row['bytes'] and f['sha256']==row['sha256'],'EXACT_CONTROL_FORM_IDENTITY')
 ns['TEXT_FORMS']={x['path']:x for x in forms['files']};need(ns['TEXT_DERIVED_VIEWS']=={},'NO_DERIVED_OR_GENERIC_CONTROL_EXCEPTIONS')
 stats={k:0 for k in ('textViews','utf16Views','cp936Views','clixmlDecodedViews','decodedBase64Views','jsonContainers','decodedJsonStringViews')};findings=[];jsonl_count=0
 def scan(b,label):
  nonlocal jsonl_count
  ns['credential_scan'](b,label,stats,findings)
  if label.endswith('.jsonl'):
   for i,line in enumerate(b.splitlines()):
    if not line.strip():continue
    need(i<250000,'JSONL_RECORD_BOUND');json.loads(line);jsonl_count+=1;ns['credential_scan'](line,label+'::jsonl/'+str(i+1),stats,findings)
  need(not findings,'CREDENTIAL_FINDING_PATH_AND_KIND_ONLY_'+json.dumps(findings))
 for row in excluded+artifacts:original(row)
 source=report('setup-replay-source-preservation-first/source-preservation-first.json');need(source['candidate']==m['candidateCommit']=='7c135c3dcb2dd4e78f70f39b97fb53746454fec2' and source['sourceBase']=='d2dd16bb25ce5d9a8b7e61546a167a1dd61e5e96' and len(source['sourceFiles'])==12 and source['actualCommitExit']==0,'FIXED_LATEST_CANDIDATE')
 for f in source['sourceFiles']:
  row=by['setup-replay-source-preservation-first/sources/'+f['path']];need(row['bytes']==f['bytes'] and row['sha256']==f['sha256'],'TWELVE_SOURCE_IDENTITIES')
 need(by['setup-replay-source-preservation-first/complete.patch']['sha256']==source['completePatchSha256'],'COMPLETE_PATCH_IDENTITY')
 sync=report('setup-replay-candidate-sync-first.json');need(sync['actualExit']==0 and sync['originalParentReaped'] and sync['commit']==m['candidateCommit'] and sync['remoteReadback']['actualExit']==0 and sync['remoteReadback']['commit']==m['candidateCommit'],'ACTUAL_CANDIDATE_REMOTE_SYNC')
 regressions=[]
 for rel,total in [('setup-native-runtime-first/report.json',65),('setup-replay-native-runtime-first/report.json',66)]:
  j=report(rel);need(j['expectedScopes']=={'controlled':6,'lifecycle':total} and j['cleanup']=='PASS_EXACT_DOWNLOADED_OWNED_SCRATCH_ONLY' and j['delete']['knownAbsent'] and j['ownedProcessReadback']['count']==0,'FINITE_COMPONENT_REGRESSION')
  regressions.append({'report':by[rel],'scopes':j['expectedScopes'],'overlap':j['scopeOverlap'],'authority':j['authority'],'cleanup':j['cleanup'],'jointProductRetest':'NOT_RUN'})
 failed=report('controlled-product-runtime-first/report.json');p=failed['phase'];need(failed['status']=='FAIL_OR_UNKNOWN_RETAIN_ORIGINALS' and failed['runtimeActualExit']==0 and failed['downloadVerified'] and len(failed['inventory']['files'])==30 and [x['client']['exitCode'] for x in p['phases']]==[0,0,1603] and not p['finallyCoordination']['pendingKnownAbsent'],'ORIGINAL_THREE_STAGE_FAIL_AND_RETAINED_PENDING')
 outer=report('root-controlled-product-runtime-operation-first.json');need(outer['actualExit']==1 and outer['originalParentReaped'],'ORIGINAL_FAIL_OUTER1')
 firstbuild=report('guard-setup-build-first/report.json');need(firstbuild['actualExit']==101,'FIRST_BUILD_FAIL_RETAINED')
 producer=report('controlled-product-producer-first/evidence.json');need(producer['status']=='DEV_FROZEN_NORMAL_BUILD_ONLY' and all(x['actual_exit']==0 for x in producer['commands']),'NORMAL_GNU_DEV_PRODUCER')
 package=report('controlled-product-package-first/report.json');need(package['status']=='PASS' and package['development_fixture_only'] and package['production_integration']=='NOT_EXERCISED' and package['payload_extraction']=='EXACT_BYTES_MATCH','STATIC_DEV_PACKAGE_ONLY')
 facts={'status':'FINITE_CONTROLLED_SETUP_EVIDENCE_ARCHIVE_NOT_NEW_RUNTIME','latestCandidate':m['candidateCommit'],'sourceBase':source['sourceBase'],'parent':source['parent'],'preservedSourceFiles':12,'candidateSync':by['setup-replay-candidate-sync-first.json'],'componentRegressions':regressions,'normalProducer':{'report':by['controlled-product-producer-first/evidence.json'],'sourceHead':producer['source_head'],'status':producer['status']},'staticPackage':{'report':by['controlled-product-package-first/report.json'],'status':package['status'],'target':package['target'],'developmentFixtureOnly':True,'payloadExtraction':'EXACT_BYTES_MATCH','setup':package['setup']},'originalJointRun':{'report':by['controlled-product-runtime-first/report.json'],'outerActualExit':1,'nestedOuterPowerShellActualExit':0,'phaseStatus':p['status'],'clientExitCodes':[0,0,1603],'actualMsiLogReturns':[0,0,1602],'downloadedOriginals':30,'Pending':'RETAINED_NOT_SETTLED','controllerFindings':by['controlled-product-failure-review-first/review-first.json']},'notRunOrNotClaimed':['No joint product retest of candidate7c135 after replay fix;65/66 are component tests only.','Original known nonzero physical process returns do not settle Pending.','Second controller fixes and teardown study are preparation/research, not Windows execution or recovery authority.','No current post-write/Pending/recovery active work included or queried.','No official hosted MSVC/Win7/Win10/ARP or production release proof.'],'actions':{'WindowsContacted':False,'worktreeOrTargetRead':False,'sourceDeletion':False,'mainProgressWritten':False,'commitPush':False}}
 for row in rows:scan(original(row),'source:'+row['path'])
 scan(mb,'frozen-manifest');scan(form_bytes,'exact-control-forms');controller=Path(__file__).read_bytes();scan(controller,'archive-controller-source');scan(selector_bytes,'frozen-explicit-selector');fb=raw(facts);scan(fb,'finite-batch-facts')
 archive=D/'evidence-originals-first.tar.gz'
 with archive.open('xb') as outer_file:
  with gzip.GzipFile(filename='',mode='wb',fileobj=outer_file,mtime=0,compresslevel=9) as zipped:
   with tarfile.open(fileobj=zipped,mode='w',format=tarfile.PAX_FORMAT) as tar:
    for row in rows:
     b=original(row);info=tarfile.TarInfo(row['path']);info.size=len(b);info.mode=0o600;info.uid=info.gid=info.mtime=0;info.uname=info.gname='';tar.addfile(info,io.BytesIO(b))
 with tarfile.open(archive,'r:gz') as tar:
  members=tar.getmembers();need([x.name for x in members]==[x['path'] for x in rows],'EXACT_TAR_MEMBER_SET')
  for member,row in zip(members,rows):
   need(member.isfile() and member.mode==0o600 and member.uid==member.gid==member.mtime==0 and member.uname==member.gname=='','REGULAR_DETERMINISTIC_TAR')
   b=tar.extractfile(member).read();need(b==original(row),'FULL_MEMBER_BYTE_REPLAY');scan(b,'stored-tar-gzip-decoded:'+row['path'])
 for rel in PROMOTE:
  b=original(by[rel]);target=D/rel;target.parent.mkdir(parents=True,exist_ok=True)
  with target.open('xb') as f:f.write(b)
  need(target.read_bytes()==b,'PROMOTED_RAW_BYTES');scan(b,'promoted:'+rel)
 (D/'batch-facts-first.json').write_bytes(fb)
 cb=archive.read_bytes();index={'status':'PASS_FINITE_CONTROLLED_SETUP_ORIGINALS_ARCHIVED','latestCandidate':m['candidateCommit'],'archive':{'path':archive.name,'bytes':len(cb),'sha256':sha(cb),'memberCount':len(rows)},'originalCount':len(rows),'originalBytes':m['originalBytes'],'manifest':{'path':'source-manifest-first.json','bytes':len(mb),'sha256':MANIFEST_SHA},'textForms':{'path':'text-forms-first.json','bytes':len(form_bytes),'sha256':sha(form_bytes)},'facts':{'path':'batch-facts-first.json','bytes':len(fb),'sha256':sha(fb)},'promoted':[by[x] for x in PROMOTE],'excludedBinaryIdentities':excluded,'producerArtifactsIdentityOnly':artifacts,'symlinksNotFollowed':m['symlinksNotFollowed'],'prunedDerivedPackageTrees':m['excludedDerivedPackageTrees'],'sourceDeletion':False,'WindowsContacted':False,'worktreeOrTargetRead':False,'mainProgressWritten':False}
 ib=raw(index);scan(ib,'new-index');(D/'archive-index-first.json').write_bytes(ib)
 receipt={'status':'PASS_FULL_TAR_REPLAY_AND_BOUNDED_DECODED_CREDENTIAL_SCAN','archiveSha256':sha(cb),'archiveBytes':len(cb),'indexSha256':sha(ib),'originalCount':len(rows),'originalBytes':m['originalBytes'],'credentialScan':{'status':'PASS_NO_MATCHES','statistics':stats,'jsonlRecordsAlsoDecoded':jsonl_count,'patterns':list(ns['PATTERNS']),'findings':[],'coverage':'All376 original text raw bytes, full decompressed tar member replay,15 raw promoted reports, manifest/forms/facts/controller/selector/index. Strict UTF8/UTF16/CP936, parsed CLIXML, JSON and JSONL strings, nested canonical generic/declared base64. Only3 exact path/size/SHA-bound structural control forms:18 license FF, two single MSI DigitalSignature stream notes; raw/visible-boundary/concatenated views all scanned. Unknown controls rejected; no generic tolerance. Depth12, JSON250000, decoded64MiB per original.','structuralControlFormCount':3},'scope':'Finite GNU DEV setup source/build/static14payload package, original joint FAIL and65/66 component regressions. No new joint MSI test, no Pending settlement/clearance or production release.','sourceDeletion':False,'WindowsContacted':False,'Cargo':False,'worktreeOrTargetRead':False,'mainProgressWritten':False,'privateKeyContentRead':False,'commitPush':False}
 rb=raw(receipt);scan(rb,'scan-receipt');(D/'archive-verification-first.json').write_bytes(rb)
 need(selector_ns['collect']()==(expected,links,pruned),'FINAL_CLOSED_SOURCE_SCOPE_UNCHANGED')
 for row in rows+excluded+artifacts:original(row)
 expected_dest={'archive-controller-first.py','archive-selector-first.py','source-manifest-first.json','text-forms-first.json','batch-facts-first.json','evidence-originals-first.tar.gz','archive-index-first.json','archive-verification-first.json'}|set(PROMOTE)
 need({p.relative_to(D).as_posix() for p in D.rglob('*') if p.is_file()}==expected_dest,'EXACT_FINITE_DESTINATION')
 print(json.dumps({'status':'PASS','textMembers':len(rows),'originalBytes':m['originalBytes'],'binaryIdentitiesOnly':len(excluded),'producerArtifactsIdentityOnly':len(artifacts),'archiveBytes':len(cb),'archiveSha256':sha(cb),'indexSha256':sha(ib),'verificationSha256':sha(rb),'factsSha256':sha(fb),'destinationFiles':len(expected_dest),'sourceDeletion':False,'WindowsContacted':False,'mainProgressWritten':False}))
if __name__=='__main__':main()
