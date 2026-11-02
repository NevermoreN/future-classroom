#!/usr/bin/env python3
"""Prepare/execute compact R79 evidence preservation. Never delete or contact Windows.
The default performs no inventory. Root must stop all writers and supply exact
actual final reports before --execute. Any unrecognized file or credential fails.
"""
from pathlib import Path
import argparse,base64,datetime,gzip,hashlib,io,json,os,re,stat,struct,subprocess,tarfile
import xml.etree.ElementTree as ET
SOURCE=Path('/tmp/fc-round79-msi-maintenance')
MAIN=Path('/workspace/共享/Eliora/未来教室计划')
DEST=MAIN/'开发计划/验证记录/2026-11-02-round79-msi-maintenance'
CONTROLLER=SOURCE/'archive-preparation-second'
EXACT_SOURCE='c44906517fbc62233dca755d7225cea375264cbc'
TEXT_FORMS_SHA='fec8008e36bb896bf70cf62c033a240ef1d8828261c92bf01f44f934a65350ab'
TEXT_FORMS={}
TEXT_DERIVED_VIEWS={}
TEXT_FORMS_FILE=CONTROLLER/'text-forms-second.json'
MAX_FILES=20000;MAX_FILE_BYTES=128*1024*1024;MAX_TOTAL_BYTES=1024*1024*1024
MAX_ARCHIVE_PATHS=27;MAX_PROMOTED=24
PRUNE_RELATIVE={'package-preparation-first/tools/root', 'package-preparation-first/web-build-first/npm-cache', 'package-preparation-first/web-build-first/process-temp', 'package-preparation-first/web-build-first/source', 'repair-implementation-first/checkout-c449-first', 'repair-candidate-target-first', 'package-preparation-first/checkout-261c58a-first'}
PRUNE_NAMES={'node_modules','__pycache__','target','.cache','.npm-cache'}
PRIVATE_SUFFIXES={'.key','.pfx','.p12'};CERT_SUFFIXES={'.pem','.crt','.cer','.der'}
BINARY_SUFFIXES={'.exe','.zip','.pdb','.dll','.lib','.rlib','.rmeta','.pyc','.db','.sqlite','.sqlite3','.cache','.obj','.o','.a','.so','.dylib','.wasm','.7z','.tar','.msi','.msp','.msm','.deb','.cab','.gz','.xz','.bz2','.bin','.nupkg'}
PATTERNS={
 'private-key-pem':re.compile(r'-----BEGIN (?:OPENSSH |RSA |EC |DSA |ENCRYPTED )?PRIVATE KEY-----\s+[A-Za-z0-9+/=\s]{32,}-----END (?:OPENSSH |RSA |EC |DSA |ENCRYPTED )?PRIVATE KEY-----'),
 'github-token':re.compile(r'(?<![A-Za-z0-9])(?:gh[pousr]_[A-Za-z0-9]{30,}|github_pat_[A-Za-z0-9_]{30,})(?![A-Za-z0-9])'),
 'openai-secret':re.compile(r'(?<![A-Za-z0-9])sk-(?:proj-|svcacct-)?[A-Za-z0-9_-]{24,}'),
 'aws-access-key':re.compile(r'(?<![A-Z0-9])(?:AKIA|ASIA)[A-Z0-9]{16}(?![A-Z0-9])'),
 'bearer-secret':re.compile(r'(?i)authorization\s*[=:]\s*[\x22\x27]?Bearer\s+[A-Za-z0-9._~-]{24,}'),
 'basic-secret':re.compile(r'(?i)authorization\s*[=:]\s*[\x22\x27]?Basic\s+[A-Za-z0-9+/]{16,}={0,2}'),
 'signed-download-url':re.compile(r'(?i)https?://[^\s\x22\x27<>]{5,}[?&](?:sig|signature|x-amz-signature)=[A-Za-z0-9%_+/-]{20,}'),
 'session-cookie-bearer':re.compile(r'__Host-FutureClassroom=[0-9a-fA-F]{64}'),
 'json-issued-web-secret':re.compile(r'(?i)[\x22\x27](?:csrf|pollSecret|sessionCookie|accessToken|refreshToken)[\x22\x27]\s*:\s*[\x22\x27][A-Za-z0-9_./+=-]{32,}[\x22\x27]'),
 'json-password-value':re.compile(r'(?i)[\x22\x27]password[\x22\x27]\s*:\s*[\x22\x27][^\x22\x27\n]{12,128}[\x22\x27]'),
 'potential-pairing-short-code':re.compile(r'(?i)\\?[\x22\x27]pairingCode\\?[\x22\x27]\s*:\s*\\?[\x22\x27][0-9]{8}\\?[\x22\x27]'),
}

