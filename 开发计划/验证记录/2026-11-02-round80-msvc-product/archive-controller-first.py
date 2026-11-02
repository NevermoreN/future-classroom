"""Finite finished MSVC product evidence. Default inert; no Windows/build/Git."""
from pathlib import Path
import argparse,ast,gzip,hashlib,io,json,tarfile
M=Path('/workspace/共享/Eliora/未来教室计划');R=Path('/tmp/fc-round80-controlled-repair');D=M/'开发计划/验证记录/2026-11-02-round80-msvc-product'
BACKEND='archive-preparation-second/archive-second.py';BACKEND_SHA='da2906c450ea8c2ac7d18e358507023f519dbcc6faf1a9ac1abb1ae16af8c30f'
PROMOTE=['a470-msvc-product-input-first/manifest.json','a470-msvc-product-package-first/report.json','a470-msvc-product-assembly-first/static-completion-first.json','a470-msvc-product-runtime-first/report.json','a470-msvc-product-runtime-second/report.json','a470-msvc-product-runtime-preparation-second/first-failure-analysis-first.json','a470-msvc-product-runtime-preparation-second/freeze-first.json','root-a470-msvc-product-runtime-review-first.json','root-a470-msvc-product-first-nonce-readonly.json','root-a470-msvc-product-ps-parse-first.json','root-a470-msvc-product-ps-parse-second.json']
def need(v,m):
 if not v:raise AssertionError(m)
