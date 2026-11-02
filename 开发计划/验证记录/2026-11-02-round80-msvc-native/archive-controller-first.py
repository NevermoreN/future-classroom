#!/usr/bin/env python3
"""Finite local a470 MSVC evidence archiver; default inert; no network/build/Git."""
from pathlib import Path,PurePosixPath
import argparse,gzip,hashlib,io,json,re,tarfile,zipfile
M=Path('/workspace/共享/Eliora/未来教室计划');R=Path('/tmp/fc-round80-controlled-repair');D=M/'开发计划/验证记录/2026-11-02-round80-msvc-native'
BACKEND='archive-preparation-second/archive-second.py';BACKEND_SHA='da2906c450ea8c2ac7d18e358507023f519dbcc6faf1a9ac1abb1ae16af8c30f'
PROMOTE=['a470-modern-native-runtime-first/report.json','a470-modern-native-retrieval-first/report.json','a470-win7-native-runtime-second/report.json','root-a470-msvc-native-two-targets-first.json','a470-integration-review-first/review-first.json','a470-download-metadata-repair-first.json','official-zip/modern/result.json','official-zip/win7/result.json']
def need(v,s):
 if not v:raise AssertionError(s)
def sha(b):return hashlib.sha256(b).hexdigest()
def raw(x):return (json.dumps(x,ensure_ascii=False,indent=2)+'\n').encode()
def main():
 ap=argparse.ArgumentParser();ap.add_argument('--execute',action='store_true');a=ap.parse_args()
 if not a.execute:print(json.dumps({'status':'PREPARED_NOT_EXECUTED','Windows':False,'Cargo':False,'Git':False}));return
 need(Path(__file__).resolve()==D/'archive-controller-first.py','EXACT_CONTROLLER_PATH')
 manifest=json.loads((D/'source-manifest-first.json').read_bytes());rows=manifest['originals'];ids=manifest['identityOnly'];by={r['path']:r for r in rows+ids};stored=[r for r in rows if r['storedPath']==r['path']]
 need(len(rows)==400 and len(stored)==307 and sum(r['bytes'] for r in stored)==15785047,'EXACT_FINITE_COUNTS')
 selector=(D/'select-first.py').read_bytes();need(sha(selector)==manifest['selectorSha256'],'EXACT_SELECTOR')
 sn={'__name__':'selector_only'};exec(compile(selector,'<finite-selector>','exec'),sn)
 fresh,freshids=sn['collect']();need([{k:v for k,v in r.items() if k!='storedPath'} for r in rows]==fresh and ids==freshids,'EXACT_SOURCE_MEMBERSHIP')
 zips={}
 def original(r):
  if r['sourceType']=='local':b=sn['regular'](R/r['path']).read_bytes()
  else:
   zp=R/r['zipPath']
   if str(zp) not in zips:
    zb=sn['regular'](zp).read_bytes();need(len(zb)==r['zipBytes'] and sha(zb)==r['zipSha256'],'EXACT_EXISTING_OFFICIAL_ZIP');zips[str(zp)]=zipfile.ZipFile(io.BytesIO(zb))
   z=zips[str(zp)];need(z.getinfo(r['member']).CRC==r['crc32'],'EXACT_MEMBER_CRC');b=z.read(r['member'])
  need(len(b)==r['bytes'] and sha(b)==r['sha256'],'ORIGINAL_CHANGED '+r['path']);return b
 def js(p):return json.loads(original(by[p]))
 modern=js('a470-modern-native-runtime-first/report.json');retrieval=js('a470-modern-native-retrieval-first/report.json');win7=js('a470-win7-native-runtime-second/report.json')
 need(modern['status']=='FAIL_RETAIN_ORIGINALS' and modern['errorType']=='NameError' and modern['error']=="name 're' is not defined",'PRESERVE_ORIGINAL_MODERN_WRAPPER_FAILURE')
 need(retrieval['status']=='PASS_EXISTING_MODERN_NATIVE_QUALIFICATION_AND_SCRATCH_CLEANUP_ONLY' and retrieval['nativeRerun'] is False,'ONLY_READBACK_NO_RERUN')
 need(win7['status']=='PASS_OFFICIAL_A470_MSVC_LIFECYCLE_NATIVE_REGRESSION_WIN11_ONLY' and win7['runtimeOS']=='WINDOWS11_ONLY' and win7['target']=='i686-win7-windows-msvc','WIN7_TARGET_ON_WIN11_ONLY')
 for n in ['modern','win7']:
  ci=js('official-zip/'+n+'/result.json');need(ci['status']=='COMPILE_CHECKS_PASSED_RUNTIME_NOT_RUN' and ci['source_sha']=='a4702b56bff66da5f5cbe74551d8751eb33fa520','OFFICIAL_CI_COMPILE_ONLY')
  need(original(by['official-zip/'+n+'/input-sha256.json'])==original(by['official-zip/'+n+'/input-sha256-after.json']),'CI_FULL_INPUTS_UNCHANGED')
 for x in (modern,win7):
  native=x['native'];need(len(native['cases'])==2 and all(c['exitCode']==0 and c['actualExit'] for c in native['cases']),'ACTUAL_TWO_PARENT_ZERO_EXITS');need(len(native['originalObjects'])==2 and all(c['Wait']==0 and c['ExitKnown'] and c['ExitCode']==0 and c['CheckedClose'] for c in native['originalObjects']),'ACTUAL_ORIGINAL_CAPS_CLOSED')
 need(retrieval['cleanup'].startswith('PASS_') and win7['cleanup'].startswith('PASS_'),'BOTH_EXACT_NONCE_CLEANUPS')
 with tarfile.open(M/'开发计划/验证记录/2026-11-02-round79-msi-maintenance/evidence-originals-first.tar.gz','r:gz') as t:backend=t.extractfile(BACKEND).read()
 need(sha(backend)==BACKEND_SHA,'COMMITTED_SCANNER_IDENTITY');ns={'__name__':'strict_scanner_only'};exec(compile(backend,'<committed-scanner>','exec'),ns)
 base=ns['text_views'];forms={r['path']:r for r in rows if r['path'].endswith(('source-git-tree-before.stdout.log','source-git-tree-after.stdout.log'))};need(len(forms)==4,'EXACT_FOUR_ORIGINAL_TREE_STREAMS')
 def views(b,label=None):
  name=None
  for prefix in ('source:','stored-tar-gzip-decoded:','promoted:'):
   if isinstance(label,str) and label.startswith(prefix):name=label[len(prefix):]
  row=forms.get(name)
  if row is None:return base(b,label)
  need(len(b)==3185229 and sha(b)=='62aa49cf18e1bb784dae312a746a7cb920dd9a670f20e8e3f02b5ab11248a51f' and row['sha256']==sha(b),'EXACT_TREE_SOURCE_BYTES')
  text=b.decode('utf-8');records=text.split('\0');need(records[-1]=='' and len(records)-1==19297,'EXACT_19297_NUL_TREE_RECORDS');paths=[]
  for rec in records[:-1]:
   m=re.fullmatch(r'(100644|100755|120000|040000|160000) (blob|tree|commit) ([0-9a-f]{40}) +([0-9]+|-)\t([^\x00]+)',rec);need(m is not None,'STRICT_GIT_LONG_RECORD')
   need((m[1] in ('100644','100755','120000') and m[2]=='blob') or (m[1]=='040000' and m[2]=='tree') or (m[1]=='160000' and m[2]=='commit'),'CONSISTENT_MODE_TYPE')
   need((m[2]=='blob' and m[4].isdigit() and str(int(m[4]))==m[4]) or (m[2] in ('tree','commit') and m[4]=='-'),'CONSISTENT_SIZE_FIELD')
   path=m[5];q=PurePosixPath(path);need(not q.is_absolute() and all(p not in ('','.','..') for p in q.parts) and not any(ord(c)<32 for c in path),'STRICT_SAFE_PATH');paths.append(path)
  need(len(set(paths))==19297 and not any(ord(c)<32 and c not in ('\0','\t') for c in text),'UNIQUE_PATHS_NO_OTHER_CONTROLS')
  return [('utf8-exact-tree19297',text),('utf8-visible-boundary',text.replace('\0','[U+0000]')),('utf8-control-concatenated',text.replace('\0',''))]
 ns['text_views']=views;stats={k:0 for k in ('textViews','utf16Views','cp936Views','clixmlDecodedViews','decodedBase64Views','jsonContainers','decodedJsonStringViews')};findings=[]
 def scan(b,label):
  try:ns['credential_scan'](b,label,stats,findings)
  except Exception as e:raise AssertionError('STRICT_SCAN_REJECT '+label+' '+str(e)) from e
  need(not findings,'CREDENTIAL_FINDING_PATH_KIND_ONLY '+json.dumps(findings))
 # Every original alias checked and scanned, not merely canonical content.
 for r in rows:
  b=original(r);need(b==original(by[r['storedPath']]),'ALIAS_FULL_BYTES_EQUAL');scan(b,'source:'+r['path'])
 for r in ids:original(r)
 for p in sorted(D.iterdir()):
  if p.is_file():scan(p.read_bytes(),'preparation:'+p.name)
 archive=D/'evidence-originals-first.tar.gz'
 with archive.open('xb') as out:
  with gzip.GzipFile(filename='',mode='wb',fileobj=out,mtime=0,compresslevel=9) as gz:
   with tarfile.open(fileobj=gz,mode='w',format=tarfile.PAX_FORMAT) as tar:
    for r in stored:
     b=original(r);i=tarfile.TarInfo(r['path']);i.size=len(b);i.mode=0o600;i.uid=i.gid=i.mtime=0;i.uname=i.gname='';tar.addfile(i,io.BytesIO(b))
 with tarfile.open(archive,'r:gz') as tar:
  members=tar.getmembers();need([x.name for x in members]==[r['path'] for r in stored],'EXACT_TAR_MEMBER_SET')
  for x,r in zip(members,stored):
   need(x.isfile() and x.mode==0o600 and x.uid==x.gid==x.mtime==0 and not x.uname and not x.gname,'REGULAR_DETERMINISTIC_TAR');b=tar.extractfile(x).read();need(b==original(r),'FULL_TAR_ORIGINAL_BYTE_REPLAY');scan(b,'stored-tar-gzip-decoded:'+r['path'])
 for name in PROMOTE:
  target=D/name;target.parent.mkdir(parents=True,exist_ok=True);target.write_bytes(original(by[name]));scan(target.read_bytes(),'promoted:'+name)
 index={'status':'PASS_FINITE_A470_OFFICIAL_MSVC_AND_WIN11_NATIVE_ARCHIVED','sourceGit':'a4702b56bff66da5f5cbe74551d8751eb33fa520','archive':{'path':archive.name,'bytes':archive.stat().st_size,'sha256':sha(archive.read_bytes()),'storedOriginalMembers':len(stored),'storedOriginalBytes':sum(r['bytes'] for r in stored)},'selectedOriginals':len(rows),'selectedOriginalBytes':sum(r['bytes'] for r in rows),'aliasContract':'All400 source originals were read, bytes/SHA checked and individually credential scanned. 93 exact full-byte duplicates reference canonical storedPath in source-manifest; the307 tar members are complete unmodified original bytes. Four before/after Git-tree streams each strictly19297 NUL-delimited records match exactly. Every alias full-byte equality checked, not hash alone.','identityOnlyCount':len(ids),'manifest':{'path':'source-manifest-first.json','sha256':sha((D/'source-manifest-first.json').read_bytes())},'promotedCoreReports':PROMOTE,'scope':{'officialCi':'COMPILE_ONLY; normal product/helper/DLL and tests built with official MSVC; no CI native execution','Win11':'Modern and Win7-target original processes actually ran controlled6 and full lifecycle66 each;6 is subset of66','source69VersusDefault66':'Three compiled-scope feature-only tests excluded from default normal CI, not silently counted as native execution','modernOriginalWrapper':'Original FAIL NameError retained; later readback qualified exact existing native result and cleaned original nonce, nativeRerun=false','Win7OS':'NOT_RUN','Win10OS':'NOT_RUN','productionMsiFromTheseArtifacts':'NOT_RUN','nativeBeginEnd':'Component synthetic/cfgtest observations only, no new production MSI transaction receipt'},'scanner':{'backendMember':BACKEND,'backendSha256':BACKEND_SHA,'extraStructuralForms':'Only four exact path/bytes/SHA Git tree streams,19,297 strict long mode/type/OID/size/tab/path records; raw/visible-boundary/concatenated views scanned','findings':[],'statistics':stats},'excluded':'PE/ZIP identities only; invalid first download metadata identity only and exact repair receipt retained; redundant broad input-domain/tree JSON identity only. No raw scope/credentials/school data/tools/target/activewriter source/future packaging/unfinished host evidence.','WindowsContacted':False,'Cargo':False,'Git':False,'sourceDeletion':False,'progressWritten':False}
 (D/'archive-index-first.json').write_bytes(raw(index));scan(raw(index),'new-index')
 verification={'status':'PASS_FULL_TAR_BYTE_REPLAY_AND_STRICT_CREDENTIAL_SCAN','sourceOriginalsCheckedAndScanned':len(rows),'storedMembersReplayed':len(stored),'identityOnlyChecked':len(ids),'promotedRawCoreReports':len(PROMOTE),'archiveSha256':index['archive']['sha256'],'credentialScan':{'status':'PASS_NO_MATCHES','statistics':stats,'patterns':list(ns['PATTERNS']),'findings':[],'coverage':'All400 source originals,307 complete decompressed tar members,8 raw promoted core reports, controller/selector/manifest/default output/index; strict committed UTF8/UTF16/CP936/CLIXML/JSON/generic+declaredbase64 decoder. Only exact four tree streams structural exception; no generic controls permitted.'},'noSourceOrWindowsMutation':True}
 (D/'archive-verification-first.json').write_bytes(raw(verification));scan(raw(verification),'new-verification')
 for r in rows+ids:original(r)
 print(json.dumps({'status':'PASS','originals':len(rows),'stored':len(stored),'identityOnly':len(ids),'archive':index['archive'],'verificationSha256':sha(raw(verification))}))
if __name__=='__main__':main()
