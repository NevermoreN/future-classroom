#!/usr/bin/env python3
"""R72 exact evidence archive + independent source reread + decoded secret scan.
Only owned new archive files and this root's new verification receipt are written.
No removal, Git mutation, source change, Windows, SSH, Cargo or network operation.
"""
from pathlib import Path
import argparse
import base64
import datetime
import gzip
import hashlib
import json
import re
import stat
import sys
import ssl

SOURCE=Path('/tmp/fc-round72-ci-evidence')
DEST=Path('/workspace/共享/Eliora/未来教室计划/开发计划/验证记录/2026-11-01-round72-ci')
RECEIPT='archive-verification-first.json'
EXCLUDED_SUFFIXES={'.exe','.zip','.pdb','.dll','.lib','.rlib','.rmeta','.pyc','.db','.sqlite','.sqlite3','.cache','.obj','.o','.a','.so','.dylib','.wasm','.7z','.tar'}
CERTIFICATES={}
CERT_SUFFIXES={'.pem','.crt','.cer','.der'}
PRIVATE_SUFFIXES={'.key','.pfx','.p12'}
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
 'potential-pairing-short-code':re.compile(r'(?i)\\?[\x22\x27]pairingCode\\?[\x22\x27]\s*:\s*\\?[\x22\x27][0-9]{8}\\?[\x22\x27]'),
}

def need(v,m):
 if not v:raise AssertionError(m)
def sha(d):return hashlib.sha256(d).hexdigest()
def original(path):
 need(stat.S_ISREG(path.lstat().st_mode),'nonregular original '+str(path))
 need(path.suffix.lower() not in PRIVATE_SUFFIXES,'private material forbidden by filename; contents not read '+str(path))
 if path.suffix.lower() in CERT_SUFFIXES:need(path.relative_to(SOURCE).as_posix() in CERTIFICATES,'unlisted certificate forbidden '+str(path))
 data=path.read_bytes()
 if path.suffix.lower() in CERT_SUFFIXES:public_certificate(path,data)
 return data,{'path':path.relative_to(SOURCE).as_posix(),'bytes':len(data),'sha256':sha(data)}
def write_new(path,data):
 path.parent.mkdir(parents=True,exist_ok=True)
 with path.open('xb') as stream:stream.write(data)
 need(path.read_bytes()==data,'write readback mismatch '+str(path))
def as_json(data):
 return json.loads(data.decode('utf-8-sig'))
def excluded(path):
 return path.suffix.lower() in EXCLUDED_SUFFIXES or path.name.endswith(('.db-wal','.db-shm','.sqlite-wal','.sqlite-shm')) or any(part in ('__pycache__','node_modules','.npm-cache','target','.cache') for part in path.parts)

def public_certificate_der(data):
    # Exactly one public certificate block, never a bundle, key or surrounding
    # text. Decode strictly, reject trailing/noncanonical DER, then parse X.509.
    match = re.fullmatch(br'-----BEGIN CERTIFICATE-----\r?\n([A-Za-z0-9+/=\r\n]+)-----END CERTIFICATE-----\r?\n?', data)
    need(match is not None, 'public certificate PEM envelope rejected')
    encoded = match.group(1).replace(b'\r', b'').replace(b'\n', b'')
    try:
        der = base64.b64decode(encoded, validate=True)
    except (ValueError, TypeError):
        raise AssertionError('public certificate base64 rejected') from None
    need(base64.b64encode(der) == encoded and len(der) >= 4 and der[0] == 0x30,
         'public certificate DER sequence rejected')
    first = der[1]
    if first < 0x80:
        header, length = 2, first
    else:
        count = first & 0x7f
        need(1 <= count <= 4 and len(der) >= 2 + count and der[2] != 0,
             'public certificate DER length rejected')
        header, length = 2 + count, int.from_bytes(der[2:2 + count], 'big')
        need(length >= 0x80 and (length.bit_length() + 7) // 8 == count,
             'public certificate DER nonminimal length rejected')
    need(header + length == len(der), 'public certificate DER trailing bytes rejected')
    try:
        ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT).load_verify_locations(cadata=der)
    except (ssl.SSLError, ValueError):
        raise AssertionError('public certificate X509 parse rejected') from None
    return der


def public_certificate(path,data):
 row=CERTIFICATES[path.relative_to(SOURCE).as_posix()]
 need(len(data)==row['bytes'] and sha(data)==row['sha256'],'exact listed public certificate')
 report=SOURCE/row['report'];need(report.resolve().is_relative_to(SOURCE.resolve()) and report.is_file() and not report.is_symlink(),'certificate original report scope')
 need(sha(report.read_bytes())==row['reportSha256'],'certificate original report identity')
 need(path.name=='tls-public.pem','only exact controlled tls-public.pem certificates are admissible')
 original=as_json(report.read_bytes());preserved=original['rawPreservation'];record=preserved['files']['tls-public.pem']
 need(preserved['status']==record['status']=='PASS' and Path(preserved['directory'])==path.parent and record['artifact']==str(path)
  and record['remote']==record['downloaded']=={'bytes':len(data),'sha256':sha(data)}
  and base64.b64decode(record['base64'],validate=True)==data,'controlled original public certificate provenance')
 der=public_certificate_der(data)
 return dict(path=path.relative_to(SOURCE).as_posix(),bytes=len(data),sha256=sha(data),derSha256=sha(der),x509ParseExitCode=0,report=row['report'],reportSha256=row['reportSha256'])

