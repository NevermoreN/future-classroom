#!/usr/bin/env python3
"""Prepare, or explicitly seal after parent confirmation, exact nonbinary R46 evidence."""
from pathlib import Path
import argparse, base64, binascii, datetime, hashlib, json, os, re, stat, sys, urllib.parse

E = Path('/tmp/fc-round46-ci-evidence')
MAIN = Path('/workspace/共享/Eliora/未来教室计划')
DEST = MAIN / '开发计划/验证记录/2026-11-01-round46-msvc'
MAX_FILES = 10000
MAX_FILE = 64 * 1024 * 1024
MAX_TOTAL = 512 * 1024 * 1024
ALLOWED = {'.json','.jsonl','.log','.py','.mjs','.ps1','.rs','.txt','.md','.diff','.patch','.pem','.stdout','.stderr','.raw','.toml','.lock','.yml','.yaml','.bin'}
BIN_EXT = {'.exe','.zip','.dll','.pdb','.pyc','.rlib','.rmeta','.obj','.o','.a','.so','.lib'}
SECRET_PATH = ('/config/.ssh/', '/config/.local/share/future-classroom-test-access/', '/root/.ssh/', '/home/')
PATTERNS = [
 ('PRIVATE_KEY', re.compile(r'-----BEGIN(?: [A-Z0-9]+)? PRIVATE KEY-----')),
 ('GITHUB_TOKEN', re.compile(r'\b(?:gh[pousr]_[A-Za-z0-9]{24,}|github_pat_[A-Za-z0-9_]{30,})\b')),
 ('API_TOKEN', re.compile(r'\bsk-(?:proj-)?[A-Za-z0-9_-]{30,}\b')),
 ('AUTHORIZATION_SECRET', re.compile(r'(?i)\bAuthorization[\x22\x27]?\s*[:=]\s*[\x22\x27]?(?:Bearer|Basic)\s+[A-Za-z0-9+/=_-]{20,}')),
 ('SESSION_COOKIE', re.compile(r'__Host-FutureClassroom=([0-9a-fA-F]{64})')),
 ('CAPABILITY_OR_CSRF', re.compile(r'(?i)(?:stopCapability|expectedCsrf|csrf)\s*[=:]\s*[\x22\x27]([0-9a-f]{64})[\x22\x27]')),
]
PS64 = re.compile(r'''(?:FromBase64String\(\s*["']|[-]EncodedCommand\s+)([A-Za-z0-9+/]{32,}={0,2})''', re.I)

def need(ok, why):
    if not ok: raise ValueError(why)

def sha(data): return hashlib.sha256(data).hexdigest()
def load(p): return json.loads(Path(p).read_text(encoding='utf-8-sig'))
def fact(p):
    p=Path(p); need(p.is_file() and not p.is_symlink(), 'missing ordinary evidence file: '+p.name)
    raw=p.read_bytes(); return {'path':str(p),'bytes':len(raw),'sha256':sha(raw)}

def sensitive_path(p):
    return str(p).startswith(SECRET_PATH) or p.name.startswith(('id_ed25519','id_rsa')) or p.name.endswith('.control.json') or p.suffix.lower() in {'.key','.pfx','.p12'}

def stable(rows):
    for row in rows:
        p=E/row['path']; need(p.is_file() and not p.is_symlink(), 'candidate disappeared: '+row['path'])
        b=p.read_bytes(); need(len(b)==row['bytes'] and sha(b)==row['sha256'], 'candidate changed: '+row['path'])

def expired_stop(rel):
    if '.control.json.stop-' not in rel: return False
    owner=E/rel.split('.control.json.stop-',1)[0]
    if not owner.is_file() or owner.is_symlink(): return False
    x=load(owner); n=x.get('nativeOutcome',{}); c=x.get('cleanupDetails',{})
    return x.get('cleanup')=='PASS' and all(n.get(k) is True for k in ['actualJobProcessesZero','originalPrimaryExited','originalHandlesClosed']) and c.get('rootAbsent') is True

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
        try:self.walk(json.loads(text),origin,depth+1,retired,synthetic)
        except ValueError:pass
    def walk(self,x,origin,depth=0,retired=False,synthetic=False):
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
                    if 'base64' in low or low=='encodedcommand':self.decode(v,where,0,retired,synthetic)
                elif isinstance(v,(dict,list)):self.walk(v,where,depth+1,retired,synthetic)
        elif isinstance(x,list):
            for i,v in enumerate(x):
                if isinstance(v,(dict,list)):self.walk(v,origin+'/'+str(i),depth+1,retired,synthetic)

