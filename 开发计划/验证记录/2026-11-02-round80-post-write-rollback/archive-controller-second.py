#!/usr/bin/env python3
"""Default inert finite actual post-write archive; no Windows/Cargo/Git/source deletion."""
from pathlib import Path
import argparse,gzip,hashlib,io,json,stat,tarfile
M=Path('/workspace/共享/Eliora/未来教室计划');R=Path('/tmp/fc-round80-controlled-repair');D=M/'开发计划/验证记录/2026-11-02-round80-post-write-rollback'
MANIFEST_SHA='c520fdede36a5fb5e1e247a8a4a443d445774003ee52f14ed2fe5d4d0ff0bb12'
SCANNER_MEMBER='archive-preparation-second/archive-second.py';SCANNER_SHA='da2906c450ea8c2ac7d18e358507023f519dbcc6faf1a9ac1abb1ae16af8c30f'
PROMOTE=['controlled-post-write-runtime-first/report.json','controlled-post-write-package-first/derivation-report.json','controlled-post-write-operation-first/report.json','controlled-post-write-operation-first/static-review.json','controlled-product-package-second/report.json','root-post-write-ps-parse-first-local-failure.json','root-post-write-runtime-ps-parse-second.json','root-post-write-runtime-operation-first.json']
def need(v,s):
 if not v:raise AssertionError(s)
