#!/usr/bin/env python3
"""Finite explicit R80 foundation archive. Never reads W/target or excluded EXEs."""
from pathlib import Path
import argparse,gzip,hashlib,io,json,stat,tarfile
R=Path('/tmp/fc-round80-controlled-repair');MAIN=Path('/workspace/共享/Eliora/未来教室计划');D=Path(__file__).resolve().parent
EXPECTED_DEST=MAIN/'开发计划/验证记录/2026-11-02-round80-controlled-repair-foundation'
MANIFEST='foundation-archive-preparation-first/manifest-first.json';MANIFEST_SHA='f476d8356abc41893068eca01e5a79533c50b145c5259a053c5bd01bc2000079'
SCANNER_ARCHIVE=MAIN/'开发计划/验证记录/2026-11-02-round79-msi-maintenance/evidence-originals-first.tar.gz';SCANNER_MEMBER='archive-preparation-second/archive-second.py';SCANNER_SHA='da2906c450ea8c2ac7d18e358507023f519dbcc6faf1a9ac1abb1ae16af8c30f'
PROMOTE=['independent-foundation-acceptance-first/acceptance-first.json','independent-foundation-acceptance-first/freeze-first.json','native-runtime-first/report.json','native-build-first/report.json','strict-clippy-first/report.json','windows-readonly-final-first.json','root-native-runtime-operation-first.json','foundation-source-commit-first.json']
def need(v,m):
 if not v:raise AssertionError(m)
def sha(b):return hashlib.sha256(b).hexdigest()
def valid_rel(s):
 p=Path(s);need(isinstance(s,str) and p.as_posix()==s and not p.is_absolute() and p.parts and all(v not in ('','.','..') for v in p.parts),'CANONICAL_RELATIVE');return p

def regular(p,root):
 need(p.is_absolute() and p.resolve()==p and p.is_relative_to(root),'EXACT_NO_ALIAS')
 for q in (p,*p.parents):
  if q==root or q.is_relative_to(root):need(not q.is_symlink(),'NO_SYMLINK_PATH')
 need(stat.S_ISREG(p.lstat().st_mode),'REGULAR_ORIGINAL');need(p.stat().st_size<=16*1024*1024,'BOUNDED_ORIGINAL')
 return p

def original(row):
 rel=valid_rel(row['path']);need(not any(x in rel.parts for x in ('target-first','checkout-52d6392-first','target','deps','__pycache__')),'NO_WORKTREE_TARGET_CACHE')
 need(rel.suffix.lower() not in ('.exe','.key','.pfx','.p12','.pem','.crt','.cer','.der','.dll','.msi','.zip','.rlib','.rmeta','.so','.o','.a'),'NO_BINARY_OR_KEY_OR_CERT_READ')
 p=regular(R/rel,R);b=p.read_bytes();need(len(b)==row['bytes'] and sha(b)==row['sha256'],'EXACT_ORIGINAL_IDENTITY_'+row['path']);return b

def write_new(path,b):
 path.parent.mkdir(parents=True,exist_ok=True)
 with path.open('xb') as f:f.write(b)
 need(path.read_bytes()==b,'NEW_FILE_READBACK')

