"""Final complete replay and text scan, local finite D only; default inert."""
from pathlib import Path
import argparse,hashlib,json,tarfile
M=Path('/workspace/共享/Eliora/未来教室计划');D=Path(__file__).resolve().parent;R=Path('/tmp/fc-round80-controlled-repair')
def sha(b):return hashlib.sha256(b).hexdigest()
def need(v,s):
 if not v:raise AssertionError(s)
def main():
 ap=argparse.ArgumentParser();ap.add_argument('--execute',action='store_true');a=ap.parse_args()
 if not a.execute:print('PREPARED_NOT_EXECUTED');return
 manifest=json.loads((D/'source-manifest-first.json').read_bytes());by={r['path']:r for r in manifest['originals']+manifest['excludedBinaryIdentities']}
 source=(D/'archive-controller-second.py').read_text();start=source.index(" with tarfile.open(M/'开发计划/验证记录/2026-11-02-round79-msi-maintenance/evidence-originals-first.tar.gz'");end=source.index(' for row in rows:scan(original(row)',start)
 setup='def make_scan():\n'+source[start:end]+' return scan,stats,findings,lambda:jsonl\n'
 ns=dict(M=M,D=D,R=R,by=by,need=need,sha=sha,Path=Path,json=json,tarfile=tarfile,SCANNER_MEMBER='archive-preparation-second/archive-second.py',SCANNER_SHA='da2906c450ea8c2ac7d18e358507023f519dbcc6faf1a9ac1abb1ae16af8c30f');exec(compile(setup,'<exact-second-scanner-setup>','exec'),ns);scan,stats,findings,jsonl=ns['make_scan']()
 index=json.loads((D/'archive-index-first.json').read_bytes());archive=D/index['archive']['path'];ab=archive.read_bytes();need(sha(ab)==index['archive']['sha256'] and len(ab)==index['archive']['bytes'],'ARCHIVE_EXACT')
 with tarfile.open(archive) as tar:
  members=tar.getmembers();need([v.name for v in members]==[r['path'] for r in manifest['originals']],'EXACT_ALL291_MEMBERS')
  for member,row in zip(members,manifest['originals']):
   need(member.isfile(),'REGULAR_MEMBER');b=tar.extractfile(member).read();need(len(b)==row['bytes'] and sha(b)==row['sha256'] and b==(R/row['path']).read_bytes(),'FULL_ORIGINAL_REPLAY');scan(b,'stored-tar-gzip-decoded:'+row['path'])
 promoted={r['path']:r for r in index['promoted']};count=0
 for p in sorted(D.rglob('*')):
  if not p.is_file():continue
  need(not p.is_symlink(),'NO_ARCHIVE_LINK')
  rel=p.relative_to(D).as_posix()
  if rel==archive.name:continue
  if rel in promoted:
   row=promoted[rel];b=p.read_bytes();need(len(b)==row['bytes'] and sha(b)==row['sha256'] and b==(R/rel).read_bytes(),'PROMOTED_ORIGINAL_EXACT');label='promoted:'+rel
  else:b=p.read_bytes();label='archive-final:'+rel
  scan(b,label);count+=1
 need(not findings,'NO_CREDENTIAL_MATCHES');receipt={'status':'PASS_FINAL_FULL_TAR_REPLAY_PROMOTED_BYTES_AND_ALL_DESTINATION_TEXT_SCAN','archiveSha256':sha(ab),'tarMembersReplayed':len(members),'destinationTextFilesScanned':count,'credentialStatistics':stats,'jsonlRecordsAlsoDecoded':jsonl(),'findings':[],'sourceDeletion':False,'WindowsContacted':False,'Cargo':False,'WOrTargetRead':False,'progressWritten':False,'GitOperation':False};data=(json.dumps(receipt,indent=2)+'\n').encode();scan(data,'new-final-receipt');p=D/'final-verification-second.json';need(not p.exists(),'NEW_FINAL_RECEIPT');p.write_bytes(data);print(json.dumps(receipt))
if __name__=='__main__':main()
