#!/usr/bin/env python3
"""Finite D14/D15 archive. Default inert; verify-only never reads R/ZIP/Git."""
from pathlib import Path
import argparse,base64,gzip,hashlib,io,json,re,stat,tarfile,zipfile
R=Path('/tmp/fc-round80-controlled-repair')
HERE=Path(__file__).resolve().parent
SOURCE='71ced30e86b3d7a7ed0d1ea5458dba81f1e61045'
DIRS14='''abi2-upgrade-failed-cleanup-preparation-first abi2-upgrade-failed-cleanup-runtime-first abi2-upgrade-final-checks-first abi2-upgrade-implementation-first abi2-upgrade-independent-review-partial-first abi2-upgrade-integrated-checks-first abi2-upgrade-package-preparation-first abi2-upgrade-postwrite-package-first abi2-upgrade-postwrite-package-second abi2-upgrade-postwrite-preparation-first abi2-upgrade-postwrite-preparation-second abi2-upgrade-postwrite-runtime-first abi2-upgrade-postwrite-runtime-preparation-first abi2-upgrade-u1-dev-product-package-first abi2-upgrade-u1-normal-producer-first abi2-upgrade-u1-package-operation-first abi2-upgrade-u1-product-runtime-first abi2-upgrade-u1-product-runtime-preparation-first abi2-upgrade-u1-publication-interop-first abi2-upgrade-u1-publication-second abi2-upgrade-u1-u2-runtime-first abi2-upgrade-u1-u2-runtime-preparation-first abi2-upgrade-u1-u2-runtime-preparation-second abi2-upgrade-u1-u2-runtime-second abi2-upgrade-u2-dev-product-package-first abi2-upgrade-u2-package-operation-first abi2-upgrade-u2-package-preparation-first abi2-upgrade-u2-product-producer-first'''.split()
DIRS15=['71-ci-original-metadata-first','71-ci-qualification-first','r06-test-module-order-first']
BINEXT={'.exe','.dll','.msi','.zip','.cab','.bin','.pdb','.rlib','.a','.so','.o','.obj','.pyc'}
PRUNE={'__pycache__','.git','target','node_modules'}
FORBID={'scope.hex','scope.bin','id_ed25519','id_rsa','authorized_keys','administrators_authorized_keys'}
PROMOTE14=['abi2-upgrade-final-checks-first/checks-first.json','abi2-upgrade-final-checks-first/freeze-first.json','root-abi2-upgrade-source-commit-first.json','abi2-upgrade-u1-normal-producer-first/evidence.json','abi2-upgrade-u1-dev-product-package-first/report.json','abi2-upgrade-u2-product-producer-first/evidence.json','abi2-upgrade-u2-dev-product-package-first/report.json','abi2-upgrade-u1-publication-second/report.json','abi2-upgrade-u1-publication-interop-first/report.json','abi2-upgrade-u1-product-runtime-first/report.json','abi2-upgrade-u1-u2-runtime-first/report.json','abi2-upgrade-failed-cleanup-runtime-first/report.json','abi2-upgrade-u1-u2-runtime-second/report.json','abi2-upgrade-postwrite-package-first/derivation-report.json','abi2-upgrade-postwrite-package-second/derivation-report.json','abi2-upgrade-postwrite-runtime-first/report.json','root-abi2-upgrade-postwrite-runtime-review-first-failure.json','root-abi2-upgrade-postwrite-runtime-review-second.json']
PROMOTE15=['71-ci-qualification-first/modern-report.json','71-ci-qualification-first/win7-report.json','71-ci-qualification-first/complete-git-tree-review-first.json','71-ci-original-metadata-first/selected-original-metadata.json','r06-test-module-order-first/checks-first.json','r06-test-module-order-first/source-relocation.json']

def need(v,m):
 if not v:raise AssertionError(m)
def sha(b):return hashlib.sha256(b).hexdigest()
def raw(j):return (json.dumps(j,ensure_ascii=False,indent=2)+'\n').encode()
def safe(n):
 p=Path(n);need(not p.is_absolute() and p.as_posix()==n and '..' not in p.parts and n!='.','CANONICAL_PATH');need(not any(any(ord(c)<32 for c in s) for s in p.parts),'PATH_CONTROL');return p
def put(p,b):
 p.parent.mkdir(parents=True,exist_ok=True)
 with p.open('xb') as f:f.write(b)
 need(p.read_bytes()==b,'WRITE_READBACK')
def identity(b):return {'bytes':len(b),'sha256':sha(b)}
def file_bytes(n):
 p=R/safe(n);need(p.is_file() and not p.is_symlink() and p.resolve()==p,'REGULAR_SOURCE');return p.read_bytes()
def is_binary(n,b):
 p=Path(n)
 return p.suffix.lower() in BINEXT or any(s in p.parts for s in ('payload','extracted','Binary','lifecycle','artifacts')) or p.name.startswith('Binary.') or re.fullmatch(r'(?:original|derived)-stream-\d+\.stdout\.log',p.name) is not None or re.fullmatch(r'binary-Fc\w+\.stdout\.log',p.name) is not None or b.startswith((b'MZ',b'MSCF',b'PK\x03\x04',b'\xd0\xcf\x11\xe0',b'\x7fELF',b'!<arch>\n'))

