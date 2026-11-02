#!/usr/bin/env python3
"""Default-inert finite host evidence archive; never runs Cargo/Windows/Git/cleanup."""
from pathlib import Path
import argparse,gzip,hashlib,io,json,tarfile
R=Path('/tmp/fc-round80-controlled-repair');M=Path('/workspace/共享/Eliora/未来教室计划');D=M/'开发计划/验证记录/2026-11-02-round80-host-regression'
TREES=['a470-host-workspace-first','a470-host-installer-targeted-first','a470-host-failure-diagnosis-first','a470-host-workspace-release-serial-first','a470-host-strict-clippy-first','host-platform-cfg-implementation-first','host-platform-cfg-validation-first']
SINGLES=['root-a470-host-release-serial-review-first.json']
PROMOTE=['a470-host-workspace-first/report.json','a470-host-workspace-first/timeout-cleanup-first.json','a470-host-installer-targeted-first/report.json','a470-host-installer-targeted-first/coverage-qualification-first.json','a470-host-failure-diagnosis-first/report.json','a470-host-failure-diagnosis-first/freeze-first.json','a470-host-workspace-release-serial-first/report.json','a470-host-workspace-release-serial-first/summary-first.json','a470-host-workspace-release-serial-first/freeze-first.json','root-a470-host-release-serial-review-first.json','a470-host-strict-clippy-first/report.json','a470-host-strict-clippy-first/diagnosis-first.json','host-platform-cfg-implementation-first/implementation-first.json','host-platform-cfg-implementation-first/freeze-first.json','host-platform-cfg-implementation-first/complete.patch','host-platform-cfg-validation-first/validation-first.json','host-platform-cfg-validation-first/freeze-first.json','host-platform-cfg-validation-first/incremental.patch']
BACKEND=M/'开发计划/验证记录/2026-11-02-round79-msi-maintenance/evidence-originals-first.tar.gz';MEMBER='archive-preparation-second/archive-second.py';BACKEND_SHA='da2906c450ea8c2ac7d18e358507023f519dbcc6faf1a9ac1abb1ae16af8c30f'
def need(condition,label):
 if not condition:raise AssertionError(label)
def sha(b):return hashlib.sha256(b).hexdigest()
def raw(j):return (json.dumps(j,ensure_ascii=False,indent=2)+'\n').encode()
def collect():
 files=[]
 def one(p):
  need(p.is_file() and not p.is_symlink() and p.resolve()==p,'EXACT_REGULAR_SOURCE')
  rel=p.relative_to(R).as_posix();need(not any(x in p.parts for x in ['target','deps','node_modules','__pycache__','.git','credentials','privatekeys']) and p.name not in ['scope.hex','scope.bin'],'NO_CACHES_MACHINE_SECRETS_SCOPE')
  need(p.name=='.owner' or p.suffix in ['.json','.jsonl','.py','.rs','.log','.txt','.patch','.sh','.toml'],'ONLY_FINITE_TEXT_SUFFIX')
  b=p.read_bytes();need(not b.startswith((b'MZ',b'\x7fELF',b'PK\x03\x04',b'\xd0\xcf\x11\xe0')),'NO_BINARY_PAYLOAD');files.append({'path':rel,'bytes':len(b),'sha256':sha(b)})
 def walk(p):
  need(p.is_dir() and not p.is_symlink(),'EXACT_SELECTED_DIRECTORY')
  for f in sorted(p.iterdir()):
   need(not f.is_symlink(),'NO_SOURCE_LINKS')
   if f.is_dir():walk(f)
   else:one(f)
 for name in TREES:walk(R/name)
 for name in SINGLES:one(R/name)
 return sorted(files,key=lambda f:f['path'])
def exact(row):
 p=R/row['path'];need(p.is_file() and not p.is_symlink() and p.resolve()==p,'EXACT_REGULAR_ORIGINAL')
 for ancestor in p.parents:
  if ancestor==R or ancestor.is_relative_to(R):need(not ancestor.is_symlink(),'NO_SYMLINK_ANCESTORS')
 b=p.read_bytes();need(len(b)==row['bytes'] and sha(b)==row['sha256'],'EXACT_ORIGINAL_BYTE_SHA_'+row['path']);return b
