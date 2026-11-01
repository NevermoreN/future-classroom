#!/usr/bin/env python3
"""R68 exact evidence archive + independent source reread + decoded secret scan.
Only owned new archive files and this root's new verification receipt are written.
No removal, Git mutation, source change, Windows, SSH, Cargo or network operation.
"""
from pathlib import Path
import base64
import datetime
import gzip
import hashlib
import json
import re
import stat
import sys

SOURCE=Path('/tmp/fc-round68-production-https/evidence')
DEST=Path('/workspace/共享/Eliora/未来教室计划/开发计划/验证记录/2026-11-01-round68-production-https')
RECEIPT='archive-verification-first.json'
EXCLUDED_SUFFIXES={'.exe','.zip','.pdb','.dll','.lib','.rlib','.rmeta','.pyc','.db','.sqlite','.sqlite3','.cache'}
COMPRESS=re.compile(r'(?:modern-artifact-original|win7-artifact-original(?:-second)?)/pe-[0-9]+\.objdump\.stdout\.log')
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
}

def need(v,m):
 if not v:raise AssertionError(m)
def sha(d):return hashlib.sha256(d).hexdigest()
def original(path):
 need(stat.S_ISREG(path.lstat().st_mode),'nonregular original '+str(path))
 data=path.read_bytes()
 return data,{'path':path.relative_to(SOURCE).as_posix(),'bytes':len(data),'sha256':sha(data)}
def write_new(path,data):
 path.parent.mkdir(parents=True,exist_ok=True)
 with path.open('xb') as stream:stream.write(data)
 need(path.read_bytes()==data,'write readback mismatch '+str(path))
def as_json(data):
 return json.loads(data.decode('utf-8-sig'))
def excluded(path):
 return path.suffix.lower() in EXCLUDED_SUFFIXES or path.name.endswith(('.db-wal','.db-shm','.sqlite-wal','.sqlite-shm')) or any(part in ('__pycache__','node_modules','.npm-cache','target','.cache') for part in path.parts)

# These two reviewed public contexts are byte-bound to their retained originals.
# They do not relax any credential pattern or permit any other token/location.
STATIC_READY_PATH='independent-C-ui-contract-second-source/scripts/windows-tls-business-probe.py'
STATIC_READY_SHA='0cb59751f0bd1e79dd86de9eb26ac780e264a969d74cbdbe074be988210c7208'
SSH_REPORT_PATH='windows-header-deadline-first.json'
SSH_REPORT_SHA='ea452674ab64b84d4620342cb0fc63f8c1023f468f583fa05bfc129ab931d28c'
SSH_STDERR_SHA='f91c9a1e0bcf5a99849dc16b4962c86892feb74ecf1c4612edae57efcec73364'

def reviewed_static_ready(data,label,encoding,text,match):
 return (label in ('source:'+STATIC_READY_PATH,'stored:'+STATIC_READY_PATH)
  and encoding=='utf8' and sha(data)==STATIC_READY_SHA
  and text.count('\n',0,match.start())+1==2148)

def reviewed_public_algorithm(kind,data,label,encoding,text,match):
 if (kind!='openai-secret' or encoding!='utf8' or sha(data)!=SSH_STDERR_SHA
  or label not in ('source:'+SSH_REPORT_PATH+'::/sshTunnelRaw/stderr/base64',
                   'stored:'+SSH_REPORT_PATH+'::/sshTunnelRaw/stderr/base64')):return False
 # Split the public algorithm literal so this helper is itself scanned normally.
 algorithm='sk-'+'ecdsa-sha2-nistp256-cert-v01'
 if match.group(0)!=algorithm or text[match.end():match.end()+13]!='@openssh.com,':return False
 line=text.splitlines()[27] if len(text.splitlines())>=28 else ''
 return (text.count('\n',0,match.start())+1==28
  and line.startswith('debug2: host key algorithms: ')
  and algorithm+'@openssh.com' in line.split(': ',2)[-1].split(','))

