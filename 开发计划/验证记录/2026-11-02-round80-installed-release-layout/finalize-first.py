"""Default inert. Freeze exact finite archive files or independently verify frozen bytes."""
from pathlib import Path
import argparse,hashlib,json,stat
D=Path('/workspace/共享/Eliora/未来教室计划/开发计划/验证记录/2026-11-02-round80-installed-release-layout')
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
 ns={'__name__':'archive_functions_only'};exec(compile((D/'archive-controller-second.py').read_bytes(),'<frozen-archive-controller>','exec'),ns)
 manifest=json.loads((D/'source-manifest-first.json').read_bytes());scan,stats,jsonl,backend=ns['scanner'](manifest['originals']);archive=ns['verify_tar'](manifest,scan);index=json.loads((D/'archive-index-first.json').read_bytes());b=archive.read_bytes();need(len(b)==index['archive']['bytes'] and sha(b)==index['archive']['sha256'],'EXACT_ARCHIVE_BYTES')
 for r in files():
  p=D/r['path']
  if p!=archive:scan(p.read_bytes(),('promoted:' if r['path'] in ns['PROMOTE'] else 'destination:')+r['path'])
 if a.execute:
  need(not F.exists() and not V.exists(),'NEW_ROOT_FREEZE_AND_FINAL_VERIFICATION_ONLY')
  for name in ['archive-operation-second.json','archive-replay-first.json']:need(json.loads((D/name).read_bytes())['actualExit']==0,'ACTUAL_ARCHIVE_AND_INDEPENDENT_REPLAY_EXIT0')
  count=len(files())+2
  need(json.loads((D/'archive-operation-first.json').read_bytes())['actualExit']==1,'FIRST_LOCAL_WRAPPER_FAIL_PRESERVED')
  result=dict(status='PASS_FINITE_W6_LAYOUT_SOURCE_POSITIVE_AND77_FULL_ARCHIVE_REPLAY_SCAN',archive=index['archive'],originals=252,identityOnly=13,promotedRawFiles=15,actualFinalFiles=count,freezeRows=count-1,exactSelfExcludedPath='freeze-first.json at archive root only',nestedFreezesRequired=['installed-release-layout-implementation-first/freeze-first.json','installed-release-layout-runtime-preparation-first/freeze-first.json','installed-release-layout-native-preparation-first/freeze-first.json','installed-release-layout-source-preservation-first/freeze-first.json'],allOriginalsPreserved=True,archiveFirstAttempt=dict(actualExit=1,scope='Local exec namespace lacked exact __file__;failed before sourcecollect/tarcreate;first source/logs retained'),archiveSecondAttempt=dict(actualExit=0,scope='Only exact selector __file__ injected and selfidentity updated;no source/policy/evidence changes'),fullSeparateReplayActualExit=0,strictCredentialScan=dict(findings=[],statistics=stats,jsonlRecords=jsonl[0],structuralScope='One exact path/size/SHA-bound UTF16 MSI diagnostic only;raw/boundary/concatenated and decoded views'),sourceBase=index['sourceBase'],sourceSavedBranch='1be5e15325f934850eba8f43d158a41cd97fe05d',sourceSavedParent=index['sourceBase'],mainMerge='NOT_CLAIMED',positiveReportSha256='f86ce1ef275fa92f5ebee1d2d10b62c84d3ff62ec2247df86b60ba0e397e8c90',regressionReportSha256='d754896883cfdfb0385890bdc57221b7edab95d5158d73d0fa2c20dc9ee5427d',sourceSplit='Fixed a470 official Win7MSVCnormalproduct;new W6GNUcfgtest reader;two producers/finalASCII;not newMSVCreader',native='Positive actual1passed/Directory6 CA1 sequence23/74internalMSIclose assertions and debugstdout not perAPIJSONledger;separate77includescontrolled6,raw81minus3featureminus1exactpriorignored;3nestedchild summaries notadded',bounds='Component/File72;totalreceipt256reserved before acquisition;64combinedlive/8owner/original120s unchanged',Upgrade='NOT_AUTHORIZED_NOT_EXERCISED',carrier73aea='SEPARATE_SOURCE_NOT_INCLUDED_IN77',Win7OS='NOT_RUN',Win10OS='NOT_RUN',WindowsContactedByArchive=False,Cargo=False,Git=False,sourceDeletion=False)
  scan(raw(result),'new-final-verification');V.write_bytes(raw(result));rows=files();need(len(rows)==count-1,'EXACT_FINAL_FREEZE_ROW_COUNT')
  freeze=dict(status='FROZEN_FINITE_W6_LAYOUT_SOURCE_AND_WIN11_NATIVE_EVIDENCE',sourceBase=index['sourceBase'],files=rows,exactSelfExcludedPath='freeze-first.json',actualTotalFiles=count,stopWriting=True)
  scan(raw(freeze),'new-root-freeze');F.write_bytes(raw(freeze))
 else:
  freeze=json.loads(F.read_bytes());scan(F.read_bytes(),'root-freeze')
 need(freeze['files']==files(),'ALL_ROOT_AND_NESTED_BYTES_EXACT')
 actual={str(p.relative_to(D)) for p in D.rglob('*') if p.is_file()};expected={r['path'] for r in freeze['files']}|{'freeze-first.json'};need(actual==expected and len(actual)==freeze['actualTotalFiles'],'EXACT_FULL_DESTINATION_MEMBERSHIP')
 need(all(p in expected for p in ['installed-release-layout-implementation-first/freeze-first.json','installed-release-layout-runtime-preparation-first/freeze-first.json','installed-release-layout-native-preparation-first/freeze-first.json','installed-release-layout-source-preservation-first/freeze-first.json']),'ALL_NESTED_FREEZES_INCLUDED')
 print(json.dumps(dict(status='PASS_EXACT_FULL_ARCHIVE_MEMBERSHIP_FREEZE_AND_STRICT_REPLAY',files=len(actual),freezeRows=len(freeze['files']),freezeBytes=F.stat().st_size,freezeSha256=sha(F.read_bytes()),archive=index['archive'],findings=[],Windows=False,Cargo=False,Git=False)))
if __name__=='__main__':main()
