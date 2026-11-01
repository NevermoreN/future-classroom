#!/usr/bin/env python3
"""R69 exact evidence archive + independent source reread + decoded secret scan.
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

SOURCE=Path('/tmp/fc-round69-ci-evidence')
DEST=Path('/workspace/共享/Eliora/未来教室计划/开发计划/验证记录/2026-11-01-round69-ci')
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
 'potential-pairing-short-code':re.compile(r'(?i)\\?[\x22\x27]pairingCode\\?[\x22\x27]\s*:\s*\\?[\x22\x27][0-9]{8}\\?[\x22\x27]'),
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

def main():
 parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--expected-runtime-sha256',required=True);args=parser.parse_args()
 need(re.fullmatch(r'[0-9a-f]{64}',args.expected_runtime_sha256),'exact root-confirmed runtime SHA argument')
 owner=SOURCE/'.owner';owner_bytes=owner.read_bytes();need(owner_bytes==b'future-classroom-owned-r69-ci-evidence/v1\n','exact R69 owner marker')
 need(SOURCE.is_dir() and not DEST.exists(),'fresh archive required')
 need(not (SOURCE/RECEIPT).exists(),'fresh receipt required')
 final=as_json((SOURCE/'windows-readonly-final-first.json').read_bytes())
 need(final['status']=='PASS' and final.get('completedAtUtc'),'terminal exact Windows final audit required')
 need(sha((SOURCE/'windows-readonly-final-first.json').read_bytes())=='a0c79bdcc98ace31e160c31c5eeba42852babcc14d17ec5a54e5e05cd8044886','root-confirmed final audit bytes')
 runtime=as_json((SOURCE/'independent-runtime-first.json').read_bytes())
 need(runtime['status']=='PASS_R69_MODERN_WIN11_REGRESSION_WITH_WIN7_CI_FAILURE_RETAINED' and runtime['sourceSha']=='8bc1c9812df765865a54988a23ff3973d04884fc','terminal exact source-scoped R69 audit')
 need(sha((SOURCE/'independent-runtime-first.json').read_bytes())==args.expected_runtime_sha256,'root-confirmed independent runtime bytes')
 need(runtime['modern']['status']=='PASS_ACTUAL_MODERN_CI_AND_FINITE_WIN11_RUNTIME_ONLY' and runtime['win7']['status']=='ACTUAL_CI_FAIL_RETAINED_NOT_RUNTIME_APPROVED' and runtime['win7']['runtime']=='NOT_RUN_NOT_APPROVED','modern PASS and original failed Win7 scopes remain separate')
 paths=sorted(p for p in SOURCE.rglob('*') if not p.is_dir())
 rows=[];omitted=[]
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
  'sourceSha':'8bc1c9812df765865a54988a23ff3973d04884fc','scope':'R69 source8bc modern CI and finite actual Win11 runtime PASS, original Win7-target CI FAIL retained and candidate NOT_RUN. No both-CI PASS, R70/R71 validation, Win7/10 OS or campus certification.',
  'sourceFreeze':'Root confirmed all original controllers terminal and no further R69 evidence writes before archive; helper independently reread full source membership and original bytes.',
  'verifiedOriginalFilesBeforeThisReceipt':original_count,'verifiedOriginalBytesBeforeThisReceipt':original_bytes,'storedBytesBeforeThisReceipt':stored_bytes,
  'verified':verified,'excludedCompiledPayloads':omitted,
  'credentialScan':{'status':'PASS_NO_MATCHES','statistics':stats,'patterns':list(PATTERNS),'findings':[],
   'coverage':'Every retained original and stored counterpart; actual UTF16 views, nested JSON base64/encodedCommand raw streams, all decoded JSON string values, and decompressed gzip. Excluded binary/ZIP bytes have exact identity only, are not committed and are not text-scanned.'},
  'finalReceiptBinding':'This receipt itself is copied byte-for-byte after its independent checks and added to the final archive index; its own identity and the final index identity are printed by the helper after final checks to avoid a circular self-hash.',
  'runtimeEvidence':{'path':'independent-runtime-first.json','sha256':sha((SOURCE/'independent-runtime-first.json').read_bytes()),'status':runtime['status']},
  'windowsFinalAudit':{'path':'windows-readonly-final-first.json','sha256':sha((SOURCE/'windows-readonly-final-first.json').read_bytes()),'status':final['status']},
  'historicalScopes':'Original failed Win7 CI/result/Clippy diagnostic and210 nonbinary originals retained exactly. Modern static report remains RUNTIME_NOT_RUN; separate actual modern Win11 harness/public/visual scope is proved by independent-runtime-first.json. No failed candidate approval or execution. R71 source reorder is separate later work.', 'ownedRootMarker':{'path':str(owner),'bytes':len(owner_bytes),'sha256':sha(owner_bytes)}}
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
  'scope':'Exact original R69 API/source/commands/CI/runtime/final-audit results retained; derived objdump is losslessly gzip-compressed with original and stored bytes/SHA. Compiled EXE/ZIP and caches excluded by exact identity. Static and actual OS runtime claims remain separate.'}
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