def validate_reviewed_contexts():
 import ast
 data=(SOURCE/STATIC_READY_PATH).read_bytes()
 need(sha(data)==STATIC_READY_SHA,'reviewed static fixture original unchanged')
 tree=ast.parse(data.decode('utf-8'))
 functions=[n for n in tree.body if isinstance(n,ast.FunctionDef) and n.name=='self_test']
 need(len(functions)==1,'reviewed synthetic self_test function')
 assignments=[n for n in ast.walk(functions[0]) if isinstance(n,ast.Assign)
  and any(isinstance(t,ast.Name) and t.id=='ready' for t in n.targets) and n.lineno==2147]
 need(len(assignments)==1,'reviewed static READY assignment position')
 fixture=ast.literal_eval(assignments[0].value)
 need(fixture['marker']=='synthetic' and fixture['port']==12345 and fixture['generation']==1
  and fixture['origin']=='https://localhost:12345' and fixture['loginName']=='probe'
  and isinstance(fixture['pairingCode'],str) and re.fullmatch(r'[0-9]{8}',fixture['pairingCode']),
  'literal synthetic READY fixture context')
 need(any(isinstance(n,ast.Call) and isinstance(n.func,ast.Name) and n.func.id=='validate_ready'
  and n.lineno==2150 for n in ast.walk(functions[0])),'fixture consumed by pure validator')
 report=(SOURCE/SSH_REPORT_PATH).read_bytes()
 need(sha(report)==SSH_REPORT_SHA,'reviewed header report original unchanged')
 raw=base64.b64decode(as_json(report)['sshTunnelRaw']['stderr']['base64'],validate=True)
 need(sha(raw)==SSH_STDERR_SHA,'reviewed SSH diagnostic exact decoded stream')
 return {'staticSyntheticReady':{'path':STATIC_READY_PATH,'sha256':STATIC_READY_SHA,
   'function':'self_test','assignmentLine':2147,'fieldLine':2148,'validatorLine':2150,
   'scope':'Literal synthetic READY validation input, not a produced pairing credential; value omitted.'},
  'publicSshAlgorithm':{'report':SSH_REPORT_PATH,'reportSha256':SSH_REPORT_SHA,
   'decodedStderrSha256':SSH_STDERR_SHA,'decodedLine':28,
   'scope':'Exact public FIDO ECDSA certificate host-key algorithm in OpenSSH host key algorithms diagnostic; no token value printed.'}}