def inventory(report_path):
    scan=Scanner();rows=[];excluded=[];initial=[];total=0
    paths=sorted(E.rglob('*'))
    need(len(paths)<=MAX_FILES*3,'evidence path enumeration bound')
    for p in paths:
        rel=p.relative_to(E).as_posix(); info=p.lstat()
        if stat.S_ISDIR(info.st_mode): continue
        initial.append(rel)
        if p==report_path:continue
        if stat.S_ISLNK(info.st_mode): scan.finding('SYMLINK_NEVER_FOLLOWED',rel);continue
        need(stat.S_ISREG(info.st_mode),'non-regular evidence type: '+rel)
        if sensitive_path(p):
            excluded.append({'path':rel,'reason':'PRIVATE_OR_LIVE_CONTROL_FILE_CONTENT_NEVER_READ_OR_HASHED'});continue
        if p.suffix.lower() in BIN_EXT or '__pycache__' in p.parts or 'programs' in p.relative_to(E).parts or 'target' in p.relative_to(E).parts:
            excluded.append({'path':rel,'bytes':info.st_size,'reason':'BINARY_OR_COMPILATION_CACHE_CONTENT_NOT_READ_OR_HASHED_BY_ARCHIVER'});continue
        if p.suffix.lower() not in ALLOWED and p.name not in {'.owner','LICENSE'}:
            scan.finding('UNKNOWN_FILE_TYPE_REQUIRES_REVIEW',rel);continue
        need(info.st_size<=MAX_FILE,'single evidence exceeds64MiB: '+rel)
        raw=p.read_bytes();total+=len(raw);need(total<=MAX_TOTAL,'nonbinary evidence exceeds512MiB')
        if raw.startswith(b'MZ') or raw.startswith(b'PK\x03\x04'):scan.finding('INLINE_BINARY_FILE_MAGIC',rel);continue
        try:
            encoding='utf-16' if raw.startswith((b'\xff\xfe',b'\xfe\xff')) else 'utf-8-sig';text=raw.decode(encoding)
        except UnicodeError:scan.finding('UNCLASSIFIED_NON_TEXT_ORIGINAL',rel);continue
        retired=expired_stop(rel); synthetic=False; obj=None
        if p.suffix=='.json':
            try:obj=json.loads(text)
            except ValueError:scan.finding('INVALID_JSON',rel)
            if isinstance(obj,dict):
                cleanup=obj.get('cleanupDetails',{})
                synthetic=(str(obj.get('credentialScope','')).startswith('Disposable synthetic probe account only') and obj.get('cleanup')=='PASS' and cleanup.get('rootAbsent') is True and cleanup.get('ownedProcessesRemaining')==0)
        scan.text(text,rel,retired=retired,synthetic=synthetic)
        if obj is not None:scan.walk(obj,rel,retired=retired,synthetic=synthetic)
        elif p.suffix=='.jsonl':
            for n,line in enumerate(text.splitlines(),1):
                if line.strip():
                    try:scan.walk(json.loads(line),rel+':'+str(n),retired=retired,synthetic=synthetic)
                    except ValueError:scan.finding('INVALID_JSONL',rel+':'+str(n))
        rows.append({'path':rel,'bytes':len(raw),'sha256':sha(raw),'encoding':encoding,'kind':'owner-marker' if rel=='.owner' else 'original-objdump' if '.objdump.' in rel else 'original-raw' if '/raw/' in rel or '-raw/' in rel else 'source-helper-or-report'})
    need(len(rows)+len(excluded)<=MAX_FILES,'file count exceeds10000')
    stable(rows)
    current=sorted(p.relative_to(E).as_posix() for p in E.rglob('*') if p.is_file() and p!=report_path)
    expected=sorted(v for v in initial if E/v!=report_path)
    need(current==expected,'evidence membership changed during scan')
    return rows,excluded,scan

