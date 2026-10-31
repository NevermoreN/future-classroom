#!/usr/bin/env python3
"""Read-only R41 evidence manifest/secret/reference review; never copies artifacts."""
from pathlib import Path
import base64,collections,datetime,hashlib,json,os,re,stat,urllib.parse
E=Path('/tmp/fc-round41-frontdoor-deadline-evidence');OUT=E/'archive-preparation-first.json'
assert not OUT.exists(), 'preserve prior manifest'
sha=lambda b:hashlib.sha256(b).hexdigest()
allowed={'.json','.jsonl','.log','.py','.mjs','.ps1','.rs','.txt','.diff','.patch','.pem','.stdout','.stderr','.raw'}
blocked=[];exclusions=[];candidates=[];decoded=[];annotations=[];refs=collections.defaultdict(set);jsons={};seen=set()
credential_paths=('/config/.ssh/','/home/','/root/.ssh/')
def sensitive_path(p):
 s=str(p)
 return s.startswith(credential_paths) or p.name.startswith(('id_ed25519','id_rsa')) or s.endswith('.control.json')
def reference(value,origin):
 for match in re.finditer(r'/(?:tmp|workspace|mnt/data|config|home)/[^\s\"\'<>;|\]\[{},()]+',value):
  text=match.group().rstrip('.:')
  if len(text)<1024:refs[text].add(origin)
 if value.startswith('/') and '\n' not in value and len(value)<1024 and not value.startswith('//'):
  refs[value].add(origin)
def finding(kind,origin,value):
 item={'kind':kind,'origin':origin,'valueBytes':len(value.encode()),'valueSha256':sha(value.encode())}
 if item not in blocked:blocked.append(item)
def scan_text(text,origin,depth=0):
 # Never print or store a found secret. Only kind/location/nonreversible digest.
 for pattern,kind in [(r'-----BEGIN(?: [A-Z0-9]+)? PRIVATE KEY-----','PRIVATE_KEY_PEM'),
  (r'\b(?:gh[pousr]_[A-Za-z0-9]{24,}|github_pat_[A-Za-z0-9_]{30,})\b','GITHUB_TOKEN'),
  (r'\bsk-(?:proj-)?[A-Za-z0-9_-]{30,}\b','API_SECRET'),
  (r'__Host-FutureClassroom=[0-9a-fA-F]{64}','SESSION_COOKIE'),
  (r'(?i)(?:stopCapability|expectedCsrf|csrf)\s*[=:]\s*[\"\']([0-9a-f]{64})[\"\']','CAPABILITY_OR_CSRF')]:
  for m in re.finditer(pattern,text):
   val=m.group(1) if m.lastindex else m.group()
   # Deliberate f/b/0/a synthetic fixture values are source models, never live capabilities.
   tail=val.split('=',1)[-1]
   if len(tail)==64 and len(set(tail.lower()))==1:
    annotations.append({'kind':'REPETITIVE_SYNTHETIC_FIXTURE_VALUE','origin':origin,'valueSha256':sha(val.encode())})
   else:finding(kind,origin,val)
 for m in re.finditer(r'https?://[^\s\"\'<>]+',text):
  url=m.group().rstrip('),;]');parsed=urllib.parse.urlsplit(url)
  keys={k.lower() for k,v in urllib.parse.parse_qsl(parsed.query)}
  if parsed.username or parsed.password or keys & {'x-amz-signature','x-goog-signature','signature','sig','access_token'}:
   finding('SIGNED_OR_CREDENTIALED_URL',origin,url)
 reference(text,origin)
 if depth<6:
  for m in re.finditer(r"(?:FromBase64String\(\s*['\"]|[\-]EncodedCommand\s+)([A-Za-z0-9+/]{32,}={0,2})",text):
   decode(m.group(1),origin+'#embedded-base64',depth+1)
def decode(value,origin,depth):
 try:data=base64.b64decode(value,validate=True)
 except (ValueError,TypeError):return
 ident=sha(data)
 if ident in seen:return
 seen.add(ident)
 row={'origin':origin,'bytes':len(data),'sha256':ident};decoded.append(row)
 if len(data)>=128 and data[:2]==b'MZ':
  offset=int.from_bytes(data[60:64],'little')
  if data[offset:offset+4]==b'PE\0\0':finding('INLINE_EXECUTABLE_BASE64',origin,ident);return
 if data.startswith(b'PK\x03\x04'):
  finding('INLINE_ZIP_BASE64_REVIEW',origin,ident);return
 try:
  encoding='utf-16le' if len(data)>2 and data[1::2].count(0)>len(data)//4 else 'utf-8-sig'
  text=data.decode(encoding)
 except UnicodeDecodeError:
  row['encoding']='NON_TEXT_PUBLIC_HASH_OR_CERTIFICATE';return
 row['encoding']=encoding;scan_text(text,origin,depth)
 if depth<6:
  try:obj=json.loads(text)
  except (ValueError,TypeError):return
  walk(obj,origin,depth+1)
