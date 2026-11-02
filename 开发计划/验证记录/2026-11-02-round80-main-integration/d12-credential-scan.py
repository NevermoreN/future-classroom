"""Additional bounded credential scan of all archived bytes and decoded views.
Control-byte admission remains the separately frozen exact-path/SHA scanner.
Never prints matching content, tokens, or URLs. No scope/private-key reads.
"""
from pathlib import Path
import base64,hashlib,json,re,tarfile,urllib.parse,time
R=Path('/tmp/fc-round80-controlled-repair');P=R/'abi2-install-maintenance-archive-staged-second';D=R/'root-d12-credential-views-first';assert not D.exists();D.mkdir();t=time.monotonic();idx=json.loads((P/'archive-index-first.json').read_bytes());archive=P/idx['archive']['path'];assert hashlib.sha256(archive.read_bytes()).hexdigest()=='0506a508e7e8d15cf5bbb73b2d431d5ad7a44caaacdfdecd1e57cbaa0514a0ae'
patterns={k:re.compile(v)for k,v in [('PRIVATE_KEY',r'-----BEGIN (?:RSA |OPENSSH |EC |DSA )?PRIVATE KEY-----'),('GITHUB_TOKEN',r'\b(?:gh[pousr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,})\b'),('OPENAI_KEY',r'\bsk-(?:proj-|svcacct-)[A-Za-z0-9_-]{20,}'),('SCOPE_ENV_VALUE',r'FC_OWNED_REMOVAL_SCOPE_HEX[\x22\x27]?\s*[:=]\s*[\x22\x27]?[0-9a-fA-F]{512,}'),('AUTH_BEARER',r'(?i)authorization[\x22\x27]?\s*[:=]\s*[\x22\x27]?bearer\s+[A-Za-z0-9_.-]{20,}') ]}
stats=dict(members=0,textViews=0,jsonStringViews=0,base64Views=0,jsonlViews=0,clixmlViews=0);hits=[]
def scan(raw,label,depth=0,budget=None):
 assert depth<=12
 if budget is None:budget=[0,0,set()]
 digest=hashlib.sha256(raw).digest()
 if digest in budget[2]:return
 budget[2].add(digest);budget[0]+=len(raw);budget[1]+=1;assert budget[0]<=256*1024*1024 and budget[1]<=1000000
 forms=[]
 if raw.startswith((b'\xff\xfe',b'\xfe\xff')):
  try:forms.append(raw.decode('utf-16'))
  except UnicodeError:pass
 else:
  try:forms.append(raw.decode('utf-8-sig'))
  except UnicodeError:
   try:forms.append(raw.decode('cp936'))
   except UnicodeError:pass
 if b'\x00'in raw and len(raw)%2==0:
  try:forms.append(raw.decode('utf-16-le'))
  except UnicodeError:pass
 for text in set(forms):
  stats['textViews']+=1
  cleaned=''.join(c for c in text if ord(c)>=32 or c in '\r\n\t')
  for name,pattern in patterns.items():
   if pattern.search(cleaned):hits.append(dict(path=label,rule=name,depth=depth))
  for url in re.findall(r'https?://[^\s\x22\x27<>]+',cleaned):
   try:keys={x.lower()for x in urllib.parse.parse_qs(urllib.parse.urlsplit(url).query)}
   except ValueError:continue
   if keys&{'sig','signature','token','access_token','x-amz-signature','x-amz-credential'}:hits.append(dict(path=label,rule='SIGNED_OR_TOKEN_URL',depth=depth))
  if '#< CLIXML' in text or '<Objs 'in text:
   decoded=re.sub(r'_x([0-9a-fA-F]{4})_',lambda m:chr(int(m[1],16)),text)
   if decoded!=text:stats['clixmlViews']+=1;scan(decoded.encode(),label+'::clixml',depth+1,budget)
  for token in set(re.findall(r'(?<![A-Za-z0-9+/])[A-Za-z0-9+/]{80,}={0,2}(?![A-Za-z0-9+/])',text)):
   if len(token)>44*1024*1024:raise AssertionError('BASE64_BOUND')
   try:b=base64.b64decode(token,validate=True)
   except ValueError:continue
   if base64.b64encode(b).decode()!=token:continue
   stats['base64Views']+=1;scan(b,label+'::base64',depth+1,budget)
  def walk(x):
   if isinstance(x,str):stats['jsonStringViews']+=1;scan(x.encode(),label+'::json-string',depth+1,budget)
   elif isinstance(x,dict):
    for k,v in x.items():walk(k);walk(v)
   elif isinstance(x,list):
    for v in x:walk(v)
  try:x=json.loads(text)
  except (ValueError,RecursionError):x=None
  if x is not None:walk(x)
  elif depth==0 and label.endswith(('.jsonl','.log')):
   for line in text.splitlines():
    if line.lstrip().startswith(('{','[')):
     try:x=json.loads(line)
     except (ValueError,RecursionError):continue
     stats['jsonlViews']+=1;walk(x)
with tarfile.open(archive,'r:gz')as tf:
 for m in tf:
  assert m.isfile()and m.size<=128*1024*1024;stats['members']+=1;scan(tf.extractfile(m).read(),m.name)
assert stats['members']==2630
result=dict(status='PASS_ADDITIONAL_ARCHIVED_CREDENTIAL_VIEWS'if not hits else'FAIL_FINDINGS_CONTENT_NOT_PRINTED',archiveSha256=idx['archive']['sha256'],stats=stats,hitCount=len(hits),findings=hits,seconds=time.monotonic()-t,scope='Credentials in original archive, JSON/JSONL, CLIXML and canonical base64 views; exact raw control exceptions checked separately',sensitiveSourceFilesRead=False);(D/'report.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(dict(status=result['status'],stats=stats,hitCount=len(hits),reportSha256=hashlib.sha256((D/'report.json').read_bytes()).hexdigest())));raise SystemExit(0 if not hits else 1)