def gates(browser,readonly):
    result=[]
    def passed(p):
        x=load(p);need(x.get('status')=='PASS' and x.get('cleanup')=='PASS','final flow not PASS+cleanup: '+p.name);result.append(fact(p));return x
    independent=load(E/'runtime-independent-first.json')
    need(independent['status']=='PASS_BOTH_ORIGINAL_ARTIFACTS_AND_ACTUAL_WIN11_RUNTIME','independent runtime audit not PASS')
    result.append(fact(E/'runtime-independent-first.json'))
    for tag in ['modern','win7']:
        audit=load(E/f'{tag}-original-independent-first.json'); normal=audit['normalExecutable']['producer']['sha256']
        for kind,status,steps in [('public','publicDesktopNativeStatus',20),('visual','subjectVisualNativeStatus',22)]:
            p=E/f'{tag}-msvc-{kind}-native-first.json';x=passed(p)
            need(x['exeSha256']==normal and x.get(status)=='PASS' and x.get(status.replace('Status','Steps'))==steps,'native exact steps or original EXE mismatch')
    first=load(E/'modern-msvc-default-browser-continuous-first.json')
    need(first.get('status')=='FAIL' and first.get('cleanup')=='PASS','original browser firstFAIL not retained')
    result.append(fact(E/'modern-msvc-default-browser-continuous-first.json'))
    need(browser is not None and readonly is not None,'explicit final browser+readonly report paths required')
    for p in (browser,readonly):need(p.is_relative_to(E) and not p.is_symlink(),'final proof must be original inside R46 evidence')
    b=passed(browser); details=b.get('cleanupDetails',{})
    need(b['exeSha256']==load(E/'modern-original-independent-first.json')['normalExecutable']['producer']['sha256'],'browser original modern EXE mismatch')
    need(b.get('strictP0')=='NOT_VALIDATED' and b.get('productionListener')=='NOT_ENABLED' and b.get('win7')=='NOT_VALIDATED' and b.get('win10')=='NOT_VALIDATED','finite flow upgraded formal gate')
    need(details.get('rootAbsent') is True and details.get('ownedProcessesRemaining')==0 and details.get('cngKeyExists') is False and details.get('myDigestRestored') is True,'native/cert/key/root cleanup not proven')
    ob=b.get('browser',{});need(ob.get('status')=='PASS' and ob.get('cleanup')=='PASS' and ob.get('nodeActualExitCode')==0,'browser itself not actualPASS+cleanup')
    owner=load(Path(ob['ownerReportPath']));n=owner.get('nativeOutcome',{})
    need(owner.get('cleanup')=='PASS' and all(n.get(k) is True for k in ['actualJobProcessesZero','originalPrimaryExited','originalHandlesClosed']) and owner.get('cleanupDetails',{}).get('rootAbsent') is True,'original browser Job not actually drained')
    node=load(Path(ob['nodeReportPath']));need(node.get('status')=='PASS' and node.get('contextDisposed') is True,'browser node context not terminal')
    result.extend([fact(Path(ob['ownerReportPath'])),fact(Path(ob['nodeReportPath']))])
    read=load(readonly)
    need(read.get('status')=='PASS' and read.get('integrityStatus')=='PASS' and read.get('publicAuthorizationUnchanged') is True and read.get('projectRunUnchanged') is True and read.get('sshd')=='Running' and read.get('ownedProcesses')==[] and read.get('ownedTasks')==[] and read.get('scratchDirectories')==[] and read.get('activityObserved') is False,'final readonly cleanup/environment proof incomplete')
    result.append(fact(readonly));return result