def plan(which):
 if which=='D16':return plan_cng()
 rows=[];bins=[];excluded=[];zips=[]
 dirs=DIRS14 if which=='D14' else DIRS15
 names=[]
 for name in dirs:
  root=R/name;need(root.is_dir() and not root.is_symlink(),'WHITELIST_DIRECTORY')
  for p in sorted(root.rglob('*')):
   rel=p.relative_to(R).as_posix();need(not p.is_symlink(),'NO_SYMLINK_SOURCE')
   if any(s in PRUNE for s in p.relative_to(root).parts):continue
   if p.is_file():names.append(rel)
 if which=='D14':
  names += [p.name for p in sorted(R.iterdir()) if p.is_file() and (p.name.startswith('root-abi2-upgrade-') or (p.name.startswith('abi2-upgrade-') and (p.name.endswith('-delivery-first.json') or p.name.endswith('-delivery-second.json') or p.name=='abi2-upgrade-normal-overlay-first.json')))]
 else:names.append('run-r06-test-module-order-checks-first.py')
 for n in sorted(set(names)):
  need(not any(s in FORBID or s.lower().endswith(('.pem','.key')) for s in safe(n).parts),'SENSITIVE_SOURCE_NAME_NOT_READ');b=file_bytes(n);row={'path':n,'source':{'file':n},**identity(b)}
  (bins if is_binary(n,b) else rows).append(row)
 if which=='D15':
  for label in ('modern','win7'):
   prefix=f'71-{label}-download-first/';zpath=prefix+'artifact.zip';zb=file_bytes(zpath);zips.append({'path':zpath,**identity(zb)})
   for n in ('download-report.json','zip-inventory.json'):
    b=file_bytes(prefix+n);rows.append({'path':prefix+n,'source':{'file':prefix+n},**identity(b)})
   with zipfile.ZipFile(io.BytesIO(zb)) as z:
    infos=[i for i in z.infolist() if not i.is_dir()];need(len(infos)<1000 and len({i.filename for i in infos})==len(infos),'UNIQUE_BOUNDED_ZIP')
    for i in infos:
     safe(i.filename);need(not i.flag_bits&1 and not stat.S_ISLNK(i.external_attr>>16) and i.file_size<=64*1024*1024,'REGULAR_BOUNDED_ZIP_MEMBER')
     b=z.read(i);n=prefix+'artifact-text/'+i.filename;row={'path':n,'source':{'zip':zpath,'member':i.filename},**identity(b)};(bins if is_binary(n,b) else rows).append(row)
 rows.sort(key=lambda r:r['path']);bins.sort(key=lambda r:r['path']);need(len({r['path'] for r in rows+bins})==len(rows+bins),'UNIQUE_FINITE_MEMBERS')
 return {'schema':'future-classroom.finite-archive/v1','archive':which,'whitelistDirectories':dirs,'originals':rows,'binaryIdentities':bins,'zips':zips,'promote':PROMOTE14 if which=='D14' else PROMOTE15,'originalTextBytes':sum(r['bytes'] for r in rows),'policy':'No checkout/target/G02/live trees, private keys, credentials, scope raw, school data, duplicated raw package-input. Binaries/rawpayload identity only; original text bytes preserved.'}

class Originals:
 def __init__(self,m):
  self.zips={}
  for r in m['zips']:
   b=file_bytes(r['path']);need(identity(b)=={k:r[k] for k in ('bytes','sha256')},'ZIP_EXACT_IDENTITY');self.zips[r['path']]=zipfile.ZipFile(io.BytesIO(b))
 def get(self,r):
  s=r['source'];b=file_bytes(s['file']) if 'file' in s else self.zips[s['zip']].read(s['member']);need(identity(b)=={k:r[k] for k in ('bytes','sha256')},'SOURCE_EXACT_'+r['path']);return b