def need(v,m):
 if not v:raise AssertionError(m)

def sha(d):return hashlib.sha256(d).hexdigest()

def write_new(path,data):
 path.parent.mkdir(parents=True,exist_ok=True)
 with path.open('xb') as stream:stream.write(data)
 need(path.read_bytes()==data,'write readback mismatch '+str(path))

def as_json(data):
 return json.loads(data.decode('utf-8-sig'))

def valid_pe(data):
 if len(data)<64 or not data.startswith(b'MZ'):return False
 offset=int.from_bytes(data[60:64],'little')
 if offset<64 or offset>len(data)-24 or data[offset:offset+4]!=b'PE\0\0':return False
 machine,sections=struct.unpack_from('<HH',data,offset+4);optional=struct.unpack_from('<H',data,offset+20)[0];characteristics=struct.unpack_from('<H',data,offset+22)[0];start=offset+24
 if machine not in (0x14c,0x8664,0xaa64) or not 1<=sections<=96 or optional<96 or start+optional+40*sections>len(data) or not characteristics&2:return False
 return struct.unpack_from('<H',data,start)[0] in (0x10b,0x20b)

def binary_magic(data):
 return valid_pe(data) or data.startswith((bytes.fromhex('d0cf11e0a1b11ae1'),b'PK\x03\x04',b'PK\x05\x06',b'PK\x07\x08',b'\x7fELF',b'!<arch>\n',b'\x1f\x8b',b'\xfd7zXZ\x00',b'7z\xbc\xaf\x27\x1c'))

def canonical(relative):
 p=Path(relative);need(isinstance(relative,str) and not p.is_absolute() and p.as_posix()==relative and bool(p.parts) and all(v not in ('','.','..') for v in p.parts),'CANONICAL_RELATIVE_PATH');return p
def owned(path):
 p=Path(path);p=p if p.is_absolute() else SOURCE/canonical(str(p))
 need(p.resolve().is_relative_to(SOURCE) and not any(q.is_symlink() for q in (p,*p.parents) if q==SOURCE or SOURCE in q.parents),'OWNED_UNALIASED_PATH');return p
def original(path):
 path=owned(path);need(stat.S_ISREG(path.lstat().st_mode),'REGULAR_ORIGINAL')
 need(path.suffix.lower() not in PRIVATE_SUFFIXES,'PRIVATE_FILENAME_CONTENTS_NOT_READ')
 need(path.suffix.lower() not in CERT_SUFFIXES,'UNLISTED_CERTIFICATE_CONTENTS_NOT_READ')
 need(path.stat().st_size<=MAX_FILE_BYTES,'ORIGINAL_PER_FILE_BOUND');data=path.read_bytes()
 return data,dict(path=path.relative_to(SOURCE).as_posix(),bytes=len(data),sha256=sha(data))
def pinned(path,digest):
 need(re.fullmatch('[0-9a-f]{64}',digest or ''),'EXPLICIT_REPORT_SHA256');p=owned(path);b,f=original(p);need(f['sha256']==digest,'EXACT_REPORT_SHA256');return p,as_json(b),f
def terminal_processes():
 # Only /proc metadata is inspected. No process is signalled or waited on.
 excluded={os.getpid()};p=os.getpid()
 while p>1:
  try:line=Path('/proc',str(p),'stat').read_text();p=int(line.rsplit(')',1)[1].split()[1])
  except (OSError,ValueError,IndexError):break
  excluded.add(p)
 live=[]
 for proc in Path('/proc').iterdir():
  if not proc.name.isdigit() or int(proc.name) in excluded:continue
  try:args=(proc/'cmdline').read_bytes().split(b'\0')
  except (OSError,PermissionError):continue
  # Block any other live command referring to the owned root, even a reader;
  # root must serialize the finite archive step after all preparation writers.
  if any(str(SOURCE).encode() in value for value in args):live.append(int(proc.name))
 need(not live,'OTHER_OWNED_ROOT_CONTROLLERS_STILL_PRESENT_'+str(live));return dict(status='PASS_NO_OTHER_OWNED_ROOT_PROCESS_REFERENCE',scope='Current /proc cmdline metadata; root terminal confirmation also required.')
