#!/usr/bin/env python3
"""Archive completed R59 text evidence verbatim; no network/process/runtime actions."""
from pathlib import Path
import base64,collections,hashlib,json,re,shutil,stat
SOURCE=Path('/tmp/fc-round59-ci-evidence')
DEST=Path('/workspace/共享/Eliora/未来教室计划/开发计划/验证记录/2026-11-01-round59-ci')
REVIEW=SOURCE/'archive-source-review-first.json'
INDEX=DEST/'archive-index-first.json'
BINARY_SUFFIX={'.zip','.exe','.dll','.pdb','.obj','.o','.a','.lib','.msi','.cab','.so','.pyc'}
issues=[];scans=collections.Counter();decoded=[]
patterns={
 'PRIVATE_KEY_MATERIAL':re.compile(r'-----BEGIN (?:OPENSSH |RSA |EC |DSA |ENCRYPTED )?PRIVATE KEY-----'),
 'GITHUB_ACCESS_TOKEN':re.compile(r'(?<![A-Za-z0-9_])(?:gh[pousr]_[A-Za-z0-9]{30,255}|github_pat_[A-Za-z0-9_]{50,255})'),
 'SIGNED_DOWNLOAD_URL':re.compile(r'https?://[^\s"<>]+[?&](?:sig|signature|x-amz-signature|x-goog-signature|jwt|token)=[^\s"<>]+',re.I),
 'AUTHORIZATION_CREDENTIAL':re.compile(r'Authorization\s*[:=]\s*[\x22\x27]?Bearer\s+[A-Za-z0-9._~-]{24,}',re.I),
}
def sha(b):return hashlib.sha256(b).hexdigest()
def ordinary(p):
 s=p.lstat()
 if not stat.S_ISREG(s.st_mode):raise RuntimeError('NONREGULAR_SOURCE '+str(p))
 return s

def classify(rel):
 if '.objdump.' in rel:return 'full-raw-objdump-output'
 if '/raw/' in rel:return 'complete-nonbinary-original-artifact-member'
 if rel.endswith(('.py','.ps1')):return 'source-helper'
 if rel.endswith('.jsonl'):return 'original-cargo-executable-metadata'
 if rel.endswith('.json'):return 'report-or-historical-observation'
 return 'full-original-text-output'

def inspect_text(text,rel,layer):
 scans['textViews']+=1
 for category,pattern in patterns.items():
  if pattern.search(text):issues.append({'path':rel,'category':category,'layer':layer})