def scanner(forms,discover=False):
 b=(HERE/'strict-scanner-original.py').read_bytes();need(len(b)==26272 and sha(b)=='da2906c450ea8c2ac7d18e358507023f519dbcc6faf1a9ac1abb1ae16af8c30f','EXACT_EXISTING_STRICT_SCANNER');ns={'__name__':'archive_scanner'};exec(compile(b,'<existing-strict-scanner>','exec'),ns)
 ns['TEXT_FORMS']={};ns['TEXT_DERIVED_VIEWS']={};stats={k:0 for k in ('textViews','utf16Views','cp936Views','clixmlDecodedViews','decodedBase64Views','jsonContainers','decodedJsonStringViews')};findings=[];table={r['path']:r for r in forms};seen=set()
 def reviewed(data,label):
  if label is None:return None
  relative=label
  for prefix in ('source:','stored-tar-gzip-decoded:','promoted:','archive-metadata:'):
   if isinstance(label,str) and label.startswith(prefix):relative=label[len(prefix):]
  try:text=data.decode('utf-16') if data.startswith((b'\xff\xfe',b'\xfe\xff')) else data.decode('utf-8-sig')
  except UnicodeError:return None
  controls=[(i,ord(c)) for i,c in enumerate(text) if ord(c)<32 and c not in '\n\r\t']
  if not controls:return None
  kinds={v for _,v in controls};kind=None;sequences=[]
  if kinds=={5}:
   if all(re.fullmatch(r'MSI \(s\) \([0-9A-F]{2}:[0-9A-F]{2}\) \[[0-9]{2}:[0-9]{2}:[0-9]{2}:[0-9]{3}\]: Note: 1: 2262 2: \x05DigitalSignature 3: -2147287038 \r?',line) for line in text.split('\n') if '\x05' in line):kind='EXACT_MSI_DIGITAL_SIGNATURE_NOTE'
   elif all(line=='\x05SummaryInformation' for line in text.splitlines() if '\x05' in line):kind='EXACT_MSI_SUMMARY_STREAM_NAME'
  elif kinds=={0} and relative.endswith(('original-table-_ForceCodepage.stdout.log','derived-table-_ForceCodepage.stdout.log')):
   need(data==b'\r\n\r\n0\t_ForceCodepage\r\n\0','EXACT_CODEPAGE_EXPORT_BYTES');kind='EXACT_MSI_FORCECODEPAGE_EXPORT'
  elif kinds=={0} and '/artifact-text/logs/source-' in relative:
   records=text.split('\0');need(records[-1]=='' and len(records)==19844,'EXACT_GIT_19843_RECORDS');paths=[]
   if 'source-git-tree-' in relative:
    for rec in records[:-1]:
     m=re.fullmatch(r'(100644|100755|120000) (blob) ([0-9a-f]{40}) +([0-9]+)\t([^\x00]+)',rec);need(m is not None and str(int(m[4]))==m[4] and int(m[4])<=64*1024*1024,'STRICT_GIT_TREE_RECORD');safe(m[5]);paths.append(m[5])
    kind='EXACT_GIT_TREE_19843_Z_RECORDS'
   elif Path(relative).name in ('source-input-paths.stdout.log','source-input-paths-after.stdout.log'):
    for rec in records[:-1]:safe(rec);paths.append(rec)
    kind='EXACT_GIT_PATHS_19843_Z_RECORDS'
   need(kind and len(set(paths))==19843,'UNIQUE_GIT_RECORDS')
  elif kinds=={12} and relative.endswith('/artifact-text/THIRD-PARTY-NOTICES.txt'):
   need(len(controls)==18,'EXACT_NOTICE_18FF');kind='EXACT_NOTICE_18_FORM_FEEDS'
  elif kinds=={27} and relative in ('71-ci-original-metadata-first/win7-job-original-decoded.log','abi2-upgrade-implementation-first/fmt-final.stdout.log'):
   allowed=('[0m','[36;1m') if relative.startswith('71-') else ('[31m','[32m','[m','(B')
   matches=list(re.finditer('\x1b(?:'+ '|'.join(re.escape(v) for v in allowed)+')',text));need([m.start() for m in matches]==[i for i,_ in controls],'EXACT_RESTRICTED_ANSI_SEQUENCES');sequences=[{'offset':m.start(),'sequenceEscaped':m.group().encode('unicode_escape').decode()} for m in matches];kind='EXACT_WIN7_JOB_SGR' if relative.startswith('71-') else 'EXACT_RUSTFMT_DIFF_COLOR_AND_ASCII_RESET'
  if kind is None:return None
  row={'path':relative,**identity(data),'textForm':kind,'characterControlCounts':{str(v):sum(c==v for _,c in controls) for v in kinds},'originalControlByteOffsets':([2+2*i for i,_ in controls] if data.startswith((b'\xff\xfe',b'\xfe\xff')) else [m.start() for m in re.finditer(b'[\x00\x05\x0c\x1b]',data)]),'sequences':sequences}
  if discover:
   if relative in table:need(table[relative]==row,'CONSISTENT_DISCOVERED_FORM')
   else:table[relative]=row;forms.append(row)
  else:need(relative in table and table[relative]==row,'EXACT_FROZEN_STRUCTURAL_EXCEPTION_'+relative)
  seen.add(relative);visible=text;concat=text
  if kinds=={27}:
   visible=re.sub('\x1b(?:'+ '|'.join(re.escape(v) for v in allowed)+')','[EXACT_ANSI]',text);concat=re.sub('\x1b(?:'+ '|'.join(re.escape(v) for v in allowed)+')','',text)
  else:
   for c in kinds:visible=visible.replace(chr(c),'[U+%04X]'%c);concat=concat.replace(chr(c),'')
  return [('utf16-reviewed' if data.startswith((b'\xff\xfe',b'\xfe\xff')) else 'utf8-reviewed',text),('reviewed-visible',visible),('reviewed-concatenated',concat)]
 ns['reviewed_control_views']=reviewed;original=ns['text_views']
 def views(data,label=None):
  v=original(data,label)
  for _,t in v:need(not any(c in '\u202a\u202b\u202c\u202d\u202e\u2066\u2067\u2068\u2069\u061c\u200e\u200f\x7f' for c in t),'BIDI_OR_DEL_FORBIDDEN')
  return v
 ns['text_views']=views
 def scan(data,label):
  try:
   ns['credential_scan'](data,label,stats,findings)
   if label.endswith(('.jsonl','.log','.LOG')):
    for i,line in enumerate(data.splitlines()):
     if line.lstrip().startswith((b'{',b'[')):
      need(i<250000,'JSONL_BOUND')
      try:json.loads(line)
      except (ValueError,UnicodeError):need(not label.endswith('.jsonl'),'DECLARED_JSONL_VALID');continue
      ns['credential_scan'](line,label+'::jsonl/'+str(i),stats,findings)
   need(not findings,'CREDENTIAL_FINDING_'+json.dumps(findings))
  except Exception as e:raise AssertionError('SCAN_PATH '+label+' '+str(e)) from e
 return scan,stats,seen

