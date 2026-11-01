#!/usr/bin/env python3
"""Local exact nonbinary evidence archiver for R47-R51. Default prepare never seals.

Write mode requires the coordinator's explicit all-consumers-finished declaration
and an exact-hash final-proof file. This script never stops processes, follows
artifact reference paths, opens private keys/databases/live control capabilities,
or cleans source roots. A failed partial archive is retained, never retried over.
"""
from pathlib import Path
import argparse,base64,binascii,datetime,hashlib,json,os,re,stat,sys,urllib.parse
sys.dont_write_bytecode=True
MAIN=Path('/workspace/共享/Eliora/未来教室计划')
MAX_FILES=10000
MAX_PATHS=30000
MAX_FILE=64*1024*1024
MAX_TOTAL=512*1024*1024
MAX_ALL_TOTAL=1024*1024*1024
ALLOWED={'.json','.jsonl','.log','.py','.mjs','.ps1','.rs','.txt','.md','.diff','.patch','.pem','.stdout','.stderr','.raw','.toml','.lock','.yml','.yaml','.before','.after'}
BIN_EXT={'.exe','.zip','.dll','.pdb','.pyc','.rlib','.rmeta','.obj','.o','.a','.so','.lib','.bin'}
PRIVATE_EXT={'.key','.pfx','.p12','.db','.sqlite','.sqlite3','.db-wal','.db-shm','.sqlite-wal','.sqlite-shm'}
EXCLUDED_DIRS={'target','programs','__pycache__','.git','private','secrets'}
SECRET_PATH=('/config/.ssh/','/config/.local/share/future-classroom-test-access/','/root/.ssh/')
PATTERNS=[
 ('PRIVATE_KEY',re.compile(r'-----BEGIN(?: [A-Z0-9]+)? PRIVATE KEY-----')),
 ('GITHUB_TOKEN',re.compile(r'\b(?:gh[pousr]_[A-Za-z0-9]{24,}|github_pat_[A-Za-z0-9_]{30,})\b')),
 ('API_TOKEN',re.compile(r'\bsk-(?:proj-)?[A-Za-z0-9_-]{30,}\b')),
 ('AUTHORIZATION_SECRET',re.compile(r'(?i)\bAuthorization[\x22\x27]?\s*[:=]\s*[\x22\x27]?(?:Bearer|Basic)\s+[A-Za-z0-9+/=_-]{20,}')),
 ('SESSION_COOKIE',re.compile(r'__Host-FutureClassroom=([0-9a-fA-F]{64})')),
 ('CAPABILITY_OR_CSRF',re.compile(r'(?i)(?:stopCapability|expectedCsrf|csrf)\s*[=:]\s*[\x22\x27]([0-9a-f]{64})[\x22\x27]')),
]
PS64=re.compile(r"(?:FromBase64String\(\s*[\"']|[-]EncodedCommand\s+)([A-Za-z0-9+/]{32,}={0,2})",re.I)
def need(ok,why):
 if not ok:raise ValueError(why)
def sha(data):return hashlib.sha256(data).hexdigest()
def safe_bytes(p):
 p=Path(p);need(p.is_absolute() and '..' not in p.parts,'absolute ordinary evidence path required')
 directory=os.open('/',os.O_RDONLY|os.O_DIRECTORY);fd=None
 try:
  for component in p.parts[1:-1]:
   nxt=os.open(component,os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW,dir_fd=directory);os.close(directory);directory=nxt
  fd=os.open(p.name,os.O_RDONLY|os.O_NOFOLLOW,dir_fd=directory);info=os.fstat(fd)
  need(stat.S_ISREG(info.st_mode) and info.st_size<=MAX_FILE,'ordinary bounded evidence required: '+p.name)
  chunks=[];remaining=MAX_FILE+1
  while remaining:
   data=os.read(fd,min(1024*1024,remaining))
   if not data:break
   chunks.append(data);remaining-=len(data)
  raw=b''.join(chunks);need(len(raw)<=MAX_FILE,'evidence grew beyond bound: '+p.name);return raw
 finally:
  if fd is not None:os.close(fd)
  os.close(directory)
def load(p):return json.loads(safe_bytes(p).decode('utf-8-sig'))
def fact(p):
 p=Path(p);info=p.lstat();need(stat.S_ISREG(info.st_mode) and not p.is_symlink(),'not ordinary evidence: '+p.name);need(info.st_size<=MAX_FILE,'proof file bound: '+p.name)
 data=safe_bytes(p);return {'path':str(p),'bytes':len(data),'sha256':sha(data)}