def sha(b):return hashlib.sha256(b).hexdigest()
def raw(x):return (json.dumps(x,ensure_ascii=False,indent=2)+'\n').encode()
def scanner(rows):
 with tarfile.open(M/'开发计划/验证记录/2026-11-02-round79-msi-maintenance/evidence-originals-first.tar.gz','r:gz') as t:b=t.extractfile(BACKEND).read()
 need(sha(b)==BACKEND_SHA,'EXACT_COMMITTED_SCANNER');ns={'__name__':'strict_scanner_only'};exec(compile(b,'<committed-scanner>','exec'),ns)
 forms=json.loads((D/'text-forms-first.json').read_bytes());by={r['path']:r for r in rows};need(len(forms['files'])==2,'ONLY_TWO_EXACT_MSI_STREAM_DIAGNOSTICS')
 for f in forms['files']:
  r=by[f['path']];need(r['bytes']==f['bytes'] and r['sha256']==f['sha256'] and f['textForm']=='EXACT_UTF16_MSI_DIGITAL_SIGNATURE_STREAM_NOTE','EXACT_STRUCTURAL_FORM')
 ns['TEXT_FORMS']={f['path']:f for f in forms['files']};need(ns['TEXT_DERIVED_VIEWS']=={},'NO_GENERIC_DERIVED_CONTROL_TOLERANCE')
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
 manifest=json.loads((D/'source-manifest-first.json').read_bytes());rows=manifest['originals'];ids=manifest['identityOnly'];need(len(rows)==229 and sum(r['bytes'] for r in rows)==9549168 and len(ids)==19,'FIXED_FINITE_CLOSURE')
 scan,stats,jsonl,ns=scanner(rows)
 if a.replay_only:
  index=json.loads((D/'archive-index-first.json').read_bytes());archive=verify_tar(manifest,scan);b=archive.read_bytes();need(len(b)==index['archive']['bytes'] and sha(b)==index['archive']['sha256'],'ARCHIVE_CONTAINER_IDENTITY')
  for p in sorted(D.iterdir()):
   if p.is_file() and p.name!=archive.name:scan(p.read_bytes(),'destination:'+p.name)
  print(json.dumps(dict(status='PASS_LOCAL_FULL_TAR_AND_PROMOTION_REPLAY_STRICT_SCAN',members=len(rows),bytes=sum(r['bytes'] for r in rows),archiveSha256=sha(b),findings=[],statistics=stats,jsonlRecords=jsonl[0],Windows=False,Cargo=False,Git=False)));return
 selector=(D/'select-first.py').read_bytes();need(sha(selector)==manifest['selectorSha256'],'EXACT_SELECTOR');sn={'__name__':'selector_only'};exec(compile(selector,'<selector>','exec'),sn);fresh,freshids,pruned=sn['collect']();need(fresh==rows and freshids==ids and pruned==manifest['pruned'],'EXACT_ORIGINAL_MEMBERSHIP')
 by={r['path']:r for r in rows+ids}
 def original(r):
  b=sn['regular'](R/r['path']).read_bytes();need(len(b)==r['bytes'] and sha(b)==r['sha256'],'UNCHANGED_ORIGINAL '+r['path']);return b
 def js(p):return json.loads(original(by[p]))
 first=js('a470-msvc-product-runtime-first/report.json');second=js('a470-msvc-product-runtime-second/report.json');review=js('root-a470-msvc-product-runtime-review-first.json');package=js('a470-msvc-product-package-first/report.json');inputs=js('a470-msvc-product-input-first/manifest.json')
 need(first['status']=='FAIL_OR_UNKNOWN_RETAIN_ORIGINALS' and first['runtime']=='NOT_RUN' and len(first['operations'])==1 and first['operations'][0]['actualExit']==1,'ORIGINAL_PREFLIGHT_FAIL_NOT_MSI_EXECUTION')
 need(by['a470-msvc-product-runtime-first/report.json']['sha256']=='34089633dda32e7e31589eff18b421f2b9ef31f88e020465bc466cbb404aba6c','FIRST_FAILURE_IDENTITY')
 need(second['status']=='PASS_MSVC_WIN7_TARGET_WIN11_CANDIDATE_CONTROLLED_PRODUCT_CLOSURE_AND_EXACT_SCRATCH_CLEANUP' and second['runtimeActualExit']==0 and second['downloadVerified'] and second['cleanup']['knownAbsent'],'ACTUAL_MSVC_JOINT_SUCCESS_AND_SCRATCH_ABSENCE')
 need(by['a470-msvc-product-runtime-second/report.json']['sha256']=='4a1dd14e32be5c8c4519876a5aee534712870dc344fd889291733c48f41c7d7f' and review['report']['sha256']==by['a470-msvc-product-runtime-second/report.json']['sha256'],'ROOT_ACTUAL_RECEIPT_BINDING')
 controller=original(by['a470-msvc-product-runtime-preparation-second/run-controlled-product.py']).decode();tree=ast.parse(controller);functions=[n for n in tree.body if isinstance(n,ast.FunctionDef) and n.name in ['phase_returned','admission']];need(len(functions)==2,'EXACT_PURE_RECEIPT_QUALIFIERS');qual={};exec(compile(ast.Module(body=functions,type_ignores=[]),'<frozen-pure-qualifiers>','exec'),qual);need(qual['admission'](second['phase']),'ACTUAL_ALL_ORIGINAL_CONTROLS_AND_POSTSTATE')
 need(len(second['inventory']['files'])==30 and len(second['inventory']['directories'])==4 and second['phase']['elapsedWholeMilliseconds']==5997,'ACTUAL_ORIGINAL_DOWNLOAD_AND_NATIVE_RANGE')
 need(review['nativeElapsedMilliseconds']==5997 and len(review['originalProcessInstances'])==6 and all(c['wait']==0 and c['exit']==0 and c['close']=='CHECKED_CLOSE_SUCCESS' for c in review['originalProcessInstances']),'THREE_B_THREE_G_ORIGINAL_CAPS')
 need(package['status']=='PASS' and package['controlled_inner'] and not package['development_fixture_only'] and package['target']=='i686-win7-windows-msvc' and package['normal_helper_provenance']['current_inputs_verified']==557 and package['payload_extraction']=='EXACT_BYTES_MATCH','FORMAL_TARGET_SOURCE_STATIC_PACKAGE')
 need(inputs['sourceSha']=='a4702b56bff66da5f5cbe74551d8751eb33fa520' and len(inputs['sourceRawInventory'])==len(inputs['materializationProof'])==618,'COMPLETE_MATERIALIZATION_LEDGER')
 need(js('root-a470-msvc-product-first-nonce-readonly.json')['native']['status']=='KNOWN_ABSENT_READONLY_FIRST_FAILED_NONCE','FIRST_NONCE_ACTUAL_ABSENT')
 for name,n in [('first',8),('second',9)]:
  astreceipt=js('root-a470-msvc-product-ps-parse-'+name+'.json');need(astreceipt['actualExit']==0 and len(astreceipt['native']['rows'])==n and all(r['errors']==[] for r in astreceipt['native']['rows']),'ACTUAL_PS_AST_ONLY_NOT_SEMANTIC_PASS')
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
 index=dict(status='PASS_FINITE_A470_OFFICIAL_MSVC_PRODUCT_AND_WIN11_JOINT_ARCHIVE',sourceSha=inputs['sourceSha'],archive=dict(path=archive.name,bytes=archive.stat().st_size,sha256=sha(archive.read_bytes()),members=len(rows),originalBytes=sum(r['bytes'] for r in rows)),originals=len(rows),identityOnly=len(ids),manifest=dict(path='source-manifest-first.json',sha256=sha((D/'source-manifest-first.json').read_bytes())),promoted=PROMOTE,upstream='upstream-reference-first.json: D3 CI source/log/officialZIP identities not duplicated',scope=dict(static='Official Win7 MSVC normal3/default/profiletestfalse; formal unchanged admission557 raw inputs; all618 materialized ledger; healthy14 payload extraction/type1/template/sealedSetup exact',first='Actual preflight FAIL before root creation/upload/PE/MSI, preserved despite AST0; exact two readonly PS schema/source/target corrections retained',second='Win11 install0→originalLICENSEhandle checkeddelete→repair0 full14→uninstall0; six original B/G caps known0 checkedclosed; actual product/components/registration/Pending/gate/cache absent;30files/4dirs full download replay then nonce absent',nativeMilliseconds=5997,outerSeconds=second['elapsedSeconds'],BeginEnd='NOT_ASSERTED_FROM_LOGS_OR_EXTERNAL_PROCESS_OBSERVER',Win7OS='NOT_RUN',Win10OS='NOT_RUN',releaseReadiness='NOT_GRANTED'),exclusions=manifest['pruned'],credentials=dict(findings=[],backendMember=BACKEND,backendSha256=BACKEND_SHA,structuralExceptions='Only2 exact path/size/SHA-bound UTF16 MSI DigitalSignature stream diagnostics; full raw/visible-boundary/concatenated views scanned; no generic control tolerance'),WindowsContacted=False,Cargo=False,Git=False,sourceDeletion=False)
 (D/'archive-index-first.json').write_bytes(raw(index));scan(raw(index),'new-index')
 verification=dict(status='PASS_COMPLETE_ORIGINAL_TAR_REPLAY_AND_STRICT_CREDENTIAL_SCAN',archiveSha256=index['archive']['sha256'],originalsChecked=len(rows),identityOnlyChecked=len(ids),fullTarMembersReplayed=len(rows),promotedRawReports=len(PROMOTE),credentialScan=dict(status='PASS_NO_MATCHES',findings=[],statistics=stats,jsonlRecords=jsonl[0],patterns=list(ns['PATTERNS']),coverage='Complete source text bytes, full decompressed tar, raw promoted files, preparation/controller/selector/manifest/index; strict UTF8/UTF16/CP936/CLIXML/JSON/JSONL and nested generic+declaredbase64; only2 exact MSI structured controls.'),originalErrorsPreserved=True,Windows=False,Cargo=False,Git=False)
 (D/'archive-verification-first.json').write_bytes(raw(verification));scan(raw(verification),'new-verification');print(json.dumps(dict(status='PASS',archive=index['archive'],verificationSha256=sha(raw(verification)))))
if __name__=='__main__':main()