def review(which,get,m):
 if which=='D16':return review_cng(get,m)
 by={r['path']:r for r in m['originals']+m['binaryIdentities']}
 def j(n):return json.loads(get(n))
 if which=='D14':
  n='abi2-upgrade-final-checks-first/freeze-first.json';need(sha(get(n))=='1228fc5bfef69c29ce03c1de1eb8d337dad41eec20d3bb5b6a8360b097f28d87','ROOT_FINAL_FREEZE');need(len([n for n in by if n.startswith('abi2-upgrade-final-checks-first/source-final/')])==21,'FINAL_21_SOURCE_TEXT')
  pins={'abi2-upgrade-u1-product-runtime-first/report.json':'4605e523204533938dc2923d2c3d1f5c4f16d507500b868b64852fa34b876a03','abi2-upgrade-u1-u2-runtime-first/report.json':'653c960f7f9f7e70bb0863be17aa40c3e654e7d2ae8d10f0cd84b61fb4163364','abi2-upgrade-failed-cleanup-runtime-first/report.json':'fb8b47bb00e53648369fe87f380066412bb9fa71a2dbfab1ccfb45fb60d26f4e','abi2-upgrade-u1-u2-runtime-second/report.json':'a89061bfdaaee4ce3581207c5a7c16b482ee6569d25ee891e66a95bf40374d43','abi2-upgrade-postwrite-runtime-first/report.json':'a8c434d0f478810a4b84de882db32d04920fc3f94dad265d49ff47960a3b9932'}
  for n,h in pins.items():need(sha(get(n))==h,'ROOT_PINNED_RUNTIME_REPORT')
  return {'scope':'GNU DEV Upgrade implementation/static compiler evidence and separately actual Win11 U1 normal, U1→U2 normal and bounded Type19 rollback. Final combined GNU static is not native evidence. 21source-final is complete including new files; complete.patch tracked diff only. Candidate b6b18631379e61b1971c20b361296db8671eb885 locally committed, not published by archive. Original overlay KeyError, normal observer FAIL, formal cleanup PASS, first Type19 local pseudo-table FAIL, second derivation PASS and root review TypeError preserved. Actual source publication cache equality is PRODUCTION_SUCCESS_BRANCH_INFERENCE, nativeCacheDigestSerialized=false; SOURCE31 canonical interop is not direct cache receipt. Begin/End numeric API receipts NOT_SERIALIZED. No Type1 full14 intermediate witness; actual rollback proof includes old damaged preimage, components/registry/cache and U2 absence. School data retention/Run-enabled Upgrade and Win7/Win10/Seewo runtime NOT_RUN.','pinnedRuntimeReports':pins}
 summaries=[]
 for label,count in [('modern',51),('win7',53)]:
  q=j(f'71-ci-qualification-first/{label}-report.json');prefix=f'71-{label}-download-first/artifact-text/';res=j(prefix+'result.json');commands=res['commands'];need(q['sourceSha']==res['source_sha']==res['dispatch_source_sha']==SOURCE and q['allNativeRuntime']=='NOT_RUN' and q['laterUpgradeSource']=='NOT_COVERED_BY_71CI','CI_SOURCE_SCOPE');need(len(commands)==len(q['allCommands'])==count and len({c['name'] for c in commands})==count and {c['name']:c for c in q['allCommands']}=={c['name']:c for c in commands},'COMPLETE_ORIGINAL_COMMANDS_NAME_BIJECTION');fails=[]
  for c in commands:
   actual=j(prefix+'logs/'+c['name']+'.result.json');need(actual==c and c['parent_exited'] and not c['timed_out_or_pipe_stalled'] and not c['log_truncated'],'ORIGINAL_COMMAND_CAP_TERMINAL')
   if c['exit_code']!=0:fails.append((c['name'],c['exit_code']))
   for stream in ('stdout','stderr'):need(sha(get(prefix+c[stream]))==c[stream+'_sha256'],'ORIGINAL_COMPLETE_LOG_SHA')
  need(fails==([] if label=='modern' else [('clippy',101)]),'EXACT_ORIGINAL_FAILURE_SET');need(res['status']==('COMPILE_CHECKS_PASSED_RUNTIME_NOT_RUN' if label=='modern' else 'FAILED_OR_INCOMPLETE'),'PRESERVE_CI_RESULT')
  inputs=j(prefix+'input-sha256.json');need(inputs==j(prefix+'input-sha256-after.json') and len(inputs)==q['committedInputRowsVerified']==len(q['committedInputMaterializationProof'])==648,'CI648_BEFORE_AFTER_PROOF');ix={r['path']:r for r in inputs}
  for proof in q['committedInputMaterializationProof']:
   r=ix[proof['path']];need(r['bytes']==proof['ciCheckoutBytes'] and r['sha256']==proof['ciCheckoutSha256'] and re.fullmatch('[0-9a-f]{40}',proof['gitBlobOid']) is not None and re.fullmatch('[0-9a-f]{64}',proof['gitBlobSha256']) is not None,'648_FROZEN_COMMITTED_BLOB_PROOF')
  for n in ('input-domains','input-git-tree'):
   need(get(prefix+n+'.json')==get(prefix+n+'-after.json'),'TREE_AND_DOMAIN_BYTES_BEFORE_AFTER')
  tree=j(prefix+'input-git-tree.json');need(tree['source_sha']==SOURCE and len(tree['files'])==19843,'TREE19843');need(len({r['path'] for r in tree['files']})==19843,'TREE_UNIQUE');need(sum(r['domain']=='sha256_input' for r in tree['files'])==648,'TREE_DOMAINS648')
  for r in tree['files']:
   safe(r['path']);need(r['mode'] in ('100644','100755','120000') and r['type']=='blob' and re.fullmatch('[0-9a-f]{40}',r['git_blob_oid_sha1']) is not None and isinstance(r['git_blob_bytes'],int) and 0<=r['git_blob_bytes']<=64*1024*1024,'ALL19843_EXACT_TREE_FIELDS');need(r['domain']==('historical_git_archive' if r['path'].startswith('开发计划/验证记录/') else 'sha256_input'),'ALL19843_EXACT_DOMAIN')
  need(sha(get(prefix+'input-git-tree.json'))==res['git_tree_manifest_sha256'] and sha(get(prefix+'input-domains.json'))==res['input_domains_manifest_sha256'] and sha(get(prefix+'input-sha256.json'))==res['input_manifest_sha256'],'ORIGINAL_RESULT_INPUT_MANIFEST_HASHES')
  ziprow=next(r for r in m['zips'] if r['path']==f'71-{label}-download-first/artifact.zip');need(ziprow['bytes']==q['zip']['bytes'] and ziprow['sha256']==q['zip']['sha256'],'QUALIFIED_ZIP_IDENTITY')
  members={r['source']['member']:r for r in m['originals']+m['binaryIdentities'] if r['source'].get('zip')==ziprow['path']}
  for r in j(prefix+'artifact-sha256.json'):need(members[r['path']]['bytes']==r['bytes'] and members[r['path']]['sha256']==r['sha256'],'ZIP_MEMBER_MANIFEST_HASH')
  for r in q['ciMemberInputs']:need(members[r['member']]['bytes']==r['bytes'] and members[r['member']]['sha256']==r['sha256'],'QUALIFIED_MEMBER_INPUTS')
  exes=j(prefix+'executables.json');need(len(exes)==25,'25_PE_IDENTITIES')
  for e in exes:need(members[e['relative_path']]['bytes']==e['bytes'] and members[e['relative_path']]['sha256']==e['sha256'] and not e['executed'],'ACTUAL_PE_IDENTITY_ONLY')
  summaries.append({'label':label,'commands':count,'failures':fails,'inputRows':648,'treeRows':19843,'runtime':'NOT_RUN','originalResult':res['status']})
 checks=j('r06-test-module-order-first/checks-first.json');need(checks['status']=='PASS' and len(checks['operations'])==6 and all(o['actualExit']==0 and o['originalReaped'] for o in checks['operations']),'R06_SIX_REAL_CHECKS');need(b'61 passed' in get('r06-test-module-order-first/presentation-tests.stdout.log'),'R06_61_REAL_TESTS')
 return {'scope':'71 official MSVC compiler evidence only. Modern51 actual0; Win7 original53 includes exactly Clippy101. Later pure R06 WindowRole block relocation +61host tests and6 GNU/static commands PASS does not change original Win7 FAIL or establish new official CI success. All71 CI native Win7/Win10/Win11/Seewo NOT_RUN; later21 Upgrade source not covered. All original ZIP text members retained; PE/ZIP identity only.','builds':summaries,'historicalContents':'19843 Git tree metadata fully reviewed;19195 historical file contents not re-read or copied.'}


