"""Finite reader/probe evidence archive. Default inert; no Windows/build/Git."""
from pathlib import Path
import argparse,ast,gzip,hashlib,io,json,tarfile
M=Path('/workspace/共享/Eliora/未来教室计划');R=Path('/tmp/fc-round80-controlled-repair');D=M/'开发计划/验证记录/2026-11-02-round80-installed-release-layout'
BACKEND='archive-preparation-second/archive-second.py';BACKEND_SHA='da2906c450ea8c2ac7d18e358507023f519dbcc6faf1a9ac1abb1ae16af8c30f'
PROMOTE=['installed-release-layout-implementation-first/freeze-first.json','installed-release-layout-implementation-first/implementation-first.json','installed-release-layout-implementation-first/build-report-first.json','installed-release-layout-implementation-first/build-report-second.json','installed-release-layout-runtime-preparation-first/freeze-first.json','installed-release-layout-runtime-first/report.json','installed-release-layout-archive-preparation-first/positive-independent-review-first.json','installed-release-layout-native-preparation-first/freeze-first.json','installed-release-layout-native-local-checks-first/report.json','installed-release-layout-native-runtime-first/report.json','installed-release-layout-archive-preparation-first/native-independent-review-first.json','installed-release-layout-source-preservation-first/report.json','installed-release-layout-source-preservation-first/freeze-first.json','root-installed-release-layout-ps-parse-first.json','root-installed-release-layout-native-ps-parse-first.json']
def need(v,m):
 if not v:raise AssertionError(m)
def sha(b):return hashlib.sha256(b).hexdigest()
def raw(x):return (json.dumps(x,ensure_ascii=False,indent=2)+'\n').encode()
def scanner(rows):
 with tarfile.open(M/'开发计划/验证记录/2026-11-02-round79-msi-maintenance/evidence-originals-first.tar.gz','r:gz') as t:b=t.extractfile(BACKEND).read()
 need(sha(b)==BACKEND_SHA,'EXACT_SCANNER_SOURCE');ns={'__name__':'strict_scanner_only'};exec(compile(b,'<committed-scanner>','exec'),ns)
 forms=json.loads((D/'text-forms-first.json').read_bytes())['files'];by={r['path']:r for r in rows};need(len(forms)==1,'ONE_EXACT_MSI_CONTROL_DIAGNOSTIC')
 for f in forms:
  r=by[f['path']];need(r['bytes']==f['bytes'] and r['sha256']==f['sha256'] and f['textForm']=='EXACT_UTF16_MSI_DIGITAL_SIGNATURE_STREAM_NOTE','EXACT_PATH_SIZE_SHA_FORM')
 ns['TEXT_FORMS']={f['path']:f for f in forms};need(ns['TEXT_DERIVED_VIEWS']=={},'NO_GENERIC_CONTROL_TOLERANCE')
 stats={k:0 for k in ('textViews','utf16Views','cp936Views','clixmlDecodedViews','decodedBase64Views','jsonContainers','decodedJsonStringViews')};findings=[];jsonl=[0]
 def scan(b,label):
  ns['credential_scan'](b,label,stats,findings)
  if label.endswith('.jsonl'):
   for i,line in enumerate(b.splitlines()):
    if not line.strip():continue
    need(i<250000,'JSONL_BOUND');json.loads(line);jsonl[0]+=1;ns['credential_scan'](line,label+'::jsonl/'+str(i+1),stats,findings)
  need(not findings,'CREDENTIAL_FINDINGS_PATH_KIND_ONLY '+json.dumps(findings))
 return scan,stats,jsonl,ns

