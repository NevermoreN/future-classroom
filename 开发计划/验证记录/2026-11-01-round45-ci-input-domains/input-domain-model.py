#!/usr/bin/env python3
"""R45 Python model + actual read-only Git/checkout inventory; not PowerShell."""
import hashlib, json, re, subprocess, pathlib, shutil, tempfile, os
EV = pathlib.Path('/tmp/fc-round45-ci-input-domains-evidence')
ROOT = pathlib.Path('/tmp/fc-round45-ci-input-domains-worktree')
BASE = 'e0d97689d6848d971bb9f751cc5f014c6a98882b'
PREFIX = '开发计划/验证记录/'
LIMIT = 16*1024*1024
MIB = 1024*1024
checks=[]
def run(args):
    p=subprocess.run(args,cwd=ROOT,capture_output=True,timeout=120)
    if p.returncode: raise RuntimeError('local Git command failed')
    return p.stdout

def domain(path):
    if not path or path.startswith('/') or re.search(r'[\\:\x00-\x1f]',path) or any(x in ('','.','..') for x in path.split('/')):
        raise ValueError('path')
    return 'historical_git_archive' if path.startswith(PREFIX) else 'sha256_input'

def parse(raw):
    if not raw or len(raw)>LIMIT or not raw.endswith(b'\0'): raise ValueError('bounded metadata')
    records=raw[:-1].decode('utf8').split('\0')
    if len(records)>10000: raise ValueError('count')
    result=[]; names=set()
    for record in records:
        header,path=record.split('\t',1)
        match=re.fullmatch(r'(100644|100755) blob ([0-9a-f]{40}) +([0-9]+)',header)
        if not match: raise ValueError('metadata')
        mode,oid,size=match.groups(); size=int(size)
        if size>64*MIB: raise ValueError('blob size')
        if path.casefold() in names: raise ValueError('case duplicate')
        names.add(path.casefold())
        result.append(dict(path=path,mode=mode,type='blob',git_blob_oid_sha1=oid,git_blob_bytes=size,domain=domain(path)))
    return result

def record(path='src/lib.rs',size=1,mode='100644',kind='blob',oid='a'*40):
    return f'{mode} {kind} {oid} {size}\t{path}\0'.encode()

def quotas(entries):
    if len(entries)>10000: raise ValueError('count')
    total=0
    for path,size,reparse in entries:
        if size>64*MIB or reparse: raise ValueError('file')
        if domain(path)=='sha256_input': total+=size
        if total>256*MIB: raise ValueError('total')
    return total

def reference(path,text):
    name=path.rsplit('/',1)[-1]
    if path.lower().endswith('.rs') or name.lower() in ('cargo.toml','rust-toolchain.toml') or path.startswith('.cargo/'):
        if PREFIX in re.sub('/+','/',text.replace('\\','/')): raise ValueError('explicit reference')

def ok(name,fn):
    fn(); checks.append(dict(name=name,passed=True))

def rejects(name,fn):
    try: fn()
    except (ValueError,UnicodeError): checks.append(dict(name=name,passed=True)); return
    raise AssertionError(name)

def equals(a,b):
    if a!=b: raise AssertionError((a,b))

ok('exact historical prefix',lambda:equals(domain(PREFIX+'old.rs'),'historical_git_archive'))
for path in ['开发计划/验证记录-旧/old.rs','开发计划/验证记录','开发计划/验证记录x/a','a/'+PREFIX+'x','Development/a.rs']:
    ok('near prefix stays SHA256 '+path,lambda p=path:equals(domain(p),'sha256_input'))
for path in ['','/x','C:/x','../x','a/../x','a//x','a/./x','a\\x','a\0x','a\tx']:
    rejects('noncanonical path '+repr(path),lambda p=path:domain(p))
ok('regular metadata retains blob size distinct from checkout',lambda:equals(parse(record(PREFIX+'x',13))[0]['git_blob_bytes'],13))
for name,raw in [('missing final NUL',record()[:-1]),('symlink',record(mode='120000')),('gitlink',record(mode='160000',kind='commit')),('bad oid',record(oid='z'*40)),('oversize blob',record(size=64*MIB+1)),('case collision',record('A')+record('a')),('duplicate',record()+record()),('10001 records',b''.join(record('f'+str(i)) for i in range(10001))),('metadata 16MiB overflow',b'x'*(LIMIT+1))]:
    rejects(name,lambda r=raw:parse(r))