CNG_ROOTS=['password-cng-interop-preparation-first','password-cng-interop-preparation-second','password-cng-interop-runtime-5a3702d9164859cd1efe6df65bd4b49e','password-cng-interop-runtime-182309509a8e143e3875f8023edf1049']
CNG_ROOT_FILES=['root-password-cng-first-failure-cleanup.json','root-password-cng-first-failure-cleanup.py','root-password-cng-first-failure-cleanup.stderr.log','root-password-cng-first-failure-cleanup.stdin.ps1','root-password-cng-first-failure-cleanup.stdout.log','root-password-cng-interop-review-first.json','root-password-cng-interop-review-first.py']
CNG_FIXTURE_SHA='36c603da5c699a67e5211499edfafc1ae2af8c69fd591469d8895af440b50a0c'
CNG_FIRST='password-cng-interop-runtime-5a3702d9164859cd1efe6df65bd4b49e/report.json'
CNG_SECOND='password-cng-interop-runtime-182309509a8e143e3875f8023edf1049/report.json'

def plan_cng():
 names=[];rows=[];ids=[];diagnosis=json.loads((HERE/'original-scan-diagnosis-first.json').read_bytes());rejected={r['path']:r for r in diagnosis['rejectedOriginals']};need(len(rejected)==8 and all(r['bytes']==2372 and r['sha256']==CNG_FIXTURE_SHA for r in rejected.values()),'EIGHT_EXACT_PUBLIC_SYNTHETIC_INPUT_IDENTITIES')
 for n in CNG_ROOTS:
  root=R/n;need(root.is_dir() and not root.is_symlink(),'EXACT_CNG_DIRECTORY')
  for p in sorted(root.rglob('*')):
   need(not p.is_symlink(),'CNG_NO_SYMLINK')
   if p.is_file() and not any(s in PRUNE for s in p.relative_to(root).parts):names.append(p.relative_to(R).as_posix())
 names+=CNG_ROOT_FILES
 for n in sorted(names):
  need(not any(s in FORBID or s.lower().endswith(('.pem','.key')) for s in safe(n).parts),'NO_SENSITIVE_INPUT_READ');b=file_bytes(n);row={'path':n,'source':{'file':n},**identity(b)}
  if n in rejected:
   need(identity(b)=={k:rejected[n][k] for k in ('bytes','sha256')} and json.loads(b)['classification']=='PUBLIC_SYNTHETIC_TEST_FIXTURES_NO_REAL_ACCOUNT_OR_SECRET','EXACT_PUBLIC_FIXTURE_CLASSIFICATION');row['identityOnlyReason']='PUBLIC_SYNTHETIC_TEST_FIXTURES_IDENTITY_ONLY_DUE_TO_STRICT_PASSWORD_PATTERN';ids.append(row)
  else:need(not is_binary(n,b),'NO_CNG_BINARY_CONTENT_ALLOWED');rows.append(row)
 need(len(rows)==151 and len(ids)==8,'FINITE159_ORIGINAL_DOMAIN');host=json.loads(file_bytes('password-cng-interop-preparation-first/host-first/report.json'));promote=[CNG_FIRST,CNG_SECOND,'password-cng-interop-preparation-first/host-first/report.json','root-password-cng-interop-review-first.json','root-password-cng-first-failure-cleanup.json']
 return {'schema':'future-classroom.finite-archive/v1','archive':'D16','whitelistDirectories':CNG_ROOTS,'whitelistRootFiles':CNG_ROOT_FILES,'originals':rows,'binaryIdentities':ids,'identityOnlyText':diagnosis['rejectedOriginals'],'externalBinaryIdentities':[host['probeBinary']],'zips':[],'promote':promote,'originalTextBytes':sum(r['bytes'] for r in rows),'policy':'PUBLIC_SYNTHETIC_TEST_FIXTURES_IDENTITY_ONLY_DUE_TO_STRICT_PASSWORD_PATTERN: eight exact input copies intentionally not stored. All151 other original texts preserved unchanged. No credential exceptions, targets, original binary contents, real account/school data, Git/Windows/build actions.'}

