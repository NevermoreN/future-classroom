#!/usr/bin/env python3
"""Explicit, default-inert fixed Type1 API probe archive. No Windows or source deletion."""
from pathlib import Path
import argparse,gzip,hashlib,io,json,os,stat,tarfile
R=Path('/tmp/fc-round80-controlled-repair')
MAIN=Path('/workspace/共享/Eliora/未来教室计划')
D=MAIN/'开发计划/验证记录/2026-11-02-round80-inner-guard-api-probe'
DIRS=['inner-guard-probe-first','inner-guard-probe-second','inner-guard-probe-build-first','inner-guard-probe-build-second','inner-guard-probe-runtime-first','inner-guard-probe-tools-first']
BACKEND=MAIN/'开发计划/验证记录/2026-11-02-round79-msi-maintenance/evidence-originals-first.tar.gz'
MEMBER='archive-preparation-second/archive-second.py'
BACKEND_SHA='da2906c450ea8c2ac7d18e358507023f519dbcc6faf1a9ac1abb1ae16af8c30f'
PROMOTE=['inner-guard-probe-runtime-first/runtime-operation.json','inner-guard-probe-build-first/build-report.json','inner-guard-probe-build-second/build-report.json','inner-guard-probe-tools-first/tools-report.json','inner-guard-probe-tools-first/tools-recovery-first.json','windows-readonly-inner-guard-first.json']
PINS={'inner-guard-probe-runtime-first/runtime-operation.json':'7e1771a067414ad8accf86c53523dd59d3fcfc2bd98f3dcbc8e06c95c0330354','windows-readonly-inner-guard-first.json':'379945c00ae8e0205ecd6161e7dc4aa02ca15373b77485e6123d5849335b4f45','inner-guard-probe-build-first/build-report.json':'edd66dcbae31f4c01bd7a36f4e53488ff795caa896f7b5a15a48a57f665d4cf0','inner-guard-probe-build-second/build-report.json':'b9ce6fb68af13209a625afce5256a358ece12ec7099583523bc2d216b6945375','inner-guard-probe-tools-first/tools-report.json':'c3aeaaa047929ad41053deeaf875ec214dee23ba7575773e820d7b3841cfa9dd','inner-guard-probe-tools-first/tools-recovery-first.json':'e4db9992fcbdfb8d7713d051de4242c54cbeef358fc1dc09463a1b118409f375'}
def need(v,m):
 if not v:raise AssertionError(m)
def sha(b):return hashlib.sha256(b).hexdigest()
def encoded(j):return (json.dumps(j,ensure_ascii=False,indent=2)+'\n').encode()
def load(rel):return json.loads((R/rel).read_bytes())
def regular(p):
 need(p.is_relative_to(R) and p.resolve()==p,'EXACT_NO_ALIAS_PATH')
 for a in (p,*p.parents):
  if a==R or a.is_relative_to(R):need(not a.is_symlink(),'NO_SYMLINK_ANCESTOR')
 need(stat.S_ISREG(p.lstat().st_mode) and p.stat().st_size<=16*1024*1024,'BOUNDED_REGULAR')
 return p

def enumerate_fixed():
 files=[];links=[]
 def walk(p):
  for entry in sorted(os.scandir(p),key=lambda e:e.name):
   q=Path(entry.path);rel=q.relative_to(R).as_posix()
   need(not any(v in q.parts for v in ('target','deps','__pycache__','checkout-52d6392-first')),'NO_ACTIVE_CACHE')
   if entry.is_symlink():
    need(rel.startswith('inner-guard-probe-tools-first/root/'),'ONLY_REBUILDABLE_TOOL_SYMLINKS');links.append({'path':rel,'type':'symlink','target':os.readlink(q),'followed':False})
   elif entry.is_dir(follow_symlinks=False):walk(q)
   else:regular(q);files.append(rel)
 for d in DIRS:need((R/d).is_dir() and not (R/d).is_symlink(),'EXACT_FIXED_DIR');walk(R/d)
 roots=sorted(p.name for p in R.glob('root-inner-guard-*') if p.is_file())
 need(len(roots)==16,'EXACT_SIXTEEN_ROOT_RECEIPTS_AND_STREAMS')
 files+=roots+['windows-readonly-inner-guard-first.json']
 need(len(files)==len(set(files)),'UNIQUE_FIXED_PATHS')
 return sorted(files),links