def verify_tar(manifest,scan):
 rows=manifest['originals'];archive=D/'evidence-originals-first.tar.gz'
 with tarfile.open(archive,'r:gz') as t:
  members=t.getmembers();need([m.name for m in members]==[r['path'] for r in rows],'EXACT_TAR_MEMBERSHIP')
  for m,r in zip(members,rows):
   need(m.isfile() and m.mode==0o600 and m.uid==m.gid==m.mtime==0 and not m.uname and not m.gname,'REGULAR_DETERMINISTIC_TAR')
   b=t.extractfile(m).read();need(len(b)==r['bytes'] and sha(b)==r['sha256'],'TAR_COMPLETE_BYTES '+r['path']);scan(b,'stored-tar-gzip-decoded:'+r['path'])
 for name in PROMOTE:
  row=next(r for r in rows if r['path']==name);b=(D/name).read_bytes();need(len(b)==row['bytes'] and sha(b)==row['sha256'],'PROMOTED_RAW_BYTES');scan(b,'promoted:'+name)
 return archive

def main():
 ap=argparse.ArgumentParser();ap.add_argument('--execute',action='store_true');ap.add_argument('--replay-only',action='store_true');a=ap.parse_args()
 if not(a.execute or a.replay_only):print(json.dumps(dict(status='PREPARED_NOT_EXECUTED',Windows=False,Cargo=False,Git=False)));return
 need(a.execute!=a.replay_only,'ONE_MODE');need(Path(__file__).resolve()==D/'archive-controller-second.py','EXACT_CONTROLLER')
 manifest=json.loads((D/'source-manifest-first.json').read_bytes());rows=manifest['originals'];ids=manifest['identityOnly'];need(len(rows)==252 and sum(r['bytes']for r in rows)==4163284 and len(ids)==13,'FINITE_EXACT_SOURCE_SELECTION')
 scan,stats,jsonl,ns=scanner(rows)
 if a.replay_only:
  index=json.loads((D/'archive-index-first.json').read_bytes());archive=verify_tar(manifest,scan);b=archive.read_bytes();need(len(b)==index['archive']['bytes'] and sha(b)==index['archive']['sha256'],'EXACT_ARCHIVE_CONTAINER')
  for p in sorted(D.rglob('*')):
   need(not p.is_symlink(),'NO_SYMLINK')
   if p.is_file() and p!=archive:scan(p.read_bytes(),('promoted:'if str(p.relative_to(D))in PROMOTE else 'destination:')+str(p.relative_to(D)))
  print(json.dumps(dict(status='PASS_FULL_STORED_TAR_PROMOTIONS_STRICT_SCAN',members=len(rows),originalBytes=sum(r['bytes']for r in rows),archiveSha256=sha(b),findings=[],statistics=stats,jsonlRecords=jsonl[0],Windows=False,Cargo=False,Git=False)));return
 sn={'__name__':'selector_only','__file__':str(D/'select-first.py')};exec(compile((D/'select-first.py').read_bytes(),'<finite-selector>','exec'),sn);need(sn['collect'](manifest['expectedNativeReportSha256'])==manifest,'EXACT_UNCHANGED_CURRENT_SOURCE_SELECTION')
 by={r['path']:r for r in rows+ids}
 def original(r):
  b=sn['regular'](R/r['path']).read_bytes();need(len(b)==r['bytes'] and sha(b)==r['sha256'],'ORIGINAL_UNCHANGED '+r['path']);return b
 def js(p):return json.loads(original(by[p]))
 for r in json.loads((D/'upstream-reference-first.json').read_bytes())['references']:
  b=(M/r['path']).read_bytes();need(len(b)==r['bytes'] and sha(b)==r['sha256'],'EXACT_FROZEN_UPSTREAM_REFERENCE')
 for dirname in ['installed-release-layout-implementation-first','installed-release-layout-runtime-preparation-first','installed-release-layout-native-preparation-first','installed-release-layout-native-local-checks-first','installed-release-layout-source-preservation-first']:
  f=js(dirname+'/freeze-first.json')
  for entry in f['files']:
   rel=entry['path'];rel=str(Path(rel).relative_to(R))if Path(rel).is_absolute()else dirname+'/'+rel;r=by[rel];need(r['bytes']==entry['bytes']and r['sha256']==entry['sha256'],'EVERY_FROZEN_EVIDENCE_BYTE')
 first=js('installed-release-layout-implementation-first/build-report-first.json');second=js('installed-release-layout-implementation-first/build-report-second.json')
 for p in [first,second]:
  need(p['sourceBase']=='a4702b56bff66da5f5cbe74551d8751eb33fa520' and p['target']=='i686-pc-windows-gnu' and p['sourceBefore']==p['sourceAfter'] and p['sourceUnchanged'] and len(p['sourceBefore'])==467,'BOTH_COMPLETE_UNCHANGED_RUST_CARGO_DOMAINS')
  need(p['compilerArtifact']['profile']['test']is True and p['compilerArtifact']['features']==['default'] and all(o['actualExit']==0 and o['parentReaped']for o in p['operations']),'ACTUAL_GNU_CFGTEST_PRODUCERS')
  for o in p['operations']:
   for stream in ['stdout','stderr']:
    f=o[stream];r=by[str(Path(f['path']).relative_to(R))];need(r['bytes']==f['bytes']and r['sha256']==f['sha256'],'COMPILER_METADATA_RAW_OUTPUT_BINDING')
  f=p['executable'];r=by[str(Path(f['path']).relative_to(R))];need(r['bytes']==f['bytes']and r['sha256']==f['sha256'],'BOTH_PRESERVED_NATIVE_PE_IDENTITIES')
 need(first['executable']['sha256']=='97afd38afe3d7e88d239e207fa1c11ae04a389ee9ef88bdf5c370f2ce1d0c07c' and second['executable']['sha256']=='7ff23691903222ff7d1dce3e592aa12db6e9236cf42c72ec26252b91d439659c','FINAL_ASCII_REFINEMENT_SECOND_PE')
 finalchecks=js('installed-release-layout-implementation-first/final-checks-second.json');need(all(c['actualExit']==0 for c in finalchecks['checks']) and finalchecks['sourceUnchanged'],'STRICT_CLIPPY_FORMAT_ARCHITECTURE_DIFF_ACTUAL0')
 pos=js('installed-release-layout-runtime-first/report.json');reg=js('installed-release-layout-native-runtime-first/report.json');pr=js('installed-release-layout-archive-preparation-first/positive-independent-review-first.json');nr=js('installed-release-layout-archive-preparation-first/native-independent-review-first.json')
 need(by['installed-release-layout-runtime-first/report.json']['sha256']=='f86ce1ef275fa92f5ebee1d2d10b62c84d3ff62ec2247df86b60ba0e397e8c90' and by['installed-release-layout-native-runtime-first/report.json']['sha256']=='d754896883cfdfb0385890bdc57221b7edab95d5158d73d0fa2c20dc9ee5427d','EXACT_TWO_ACTUAL_RUNTIME_REPORTS')
 need(pr['report']['sha256']==by['installed-release-layout-runtime-first/report.json']['sha256'] and nr['report']['sha256']==by['installed-release-layout-native-runtime-first/report.json']['sha256'],'INDEPENDENT_FINITE_RAW_REVIEW_BINDINGS')
 need(pos['status']=='PASS_NATIVE_RUST_CACHE_DB_PROBE_AND_MSVC_PRODUCT_EXACT_CLEANUP' and pos['runtimeActualExit']==0 and pos['downloadVerified'] and pos['cleanup']['knownAbsent'],'ACTUAL_POSITIVE_AND_EXACT_CLEANUP')
 tree=ast.parse(original(by['installed-release-layout-runtime-preparation-first/run-controlled-product.py']));defs=[n for n in tree.body if isinstance(n,ast.FunctionDef)and n.name in ['phase_returned','probe_passed','admission']];need(len(defs)==3,'EXACT_PURE_POSITIVE_QUALIFIERS');q={};exec(compile(ast.Module(body=defs,type_ignores=[]),'<frozen-pure-positive>','exec'),q);need(q['admission'](pos['phase']),'ALL_ORIGINAL_POSITIVE_CAPS_AND_NATIVE_EFFECTS')
 need(pos['phase']['elapsedWholeMilliseconds']==4797 and pos['phase']['setupInternalSeconds']==120 and pos['phase']['wholeMilliseconds']==600000,'ORIGINAL_POSITIVE_TIME_RANGE')
 need(reg['status']=='PASS_GNU_DEFAULT_CONTROLLED6_LIFECYCLE77_NATIVE_REGRESSION_WIN11_ONLY' and reg['delete']['knownAbsent'] and reg['ownedProcessReadback']['count']==0 and reg['candidateSourcesUnchanged'] and reg['controllerAndFixturesUnchanged'] and reg['compiledExeUnchanged'] and reg['producerCompilerStdoutUnchanged'],'ACTUAL77_CLEANUP_AND_FIXED_INPUTS')
 need(reg['expectedScopes']==dict(controlled=6,lifecycle=77) and reg['nativeInput']['sha256']==second['executable']['sha256'] and reg['sourceInputs']==second['sourceBefore'],'SAME_FINAL_TEST_ARTIFACT_AND467_SOURCE_ROWS')
 need(reg['featureScope']=='THREE_TESTS_NOT_COMPILED_DEFAULT_ONLY' and reg['compiledScopeEnvironment']=='UNSET_BY_FROZEN_BUILD_CONTROLLER_AND_MODULE_NOT_COMPILED','NO_RECOVERY_FEATURE_OR_SCOPE_AUTHORITY')
 source=original(by['installed-release-layout-native-preparation-first/run-native.py']);t=ast.parse(source);defs=[n for n in t.body if isinstance(n,ast.FunctionDef)and n.name=='qualify_harness'];qual={'re':__import__('re')};exec(compile(ast.Module(body=defs,type_ignores=[]),'<frozen-original-harness-qualifier>','exec'),qual)
 declarations=js('installed-release-layout-native-preparation-first/source-declarations-first.json');need(len(declarations['raw'])==81 and len(declarations['parentNames'])==77 and len(declarations['excludedFeature'])==3 and len(declarations['explicitSkip'])==1 and set(declarations['controlledSubset'])<=set(declarations['parentNames']),'RAW81_TO77_EXACT_DECLARATION_PROOF')
 qualified=[]
 for c,required_count in zip(reg['native']['cases'],[6,77]):
  required=declarations['controlledSubset']if c['name']=='controlled'else declarations['parentNames'];qualified.append(dict(name=c['name'],**qual['qualify_harness'](c['stdout'],required,required_count)));o=c['original'];need(c['actualExit'] and c['exitCode']==0 and o['Wait']==0 and o['ExitKnown'] and o['ExitCode']==0 and o['CheckedClose'] and o['Sha256']==second['executable']['sha256'],'ORIGINAL77_PARENT_CAPS_KNOWN0_CHECKEDCLOSE')
 need(qualified==reg['qualifiedCases'] and len(reg['native']['originalObjects'])==2 and all(o['CheckedClose']and o['Wait']==0 and o['ExitKnown']and o['ExitCode']==0 for o in reg['native']['originalObjects']),'PARENT_HARNESS_SUMMARIES_NOT_NESTED_CHILD_COUNTS')
 for report,name,count,dirs in [(pos,'installed-release-layout-runtime-first',31,3),(reg,'installed-release-layout-native-runtime-first',4,2)]:
  need(len(report['inventory']['files'])==count and len(report['inventory']['directories'])==dirs,'EXACT_COMPLETE_DOWNLOAD_COUNTS');rootname=report['windowsRoot'].split('\\')[-1]
  for f in report['inventory']['files']:
   r=by[name+'/download/'+rootname+'/'+f['path']];need(r['bytes']==f['bytes']and r['sha256']==f['sha256'],'EVERY_ORIGINAL_DOWNLOADED_BYTE')
  actual=js(name+'/download/'+rootname+('/product-result.json'if count==31 else '/native-result.json'));need(actual==(report['phase']if count==31 else report['native']),'DOWNLOADED_RAW_RESULT_EQUAL_ORIGINAL')
 text=original(next(r for r in rows if r['path'].endswith('/rust-db-probe.stdout.log'))).decode();need('msi_closes=74 claims=0 retained=0 faulted=false'in text and text.count('CloseReceipt {')==74 and text.count('handle_close_return: 0')==74 and text.count('view_close_return: Some(0)')==8 and text.count('DirectoryRow {')==6 and text.count('CustomActionRow {')==1 and text.count('ExecuteSequenceRow {')==23,'REAL_INTERNAL_ASSERTIONS_DEBUG_STDOUT_NOT_PERAPI_JSON_LEDGER')
 for name,count in [('root-installed-release-layout-ps-parse-first.json',8),('root-installed-release-layout-native-ps-parse-first.json',6)]:
  r=js(name);need(r['actualExit']==0 and len(r['native']['rows'])==count and all(x['errors']==[]for x in r['native']['rows']),'ACTUAL_PS51_AST_ONLY')
 need(js('installed-release-layout-native-local-checks-first/report.json')['actualExit']==0 and js('installed-release-layout-runtime-preparation-first/semantic-check-first.json')['status']=='FAIL_LOCAL_SOURCE_CHUNK_EQUALITY_CAUGHT_BEFORE_FREEZE_OR_WINDOWS','LOCAL_ORIGINAL_NAMESPACE_FAILURE_AND_FINAL_QUALIFICATION_PRESERVED')
 saved=js('installed-release-layout-source-preservation-first/report.json');need(saved['status']=='PASS_SIX_TESTED_LAYOUT_SOURCE_BLOBS_COMMITTED_SYNCED' and saved['commit']==saved['remote']=='1be5e15325f934850eba8f43d158a41cd97fe05d' and saved['parent']==manifest['sourceBase'] and saved['worktreeClean'] and all(o['actualExit']==0 for o in saved['operations']),'SOURCE_BRANCH_COMMIT_REMOTE_RECEIPTS_NOT_MAIN_MERGE')
 compiled={r['path']:r for r in second['sourceBefore']}
 for i,f in enumerate(saved['files']):
  r=by['installed-release-layout-source-preservation-first/blob-'+str(i)+'.stdout.log'];need(r['bytes']==f['bytes']and r['sha256']==f['sha256']and compiled[f['path']]==f,'ALL6_ACTUAL_SAVED_GIT_BLOBS_EQUAL_COMPILED_SOURCE')
 for r in rows:scan(original(r),'source:'+r['path'])
 for r in ids:original(r)
 for p in sorted(D.iterdir()):
  if p.is_file():scan(p.read_bytes(),'preparation:'+p.name)
 archive=D/'evidence-originals-first.tar.gz'
 with archive.open('xb')as out:
  with gzip.GzipFile(filename='',mode='wb',fileobj=out,mtime=0,compresslevel=9)as gz:
   with tarfile.open(fileobj=gz,mode='w',format=tarfile.PAX_FORMAT)as t:
    for r in rows:
     b=original(r);info=tarfile.TarInfo(r['path']);info.size=len(b);info.mode=0o600;info.uid=info.gid=info.mtime=0;info.uname=info.gname='';t.addfile(info,io.BytesIO(b))
 for name in PROMOTE:
  p=D/name;p.parent.mkdir(parents=True,exist_ok=True);p.write_bytes(original(by[name]))
 verify_tar(manifest,scan)
 for r in rows+ids:original(r)
 index=dict(status='PASS_FINITE_W6_LAYOUT_SOURCE_NATIVE_POSITIVE_AND77_ARCHIVE',sourceBase=manifest['sourceBase'],sourceSavedBranch=saved['commit'],sourceSavedParent=saved['parent'],mainMerge='NOT_CLAIMED',archive=dict(path=archive.name,bytes=archive.stat().st_size,sha256=sha(archive.read_bytes()),members=len(rows),originalBytes=sum(r['bytes']for r in rows)),originals=len(rows),identityOnly=len(ids),manifest=dict(path='source-manifest-first.json',sha256=sha((D/'source-manifest-first.json').read_bytes())),promoted=PROMOTE,upstream='D3/D4 official a470MSVCnormal product;D6 W5 foundation/database original41/native74 source and artifacts preserved upstream;not wholesale rescanned',scope=dict(source='Independent W6baseda470+frozenW5 source6files;3newlayout changes;compiled/runtime whileuncommitted;later rootbranch1be5 saved exactlysame6blobs,parenta470,remoteequal;not mainmerge',static='Two realGNUcfgtest producers467beforeafter;finalASCII gate;strictClippy/fmt/architecture12/33/diff actual0;7crate pure rules plus1standalone same-source Unicode property',positive='Fixed a470 official Win7MSVCnormal install0→separate W6GNUcfgtest actual native reader1passed→same-source uninstall0;Directory6/CA1/sequence23+74internalclose assertions/debugtext,not perAPIJSONledger;31file3dir fullreadback and nonceAbsent',regression='Same finalGNUPE controlled6→lifecycle77 originalparent0 checkedclosed;77includes6;81source declarations minus3uncompiledfeature minus1exactprioractualignored probe;3nestedchild summaries notadded;4file2dir exactreadback and nonceAbsent',positiveNativeMilliseconds=4797,positiveOuterSeconds=pos['elapsedSeconds'],regressionNativeMilliseconds=reg['native']['elapsedMilliseconds'],regressionOuterSeconds=reg['elapsedSeconds'],budgets='MSI/kernel64live,8owner,original120s;raw rowsComponent/File72,totalclose ledger256 preacquisitionreserved;no increase',BeginEnd='NOT_DIRECTLY_SERIALIZED_SOURCE_G0_EFFECTS_INFERENCE_ONLY',Upgrade='NOT_AUTHORIZED_NOT_EXERCISED',newMSVCSource='NOT_VALIDATED',carrier73aea='SEPARATE_SOURCE_NOT_INCLUDED_IN77',Win7OS='NOT_RUN',Win10OS='NOT_RUN',releaseReadiness='NOT_GRANTED'),credentials=dict(findings=[],backendMember=BACKEND,backendSha256=BACKEND_SHA,structuralForms='One exact path/size/SHA-bound UTF16 MSI diagnostic only;raw/boundary/concatenated views,not generic controls'),exclusions=manifest['notTraversed'],WindowsContacted=False,Cargo=False,Git=False,sourceDeletion=False)
 scan(raw(index),'new-index');(D/'archive-index-first.json').write_bytes(raw(index));verification=dict(status='PASS_ALL_ORIGINALS_FULL_TAR_REPLAY_AND_STRICT_CREDENTIAL_SCAN',archiveSha256=index['archive']['sha256'],originalsChecked=len(rows),identityOnlyChecked=len(ids),fullTarMembersReplayed=len(rows),promotedRawFiles=len(PROMOTE),credentialScan=dict(findings=[],statistics=stats,jsonlRecords=jsonl[0],patterns=list(ns['PATTERNS']),coverage='All selected rawtext/full decompressed tar/rawpromotions/controller/manifests/index;UTF8/UTF16/CP936/CLIXML/JSON/JSONL/nestedbase64;only1exactMSIcontrol'),originalReportsAndFailuresPreserved=True,Windows=False,Cargo=False,Git=False);scan(raw(verification),'new-verification');(D/'archive-verification-first.json').write_bytes(raw(verification));print(json.dumps(dict(status='PASS',archive=index['archive'],verificationSha256=sha(raw(verification)))))
if __name__=='__main__':main()