def review_cng(get,m):
 by={r['path']:r for r in m['originals']+m['binaryIdentities']}
 def j(n):return json.loads(get(n))
 for directory,count in [('password-cng-interop-preparation-first',34),('password-cng-interop-preparation-second',39)]:
  f=j(directory+'/freeze-first.json');need(len(f['files'])==count,'EXACT_CNG_FROZEN_PREPARATION_COUNT');expected={directory+'/'+r['path'] for r in f['files']}|{directory+'/freeze-first.json'};need(expected=={n for n in by if n.startswith(directory+'/')},'EXACT_CNG_PREPARATION_FREEZE_MEMBERSHIP')
  for r in f['files']:
   actual=by[directory+'/'+r['path']];need(actual['bytes']==r['bytes'] and actual['sha256']==r['sha256'],'CNG_FROZEN_ORIGINAL_BYTES_SHA')
 host=j('password-cng-interop-preparation-first/host-first/report.json');need(host['sourceUnchanged'] and host['sourceBefore']==host['sourceAfter'] and len(host['commands'])==4 and all(c['actualExit']==0 and c['originalReaped'] for c in host['commands']),'REAL_PRODUCTION_RUST_HOST_COMMANDS');need(len(host['cases'])==4 and all(c['iterations']==600000 and c['correctVerified'] and not c['wrongVerified'] and c['pythonOpenSslMatches'] for c in host['cases']),'FOUR_HOST_RUST_PYTHON_PRODUCTION600000')
 for c in host['commands']:
  for stream in ('stdout','stderr'):
   row=c[stream];n=Path(row['path']).relative_to(R).as_posix();need(by[n]['bytes']==row['bytes'] and by[n]['sha256']==row['sha256'],'HOST_ORIGINAL_COMMAND_STREAM_HASH')
 first=j(CNG_FIRST);second=j(CNG_SECOND);need(first['status']=='FAIL_RETAIN_ORIGINALS' and first['runtimeActualExit']==1 and first['native']['actualExit']==1,'PRESERVE_FIRST_CNG_FAILURE');need(sha(get(CNG_SECOND))=='bd5b66be7849c0bb276d811571e9214a265cb75a4c3cee52735ecaa69807a774','ROOT_PINNED_CNG_SUCCESS');need(second['status']=='PASS_PRODUCTION_RUST_PYTHON_AND_WIN11_CNG_FOUR_SYNTHETIC_600000' and second['runtimeActualExit']==0 and second['native']['actualExit']==0,'ACTUAL_CNG_SECOND_SUCCESS')
 for result,code in [(first,1),(second,0)]:
  cap=result['native']['original'];need(cap['Wait']==0 and cap['ExitKnown'] and cap['ExitCode']==code and cap['CheckedClose'] and result['completeDownloadVerified'],'CNG_ORIGINAL_PROCESS_ACTUAL_WAIT_EXIT_CLOSE_DOWNLOAD')
 cng=second['native']['cng'];need(cng['processBits']==64 and cng['iterations']==600000 and len(cng['cases'])==4,'CNG_WIN11_X64_FOUR600000');hostcases={c['id']:c for c in host['cases']}
 for c in cng['cases']:
  need(c['opened'] and c['derived'] and c['closeAttempted'] and c['checkedClose'] and c['openStatus']==c['deriveStatus']==c['closeStatus']==0 and c['rustMatch'] and c['verifierHex']==hostcases[c['id']]['verifierHex'],'FOUR_ACTUAL_CNG_PROVIDER_HANDLES_DERIVE_AND_CLOSE_MATCH')
 root=j('root-password-cng-interop-review-first.json');need(root['windowsNonceCleanup']['knownAbsent'] and root['hostBuildTargetCleanup']['knownAbsent'] and root['hostBuildTargetCleanup']['originalBinaryIdentity']==host['probeBinary'],'ORIGINAL_HOST_BINARY_IDENTITY_AND_KNOWN_OWNED_CLEANUP');need(j('root-password-cng-first-failure-cleanup.json')['actualExit']==0,'FIRST_FAILED_NONCE_INDEPENDENT_CLEANUP')
 need(j('password-cng-interop-preparation-first/ast-first.json')['actualExit']==255 and j('password-cng-interop-preparation-first/ast-second.json')['actualExit']==0 and j('password-cng-interop-preparation-second/ast-third.json')['actualExit']==0,'PRESERVE_ORIGINAL_AST_REQUEST255_AND_CORRECTED_AST0')
 return {'scope':'Independent Win11 x64 CNG API comparison only: four original provider handles open/derive/close all actual0 and production Rust+Python+CNG600000 rounds agree for four public synthetic vectors. First AST SSH request-length255, later AST0, first PS5 ulong type-error actual1 before CNG, independent failed nonce cleanup and successful UInt64 second run retained. Windows/host exact test roots cleaned according to original independent receipts. No Win7/Win10 run, no old user database migration, no complete P0 performance result, no production password or school data. Finite151 text originals retained; eight exact PUBLIC_SYNTHETIC_TEST_FIXTURES_IDENTITY_ONLY_DUE_TO_STRICT_PASSWORD_PATTERN copies omitted as identity only under unchanged strict credential rules. No credential exceptions. Original host executable represented by original report identity only; not read or archived.', 'retainedTextCount':151,'identityOnlyPublicInputCopies':8,'hostProbeBinaryIdentity':host['probeBinary'],'preparationFrozenFileCounts':[34,39],'originalRuntimeReportSha256':sha(get(CNG_SECOND))}