def source_members():
 files=[];pruned=[];count=0
 def walk(directory):
  nonlocal count
  for path in sorted(directory.iterdir()):
   count+=1;need(count<=MAX_FILES,'BOUNDED_SOURCE_ENTRY_COUNT')
   relative=path.relative_to(SOURCE).as_posix();canonical(relative)
   if relative in PRUNE_RELATIVE or path.name in PRUNE_NAMES:
    need(path.is_dir() and not path.is_symlink(),'PRUNED_REAL_DIRECTORY')
    pruned.append(dict(path=relative,scope='ENTIRE_SUBTREE_PRUNED_NOT_RECURSED_NOT_MEMBER_HASHED',reason='Rebuildable exact-source checkout, copied Web source/dist, dependencies, caches, process temp, or extracted tools. Original source/producer/provenance reports retained separately.'));continue
   need(not path.is_symlink(),'UNPRUNED_SYMLINK_FORBIDDEN')
   if path.is_dir():walk(path)
   else:need(stat.S_ISREG(path.lstat().st_mode),'NONREGULAR_ORIGINAL_FORBIDDEN');files.append(path)
 walk(SOURCE);return files,pruned

def binary_kind(data):
 if valid_pe(data):return 'PE_EXECUTABLE_OR_EMBEDDED_BINARY_LOG'
 signatures=[(bytes.fromhex('d0cf11e0a1b11ae1'),'OLE_COMPOUND_MSI_OR_BINARY_LOG'),(b'PK\x03\x04','ZIP'),(b'PK\x05\x06','EMPTY_ZIP'),(b'PK\x07\x08','ZIP_SPANNED'),(b'\x7fELF','ELF'),(b'!<arch>\n','AR_DEB_OR_STATIC_ARCHIVE'),(b'\x1f\x8b','GZIP'),(b'\xfd7zXZ\x00','XZ'),(b'7z\xbc\xaf\x27\x1c','SEVEN_ZIP'),(b'Microsoft C/C++ MSF 7.00','PDB'),(b'SQLite format 3\x00','SQLITE'),(b'MSCF','CAB'),(b'\x00asm','WASM')]
 for magic,label in signatures:
  if data.startswith(magic):return label
 return None

