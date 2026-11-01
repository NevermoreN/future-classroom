#!/usr/bin/env python3
"""Finite adapter component evidence acceptance, never reads W/target or EXEs."""
from pathlib import Path
import argparse,hashlib,json,re,stat,subprocess
R=Path('/tmp/fc-round80-controlled-repair');D=Path(__file__).resolve().parent;MAIN=Path('/workspace/共享/Eliora/未来教室计划');COMMIT='b7e4c938ab8ff391e2ad6c1d681e074881118c0a'
DIRECTORIES=['adapter-implementation-first','adapter-revision-first','adapter-semantic-review-first','setup-seal-implementation-first','adapter-build-first','adapter-build-second','adapter-build-third','adapter-clippy-first','adapter-clippy-second','adapter-native-preparation-first','adapter-native-preparation-second','adapter-native-runtime-first','adapter-source-preservation-first','inner-guard-design-first']
ROOT_FILES=['adapter-inputs-first.json','adapter-inputs-second.json','adapter-inputs-third.json','check-adapter-candidate.py','root-adapter-native-ps-parse-first.json','root-adapter-native-ps-parse-first.stderr.log','root-adapter-native-ps-parse-first.stdin.ps1','root-adapter-native-ps-parse-first.stdout.log','root-adapter-native-ps-parse-second.json','root-adapter-native-ps-parse-second.stderr.log','root-adapter-native-ps-parse-second.stdin.ps1','root-adapter-native-ps-parse-second.stdout.log','root-adapter-native-runtime-operation-first.json','root-adapter-native-runtime-operation-first.stderr.log','root-adapter-native-runtime-operation-first.stdout.log','windows-readonly-adapter-first.json','adapter-candidate-sync-first.json']
inputs={};checks=[]
def need(x,s):
 if not x:raise AssertionError(s)
def sha(b):return hashlib.sha256(b).hexdigest()
def regular(p):
 need(p.resolve()==p and p.is_relative_to(R) and p.is_file() and not p.is_symlink(),'EXACT_OWNED_REGULAR')
 for q in (p,*p.parents):
  if q==R or q.is_relative_to(R):need(not q.is_symlink(),'NO_LINK_ANCESTOR')
 need(p.stat().st_size<=16*1024*1024,'FINITE_ORIGINAL');return p

def read(rel):
 need(not any(x in Path(rel).parts for x in ('target','target-first','deps','checkout-52d6392-first','__pycache__')),'NO_ACTIVE_SOURCE_CACHE_READ');need(Path(rel).suffix.lower() not in ('.exe','.dll','.msi','.key','.pem','.pfx','.p12'),'NO_BINARY_OR_KEY_CONTENT_READ')
 b=regular(R/rel).read_bytes();inputs[rel]={'path':rel,'bytes':len(b),'sha256':sha(b)};return b

def j(rel):return json.loads(read(rel))
def pinned(rel,h):
 b=read(rel);need(sha(b)==h,'PINNED_SHA_'+rel);return json.loads(b)
def eq(rel,row):
 b=read(rel);need(len(b)==row['bytes'] and sha(b)==row['sha256'],'EXACT_BYTE_IDENTITY_'+rel)