def walk(obj,origin,depth=0):
 if depth>6:return
 if isinstance(obj,dict):
  for k,v in obj.items():
   where=origin+'/'+str(k)
   if isinstance(v,str):
    reference(v,where)
    if 'base64' in str(k).lower():decode(v,where,depth+1)
    if k.lower() in {'stopcapability','csrf','expectedcsrf','password','access_token','token'} and v:
     if re.fullmatch('[0-9a-fA-F]{64}',v):
      if len(set(v.lower()))>1:finding('SENSITIVE_JSON_VALUE',where,v)
     elif k.lower() in {'password','access_token'} and len(v)>8 and not any(t in v.lower() for t in ['synthetic','fixture','not_run','omitted','redacted']):
      finding('PASSWORD_OR_TOKEN_JSON_VALUE_REVIEW',where,v)
   elif isinstance(v,(dict,list)):walk(v,where,depth+1)
 elif isinstance(obj,list):
  for i,v in enumerate(obj):
   if isinstance(v,str):reference(v,origin+'/'+str(i))
   elif isinstance(v,(dict,list)):walk(v,origin+'/'+str(i),depth+1)
paths=sorted(p for p in E.rglob('*') if p.is_file() or p.is_symlink())
for p in paths:
 if p==OUT:continue
 rel=str(p.relative_to(E));info=p.lstat()
 if stat.S_ISLNK(info.st_mode):exclusions.append({'path':rel,'reason':'SYMLINK_NEVER_FOLLOWED'});continue
 if sensitive_path(p):exclusions.append({'path':rel,'bytes':info.st_size,'reason':'PRIVATE_KEY_OR_REAL_CONTROL_NEVER_READ'});continue
 if '__pycache__' in p.parts or p.suffix.lower() in {'.exe','.zip','.pyc','.pdb','.dll','.rlib','.rmeta','.o','.obj'}:
  exclusions.append({'path':rel,'bytes':info.st_size,'reason':'BINARY_BUILD_CACHE_EXCLUDED_NO_CONTENT_READ'});continue
 if p.suffix.lower() not in allowed:
  exclusions.append({'path':rel,'bytes':info.st_size,'reason':'UNKNOWN_EXTENSION_REVIEW'});continue
 data=p.read_bytes()
 try:text=data.decode('utf-8-sig')
 except UnicodeDecodeError:
  finding('NON_UTF8_OR_BINARY_ORIGINAL_REVIEW',rel,sha(data));continue
 row={'path':rel,'bytes':len(data),'sha256':sha(data),'kind':('objdump-original' if '.objdump.' in rel else 'original-raw' if '-raw/' in rel else 'source-or-helper' if p.suffix in {'.py','.mjs','.ps1','.rs'} else 'report-or-log-model-freeze')}
 candidates.append(row);scan_text(text,rel)
 if p.suffix=='.json':
  try:obj=json.loads(text)
  except ValueError:finding('INVALID_JSON',rel,sha(data));continue
  jsons[rel]=obj;walk(obj,rel)
 elif p.suffix=='.jsonl':
  for i,line in enumerate(text.splitlines(),1):
   if not line.strip():continue
   try:walk(json.loads(line),rel+':'+str(i))
   except ValueError:finding('INVALID_JSONL',rel+':'+str(i),sha(line.encode()))
# Real .control.json itself is excluded; request-only stop journals are safe
# only when original owner physical cleanup and exact root deletion are proven.
stop_reports=[]
for row in candidates:
 name=row['path']
 if '.control.json.stop-' not in name:continue
 owner=name.split('.control.json.stop-',1)[0];value=jsons.get(owner,{})
 outcome=value.get('nativeOutcome',{});cleanup=value.get('cleanupDetails',{})
 expired=(value.get('cleanup')=='PASS' and all(outcome.get(k) is True for k in
          ['actualJobProcessesZero','originalPrimaryExited','originalHandlesClosed']) and cleanup.get('rootAbsent') is True)
 stop_reports.append({'path':name,'ownerReport':owner,'ownerStatus':value.get('status'),'ownerCleanup':value.get('cleanup'),'originalPhysicalCleanupAndRootAbsenceProven':expired,'operation':jsons[name].get('operation'),'candidate':expired})
 if not expired:finding('STOP_REPORT_EXPIRY_UNPROVEN',name,name)