def reviewed_control_views(data,label):
 relative=None
 for prefix in ('source:','stored-tar-gzip-decoded:','promoted:'):
  if isinstance(label,str) and label.startswith(prefix):
   candidate=label[len(prefix):]
   relative=candidate
 row=TEXT_FORMS.get(relative) or TEXT_DERIVED_VIEWS.get(relative)
 if row is None:return None
 need(len(data)==row['bytes'] and sha(data)==row['sha256'],'EXACT_REVIEWED_TEXT_FORM_BYTES')
 kind=row['textForm']
 if kind=='EXACT_UTF16_MSI_DIGITAL_SIGNATURE_STREAM_NOTE':
  need(data.startswith(b'\xff\xfe'),'EXACT_MSI_UTF16_LE_BOM')
  text=data.decode('utf-16');need(text.encode('utf-16')==data,'EXACT_MSI_UTF16_ROUNDTRIP')
  control='\x05';need(text.count(control)==1,'EXACT_ONE_MSI_STREAM_CONTROL')
  line=text.split('\n')[row['controlLineNumber']-1];need(line==row['controlLineEscaped'].replace(r'\x05','\x05'),'EXACT_DIAGNOSED_MSI_CONTROL_LINE')
  need(re.fullmatch(r'MSI \(s\) \((?:[0-9A-F]{2}:[0-9A-F]{2})\) \[[0-9]{2}:[0-9]{2}:[0-9]{2}:[0-9]{3}\]: Note: 1: 2262 2: \x05DigitalSignature 3: -2147287038 \r',line) is not None,'EXACT_MSI_OLE_DIGITAL_SIGNATURE_STREAM_NOTE')
  encoding='utf16-reviewed-msi-stream-note'
 elif kind=='EXACT_UTF8_JSON_MSI_DIGITAL_SIGNATURE_STREAM_NOTE_EXCERPT':
  text=data.decode('utf-8');control='\x05';need(text==row['controlLineEscaped'].replace(r'\x05','\x05') and text.count(control)==1,'EXACT_FROZEN_JSON_MSI_NOTE_EXCERPT')
  need(re.fullmatch(r'MSI \(s\) \((?:[0-9A-F]{2}:[0-9A-F]{2})\) \[[0-9]{2}:[0-9]{2}:[0-9]{2}:[0-9]{3}\]: Note: 1: 2262 2: \x05DigitalSignature 3: -2147287038 ',text) is not None,'STRICT_JSON_MSI_NOTE_EXCERPT_STRUCTURE')
  encoding='utf8-reviewed-json-msi-note'
 else:
  text=data.decode('utf-8');need(text.encode('utf-8')==data,'EXACT_REVIEWED_UTF8_ROUNDTRIP');encoding='utf8-reviewed-structured-control'
  if kind=='EXACT_UTF8_LICENSE_WITH_18_FORM_FEEDS':
   control='\x0c';need(text.count(control)==18,'EXACT_LICENSE_FORM_FEED_COUNT')
  elif kind in ('EXACT_UTF8_GIT_LS_TREE_Z_19252_RECORDS','EXACT_UTF8_GIT_WEB_LS_FILES_Z_65_RECORDS'):
   control='\x00';records=text.split(control);need(records[-1]=='' and len(records)-1==(19252 if 'TREE' in kind else 65),'EXACT_GIT_Z_RECORD_COUNT')
   paths=[]
   for record in records[:-1]:
    if 'TREE' in kind:
     match=re.fullmatch(r'(100644|100755|120000|040000|160000) (blob|tree|commit) ([0-9a-f]{40}) +([0-9]+|-)\t([^\x00]+)',record)
     need(match is not None,'STRICT_GIT_LS_TREE_LONG_MODE_TYPE_OID_SIZE_TAB_PATH')
     need((match[1] in ('100644','100755','120000') and match[2]=='blob') or (match[1]=='040000' and match[2]=='tree') or (match[1]=='160000' and match[2]=='commit'),'CONSISTENT_GIT_TREE_MODE_TYPE')
     need((match[2]=='blob' and match[4].isdigit() and str(int(match[4]))==match[4] and int(match[4])<=MAX_FILE_BYTES) or (match[2] in ('tree','commit') and match[4]=='-'),'GIT_LONG_SIZE_FIELD_MATCHES_OBJECT_TYPE')
     path=match[5]
    else:path=record;need(path.startswith('web/'),'ONLY_TRACKED_WEB_SOURCE_PATHS')
    canonical(path);need(not any(ord(c)<32 for c in path),'NO_CONTROL_INSIDE_GIT_FILENAME');paths.append(path)
   need(len(set(paths))==len(paths),'UNIQUE_GIT_Z_FILENAME_DOMAIN')
  else:raise AssertionError('UNRECOGNIZED_REVIEWED_TEXT_FORM')
 need(not any(ord(c)<32 and c not in ('\r','\n','\t',control) for c in text),'NO_OTHER_CONTROL_IN_REVIEWED_TEXT')
 # All raw bytes remain untouched in tar. Scan the complete strict raw view,
 # an explicit visible boundary view, and a concatenated control-free view.
 # The latter prevents a token split by a permitted structural control from
 # evading the credential patterns; no other text or line is skipped.
 return [(encoding,text),(encoding+'-visible-boundary',text.replace(control,'[U+%04X]'%ord(control))),(encoding+'-control-concatenated',text.replace(control,''))]

