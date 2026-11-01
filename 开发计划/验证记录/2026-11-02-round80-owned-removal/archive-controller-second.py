#!/usr/bin/env python3
"""Default-inert finite archive. No Windows, Cargo, worktree, cleanup or Git writes."""
from pathlib import Path
import argparse,ast,gzip,hashlib,io,json,stat,tarfile
R=Path('/tmp/fc-round80-controlled-repair');M=Path('/workspace/共享/Eliora/未来教室计划');D=M/'开发计划/验证记录/2026-11-02-round80-owned-removal'
PINS={'source-manifest-first.json':'7c2783149ad7c975537c3d3916cdbea95321d321fa5e5ebd61b463bc779c2fc0','text-forms-first.json':'a0dc5dd2fa80e10c9f82cd87be7b49f28c4e9f42805b1da8f16f1dc9b20721d2','derived-forms-first.json':'b5da6e00424ff9916951fcc5f1359c29f30a3b588ff622e98a0a8f459576faa4','structural-views-first.py':'d309e7198267a1c5d7e6eb031f3b141cbf73afb41ec7666c49152e69014fc907','select-first.py':'14cd31032f086245337be9daafb07f85bb4ea97f8b689955a53f918716ea8636'}
PINS.update({'archive-controller-first.py': 'e4816d532f42f7007b2b8cd15fc198855875d15f9dff4a9302b6cb60021a0f9d', 'plain-tool-forms-first.json': 'c8dfa1290fb08041911d1cde675c0ed3bc34af783d8b5ab471748b58e4a7702d', 'archive-operation-first.json': '002cf34c3bda57e83151914f67fbaaa19b21b7448cdd2c576eb1c18de318978e'})
PROMOTE=['owned-removal-native-runtime-first/report.json','owned-removal-runtime-first/report.json','owned-removal-cleanup-first/report.json','controlled-product-runtime-second/report.json','guard-owned-removal-clippy-feature-first/report.json','guard-owned-removal-clippy-feature-second/report.json','guard-owned-removal-clippy-default-first/report.json','guard-owned-removal-build-second/report.json','pending-readonly-runtime-first/report.json','pending-readonly-retrieval-first/report.json','owned-removal-source-preservation-first/source-preservation-first.json','owned-removal-candidate-sync-first.json','setup-msvc-monitor-first/report.json','setup-msvc-monitor-second/report.json','setup-msvc-modern-download-first/reports/result.json','setup-msvc-win7-download-first/diagnostics/logs/clippy.result.json']
BACKEND=M/'开发计划/验证记录/2026-11-02-round79-msi-maintenance/evidence-originals-first.tar.gz';MEMBER='archive-preparation-second/archive-second.py';BACKEND_SHA='da2906c450ea8c2ac7d18e358507023f519dbcc6faf1a9ac1abb1ae16af8c30f'
def need(v,m):
 if not v:raise AssertionError(m)
def sha(b):return hashlib.sha256(b).hexdigest()
def raw(j):return (json.dumps(j,ensure_ascii=False,indent=2)+'\n').encode()
def original(row):
 p=R/row['path'];need(p.resolve()==p and p.is_file() and not p.is_symlink(),'EXACT_REGULAR_ORIGINAL')
 for v in p.parents:
  if v==R or v.is_relative_to(R):need(not v.is_symlink(),'NO_SOURCE_SYMLINK_ANCESTOR')
 need(not any(x in p.parts for x in ['target','target-first','tools','node_modules','__pycache__','.git','checkout-52d6392-first','checkout-normal-33c11-first']),'NO_ACTIVE_WORKTREE_CACHE_TOOL_READ')
 b=p.read_bytes();need(len(b)==row['bytes'] and sha(b)==row['sha256'],'EXACT_SOURCE_BYTES_'+row['path']);return b

