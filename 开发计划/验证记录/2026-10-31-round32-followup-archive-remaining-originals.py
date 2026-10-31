from pathlib import Path
import hashlib,json,re,subprocess

repo=Path.cwd();dest=repo/'开发计划/验证记录';base=Path('/tmp/fc-round30-evidence');index=dest/'2026-10-31-round30-late-originals-index.json';assert not index.exists()
def sha(data):return hashlib.sha256(data).hexdigest()
archived={}
prior=[]
for label,expected in [('round30','7ad3e92fa452e00b3060a1eb1149934d84625abef0657372bde008edab600dfa'),('round31','10bbb497944bbd2a509622261d4e29f9ef5ed4240a21ec2bfb7bcf71b665b25e'),('round32','891087fb677173b8cfec9b0c43961ba28f6ba6ac5b95f8e9e43f24726e4385e7')]:
    p=dest/f'2026-10-31-{label}-stage-progress-index.json';assert sha(p.read_bytes())==expected;prior.append({'path':p.name,'sha256':expected})
    rows=json.loads(p.read_bytes())['files']+[{'path':p.name,'bytes':p.stat().st_size,'sha256':expected}]
    for row in rows:
        data=(dest/row['path']).read_bytes();assert len(data)==row['bytes'] and sha(data)==row['sha256'];archived[(len(data),sha(data))]=row['path']
credential=subprocess.run(['git','credential','fill'],input=b'protocol=https\nhost=github.com\n\n',capture_output=True,timeout=15)
values=dict(line.split(b'=',1) for line in credential.stdout.splitlines() if b'=' in line);token=values.get(b'password',b'')
rows=[];existing=[];caches=[]
for p in sorted(base.rglob('*')):
    if p.is_dir():continue
    assert p.is_file() and not p.is_symlink();data=p.read_bytes();relative=p.relative_to(base).as_posix();key=(len(data),sha(data))
    if '__pycache__' in p.parts:
        assert p.suffix=='.pyc';caches.append({'source':str(p),'bytes':len(data),'sha256':sha(data),'scope':'Disposable Python cache, not original source/evidence'});continue
    if key in archived:
        existing.append({'source':str(p),'archived':archived[key],'bytes':key[0],'sha256':key[1]});continue
    assert len(data)<=16*1024*1024 and p.suffix.lower() not in ['.exe','.dll','.zip','.pdb','.obj','.lib','.a']
    assert not data.startswith((b'MZ',b'PK\x03\x04',b'\x7fELF'))
    assert not re.search(rb'-----BEGIN (?:OPENSSH |RSA |EC )?PRIVATE KEY-----\r?\n[A-Za-z0-9+/=\r\n]+-----END (?:OPENSSH |RSA |EC )?PRIVATE KEY-----',data)
    assert not token or token not in data
    assert not re.search(rb'https://[^\s"<>]*[?&](?:sig|signature|X-Amz-Signature)=',data,re.I)
    name='2026-10-31-round30-late-originals/'+relative;q=dest/name;assert not q.exists();q.parent.mkdir(parents=True,exist_ok=True);q.write_bytes(data);assert q.read_bytes()==data
    rows.append({'source':str(p),'path':name,'bytes':len(data),'sha256':sha(data)})
followup=[]
for source,name in [(Path('/mnt/data/FutureClassroom-round32-msvc-dispatch.json'),'2026-10-31-round32-followup-msvc-dispatch.json'),(Path('/tmp/fc-round32-msvc-dispatch-once.py'),'2026-10-31-round32-followup-msvc-dispatch-once.py'),(Path('/mnt/data/FutureClassroom-round32-msvc-jobs-initial.json'),'2026-10-31-round32-followup-msvc-jobs-initial.json'),(Path(__file__),'2026-10-31-round32-followup-archive-remaining-originals.py')]:
    data=source.read_bytes();assert not token or token not in data;q=dest/name;assert not q.exists();q.write_bytes(data);assert q.read_bytes()==data;followup.append({'source':str(source),'path':name,'bytes':len(data),'sha256':sha(data)})
r={'status':'PRESERVED_ORIGINALS_ONLY_NO_NEW_EXECUTION','scope':'Late direct preservation of prior Round30 original files not individually indexed before. Original compile/test failure and prepared/pending states retain exact bytes; no new functional test or acceptance result. Current source9d42f7c MSVC dispatch and initial job are distinct, still in progress.','files':rows,'alreadyArchivedOldFiles':existing,'disposableCaches':caches,'followupFiles':followup,'priorIndexesUnchanged':prior,'rustSourceCommit':'9d42f7cc75c7d2545caa18366d4a6d047aedaa99','newMsvcRunId':41215512635,'newMsvcJobId':124579085223,'newMsvcConclusion':None,'cleanup':'All exact own evidence roots remain until this independent map is committed and synced; cleanup receipt is a separate final delivery.'}
index.write_text(json.dumps(r,ensure_ascii=False,indent=2)+'\n')
print(json.dumps({'lateOriginalFiles':len(rows),'lateOriginalBytes':sum(x['bytes'] for x in rows),'alreadyArchivedOldFiles':len(existing),'disposableCaches':len(caches),'followupFiles':len(followup),'indexSha256':sha(index.read_bytes())}),flush=True)
