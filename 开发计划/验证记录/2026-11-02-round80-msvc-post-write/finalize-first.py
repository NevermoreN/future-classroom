"""Default inert. Freeze exact finite archive files or independently verify frozen bytes."""
from pathlib import Path
import argparse,hashlib,json,stat
D=Path('/workspace/共享/Eliora/未来教室计划/开发计划/验证记录/2026-11-02-round80-msvc-post-write')
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
  need(json.loads((D/'archive-operation-first.json').read_bytes())['actualExit']==1,'ORIGINAL_SCANNER_WRAPPER_FAILURE_PRESERVED')
  count=len(files())+2
  result=dict(status='PASS_FINITE_A470_MSVC_PRODUCT_GNU_TEST_WITNESS_WIN11_POST_WRITE_ARCHIVE_REPLAY_SCAN',archive=index['archive'],originals=278,identityOnly=23,promotedRawFiles=8,actualFinalFiles=count,freezeRows=count-1,exactSelfExcludedPath='freeze-first.json at archive root only',nestedFreezeRequired='a470-msvc-post-write-runtime-preparation-first/freeze-first.json',allOriginalsPreserved=True,archiveFirstAttempt=dict(actualExit=1,scope='Local scanner adapter did not accept None label from generic text decoding; no Windows or source mutation; original controller/logs preserved'),archiveSecondAttempt=dict(actualExit=0,change='Only isinstance(label,str) guard and actual controller identity; no credential-pattern or evidence policy relaxation'),fullSeparateReplayActualExit=0,strictCredentialScan=dict(findings=[],statistics=stats,jsonlRecords=jsonl[0],structuralScope='Only14 exact path/size/SHA-bound forms; raw, visible-boundary, concatenated and nested decoded views'),sourceSha=index['sourceSha'],actualRuntimeReport=dict(bytes=185723,sha256='ba2826bb9dd3b80ad18fa8c9c7511312b07fa9a0a3217f56400f448c99542dad'),sourceSplit='Official a470 Win7 MSVC product; GNU C readonly test witness only',BeginEnd='NOT_DIRECTLY_SERIALIZED_INFERENCE_ONLY',Win7OS='NOT_RUN',Win10OS='NOT_RUN',WindowsContactedByArchive=False,Cargo=False,Git=False,sourceDeletion=False)
  scan(raw(result),'new-final-verification');V.write_bytes(raw(result));rows=files();need(len(rows)==count-1,'EXACT_FINAL_FREEZE_ROW_COUNT')
  freeze=dict(status='FROZEN_FINITE_A470_MSVC_PRODUCT_GNU_WITNESS_POST_WRITE_EVIDENCE',sourceSha=index['sourceSha'],files=rows,exactSelfExcludedPath='freeze-first.json',actualTotalFiles=count,stopWriting=True)
  scan(raw(freeze),'new-root-freeze');F.write_bytes(raw(freeze))
 else:
  freeze=json.loads(F.read_bytes());scan(F.read_bytes(),'root-freeze')
 need(freeze['files']==files(),'ALL_ROOT_AND_NESTED_BYTES_EXACT')
 actual={str(p.relative_to(D)) for p in D.rglob('*') if p.is_file()};expected={r['path'] for r in freeze['files']}|{'freeze-first.json'};need(actual==expected and len(actual)==freeze['actualTotalFiles'],'EXACT_FULL_DESTINATION_MEMBERSHIP')
 need('a470-msvc-post-write-runtime-preparation-first/freeze-first.json' in expected,'NESTED_FREEZE_INCLUDED')
 print(json.dumps(dict(status='PASS_EXACT_FULL_ARCHIVE_MEMBERSHIP_FREEZE_AND_STRICT_REPLAY',files=len(actual),freezeRows=len(freeze['files']),freezeBytes=F.stat().st_size,freezeSha256=sha(F.read_bytes()),archive=index['archive'],findings=[],Windows=False,Cargo=False,Git=False)))
if __name__=='__main__':main()