def sensitive_path(p):
 low=p.name.lower()
 return str(p).startswith(SECRET_PATH) or low.startswith(('id_ed25519','id_rsa')) or low.endswith('.control.json') or low in {'.request-stop','.browser-owner.json'} or p.suffix.lower() in PRIVATE_EXT or low.endswith(('.sqlite3-wal','.sqlite3-shm'))
def relative(p,root):
 rel=p.relative_to(root).as_posix();need(rel and len(rel)<=2048 and len(p.relative_to(root).parts)<=32 and not any(x in rel for x in ['\0','\r','\n']),'unsafe evidence relative path');return rel

BINDINGS={'47': {'root': '/tmp/fc-round47-https-settings-evidence', 'owner': '.owner', 'ownerBytes': 50, 'ownerSha256': 'b3260b8fcecebb3d67483f7f6c7e2bb56fe95974943b75c2f42fbaab64f3171f', 'destination': '2026-11-01-round47-https-settings'}, '48': {'root': '/tmp/fc-round48-certificate-query-evidence', 'owner': '.owner.json', 'ownerBytes': 165, 'ownerSha256': '85925c51e7827d9739f3bf4e8bcbfee18236a82b93a1dc6e673299e132216489', 'destination': '2026-11-01-round48-certificate-query'}, '49': {'root': '/tmp/fc-round49-https-settings-ui-evidence', 'owner': '.owner', 'ownerBytes': 46, 'ownerSha256': 'cd125bcf7c62e90ae9af055836d2c23101b1b1bad9ce090dae8a1a14891139f4', 'destination': '2026-11-01-round49-https-ui'}, '50': {'root': '/tmp/fc-round50-https-integration-evidence', 'owner': '.owner', 'ownerBytes': 57, 'ownerSha256': '8828909afe415d1004addc3131cca8f5985841c32493dba0fb3bbe6e20d55f4b', 'destination': '2026-11-01-round50-https-integration'}, '51': {'root': '/tmp/fc-round51-browser-diagnostics-evidence', 'owner': '.owner', 'ownerBytes': 193, 'ownerSha256': '9500e7cc18a6ad2d0b98b49a4dc47ede60a4502310f51f698f9dd57c9f94d681', 'destination': '2026-11-01-round51-browser-diagnostics'}}

