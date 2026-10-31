from pathlib import Path, PurePosixPath
import hashlib,json,re,zipfile,zlib,datetime
EV=Path('/tmp/fc-round42-ci-evidence');BASE=EV/'win7-artifact-raw';ZIP=Path('/tmp/fc-round42-win7-original.zip');OUT=EV/'win7-failure-diagnosis-first.json'
SOURCE='bc2a86fb4413dcb3e8acf6b0d4bb2dfe601c22dc'
sha=lambda b:hashlib.sha256(b).hexdigest()
load=lambda b:json.loads(b.decode('utf-8-sig'))
assert not BASE.exists() and not OUT.exists()
BASE.mkdir()
r={'status':'RUNNING','scope':'Exact failed R42 Win7 CI original ZIP/ledger/raw logs and observed failed-command diagnostics only; no executable or remote run, no inference from prior CI failure.', 'sourceSha':SOURCE,'runId':41244342769,'jobId':124664537017,'artifactId':13058948360,'members':[]}
try:
 raw=ZIP.read_bytes();assert len(raw)==41487448 and sha(raw)=='35ce3793f54327a948113f891f3dfda6cbe623af1459412eda212443dab7133b'
 r['zip']={'path':str(ZIP),'bytes':len(raw),'sha256':sha(raw)};del raw
 with zipfile.ZipFile(ZIP) as z:
  infos=z.infolist();names=[x.filename for x in infos];assert len(names)==len(set(names)) and not any(x.is_dir() for x in infos);assert z.testzip() is None
  producer=load(z.read('artifact-sha256.json'));ledger={x['path']:x for x in producer};assert len(ledger)==len(producer) and set(names)==set(ledger)|{'artifact-sha256.json'}
  rows=load(z.read('executables.json'));exes={x['relative_path']:x for x in rows};assert len(exes)==len(rows)==22
  assert sum(x['role']=='tests' for x in rows)==21
  for info in infos:
   name=info.filename;pp=PurePosixPath(name);assert not pp.is_absolute() and '..' not in pp.parts and '\\' not in name and pp.as_posix()==name
   assert ((info.external_attr>>16)&0o170000)!=0o120000
   data=z.read(info);assert len(data)==info.file_size and zlib.crc32(data)&0xffffffff==info.CRC
   digest=sha(data)
   if name in ledger:assert ledger[name]['bytes']==len(data) and ledger[name]['sha256']==digest
   if name not in exes:
    assert not data.startswith(b'MZ');p=BASE.joinpath(*pp.parts);p.parent.mkdir(parents=True,exist_ok=True)
    with p.open('xb') as f:f.write(data)
    assert p.read_bytes()==data
   else:assert exes[name]['bytes']==len(data) and exes[name]['sha256']==digest
   r['members'].append({'path':name,'bytes':len(data),'sha256':digest,'crc32':f'{info.CRC:08x}','producerLedgerMatched':name in ledger,'nonbinaryPreserved':name not in exes})
 result=load((BASE/'result.json').read_bytes());cleanup=load((BASE/'cleanup.json').read_bytes());start=load((BASE/'run-start.json').read_bytes())
 assert result['source_sha']==result['dispatch_source_sha']==SOURCE
 assert all(start[k]==SOURCE for k in ['expected_source_sha','dispatch_source_sha','workflow_sha'])
 assert result['status']=='FAILED_OR_INCOMPLETE' and result['baseline']=='Win7' and result['target']=='i686-win7-windows-msvc'
 assert result['input_manifest_sha256']==sha((BASE/'input-sha256.json').read_bytes())
 commands=result['commands'];assert len(commands)==41
 assert all(c['parent_exited'] and not c['timed_out_or_pipe_stalled'] and not c['log_truncated'] for c in commands)
 failed=[c for c in commands if not c['passed']];assert failed
 assert cleanup['status']=='OWNED_BUILD_ROOTS_REMOVED' and all(x['removed'] for x in cleanup['roots'])
 diagnostics=[]
 for c in failed:
  for kind in ['stdout','stderr']:
   log=c[kind];text=(BASE/log).read_text(encoding='utf-8-sig');lines=text.splitlines()
   for index,line in enumerate(lines):
    if re.match(r'^error(?:\[|:)',line):diagnostics.append({'command':c['name'],'stream':kind,'log':log,'line':index+1,'originalDiagnosticBlock':'\n'.join(lines[index:index+25])})
 r.update(status='PASS_ORIGINAL_VERIFICATION_CI_FAILED',originalCiStatus=result['status'],originalResult=result,originalStart=start,originalCleanup=cleanup,failedCommands=failed,diagnostics=diagnostics,producerLedger=producer,counts={'zipMembers':len(infos),'producerLedgerRows':len(producer),'nonbinaryRawFiles':len(infos)-len(exes),'commands':len(commands),'failedCommands':len(failed),'testExecutables':21,'productExecutables':1},helper={'path':str(Path(__file__)),'sha256':sha(Path(__file__).read_bytes())},runtime='NOT_RUN')
except BaseException as e:r.update(status='FAIL_INDEPENDENT_VERIFICATION',errorKind=type(e).__name__)
with OUT.open('x') as f:f.write(json.dumps(r,ensure_ascii=False,indent=2)+'\n')
print(json.dumps({'status':r['status'],'counts':r.get('counts'),'failedCommands':[{k:x[k] for k in ['name','exit_code','parent_exited']} for x in r.get('failedCommands',[])],'diagnostics':r.get('diagnostics',[]),'reportSha256':sha(OUT.read_bytes())},ensure_ascii=False))
raise SystemExit(0 if r['status']=='PASS_ORIGINAL_VERIFICATION_CI_FAILED' else 1)