def sha(b):return hashlib.sha256(b).hexdigest()
def raw(j):return (json.dumps(j,ensure_ascii=False,indent=2)+'\n').encode()
def main():
 p=argparse.ArgumentParser();p.add_argument('--execute',action='store_true');a=p.parse_args()
 if not a.execute:print(json.dumps({'status':'PREPARED_NOT_EXECUTED','Windows':False,'sourceDeletion':False}));return
 need(Path(__file__).resolve()==D/'archive-controller-second.py','EXACT_DESTINATION_CONTROLLER')
 mb=(D/'source-manifest-first.json').read_bytes();need(sha(mb)==MANIFEST_SHA,'FIXED_FINITE_MANIFEST');manifest=json.loads(mb);rows=manifest['originals'];excluded=manifest['excludedBinaryIdentities'];by={r['path']:r for r in rows+excluded}
 need(len(rows)==291 and len(excluded)==37 and len(by)==328 and sum(r['bytes'] for r in rows)==3248203,'EXACT_FINITE_COUNTS')
 selector=(D/'select-first.py').read_bytes();need(sha(selector)==manifest['selectorSha256'],'FIXED_SELECTOR');sel={'__name__':'closed_selector'};exec(compile(selector,'<closed-selector>','exec'),sel)
 selected,payload_ids=sel['collect']();need(sorted(by)==sorted(selected+payload_ids) and payload_ids==manifest['rawPayloadIdentityPaths'],'EXACT_SELECTED_MEMBERSHIP')
 def original(row):
  p=sel['regular'](R/row['path']);b=p.read_bytes();need(len(b)==row['bytes'] and sha(b)==row['sha256'],'ORIGINAL_BYTES_'+row['path']);return b
 def report(name):return json.loads(original(by[name]))
 need(by['controlled-post-write-runtime-first/report.json']['sha256']=='c877f6972c67ebd07ef6eb28d3aead351281978e2a9fe5b3ff9cf2f4a7a56387','EXACT_REAL_RUNTIME')
 runtime=report('controlled-post-write-runtime-first/report.json');phase=runtime['phase'];outer=report('root-post-write-runtime-operation-first.json')
 need(runtime['status']=='PASS_GNU_WIN11_DEV_POST_WRITE_ROLLBACK_CLOSURE_AND_EXACT_SCRATCH_CLEANUP' and runtime['runtimeActualExit']==0 and runtime['downloadVerified'] is True and runtime['cleanup']['knownAbsent'] is True and len(runtime['inventory']['files'])==46 and len(runtime['inventory']['directories'])==8 and not runtime['originalWholeBudgetExceeded'],'REAL_DOWNLOAD_AND_NONCE_CLEANUP')
 need(outer['actualExit']==0 and outer['originalParentReaped'] is True and phase['elapsedWholeMilliseconds']==9331 and outer['seconds']==12.234052500996768,'REAL_ORIGINAL_PARENT_RETURN')
 need(phase['status']=='PASS_GNU_WIN11_DEV_POST_WRITE_FRESH_AND_DAMAGED_ROLLBACK_CANONICAL_CLEANUP' and not phase['ownerUnknown'] and not phase['instrumentUnknown'] and not phase['originalBudgetExceeded'] and phase['freshRollbackAbsent']['knownAbsent'] and phase['damagedPreimageRestored'] and phase['repaired']['healthyPresent'] and phase['finallyProduct']['knownAbsent'] and phase['finallyCoordination']['pendingKnownAbsent'] and phase['finallyCoordination']['namespaceKnownAbsent'],'REAL_POSTSTATES_AND_PENDING_GATE_ABSENCE')
 need(phase['probeDamage']['bytes']==51 and phase['damagedProbePreimage']['base64']==phase['damagedRollbackProbe']['base64'] and phase['damagedProbePreimage']['sha256']==phase['damagedRollbackProbe']['sha256'] and phase['licenseDamage']['checkedClose'],'REAL_OWNED_DAMAGE_PREIMAGE')
 need(phase['uninstalledCache']['allObservedAbsent'] and len(phase['uninstalledCache']['paths'])==2 and len(phase['derivedInstallerPathReadback'])==20 and all(x['status']=='KNOWN_ABSENT' for x in phase['derivedInstallerPathReadback']),'EXACT_TWO_CACHE_TWENTY_LOG_PATHS_ABSENT')
 phases=phase['phases'];need(len(phases)==5 and [x['client']['exitCode'] for x in phases]==[1603,0,1603,0,0],'FIVE_B_ACTUAL_BUSINESS_EXITS')
 for index,x in enumerate(phases):
  need(x['knownControlsReturned'] and x['expectedBusinessExitObserved'] and x['postPhaseProcesses']['knownZero'],'ALL_ORIGINAL_CONTROLS_RETURNED')
  cap=x['directOriginalFinal'];cap=[cap] if isinstance(cap,dict) else cap;need(len(cap)==1 and cap[0]['Wait']==0 and cap[0]['ExitCode']==x['client']['exitCode'] and cap[0]['CheckedClose'] and cap[0]['CloseAttempted'],'ORIGINAL_B_CAP_WAIT_EXIT_CLOSE')
  obs=x['observerFinal'];need(obs['Closed'] and obs['Errors']==[],'ORIGINAL_OBSERVER_RETURN');g=[f for f in obs['Processes'] if f['Role']=='owned-guardian'];need(len(g)==1 and g[0]['ActualWait']==0 and g[0]['ActualExitCode']==0 and g[0]['CloseStatus']=='CHECKED_CLOSE_SUCCESS' and g[0]['ParentPid']==cap[0]['Pid'] and g[0]['ParentCreationFiletime']==cap[0]['Creation'],'EXACT_ORIGINAL_G_PARENT_INSTANCE')
  need(all(f['ActualWait']==0 and f['CloseStatus']=='CHECKED_CLOSE_SUCCESS' for f in obs['Processes']),'EVERY_CAPTURED_OBJECT_CHECKED')
  need(x['terminalEvidence']['classification']=='INFERRED_FROM_FIXED_SOURCE_NATIVE_CODE_G0_AND_PENDING_ABSENT_NOT_DIRECT_API_RECEIPT' and x['terminalEvidence']['beginActualNumber']=='NOT_SERIALIZED' and x['terminalEvidence']['endActualNumber']=='NOT_SERIALIZED','NO_SYNTHETIC_API_RETURN')
  if index in (0,2):need(x['msiLogs']['postWriteSequenceObserved'] and any(v['orderedSameLog'] and v['guardOffset']<v['executeOffset']<v['witnessOffset']<v['failureOffset']<v['rollbackHeaderOffset'] for v in x['msiLogs']['postWriteFacts']),'REAL_SAME_LOG_POST_WRITE_SEQUENCE')
 derivation=report('controlled-post-write-package-first/derivation-report.json');need(len(derivation['commands'])==85 and all(c['actual_exit']==0 and c['parent_reaped'] and not c['timed_out'] for c in derivation['commands']) and derivation['payloads_unchanged']==14 and derivation['summary_unchanged'] and derivation['canonical_unchanged'],'ACTUAL_85_LOCAL_TOOL_RETURNS_AND_FIXED_DELTA')
 failure=report('root-post-write-ps-parse-first-local-failure.json');ast=report('root-post-write-runtime-ps-parse-second.json');need(failure['actualLocalExit']==1 and failure['errorType']=='KeyError' and failure['field']=='text' and ast['actualExit']==0 and ast['originalParentReaped'] and len(ast['native']['rows'])==7 and all(x['errors']==[] for x in ast['native']['rows']),'PRESERVED_INITIAL_LOCAL_FAILURE_AND_REAL_AST0')
 # Bind all downloaded identities, including the four excluded runtime bundles.
 download_prefix='controlled-post-write-runtime-first/download/'+runtime['windowsRoot'].replace('\\','/').split('/')[-1]+'/'
 for item in runtime['inventory']['files']:
  row=by[download_prefix+item['path']];need(row['bytes']==item['bytes'] and row['sha256']==item['sha256'],'ALL46_DOWNLOAD_IDENTITIES')
 with tarfile.open(M/'开发计划/验证记录/2026-11-02-round79-msi-maintenance/evidence-originals-first.tar.gz','r:gz') as t:
  scanner=t.extractfile(SCANNER_MEMBER).read()
 need(sha(scanner)==SCANNER_SHA,'COMMITTED_SCANNER_IDENTITY');ns={'__name__':'committed_finite_credential_scanner'};exec(compile(scanner,'<committed-scanner>','exec'),ns)
 form_bytes=(D/'text-forms-first.json').read_bytes();forms=json.loads(form_bytes);need(len(forms['files'])==4 and len(forms['reviewedMsiLogs'])==5 and len(forms['streamEnumerations'])==2 and len(forms['forceCodepageTables'])==2,'EXACT_FINITE_STRUCTURAL_FORMS')
 for f in forms['files']+forms['streamEnumerations']+forms['forceCodepageTables']:
  need(f['bytes']==by[f['path']]['bytes'] and f['sha256']==by[f['path']]['sha256'],'EXACT_STRUCTURAL_FORM_SOURCE')
 ns['TEXT_FORMS']={f['path']:f for f in forms['files']};need(ns['TEXT_DERIVED_VIEWS']=={},'NO_DERIVED_JSON_CONTROL_EXCEPTION');base_views=ns['text_views'];special={f['path']:f for f in forms['streamEnumerations']+forms['forceCodepageTables']};derived_bytes=(D/'derived-views-first.json').read_bytes();need(sha(derived_bytes)=='feb2da249fab3d59b87584d2daccadcbf3c6fffe3c241bed36282ab9e07a30da','FIXED_SIX_DERIVED_VIEWS');derived=json.loads(derived_bytes);need(len(derived['files'])==6,'EXACT_SIX_DERIVED_VIEWS')
 for f in derived['files']:
  parent=by[f['parentPath']];need(parent['bytes']==f['parentBytes'] and parent['sha256']==f['parentSha256'] and f['path'].startswith(f['parentPath']+'::json/commands/') and f['path'].endswith('/argv/3'),'EXACT_DERIVED_PARENT_DOCUMENT_AND_TRAIL');special[f['path']]=f
 def exact_views(data,label=None):
  relative=None
  for prefix in ('source:','stored-tar-gzip-decoded:','promoted:'):
   if isinstance(label,str) and label.startswith(prefix):relative=label[len(prefix):]
  row=special.get(relative)
  if row is None:return base_views(data,label)
  need(len(data)==row['bytes'] and sha(data)==row['sha256'],'EXACT_EXTRA_STRUCTURAL_BYTES');text=data.decode('ascii')
  if row['textForm']=='EXACT_MSI_SUMMARYINFORMATION_ARGV_ELEMENT':
   control='\x05';need(data==bytes.fromhex(row['exactBytesHex'])==b'\x05SummaryInformation','EXACT19_SUMMARYINFORMATION_ARGV')
  elif row['textForm']=='EXACT_ASCII_MSI_STREAM_NAME_ENUMERATION':
   control='\x05';expected='\n'.join(row['linesEscaped']).replace(r'\x05','\x05')+'\n';need(text==expected and text.count(control)==1 and text.splitlines()[-1]=='\x05SummaryInformation','EXACT_FINITE_STREAM_NAMES')
   need(set(text.splitlines()) in ({'payload.cab','Binary.FcControlledGuardDll','Binary.FcGuardClientTemplate','\x05SummaryInformation'},{'payload.cab','Binary.FcPostWriteWitness','Binary.FcControlledGuardDll','Binary.FcGuardClientTemplate','\x05SummaryInformation'}),'CLOSED_MSI_STREAM_ENUMERATION_SET')
  else:
   control='\0';need(row['textForm']=='EXACT_FORCECODEPAGE_IDT23_WITH_SINGLE_NUL_TERMINATOR' and data==bytes.fromhex(row['exactBytesHex'])==b'\r\n\r\n0\t_ForceCodepage\r\n\0','EXACT23_CODEPAGE_TABLE_BYTES')
  need(not any(ord(c)<32 and c not in ('\r','\n','\t',control) for c in text),'NO_OTHER_STRUCTURAL_CONTROL')
  return [('ascii-exact-structural',text),('ascii-visible-boundary',text.replace(control,'[U+%04X]'%ord(control))),('ascii-control-concatenated',text.replace(control,''))]
 ns['text_views']=exact_views
 stats={k:0 for k in ('textViews','utf16Views','cp936Views','clixmlDecodedViews','decodedBase64Views','jsonContainers','decodedJsonStringViews')};findings=[];jsonl=0
 def scan(b,label):
  nonlocal jsonl
  ns['credential_scan'](b,label,stats,findings)
  if label.endswith('.jsonl'):
   for i,line in enumerate(b.splitlines()):
    if not line.strip():continue
    need(i<250000,'JSONL_LIMIT');json.loads(line);jsonl+=1;ns['credential_scan'](line,label+'::jsonl/'+str(i+1),stats,findings)
  need(not findings,'CREDENTIAL_FINDINGS_KIND_PATH_ONLY_'+json.dumps(findings))
 for row in rows:scan(original(row),'source:'+row['path'])
 for row in excluded:original(row)
 for path in sorted(D.iterdir()):
  if path.is_file():scan(path.read_bytes(),'archive-preparation:'+path.name)
 facts={'status':'FINITE_ACTUAL_WIN11_POST_WRITE_ROLLBACK_EVIDENCE_ONLY','sourceGit':'33c11f5e41cce727746aeb5b5801b0e87eaf0f41','actualRuntimeReport':by['controlled-post-write-runtime-first/report.json'],'localDerivationActualCommands':85,'localDerivation':by['controlled-post-write-package-first/derivation-report.json'],'BActualExits':[1603,0,1603,0,0],'GActualExits':[0,0,0,0,0],'actualOuterExit':0,'originalParentReaped':True,'outerSeconds':outer['seconds'],'nativeWholeMilliseconds':9331,'originalWholeSeconds':600,'eachClientSeconds':150,'nativeSeconds':120,'forcedSameLogSequence':'guard1 -> InstallExecute1 -> full14witness1 -> fixedType19fail3 -> ScriptType2 rollback header','damagedPreimage':'LICENSE absent + exact51B PROBE restored; remaining12 bytes and registration/component/marker preimage equal','final':'canonical repair0 + uninstall0; product/full14/registration/marker/Pending/gate/20 exact installer paths/2 cache paths/46files8dirs nonce absent','nativeBeginEnd':'NOT_SERIALIZED; fixed source+G0+PendingAbsent inference only','externalOriginalGThreadCap':'NOT_AVAILABLE_CODEPATH_INFERENCE_ONLY','initialAstFailure':by['root-post-write-ps-parse-first-local-failure.json'],'actualSecondAst0':by['root-post-write-runtime-ps-parse-second.json'],'notClaimed':['Win7/Win10/Seewo actual tests','production crash recovery/release/ARP uninstall','direct serialized Begin/End receipt','all14 replacement during damaged repair (only two damaged; witness all14 healthy at instant)'],'preparationNotRunOriginalsPreserved':True,'sourceDeletion':False,'WindowsContactedByArchiver':False,'WOrTargetRead':False,'progressWritten':False,'GitOperation':False}
 fb=raw(facts);scan(fb,'new-facts')
 archive=D/'evidence-originals-first.tar.gz'
 with archive.open('xb') as out:
  with gzip.GzipFile(filename='',mode='wb',fileobj=out,mtime=0,compresslevel=9) as zipped:
   with tarfile.open(fileobj=zipped,mode='w',format=tarfile.PAX_FORMAT) as tar:
    for row in rows:
     b=original(row);info=tarfile.TarInfo(row['path']);info.size=len(b);info.mode=0o600;info.uid=info.gid=info.mtime=0;info.uname=info.gname='';tar.addfile(info,io.BytesIO(b))
 with tarfile.open(archive,'r:gz') as tar:
  members=tar.getmembers();need([v.name for v in members]==[r['path'] for r in rows],'EXACT_TAR_MEMBER_SET')
  for v,row in zip(members,rows):
   need(v.isfile() and v.mode==0o600 and v.uid==v.gid==v.mtime==0 and v.uname==v.gname=='','DETERMINISTIC_REGULAR_TAR_MEMBER');b=tar.extractfile(v).read();need(b==original(row),'FULL_MEMBER_BYTE_REPLAY');scan(b,'stored-tar-gzip-decoded:'+row['path'])
 for name in PROMOTE:
  b=original(by[name]);target=D/name;target.parent.mkdir(parents=True,exist_ok=True)
  with target.open('xb') as f:f.write(b)
  scan(target.read_bytes(),'promoted:'+name)
 (D/'batch-facts-first.json').write_bytes(fb)
 archive_bytes=archive.read_bytes();index={'status':'PASS_FINITE_ACTUAL_POST_WRITE_ORIGINALS_ARCHIVED','archive':{'path':archive.name,'bytes':len(archive_bytes),'sha256':sha(archive_bytes),'members':291},'originalBytes':3248203,'manifest':{'path':'source-manifest-first.json','bytes':len(mb),'sha256':sha(mb)},'facts':{'path':'batch-facts-first.json','bytes':len(fb),'sha256':sha(fb)},'promoted':[by[n] for n in PROMOTE],'binaryIdentitiesOnly':excluded,'derivedViews':{'path':'derived-views-first.json','bytes':len(derived_bytes),'sha256':sha(derived_bytes)},'scannerBackend':{'priorCommittedTar':'开发计划/验证记录/2026-11-02-round79-msi-maintenance/evidence-originals-first.tar.gz','member':SCANNER_MEMBER,'sha256':SCANNER_SHA},'sourceDeletion':False,'WindowsContacted':False,'progressWritten':False}
 ib=raw(index);scan(ib,'new-index');(D/'archive-index-first.json').write_bytes(ib)
 verify={'status':'PASS_FULL_TAR_REPLAY_AND_BOUNDED_DECODED_CREDENTIAL_SCAN','members':291,'originalBytes':3248203,'binaryIdentityOnlyCount':37,'promotedRawReports':len(PROMOTE),'archiveSha256':sha(archive_bytes),'credentialScan':{'status':'PASS_NO_MATCHES','statistics':stats,'jsonlRecordsAlsoDecoded':jsonl,'patterns':list(ns['PATTERNS']),'findings':[],'coverage':'All291 originals/raw text, full gzip/tar decompressed byte replay, 8 raw promoted reports, D controller/selector/manifest/forms/diagnostics/facts/index. Existing committed scanner decodes UTF8/UTF16/CP936/CLIXML/JSON/JSONL/nested generic and declared base64 under its original limits. Four exact one-note MSI controls among five reviewed logs, two exact ASCII stream listings, two exact23B ForceCodepage IDTs and six exact parent-document/JSON-trail/19B SummaryInformation argv views: raw/visible-boundary/concatenated complete views all scanned. No generic control tolerance. Raw PE/ZIP/CAB/MSI/DLL/payload/propertyset streams only identities.'},'sourceDeletion':False,'WindowsContacted':False,'Cargo':False,'WOrTargetRead':False,'privateKeyRead':False,'progressWritten':False,'GitOperation':False}
 vb=raw(verify);scan(vb,'new-verification');(D/'archive-verification-first.json').write_bytes(vb)
 need(sel['collect']()==(selected,payload_ids),'FINAL_SELECTED_MEMBERSHIP_UNCHANGED')
 for row in rows+excluded:original(row)
 print(json.dumps({'status':'PASS','textMembers':291,'binaryIdentityOnly':37,'promotedReports':len(PROMOTE),'archiveBytes':len(archive_bytes),'archiveSha256':sha(archive_bytes),'verificationSha256':sha(vb),'factsSha256':sha(fb),'sourceDeletion':False,'WindowsContacted':False,'progressWritten':False}))
if __name__=='__main__':main()