class Scanner:
    def __init__(self):
        self.blocked=[]; self.annotations=[]; self.decoded=[]; self.seen=set(); self.nodes=0; self.decoded_total=0
    def finding(self,kind,origin):
        row={'kind':kind,'pathOrDecodedLocation':origin}
        if row not in self.blocked: self.blocked.append(row)
    def annotation(self,kind,origin):
        row={'kind':kind,'pathOrDecodedLocation':origin}
        if row not in self.annotations: self.annotations.append(row)
    def hit(self,kind,value,origin,retired=False,synthetic=False):
        # No found secret value or hash is emitted. Repetitive source models and
        # exact retired synthetic fixtures are classified, not called real keys.
        tail=value.split('=',1)[-1]
        if len(tail)==64 and len(set(tail.lower()))==1:
            self.annotation('REPETITIVE_SYNTHETIC_FIXTURE',origin); return
        if kind=='CAPABILITY_OR_CSRF' and retired:
            self.annotation('EXPIRED_EXACT_OWNED_STOP_CAPABILITY',origin); return
        if kind in ('SESSION_COOKIE','CAPABILITY_OR_CSRF','JSON_SESSION_SECRET') and synthetic:
            self.annotation('EXPIRED_EXPLICIT_SYNTHETIC_FIXTURE_SECRET',origin); return
        self.finding(kind,origin)
    def text(self,text,origin,depth=0,retired=False,synthetic=False):
        if depth>8: self.finding('DECODE_DEPTH_BOUND',origin); return
        for kind,pattern in PATTERNS:
            for m in pattern.finditer(text):
                self.hit(kind,m.group(1) if m.lastindex else m.group(),origin,retired,synthetic)
        for m in re.finditer(r'https?://[^\s\x22\x27<>]+',text):
            try:
                u=urllib.parse.urlsplit(m.group().rstrip('),;]'))
                keys={k.lower() for k,_ in urllib.parse.parse_qsl(u.query)}
                if u.username or u.password or keys & {'sig','signature','x-amz-signature','x-goog-signature','access_token'}:
                    self.finding('SIGNED_OR_CREDENTIALED_URL',origin)
            except ValueError:self.finding('MALFORMED_URL_REVIEW',origin)
        for m in PS64.finditer(text): self.decode(m.group(1),origin+'#PowerShellBase64',depth+1,retired,synthetic)
    def decode(self,value,origin,depth,retired,synthetic):
        if depth>8: self.finding('DECODE_DEPTH_BOUND',origin); return
        if not value: return
        if len(value)>MAX_FILE*2: self.finding('BASE64_PAYLOAD_BOUND',origin); return
        try:data=base64.b64decode(value,validate=True)
        except (ValueError,binascii.Error):self.finding('INVALID_DECLARED_BASE64',origin);return
        self.decoded_total+=len(data)
        if len(data)>MAX_FILE or self.decoded_total>MAX_TOTAL: self.finding('DECODE_TOTAL_BOUND',origin);return
        key=(sha(data),retired,synthetic)
        if key in self.seen:return
        self.seen.add(key)
        row={'location':origin,'bytes':len(data),'sha256':key[0]};self.decoded.append(row)
        if data.startswith(b'MZ') or data.startswith(b'PK\x03\x04'):
            self.finding('INLINE_EXECUTABLE_OR_ZIP',origin);return
        try:
            enc='utf-16le' if len(data)>2 and data[1::2].count(0)>len(data)//4 else 'utf-8-sig'
            text=data.decode(enc)
        except UnicodeError:
            row['classification']='NON_TEXT_PUBLIC_HASH_OR_CERTIFICATE_NOT_DESERIALIZED';return
        row['encoding']=enc;self.text(text,origin,depth,retired,synthetic)
        try:self.walk(json.loads(text),origin,0,retired,synthetic,depth)
        except ValueError:pass
    def walk(self,x,origin,depth=0,retired=False,synthetic=False,decode_depth=0):
        self.nodes+=1
        if self.nodes>2000000 or depth>16:self.finding('JSON_SCAN_BOUND',origin);return
        if isinstance(x,dict):
            for k,v in x.items():
                where=origin+'/'+str(k); low=str(k).lower()
                if isinstance(v,str):
                    if low in {'password','access_token','authorization','stopcapability','capability','token','csrf','expectedcsrf','cookie','pollsecret'} and v:
                        if low in {'password','access_token','authorization'}:
                            if len(v)>8 and not any(s in v.lower() for s in ['synthetic','fixture','not_run','redacted','omitted']):self.finding('JSON_PASSWORD_OR_ACCESS_TOKEN_REVIEW',where)
                        elif re.fullmatch(r'(?:__Host-FutureClassroom=)?[0-9a-fA-F]{64}',v):self.hit('CAPABILITY_OR_CSRF' if low in {'stopcapability','capability'} else 'JSON_SESSION_SECRET',v,where,retired,synthetic)
                    if 'base64' in low or low=='encodedcommand':self.decode(v,where,decode_depth+1,retired,synthetic)
                elif isinstance(v,(dict,list)):self.walk(v,where,depth+1,retired,synthetic,decode_depth)
        elif isinstance(x,list):
            for i,v in enumerate(x):
                if isinstance(v,(dict,list)):self.walk(v,origin+'/'+str(i),depth+1,retired,synthetic,decode_depth)

def enumerate_tree(root,skip_paths=()):
 need(root.is_dir() and not root.is_symlink(),'source evidence root invalid')
 skip={Path(x) for x in skip_paths};entries=[];paths=0;files=0;folded=set()
 def visit(directory):
  nonlocal paths,files
  children=[]
  # Bound each materialized directory as well as total traversed paths.
  with os.scandir(directory) as it:
   for child in it:
    paths+=1;need(paths<=MAX_PATHS,'evidence path enumeration bound');children.append(child)
  for child in sorted(children,key=lambda c:c.name):
   p=Path(child.path);rel=relative(p,root)
   if p in skip:continue
   need(rel.casefold() not in folded,'case-colliding evidence paths');folded.add(rel.casefold())
   info=p.lstat();need(not stat.S_ISLNK(info.st_mode),'symlink never followed: '+rel)
   if stat.S_ISDIR(info.st_mode):
    if p.name.lower() in EXCLUDED_DIRS:entries.append({'path':rel,'type':'excluded-directory','reason':'PRIVATE_OR_COMPILATION_SUBTREE_NEVER_OPENED_OR_ENUMERATED'});continue
    entries.append({'path':rel,'type':'directory'});visit(p)
   else:
    need(stat.S_ISREG(info.st_mode),'non-regular evidence: '+rel);files+=1;need(files<=MAX_FILES,'evidence files exceed10000')
    entries.append({'path':rel,'type':'file','bytes':info.st_size})
 visit(root);return entries