ok('exact 256MiB SHA256 input allowed',lambda:equals(quotas([(f'f{i}',64*MIB,False) for i in range(4)]),256*MIB))
rejects('256MiB+1 actual input rejected',lambda:quotas([(f'f{i}',64*MIB,False) for i in range(4)]+[('f4',1,False)]))
ok('historical aggregate isolated while per-item bound retained',lambda:equals(quotas([(PREFIX+str(i),64*MIB,False) for i in range(5)]),0))
rejects('near historical prefix does not escape actual byte limit',lambda:quotas([('开发计划/验证记录-旧/'+str(i),64*MIB,False) for i in range(5)]))
rejects('historical individual oversize rejected',lambda:quotas([(PREFIX+'a',64*MIB+1,False)]))
rejects('historical reparse rejected',lambda:quotas([(PREFIX+'a',1,True)]))
for label,path,text in [('Rust forward','src/x.rs','include_bytes!("../../'+PREFIX+'x")'),('Rust escaped backslash','build.rs','"开发计划\\\\验证记录\\\\x"'),('Cargo build path','Cargo.toml','build="开发计划/验证记录/x.rs"'),('Cargo config','.cargo/config.toml','开发计划\\验证记录\\x')]:
    rejects('explicit reference '+label,lambda p=path,t=text:reference(p,t))
ok('normal include unaffected',lambda:reference('src/x.rs','include_bytes!("../tests/fixtures/minimal.docx")'))
ok('ordinary prose may discuss history',lambda:reference('README.md',PREFIX))

# Actual e0 HEAD object metadata, no checkout/hash of historical payloads here.
raw=run(['git','ls-tree','-r','-z','-l','--full-tree',BASE])
(EV/'head-tree-original.bin').write_bytes(raw)
entries=parse(raw)
normal=[e for e in entries if e['domain']=='sha256_input']; history=[e for e in entries if e['domain']=='historical_git_archive']
actual=[]; archive_metadata=[]; changed=[]
for e in entries:
    p=ROOT/e['path']; st=p.lstat()
    if not p.is_file() or p.is_symlink() or st.st_size>64*MIB: raise AssertionError('actual file/reparse guard')
    if e['domain']=='historical_git_archive': archive_metadata.append({'path':e['path'],'checkout_bytes':st.st_size}); continue
    data=p.read_bytes(); reference(e['path'],data.decode('utf8','replace'))
    actual.append({'path':e['path'],'bytes':len(data),'sha256':hashlib.sha256(data).hexdigest()})
actual_total=sum(e['bytes'] for e in actual)
if actual_total>256*MIB: raise AssertionError('actual normal total')
(EV/'head-git-tree-model.json').write_text(json.dumps(entries,ensure_ascii=False,indent=2)+'\n')
(EV/'checkout-sha256-model.json').write_text(json.dumps(actual,ensure_ascii=False,indent=2)+'\n')
(EV/'checkout-history-metadata-model.json').write_text(json.dumps(archive_metadata,ensure_ascii=False,indent=2)+'\n')

# Real Git cleanliness / same-length mutation / untracked path tests, using only
# two existing blobs in an owned sparse shared clone; no new commit or network.
fixture=EV/'git-clean-fixture'
if fixture.exists(): raise RuntimeError('fixture already exists')
fixture.mkdir(); (fixture/'.owner').write_text('r45-owned-git-clean-fixture/v1\n')
repo=fixture/'repo'
commands=[]
def git(args,**kw):
    argv=['git','-C',str(repo),*args]; p=subprocess.run(argv,capture_output=True,timeout=120,**kw)
    commands.append(dict(argv=argv,exit=p.returncode,stdout=p.stdout.decode('utf8','replace'),stderr=p.stderr.decode('utf8','replace')))
    if p.returncode: raise RuntimeError('owned fixture git failed')
    return p.stdout
