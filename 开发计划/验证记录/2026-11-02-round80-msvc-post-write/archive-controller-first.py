"""Finite completed MSVC product/GNU witness evidence archive. Default inert. No Windows/build/Git."""
from pathlib import Path
import argparse,ast,gzip,hashlib,io,json,re,tarfile
M=Path('/workspace/共享/Eliora/未来教室计划');R=Path('/tmp/fc-round80-controlled-repair');D=M/'开发计划/验证记录/2026-11-02-round80-msvc-post-write'
BACKEND='archive-preparation-second/archive-second.py';BACKEND_SHA='da2906c450ea8c2ac7d18e358507023f519dbcc6faf1a9ac1abb1ae16af8c30f'
PROMOTE=['a470-msvc-post-write-package-first/derivation-report.json','a470-msvc-post-write-operation-first/report.json','a470-msvc-post-write-operation-first/static-review.json','a470-msvc-post-write-runtime-preparation-first/freeze-first.json','a470-msvc-post-write-runtime-preparation-first/semantic-source-review-first.json','a470-msvc-post-write-runtime-first/report.json','root-a470-msvc-post-write-ps-parse-first.json','root-a470-msvc-post-write-runtime-review-first.json']
def need(v,m):
 if not v:raise AssertionError(m)
def sha(b):return hashlib.sha256(b).hexdigest()
def raw(x):return (json.dumps(x,ensure_ascii=False,indent=2)+'\n').encode()
def scanner(rows):
 with tarfile.open(M/'开发计划/验证记录/2026-11-02-round79-msi-maintenance/evidence-originals-first.tar.gz','r:gz') as t:b=t.extractfile(BACKEND).read()
 need(sha(b)==BACKEND_SHA,'EXACT_SCANNER_SOURCE');ns={'__name__':'strict_scanner_only'};exec(compile(b,'<committed-scanner>','exec'),ns)
 forms=json.loads((D/'text-forms-first.json').read_bytes())['files'];by={r['path']:r for r in rows};need(len(forms)==14,'EXACT_FOURTEEN_BOUNDED_STRUCTURAL_FORMS')
 for f in forms:
  parent=f['path'].split('::json',1)[0];r=by[parent]
  need(f.get('parentSha256',f['sha256'])==r['sha256'],'FORM_PARENT_RAW_SHA')
  if '::json' not in f['path']:need(f['bytes']==r['bytes'],'FORM_SIZE')
 ns['TEXT_FORMS']={f['path']:f for f in forms};need(ns['TEXT_DERIVED_VIEWS']=={},'NO_GENERIC_CONTROL_TOLERANCE')
 original_review=ns['reviewed_control_views']
 def reviewed(data,label):
  relative=next((label[len(p):] for p in ('source:','stored-tar-gzip-decoded:','promoted:') if label.startswith(p)),None);f=ns['TEXT_FORMS'].get(relative)
  if f is None or f['textForm']=='EXACT_UTF16_MSI_DIGITAL_SIGNATURE_STREAM_NOTE':return original_review(data,label)
  need(len(data)==f['bytes'] and sha(data)==f['sha256'],'EXACT_BOUND_STRUCTURAL_BYTES');text=data.decode('utf-8');need(text.encode()==data,'EXACT_UTF8_ROUNDTRIP')
  kind=f['textForm']
  if kind=='EXACT_UTF8_MSI_OLE_STREAM_LIST':
   expected='payload.cab\n'+('Binary.FcPostWriteWitness\n' if relative.endswith('derived-streams.stdout.log') else '')+'Binary.FcControlledGuardDll\nBinary.FcGuardClientTemplate\n\x05SummaryInformation\n';need(text==expected,'EXACT_ORDINAL_STREAM_LIST');control='\x05'
  elif kind=='EXACT_UTF8_MSI_FORCE_CODEPAGE_ZERO_TERMINATOR':need(text=='\r\n\r\n0\t_ForceCodepage\r\n\x00','EXACT_FORCE_CODEPAGE_EXPORT');control='\x00'
  elif kind=='EXACT_UTF8_MSI_OLE_SUMMARY_INFORMATION_NAME':need(text=='\x05SummaryInformation','EXACT_SUMMARY_STREAM_ARGV');control='\x05'
  else:raise AssertionError('UNKNOWN_STRUCTURAL_FORM')
  need(text.count(control)==1 and not any(ord(c)<32 and c not in ('\r','\n','\t',control) for c in text),'ONLY_EXACT_STRUCTURAL_CONTROL')
  return [('utf8-reviewed-msi-structure',text),('utf8-reviewed-msi-structure-visible-boundary',text.replace(control,'[U+%04X]'%ord(control))),('utf8-reviewed-msi-structure-concatenated',text.replace(control,''))]
 ns['reviewed_control_views']=reviewed
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
 if not a.execute and not a.replay_only:print(json.dumps(dict(status='PREPARED_NOT_EXECUTED',Windows=False,Cargo=False,Git=False)));return
 need(not(a.execute and a.replay_only),'ONE_MODE');need(Path(__file__).resolve()==D/'archive-controller-first.py','EXACT_CONTROLLER')
 manifest=json.loads((D/'source-manifest-first.json').read_bytes());rows=manifest['originals'];ids=manifest['identityOnly'];need(len(rows)==278 and sum(r['bytes'] for r in rows)==3013146 and len(ids)==23,'FIXED_FINITE_SELECTION')
 scan,stats,jsonl,ns=scanner(rows)
 if a.replay_only:
  index=json.loads((D/'archive-index-first.json').read_bytes());archive=verify_tar(manifest,scan);b=archive.read_bytes();need(len(b)==index['archive']['bytes'] and sha(b)==index['archive']['sha256'],'EXACT_ARCHIVE_CONTAINER')
  for p in sorted(D.rglob('*')):
   need(not p.is_symlink(),'NO_DEST_SYMLINK')
   if p.is_file() and p!=archive:scan(p.read_bytes(),('promoted:' if str(p.relative_to(D)) in PROMOTE else 'destination:')+str(p.relative_to(D)))
  print(json.dumps(dict(status='PASS_COMPLETE_LOCAL_TAR_PROMOTION_REPLAY_AND_STRICT_SCAN',members=len(rows),originalBytes=sum(r['bytes'] for r in rows),archiveSha256=sha(b),findings=[],statistics=stats,jsonlRecords=jsonl[0],Windows=False,Cargo=False,Git=False)));return
 selector=(D/'select-first.py').read_bytes();need(sha(selector)==manifest['selectorSha256'],'SELECTOR_IDENTITY');sn={'__name__':'selector_only'};exec(compile(selector,'<selector>','exec'),sn);fresh,freshids,pruned=sn['collect']();need(fresh==rows and freshids==ids and pruned==manifest['pruned'],'EXACT_CURRENT_MEMBERSHIP')
 by={r['path']:r for r in rows+ids}
 def original(r):
  b=sn['regular'](R/r['path']).read_bytes();need(len(b)==r['bytes'] and sha(b)==r['sha256'],'UNCHANGED_ORIGINAL '+r['path']);return b
 def js(p):return json.loads(original(by[p]))
 refs=json.loads((D/'upstream-reference-first.json').read_bytes())
 for r in refs['references']:
  p=M/r['path'];need(not p.is_symlink(),'UPSTREAM_REGULAR');b=p.read_bytes();need(len(b)==r['bytes'] and sha(b)==r['sha256'],'IMMUTABLE_UPSTREAM_REFERENCE')
 need(next(r for r in refs['references'] if r['path'].endswith('msvc-product/freeze-second.json'))['sha256']=='be8a3d1c0807dbf7bc6e2ee02658b82ea366ecbe66980e6f7d88ec2f9bf8c946','CORRECTED_UPSTREAM_FULL_FREEZE')
 deriv=js('a470-msvc-post-write-package-first/derivation-report.json');operation=js('a470-msvc-post-write-operation-first/report.json');static=js('a470-msvc-post-write-operation-first/static-review.json');runtime=js('a470-msvc-post-write-runtime-first/report.json');review=js('root-a470-msvc-post-write-runtime-review-first.json');ps=js('root-a470-msvc-post-write-ps-parse-first.json')
 need(by['a470-msvc-post-write-package-first/derivation-report.json']['sha256']=='3dd79dd9438c7f800ad647d5c411e49ae9f622b57f39212449fb7b6ab93aeb50','DERIVATION_IDENTITY')
 need(deriv['canonical_product_source']==dict(sourceSha='a4702b56bff66da5f5cbe74551d8751eb33fa520',target='i686-win7-windows-msvc',origin='OFFICIAL_NORMAL_MSVC_MAIN_HELPER_GUARD_DEFAULT_PROFILE_TEST_FALSE') and deriv['auxiliary_witness_source']=='DEDICATED_GNU_C_DIAGNOSTIC_DLL_ONLY_NOT_PRODUCT_OR_PERMISSION','ACTUAL_SOURCE_SPLIT')
 need(deriv['canonical_report_sha256']=='f6b5265bf94dd0b88cd493a6a731a82cfe297cd991e7505d5ca5b811099eb6e5' and len(deriv['commands'])==85 and all(c['actual_exit']==0 and c['parent_reaped'] and not c['timed_out'] for c in deriv['commands']),'ALL_85_ACTUAL_LOCAL_COMMANDS')
 for c in deriv['commands']:
  for stream in ('stdout','stderr'):
   r=by['a470-msvc-post-write-package-first/'+c[stream]];need(r['sha256']==c[stream+'_sha256'],'ACTUAL_COMMAND_FULL_OUTPUT_SHA')
 need(operation['actualExit']==0 and operation['originalParentReaped'] and not operation['timedOut'] and operation['outerBudgetSeconds']==240,'ORIGINAL_STATIC_PRODUCER_TERMINAL')
 need(all(deriv[k] is True for k in ('original_tables_unchanged_except_one_binary_two_ca_two_sequence_rows','summary_unchanged','original_streams_unchanged_plus_one_test_dll','guard_type1_1499_condition1_unchanged','canonical_unchanged')) and deriv['payloads_unchanged']==14 and deriv['budget_seconds']==180,'ONLY_BOUNDED_TWO_ACTION_INJECTION_CANONICAL_PAYLOADS_UNCHANGED')
 need(static['allOriginalRowsAndSummaryPreserved'] and static['allFourteenPayloadsPreserved'] and static['actualToolCommands']==85,'INDEPENDENT_STATIC_RECEIPT')
 need(by['a470-msvc-post-write-runtime-first/report.json']['sha256']=='ba2826bb9dd3b80ad18fa8c9c7511312b07fa9a0a3217f56400f448c99542dad' and review['report']['sha256']==by['a470-msvc-post-write-runtime-first/report.json']['sha256'],'ACTUAL_RUNTIME_IDENTITY')
 need(runtime['status']=='PASS_MSVC_PRODUCT_GNU_WITNESS_WIN11_DEV_POST_WRITE_ROLLBACK_CLOSURE_AND_EXACT_SCRATCH_CLEANUP' and runtime['runtimeActualExit']==0 and runtime['downloadVerified'] and runtime['cleanup']['knownAbsent'],'ACTUAL_RUNTIME_DOWNLOAD_AND_EXACT_CLEANUP')
 need(runtime['nonce']==runtime['cleanup']['nonce']==review['nonce']=='45683d45ba7948bb850c2319d92c1bf2' and runtime['cleanup']['status']=='REMOVED_EXACT_DOWNLOADED_GNU_DEV_PRODUCT_ROOT','ORIGINAL_LEGACY_CLEANUP_LABEL_NOT_PROVENANCE')
 need(len(runtime['inventory']['files'])==46 and len(runtime['inventory']['directories'])==8 and runtime['phase']['elapsedWholeMilliseconds']==9872 and runtime['elapsedSeconds']==review['outerElapsedSeconds'],'ORIGINAL_DISTINCT_TIMING_AND_COMPLETE_INVENTORY')
 rootname=runtime['windowsRoot'].split('\\')[-1]
 for r in runtime['inventory']['files']:
  local=by['a470-msvc-post-write-runtime-first/download/'+rootname+'/'+r['path']];need(local['bytes']==r['bytes'] and local['sha256']==r['sha256'],'ALL46_COMPLETE_DOWNLOAD_BYTES')
 plan=js('a470-msvc-post-write-runtime-first/upload/runtime-config.json');need(plan['target']=='i686-win7-windows-msvc' and plan['sourceGit']=='a4702b56bff66da5f5cbe74551d8751eb33fa520','EXACT_RUNTIME_TARGET_SOURCE_BINDING')
 source=original(by['a470-msvc-post-write-runtime-preparation-first/run-post-write.py']);tree=ast.parse(source);functions=[n for n in tree.body if isinstance(n,ast.FunctionDef) and n.name in ('phase_returned','admission')];need(len(functions)==2,'EXACT_PURE_FROZEN_RECEIPT_QUALIFIERS');qual={};exec(compile(ast.Module(body=functions,type_ignores=[]),'<frozen-pure-admission>','exec'),qual);need(qual['admission'](runtime['phase'],plan),'FIVE_REAL_SETUP_CAPS_AND_POSTSTATE')
 need(runtime['phase']['wholeMilliseconds']==600000 and runtime['phase']['cleanupReserveMilliseconds']==180000 and runtime['phase']['eachClientMaximumMilliseconds']==150000 and runtime['phase']['setupInternalSeconds']==120,'ORIGINAL_BUDGET_NOT_EXTENDED')
 caps=review['originalProcessInstances'];need(len(caps)==10 and all(c['wait']==0 and c['close']=='CHECKED_CLOSE_SUCCESS' for c in caps) and [c['exit'] for c in caps if c['role']!='owned-guardian']==[1603,0,1603,0,0] and all(c['exit']==0 for c in caps if c['role']=='owned-guardian'),'FIVE_B_FIVE_G_KNOWN_ORIGINAL_CAPS')
 need(review['freshForcedRollbackAbsent'] and review['damagedPreimageRestored'] and review['knownAbsentNonce'] and review['windows7Actual']==review['windows10Actual']=='NOT_RUN','ACTUAL_ROLLBACK_AND_OS_LIMITS')
 need(ps['actualExit']==0 and len(ps['native']['rows'])==8 and all(r['errors']==[] for r in ps['native']['rows']),'ACTUAL_PS51_EIGHT_CHUNKS_AST_ONLY')
 # Original frozen preparation files are exact selected bytes; no basename filtering.
 for dirname in ('a470-msvc-post-write-preparation-first','a470-msvc-post-write-runtime-preparation-first'):
  f=js(dirname+'/freeze-first.json')
  for entry in f['files']:
   key=entry['path'];key=str(Path(key).relative_to(R)) if Path(key).is_absolute() else dirname+'/'+key
   r=by[key];need(r['bytes']==entry['bytes'] and r['sha256']==entry['sha256'],'FROZEN_PREPARATION_BYTE_BINDING')
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
 index=dict(status='PASS_FINITE_A470_OFFICIAL_MSVC_PRODUCT_GNU_WITNESS_WIN11_POST_WRITE_ARCHIVE',sourceSha=plan['sourceGit'],archive=dict(path=archive.name,bytes=archive.stat().st_size,sha256=sha(archive.read_bytes()),members=len(rows),originalBytes=sum(r['bytes'] for r in rows)),originals=len(rows),identityOnly=len(ids),manifest=dict(path='source-manifest-first.json',sha256=sha((D/'source-manifest-first.json').read_bytes())),promoted=PROMOTE,upstream='upstream-reference-first.json immutable D3 CI and corrected D4 canonical normal-product freeze; bodies not duplicated',scope=dict(product='Official a470 Win7 MSVC main/helper/guard/default/profiletestfalse normal14',witness='GNU_C_READONLY_TEST_DLL_ONLY_NOT_PRODUCT_OR_PERMISSION',injection='InstallExecute6500/RemoveExistingProducts6501 unchanged; witness6502 verifies actual14 bytes then Type19 6503 fails; same PackageCode diagnostic fixture only',actual='Win11 forced fresh1603→Absent; canonical install0; original LICENSE delete+51B probe preimage; forced repair1603→exact damaged preimage restored; canonical repair0 healthy14; uninstall0 Absent; five B exits1603/0/1603/0/0 and five G0 known checkedclosed;46files/8dirs exactdownload and new nonceAbsent',nativeMilliseconds=9872,outerSeconds=runtime['elapsedSeconds'],BeginEnd='NOT_DIRECTLY_SERIALIZED_SOURCE_G0_AND_EFFECTS_INFERENCE_ONLY',originalCleanupLabel=runtime['cleanup']['status'],cleanupLabelScope='Preserved generic GNU_DEV label; actual namespace and payload/product provenance are new MSVC and independently byte bound',Win7OS='NOT_RUN',Win10OS='NOT_RUN',releaseReadiness='NOT_GRANTED'),exclusions=manifest['pruned'],credentials=dict(findings=[],backendMember=BACKEND,backendSha256=BACKEND_SHA,structuralForms='Four exact UTF16 MSI DigitalSignature notes; two exact stream lists; two exact ForceCodepage terminators; six exact decoded SummaryInformation argv names; all path/size/SHA/parent-SHA bound, raw/boundary/concatenated views scanned'),WindowsContacted=False,Cargo=False,Git=False,sourceDeletion=False)
 scan(raw(index),'new-index');(D/'archive-index-first.json').write_bytes(raw(index))
 verification=dict(status='PASS_ALL_ORIGINALS_FULL_TAR_REPLAY_AND_STRICT_CREDENTIAL_SCAN',archiveSha256=index['archive']['sha256'],originalsChecked=len(rows),identityOnlyChecked=len(ids),fullTarMembersReplayed=len(rows),promotedRawReports=len(PROMOTE),credentialScan=dict(status='PASS_NO_MATCHES',findings=[],statistics=stats,jsonlRecords=jsonl[0],patterns=list(ns['PATTERNS']),coverage='Complete selected rawtext/full decompressed tar/raw promotions/controller/selector/manifests/index; UTF8/UTF16/CP936/CLIXML/JSON/JSONL/nested generic and declaredbase64; only14 exact structural forms'),originalReportsAndLabelsPreserved=True,Windows=False,Cargo=False,Git=False)
 scan(raw(verification),'new-verification');(D/'archive-verification-first.json').write_bytes(raw(verification));print(json.dumps(dict(status='PASS',archive=index['archive'],verificationSha256=sha(raw(verification)))))
if __name__=='__main__':main()