# Exactly identify raw/report FAILs and terminal successors; never reinterpret
# a source-frozen/local model status as actual target evidence.
chain_names=['windows-integrated-first.json','windows-integrated-second.json','windows-integrated-third.json',
 'windows-gnu-runtime-counter-first-failure.json','windows-gnu-runtime-first.json','windows-gnu-runtime-third.json',
 'windows-header-deadline-first.json','windows-header-deadline-second.json','windows-header-deadline-third.json',
 'windows-default-browser-continuous-first.json','windows-default-browser-continuous-second.json','windows-default-browser-continuous-final.json',
 'editor-edge-atomic-stop-windows-first.json','editor-edge-atomic-stop-windows-second.json',
 'windows-public-native-final.json','windows-visual-native-final.json','windows-final-flow-series-first.json',
 'header-deadline-final-independent-first.json','default-browser-final-independent-first.json','public-visual-final-independent-first.json',
 'host-targeted-first.json','windows-readonly-final-first.json','windows-readonly-final-second.json',
 'main-final-integration-first.json']
chains=[]
for n in chain_names:
 obj=jsons.get(n);chains.append({'path':n,'present':obj is not None,'status':obj.get('status') if isinstance(obj,dict) else None,'cleanup':obj.get('cleanup') if isinstance(obj,dict) else None,'bytes':(E/n).stat().st_size if (E/n).exists() else None})
# External references are inventory only. Never read private/control data or
# build outputs; source helpers may need an immutable companion copy later.
hashes={x['sha256']:x['path'] for x in candidates};external=[]
source_ext={'.py','.mjs','.ps1','.rs','.toml','.lock','.yml','.yaml','.json','.jsonl','.md','.txt','.log','.diff','.patch'}
for value,origins in sorted(refs.items()):
 p=Path(value)
 if str(p).startswith(str(E)+'/') or p==E:continue
 row={'path':value,'referencedBy':sorted(origins)[:8],'referenceCount':len(origins)}
 if sensitive_path(p):row['classification']='SENSITIVE_ENV_OR_CONTROL_REFERENCE_CONTENT_NOT_READ'
 elif str(p).startswith('/tmp/fc-round41-build-target') or str(p).startswith(('/tmp/fc-round41-programs-second','/tmp/fc-round41-programs-third')):
  row['classification']='COMPILED_ARTIFACT_REFERENCE_NEVER_ARCHIVE_BINARY'
 elif p.suffix.lower() in {'.exe','.zip','.dll','.pdb','.rlib','.rmeta','.o','.obj'}:
  row['classification']='BINARY_OR_PACKAGE_REFERENCE_NOT_ARCHIVED'
 elif p.name=='AGENTS.md':row['classification']='ENV_INSTRUCTIONS_NOT_ARCHIVE_NO_CONTENT_READ'
 elif p.is_file() and not p.is_symlink() and p.suffix.lower() in source_ext and (str(p).startswith(('/tmp/fc-round41','/workspace/'))):
  raw=p.read_bytes();digest=sha(raw);row.update(bytes=len(raw),sha256=digest)
  if digest in hashes:row.update(classification='ALREADY_PRESENT_IDENTICAL_EVIDENCE',candidatePath=hashes[digest])
  elif p.suffix in {'.py','.mjs','.ps1','.jsonl'}:
   row['classification']='EXTERNAL_SOURCE_HELPER_OR_METADATA_NEEDS_EXPLICIT_COPY_DECISION'
  else:row['classification']='GIT_BOUND_SOURCE_OR_DOC_REFERENCE_NOT_AUTO_DUPLICATED'
 elif p.exists():row['classification']='EXISTING_ENV_DIRECTORY_OR_TOOLCHAIN_REFERENCE_NO_CONTENT_READ'
 else:row['classification']='MISSING_OR_EPHEMERAL_REFERENCE_REVIEW_SOURCE_BOUNDARY'
 external.append(row)
