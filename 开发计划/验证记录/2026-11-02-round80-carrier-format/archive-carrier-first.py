#!/usr/bin/env python3
"""Default-inert finite carrier format evidence archive; never runs Cargo/Windows/Git/cleanup."""
from pathlib import Path
import argparse,gzip,hashlib,io,json,tarfile
R=Path('/tmp/fc-round80-controlled-repair');M=Path('/workspace/共享/Eliora/未来教室计划');D=M/'开发计划/验证记录/2026-11-02-round80-carrier-format'
TREES=['carrier-abi2-implementation-first','carrier-python-codec-first','carrier-combined-checks-first']
SINGLES=[]
PROMOTE=['carrier-abi2-implementation-first/freeze-first.json','carrier-abi2-implementation-first/implementation-first.json','carrier-abi2-implementation-first/raw-contract-first.json','carrier-abi2-implementation-first/native-checks-first.json','carrier-abi2-implementation-first/native-checks-second.json','carrier-abi2-implementation-first/pure-checks-first.json','carrier-abi2-implementation-first/pure-checks-fourth.json','carrier-python-codec-first/freeze-first.json','carrier-python-codec-first/implementation-first.json','carrier-python-codec-first/tests-first.json','carrier-python-codec-first/tests-second.json','carrier-python-codec-first/rust-interop-first.json','carrier-combined-checks-first/freeze-first.json','carrier-combined-checks-first/report-first.json','carrier-combined-checks-first/source-preservation-first.json']
FREEZE_PINS={'carrier-abi2-implementation-first/freeze-first.json':'44eef26f5f4668687275cb89faec33d1524cdd517d30137c26edafbc2a62709c','carrier-python-codec-first/freeze-first.json':'67c8e4e2040b60ce8cd86867e2626c355398f5b27ef320852628ab2cec710a15','carrier-combined-checks-first/freeze-first.json':'6e7082ed857eaed169f4a23272ac37274565e8664b3c4bba085128a012b7f46f'}
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
 need(sha(b)==BACKEND_SHA,'EXACT_FROZEN_SCANNER_SHA');ns={'__name__':'carrier_format_existing_strict_scanner'};exec(compile(b,'<existing-strict-r79-scanner>','exec'),ns);ns['TEXT_FORMS']={};ns['TEXT_DERIVED_VIEWS']={};return ns