def archive(rows,proof,report):
    need(not DEST.exists(),'archive destination already exists; overwrite forbidden')
    for parent in [MAIN,MAIN/'开发计划',MAIN/'开发计划/验证记录']:need(parent.is_dir() and not parent.is_symlink(),'archive parent invalid')
    stable(rows); DEST.mkdir()
    copied=[]
    for row in rows:
        src=E/row['path'];data=src.read_bytes();need(len(data)==row['bytes'] and sha(data)==row['sha256'],'source changed before copy: '+row['path'])
        dest=DEST/row['path'];dest.parent.mkdir(parents=True,exist_ok=True)
        with dest.open('xb') as f:f.write(data)
        need(dest.read_bytes()==data,'archive copy mismatch: '+row['path']);copied.append(row)
    stable(rows)
    index={'schema':'fc.r46.exact-nonbinary-archive/v1','status':'EXACT_BYTES_COPIED_ALL_CONSUMERS_EXPLICITLY_FINISHED','sourceRoot':str(E),'sourceOwner':fact(E/'.owner'),'files':copied,'fileCount':len(copied),'bytes':sum(v['bytes'] for v in copied),'fileManifestSha256':sha(json.dumps(copied,sort_keys=True,separators=(',',':')).encode()),'finalProofs':proof,'firstFailures':'Original default60s runtime timeout and first browser Network.loadingFailed remain unchanged; no success overwrites.','generatedIndexExcludedFromOwnLedger':True,'independentArchiveReport':str(report),'excluded': 'ZIP/EXE/compilation cache and live control/private files explicitly excluded in independent report. No private environment key read or hashed.','runtimeScope':'Modern and Win7-target each21EXE/1420 tests onWin11 only; public20/visual22 and finite native/Edge synthetic proof separate. ActualWin7/Win10/visibleSeewo/72h and P0 strictTCP+SSPI not upgraded.','cleanup':'Archiver performs no process, Windows, source-root, target or credential cleanup. Parent coordinates final owned-root removal after commit proof.'}
    (DEST/'INDEX.json').write_text(json.dumps(index,ensure_ascii=False,indent=2)+'\n')
    (DEST/'INDEX.md').write_text('# R46 正式 MSVC 产物与 Win11 补充验证原件\n\n本目录逐字节保留非二进制原件，完整文件清单、字节数与 SHA256 见 INDEX.json。\n\n现代与 Win7 目标产物各 21 个测试 EXE、1420 项在 Win11 通过；合法零测试 example 未省略。首次默认 60 秒超时及首次浏览器失败保留，后续明确终态另列。\n\n本机公共课堂流程、配色流程与有限 Native TLS/隔离 Edge 证据分别记录，不代表真实 Windows 7/10、希沃可见验收或生产监听/P0 门槛已通过。EXE、ZIP、缓存及私钥不入库。\n\n归档不会清理任何活动根；R51 等消费者全部终态由主协调者显式确认后才允许本次封存。最终 owned-root 清理由主协调者在提交字节证明后另行执行。\n',encoding='utf-8')
    return {'destination':str(DEST),'copiedFiles':len(copied),'copiedBytes':index['bytes'],'indexJson':fact(DEST/'INDEX.json'),'indexMarkdown':fact(DEST/'INDEX.md'),'allCopiedBytesMatch':True}

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--report',required=True,type=Path);p.add_argument('--write-archive',action='store_true');p.add_argument('--all-consumers-finished',action='store_true');p.add_argument('--final-browser-report',type=Path);p.add_argument('--final-readonly-report',type=Path);a=p.parse_args()
    need(not a.report.exists(),'fresh report required')
    report={'status':'FAIL','createdUtc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'helper':fact(Path(__file__)),'mainModified':False,'archivePerformed':False,'scope':'Local-only evidence scan/copy; no SSH/Windows/Cargo/Git/network or cleanup operations.'}
    try:
        rows,excluded,scan=inventory(a.report)
        report.update(files=rows,fileCount=len(rows),bytes=sum(v['bytes'] for v in rows),excluded=excluded,sensitiveReview={'findings':scan.blocked,'annotations':scan.annotations,'decodedOriginals':scan.decoded,'decodedBytes':scan.decoded_total,'scope':'All eligible text + bounded JSON/PowerShell base64 recursively scanned. Findings reveal only location/type, never matched secret value/hash. No reference paths followed. Pattern review cannot prove arbitrary encodings secret-free.'})
        need(not scan.blocked,'sensitive/type review has blockers; see path/type only')
        if a.write_archive:
            need(a.all_consumers_finished,'R51/other active consumers require parent explicit all-finished confirmation')
            need(not a.report.is_relative_to(E) and not a.report.is_relative_to(DEST),'independent final archive report must be outside source/destination to avoid recursive self-inventory')
            proof=gates(a.final_browser_report,a.final_readonly_report)
            report['archive']=archive(rows,proof,a.report);report.update(status='PASS_EXACT_NONBINARY_ARCHIVE',mainModified=True,archivePerformed=True,activeConsumers='PARENT_EXPLICITLY_CONFIRMED_ALL_FINISHED')
        else:report.update(status='PREPARED_ONLY_NOT_SEALED',activeConsumers='R51_DIAGNOSTIC_IS_ACTIVE_OR_NOT_YET_PARENT_CONFIRMED_FINISHED',cleanupForbiddenUntilParentConfirmation=True)
    except Exception as err:report['error']={'type':type(err).__name__,'message':str(err)[:1000]}
    report['helperUnchanged']=sha(Path(__file__).read_bytes())==report['helper']['sha256']
    with a.report.open('x') as f:json.dump(report,f,ensure_ascii=False,indent=2);f.write('\n')
    print(json.dumps({'status':report['status'],'report':str(a.report),'reportSha256':sha(a.report.read_bytes()),'files':report.get('fileCount'),'sensitiveKinds':sorted({x['kind'] for x in report.get('sensitiveReview',{}).get('findings',[])})},ensure_ascii=False))
    return 0 if report['status'] in ('PREPARED_ONLY_NOT_SEALED','PASS_EXACT_NONBINARY_ARCHIVE') else 1

if __name__=='__main__':sys.exit(main())