def expired_stop(root,rel):
 if '.control.json.stop-' not in rel:return False
 parent=root/rel.split('.control.json.stop-',1)[0]
 if not parent.is_file() or parent.is_symlink() or parent.stat().st_size>MAX_FILE:return False
 try:x=load(parent)
 except (ValueError,UnicodeError):return False
 n=x.get('nativeOutcome',{});c=x.get('cleanupDetails',{})
 return x.get('cleanup')=='PASS' and all(n.get(k) is True for k in ['actualJobProcessesZero','originalPrimaryExited','originalHandlesClosed']) and c.get('rootAbsent') is True

def stable(root,rows,entries,skip_paths=()):
 need(enumerate_tree(root,skip_paths)==entries,'source membership or size changed')
 for row in rows:
  p=root/row['path'];data=safe_bytes(p);need(len(data)==row['bytes'] and sha(data)==row['sha256'],'source bytes changed: '+row['path'])

def inventory(root,skip_paths=()):
 entries=enumerate_tree(root,skip_paths);scan=Scanner();rows=[];excluded=[];total=0
 for entry in entries:
  rel=entry['path'];p=root/rel
  if entry['type']=='excluded-directory':excluded.append(entry);continue
  if entry['type']!='file':continue
  if sensitive_path(p):excluded.append({'path':rel,'reason':'PRIVATE_DATABASE_OR_LIVE_CAPABILITY_NEVER_READ_OR_HASHED'});continue
  if p.suffix.lower()=='.pem' and p.name!='tls-public.pem':excluded.append({'path':rel,'reason':'UNDECLARED_PEM_NEVER_READ_OR_HASHED'});continue
  if p.suffix.lower() in BIN_EXT:excluded.append({'path':rel,'bytes':entry['bytes'],'reason':'BINARY_OR_COMPILATION_CONTENT_NEVER_READ_OR_HASHED'});continue
  if p.suffix.lower() not in ALLOWED and p.name not in {'.owner','LICENSE'}:scan.finding('UNKNOWN_FILE_TYPE_REQUIRES_REVIEW',rel);continue
  need(entry['bytes']<=MAX_FILE,'single evidence file exceeds64MiB: '+rel)
  raw=safe_bytes(p);total+=len(raw);need(total<=MAX_TOTAL,'round evidence exceeds512MiB')
  if raw.startswith(b'MZ') or raw.startswith(b'PK\x03\x04'):scan.finding('INLINE_BINARY_FILE_MAGIC',rel);continue
  try:
   encoding='utf-16' if raw.startswith((b'\xff\xfe',b'\xfe\xff')) else 'utf-8-sig';text=raw.decode(encoding)
  except UnicodeError:scan.finding('UNCLASSIFIED_NON_TEXT_ORIGINAL',rel);continue
  if p.suffix.lower()=='.pem' and not text.lstrip().startswith(('-----BEGIN CERTIFICATE-----','-----BEGIN PUBLIC KEY-----')):scan.finding('PEM_NOT_PUBLIC_CERTIFICATE_OR_KEY',rel)
  retired=expired_stop(root,rel);synthetic=False;obj=None
  if p.suffix.lower()=='.json':
   try:obj=json.loads(text)
   except ValueError:scan.finding('INVALID_JSON',rel)
   if isinstance(obj,dict):
    cleanup=obj.get('cleanupDetails',{});synthetic=(str(obj.get('credentialScope','')).startswith('Disposable synthetic probe account only') and obj.get('cleanup')=='PASS' and cleanup.get('rootAbsent') is True and cleanup.get('ownedProcessesRemaining')==0)
  scan.text(text,rel,retired=retired,synthetic=synthetic)
  if obj is not None:scan.walk(obj,rel,retired=retired,synthetic=synthetic)
  elif p.suffix.lower()=='.jsonl':
   for n,line in enumerate(text.splitlines(),1):
    if line.strip():
     try:scan.walk(json.loads(line),rel+':'+str(n),retired=retired,synthetic=synthetic)
     except ValueError:scan.finding('INVALID_JSONL',rel+':'+str(n))
  rows.append({'path':rel,'bytes':len(raw),'sha256':sha(raw),'encoding':encoding,'kind':'owner-marker' if p.name in {'.owner','.owner.json'} else 'original-raw' if '-raw/' in rel or '/raw/' in rel else 'original-source-helper-command-or-report'})
 stable(root,rows,entries,skip_paths)
 return {'rows':rows,'excluded':excluded,'entries':entries,'bytes':total,'scanner':scan}