programs=[]
for root in ['/tmp/fc-round41-programs-second','/tmp/fc-round41-programs-third','/tmp/fc-round41-build-target']:
 p=Path(root);files=[q for q in p.rglob('*') if q.is_file()] if p.is_dir() else []
 programs.append({'root':root,'exists':p.exists(),'files':len(files),'bytes':sum(q.stat().st_size for q in files),'included':False,'contentRead':False,'reason':'Compiled executable/build cache excluded; retain metadata/SHA/objdump originals inside evidence instead'})
canonical=json.dumps(candidates,ensure_ascii=False,sort_keys=True,separators=(',',':')).encode()
large=[x for x in candidates if x['bytes']>64*1024*1024]
report={'schema':'fc.r41.archive-preparation.v1','status':'PREPARED_REVIEW_BLOCKERS' if blocked else 'PREPARED_NO_COPY_SECRET_PATTERNS_CLEAR',
 'generatedAtUtc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'root':str(E),'scope':'Read-only preparation; no archive copy/index/plan/source mutation, no deletes, Windows/Cargo/Git/remote. Manifest includes this helper; generated JSON itself is additional and excluded from recursive self-hash.',
 'counts':{'sourceFilesConsidered':len(paths),'eligibleOriginalFiles':len(candidates),'eligibleOriginalBytes':sum(x['bytes'] for x in candidates),'excludedFiles':len(exclusions),'secretOrFormatBlockers':len(blocked),'decodedUniquePayloads':len(decoded),'filesOver64MiB':len(large)},
 'candidateManifestSha256':sha(canonical),'candidateManifest':candidates,'excluded':exclusions,'over64MiB':large,'largestOriginals':sorted(candidates,key=lambda x:x['bytes'],reverse=True)[:10],
 'secretScan':{'knownPatternBlockers':blocked,'annotations':annotations,'decodedPayloads':decoded,'scope':'UTF8 text, recursive JSON/string base64, PowerShell FromBase64String/EncodedCommand payloads through bounded6depth, executable/ZIP inline magic, known credential/cookie/capability keys, private PEM and signed/credentialed URLs. No private/control content read. This is pattern-based review, not proof against unknown encodings. Public certificate PEM permitted; reference paths are not key contents.'},
 'retiredStopRequestReports':stop_reports,'failureSuccessChainChecklist':chains,'externalReferences':external,
 'externalHelpersOrMetadataNeedingDecision':[x for x in external if x['classification']=='EXTERNAL_SOURCE_HELPER_OR_METADATA_NEEDS_EXPLICIT_COPY_DECISION'],
 'outsideCompiledRootsExcluded':programs,'sameSourceBoundaries':['Keep first compile timeout/observer, parser/owner-sharing failures, first file-fixture hook failure and all final PASS originals unchanged.','Do not call the first default-browser failed run a normal full nativeDrain proof; later distinct full runs provide their own source/binary identities.','Header finite conservative pre-connect samples are not kernel ESTABLISHED identity, backlog certification or original strictP0 acceptance. Win7/Win10/Seewo/production listener gates remain separate.','Second/third frozen22EXEs and target are not archive inputs; metadata+objdump+actual runtime source/binary bindings preserve the evidence.'],
 'lateAdditions':'Two service final independent reports/helpers already included if present. Any file created after this snapshot must be explicit supplemental entry; this script does not watch/copy live outputs.'}
# Verify candidate original bytes remained stable during this preparation.
changed=[x['path'] for x in candidates if not (E/x['path']).is_file() or sha((E/x['path']).read_bytes())!=x['sha256']]
report['candidateBytesStableAfterScan']=not changed;report['changedDuringScan']=changed
newpaths=[str(p.relative_to(E)) for p in E.rglob('*') if p.is_file() and p not in paths and p!=OUT]
report['lateFilesNotInSnapshot']=sorted(newpaths)
raw=(json.dumps(report,ensure_ascii=False,indent=2)+'\n').encode();OUT.write_bytes(raw)
print(json.dumps({'report':str(OUT),'reportBytes':len(raw),'reportSha256':sha(raw),'counts':report['counts'],'manifestSha256':report['candidateManifestSha256'],'filesystemFilesIncludingGeneratedReport':len(paths)+1,'filesystemBytesIncludingGeneratedReport':sum(p.stat().st_size for p in paths if p.is_file())+len(raw),'blockingKinds':dict(collections.Counter(x['kind'] for x in blocked)),'externalHelperDecisions':len(report['externalHelpersOrMetadataNeedingDecision']),'lateFilesNotInSnapshot':newpaths},ensure_ascii=False))