def main():
 a=argparse.ArgumentParser();a.add_argument('--execute',action='store_true');args=a.parse_args()
 if not args.execute:print(json.dumps({'status':'PREPARED_NOT_EXECUTED','Windows':False,'sourceDeletion':False}));return
 need(Path(__file__).resolve()==D/'archive-controller-second.py','EXACT_CONTROLLER')
 need({p.name for p in D.iterdir()}=={'archive-controller-first.py','archive-controller-second.py','archive-preparation-first-failure.json'},'FRESH_DESTINATION')
 need((R/'.owner').read_bytes()==b'future-classroom-owned-r80-controlled-repair/v1\n','SOURCE_OWNER')
 with tarfile.open(BACKEND,'r:gz') as t:
  m=t.getmember(MEMBER);need(m.isfile() and m.size==26272,'FROZEN_SCANNER_MEMBER');scanner=t.extractfile(m).read()
 need(sha(scanner)==BACKEND_SHA,'FROZEN_SCANNER_SHA')
 ns={'__name__':'frozen_probe_scan_backend'};exec(compile(scanner,'<frozen-r79-text-scanner>','exec'),ns)
 files,links=enumerate_fixed();texts=[];binary=[];tools=[]
 for rel in files:
  b=regular(R/rel).read_bytes();row={'path':rel,'bytes':len(b),'sha256':sha(b)}
  if rel in PINS:need(row['sha256']==PINS[rel],'FIXED_REPORT_SHA_'+rel)
  kind=ns['binary_kind'](b)
  if rel.startswith('inner-guard-probe-tools-first/root/'):
   row['kind']=kind or 'EXTRACTED_REBUILDABLE_TOOL_TEXT';tools.append(row)
  elif kind:
   row['kind']=kind;binary.append(row)
  else:texts.append(row)
 need(set(PINS)<={x['path'] for x in texts},'ALL_CORE_REPORTS_TEXT')
 bypath={x['path']:x for x in texts+binary+tools}
 # Two narrowly bound, genuine MSI stream-name control characters; all bytes stay raw.
 forms=[]
 for leaf in ('healthy-rollback-msi.log','early-fail-rollback-msi.log'):
  rel='inner-guard-probe-runtime-first/download/'+leaf;b=(R/rel).read_bytes();text=b.decode('utf-16')
  found=[(i+1,line) for i,line in enumerate(text.split('\n')) if '\x05' in line]
  need(len(found)==1 and text.count('\x05')==1,'ONE_EXACT_MSI_STREAM_NOTE')
  row={**bypath[rel],'textForm':'EXACT_UTF16_MSI_DIGITAL_SIGNATURE_STREAM_NOTE','controlLineNumber':found[0][0],'controlLineEscaped':found[0][1].replace('\x05',r'\x05')};forms.append(row)
 ns['TEXT_FORMS']={x['path']:x for x in forms};need(ns['TEXT_DERIVED_VIEWS']=={},'NO_DERIVED_CONTROL_EXCEPTIONS')
 stats={k:0 for k in ('textViews','utf16Views','cp936Views','clixmlDecodedViews','decodedBase64Views','jsonContainers','decodedJsonStringViews')};findings=[];jsonl_lines=0
 def scan(b,label):
  nonlocal jsonl_lines
  ns['credential_scan'](b,label,stats,findings)
  if label.endswith('.jsonl'):
   for i,line in enumerate(b.splitlines()):
    if not line.strip():continue
    need(i<250000,'JSONL_RECORD_BOUND');json.loads(line);jsonl_lines+=1
    ns['credential_scan'](line,label+'::jsonl/'+str(i+1),stats,findings)
  need(not findings,'CREDENTIAL_FINDING_PATH_AND_KIND_ONLY_'+json.dumps(findings))
 def original(row):
  b=regular(R/row['path']).read_bytes();need(len(b)==row['bytes'] and sha(b)==row['sha256'],'SOURCE_STILL_EXACT_'+row['path']);return b
 # Verify all fixed runtime downloads and all raw transport streams against their original ledgers.
 j=load('inner-guard-probe-runtime-first/runtime-operation.json');phase=load('inner-guard-probe-runtime-first/download/probe-result.json')
 need(j['phase']==phase and j['downloadVerified'] and j['phaseParentExit']==0 and j['originalPhaseNaturalReturn'],'ACTUAL_ORIGINAL_PHASE')
 need(j['inventory']==j['finalInventory'] and len(j['inventory']['files'])==24,'COMPLETE_DOWNLOAD_LEDGER')
 for row in j['inventory']['files']:
  b=regular(R/'inner-guard-probe-runtime-first/download'/row['path']).read_bytes();need(len(b)==row['bytes'] and sha(b)==row['sha256'],'DOWNLOAD_EXACT_'+row['path'])
 for op in j['operations']:
  need(op['actualLocalExit']==0 and op['localParentReaped'] and not op['timedOut'] and op['remoteReturnKnown'],'ORIGINAL_OPERATION_ACTUAL')
  for key in ('stdin.ps1','stdout.log','stderr.log'):
   if key in op:
    row=op[key];b=regular(R/'inner-guard-probe-runtime-first'/row['path']).read_bytes();need(len(b)==row['bytes'] and sha(b)==row['sha256'],'OP_STREAM_EXACT')
 need(j['productionAuthority']=='NONE' and len(phase['cases'])==2 and phase['errors']==[] and not phase['nativeCloseUnknown'],'FINITE_RUNTIME_CLOSED')
 cases=[]
 for c,expected in zip(phase['cases'],[0,1603]):
  api={x['stage']:x for x in c['api']};need([api[x]['return'] for x in ('BEGIN_RETURN','INSTALL_RETURN','END_RETURN','CHANGE_EVENT_CLOSE_RETURN')]==[0,expected,0,0],'REAL_ROLLBACK_API_SEQUENCE')
  cases.append({'name':c['name'],'Begin':0,'Install':expected,'EndRollback':0,'changeEventCheckedClose':0})
 for c in j['apiProbeFacts']:
  need(c['guard']['messageReturn']==1 and c['guard']['recordCloseReturn']==0 and c['guard']['productValid'],'REAL_HINSTALL_AND_OWN_RECORD')
 need(j['apiProbeFacts'][0]['successorSequences']==[26] and j['apiProbeFacts'][1]['successorSequences']==[],'SUCCESSOR_DISTINCTION')
 need(j['cleanup']['knownAbsent'] and j['zeroOwnProcesses']['ownCount']==0 and len(j['derivedExactSystemPaths'])==5,'FINITE_CLEANUP')
 audit=load('windows-readonly-inner-guard-first.json');need(audit['status']=='PASS' and audit['scratchDirectories']==audit['ownedProcesses']==audit['ownedTasks']==[] and audit['sshd']=='Running' and audit['persistentKeyMode']=='0o600' and audit['publicAuthorizationUnchanged'] and audit['projectRunUnchanged'],'FINAL_READONLY_AUDIT')
 need(load('inner-guard-probe-build-first/build-report.json')['status']=='FAIL_STATIC_ONLY','KEEP_BUILD_FAIL')
 need(load('inner-guard-probe-tools-first/tools-report.json')['status']=='FAIL_RETAIN_ORIGINALS','KEEP_TOOLS_FAIL')
 need(load('inner-guard-probe-tools-first/tools-recovery-first.json')['status']=='PASS_PRIVATE_TOOL_WRAPPER_RECOVERY','KEEP_TOOL_RECOVERY')
 manifest={'schemaVersion':1,'scope':'Fixed Type1 API probe only; no active guard code, W, target, caches or private keys','originals':texts,'originalCount':len(texts),'originalBytes':sum(x['bytes'] for x in texts),'excludedBinaryIdentities':binary,'rebuildableExtractedToolIdentities':tools,'toolSymlinksNotFollowed':links,'toolTreePolicy':'All extracted tool payloads are identity-only, including generated upstream text; seven original pinned deb identities and recovery metadata preserved. No tools deleted.','exactControlForms':forms,'selectedDirectories':DIRS,'selectedRootFiles':[x for x in files if '/' not in x],'scanner':{'archive':str(BACKEND.relative_to(MAIN)),'member':MEMBER,'sha256':BACKEND_SHA,'mainExecuted':False}}
 facts={'schemaVersion':1,'status':'PASS_FINITE_API_PROBE_ONLY_NOT_PRODUCTION_AUTHORITY','productionAuthority':'NONE','runtimeOriginal':bypath['inner-guard-probe-runtime-first/runtime-operation.json'],'finalReadOnlyAudit':bypath['windows-readonly-inner-guard-first.json'],'cases':cases,'originalProcessAndThreadControls':'Each original G process and original native thread actually exited0 and had checked close; original change-event close0. No kill of shared service, no manual registry/cache deletion.','apiProbeFacts':j['apiProbeFacts'],'nativePhaseMilliseconds':phase['elapsedMilliseconds'],'transportSeconds':j['elapsedSeconds'],'downloadFiles':24,'downloadAllHashesVerified':True,'ownedProcessesAfter':0,'derivedExactSystemPaths':j['derivedExactSystemPaths'],'scratchRemoved':True,'preservedFailures':['Original private-tool preparation FAIL_RETAIN_ORIGINALS; fixed private wixl wrapper recovery retained separately.','First C build FAIL_STATIC_ONLY: MSIHANDLE printf diagnostic type mismatch under -Werror; minimal format/cast correction retained, API calls unchanged.','Original pending/NOT_RUN snapshots remain unchanged.'],'limits':['Only quiet-mode Type1 borrowed-hInstall INFO, own-record close and successor API observation on Win11.','Healthy case next InstallInitialize observed; early-fail case absent. Both end rollback, no product commit/uninstall coverage.','No production permit/session binding proof, no newly integrated Rust guard or public Setup entry proof.','New Rust guard implementation remains outside this archive; no current production linkage or release approval.','Win7 SP1 x86 and Win10 true-machine runs NOT_RUN.'],'actions':{'WindowsContacted':False,'Cargo':False,'worktreeOrTargetRead':False,'privateKeyContentRead':False,'sourceDeletion':False,'commitPush':False}}
 for row in texts:scan(original(row),'source:'+row['path'])
 generated={'source-manifest-first.json':encoded(manifest),'probe-facts-first.json':encoded(facts)}
 for name,b in generated.items():scan(b,'generated:'+name)
 controller=Path(__file__).read_bytes();scan(controller,'archive-controller-source')
 for name in ('archive-controller-first.py','archive-preparation-first-failure.json'):scan((D/name).read_bytes(),'preserved-archive-preparation:'+name)
 # The entire source closure passed before creating an archive payload.
 for name,b in generated.items():(D/name).write_bytes(b)
 archive=D/'evidence-originals-first.tar.gz'
 with archive.open('xb') as outer:
  with gzip.GzipFile(filename='',mode='wb',fileobj=outer,mtime=0,compresslevel=9) as zipped:
   with tarfile.open(fileobj=zipped,mode='w',format=tarfile.PAX_FORMAT) as tar:
    for row in texts:
     b=original(row);info=tarfile.TarInfo(row['path']);info.size=len(b);info.mode=0o600;info.uid=info.gid=info.mtime=0;info.uname=info.gname='';tar.addfile(info,io.BytesIO(b))
 with tarfile.open(archive,'r:gz') as tar:
  members=tar.getmembers();need([x.name for x in members]==[x['path'] for x in texts],'EXACT_TAR_MEMBER_SET')
  for member,row in zip(members,texts):
   need(member.isfile() and member.mode==0o600 and member.uid==member.gid==member.mtime==0,'REGULAR_DETERMINISTIC_TAR')
   b=tar.extractfile(member).read();need(b==original(row),'FULL_MEMBER_BYTE_REPLAY');scan(b,'stored-tar-gzip-decoded:'+row['path'])
 for rel in PROMOTE:
  row=bypath[rel];b=original(row);p=D/rel;p.parent.mkdir(parents=True,exist_ok=True);p.write_bytes(b);need(p.read_bytes()==b,'PROMOTED_BYTE_EXACT');scan(b,'promoted:'+rel)
 cb=archive.read_bytes();index={'schemaVersion':1,'status':'PASS_FIXED_PROBE_ARCHIVED','archive':{'path':archive.name,'bytes':len(cb),'sha256':sha(cb),'memberCount':len(texts)},'manifest':{'path':'source-manifest-first.json','bytes':len(generated['source-manifest-first.json']),'sha256':sha(generated['source-manifest-first.json'])},'facts':{'path':'probe-facts-first.json','bytes':len(generated['probe-facts-first.json']),'sha256':sha(generated['probe-facts-first.json'])},'promoted':[bypath[x] for x in PROMOTE],'binaryIdentitiesOnly':len(binary),'extractedToolIdentitiesOnly':len(tools),'symlinksNotFollowed':len(links),'allTextMembersReplayed':True,'productionAuthority':'NONE','sourceDeleted':False}
 ib=encoded(index);scan(ib,'new-index');(D/'archive-index-first.json').write_bytes(ib)
 receipt={'schemaVersion':1,'status':'PASS_FULL_TAR_REPLAY_AND_BOUNDED_DECODED_CREDENTIAL_SCAN','archiveSha256':sha(cb),'archiveBytes':len(cb),'indexSha256':sha(ib),'originalCount':len(texts),'originalBytes':sum(x['bytes'] for x in texts),'credentialScan':{'status':'PASS_NO_MATCHES','statistics':stats,'jsonlRecordsAlsoDecoded':jsonl_lines,'patterns':list(ns['PATTERNS']),'findings':[],'coverage':'All selected text raw bytes, strict UTF8/UTF16/CP936, CLIXML parsed text, JSON and JSONL strings, nested canonical generic/declared base64, full tar replay, six promoted core reports, manifest/facts/controller/index. Unknown controls rejected; exactly two path/size/SHA-bound UTF16 MSI stream notes scanned in raw/visible/concatenated forms. Decoder depth12; JSON250000; decoded64MiB per original.','exactControlFormCount':2},'scope':'API probe evidence only; production authority NONE. Historical original FAIL/NOT_RUN preserved.','sourceDeletion':False,'toolsDeleted':False,'WindowsContacted':False,'Cargo':False,'worktreeOrTargetRead':False,'privateKeyContentRead':False}
 rb=encoded(receipt);scan(rb,'scan-receipt');(D/'archive-verification-first.json').write_bytes(rb)
 need(enumerate_fixed()==(files,links),'FIXED_PATH_SET_UNCHANGED')
 for row in texts+binary+tools:original(row)
 expected={'archive-controller-first.py','archive-controller-second.py','archive-preparation-first-failure.json','source-manifest-first.json','probe-facts-first.json','evidence-originals-first.tar.gz','archive-index-first.json','archive-verification-first.json'}|set(PROMOTE)
 need({p.relative_to(D).as_posix() for p in D.rglob('*') if p.is_file()}==expected,'EXACT_FINITE_DESTINATION')
 print(json.dumps({'status':'PASS','textMembers':len(texts),'originalBytes':sum(x['bytes'] for x in texts),'binaryIdentitiesOnly':len(binary),'toolIdentitiesOnly':len(tools),'symlinksNotFollowed':len(links),'archiveBytes':len(cb),'archiveSha256':sha(cb),'indexSha256':sha(ib),'verificationSha256':sha(rb),'factsSha256':sha(generated['probe-facts-first.json']),'destinationFiles':len(expected),'sourceDeletion':False,'WindowsContacted':False}))
if __name__=='__main__':main()
