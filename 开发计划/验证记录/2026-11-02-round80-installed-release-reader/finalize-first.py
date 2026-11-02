"""Default inert. Freeze exact finite archive files or independently verify frozen bytes."""
from pathlib import Path
import argparse,hashlib,json,stat
D=Path('/workspace/共享/Eliora/未来教室计划/开发计划/验证记录/2026-11-02-round80-installed-release-reader')
F=D/'freeze-first.json';V=D/'final-verification-first.json'
def raw(x):return (json.dumps(x,ensure_ascii=False,indent=2)+'\n').encode()
def need(x,m):
 if not x:raise AssertionError(m)
def sha(b):return hashlib.sha256(b).hexdigest()
def files():
 rows=[]
 for p in sorted(D.rglob('*')):
  mode=p.lstat().st_mode;need(not stat.S_ISLNK(mode),'NO_SYMLINK')
  if stat.S_ISDIR(mode):continue
  need(stat.S_ISREG(mode),'ONLY_REGULAR_FILES')
  # Only this exact root freeze is self-excluded; every nested freeze is included.
  if p==F:continue
  b=p.read_bytes();rows.append(dict(path=str(p.relative_to(D)),bytes=len(b),sha256=sha(b)))
 return rows

def main():
 ap=argparse.ArgumentParser();ap.add_argument('--execute',action='store_true');ap.add_argument('--verify-only',action='store_true');a=ap.parse_args()
 if not(a.execute or a.verify_only):print(json.dumps(dict(status='PREPARED_NOT_EXECUTED',Windows=False,Cargo=False,Git=False)));return
 need(a.execute!=a.verify_only,'ONE_MODE');need(Path(__file__).resolve()==D/'finalize-first.py','EXACT_FINALIZER')
 ns={'__name__':'archive_functions_only'};exec(compile((D/'archive-controller-first.py').read_bytes(),'<frozen-archive-controller>','exec'),ns)
 manifest=json.loads((D/'source-manifest-first.json').read_bytes());scan,stats,jsonl,backend=ns['scanner'](manifest['originals']);archive=ns['verify_tar'](manifest,scan);index=json.loads((D/'archive-index-first.json').read_bytes());b=archive.read_bytes();need(len(b)==index['archive']['bytes'] and sha(b)==index['archive']['sha256'],'EXACT_ARCHIVE_BYTES')
 for r in files():
  p=D/r['path']
  if p!=archive:scan(p.read_bytes(),('promoted:' if r['path'] in ns['PROMOTE'] else 'destination:')+r['path'])
 if a.execute:
  need(not F.exists() and not V.exists(),'NEW_ROOT_FREEZE_AND_FINAL_VERIFICATION_ONLY')
  for name in ['archive-operation-first.json','archive-replay-first.json']:need(json.loads((D/name).read_bytes())['actualExit']==0,'ACTUAL_ARCHIVE_AND_INDEPENDENT_REPLAY_EXIT0')
  count=len(files())+2
  result=dict(status='PASS_FINITE_FROZEN_INSTALLED_READER_TWO_GNU_PRODUCERS_WIN11_POSITIVE_AND_74_ARCHIVE_REPLAY_SCAN',archive=index['archive'],originals=312,identityOnly=10,promotedRawFiles=12,actualFinalFiles=count,freezeRows=count-1,exactSelfExcludedPath='freeze-first.json at archive root only',nestedFreezesRequired=['installed-release-reader-implementation-first/freeze-first.json','installed-release-db-implementation-first/freeze-first.json','installed-release-db-implementation-second/freeze-first.json','installed-release-db-native-preparation-second/freeze-first.json'],allOriginalsPreserved=True,archiveActualExit=0,fullSeparateReplayActualExit=0,strictCredentialScan=dict(findings=[],statistics=stats,jsonlRecords=jsonl[0],structuralScope='One exact path/size/SHA-bound UTF16 MSI structured diagnostic; raw/boundary/concatenated and decodedviews;no generic controls'),sourceBase=index['sourceBase'],actualPositiveReport=dict(bytes=101874,sha256='c477f6777ba72ac9d100370602fadc09e770b167e206818b0d539485ee25ce95'),actualRegressionReport=dict(bytes=115609,sha256='353637662fd9fbb3ed02d90608439ab3678c172e5ac7d2317a36d69bf422f823'),sourceSplit='Fixed a470 official Win7MSVC normal product;new GNU cfgtest reader W3/W4/W5 separate;no newMSVCreader validation',msiCloseEvidence='41 internal actual Rust assertions;not per-API serialized JSON ledger',sourceFailures='Original W3/W4 compile/clippy and native local qualification failures preserved;Summary38/39 capacity correction before first actual native,old PE nativeNOT_RUN',counts='One separate real installed DB probe plus lifecycle74 that includes controlled6;no addition of6+74 or nested summaries',Upgrade='NOT_AUTHORIZED_NOT_EXERCISED',Win7OS='NOT_RUN',Win10OS='NOT_RUN',WindowsContactedByArchive=False,Cargo=False,Git=False,sourceDeletion=False)
  scan(raw(result),'new-final-verification');V.write_bytes(raw(result));rows=files();need(len(rows)==count-1,'EXACT_FINAL_FREEZE_ROW_COUNT')
  freeze=dict(status='FROZEN_FINITE_INSTALLED_RELEASE_READER_AND_WIN11_NATIVE_EVIDENCE',sourceBase=index['sourceBase'],files=rows,exactSelfExcludedPath='freeze-first.json',actualTotalFiles=count,stopWriting=True)
  scan(raw(freeze),'new-root-freeze');F.write_bytes(raw(freeze))
 else:
  freeze=json.loads(F.read_bytes());scan(F.read_bytes(),'root-freeze')
 need(freeze['files']==files(),'ALL_ROOT_AND_NESTED_BYTES_EXACT')
 actual={str(p.relative_to(D)) for p in D.rglob('*') if p.is_file()};expected={r['path'] for r in freeze['files']}|{'freeze-first.json'};need(actual==expected and len(actual)==freeze['actualTotalFiles'],'EXACT_FULL_DESTINATION_MEMBERSHIP')
 need(all(p in expected for p in ['installed-release-reader-implementation-first/freeze-first.json','installed-release-db-implementation-first/freeze-first.json','installed-release-db-implementation-second/freeze-first.json','installed-release-db-native-preparation-second/freeze-first.json']),'ALL_NESTED_FREEZES_INCLUDED')
 print(json.dumps(dict(status='PASS_EXACT_FULL_ARCHIVE_MEMBERSHIP_FREEZE_AND_STRICT_REPLAY',files=len(actual),freezeRows=len(freeze['files']),freezeBytes=F.stat().st_size,freezeSha256=sha(F.read_bytes()),archive=index['archive'],findings=[],Windows=False,Cargo=False,Git=False)))
if __name__=='__main__':main()