def main():
 p=argparse.ArgumentParser();p.add_argument('--execute',action='store_true');a=p.parse_args()
 if not a.execute:print(json.dumps({'status':'PREPARED_NOT_EXECUTED','selectedTrees':TREES,'Windows':False,'Cargo':False,'sourceDeletion':False}));return
 need(Path(__file__).resolve()==D/'archive-carrier-first.py','EXACT_REVIEWABLE_CONTROLLER_DESTINATION');need((R/'.owner').read_bytes()==b'future-classroom-owned-r80-controlled-repair/v1\n','R80_OWNER')
 need({x.name for x in D.iterdir()}=={'archive-carrier-first.py','source-manifest-first.json'},'EXACT_NEW_D8_DIRECTORY')
 manifest=json.loads((D/'source-manifest-first.json').read_bytes());rows=manifest['originals'];need(collect()==rows,'EXACT_FROZEN_FINITE_MEMBERSHIP');by={r['path']:r for r in rows}
 for path,pin in FREEZE_PINS.items():need(by[path]['sha256']==pin,'EXACT_ROOT_SOURCE_FREEZE_'+path)
 need(len(by)==len(rows)==manifest['originalCount'] and sum(r['bytes'] for r in rows)==manifest['originalBytes'],'FINITE_COUNT_BYTES')
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
 rust=j('carrier-abi2-implementation-first/freeze-first.json');python=j('carrier-python-codec-first/implementation-first.json');interop=j('carrier-python-codec-first/rust-interop-first.json');combined=j('carrier-combined-checks-first/report-first.json');preserved=j('carrier-combined-checks-first/source-preservation-first.json');firstpure=j('carrier-abi2-implementation-first/pure-checks-first.json');firstnative=j('carrier-abi2-implementation-first/native-checks-first.json');secondpython=j('carrier-python-codec-first/tests-second.json')
 need(rust['actualPureTestsPassed']==13 and rust['actualGNUCheckExit']==0 and rust['actualStrictClippyExit']==0 and rust['actual12Crate33Edges'] and rust['publicationRegistryEmpty'],'ORIGINAL_RUST_FORMAT_FOUNDATION_ACTUALS')
 need(firstpure[-1]['actualExit']==101 and firstnative['checks'][-1]['actualExit']==101,'ORIGINAL_PURE_NEGATIVE_AND_CLIPPY_FAIL_RETAINED')
 need(python['pureTests']==10 and python['legacySealRegression']==13 and secondpython['sourceUnchanged'] and all(op['actualExit']==0 for op in secondpython['operations']),'SOURCE_BOUND_FINAL_PYTHON10_LEGACY13')
 need(interop['status']=='PASS_ACTUAL_RUST_EMITTED_AND_PYTHON_FULL_BYTE_CODEC_INTEROPERABILITY' and len(interop['fieldsCompared'])==7 and interop['syntheticOnly'] and not interop['publication'] and not interop['MSIValidity'],'SYNTHETIC_SEVEN_FIELD_INTEROP_NOT_AUTHORITY')
 need(combined['status']=='PASS_COMBINED_SOURCE_FORMAT_CHECKS' and combined['sourceBefore']==combined['sourceAfter'] and combined['sourceUnchanged'] and all(op['actualExit']==0 for op in combined['operations']),'ACTUAL_FINAL_INTEGRATED_CHECKS')
 need(preserved['commit']==preserved['remote']=='73aea81edbf6683da14e6cb5650c5ca0d1029db1' and preserved['parent']==combined['sourceBase']=='abf71f4b387616f8b83c4c5af8622898e594f60d' and preserved['worktreeClean'] and len(preserved['files'])==6 and all(op['actualExit']==0 for op in preserved['operations']),'RECORDED_SIX_SOURCE_COMMIT_AND_REMOTE_READBACK')
 frozen_sources={row['path']:row for row in rust['ownedSourceFiles']};frozen_sources.update({row['path']:row for row in python['source']})
 for row in preserved['files']:need(row==frozen_sources[row['path']],'ALL_SIX_COMMITTED_SOURCE_IDENTITIES_EQUAL_FROZEN')
 facts={'status':'FINITE_ABI2_CARRIER_AND_PAYLOAD_CODEC_FORMAT_FOUNDATION_EVIDENCE','sourceOrigins':{'originalRustPythonBase':'a4702b56bff66da5f5cbe74551d8751eb33fa520','actualIntegratedParent':combined['sourceBase'],'actualRecordedCommit':preserved['commit'],'recordedRemoteReadback':preserved['remote'],'sixSources':preserved['files'],'inputLedgerCount':len(combined['sourceBefore']),'originalRustCompilerInputs':rust['compilerInputCount'],'currentBranchHead':'Not queried; actual immutable73aea recorded commit is distinguished from raw frozen source comparison. Old producers requiring originala470 were not rerun.'},'RustFormat':{'pureTestsPassed':13,'GNUCheck':0,'strictClippy':0,'architecture':'12crates33edges','firstPureNegativeActualExit':101,'firstClippyActualExit':101,'historyPreserved':True},'PythonFormat':{'finalSourceBoundTests':10,'legacySealTests':13,'original9':'Historical command outcome only; original complete source snapshot unavailable. Not source-bound final acceptance.','RustInteroperability':'Actual Rust cfgtest-emitted synthetic fixture; all7 declared fields compared byte-for-byte by Python','syntheticFieldsCompared':interop['fieldsCompared'],'fixtureIsRealMSI':False},'FinalIntegratedChecks':{'sourceBase':combined['sourceBase'],'sourceInputCount':len(combined['sourceBefore']),'allOperations':combined['operations'],'sixSourceBytesMatchBothPriorFreezes':True,'commitAndPushActual0':True},'publicationRegistryEmpty':True,'nativeUpgrade':'NOT_IMPLEMENTED_NOT_AUTHORIZED','scope':'Only carrier/payload byte formats, bounded parsers and synthetic cross-language codec foundation. No native CA hookup, carrier launch, PublishedHost record, MSI execution, finalSetup, actual native memory, new-source MSVC/Win7/Win10/Seewo evidence or atomic upgrade pass. Active layout/newCA andMSVC-abf excluded; that MSVC source does not contain these6 new sources.','actions':{'Windows':False,'Cargo':False,'activeWorktreeRead':False,'sourceDeletion':False,'otherDocsCodeGitWritten':False}}
 for n in ['source-manifest-first.json','archive-carrier-first.py']:scan((D/n).read_bytes(),'archive-metadata:'+n)
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
 (D/'batch-facts-first.json').write_bytes(fb);ab=archive.read_bytes();index={'status':'PASS_FINITE_CARRIER_FORMAT_ARCHIVE','archive':{'path':archive.name,'bytes':len(ab),'sha256':sha(ab),'members':len(rows)},'originalTextBytes':manifest['originalBytes'],'sourceManifest':{'path':'source-manifest-first.json','sha256':sha((D/'source-manifest-first.json').read_bytes())},'promotedOriginals':[by[p] for p in PROMOTE],'notEntered':['active worktrees','target/deps/build caches','other active R tasks','tools/npm','machine credentials/private keys'],'sourceDeletion':False};ib=raw(index);scan(ib,'archive-index');(D/'archive-index-first.json').write_bytes(ib)
 receipt={'status':'PASS_FULL_MEMBER_REPLAY_AND_STRICT_CREDENTIAL_SCAN','archiveSha256':sha(ab),'archiveBytes':len(ab),'members':len(rows),'originalTextBytes':manifest['originalBytes'],'statistics':stats,'jsonlRecordsAlsoDecoded':jsonl,'credentialFindings':[],'scanner':{'sourceArchive':str(BACKEND.relative_to(M)),'member':MEMBER,'bytes':26272,'sha256':BACKEND_SHA},'patterns':list(ns['PATTERNS']),'coverage':'Strict UTF8/UTF16/CP936/CLIXML, JSON strings and nested base64; original depth12/decoded64MiB/container250000 bounds. No text/control whitelist added; unknown controls rejected.','actions':facts['actions']};rb=raw(receipt);scan(rb,'archive-scan-receipt');(D/'archive-verification-first.json').write_bytes(rb)
 need(collect()==rows,'ORIGINAL_MEMBERSHIP_AND_ALL_BYTES_UNCHANGED')
 print(json.dumps({'status':'PASS','members':len(rows),'textBytes':manifest['originalBytes'],'archiveBytes':len(ab),'archiveSha256':sha(ab),'indexSha256':sha(ib),'verificationSha256':sha(rb),'promoted':len(PROMOTE),'credentialFindings':0,'sourceDeletion':False,'Windows':False}))
if __name__=='__main__':main()