def main():
 global CERTIFICATES
 parser=argparse.ArgumentParser(description=__doc__)
 parser.add_argument('--source-sha',required=True)
 parser.add_argument('--acceptance-report',required=True)
 parser.add_argument('--expected-acceptance-sha256',required=True)
 parser.add_argument('--expected-acceptance-status',required=True)
 parser.add_argument('--readonly-audit',default='windows-readonly-final-first.json')
 parser.add_argument('--expected-readonly-sha256',required=True)
 parser.add_argument('--public-certificate-allowlist',type=Path)
 parser.add_argument('--root-controllers-terminal',action='store_true')
 parser.add_argument('--execute',action='store_true')
 args=parser.parse_args()
 need(re.fullmatch(r'[0-9a-f]{40}',args.source_sha),'explicit source SHA')
 for value in (args.expected_acceptance_sha256,args.expected_readonly_sha256):need(re.fullmatch(r'[0-9a-f]{64}',value),'explicit terminal report SHA')
 need(args.expected_acceptance_status.startswith('PASS_R72_') and not any(x in args.expected_acceptance_status for x in ('STATIC','PREPARED','NOT_RUN')),'actual runtime acceptance status required')
 if not args.execute:
  print(json.dumps({'status':'PREPARED_ARCHIVE_ONLY_NOT_EXECUTED','sourceSha':args.source_sha,'sourceRoot':str(SOURCE),'destination':str(DEST),'sourcePayloadRead':False,'scope':'Await final independent finite runtime audit and readonly terminal hashes; no archive or deletion.'}));return
 need(args.root_controllers_terminal,'root must confirm every original controller terminal and source membership frozen')
 owner=SOURCE/'.owner';owner_bytes=owner.read_bytes();need(owner_bytes==b'future-classroom-owned-r72-ci-evidence/v1\n','exact R72 owner marker')
 need(SOURCE.is_dir() and not SOURCE.is_symlink() and not DEST.exists(),'fresh archive required')
 need(not (SOURCE/RECEIPT).exists(),'fresh receipt required')
 acceptance_path=SOURCE/args.acceptance_report;final_path=SOURCE/args.readonly_audit
 for path in (acceptance_path,final_path):
  need(path.resolve().is_relative_to(SOURCE.resolve()),'terminal report scope')
  if not path.exists():
   print(json.dumps({'status':'WAIT','reasonCode':'MISSING_FINAL_'+path.name,'reportCreated':False}));return
  need(path.is_file() and not path.is_symlink(),'terminal report regular file')
  try:pending=as_json(path.read_bytes())
  except (ValueError,UnicodeError):
   print(json.dumps({'status':'WAIT','reasonCode':'INCOMPLETE_FINAL_'+path.name,'reportCreated':False}));return
  if pending.get('status') in ('RUNNING','STARTED','OBSERVATION_INTERRUPTED_NOT_COMPLETION'):
   print(json.dumps({'status':'WAIT','reasonCode':'NONTERMINAL_FINAL_'+path.name,'reportCreated':False}));return
 final=as_json(final_path.read_bytes())
 need(final['status']=='PASS' and final.get('completedAtUtc') and sha(final_path.read_bytes())==args.expected_readonly_sha256,'exact terminal readonly audit')
 runtime=as_json(acceptance_path.read_bytes())
 need(runtime['status']==args.expected_acceptance_status and runtime.get('sourceSha')==args.source_sha and sha(acceptance_path.read_bytes())==args.expected_acceptance_sha256,'exact actual source-scoped independent acceptance')
 if args.public_certificate_allowlist:
  p=args.public_certificate_allowlist;need(p.resolve().is_relative_to(SOURCE.resolve()) and not p.is_symlink(),'certificate allowlist scope')
  listing=as_json(p.read_bytes());need(listing['sourceSha']==args.source_sha and listing['status']=='ROOT_REVIEWED_PUBLIC_CERTIFICATES_ONLY','exact reviewed public certificate list')
  CERTIFICATES=listing['certificates'];need(isinstance(CERTIFICATES,dict),'certificate list object')
 members=list(SOURCE.rglob('*'))
 need(all(not p.is_symlink() and (p.is_dir() or stat.S_ISREG(p.lstat().st_mode)) for p in members),'nonregular/symlink source membership forbidden')
 paths=sorted(p for p in members if p.is_file())
 rows=[];omitted=[]
 for path in paths:
  need(path.suffix.lower() not in PRIVATE_SUFFIXES,'private material forbidden; contents not read '+str(path))
  if path.suffix.lower() in CERT_SUFFIXES:need(path.relative_to(SOURCE).as_posix() in CERTIFICATES,'unlisted certificate forbidden '+str(path))
 DEST.mkdir(parents=True)
 for path in paths:
  data,row=original(path)
  if excluded(path):
   row['reason']='Compiled/database payload, downloadable original CI ZIP, or build/interpreter cache; original exact identity preserved, bytes not committed.'
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
 findings=[];stats={'sourceFiles':0,'storedFiles':0,'gzipFiles':0,'utf16Views':0,'decodedRawPayloads':0,'jsonContainers':0,'textViews':0,'decodedJsonStringViews':0}
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
  need(bool(views),'unrecognized retained evidence encoding '+label)
  for encoding,text in views:
   stats['textViews']+=1
   for kind,pattern in PATTERNS.items():
    if pattern.search(text):findings.append({'path':label,'kind':kind,'view':encoding})
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
      elif isinstance(value,str):
       stats['decodedJsonStringViews']+=1;scan(value.encode('utf-8'),label+'::json-string'+trail+'/'+str(key),depth+1)
      elif isinstance(value,(dict,list)):walk(value,trail+'/'+str(key))
    elif isinstance(item,list):
     for index,value in enumerate(item):
      if isinstance(value,str):
       stats['decodedJsonStringViews']+=1;scan(value.encode('utf-8'),label+'::json-string'+trail+'/'+str(index),depth+1)
      else:walk(value,trail+'/'+str(index))
   walk(value,'')
 for row in rows:
  source_data=(SOURCE/row['path']).read_bytes()
  if Path(row['path']).suffix.lower() not in CERT_SUFFIXES:scan(source_data,'source:'+row['path'])
  stats['sourceFiles']+=1
  stored_data=(DEST/row.get('archivePath',row['path'])).read_bytes()
  if row.get('storage')=='gzip_original_bytes':
   stats['gzipFiles']+=1;scan(gzip.decompress(stored_data),'stored-gzip-decoded:'+row['path'])
  elif Path(row['path']).suffix.lower() not in CERT_SUFFIXES:scan(stored_data,'stored:'+row['path'])
  stats['storedFiles']+=1
 need(not findings,'credential findings by path/kind only: '+json.dumps(findings))
 original_count=len(rows);original_bytes=sum(r['bytes'] for r in rows);stored_bytes=sum(r.get('storedBytes',r['bytes']) for r in rows)
 receipt={'schemaVersion':1,'status':'PASS_EXACT_ORIGINAL_AND_STORED_BYTES_AND_DECODED_CREDENTIAL_SCAN',
  'recordedUtc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'sourceRoot':str(SOURCE),'archiveRoot':str(DEST),
  'sourceSha':args.source_sha,'scope':'R72 exact root-admitted independent acceptance scope only; archive does not promote any static/phase/CI result to actual runtime, or Win11 target execution to Win7/10 OS acceptance. R70 GNU31 remains separate archived evidence.',
  'sourceFreeze':'Root confirmed all original controllers terminal and no further R72 evidence writes before archive; helper independently reread full source membership and original bytes.',
  'verifiedOriginalFilesBeforeThisReceipt':original_count,'verifiedOriginalBytesBeforeThisReceipt':original_bytes,'storedBytesBeforeThisReceipt':stored_bytes,
  'verified':verified,'excludedCompiledPayloads':omitted,
  'credentialScan':{'status':'PASS_NO_MATCHES','statistics':stats,'patterns':list(PATTERNS),'findings':[],
   'coverage':'Every retained original and stored counterpart; actual UTF16 views, nested JSON base64/encodedCommand raw streams, all decoded JSON string values, and decompressed gzip. Excluded binary/ZIP bytes have exact identity only, are not committed and are not text-scanned.'},
  'finalReceiptBinding':'This receipt itself is copied byte-for-byte after its independent checks and added to the final archive index; its own identity and the final index identity are printed by the helper after final checks to avoid a circular self-hash.',
  'runtimeEvidence':{'path':args.acceptance_report,'sha256':sha(acceptance_path.read_bytes()),'status':runtime['status']},
  'windowsFinalAudit':{'path':args.readonly_audit,'sha256':sha(final_path.read_bytes()),'status':final['status']},
  'publicCertificates':[public_certificate(SOURCE/name,(SOURCE/name).read_bytes()) for name in sorted(CERTIFICATES)],
  'historicalScopes':'All original observations/failures/statuses/phase logs retained without edits. Static reports retaining RUNTIME_NOT_RUN remain static; actual runtime has separate final independent acceptance. No currently failed candidate is implicitly approved.', 'ownedRootMarker':{'path':str(owner),'bytes':len(owner_bytes),'sha256':sha(owner_bytes)}}
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
  'scope':'Exact original R72 API/source/commands/phase/metadata/CI/runtime/final-audit results retained; derived objdump is losslessly gzip-compressed with original and stored bytes/SHA. Compiled EXE/ZIP and caches excluded by exact identity. Static and actual OS runtime claims remain separate.'}
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
  'indexSha256':sha(index_data),'verificationReceiptSha256':sha(receipt_data),'credentialScan':'PASS_NO_MATCHES',
  'decodedRawPayloads':stats['decodedRawPayloads'],'utf16Views':stats['utf16Views'],'sourceRemoved':False}))

if __name__=='__main__':main()