def main():
 ap=argparse.ArgumentParser();ap.add_argument('--execute',action='store_true');a=ap.parse_args()
 if not a.execute:print(json.dumps({'status':'PREPARED_NOT_EXECUTED','Windows':False,'sourceDeletion':False}));return
 need(Path(__file__).resolve()==D/'archive-controller-second.py' and {p.name for p in D.iterdir()}==set(PINS)|{'archive-controller-second.py'},'EXACT_NEW_FINITE_DESTINATION');need((R/'.owner').read_bytes()==b'future-classroom-owned-r80-controlled-repair/v1\n','R80_OWNER')
 for n,h in PINS.items():need(sha((D/n).read_bytes())==h,'FROZEN_PREPARATION_BYTES_'+n)
 m=json.loads((D/'source-manifest-first.json').read_bytes());rows=m['originals'];by={r['path']:r for r in rows};need(len(rows)==584 and m['originalCount']==584 and sum(r['bytes'] for r in rows)==m['originalBytes']==7919248,'FINITE_584_TEXT_CLOSURE')
 selector={'__name__':'frozen_explicit_selector'};exec(compile((D/'select-first.py').read_bytes(),'<frozen-explicit-selector>','exec'),selector);membership=selector['collect']();need(membership[0]==sorted(set(by)|{r['path'] for r in m['binaryIdentitiesOnly']}|{r['path'] for r in m['derivedDisassemblyIdentitiesOnly']}) and membership[1:]==(m['symlinksNotFollowed'],m['prunedNotEntered'],m['explicitExcluded']),'EXACT_SELECTED_MEMBERSHIP')
 # These are identities only. Explicit scope and official ZIP contents are never opened.
 for row in m['binaryIdentitiesOnly']+m['derivedDisassemblyIdentitiesOnly']:original(row)
 for row in m['explicitExcluded']:
  p=R/row['path'];need(p.is_file() and not p.is_symlink() and p.stat().st_size==row['bytes'],'EXCLUDED_METADATA_SIZE')
 with tarfile.open(BACKEND,'r:gz') as t:
  mem=t.getmember(MEMBER);need(mem.isfile() and mem.size==26272,'EXACT_BACKEND_MEMBER');backend=t.extractfile(mem).read()
 need(sha(backend)==BACKEND_SHA,'FROZEN_EXISTING_CREDENTIAL_SCANNER');ns={'__name__':'frozen_finite_owned_removal_scanner'};exec(compile(backend,'<frozen-r79-scanner>','exec'),ns)
 forms=json.loads((D/'text-forms-first.json').read_bytes());need(len(forms['files'])==3,'THREE_MSI_CONTROL_FORMS');ns['TEXT_FORMS']={r['path']:r for r in forms['files']};ns['TEXT_DERIVED_VIEWS']={}
 for row in forms['files']:need(by[row['path']]['bytes']==row['bytes'] and by[row['path']]['sha256']==row['sha256'],'EXACT_MSI_FORM_PARENT')
 structural={'__name__':'exact_structural_views'};exec(compile((D/'structural-views-first.py').read_bytes(),'<exact-native-structures>','exec'),structural);derived=json.loads((D/'derived-forms-first.json').read_bytes());need(len(derived['views'])==51,'EXACT_51_DECLARED_JSON_NATIVE_STRUCTURES');native_counts=structural['attach'](ns,derived,rows)
 stats={k:0 for k in ['textViews','utf16Views','cp936Views','clixmlDecodedViews','decodedBase64Views','jsonContainers','decodedJsonStringViews']};findings=[];jsonl=0;plain_lines=0;plain={r['path']:r for r in json.loads((D/'plain-tool-forms-first.json').read_bytes())['files']}
 for rel,row in plain.items():need(row['bytes']==by[rel]['bytes'] and row['sha256']==by[rel]['sha256'],'EXACT_PLAIN_TOOL_PARENT')
 def scan(b,label):
  nonlocal jsonl,plain_lines
  ns['credential_scan'](b,label,stats,findings)
  if label.endswith('.jsonl'):
   for i,line in enumerate(b.splitlines()):
    if not line.strip():continue
    need(i<250000,'JSONL_RECORD_BOUND')
    try:json.loads(line)
    except ValueError:
     rel=label.split(':',1)[1];need(rel in plain and i+1 in plain[rel]['notJsonLines'],'NO_UNKNOWN_NON_JSONL_LINE');plain_lines+=1;ns['credential_scan'](line,label+'::exact-plain-tool-line/'+str(i+1),stats,findings)
    else:jsonl+=1;ns['credential_scan'](line,label+'::jsonl/'+str(i+1),stats,findings)
  need(not findings,'CREDENTIAL_FINDING_STOP_PATH_KIND_ONLY_'+json.dumps(findings))
 for row in rows:scan(original(row),'source:'+row['path'])
 def j(rel):return json.loads(original(by[rel]))
 pins={'owned-removal-runtime-first/report.json':'a252752d124c4053db9c618135c0b5c2b326ec655e96419de6ac4ac87fc36b2c','owned-removal-cleanup-first/report.json':'8c9daab45c4b2e38944d41bbe482536ae1252eac71dc9556a5f384b1b6a1013e','controlled-product-runtime-second/report.json':'04671228fafe69c3eb799640dce2eae2802efc847838df4d81a5392bebb89de9'}
 for p,h in pins.items():need(by[p]['sha256']==h,'ROOT_PINNED_ACTUAL_REPORT')
 recovery=j('owned-removal-runtime-first/report.json');cleanup=j('owned-removal-cleanup-first/report.json');normal=j('controlled-product-runtime-second/report.json');native=j('owned-removal-native-runtime-first/report.json');oldpending=j('pending-readonly-runtime-first/report.json');retrieval=j('pending-readonly-retrieval-first/report.json');lintfail=j('guard-owned-removal-clippy-feature-first/report.json')
 need(recovery['runtimeActualExit']==0 and recovery['oldEnd']=='UNKNOWN_UNCHANGED' and recovery['downloadVerified'] and len(recovery['inventory']['files'])==21,'ACTUAL_EXPLICIT_REMOVAL_NOT_OLD_SETTLEMENT');need(cleanup['actualExit']==0 and cleanup['status']=='PASS_EXACT_EMPTY_GATE_AND_BOTH_DOWNLOADED_NONCES_CLEANED' and cleanup['oldEnd']=='UNKNOWN_UNCHANGED','ACTUAL_SEPARATE_CLEANUP');need(normal['runtimeActualExit']==0 and normal['status']=='PASS_GNU_WIN11_DEV_CONTROLLED_PRODUCT_CLOSURE_AND_EXACT_SCRATCH_CLEANUP' and normal['downloadVerified'] and len(normal['inventory']['files'])==30 and normal['cleanup']['knownAbsent'],'ACTUAL_NORMAL_JOINT_AND_NONCE_CLEANUP');need(native['expectedScopes']=={'controlled':6,'lifecycle':69},'69_INCLUDES_CONTROLLED6');need(lintfail['actualExit']==101,'ORIGINAL_FEATURE_CLIPPY_FAIL_RETAINED')
 for p in ['guard-owned-removal-clippy-feature-second/report.json','guard-owned-removal-clippy-default-first/report.json']:need(j(p)['actualExit']==0,'FINITE_LATER_CLIPPY_SUCCESS')
 facts={'status':'FINITE_COMPLETED_OWNED_FIXTURE_REMOVAL_AND_NORMAL33_JOINT_EVIDENCE','candidate':'33c11f5e41cce727746aeb5b5801b0e87eaf0f41','oldPayloadSource':'c50882df40b86248e3e2d4e404d0fbe8e43e30a9','coreReports':[by[p] for p in PROMOTE],'core69':{'scopes':native['expectedScopes'],'overlap':'69 includes6; component scope distinct from joint product runtime'},'PendingCaptureHistory':{'originalStatus':oldpending['status'],'retrievalStatus':retrieval['status'],'originalFailurePreserved':True},'explicitRemoval':{'status':recovery['status'],'BExit':0,'newNativeTransactions':2,'oldEnd':'UNKNOWN_UNCHANGED','newNativeAuditStage':'RemovalAuthorized plus actual B0/fullAbsent and PendingAbsent; not audit-only deletion proof','filesDownloaded':21,'cleanupSeparate':cleanup['status']},'normal33Joint':{'status':normal['status'],'phases':['install','repair after exact LICENSE damage','uninstall'],'allThreeActualB0':True,'all14Restored':True,'finalProductPendingGateAbsent':True,'downloadedFiles':30,'nonceKnownAbsent':True},'historicalClippy':{'featureFirstActualExit':101,'featureSecondActualExit':0,'defaultFirstActualExit':0,'oldHostFailuresIndependent':True},'CI7cScope':'Modern compile checks passed/runtime NOT_RUN; Win7 actual lint failure retained. Separate source7c, not a33c official product release. Only finite metadata and useful official diagnostic reports; ZIP/PE identity only.','oldFullBatchReference':{'path':'开发计划/验证记录/2026-11-02-round80-controlled-setup/evidence-originals-first.tar.gz','sha256':'f49494a0d6b540724ba3bf66b0b23610e3c69952a6d1e043d3337bcf48a07345','originalThreeStageFail':'Retained unchanged in that previous archive; not reclassified or duplicated wholesale.'},'notClaimed':['No old unknown transaction settlement','No general production crash recovery/ARP proof','No Win7/Win10 actual runtime or official hostedMSVC product joint proof','No active a470 monitor/W2/postwrite scan or archival'], 'actions':{'Windows':False,'Cargo':False,'activeWorktreeRead':False,'sourceDeletion':False,'progressWritten':False,'commitPush':False}}
 fb=raw(facts)
 for n in PINS:scan((D/n).read_bytes(),'archive-metadata:'+n)
 scan(Path(__file__).read_bytes(),'archive-controller');scan(fb,'finite-facts')
 archive=D/'evidence-originals-first.tar.gz'
 with archive.open('xb') as out:
  with gzip.GzipFile(filename='',mode='wb',fileobj=out,mtime=0,compresslevel=9) as z:
   with tarfile.open(fileobj=z,mode='w',format=tarfile.PAX_FORMAT) as t:
    for row in rows:
     b=original(row);i=tarfile.TarInfo(row['path']);i.size=len(b);i.uid=i.gid=i.mtime=0;i.mode=0o600;i.uname=i.gname='';t.addfile(i,io.BytesIO(b))
 with tarfile.open(archive,'r:gz') as t:
  members=t.getmembers();need([v.name for v in members]==[r['path'] for r in rows],'EXACT_TAR_MEMBERS_NO_UNKNOWN')
  for member,row in zip(members,rows):
   need(member.isfile() and member.uid==member.gid==member.mtime==0 and member.mode==0o600 and member.uname==member.gname=='','REGULAR_CANONICAL_TAR');b=t.extractfile(member).read();need(b==original(row),'FULL_TAR_BYTE_REPLAY');scan(b,'stored-tar-gzip-decoded:'+row['path'])
 for rel in PROMOTE:
  b=original(by[rel]);p=D/rel;p.parent.mkdir(parents=True,exist_ok=True)
  with p.open('xb') as f:f.write(b)
  need(p.read_bytes()==b,'DIRECT_RAW_BYTES_MATCH');scan(b,'promoted:'+rel)
 (D/'batch-facts-first.json').write_bytes(fb);ab=archive.read_bytes();index={'status':'PASS_FINITE_ARCHIVE','archive':{'path':archive.name,'bytes':len(ab),'sha256':sha(ab),'members':len(rows)},'originalTextBytes':m['originalBytes'],'sourceManifest':{'path':'source-manifest-first.json','sha256':PINS['source-manifest-first.json']},'promoted':[by[p] for p in PROMOTE],'binaryIdentitiesOnly':m['binaryIdentitiesOnly'],'derivedDisassemblyIdentitiesOnly':m['derivedDisassemblyIdentitiesOnly'],'explicitExcluded':m['explicitExcluded'],'prunedNotEntered':m['prunedNotEntered'],'symlinksNotFollowed':m['symlinksNotFollowed'],'sourceDeletion':False};ib=raw(index);scan(ib,'archive-index');(D/'archive-index-first.json').write_bytes(ib)
 receipt={'status':'PASS_FULL_MEMBER_REPLAY_AND_STRICT_FINITE_CREDENTIAL_SCAN','archiveSha256':sha(ab),'archiveBytes':len(ab),'memberCount':len(rows),'originalTextBytes':m['originalBytes'],'statistics':stats,'jsonlRecordsAlsoDecoded':jsonl,'exactPlainToolVersionLinesAlsoScanned':plain_lines,'nativeStructuralDecodedViews':native_counts,'credentialFindings':[],'credentialPatterns':list(ns['PATTERNS']),'textForms':'3 exact UTF16 MSI notes,51 exact parentPath/bytes/SHA+JSONtrail+decodedSHA native structures;3 known decoded identities only. Every raw original preserved; full structural/hex/visible/concatenated native views scanned. Unknown controls rejected.','encodingCoverage':'Strict UTF8/UTF16/CP936,CLIXML,JSON/JSONL strings,canonical and declared nestedbase64. Depth12/JSON250000/decoded64MiB per original bounds retained.','directReports':len(PROMOTE),'actions':facts['actions']};rb=raw(receipt);scan(rb,'scan-receipt');(D/'archive-verification-first.json').write_bytes(rb)
 need(selector['collect']()==membership,'FINITE_SOURCE_MEMBERSHIP_UNCHANGED')
 for row in rows:original(row)
 print(json.dumps({'status':'PASS','members':len(rows),'textBytes':m['originalBytes'],'archiveBytes':len(ab),'archiveSha256':sha(ab),'indexSha256':sha(ib),'verificationSha256':sha(rb),'factsSha256':sha(fb),'promoted':len(PROMOTE),'credentialFindings':0,'sourceDeletion':False,'Windows':False}))
if __name__=='__main__':main()
