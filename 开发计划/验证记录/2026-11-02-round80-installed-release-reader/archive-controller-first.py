"""Finite reader/probe evidence archive. Default inert; no Windows/build/Git."""
from pathlib import Path
import argparse,ast,gzip,hashlib,io,json,tarfile
M=Path('/workspace/共享/Eliora/未来教室计划');R=Path('/tmp/fc-round80-controlled-repair');D=M/'开发计划/验证记录/2026-11-02-round80-installed-release-reader'
BACKEND='archive-preparation-second/archive-second.py';BACKEND_SHA='da2906c450ea8c2ac7d18e358507023f519dbcc6faf1a9ac1abb1ae16af8c30f'
PROMOTE=['installed-release-reader-implementation-first/freeze-first.json','installed-release-db-implementation-first/freeze-first.json','installed-release-db-implementation-second/freeze-first.json','installed-release-db-implementation-second/summary-capacity-correction-first.json','installed-release-db-implementation-second/build-report-first.json','installed-release-db-implementation-second/build-report-second.json','installed-release-db-runtime-first/report.json','installed-release-db-native-local-checks-first/report.json','installed-release-db-native-local-checks-second/report.json','installed-release-db-native-preparation-second/freeze-first.json','installed-release-db-native-runtime-first/report.json','root-installed-release-db-native-two-runs-review-first.json']
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
 need(a.execute!=a.replay_only,'ONE_MODE');need(Path(__file__).resolve()==D/'archive-controller-first.py','EXACT_CONTROLLER')
 manifest=json.loads((D/'source-manifest-first.json').read_bytes());rows=manifest['originals'];ids=manifest['identityOnly'];need(len(rows)==312 and sum(r['bytes'] for r in rows)==4188991 and len(ids)==10,'FINITE_SELECTION')
 scan,stats,jsonl,ns=scanner(rows)
 if a.replay_only:
  index=json.loads((D/'archive-index-first.json').read_bytes());archive=verify_tar(manifest,scan);b=archive.read_bytes();need(len(b)==index['archive']['bytes'] and sha(b)==index['archive']['sha256'],'EXACT_ARCHIVE_BYTES')
  for p in sorted(D.rglob('*')):
   need(not p.is_symlink(),'NO_SYMLINK')
   if p.is_file() and p!=archive:scan(p.read_bytes(),('promoted:' if str(p.relative_to(D)) in PROMOTE else 'destination:')+str(p.relative_to(D)))
  print(json.dumps(dict(status='PASS_FULL_STORED_TAR_PROMOTIONS_STRICT_SCAN',members=len(rows),originalBytes=sum(r['bytes'] for r in rows),archiveSha256=sha(b),findings=[],statistics=stats,jsonlRecords=jsonl[0],Windows=False,Cargo=False,Git=False)));return
 selector=(D/'select-first.py').read_bytes();sn={'__name__':'selector_only'};exec(compile(selector,'<selector>','exec'),sn);fresh=sn['collect']();need(fresh==manifest,'CURRENT_SOURCE_SCOPE_EXACT')
 by={r['path']:r for r in rows+ids}
 def original(r):
  b=sn['regular'](R/r['path']).read_bytes();need(len(b)==r['bytes'] and sha(b)==r['sha256'],'ORIGINAL_UNCHANGED '+r['path']);return b
 def js(p):return json.loads(original(by[p]))
 refs=json.loads((D/'upstream-reference-first.json').read_bytes())
 for r in refs['references']:
  p=M/r['path'];need(not p.is_symlink(),'UPSTREAM_REGULAR');b=p.read_bytes();need(len(b)==r['bytes'] and sha(b)==r['sha256'],'FROZEN_UPSTREAM_IDENTITY')
 # Verify all listed frozen evidence, including nested freeze filenames as regular data.
 for dirname,key in [('installed-release-reader-implementation-first','files'),('installed-release-db-implementation-first','evidenceFiles'),('installed-release-db-implementation-second','files'),('installed-release-db-runtime-preparation-first','files'),('installed-release-db-native-preparation-first','files'),('installed-release-db-native-preparation-second','files'),('installed-release-db-native-local-checks-second','files')]:
  f=js(dirname+'/freeze-first.json')
  for entry in f[key]:
   rel=entry['path'];rel=str(Path(rel).relative_to(R)) if Path(rel).is_absolute() else dirname+'/'+rel;r=by[rel];need(r['bytes']==entry['bytes'] and r['sha256']==entry['sha256'],'EVERY_FROZEN_EVIDENCE_BYTE')
 first=js('installed-release-db-implementation-second/build-report-first.json');second=js('installed-release-db-implementation-second/build-report-second.json')
 for p in [first,second]:
  need(p['sourceBase']=='a4702b56bff66da5f5cbe74551d8751eb33fa520' and p['target']=='i686-pc-windows-gnu' and p['sourceUnchanged'] and p['sourceBefore']==p['sourceAfter'] and len(p['sourceBefore'])==467,'BOTH_PRODUCERS_COMPLETE_UNCHANGED_RUST_DOMAIN')
  need(p['compilerArtifact']['profile']['test'] is True and p['compilerArtifact']['features']==['default'] and all(o['actualExit']==0 and o['parentReaped'] for o in p['operations']),'ACTUAL_GNU_CFGTEST_ARTIFACT_NOT_NORMAL_MSVC_PRODUCT')
  for o in p['operations']:
   for stream in ['stdout','stderr']:
    f=o[stream];rel=str(Path(f['path']).relative_to(R));r=by[rel];need(r['bytes']==f['bytes'] and r['sha256']==f['sha256'],'FULL_COMPILER_METADATA_OUTPUT')
  f=p['executable'];r=by[str(Path(f['path']).relative_to(R))];need(r['bytes']==f['bytes'] and r['sha256']==f['sha256'],'ACTUAL_PRESERVED_PE_IDENTITY')
 need(first['executable']['sha256']=='d317e26f8e554145a3d38253d858881176e6fa339e3e6473482b5c757ed25f93' and second['executable']['sha256']=='5af49451fe8a716993f558c5f374095337bfdd6e27a7bed5db8a5594807edc45','FIRST_AND_CORRECTED_PE_DISTINCT')
 correction=js('installed-release-db-implementation-second/summary-capacity-correction-first.json');need(correction['oldNativeExecution']=='NOT_RUN' and correction['oldProducer']['sha256']==by['installed-release-db-implementation-second/build-report-first.json']['sha256'],'SUMMARY_BUFFER_FIX_BEFORE_NATIVE_NOT_FAKE_FIRST_FAILURE')
 pos=js('installed-release-db-runtime-first/report.json');reg=js('installed-release-db-native-runtime-first/report.json');review=js('root-installed-release-db-native-two-runs-review-first.json')
 need(by['installed-release-db-runtime-first/report.json']['sha256']=='c477f6777ba72ac9d100370602fadc09e770b167e206818b0d539485ee25ce95' and by['installed-release-db-native-runtime-first/report.json']['sha256']=='353637662fd9fbb3ed02d90608439ab3678c172e5ac7d2317a36d69bf422f823','EXACT_ACTUAL_REPORTS')
 need(review['positive']['sha256']==by['installed-release-db-runtime-first/report.json']['sha256'] and review['regression']['sha256']==by['installed-release-db-native-runtime-first/report.json']['sha256'] and review['countsAdditive'] is False,'ROOT_INDEPENDENT_TWO_RUN_BINDING')
 need(pos['status']=='PASS_NATIVE_RUST_CACHE_DB_PROBE_AND_MSVC_PRODUCT_EXACT_CLEANUP' and pos['runtimeActualExit']==0 and pos['downloadVerified'] and pos['cleanup']['knownAbsent'],'REAL_INSTALLED_READER_AND_PRODUCT_EXACT_CLEANUP')
 source=original(by['installed-release-db-runtime-preparation-first/run-controlled-product.py']);tree=ast.parse(source);functions=[n for n in tree.body if isinstance(n,ast.FunctionDef) and n.name in ['phase_returned','probe_passed','admission']];need(len(functions)==3,'FROZEN_PURE_POSITIVE_QUALIFIERS');qual={};exec(compile(ast.Module(body=functions,type_ignores=[]),'<frozen-pure-positive-qualifiers>','exec'),qual);need(qual['admission'](pos['phase']),'ALL_ORIGINAL_POSITIVE_CAPS_AND_EFFECTS')
 need(pos['phase']['elapsedWholeMilliseconds']==4672 and pos['phase']['setupInternalSeconds']==120 and pos['phase']['wholeMilliseconds']==600000,'ORIGINAL_POSITIVE_TIME_SCOPE')
 need(reg['status']=='PASS_GNU_DEFAULT_CONTROLLED6_LIFECYCLE74_NATIVE_REGRESSION_WIN11_ONLY' and reg['delete']['knownAbsent'] and reg['cleanup']=='PASS_EXACT_DOWNLOADED_OWNED_SCRATCH_ONLY' and reg['candidateSourcesUnchanged'] and reg['controllerAndFixturesUnchanged'] and reg['compiledExeUnchanged'],'ACTUAL74_EXACT_CLEANUP_UNCHANGED_SOURCE')
 need(reg['expectedScopes']==dict(controlled=6,lifecycle=74) and reg['nativeInput']['sha256']==second['executable']['sha256'] and reg['sourceInputs']==second['sourceBefore'],'SAME_CORRECTED_PE_AND_COMPILER_DOMAIN')
 native=reg['native'];need(native['originalParentReturned'] is True and native['elapsedMilliseconds']==1234 and len(native['cases'])==2,'ACTUAL_PARENT_NATIVE_RANGE')
 for c,required in zip(native['cases'],[6,74]):
  o=c['original'];need(c['actualExit'] and c['exitCode']==0 and o['Wait']==0 and o['ExitKnown'] and o['ExitCode']==0 and o['CheckedClose'] and o['Sha256']==second['executable']['sha256'],'TWO_REAL_PARENT_CAPS0_CHECKEDCLOSE')
  n=next(x for x in reg['qualifiedCases'] if x['name']==c['name']);need(n['outerTotals']['passed']==required and all(n['outerTotals'][k]==0 for k in ['failed','ignored','measured']) and n['expectedOuterStarts']==required,'CONTROLLED6_IS_SUBSET_OF74_PARENT_ONLY')
  need(c['stdout'].splitlines()[-2].startswith('test result: ok. '+str(required)+' passed;'),'FINAL_PARENT_SUMMARY_NOT_CHILD')
 for report,name,count in [(pos,'installed-release-db-runtime-first',31),(reg,'installed-release-db-native-runtime-first',4)]:
  need(len(report['inventory']['files'])==count,'COMPLETE_RUNTIME_INVENTORY_COUNT');rootname=report['windowsRoot'].split('\\')[-1]
  for f in report['inventory']['files']:
   r=by[name+'/download/'+rootname+'/'+f['path']];need(r['bytes']==f['bytes'] and r['sha256']==f['sha256'],'EVERY_DOWNLOADED_ORIGINAL_BYTE')
 probeout=next(r for r in rows if r['path'].endswith('/rust-db-probe.stdout.log'));s=original(probeout).decode();need('msi_closes=41 claims=0 retained=0 faulted=false' in s and 'test result: ok. 1 passed; 0 failed; 0 ignored;' in s,'REAL_RUST_PROBE41_INTERNAL_ASSERTIONS_NOT_JSON_API_LEDGER')
 need(js('installed-release-db-native-local-checks-first/report.json')['actualExit']==1 and js('installed-release-db-native-local-checks-second/report.json')['actualExit']==0,'FIRST_LOCAL_QUALIFICATION_FAIL_PRESERVED')
 for name,count in [('root-installed-release-db-ps-parse-first.json',8),('root-installed-release-db-native-ps-parse-first.json',6)]:
  r=js(name);need(r['actualExit']==0 and len(r['native']['rows'])==count and all(x['errors']==[] for x in r['native']['rows']),'ACTUAL_PS51_AST_ONLY')
 for r in rows:scan(original(r),'source:'+r['path'])
 for r in ids:original(r)
 for p in sorted(D.iterdir()):
  if p.is_file():scan(p.read_bytes(),'preparation:'+p.name)
 archive=D/'evidence-originals-first.tar.gz'
 with archive.open('xb') as out:
  with gzip.GzipFile(filename='',mode='wb',fileobj=out,mtime=0,compresslevel=9) as gz:
   with tarfile.open(fileobj=gz,mode='w',format=tarfile.PAX_FORMAT) as t:
    for r in rows:
     b=original(r);info=tarfile.TarInfo(r['path']);info.size=len(b);info.mode=0o600;info.uid=info.gid=info.mtime=0;info.uname=info.gname='';t.addfile(info,io.BytesIO(b))
 for name in PROMOTE:
  q=D/name;q.parent.mkdir(parents=True,exist_ok=True);q.write_bytes(original(by[name]))
 verify_tar(manifest,scan)
 for r in rows+ids:original(r)
 index=dict(status='PASS_FINITE_FROZEN_READER_TWO_GNU_PRODUCERS_WIN11_NATIVE_POSITIVE_AND_74_ARCHIVE',sourceBase=manifest['sourceBase'],archive=dict(path=archive.name,bytes=archive.stat().st_size,sha256=sha(archive.read_bytes()),members=len(rows),originalBytes=sum(r['bytes'] for r in rows)),originals=len(rows),identityOnly=len(ids),manifest=dict(path='source-manifest-first.json',sha256=sha((D/'source-manifest-first.json').read_bytes())),promoted=PROMOTE,upstream='D3 official MSVC product CI;D4 normal product;D5 separate postwrite fixture and independent raw review; upstream fullbodies not duplicated',scope=dict(implementation='Frozen W3/W4/W5 complete snapshots/patches/failures/corrected tests;Summary input39/output38 correction before first actual native',producers='Two real GNU cfgtest releases,467 unchangedRustCargo inputs each, default feature/profiletesttrue; first preservedPE d317 and corrected5af4; no newMSVCreader validation',positive='Official a470 Win7MSVC normal product install0→separate GNU cfgtest actual Rust cached database reader0→uninstall0;31files full download/exactnonce cleanup',msiCloseEvidence='41 internal actual Rust assertions; not serialized per-API JSON ledger; source/debugstdout preserved',regression='GNU default controlled6 then lifecycle74 originalparent0/checkedclose;74 includes6;three nested child summaries not added; explicit installed test skipped here because independently actualpassed above;4files full download/exactcleanup',positiveNativeMilliseconds=4672,positiveOuterSeconds=pos['elapsedSeconds'],regressionNativeMilliseconds=1234,regressionOuterSeconds=reg['elapsedSeconds'],Upgrade='NOT_AUTHORIZED_NOT_EXERCISED',newMSVCReader='NOT_VALIDATED',Win7OS='NOT_RUN',Win10OS='NOT_RUN',releaseReadiness='NOT_GRANTED'),credentials=dict(findings=[],backendMember=BACKEND,backendSha256=BACKEND_SHA,structuralForms='Only one exact path/size/SHA-bound UTF16 MSI DigitalSignature note; raw/boundary/concatenated views; no generic control tolerance'),excluded=manifest['excluded'],WindowsContacted=False,Cargo=False,Git=False,sourceDeletion=False)
 scan(raw(index),'new-index');(D/'archive-index-first.json').write_bytes(raw(index))
 verification=dict(status='PASS_ALL_ORIGINALS_FULL_TAR_REPLAY_AND_STRICT_CREDENTIAL_SCAN',archiveSha256=index['archive']['sha256'],originalsChecked=len(rows),identityOnlyChecked=len(ids),fullTarMembersReplayed=len(rows),promotedRawFiles=len(PROMOTE),credentialScan=dict(status='PASS_NO_MATCHES',findings=[],statistics=stats,jsonlRecords=jsonl[0],patterns=list(ns['PATTERNS']),coverage='Complete raw source/full decompressed tar/raw promotions/preparation/controller/manifest/index;UTF8/UTF16/CP936/CLIXML/JSON/JSONL/nestedbase64;one exact MSI structural form'),originalFailuresAndSchemasPreserved=True,Windows=False,Cargo=False,Git=False)
 scan(raw(verification),'new-verification');(D/'archive-verification-first.json').write_bytes(raw(verification));print(json.dumps(dict(status='PASS',archive=index['archive'],verificationSha256=sha(raw(verification)))))
if __name__=='__main__':main()