def validate_owner(binding):
 root=Path(binding['root']);need(root.is_dir() and not root.is_symlink(),'invalid exact owned root');p=root/binding['owner'];f=fact(p)
 need(f['bytes']==binding['ownerBytes'] and f['sha256']==binding['ownerSha256'],'original owner marker changed: '+root.name);return f

def allowed_proof(p):
 p=Path(p);need(p.is_absolute() and not p.is_symlink(),'proof path must be explicit ordinary absolute path')
 roots=[Path(b['root']) for b in BINDINGS.values()]
 need(any(p.is_relative_to(root) and p.resolve().is_relative_to(root.resolve()) for root in roots),'proof outside explicitly owned evidence roots')
 need(not sensitive_path(p) and p.suffix.lower()=='.json','proof must be safe JSON, no capability/key/database');return p

def final_proof(path):
 path=allowed_proof(path);fact(path);proof=load(path);need(proof.get('schema')=='fc.https-rounds.final-archive-authority/v1','final-proof schema required')
 need(proof.get('status')=='ALL_CONSUMERS_FINISHED_SOURCE_INTEGRATED' and proof.get('activeConsumers')==[],'all consumers/source integration not explicitly terminal')
 need(proof.get('sourceIntegration')=='EXACT_VALIDATED_BYTES_INTEGRATED','source bytes not explicitly integrated')
 commit=proof.get('gitHead');need(isinstance(commit,str) and re.fullmatch(r'[0-9a-f]{40}',commit),'explicit exact Git head required, not substituted for dirty-delta SHA proof')
 rows=proof.get('proofs');need(isinstance(rows,list) and 5<=len(rows)<=100,'bounded final proofs required');facts=[];roles=set()
 for row in rows:
  need(isinstance(row,dict),'invalid final proof row');p=allowed_proof(row['path']);got=fact(p);need(got['sha256']==row['sha256'],'final proof changed: '+p.name)
  value=load(p);status=value.get('status');need(isinstance(status,str) and status.startswith('PASS') and status==row['status'],'final proof not explicitly PASS: '+p.name)
  if row.get('cleanupRequired',False):need(value.get('cleanup')=='PASS','required final cleanup absent: '+p.name)
  roles.add(row['role']);facts.append({**got,'role':row['role'],'status':status})
 need({'source-integration','host-full-matrix','windows-runtime','browser-final','readonly-final'}<=roles,'missing terminal source/Host/runtime/browser/readonly roles')
 return {'declaration':fact(path),'gitHead':commit,'sourceIntegration':proof['sourceIntegration'],'proofs':facts,'scope':'Explicit coordinator all-finished/source-integration declaration + exact-hash PASS proof originals. Does not infer compiler success from a baseline Git hash or independently rerun those tests.'}

def copy_rows_to_destination(root,dest,rows,progress):
 need(not dest.exists(),'destination exists; no overwrite: '+dest.name);dest.mkdir();progress['destinationCreated']=True
 for row in rows:
  src=root/row['path'];raw=safe_bytes(src);need(len(raw)==row['bytes'] and sha(raw)==row['sha256'],'source changed before copy: '+row['path'])
  target=dest/row['path'];target.parent.mkdir(parents=True,exist_ok=True)
  with target.open('xb') as f:f.write(raw)
  need(safe_bytes(target)==raw,'exact copied byte mismatch');progress['copiedFiles']+=1;progress['copiedBytes']+=len(raw)