def text_views(data,label=None):
 reviewed=reviewed_control_views(data,label)
 if reviewed is not None:return reviewed
 views=[]
 def add(name,text):
  need(not any(ord(c)<32 and c not in '\r\n\t\ufeff' for c in text),'NON_TEXT_CONTROL_BYTES')
  views.append((name,text.lstrip('\ufeff')))
 if data.startswith((b'\xff\xfe',b'\xfe\xff')):
  try:add('utf16',data.decode('utf-16'))
  except UnicodeError:raise AssertionError('INVALID_UTF16_TEXT') from None
 elif b'\0' in data:
  for enc in ('utf-16-le','utf-16-be'):
   try:add(enc,data.decode(enc))
   except (UnicodeError,AssertionError):pass
 else:
  try:add('utf8',data.decode('utf-8-sig'))
  except UnicodeError:
   try:text=data.decode('cp936');need(text.encode('cp936')==data,'CP936_EXACT_ROUNDTRIP');add('cp936',text)
   except UnicodeError:raise AssertionError('UNRECOGNIZED_TEXT_ENCODING') from None
 need(bool(views),'UNRECOGNIZED_OR_CONTROL_BINARY')
 for encoding,text in list(views):
  if text.startswith('#< CLIXML'):
   match=re.match(r'\A#< CLIXML\r?\n',text);need(match is not None,'CLIXML_HEADER')
   xml=text[match.end():];need('<!DOCTYPE' not in xml and '<!ENTITY' not in xml,'CLIXML_ENTITY_DTD_FORBIDDEN')
   try:doc=ET.fromstring(xml)
   except ET.ParseError:raise AssertionError('CLIXML_XML_PARSE') from None
   need(doc.tag=='{http://schemas.microsoft.com/powershell/2004/04}Objs','CLIXML_POWERSHELL_NAMESPACE')
   decoded='\n'.join(''.join(doc.itertext()).splitlines())
   decoded=re.sub(r'_x([0-9a-fA-F]{4})_',lambda m:chr(int(m.group(1),16)),decoded)
   need(not any(ord(c)<32 and c not in '\r\n\t' for c in decoded),'UNREVIEWED_CONTROL_IN_CLIXML_DECODED_VIEW')
   views.append((encoding+'-clixml-decoded',decoded))
 return views

def credential_scan(data,label,stats,findings,depth=0,decoded_budget=None):
 need(depth<=12,'CREDENTIAL_DECODER_DEPTH_BOUND')
 if decoded_budget is None:decoded_budget=[0]
 views=text_views(data,label)
 for encoding,text in views:
  stats['textViews']+=1
  if encoding.startswith('utf16') or encoding.startswith('utf-16'):stats['utf16Views']+=1
  if encoding.startswith('cp936'):stats['cp936Views']+=1
  if 'clixml' in encoding:stats['clixmlDecodedViews']+=1
  for name,pattern in PATTERNS.items():
   if pattern.search(text):findings.append(dict(path=label,kind=name,view=encoding))
  # Also decode long canonical base64 in PowerShell argv/log/source strings;
  # declared JSON base64 receives the stricter mandatory decoder below.
  attempted=0
  for token in re.findall(r'(?<![A-Za-z0-9+/])[A-Za-z0-9+/]{80,}={0,2}(?![A-Za-z0-9+/])',text):
   attempted+=1;need(attempted<=250000,'BASE64_TOKEN_COUNT_BOUND')
   need(len(token)<=44*1024*1024,'BASE64_TOKEN_BOUND')
   try:decoded=base64.b64decode(token,validate=True)
   except (ValueError,TypeError):continue
   if base64.b64encode(decoded).decode()!=token:continue
   need(not binary_kind(decoded),'ENCODED_BINARY_REQUIRES_EXPLICIT_IDENTITY_EXCLUSION')
   try:text_views(decoded)
   except (AssertionError,UnicodeError):continue
   need(len(decoded)<=32*1024*1024,'DECODED_CREDENTIAL_VIEW_BOUND');decoded_budget[0]+=len(decoded);need(decoded_budget[0]<=64*1024*1024,'DECODED_CREDENTIAL_TOTAL_BOUND')
   stats['decodedBase64Views']+=1;credential_scan(decoded,label+'::base64',stats,findings,depth+1,decoded_budget)
  try:value=json.loads(text)
  except (ValueError,RecursionError):continue
  stats['jsonContainers']+=1;budget=[0]
  def walk(item,trail):
   budget[0]+=1;need(budget[0]<=250000,'JSON_CREDENTIAL_TRAVERSAL_BOUND')
   if isinstance(item,dict):
    for key,value in item.items():
     if isinstance(value,str) and (key.lower() in ('base64','encodedcommand','stdoutbase64','stderrbase64') or key.lower().endswith('base64')):
      try:decoded=base64.b64decode(value,validate=True)
      except (ValueError,TypeError):continue
      need(len(decoded)<=32*1024*1024,'DECLARED_BASE64_BOUND');need(not binary_kind(decoded),'DECLARED_BASE64_BINARY_FORBIDDEN')
      decoded_budget[0]+=len(decoded);need(decoded_budget[0]<=64*1024*1024,'DECLARED_BASE64_TOTAL_BOUND')
      stats['decodedBase64Views']+=1;credential_scan(decoded,label+'::'+trail+'/'+key,stats,findings,depth+1,decoded_budget)
     elif isinstance(value,str):
      stats['decodedJsonStringViews']+=1;credential_scan(value.encode(),label+'::json'+trail+'/'+str(key),stats,findings,depth+1,decoded_budget)
     elif isinstance(value,(dict,list)):walk(value,trail+'/'+str(key))
   elif isinstance(item,list):
    for i,value in enumerate(item):walk(value,trail+'/'+str(i))
   elif isinstance(item,str):
    stats['decodedJsonStringViews']+=1;credential_scan(item.encode(),label+'::json'+trail,stats,findings,depth+1,decoded_budget)
  walk(value,'')