def scanner():
 with tarfile.open(BACKEND,'r:gz') as t:
  member=t.getmember(MEMBER);need(member.isfile() and member.size==26272,'EXACT_FROZEN_SCANNER_MEMBER');b=t.extractfile(member).read()
 need(sha(b)==BACKEND_SHA,'EXACT_FROZEN_SCANNER_SHA');ns={'__name__':'host_regression_existing_strict_scanner'};exec(compile(b,'<existing-strict-r79-scanner>','exec'),ns);ns['TEXT_FORMS']={};ns['TEXT_DERIVED_VIEWS']={};return ns

def main():
 p=argparse.ArgumentParser();p.add_argument('--execute',action='store_true');a=p.parse_args()
 if not a.execute:print(json.dumps({'status':'PREPARED_NOT_EXECUTED','selectedTrees':TREES,'Windows':False,'Cargo':False,'sourceDeletion':False}));return
 need(Path(__file__).resolve()==D/'archive-host-first.py','EXACT_REVIEWABLE_CONTROLLER_DESTINATION');need((R/'.owner').read_bytes()==b'future-classroom-owned-r80-controlled-repair/v1\n','R80_OWNER')
 need({x.name for x in D.iterdir()}=={'archive-host-first.py','source-manifest-first.json'},'EXACT_NEW_D7_DIRECTORY')
 manifest=json.loads((D/'source-manifest-first.json').read_bytes());rows=manifest['originals'];need(collect()==rows,'EXACT_FROZEN_FINITE_MEMBERSHIP');by={r['path']:r for r in rows};need(len(by)==len(rows)==manifest['originalCount'] and sum(r['bytes'] for r in rows)==manifest['originalBytes'],'FINITE_COUNT_BYTES')
 ns=scanner();stats={k:0 for k in ['textViews','utf16Views','cp936Views','clixmlDecodedViews','decodedBase64Views','jsonContainers','decodedJsonStringViews']};findings=[];jsonl=0
 def scan(b,label):
  nonlocal jsonl
  ns['credential_scan'](b,label,stats,findings)
  if label.endswith('.jsonl'):
   for i,line in enumerate(b.splitlines()):
    if line.strip():need(i<250000,'JSONL_RECORD_BOUND');json.loads(line);ns['credential_scan'](line,label+'::jsonl/'+str(i+1),stats,findings);jsonl+=1
  need(not findings,'CREDENTIAL_FINDING_STOP_PATH_KIND_ONLY_'+json.dumps(findings))
 for row in rows:scan(exact(row),'source:'+row['path'])
 def j(name):return json.loads(exact(by[name]))
 old=j('a470-host-workspace-first/report.json');serial=j('a470-host-workspace-release-serial-first/report.json');summary=j('a470-host-workspace-release-serial-first/summary-first.json');diagnosis=j('a470-host-failure-diagnosis-first/report.json');lint=j('a470-host-strict-clippy-first/report.json');fixed=j('host-platform-cfg-implementation-first/implementation-first.json');validation=j('host-platform-cfg-validation-first/validation-first.json')
 need(old['actualExit']==-9 and old['timedOut'] and old['seconds']>=600,'ORIGINAL_DEBUG_FAIL_RETAINED');need(serial['actualExit']==0 and summary['passed']==989 and summary['failed']==0 and not serial['remainingGroup'],'ACTUAL_NEW_SERIAL_989');need(diagnosis['parallelDisk']['actualExit']==101 and diagnosis['parallelDisk']['failed']==9,'ACTUAL_PARALLEL_KDF_FAIL');need(lint['actualExit']==101 and all(v['actualExit']==0 for v in fixed['actualChecks']),'STRICT_HOST_FAIL_THEN_MINIMAL_CFG_PASS');need(validation['runtimeTestsPassed']==8 and validation['selfCheckActualExit']==0 and validation['sourcesMatchFrozenImplementation'],'ACTUAL8_RUNTIME_AND_SELFCHECK')
 facts={'status':'FINITE_HOST_REGRESSION_HISTORY_AND_MINIMAL_CFG_FIX_EVIDENCE','baselineGit':'a4702b56bff66da5f5cbe74551d8751eb33fa520','originalDebug':'600s timeout/actual-9 and11FAILED lines preserved. Original0matching installer host check is qualified as no native coverage.','diagnosis':'11 original names pass in individual disk runs; early60s reopen timeout retained;8thread concurrent11case batch actual101 with9fixture LoginCapacity panics and2passes. Existing verify.sh already bounds process-wide2KDFslots with2testthreads.','additionalReleaseSerial':'Wholeworkspace actual0,33harnesses989passed, original group and fixtures returned. Host release/serial qualification only; prior failure not reclassified.','minimalCfgCandidate':{'files':4,'cfgAddedLines':11,'functionBodiesChanged':False,'broadAllowAdded':False,'strictHostActualExit':0,'runtimeHostTestsPassed':8,'selfCheckActualExit':0},'scope':'No native Windows runtime, Win7/10 claim, native lifecycle coverage from host0test harness, full newcfg989rerun, production upgrade authorization or release inferred. Existing Linux tmpfs fixtures preserve original scope.','actions':{'Windows':False,'Cargo':False,'activeWorktreeRead':False,'sourceDeletion':False,'otherDocsCodeGitWritten':False}}
 for n in ['source-manifest-first.json','archive-host-first.py']:scan((D/n).read_bytes(),'archive-metadata:'+n)
 fb=raw(facts);scan(fb,'archive-facts')
 archive=D/'evidence-originals-first.tar.gz'
 with archive.open('xb') as out:
  with gzip.GzipFile(filename='',mode='wb',fileobj=out,mtime=0,compresslevel=9) as z:
   with tarfile.open(fileobj=z,mode='w',format=tarfile.PAX_FORMAT) as t:
    for row in rows:
     b=exact(row);member=tarfile.TarInfo(row['path']);member.size=len(b);member.uid=member.gid=member.mtime=0;member.mode=0o600;member.uname=member.gname='';t.addfile(member,io.BytesIO(b))
 with tarfile.open(archive,'r:gz') as t:
  members=t.getmembers();need([m.name for m in members]==[r['path'] for r in rows],'EXACT_TAR_NO_UNKNOWN_MEMBERS')
  for member,row in zip(members,rows):
   need(member.isfile() and member.uid==member.gid==member.mtime==0 and member.mode==0o600 and member.uname==member.gname=='','CANONICAL_REGULAR_MEMBER');b=t.extractfile(member).read();need(b==exact(row),'FULL_RAW_MEMBER_REPLAY');scan(b,'stored-tar-gzip-decoded:'+row['path'])
 for rel in PROMOTE:
  b=exact(by[rel]);dest=D/rel;dest.parent.mkdir(parents=True,exist_ok=True)
  with dest.open('xb') as f:f.write(b)
  need(dest.read_bytes()==b,'PROMOTED_ORIGINAL_RAW_BYTES');scan(b,'promoted:'+rel)
 (D/'batch-facts-first.json').write_bytes(fb);ab=archive.read_bytes();index={'status':'PASS_FINITE_HOST_REGRESSION_ARCHIVE','archive':{'path':archive.name,'bytes':len(ab),'sha256':sha(ab),'members':len(rows)},'originalTextBytes':manifest['originalBytes'],'sourceManifest':{'path':'source-manifest-first.json','sha256':sha((D/'source-manifest-first.json').read_bytes())},'promotedOriginals':[by[p] for p in PROMOTE],'notEntered':['active worktrees','target/deps/build caches','other active R tasks','tools/npm','machine credentials/private keys'],'sourceDeletion':False};ib=raw(index);scan(ib,'archive-index');(D/'archive-index-first.json').write_bytes(ib)
 receipt={'status':'PASS_FULL_MEMBER_REPLAY_AND_STRICT_CREDENTIAL_SCAN','archiveSha256':sha(ab),'archiveBytes':len(ab),'members':len(rows),'originalTextBytes':manifest['originalBytes'],'statistics':stats,'jsonlRecordsAlsoDecoded':jsonl,'credentialFindings':[],'scanner':{'sourceArchive':str(BACKEND.relative_to(M)),'member':MEMBER,'bytes':26272,'sha256':BACKEND_SHA},'patterns':list(ns['PATTERNS']),'coverage':'Strict UTF8/UTF16/CP936/CLIXML, JSON strings and nested base64; original depth12/decoded64MiB/container250000 bounds. No text/control whitelist added; unknown controls rejected.','actions':facts['actions']};rb=raw(receipt);scan(rb,'archive-scan-receipt');(D/'archive-verification-first.json').write_bytes(rb)
 need(collect()==rows,'ORIGINAL_MEMBERSHIP_AND_ALL_BYTES_UNCHANGED')
 print(json.dumps({'status':'PASS','members':len(rows),'textBytes':manifest['originalBytes'],'archiveBytes':len(ab),'archiveSha256':sha(ab),'indexSha256':sha(ib),'verificationSha256':sha(rb),'promoted':len(PROMOTE),'credentialFindings':0,'sourceDeletion':False,'Windows':False}))
if __name__=='__main__':main()