def main():
 p=argparse.ArgumentParser(description=__doc__);p.add_argument('--archive',choices=['D14','D15','D16']);p.add_argument('--plan',action='store_true');p.add_argument('--execute',action='store_true');p.add_argument('--verify-only',action='store_true');p.add_argument('--directory',type=Path);p.add_argument('--expected-freeze-sha256');a=p.parse_args()
 if not any((a.plan,a.execute,a.verify_only)):print('{"status":"PREPARED_NOT_EXECUTED","Windows":false,"Git":false}');return
 need(sum((a.plan,a.execute,a.verify_only))==1 and a.archive,'EXACT_MODE_AND_SCOPE');d=a.directory
 if a.plan:
  need(d is not None and not d.exists(),'FRESH_PLAN_DIRECTORY');d.mkdir();m=plan(a.archive);s=Originals(m);forms=[];scan,stats,_=scanner(forms,True)
  for row in m['originals']:scan(s.get(row),'source:'+row['path'])
  review(a.archive,lambda n:s.get(next(r for r in m['originals'] if r['path']==n)),m)
  for n,b in [('source-manifest-first.json',raw(m)),('text-forms-first.json',raw(sorted(forms,key=lambda r:r['path']))),('archive.py',Path(__file__).read_bytes()),('strict-scanner-original.py',(HERE/'strict-scanner-original.py').read_bytes())]:put(d/n,b)
  print(json.dumps({'status':'PASS_FINITE_PLAN_AND_STRICT_ORIGINAL_SCAN','archive':a.archive,'textMembers':len(m['originals']),'textBytes':m['originalTextBytes'],'binaryIdentities':len(m['binaryIdentities']),'structuralExceptions':len(forms),'statistics':stats}));return
 need(d is not None and d.resolve()==HERE,'RUN_ARCHIVE_COPY_IN_DESTINATION');m=json.loads((d/'source-manifest-first.json').read_bytes());need(m['archive']==a.archive,'EXACT_SCOPE_MANIFEST');rows=m['originals'];by={r['path']:r for r in rows};scan,stats,seen=scanner(json.loads((d/'text-forms-first.json').read_bytes()))
 if a.verify_only:
  fb=(d/'freeze-first.json').read_bytes();need(a.expected_freeze_sha256 and sha(fb)==a.expected_freeze_sha256,'CALLER_PINNED_FREEZE');f=json.loads(fb);actual={p.relative_to(d).as_posix() for p in d.rglob('*') if p.is_file() and p!=d/'freeze-first.json'};need(actual=={r['path'] for r in f['files']},'EXACT_FROZEN_MEMBERSHIP')
  for row in f['files']:b=(d/safe(row['path'])).read_bytes();need(identity(b)=={k:row[k] for k in ('bytes','sha256')},'ARCHIVE_FILE_EXACT_FREEZE')
 else:
  need({p.name for p in d.iterdir()}=={'archive.py','strict-scanner-original.py','source-manifest-first.json','text-forms-first.json'},'FRESH_ASSEMBLY_DIR');s=Originals(m)
  for row in rows:scan(s.get(row),'source:'+row['path'])
  for row in m['binaryIdentities']:s.get(row)
  review(a.archive,lambda n:s.get(by[n]),m)
  with (d/'evidence-originals-first.tar.gz').open('xb') as outer:
   with gzip.GzipFile(filename='',fileobj=outer,mode='wb',compresslevel=9,mtime=0) as gz:
    with tarfile.open(fileobj=gz,mode='w',format=tarfile.PAX_FORMAT) as t:
     for row in rows:
      b=s.get(row);i=tarfile.TarInfo(row['path']);i.size=len(b);i.mode=0o600;i.uid=i.gid=i.mtime=0;i.uname=i.gname='';t.addfile(i,io.BytesIO(b))
 stored={}
 with tarfile.open(d/'evidence-originals-first.tar.gz','r:gz') as t:
  infos=t.getmembers();need([i.name for i in infos]==[r['path'] for r in rows],'EXACT_TAR_MEMBERS')
  for i,row in zip(infos,rows):
   need(i.isfile() and i.mode==0o600 and i.uid==i.gid==i.mtime==0 and not i.uname and not i.gname,'REGULAR_CANONICAL_TAR');b=t.extractfile(i).read();need(identity(b)=={k:row[k] for k in ('bytes','sha256')},'FULL_TAR_BYTE_REPLAY')
   if a.execute:need(b==s.get(row),'FULL_ORIGINAL_EQUALITY')
   scan(b,'stored-tar-gzip-decoded:'+row['path']);stored[row['path']]=b
 facts=review(a.archive,stored.__getitem__,m)
 for n in m['promote']:
  if a.execute:put(d/safe(n),stored[n])
  need((d/n).read_bytes()==stored[n],'PROMOTED_FULL_ORIGINAL');scan(stored[n],'promoted:'+n)
 if a.execute:
  ab=(d/'evidence-originals-first.tar.gz').read_bytes();docs={'archive-index-first.json':{'status':'PASS_FINITE_ORIGINAL_ARCHIVE','scope':facts,'archive':{'path':'evidence-originals-first.tar.gz',**identity(ab),'members':len(rows)},'originalTextBytes':m['originalTextBytes'],'binaryIdentityCount':len(m['binaryIdentities']),'originalZips':m['zips'],'identityOnlyText':m.get('identityOnlyText',[]),'externalBinaryIdentities':m.get('externalBinaryIdentities',[]),'sourceDeletion':False,'Windows':False,'Git':False},'archive-verification-first.json':{'status':'PASS_FULL_TAR_ORIGINAL_BYTE_REPLAY_STRICT_SCAN','members':len(rows),'credentialFindings':[],'statistics':stats,'coverage':'Unchanged pinned existing scanner UTF8/UTF16/CP936/CLIXML/JSON/JSONL/nestedbase64; depth12/64MiB decoded/250000 bounds. Extra BIDI/DEL rejection; exact structural exceptions by full origin+decoded view path/bytes/SHA/complete offsets and grammar. All raw, visible and concatenated views scanned. No ANSI stripping of originals.','sourceDeletion':False},'README.md':('# '+a.archive+' finite evidence\n\n'+facts['scope']+'\n\nOriginal text is preserved byte for byte in evidence-originals-first.tar.gz. Binary material is represented only by the complete identities in source-manifest-first.json. Failed original operations remain failed; subsequent verification does not rewrite them. No Windows, Git, source removal, download, build, or permission changes are performed by this archive.\n').encode()}
  for n,j in docs.items():put(d/n,j if isinstance(j,bytes) else raw(j))
  for z in m['zips']:need(identity(file_bytes(z['path']))=={k:z[k] for k in ('bytes','sha256')},'ZIP_UNCHANGED_AFTER_ARCHIVE')
 for p in d.rglob('*'):
  need(not p.is_symlink(),'NO_STAGING_SYMLINK')
  if p.is_file() and p.name not in ('evidence-originals-first.tar.gz','freeze-first.json') and p.relative_to(d).as_posix() not in m['promote']:scan(p.read_bytes(),'archive-metadata:'+p.relative_to(d).as_posix())
 if a.execute:
  files=[{'path':p.relative_to(d).as_posix(),**identity(p.read_bytes())} for p in sorted(d.rglob('*')) if p.is_file()];put(d/'freeze-first.json',raw({'status':'FROZEN_FINITE_ARCHIVE','archive':a.archive,'files':files,'actions':{'Windows':False,'Git':False,'Cargo':False,'sourceDeletion':False}}))
 print(json.dumps({'status':'PASS_VERIFY_ONLY_NO_WRITES' if a.verify_only else 'PASS_ACTUAL_FINITE_ARCHIVE','archive':a.archive,'members':len(rows),'textBytes':m['originalTextBytes'],'binaryIdentities':len(m['binaryIdentities']),'archiveBytes':(d/'evidence-originals-first.tar.gz').stat().st_size,'archiveSha256':sha((d/'evidence-originals-first.tar.gz').read_bytes()),'freezeSha256':sha((d/'freeze-first.json').read_bytes()),'credentialFindings':0,'statistics':stats,'Windows':False,'Git':False,'Cargo':False,'sourceDeletion':False}))
if __name__=='__main__':main()