def gates(a):
 global TEXT_FORMS,TEXT_DERIVED_VIEWS
 form_bytes=TEXT_FORMS_FILE.read_bytes();need(sha(form_bytes)==TEXT_FORMS_SHA,'EXACT_REVIEWED_TEXT_FORMS_CONFIG');forms=json.loads(form_bytes)
 need(forms['status']=='EXACT_REVIEWED_R79_SECOND_STRUCTURED_TEXT_NOT_GENERAL_BINARY_EXCEPTION' and len(forms['files'])==15 and not forms['derivedJsonViews'],'EXACT_R79_FINITE_REVIEWED_FORMS')
 TEXT_FORMS={row['path']:row for row in forms['files']};TEXT_DERIVED_VIEWS={};need(len(TEXT_FORMS)==15,'UNIQUE_REVIEWED_TEXT_FORM_PATHS')
 diagnosis=SOURCE/forms['diagnosis']['path'];need(len(diagnosis.read_bytes())==forms['diagnosis']['bytes'] and sha(diagnosis.read_bytes())==forms['diagnosis']['sha256'],'EXACT_ORIGINAL_TEXT_DIAGNOSIS')
 need(a.root_controllers_terminal,'ROOT_ALL_WRITERS_TERMINAL_CONFIRMATION_REQUIRED')
 need((SOURCE/'.owner').read_bytes()==b'future-classroom-owned-r79-msi-maintenance/v1\n','EXACT_R79_OWNER')
 need((CONTROLLER/'.owner').read_bytes()==b'future-classroom-owned-r79-archive-preparation-second/v1\n' and Path(__file__).resolve().parent==CONTROLLER,'EXACT_ARCHIVER_OWNER')
 need(a.source_base_git==EXACT_SOURCE,'EXACT_R79_COMMITTED_SOURCE')
 need(subprocess.check_output(['git','-C',str(MAIN),'rev-parse','HEAD'],timeout=10).decode().strip()==EXACT_SOURCE,'CURRENT_SOURCE_HEAD_UNCHANGED')
 _,accept,accept_fact=pinned(a.acceptance_report,a.expected_acceptance_sha256)
 need(isinstance(a.expected_acceptance_status,str) and a.expected_acceptance_status.startswith('PASS_R79_') and not any(x in a.expected_acceptance_status for x in ('STATIC','PREPARED','NOT_RUN')),'EXPLICIT_FINITE_RUNTIME_ACCEPTANCE_SCOPE')
 need(accept['status']==a.expected_acceptance_status and accept['sourceSha']==EXACT_SOURCE,'ACTUAL_FINITE_R79_ACCEPTANCE')
 _,final,final_fact=pinned(a.readonly_audit,a.expected_readonly_sha256)
 need(final['status']=='PASS' and final.get('completedAtUtc'),'ACTUAL_TERMINAL_READONLY_AUDIT')
 return dict(acceptance=accept_fact,acceptanceStatus=accept['status'],readonlyAudit=final_fact,processAudit=terminal_processes(),scope='Evidence preservation only. Candidate repair and observational prototype remain separately scoped; does not establish production transaction authority or Win7/Win10 runtime.')