def seal_round(number,binding,inv,authority,report_path,progress):
 root=Path(binding['root']);need(not any(x['path'] in {'INDEX.json','INDEX.md'} for x in inv['rows']),'source reserved INDEX collision');dest=MAIN/'开发计划/验证记录'/binding['destination']
 for parent in [MAIN,MAIN/'开发计划',MAIN/'开发计划/验证记录']:need(parent.is_dir() and not parent.is_symlink(),'archive parent invalid')
 stable(root,inv['rows'],inv['entries']);copy_rows_to_destination(root,dest,inv['rows'],progress);stable(root,inv['rows'],inv['entries'])
 manifest=sha(json.dumps(inv['rows'],sort_keys=True,separators=(',',':')).encode())
 index={'schema':'fc.https-rounds.exact-nonbinary-archive/v1','round':number,'status':'EXACT_ORIGINAL_BYTES_COPIED_ALL_CONSUMERS_EXPLICITLY_FINISHED','sourceRoot':str(root),'sourceOwner':validate_owner(binding),'files':inv['rows'],'fileCount':len(inv['rows']),'bytes':inv['bytes'],'membership':inv['entries'],'fileManifestSha256':manifest,'excluded':inv['excluded'],'finalAuthority':authority,'independentReport':str(report_path),'generatedIndexExcludedFromOwnLedger':True,'failureHistory':'Every eligible original FAIL, before/source snapshot, command/stdout/stderr/model and actual public raw journal is preserved without status rewrite. The R46 first browser NETWORK_LOAD_FAILED root cause remains unknown; later finite PASS does not prove a fix. R50 original900s Host timeout is retained separately from the terminal rerun.','safetyScope':'Keys/databases/LIVE control contents, binaries, ZIPs and compilation subtrees are never read or hashed. Public owned certificate PEM and exact raw text are included. Embedded JSON/PowerShell text is scanned, not executed. Only explicitly declared final-authority proof members in the exact owned roots are opened; arbitrary artifact references are not followed.','gateScope':'No production listener/start/readiness permission, strictTCP5s, actualWin7/Win10, visibleSeewo, audible playback or native-held-cancellation claim is upgraded by archiving.','cleanup':'No source-root, target, process, Windows, SSH, credential or archive cleanup is performed. Parent may later clean exact roots only after commit/byte proof.'}
 for name,data in [('INDEX.json',json.dumps(index,ensure_ascii=False,indent=2)+'\n'),('INDEX.md',f'# R{number} HTTPS 开发与验证非二进制原件\n\nINDEX.json 列出所有原件的字节数、SHA256、实际 owner 和排除范围。源文件逐字节复制，生成索引不进入自己的清单。所有失败、修正前后源码、实际命令与原始输出均保留；后续通过不覆盖历史失败或证明其根因已修复。\n\n本目录是有限实现及实测证据，不授予生产监听或证书就绪许可，也不升级严格 TCP 五秒、Win7/Win10 真机、希沃可见、可听播放及长期现场门槛。归档不会清理任何原目录、进程、缓存、SSH 或授权。\n')]:
  with (dest/name).open('x',encoding='utf-8') as f:f.write(data)
 stable(root,inv['rows'],inv['entries']);return {'destination':str(dest),'copiedFiles':progress['copiedFiles'],'copiedBytes':progress['copiedBytes'],'indexJson':fact(dest/'INDEX.json'),'indexMarkdown':fact(dest/'INDEX.md'),'allCopiedBytesMatch':True,'manifestSha256':manifest}