def main():
 a=argparse.ArgumentParser();a.add_argument('--execute',action='store_true');args=a.parse_args()
 if not args.execute:print(json.dumps({'status':'PREPARED_NOT_EXECUTED','archive':False,'Windows':False,'sourceDeletion':False}));return
 need(D==EXPECTED_DEST and D.resolve()==D,'EXACT_NEW_DEST');need({p.name for p in D.iterdir()}=={'archive-controller-first.py'},'FRESH_DESTINATION')
 need((R/'.owner').read_bytes()==b'future-classroom-owned-r80-controlled-repair/v1\n','EXACT_SOURCE_OWNER')
 manifest_bytes=regular(R/MANIFEST,R).read_bytes();need(sha(manifest_bytes)==MANIFEST_SHA,'FROZEN_MANIFEST');m=json.loads(manifest_bytes);rows=m['retainedTextOriginals'];excluded=m['excludedBinaryIdentities']
 need(m['retainedCount']==len(rows)==110 and sum(x['bytes'] for x in rows)==m['retainedBytes']==729604 and len(excluded)==2,'EXACT_FROZEN_CLOSURE')
 need(len({x['path'] for x in rows})==110 and set(PROMOTE)<={r['path'] for r in rows},'UNIQUE_EXACT_TEXT_ORIGINALS')
 need(all(x['path'].endswith('candidate-tests.exe') and x['bytes']==4139987 and x['sha256']=='36f8687c3bc4b12afa8995c72514a79f9ad4e7f85e88152af6225e4cd631b471' for x in excluded),'EXCLUDED_EXE_IDENTITIES_FROM_FROZEN_MANIFEST_ONLY')
 # Load only the exact reviewed scanner source bytes, never extract or traverse
 # the old source root. Its __main__/gates/inventory functions are not invoked.
 with tarfile.open(SCANNER_ARCHIVE,'r:gz') as old:
  member=old.getmember(SCANNER_MEMBER);need(member.isfile() and member.size==26272,'EXACT_SCANNER_MEMBER');scanner=old.extractfile(member).read()
 need(sha(scanner)==SCANNER_SHA,'EXACT_REVIEWED_SCANNER_SOURCE')
 ns={'__name__':'r79_frozen_scan_backend'};exec(compile(scanner,'<exact-reviewed-r79-scanner>','exec'),ns);need(ns['TEXT_FORMS']=={} and ns['TEXT_DERIVED_VIEWS']=={},'NO_CONTROL_EXCEPTIONS')
 stats={k:0 for k in ('textViews','utf16Views','cp936Views','clixmlDecodedViews','decodedBase64Views','jsonContainers','decodedJsonStringViews')};findings=[]
 def scan(b,label):
  ns['credential_scan'](b,label,stats,findings);need(not findings,'CREDENTIAL_FINDING_PATH_KIND_ONLY_'+json.dumps(findings))
 # No destination payload is written until the entire frozen text closure passes.
 for row in rows:scan(original(row),'source:'+row['path'])
 controller=Path(__file__).read_bytes();scan(controller,'new-archive-controller');scan(manifest_bytes,'frozen-explicit-manifest')
 container=D/'evidence-originals-first.tar.gz'
 with container.open('xb') as outer:
  with gzip.GzipFile(filename='',mode='wb',fileobj=outer,compresslevel=9,mtime=0) as zipped:
   with tarfile.open(fileobj=zipped,mode='w',format=tarfile.PAX_FORMAT) as tar:
    for row in rows:
     b=original(row);info=tarfile.TarInfo(row['path']);info.size=len(b);info.mode=0o600;info.uid=info.gid=info.mtime=0;info.uname=info.gname='';tar.addfile(info,io.BytesIO(b))
 with tarfile.open(container,'r:gz') as tar:
  members=tar.getmembers();need(len(members)==110 and len({x.name for x in members})==110,'EXACT_TAR_MEMBER_SET')
  for member,row in zip(members,rows):
   need(member.name==row['path'] and member.isfile() and member.mode==0o600 and member.uid==member.gid==member.mtime==0 and member.uname==member.gname=='','EXACT_REGULAR_TAR_METADATA')
   b=tar.extractfile(member).read();need(b==original(row) and sha(b)==row['sha256'],'FULL_TAR_BYTE_REPLAY');scan(b,'stored-tar:'+row['path'])
 promoted=[]
 for rel in PROMOTE:
  row=next(r for r in rows if r['path']==rel);b=original(row);write_new(D/rel,b);scan((D/rel).read_bytes(),'promoted:'+rel);promoted.append(row)
 write_new(D/'source-manifest-first.json',manifest_bytes);scan((D/'source-manifest-first.json').read_bytes(),'promoted-frozen-manifest')
 cb=container.read_bytes();index={'schemaVersion':1,'status':'PASS_FINITE_FOUNDATION_ORIGINALS_ARCHIVED_NOT_NEW_RUNTIME','sourceCommit':m['sourceCommit'],'originals':rows,'originalCount':110,'originalBytes':729604,'archiveContainer':{'path':container.name,'bytes':len(cb),'sha256':sha(cb),'memberCount':110,'format':'deterministic gzip/PAX tar','gzipMtime':0},'excludedBinaryIdentities':excluded,'excludedBinaryOriginalsReadByArchiver':False,'promoted':promoted,'manifest':{'path':'source-manifest-first.json','bytes':len(manifest_bytes),'sha256':MANIFEST_SHA},'scanner':{'archive':str(SCANNER_ARCHIVE.relative_to(MAIN)),'member':SCANNER_MEMBER,'bytes':len(scanner),'sha256':SCANNER_SHA,'originalModuleMainExecuted':False,'structuralControlExceptions':[]},'sourceDeleted':False,'WindowsContacted':False,'worktreeOrTargetRead':False}
 ib=(json.dumps(index,ensure_ascii=False,indent=2)+'\n').encode();scan(ib,'new-archive-index');write_new(D/'archive-index-first.json',ib)
 receipt={'schemaVersion':1,'status':'PASS_110_ORIGINALS_FULL_TAR_REPLAY_AND_DECODED_CREDENTIAL_SCAN','sourceCommit':m['sourceCommit'],'archiveSha256':sha(cb),'archiveBytes':len(cb),'indexSha256':sha(ib),'memberCount':110,'originalBytes':729604,'binaryIdentitiesOnly':2,'binaryOriginalReads':False,'credentialScan':{'status':'PASS_NO_MATCHES','statistics':stats,'patterns':list(ns['PATTERNS']),'findings':[],'coverage':'All110 original text byte streams, full decompressed tar replay, 8 promoted original reports, frozen manifest, archiver source and index; strict UTF8/UTF16/CP936 roundtrip, CLIXML parsed/decoded text, JSON strings and canonical nested generic/declared base64; unknown controls rejected without exceptions. Limits depth12, JSON250000 and decoded64MiB per original.'},'scope':'Foundation source/build/Clippy and Win11 synthetic-observation gate tests; production native MSI adapter not exercised. 52 includes6. Win7/Win10 true runtime NOT_RUN. No claim of full project completion.','sourceDeletion':False,'WindowsContacted':False,'Cargo':False,'worktreeOrTargetRead':False,'newIndependentReviewStarted':False}
 vb=(json.dumps(receipt,ensure_ascii=False,indent=2)+'\n').encode();scan(vb,'new-scan-receipt');write_new(D/'archive-verification-first.json',vb)
 expected={'archive-controller-first.py','evidence-originals-first.tar.gz','source-manifest-first.json','archive-index-first.json','archive-verification-first.json'}|set(PROMOTE);actual={x.relative_to(D).as_posix() for x in D.rglob('*') if x.is_file()};need(actual==expected,'EXACT_13_DEST_FILES')
 for row in rows:original(row)
 need(sha(container.read_bytes())==sha(cb) and Path(__file__).read_bytes()==controller,'FINAL_CONTAINER_CONTROLLER_STABLE')
 print(json.dumps({'status':'PASS','members':110,'originalBytes':729604,'binaryIdentitiesOnly':2,'destinationFiles':len(actual),'archiveBytes':len(cb),'archiveSha256':sha(cb),'indexSha256':sha(ib),'verificationSha256':sha(vb),'sourceDeleted':False,'Windows':False}))
if __name__=='__main__':main()