def main():
 p=argparse.ArgumentParser(description=__doc__)
 p.add_argument('--execute',action='store_true');p.add_argument('--root-controllers-terminal',action='store_true');p.add_argument('--source-base-git')
 for name in ('acceptance-report','expected-acceptance-sha256','expected-acceptance-status','readonly-audit','expected-readonly-sha256'):p.add_argument('--'+name)
 p.add_argument('--inventory-only',action='store_true');p.add_argument('--promote',action='append',default=[]);a=p.parse_args()
 if not a.execute:
  print(json.dumps(dict(status='PREPARED_ARCHIVE_ONLY_NOT_EXECUTED',sourceRoot=str(SOURCE),destination=str(DEST),sourcePayloadRead=False,inventory='NOT_RUN',sourceDeletion=False,windows='NOT_CONNECTED')));return 0
 evidence=gates(a);need(not DEST.exists(),'FRESH_DESTINATION_REQUIRED')
 paths,pruned=source_members();need(len(paths)<=MAX_FILES,'SOURCE_FILE_COUNT_BOUND');rows=[];omitted=[];total=0
 promoted=set(a.promote);need(len(promoted)==len(a.promote) and len(promoted)<=MAX_PROMOTED,'AT_MOST_24_UNIQUE_PROMOTED')
 for name in promoted:canonical(name);need(name not in ('archive-index-first.json','archive-verification-first.json','evidence-originals-first.tar.gz'),'RESERVED_PROMOTED_NAME')
 stats={k:0 for k in ('textViews','utf16Views','cp936Views','clixmlDecodedViews','decodedBase64Views','jsonContainers','decodedJsonStringViews','sourceFiles','storedFiles','promotedFiles')};findings=[]
 for path in paths:
  data,row=original(path);total+=len(data);need(total<=MAX_TOTAL_BYTES,'SOURCE_TOTAL_BYTES_BOUND');kind=binary_kind(data)
  if kind:row.update(reason='Recognized original compiled/archive/database binary: identity only, no bytes committed.',binaryKind=kind);omitted.append(row)
  else:
   need(path.suffix.lower() not in BINARY_SUFFIXES,'UNRECOGNIZED_BINARY_SUFFIX_REQUIRES_REVIEW_'+row['path'])
   credential_scan(data,'source:'+row['path'],stats,findings);stats['sourceFiles']+=1;rows.append(row)
 need(set(TEXT_FORMS)<={r['path'] for r in rows},'ALL_15_REVIEWED_STRUCTURED_TEXT_ORIGINALS_RETAINED')
 need(not findings,'CREDENTIAL_FINDINGS_PATH_KIND_ONLY_'+json.dumps(findings))
 need(promoted<={r['path'] for r in rows},'PROMOTE_ONLY_RETAINED_TEXT_ORIGINALS')
 if a.inventory_only:
  print(json.dumps(dict(status='PASS_READ_ONLY_FINITE_INVENTORY_AND_CREDENTIAL_SCAN_NOT_ARCHIVED',originals=rows,excludedBinaries=omitted,prunedGeneratedTrees=pruned,statistics=stats,sourceRemoved=False,windowsContacted=False)));return 0
 # Everything above is read-only. Only a complete accepted finite inventory
 # permits creation of the root's new compact archive destination.
 DEST.mkdir();container=DEST/'evidence-originals-first.tar.gz';promoted_rows=[]
 with container.open('xb') as outer:
  with gzip.GzipFile(filename='',mode='wb',fileobj=outer,compresslevel=9,mtime=0) as compressed:
   with tarfile.open(fileobj=compressed,mode='w',format=tarfile.PAX_FORMAT) as archive:
    for row in rows:
     data,current=original(SOURCE/row['path']);need(current==row,'SOURCE_CHANGED_BEFORE_ARCHIVE')
     info=tarfile.TarInfo(row['path']);info.size=len(data);info.mode=0o600;info.uid=info.gid=0;info.uname=info.gname='';info.mtime=0;archive.addfile(info,io.BytesIO(data))
     if row['path'] in promoted:write_new(DEST/row['path'],data);promoted_rows.append(row.copy())
 with tarfile.open(container,'r:gz') as archive:
  members=archive.getmembers();need(len(members)==len(rows) and len({m.name for m in members})==len(rows),'EXACT_UNIQUE_TAR_MEMBERS')
  for member,row in zip(members,rows):
   need(member.name==row['path'] and member.isfile() and member.uid==member.gid==member.mtime==0 and member.mode==0o600 and member.uname==member.gname=='','DETERMINISTIC_REGULAR_TAR_ORIGINALS')
   source,identity=original(SOURCE/row['path']);stored=archive.extractfile(member).read();need(identity==row and stored==source and sha(stored)==row['sha256'],'FULL_MEMBER_SOURCE_REPLAY')
   credential_scan(stored,'stored-tar-gzip-decoded:'+row['path'],stats,findings);stats['storedFiles']+=1
   if row['path'] in promoted:
    data=(DEST/row['path']).read_bytes();need(data==source,'PROMOTED_EXACT_BYTES');credential_scan(data,'promoted:'+row['path'],stats,findings);stats['promotedFiles']+=1
 for row in omitted:
  _,current=original(SOURCE/row['path']);need(current=={k:row[k] for k in ('path','bytes','sha256')},'EXCLUDED_BINARY_IDENTITY_CHANGED')
 newpaths,newpruned=source_members();need([p.relative_to(SOURCE).as_posix() for p in newpaths]==[p.relative_to(SOURCE).as_posix() for p in paths] and newpruned==pruned,'SOURCE_MEMBERSHIP_OR_PRUNES_CHANGED')
 need(not findings,'CREDENTIAL_FINDINGS_AFTER_REPLAY_'+json.dumps(findings));process_after=terminal_processes()
 cb=container.read_bytes();container_fact=dict(path=container.name,bytes=len(cb),sha256=sha(cb),format='deterministic gzip/PAX tar',memberCount=len(rows),gzipMtime=0)
 index=dict(reviewedTextForms=dict(derivedJsonViews=list(TEXT_DERIVED_VIEWS.values()),configPath=TEXT_FORMS_FILE.relative_to(SOURCE).as_posix(),configSha256=TEXT_FORMS_SHA,files=[dict(path=r['path'],bytes=r['bytes'],sha256=r['sha256'],textForm=r['textForm']) for r in TEXT_FORMS.values()]),schemaVersion=1,status='PASS_ARCHIVED_BYTES_NOT_NEW_RUNTIME',sourceRoot=str(SOURCE),sourceSha=EXACT_SOURCE,archiveContainer=container_fact,originals=rows,excludedBinaries=omitted,prunedGeneratedTrees=pruned,promoted=promoted_rows,originalCount=len(rows),originalBytes=sum(r['bytes'] for r in rows),evidence=evidence,sourceDeletion=False)
 ib=(json.dumps(index,ensure_ascii=False,indent=2)+'\n').encode();credential_scan(ib,'new-index',stats,findings);need(not findings,'CREDENTIAL_FINDING_IN_INDEX');write_new(DEST/'archive-index-first.json',ib)
 verification=dict(schemaVersion=1,status='PASS_EXACT_ORIGINAL_TAR_REPLAY_AND_DECODED_CREDENTIAL_SCAN',sourceSha=EXACT_SOURCE,members=len(rows),originalBytes=index['originalBytes'],archiveBytes=len(cb),archiveSha256=sha(cb),indexSha256=sha(ib),excludedBinaries=len(omitted),prunedSubtrees=pruned,credentialScan=dict(status='PASS_NO_MATCHES',statistics=stats,patterns=list(PATTERNS),findings=[],coverage='Retained originals, full decompressed tar replay, promoted copies and index; strict UTF8/UTF16/CP936 roundtrip and parsed CLIXML text, declared and generic canonical nested base64 and JSON strings; maximum depth12 and traversal250000. Excluded binaries have exact identities only.'),processAfter=process_after,sourceDeletion=False,windowsContacted=False,recordedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat())
 vb=(json.dumps(verification,ensure_ascii=False,indent=2)+'\n').encode();credential_scan(vb,'new-verification',stats,findings);need(not findings,'CREDENTIAL_FINDING_IN_VERIFICATION');write_new(DEST/'archive-verification-first.json',vb)
 expected={container.name,'archive-index-first.json','archive-verification-first.json'}|promoted;actual={p.relative_to(DEST).as_posix() for p in DEST.rglob('*') if p.is_file()};need(actual==expected and len(actual)<=MAX_ARCHIVE_PATHS,'COMPACT_ARCHIVE_EXACT_MEMBERSHIP_AND_27_PATH_BUDGET')
 tracked=subprocess.check_output(['git','-C',str(MAIN),'ls-files','-z'],timeout=30).split(b'\0');need(len([x for x in tracked if x])+len(actual)<=MAX_FILES,'PROJECT_TRACKED_PLUS_NEW_PATH_COUNT_BUDGET')
 need(sha(container.read_bytes())==container_fact['sha256'],'FINAL_ARCHIVE_UNCHANGED');need(source_members()==(newpaths,newpruned),'FINAL_SOURCE_MEMBERSHIP_UNCHANGED');print(json.dumps(dict(status='PASS',members=len(rows),binaryIdentities=len(omitted),archiveFiles=len(actual),archiveSha256=sha(cb),indexSha256=sha(ib),verificationSha256=sha(vb),sourceRemoved=False)));return 0
if __name__=='__main__':raise SystemExit(main())