def main():
 parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--round',action='append',choices=sorted(BINDINGS),dest='rounds');parser.add_argument('--report',required=True,type=Path);parser.add_argument('--write-archive',action='store_true');parser.add_argument('--all-consumers-finished',action='store_true');parser.add_argument('--final-proof',type=Path);a=parser.parse_args()
 selected=a.rounds or sorted(BINDINGS);need(len(set(selected))==len(selected),'duplicate selected round')
 report_paths=[a.report]+[a.report.with_name(a.report.stem+'.round'+n+a.report.suffix) for n in selected]
 for p in report_paths:need(p.is_absolute() and p.suffix=='.json' and not p.is_relative_to(MAIN) and not p.exists() and p.parent.is_dir() and not p.parent.is_symlink(),'fresh explicit report path required')
 if a.write_archive:
  for p in report_paths:need(not any(p.is_relative_to(Path(b['root'])) or p.is_relative_to(MAIN/'开发计划/验证记录'/b['destination']) for b in BINDINGS.values()),'write report must be outside source/dest; no recursive ledger')
 handles={};summary={'schema':'fc.https-rounds.archive-run/v1','status':'FAIL','createdUtc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'helper':fact(Path(__file__)),'archivePerformed':False,'mainModified':False,'partialArchiveMayRemain':False,'rounds':[],'scope':'Local reads and explicitly gated create-new copies only; no SSH/Cargo/Windows/Git mutation or cleanup.'}
 per={n:{'round':n,'status':'RUNNING','destinationCreated':False,'copiedFiles':0,'copiedBytes':0,'sourceRoot':BINDINGS[n]['root']} for n in selected}
 try:
  for p in report_paths:handles[p]=p.open('x',encoding='utf-8')
  # Entire five-root read/scan occurs only when root explicitly invokes this helper.
  inventories={};total=0
  for n in selected:
   owner=validate_owner(BINDINGS[n]);inv=inventory(Path(BINDINGS[n]['root']),report_paths);inventories[n]=inv;total+=inv['bytes'];need(total<=MAX_ALL_TOTAL,'combined nonbinary evidence exceeds1GiB')
   scan=inv['scanner'];per[n].update(status='PREPARED_ONLY_NOT_SEALED',owner=owner,fileCount=len(inv['rows']),bytes=inv['bytes'],files=inv['rows'],excluded=inv['excluded'],sourceMembership=inv['entries'],sensitiveReview={'findings':scan.blocked,'annotations':scan.annotations,'decodedOriginals':scan.decoded,'decodedBytes':scan.decoded_total,'scope':'Bounded eligible text/JSON/PowerShell decoding only; paths/types, never discovered secret values/hashes. Arbitrary encodings cannot be proven secret-free.'})
   need(not scan.blocked,'sensitive/type blockers in round'+n+'; see only location/type in independent report')
  if a.write_archive:
   need(a.all_consumers_finished and a.final_proof is not None,'explicit all-consumers-finished + final-proof required');authority=final_proof(a.final_proof)
   for n in selected:
    destination=MAIN/'开发计划/验证记录'/BINDINGS[n]['destination'];need(not destination.exists(),'archive destination already exists; no overwrite')
   for n in selected:
    per[n]['archive']=seal_round(n,BINDINGS[n],inventories[n],authority,report_paths[selected.index(n)+1],per[n]);per[n]['status']='PASS_EXACT_NONBINARY_ARCHIVE'
   for n in selected:stable(Path(BINDINGS[n]['root']),inventories[n]['rows'],inventories[n]['entries'])
   # Explicit final proofs are re-hashed after all copying as well.
   need(final_proof(a.final_proof)==authority,'final authority/proof changed while copying');summary.update(status='PASS_EXACT_NONBINARY_ARCHIVES',archivePerformed=True,allConsumers='EXPLICITLY_CONFIRMED_FINISHED',finalAuthority=authority)
  else:summary.update(status='PREPARED_ONLY_NOT_SEALED',allConsumers='NOT_ASSERTED_BY_PREPARATION',cleanupForbiddenUntilParentConfirmation=True)
 except Exception as error:summary['error']={'type':type(error).__name__,'message':str(error)[:1000]}
 finally:
  for v in per.values():
   if v['destinationCreated'] and v['status']!='PASS_EXACT_NONBINARY_ARCHIVE':v['status']='FAIL_PARTIAL_ARCHIVE_RETAINED'
  summary['mainModified']=any(v['destinationCreated'] for v in per.values());summary['partialArchiveMayRemain']=summary['mainModified'] and not summary['archivePerformed'];summary['rounds']=list(per.values());summary['helperUnchanged']=sha(Path(__file__).read_bytes())==summary['helper']['sha256']
  for n,p in zip(selected,report_paths[1:]):
   if p in handles:json.dump(per[n],handles[p],ensure_ascii=False,indent=2);handles[p].write('\n')
  if a.report in handles:json.dump(summary,handles[a.report],ensure_ascii=False,indent=2);handles[a.report].write('\n')
  for f in handles.values():f.close()
 print(json.dumps({'status':summary['status'],'report':str(a.report),'sha256':sha(a.report.read_bytes()),'rounds':selected,'mainModified':summary['mainModified'],'partialArchiveMayRemain':summary['partialArchiveMayRemain'],'error':summary.get('error')},ensure_ascii=False));return 0 if summary['status'].startswith(('PASS','PREPARED')) else 1
if __name__=='__main__':sys.exit(main())