try:
    p=subprocess.run(['git','clone','--shared','--no-checkout',str(ROOT),str(repo)],capture_output=True,timeout=120)
    commands.append(dict(argv=['git','clone','--shared','--no-checkout',str(ROOT),str(repo)],exit=p.returncode,stdout=p.stdout.decode(),stderr=p.stderr.decode()))
    if p.returncode: raise RuntimeError('owned clone failed')
    normal_path='Cargo.toml'; archive_path=next(e['path'] for e in history if e['git_blob_bytes']>0 and '\n' not in e['path'])
    git(['sparse-checkout','set','--no-cone','--', '/'+normal_path,'/'+archive_path])
    git(['checkout','--detach',BASE])
    equals(git(['status','--porcelain','--untracked-files=all']),b'')
    checks.append(dict(name='actual owned sparse Git checkout clean',passed=True))
    for path,label in [(normal_path,'actual ordinary same-size content mutation dirty'),(archive_path,'actual history same-size mutation dirty')]:
        target=repo/path; data=target.read_bytes(); altered=bytes([data[0]^1])+data[1:]; target.write_bytes(altered)
        if not git(['status','--porcelain','--untracked-files=all']): raise AssertionError('dirty content missed')
        checks.append(dict(name=label,passed=True)); target.write_bytes(data)
        equals(git(['status','--porcelain','--untracked-files=all']),b'')
    stray=repo/'not-tracked.txt'; stray.write_text('only own test')
    if not git(['status','--porcelain','--untracked-files=all']): raise AssertionError('untracked missed')
    checks.append(dict(name='actual untracked file dirty',passed=True)); stray.unlink()
    equals(git(['status','--porcelain','--untracked-files=all']),b'')
    (EV/'git-fixture-commands.json').write_text(json.dumps(commands,ensure_ascii=False,indent=2)+'\n')
finally:
    if (fixture/'.owner').read_text()!='r45-owned-git-clean-fixture/v1\n': raise RuntimeError('fixture owner changed')
    shutil.rmtree(fixture)

report=dict(status='PASS_PYTHON_MODEL_AND_LOCAL_GIT_ONLY',powershell='NOT_RUN_NO_LOCAL_PWSH',source_sha256=hashlib.sha256((ROOT/'scripts/windows-msvc-ci.ps1').read_bytes()).hexdigest(),base=BASE,checks=checks,
    actual_head=dict(tracked=len(entries),historical=len(history),normal=len(normal),git_blob_bytes=sum(e['git_blob_bytes'] for e in entries),historical_git_blob_bytes=sum(e['git_blob_bytes'] for e in history),normal_git_blob_bytes=sum(e['git_blob_bytes'] for e in normal),metadata_bytes=len(raw),metadata_sha256=hashlib.sha256(raw).hexdigest()),
    current_checkout=dict(sha256_files=len(actual),sha256_bytes=actual_total,historical_metadata_files=len(archive_metadata),historical_metadata_bytes=sum(e['checkout_bytes'] for e in archive_metadata),dirty_expected=['scripts/windows-msvc-ci.ps1'],note='Current PS1 intentionally differs from base; this is not a clean CI run. SHA256 hashes only ordinary current checkout files, never historical payloads.'),
    cleanup=dict(owned_sparse_fixture_removed=not fixture.exists()),limits=dict(paths=10000,per_file_bytes=64*MIB,sha256_total_bytes=256*MIB,metadata_output_bytes=LIMIT),scope='Python model is not PowerShell execution. Local actual Git mutation tests prove porcelain catches owned changes, not execution of CI wrapper. Explicit path scan is a conservative policy, not arbitrary build.rs IO isolation.')
with (EV/'input-domain-model-first.json').open('x') as f: json.dump(report,f,ensure_ascii=False,indent=2); f.write('\n')
print(json.dumps({'status':report['status'],'checks':len(checks),'tracked':len(entries),'normal':len(normal),'history':len(history),'sha256_bytes':actual_total,'cleanup':report['cleanup']},ensure_ascii=False))