def check(name,**kw):checks.append({'check':name,'status':'PASS',**kw})
def git(*args):return subprocess.check_output(['git','-C',str(MAIN),*args],timeout=20)
def main():
 p=argparse.ArgumentParser();p.add_argument('--execute',action='store_true');p.add_argument('--design-stopped',action='store_true');p.add_argument('--design-freeze-name');p.add_argument('--design-freeze-sha256');a=p.parse_args()
 if not a.execute:print(json.dumps({'status':'PREPARED_NOT_RUN','Windows':False}));return
 need(a.design_stopped and a.design_freeze_name and re.fullmatch(r'[A-Za-z0-9._-]+\.json',a.design_freeze_name) and re.fullmatch('[0-9a-f]{64}',a.design_freeze_sha256 or ''),'AUTHORITATIVE_DESIGN_STOP_AND_EXACT_FREEZE')
 design=pinned('inner-guard-design-first/'+a.design_freeze_name,a.design_freeze_sha256)
 for row in design['files']:eq('inner-guard-design-first/'+row['path'],row)
 check('design_frozen_stopped_scope_only',freezeSha256=a.design_freeze_sha256)
 pres=j('adapter-source-preservation-first/preservation-first.json');need(pres['candidateCommit']==COMMIT and pres['parent']=='3e790e555a61a843e801000b371773a134df5c03' and len(pres['sources'])==9,'FIXED_COMMIT')
 need(git('rev-parse',COMMIT+'^').decode().strip()==pres['parent'],'GIT_PARENT');need(set(git('diff-tree','--no-commit-id','--name-only','-r',COMMIT).decode().splitlines())=={s['path'] for s in pres['sources']},'EXACT_NINE_COMMIT_PATHS')
 inv=pinned('adapter-inputs-third.json','86d98bcf92ac77125195f0c4c2acf0931e2204eb0101fc1e150434593df3b4e8');by={s['path']:s for s in inv['sources']}
 for row in pres['sources']:
  b=read('adapter-source-preservation-first/source/'+row['path']);need(len(b)==row['bytes'] and sha(b)==row['sha256'] and git('cat-file','blob',row['blob'])==b and git('show',COMMIT+':'+row['path'])==b,'COMMITTED_SOURCE_COPY_EQUALS_BLOB');need(all(row[k]==by[row['path']][k] for k in ('bytes','sha256')),'NINE_SOURCE_IN_COMPILE_INVENTORY')
 need(sha(read('adapter-source-preservation-first/candidate.patch'))==pres['patchSha256'],'FULL_PATCH');sync=j('adapter-candidate-sync-first.json');need(sync['candidateCommit']==COMMIT and sync['actualExit']==0 and sync['parentReaped'] is True,'ACTUAL_PUSH_RECEIPT');check('nine_immutable_git_blobs_full_patch_compile_inventory_and_sync',commit=COMMIT,sourceCount=9)
 producers={}
 for dirname,expected in [('adapter-build-first',101),('adapter-build-second',0),('adapter-build-third',0),('adapter-clippy-first',0),('adapter-clippy-second',0)]:
  r=j(dirname+'/report.json');need(r['actualExit']==expected and r['originalParentReaped'] is True and r['sourceUnchanged'] is True and r['sourcesBefore']==r['sourcesAfter'] and r['budgetExceeded'] is False,'ORIGINAL_BUILD_RETURN')
  out=read(dirname+'/cargo.stdout.jsonl');err=read(dirname+'/cargo.stderr.log');need(out is not None and err is not None,'RAW_LOGS_PRESERVED');producers[dirname]=r
 need(b'sealed_package_sha' in read('adapter-build-first/cargo.stderr.log'),'ORIGINAL_FIRST_TEST_FIELD_FAILURE')
 build=producers['adapter-build-third'];clippy=producers['adapter-clippy-second'];need(build['sourcesBefore']==clippy['sourcesBefore']==inv['sources'] and build['frozenInventorySha256']==clippy['frozenInventorySha256']==inputs['adapter-inputs-third.json']['sha256'],'FINAL_PRODUCER_SOURCE_BINDING');need(clippy['argv'][-3:]==['--','-D','warnings'],'STRICT_WARNINGS');check('actual_gnu_build_and_strict_clippy_with_first101_preserved',finalBuild0=True,finalClippy0=True,firstActualExit=101)
 review=j('adapter-semantic-review-first/review-second.json');need(review['blockingFindings']==[] and review['sourceInventory']['sha256']==inputs['adapter-inputs-third.json']['sha256'] and len(review['closedFindings'])==2,'REVIEW_FIXES_BOUND');j('adapter-semantic-review-first/review-first-draft.json');py=j('setup-seal-implementation-first/tests-first-operation.json');need(py['actualExit']==0 and py['originalParentReaped'] is True,'PURE_PYTHON_TESTS');eq('setup-seal-implementation-first/tests-first.stdout.log',py['stdout']);eq('setup-seal-implementation-first/tests-first.stderr.log',py['stderr']);need(b'Ran 9 tests' in read('setup-seal-implementation-first/tests-first.stderr.log'),'NINE_PURE_TESTS');check('two_control_findings_closed_and_nine_python_byte_tests_bound')
 r=pinned('adapter-native-runtime-first/report.json','668911bdd55e00e7a1807d884aee9cce30db13bff445408da5a4bdb7dded3602');need(r['status']=='PASS_ADAPTER_COMPONENT_REGRESSION_NATIVE_TESTS_WIN11_ONLY' and r['sourceInputs']==inv['sources'] and r['candidateSourcesUnchanged'] is True and r['compiledExeUnchanged'] is True,'RUNTIME_SOURCE');eq('adapter-build-third/report.json',r['producer']);eq('adapter-inputs-third.json',r['compileInventory']);need(len(build['outputs'])==1 and all(build['outputs'][0][k]==r['nativeInput'][k] for k in ('bytes','sha256')),'EXE_PRODUCER_BINDING_WITHOUT_BINARY_READ')
 for op in r['operations']:
  need(op['actualReturnCode']==0 and op['originalLocalReaped'] is True and op['remoteOriginalReturnUnknown'] is False and op['budgetExceeded'] is False,'ORIGINAL_TRANSPORT_RETURNS')
  for k in ('stdout','stderr'):eq(str(Path(op[k]['path']).relative_to(R)),op[k])
 wrapper=j('root-adapter-native-runtime-operation-first.json');need(wrapper['actualExit']==0 and wrapper['parentReaped'] is True and json.loads(wrapper['stdout'])['sha256']==inputs['adapter-native-runtime-first/report.json']['sha256'],'ROOT_WRAPPER')
 download=R/'adapter-native-runtime-first/download';roots=list(download.iterdir());need(len(roots)==1 and roots[0].is_dir() and not roots[0].is_symlink(),'ONE_ORIGINAL_DOWNLOAD_ROOT');actual={p.relative_to(roots[0]).as_posix() for p in roots[0].rglob('*') if p.is_file()};need(actual=={s['path'] for s in r['inventory']['files']},'COMPLETE_FOUR_DOWNLOAD_MEMBERS')
 binaryRows=[]
 for row in r['inventory']['files']:
  rel=str((roots[0]/row['path']).relative_to(R))
  if rel.endswith('.exe'):need(regular(R/rel).stat().st_size==row['bytes'],'BINARY_METADATA_SIZE_ONLY');binaryRows.append({'path':rel,'bytes':row['bytes'],'sha256':row['sha256']})
  else:eq(rel,row)
 native=j(str((roots[0]/'native-result.json').relative_to(R)));need(native==r['native']==j('adapter-native-runtime-first/run.stdout.log'),'NATIVE_DOWNLOAD_REPLAY')
 need(native['controlledNativeReturns']=='CFG_TEST_SYNTHETIC_BEGIN_END_OBSERVATIONS_ONLY' and native['productionMsiAdapter']=='NOT_EXERCISED' and native['originalWholeSeconds']==600,'NATIVE_COMPONENT_SCOPE')
 cases=[]
 for case,count in zip(native['cases'],[6,54]):
  cap=case['original'];need(cap in native['originalObjects'] and type(cap['Wait']) is int and cap['Wait']==0 and type(cap['ExitCode']) is int and cap['ExitCode']==case['exitCode']==0 and cap['ExitKnown'] is True and cap['CheckedClose'] is True and cap['Status']=='HELD_ORIGINAL_IDENTITY' and cap['Pid']==case['pid'] and cap['Creation']>0 and cap['Sha256']==r['nativeInput']['sha256'] and cap['Image']==r['windowsRoot']+'\\candidate-tests.exe' and cap['Sid']=='S-1-5-21-3896111207-1867804362-1971593122-1001','ORIGINAL_PARENT_CAP')
  summaries=re.findall(r'^test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out;',case['stdout'],re.M);need(summaries and tuple(map(int,summaries[-1][:4]))==(count,0,0,0),'FINAL_OUTER_SUMMARY');need(case['stderr']=='' and case['actualExit'] is True and case['endedWholeMilliseconds']<=case['originalDeadlineWholeMilliseconds'],'NATIVE_CASE_RETURN');cases.append({'name':case['name'],'outerPassed':count,'nestedSummaries':len(summaries)-1,'originalExit':0,'checkedClose':True})
 need(len(native['cases'])==len(native['originalObjects'])==2 and r['cleanup']=='PASS_EXACT_DOWNLOADED_OWNED_SCRATCH_ONLY' and r['delete']['knownAbsent'] is True and r['ownedProcessReadback']['count']==0,'EXACT_NATIVE_CLEANUP');check('win11_component_original_caps_final_summaries_complete_download_and_cleanup',cases=cases,scopeOverlap='54 includes6/11effects/2newparser tests; do not sum',downloadMembers=4)
 ast1=j('root-adapter-native-ps-parse-first.json');ast2=j('root-adapter-native-ps-parse-second.json');need(ast1['actualExit']==1 and ast2['actualExit']==0 and ast1['parentReaped'] is ast2['parentReaped'] is True and all(x['errors']==[] for x in ast2['native']['rows']),'ORIGINAL_ACTUAL_PS_AST_FAILURE_AND_SUCCESS')
 for n in ('run-native.py','run.ps1'):
  x=read('adapter-native-preparation-first/'+n);y=read('adapter-native-preparation-second/'+n);need(len(x)==len(y) and sum(a!=b for a,b in zip(x,y))==1,'SINGLE_CHARACTER_CORRECTION')
 for n in ('create.ps1','delete.ps1','boot.ps1','check-qualification.py'):need(read('adapter-native-preparation-first/'+n)==read('adapter-native-preparation-second/'+n),'UNCHANGED_PS_AND_QUALIFIER')
 audit=pinned('windows-readonly-adapter-first.json','df246bee9cffaf236492554a199b210fbbee357f8af2bf9a24e80260c9a06a61');need(audit['status']=='PASS' and audit['scratchDirectories']==audit['ownedProcesses']==audit['ownedTasks']==[] and audit['publicAuthorizationUnchanged'] is True and audit['projectRunUnchanged'] is True and audit['sshd']=='Running' and audit['persistentKeyMode']=='0o600','FINAL_READONLY_AUDIT');check('ps_ast1_and_one_character_ast0_and_final_readonly_audit')
 # Bound all selected text originals. Never scan active W, target or probe dirs.
 files=[R/x for x in ROOT_FILES]
 for name in DIRECTORIES:
  for p in sorted((R/name).rglob('*')):
   if p.is_dir():need(not p.is_symlink(),'NO_DIRECTORY_LINK');continue
   need('__pycache__' not in p.parts,'NO_UNPLANNED_CACHE_MEMBER');files.append(p)
 rows=[];excluded=[];seen=set()
 for p in sorted(files):
  rel=p.relative_to(R).as_posix();need(rel not in seen,'UNIQUE_MEMBERS');seen.add(rel)
  if p.suffix.lower()=='.exe':
   need(rel in {x['path'] for x in binaryRows}|{'adapter-native-runtime-first/upload/candidate-tests.exe'},'ONLY_ORIGINAL_NATIVE_EXE_IDENTITIES');need(regular(p).stat().st_size==r['nativeInput']['bytes'],'BINARY_SIZE_METADATA_ONLY');excluded.append({'path':rel,'bytes':r['nativeInput']['bytes'],'sha256':r['nativeInput']['sha256'],'source':'original runtime/producer digest metadata; archiver does not read binary contents'});continue
  b=read(rel);rows.append(inputs[rel])
 need(len(rows)<=350 and sum(x['bytes'] for x in rows)<=16*1024*1024 and len(excluded)==2,'SMALL_EXPLICIT_BATCH')
 manifest={'schemaVersion':1,'status':'EXACT_FINITE_ADAPTER_COMPONENT_ORIGINALS_NOT_YET_ARCHIVED','sourceCommit':COMMIT,'originals':rows,'originalCount':len(rows),'originalBytes':sum(x['bytes'] for x in rows),'excludedBinaryIdentities':excluded,'scope':'Only root-authorized fixed component directories/files. No W/target/cache/private-key/probe reads.','designFreezeSha256':a.design_freeze_sha256}
 acceptance={'schemaVersion':1,'status':'PASS_FINITE_R80_ADAPTER_COMPONENT_BATCH_NOT_PRODUCTION_COUPLED_FLOW','sourceCommit':COMMIT,'checks':checks,'coreInputs':[inputs[x] for x in ['adapter-source-preservation-first/preservation-first.json','adapter-inputs-third.json','adapter-build-third/report.json','adapter-clippy-second/report.json','adapter-native-runtime-first/report.json','windows-readonly-adapter-first.json','adapter-candidate-sync-first.json']],'preservedFailures':['first GNU actual101 missing cfg(test) Session field; original stdout/stderr retained','first semantic draft two control-order findings retained; closed only by third source static review','first actual PS AST1 comma error; exact1byte semicolon correction then second actualAST0; original streams retained','setup-seal local git diff new-file actual1 wrapper failure retained separately from passing9 pure tests','pending/preparation NOT_RUN evidence kept as contemporaneous original'],'qualifiedScopes':{'purePythonByteTests':9,'Win11Controlled':6,'Win11Lifecycle':54,'scopeOverlap':'54 includes6,11effects,2new pure parsers','actualNativeCoupledBeginFlow':'NOT_RUN','genuineInnerGuard':'NOT_IMPLEMENTED_IN_THIS_BATCH','publicSetupEntry':'NOT_IMPLEMENTED_IN_THIS_BATCH','Windows7':'NOT_RUN','Windows10':'NOT_RUN'},'productionAdmission':False,'fullGoalComplete':False,'archiveScope':'Components and pure sealing functions only; no production release','reviewerActions':{'Windows':False,'Cargo':False,'sourceWrite':False,'keyRead':False,'binaryContentRead':False,'commit':False,'deletion':False}}
 for name,obj in [('source-manifest-first.json',manifest),('acceptance-first.json',acceptance)]:
  with (D/name).open('xb') as f:f.write((json.dumps(obj,ensure_ascii=False,indent=2)+'\n').encode())
 print(json.dumps({'status':acceptance['status'],'originalCount':len(rows),'originalBytes':manifest['originalBytes'],'binaryIdentitiesOnly':2,'manifestSha256':sha((D/'source-manifest-first.json').read_bytes()),'acceptanceSha256':sha((D/'acceptance-first.json').read_bytes())}))
if __name__=='__main__':main()