def main():
 need(SOURCE.is_dir() and not DEST.exists(),'fresh archive required')
 need(not (SOURCE/RECEIPT).exists(),'fresh receipt required')
 final=as_json((SOURCE/'windows-readonly-final-first.json').read_bytes())
 need(final['status']=='PASS' and final.get('completedAtUtc'),'terminal exact Windows final audit required')
 need(sha((SOURCE/'windows-readonly-final-first.json').read_bytes())=='75fdc192da4ad7026a8480118ab2cc305ccfe978a1cc8cb69cdc423defeb8659','root-confirmed final audit bytes')
 runtime=as_json((SOURCE/'independent-acceptance-first.json').read_bytes())
 need(runtime['status']=='PASS_R68_SOURCE7_FINITE_WIN11_REGRESSION_ONLY','terminal exact source-scoped R68 audit')
 need(sha((SOURCE/'independent-acceptance-first.json').read_bytes())=='7a83672d1b00ea5c518662e8b163bc140f78f2e3f41a1105122fb4fd7bd73e65','root-confirmed independent acceptance bytes')
 native_review=as_json((SOURCE/'independent-main-native-integration-review-first.json').read_bytes())
 need(native_review['status']=='PASS_FINITE_READ_ONLY_MAIN_SOURCE_AND_NATIVE_EVIDENCE_REVIEW','native main integration review terminal')
 reviewed_contexts=validate_reviewed_contexts()
 owner=SOURCE.parent/'.owner';need(owner.is_file() and not owner.is_symlink(),'exact R68 owner marker regular')
 owner_data=owner.read_bytes();need(owner_data==b'future-classroom-owned-r68-production-https/v1\n','exact R68 owner marker')
 # Only these two physically closed, removed synthetic deployments may retain
 # their old one-use pairing short code in original legacy diagnostic bytes.
 # No values or standalone hashes of codes enter this receipt or scanner output.
 expired_scopes=[];allowed_codes={}
 for name in ['windows-business-tls-first.json','windows-header-deadline-first.json']:
  value=as_json((SOURCE/name).read_bytes());cleanup=value['cleanupDetails'];native=value['native']
  need(value['status']==value['cleanup']=='PASS' and cleanup['rootAbsent'] is True and cleanup['cngKeyExists'] is False
   and cleanup['myDigestRestored'] is True and cleanup['ownedProcessesRemaining']==0 and native['physicalNativeDrain'] is True
   and native['workerJoin'] is True and value['nativeProcessExit']=={'code':0},'expired synthetic pairing scope physically ended')
  raw=Path(value['rawPreservation']['directory']);need(raw.is_relative_to(SOURCE),'owned raw scope')
  # The public report intentionally scrubs pairingCode from ready metadata.
  # Inspect only the already retained exact native original into memory.
  native_stdout=(raw/'tls.stdout').read_bytes()
  raw_meta=value['rawPreservation']['files']['tls.stdout']['remote']
  need(len(native_stdout)==raw_meta['bytes'] and sha(native_stdout)==raw_meta['sha256'],'exact legacy native stdout original')
  decoded=native_stdout.decode('utf-16') if native_stdout.startswith((b'\xff\xfe',b'\xfe\xff')) else native_stdout.decode('utf-8-sig')
  native_ready=[json.loads(line.split(' ',1)[1]) for line in decoded.splitlines() if line.startswith('TLS_BUSINESS_READY ')]
  need(len(native_ready)==1 and native_ready[0]['marker']==value['marker'],'exact original closed fixture READY')
  code=native_ready[0]['pairingCode'];need(isinstance(code,str) and re.fullmatch(r'[0-9]{8}',code),'synthetic legacy short-code shape')
  allowed_codes[name]={code};allowed_codes[raw.relative_to(SOURCE).as_posix()+'/']={code}
  expired_scopes.append({'report':name,'reportSha256':sha((SOURCE/name).read_bytes()),'rawDirectory':raw.relative_to(SOURCE).as_posix(),
   'reason':'Synthetic one-use pairing window belongs to physically ended native domain; exact deployment root, certificate/CNG key and processes removed, old session namespace cannot be used. Original bytes retained; short-code value omitted from this receipt.'})
 paths=sorted(p for p in SOURCE.rglob('*') if not p.is_dir())
 rows=[];omitted=[]
 DEST.mkdir(parents=True)
 for path in paths:
  data,row=original(path)
  if excluded(path):
   row['reason']='Compiled/database payload, downloadable original ZIP, or build/interpreter cache; original exact identity preserved, bytes not committed.'
   omitted.append(row);continue
  rel=row['path']
  if COMPRESS.fullmatch(rel):
   packed=gzip.compress(data,compresslevel=9,mtime=0)
   need(gzip.decompress(packed)==data,'gzip encoder roundtrip')
   row.update(storage='gzip_original_bytes',archivePath='compressed-derived-pe/'+rel+'.gz',storedBytes=len(packed),storedSha256=sha(packed))
   output=packed
  else:output=data
  write_new(DEST/row.get('archivePath',rel),output)
  rows.append(row)
 # Independent second pass: reopen each exact source and each stored file;
 # decompression and equality, rather than only trusting the first pass hashes.
 verified=[]
 for row in rows:
  data,identity=original(SOURCE/row['path'])
  need(identity=={k:row[k] for k in ('path','bytes','sha256')},'source changed during archive '+row['path'])
  stored=(DEST/row.get('archivePath',row['path'])).read_bytes()
  if row.get('storage')=='gzip_original_bytes':
   need(len(stored)==row['storedBytes'] and sha(stored)==row['storedSha256'],'stored gzip digest '+row['path'])
   unpacked=gzip.decompress(stored)
  else:unpacked=stored
  need(unpacked==data and len(unpacked)==row['bytes'] and sha(unpacked)==row['sha256'],'independent exact archived/source bytes '+row['path'])
  verified.append({'path':row['path'],'bytes':len(data),'sha256':sha(data),'archivePath':row.get('archivePath',row['path']),
   'storedBytes':len(stored),'storedSha256':sha(stored),'verification':'REOPENED_SOURCE_AND_STORED_BYTES_EQUAL_AFTER_OPTIONAL_GZIP_DECOMPRESSION'})
 for row in omitted:
  _,identity=original(SOURCE/row['path'])
  need(identity=={k:row[k] for k in ('path','bytes','sha256')},'excluded payload identity changed '+row['path'])
 need({p.relative_to(SOURCE).as_posix() for p in SOURCE.rglob('*') if not p.is_dir()}=={r['path'] for r in rows+omitted},'source membership changed during copy')
 # Credential scan checks UTF8 plus actual UTF16 views, parsed nested JSON raw
 # encodings and gzip decompressed originals. It reports names/kinds only, never
 # matched credential contents. Excluded executable/ZIP payloads are not scanned.
 findings=[];expired_annotations=[];public_annotations=[];stats={'sourceFiles':0,'storedFiles':0,'gzipFiles':0,'utf16Views':0,'decodedRawPayloads':0,'jsonContainers':0,'textViews':0}
 def scan(data,label,depth=0):
  need(depth<=12,'credential decoder depth bound '+label)
  views=[]
  try:views.append(('utf8',data.decode('utf-8-sig')))
  except UnicodeError:pass
  if data.startswith((b'\xff\xfe',b'\xfe\xff')) or (len(data)>4 and data.count(b'\0')>len(data)//8):
   for encoding in ('utf-16-le','utf-16-be'):
    try:
     views.append((encoding,data.decode(encoding).lstrip('\ufeff')));stats['utf16Views']+=1
    except UnicodeError:pass
  for encoding,text in views:
   stats['textViews']+=1
   for kind,pattern in PATTERNS.items():
    for match in pattern.finditer(text):
     if reviewed_public_algorithm(kind,data,label,encoding,text,match):
      public_annotations.append({'path':label,'kind':'EXACT_PUBLIC_OPENSSH_HOST_KEY_ALGORITHM','view':encoding,'matchedValueOmitted':True})
     else:findings.append({'path':label,'kind':kind,'view':encoding})
   short_codes=list(re.finditer(r'(?i)\\?[\x22\x27]pairingCode\\?[\x22\x27]\s*:\s*\\?[\x22\x27]([0-9]{8})\\?[\x22\x27]',text))
   if short_codes:
    rel=label.split(':',1)[1].split('::',1)[0] if ':' in label else label
    permitted=set()
    for owned,codes in allowed_codes.items():
     if rel==owned or owned.endswith('/') and rel.startswith(owned):permitted.update(codes)
    for match in short_codes:
     value=match.group(1)
     if reviewed_static_ready(data,label,encoding,text,match):public_annotations.append({'path':label,'kind':'EXACT_STATIC_SYNTHETIC_READY_PURE_TEST_VECTOR','view':encoding,'valueOmitted':True})
     elif value in permitted:expired_annotations.append({'path':label,'kind':'EXPIRED_SYNTHETIC_ONE_USE_PAIRING_SHORT_CODE','view':encoding,'valueOmitted':True})
     elif value=='00000000' and rel.endswith(('.py','.rs','.patch')):expired_annotations.append({'path':label,'kind':'LITERAL_SYNTHETIC_ZERO_TEST_VECTOR','view':encoding,'valueOmitted':True})
     else:findings.append({'path':label,'kind':'UNSCOPED_POTENTIAL_PAIRING_SHORT_CODE','view':encoding})
   try:value=json.loads(text)
   except (ValueError,RecursionError):continue
   stats['jsonContainers']+=1
   budget=[0]
   def walk(item,trail):
    budget[0]+=1;need(budget[0]<=250000,'JSON credential traversal bound '+label)
    if isinstance(item,dict):
     for key,value in item.items():
      if isinstance(value,str) and (key.lower() in ('base64','encodedcommand','stdoutbase64','stderrbase64') or key.lower().endswith('base64')):
       try:decoded=base64.b64decode(value,validate=True)
       except (ValueError,TypeError):pass
       else:
        need(len(decoded)<=32*1024*1024,'decoded credential view bound '+label)
        stats['decodedRawPayloads']+=1;scan(decoded,label+'::'+trail+'/'+key,depth+1)
      elif isinstance(value,str) and value[:1] in ('{','['):
       try:nested=json.loads(value)
       except (ValueError,RecursionError):pass
       else:walk(nested,trail+'/'+str(key))
      elif isinstance(value,(dict,list)):walk(value,trail+'/'+str(key))
    elif isinstance(item,list):
     for index,value in enumerate(item):walk(value,trail+'/'+str(index))
   walk(value,'')
 for row in rows:
  source_data=(SOURCE/row['path']).read_bytes();scan(source_data,'source:'+row['path']);stats['sourceFiles']+=1
  stored_data=(DEST/row.get('archivePath',row['path'])).read_bytes()
  if row.get('storage')=='gzip_original_bytes':
   stats['gzipFiles']+=1;scan(gzip.decompress(stored_data),'stored-gzip-decoded:'+row['path'])
  else:scan(stored_data,'stored:'+row['path'])
  stats['storedFiles']+=1
 need(not findings,'credential findings by path/kind only: '+json.dumps(findings))
 original_count=len(rows);original_bytes=sum(r['bytes'] for r in rows);stored_bytes=sum(r.get('storedBytes',r['bytes']) for r in rows)
 receipt={'schemaVersion':1,'status':'PASS_EXACT_ORIGINAL_AND_STORED_BYTES_AND_DECODED_CREDENTIAL_SCAN',
  'recordedUtc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'sourceRoot':str(SOURCE),'archiveRoot':str(DEST),
  'sourceFreezeSha256':'89148f095235a850b0fc408aa24b79f6701d790791ccba6e1535a87cfc770137','scope':'R68 source7 implementation and finite actual Win11 development evidence; no Win7/10 OS, classroom campus or final product certification.',
  'sourceFreeze':'Root confirmed all original controllers terminal and no further R68 evidence writes before archive; helper independently reread full source membership and original bytes.',
  'verifiedOriginalFilesBeforeThisReceipt':original_count,'verifiedOriginalBytesBeforeThisReceipt':original_bytes,'storedBytesBeforeThisReceipt':stored_bytes,
  'verified':verified,'excludedCompiledPayloads':omitted,
  'credentialScan':{'status':'PASS_NO_BEARER_PASSWORD_PRIVATEKEY_MATCHES_WITH_EXPIRED_FIXTURE_SHORT_CODES_TRANSPARENTLY_RECORDED','statistics':stats,'patterns':list(PATTERNS),'findings':[],'expiredSyntheticShortCodeScopes':expired_scopes,'expiredSyntheticShortCodeOccurrences':expired_annotations,'reviewedPublicContexts':reviewed_contexts,'reviewedPublicContextOccurrences':public_annotations,
   'coverage':'Every retained original and stored counterpart; actual UTF16 views, nested JSON base64/encodedCommand raw streams, and decompressed gzip. Excluded binary/ZIP bytes have exact identity only, are not committed and are not text-scanned.'},
  'finalReceiptBinding':'This receipt itself is copied byte-for-byte after its independent checks and added to the final archive index; its own identity and the final index identity are printed by the helper after final checks to avoid a circular self-hash.',
  'runtimeEvidence':{'path':'independent-acceptance-first.json','sha256':sha((SOURCE/'independent-acceptance-first.json').read_bytes()),'status':runtime['status']},
  'windowsFinalAudit':{'path':'windows-readonly-final-first.json','sha256':sha((SOURCE/'windows-readonly-final-first.json').read_bytes()),'status':final['status']},
  'historicalScopes':'All early compile/IPv6/ordinary and suite FAILs and their old scope statements retained as original bytes. Later changed implementation, user-reported Clash closure association, runner count qualification and actual1542/ordinary/browsing passes are separate evidence; no sole causal attribution or old FAIL promotion.','ownedRootMarker':{'path':str(owner),'bytes':len(owner_data),'sha256':sha(owner_data),'sourceScope':'Only evidence directory archived, not worktree/target/web-build/cache.'},'nativeIntegrationReview':{'path':'independent-main-native-integration-review-first.json','sha256':sha((SOURCE/'independent-main-native-integration-review-first.json').read_bytes()),'status':native_review['status']}}
 receipt_data=(json.dumps(receipt,ensure_ascii=False,indent=2)+'\n').encode('utf-8')
 # Receipt contains no original result bodies/secrets, only reviewed identities.
 # Scan it too before creating either immutable copy.
 scan(receipt_data,'new-verification-receipt')
 need(not findings,'credential finding in verification receipt')
 write_new(SOURCE/RECEIPT,receipt_data);write_new(DEST/RECEIPT,receipt_data)
 receipt_row={'path':RECEIPT,'bytes':len(receipt_data),'sha256':sha(receipt_data)}
 rows.append(receipt_row)
 index={'schemaVersion':2,'status':'ORIGINAL_BYTES_ARCHIVED','sourceRoot':str(SOURCE),'files':rows,
  'originalFiles':len(rows),'originalBytes':sum(r['bytes'] for r in rows),'storedBytes':sum(r.get('storedBytes',r['bytes']) for r in rows),
  'excludedCompiledPayloads':omitted,'verificationReceipt':receipt_row,
  'scope':'Exact original R68 source/commands/runtime/integration/final-audit results retained; derived objdump is losslessly gzip-compressed with original and stored bytes/SHA. Compiled EXE/ZIP and caches excluded by exact identity. Static and actual OS runtime claims remain separate.'}
 index_data=(json.dumps(index,ensure_ascii=False,indent=2)+'\n').encode('utf-8')
 write_new(DEST/'archive-index-first.json',index_data)
 for row in rows:
  data=(SOURCE/row['path']).read_bytes();stored=(DEST/row.get('archivePath',row['path'])).read_bytes()
  need(len(data)==row['bytes'] and sha(data)==row['sha256'],'final original bytes '+row['path'])
  if row.get('storage')=='gzip_original_bytes':
   need(len(stored)==row['storedBytes'] and sha(stored)==row['storedSha256'],'final stored gzip')
   stored=gzip.decompress(stored)
  need(stored==data,'final original versus archive '+row['path'])
 need({p.relative_to(SOURCE).as_posix() for p in SOURCE.rglob('*') if not p.is_dir()}=={r['path'] for r in rows+omitted},'final complete source membership')
 need({p.relative_to(DEST).as_posix() for p in DEST.rglob('*') if not p.is_dir()}=={r.get('archivePath',r['path']) for r in rows}|{'archive-index-first.json'},'final exact archive membership')
 print(json.dumps({'status':'PASS','originalFiles':index['originalFiles'],'originalBytes':index['originalBytes'],'storedBytes':index['storedBytes'],
  'excludedCompiledPayloads':len(omitted),'compressedObjdumpFiles':stats['gzipFiles'],
  'indexSha256':sha(index_data),'verificationReceiptSha256':sha(receipt_data),'credentialScan':'PASS_NO_LIVE_SECRETS_EXPIRED_SYNTHETIC_SHORT_CODES_RECORDED',
  'decodedRawPayloads':stats['decodedRawPayloads'],'utf16Views':stats['utf16Views'],'sourceRemoved':False}))

if __name__=='__main__':main()