def decode_candidate(value,rel,label):
 if len(value)<24 or len(value)>24*1024*1024 or re.fullmatch(r'[0-9a-fA-F]{40,}',value):return
 try:b=base64.b64decode(value,validate=True)
 except (ValueError,TypeError):return
 if not b:return
 scans['base64DecodedCandidates']+=1
 if b.startswith((b'MZ',b'PK\x03\x04',b'\x7fELF')):
  issues.append({'path':rel,'category':'EMBEDDED_BINARY_BASE64','layer':label});return
 variants=[]
 try:variants.append(('UTF8',b.decode('utf-8')))
 except UnicodeError:pass
 if len(b)%2==0 and (b.startswith((b'\xff\xfe',b'\xfe\xff')) or b.count(b'\0')>len(b)//8):
  try:variants.append(('UTF16LE',b.decode('utf-16-le')))
  except UnicodeError:pass
 for encoding,text in variants:
  if text and sum(c.isprintable() or c in '\r\n\t' for c in text)/len(text)>=0.90:
   scans['readableBase64Payloads']+=1;inspect_text(text,rel,label+'/'+encoding)
   decoded.append({'path':rel,'label':label,'decodedBytes':len(b),'sha256':sha(b),'encoding':encoding})

def walk_strings(value,rel,path='$'):
 if isinstance(value,dict):
  for key,item in value.items():
   if isinstance(key,str) and re.search(r'(?i)(?:^|[_-])(password|secret|access[_-]?token|refresh[_-]?token|cookie|csrf)(?:$|[_-])',key) and isinstance(item,str) and item:
    # Published schema/source examples and references are allowed; a literal
    # high-entropy credential under a credential-bearing JSON key is not.
    if re.fullmatch(r'[A-Za-z0-9_+/.=-]{32,}',item) and not re.fullmatch(r'[0-9a-fA-F]{40}|[0-9a-fA-F]{64}',item):
     issues.append({'path':rel,'category':'CREDENTIAL_BEARING_JSON_FIELD','layer':path+'.'+key})
   walk_strings(item,rel,path+'.'+str(key))
 elif isinstance(value,list):
  for index,item in enumerate(value):walk_strings(item,rel,path+'[]')
 elif isinstance(value,str):
  # Inspect the entire parsed field too (escaped literal URL/credential syntax).
  inspect_text(value,rel,'JSON:'+path)
  decode_candidate(value,rel,'JSON:'+path)

def review_file(p,rel):
 ordinary(p);b=p.read_bytes();scans['files']+=1;scans['utf8Bytes']+=len(b)
 try:text=b.decode('utf-8-sig')
 except UnicodeError:raise RuntimeError('NON_UTF8_CANDIDATE '+rel)
 inspect_text(text,rel,'originalUTF8')
 # Inspect encoded PowerShell/embedded scripts without executing any payload.
 for match in re.finditer(r'(?<![A-Za-z0-9+/=])[A-Za-z0-9+/]{24,}={0,2}(?![A-Za-z0-9+/=])',text):
  decode_candidate(match.group(),rel,'text-base64-token')
 if rel.endswith('.json'):
  try:walk_strings(json.loads(text),rel)
  except json.JSONDecodeError:raise RuntimeError('INVALID_JSON_CANDIDATE '+rel)
 elif rel.endswith('.jsonl'):
  for line in text.splitlines():
   if line.strip():walk_strings(json.loads(line),rel)
 return {'source':str(p),'relative':rel,'bytes':len(b),'sha256':sha(b),'classification':classify(rel),'utf8':'VALID','nulRecordSeparators':b.count(b'\0')}

assert not DEST.exists(),'Refuse existing archive directory.'
assert not REVIEW.exists(),'Refuse existing source-review report.'
files=[];excluded=[]
for p in sorted(SOURCE.rglob('*'),key=lambda x:x.relative_to(SOURCE).as_posix()):
 info=p.lstat();rel=p.relative_to(SOURCE).as_posix()
 if stat.S_ISDIR(info.st_mode):continue
 if not stat.S_ISREG(info.st_mode):raise RuntimeError('SYMLINK_OR_NONREGULAR_SOURCE '+rel)
 b=p.read_bytes()
 if p.name=='.owner' or p.suffix.lower() in BINARY_SUFFIX or b.startswith((b'MZ',b'PK\x03\x04',b'\x7fELF')):
  excluded.append({'source':str(p),'relative':rel,'bytes':len(b),'sha256':sha(b),'reason':'ownership marker excluded' if p.name=='.owner' else 'actual compiled binary or binary archive excluded'});continue
 if p.name.endswith('.control.json'):raise RuntimeError('ACTIVE_CONTROL_CAPABILITY '+rel)
 files.append(review_file(p,rel))
review={'schema':'future-classroom.r59-source-archive-safety/v1','status':'PASS_NO_MATCHED_CREDENTIAL_OR_SIGNED_URL' if not issues else 'BLOCKED_SUSPECT_TEXT','scope':'All completed regular non-symlink UTF8 candidates; complete original bytes, parsed JSON strings and readable base64 UTF8/UTF16LE payloads scanned without execution. Public metadata, published synthetic data, private-key path references and hashes are not credential contents. No private key/control file opened. Pattern scans are bounded evidence review, not a proof against arbitrary encrypted/novel secret encodings.','candidateFiles':len(files),'candidateBytes':sum(x['bytes'] for x in files),'maxFileBytes':max(x['bytes'] for x in files),'over64MiB':[x['relative'] for x in files if x['bytes']>64*1024*1024],'scans':dict(scans),'readableDecodedPayloads':decoded,'issues':issues,'exclusions':excluded,'excludedFiles':len(excluded),'excludedBytes':sum(x['bytes'] for x in excluded),'historicalStatuses':'Original FAIL/RUNNING/initial observations remain untouched; this is copy/safety status only.','newReviewReport':'This report is appended after the candidate scan and then itself UTF8/credential-scanned once; it does not recursively list its own digest.'}
REVIEW.write_bytes((json.dumps(review,ensure_ascii=False,indent=2)+'\n').encode())
if issues:
 print(json.dumps({'status':'BLOCKED','issues':issues},ensure_ascii=False));raise SystemExit(2)
# Self-review report contains hashes/paths/counts only; append as a separate source.
reportrow=review_file(REVIEW,REVIEW.name)
if issues:raise RuntimeError('GENERATED_REVIEW_CONTAINS_SUSPECT_TEXT')
files.append(reportrow)
DEST.mkdir(parents=False)
for row in files:
 p=Path(row['source']);ordinary(p);b=p.read_bytes()
 if len(b)!=row['bytes'] or sha(b)!=row['sha256']:raise RuntimeError('SOURCE_CHANGED '+row['relative'])
 d=DEST/row['relative'];d.parent.mkdir(parents=True,exist_ok=True)
 with d.open('xb') as f:f.write(b)
# Independent post-copy full bytes comparison; never normalize LF/CRLF/BOM.
for row in files:
 p=Path(row['source']);d=DEST/row['relative'];ordinary(p);ordinary(d)
 b=p.read_bytes();copied=d.read_bytes()
 if b!=copied or len(copied)!=row['bytes'] or sha(copied)!=row['sha256']:raise RuntimeError('POSTCOPY_BYTES_MISMATCH '+row['relative'])
index={'schema':'future-classroom.r59-ci-archive/v1','status':'PASS_VERBATIM_COPY_REHASHED','sourceRoot':str(SOURCE),'archiveRoot':str(DEST),'files':files,'fileCount':len(files),'totalBytes':sum(x['bytes'] for x in files),'exclusions':excluded,'excludedFileCount':len(excluded),'excludedBytes':sum(x['bytes'] for x in excluded),'verification':'All listed sources and destinations are regular non-symlink UTF8 files, bytes compared exactly after copy and SHA256 rehashed. Historical original statuses unchanged. Full logs/objdump and artifact nonbinary members retained, not extracted excerpts.','safetyReview':{'relative':REVIEW.name,'bytes':reportrow['bytes'],'sha256':reportrow['sha256'],'status':review['status']},'archiveIndexSelfReference':'The index does not recursively index itself. Its final bytes/SHA are reported separately to the caller.','scope':'No Git add/commit, source/progress/.gitattributes changes, Cargo, Windows/SSH/network, runtime execution or original deletion.'}
INDEX.write_bytes((json.dumps(index,ensure_ascii=False,indent=2)+'\n').encode())
final=json.loads(INDEX.read_bytes());assert final['fileCount']==len(final['files'])
for row in final['files']:
 d=DEST/row['relative'];assert d.read_bytes()==Path(row['source']).read_bytes() and sha(d.read_bytes())==row['sha256']
assert len([x for x in DEST.rglob('*') if x.is_file()])==len(files)+1
print(json.dumps({'status':index['status'],'files':len(files),'bytes':index['totalBytes'],'excludedFiles':len(excluded),'excludedBytes':index['excludedBytes'],'indexPath':str(INDEX),'indexBytes':INDEX.stat().st_size,'indexSha256':sha(INDEX.read_bytes()),'safetyScans':dict(scans)},ensure_ascii=False))
