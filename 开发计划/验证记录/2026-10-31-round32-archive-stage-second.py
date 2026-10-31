from pathlib import Path
import json,hashlib,re,subprocess

base=Path('/tmp/fc-round32-deadline-evidence');repo=Path.cwd();dest=repo/'开发计划/验证记录';prefix='2026-10-31-round32-'
summary_path=dest/(prefix+'deadline-stage-summary.json');index_path=dest/(prefix+'stage-progress-index.json')
assert not summary_path.exists() and not index_path.exists()
def digest(data):return hashlib.sha256(data).hexdigest()
def load(name):return json.loads((base/name).read_bytes())
prior=[]
for label,count,expected in [('round30',743,'7ad3e92fa452e00b3060a1eb1149934d84625abef0657372bde008edab600dfa'),('round31',78,'10bbb497944bbd2a509622261d4e29f9ef5ed4240a21ec2bfb7bcf71b665b25e')]:
    p=dest/f'2026-10-31-{label}-stage-progress-index.json';data=p.read_bytes();assert digest(data)==expected
    rows=json.loads(data)['files'];assert len(rows)==count
    for row in rows:
        raw=(dest/row['path']).read_bytes();assert len(raw)==row['bytes'] and digest(raw)==row['sha256']
    prior.append({'path':p.relative_to(repo).as_posix(),'sha256':expected,'files':count,'unchanged':True})
binding=load('final-source-and-runtime-binding.json');runtime=load('windows-platform-tests-first.json');tls=load('windows-tls-business-first.json');cleanup=load('local-build-and-test-cleanup.json');package=load('development-package-first.json')
assert binding['status']=='PASS_AFFECTED_SOURCE_AND_ACTUAL_WIN11_BINDING' and cleanup['status']=='PASS'
assert runtime['status']==runtime['cleanup']==tls['status']==tls['cleanup']=='PASS'
jobs=load('ci-feature-jobs-terminal.json')['value']['structuredContent']['jobs'];assert len(jobs)==1 and jobs[0]['id']==124573173779 and jobs[0]['conclusion']=='success'
tools=load('ci-feature-run-tools.json');log=tools[0]['value']['structuredContent']['content'];artifact=tools[1]['value']['structuredContent']['artifacts'][0]
assert artifact['id']==13047578479 and artifact['workflow_run']['head_sha']=='c7b90fda5ecae688ab6ee70c10cb2dcb4f78cec3'
for name in ['workspace-test-compile','product-compile','clippy','source-clean-after']+[f'pe-imports-{n}' for n in range(22)]:
    assert re.search(re.escape(name)+r': exit=0 passed=True',log),name
assert artifact['digest'].removeprefix('sha256:') in log
# Inspect actual credential bytes only in memory; none are serialized or printed.
credential=subprocess.run(['git','credential','fill'],input=b'protocol=https\nhost=github.com\n\n',capture_output=True,timeout=15)
values=dict(line.split(b'=',1) for line in credential.stdout.splitlines() if b'=' in line);token=values.get(b'password',b'')
files=[];mapping=[]
for source in sorted(base.rglob('*')):
    if source.is_dir():continue
    assert source.is_file() and not source.is_symlink()
    data=source.read_bytes();relative=source.relative_to(base).as_posix()
    assert len(data)<=16*1024*1024
    assert source.suffix.lower() not in ['.exe','.dll','.zip','.pdb','.obj','.lib','.a']
    assert not data.startswith((b'MZ',b'PK\x03\x04',b'\x7fELF'))
    assert not re.search(rb'-----BEGIN (?:OPENSSH |RSA |EC )?PRIVATE KEY-----\r?\n[A-Za-z0-9+/=\r\n]+-----END (?:OPENSSH |RSA |EC )?PRIVATE KEY-----',data)
    assert not token or token not in data
    assert not re.search(rb'https://[^\s"<>]*[?&](?:sig|signature|X-Amz-Signature)=',data,re.I)
    name=prefix+relative;path=dest/name;assert not path.exists();path.parent.mkdir(parents=True,exist_ok=True);path.write_bytes(data);assert path.read_bytes()==data
    row={'path':name,'bytes':len(data),'sha256':digest(data)};files.append(row);mapping.append({'source':str(source),**row})
summary={'status':'PASS_BOUNDED_REQUEST_DEADLINE_SLICE_FULL_GOAL_ACTIVE','scope':'Legal complete HTTP Head immediately starts its original120s request lifetime; admission only tightens and late Head cannot publish. Does not solve strict first-TCP epoch/backlog.','baseline':'1.44/D11','sourceBinding':binding,'runtime':{'actualPlatform':runtime['platform'],'tests':168,'failed':0,'ignored':0,'newTests':binding['newExactTestNames'],'cleanup':runtime['cleanup']},'realSchannelRegression':{'events':len(tls['events']),'native':tls['native'],'business':tls['business'],'cleanupDetails':tls['cleanupDetails'],'browser':'NOT_RUN','normalExeSha256':tls['exeSha256']},'localBuildCleanup':cleanup,'developmentPackage':package['package'],'packageValidation':{'crcAndAllMemberSha256':'PASS','byteIdenticalRepack':package['repackByteIdentical'],'productionRelease':'NOT_ALLOWED'},'priorMsvcFeature':{'runId':41213503607,'jobId':124573173779,'sourceSha':artifact['workflow_run']['head_sha'],'conclusion':'success','workspaceCompileClippy22PeAndSourceAfter':'Original job logs show all exit0 and terminal success; these are compile gates, not runtime.','artifact':artifact,'artifactBytesLocallyDownloaded':False,'artifactCrcAndWholeShaIndependentlyVerified':False,'doesNotValidateRound32Rust':True},'baselineCrtOriginals':load('baseline-selected-verification.json'),'retainedDownloadFailures':{'fullDownload':load('baseline-artifact-download.json'),'selectedReaderStatus':load('baseline-selected-originals.json')['status'],'selectedReaderError':load('baseline-selected-originals.json')['errorKind'],'selectedCompleteMembers':len(load('baseline-selected-originals.json')['members']),'partialBinaryCleanup':load('baseline-partial-artifact-cleanup.json')},'priorIndexes':prior,'sourceMapping':mapping,'originalFiles':len(files),'unpassedGates':['Original strict TCP 5_000_000_000ns Round13 FAIL preserved','Continuous listener backlog and exact ESTABLISHED epoch','New Round32 source formal MSVC compile/runtime','Actual Win7SP1x86 and Win10','Visible Seewo touch/DPI/PPT and physical audible/driver stop','Install/upgrade/uninstall/logon and72h soak'],'normalProductionListener':'DISABLED','goal':'ACTIVE: 按照计划完成全部开发。'}
summary_data=(json.dumps(summary,ensure_ascii=False,indent=2)+'\n').encode();summary_path.write_bytes(summary_data);files.append({'path':summary_path.name,'bytes':len(summary_data),'sha256':digest(summary_data)})
index={'scope':'Round32 verified request-lifetime slice, actual Win11 evidence and prior c7 feature MSVC CI follow-up. Full objective active; previous indexes immutable.','files':files};index_path.write_text(json.dumps(index,ensure_ascii=False,indent=2)+'\n')
for row in files:
    data=(dest/row['path']).read_bytes();assert len(data)==row['bytes'] and digest(data)==row['sha256']
print(json.dumps({'status':summary['status'],'originalFiles':len(mapping),'indexedFiles':len(files),'indexedBytes':sum(x['bytes'] for x in files),'indexSha256':digest(index_path.read_bytes()),'summarySha256':digest(summary_data)},ensure_ascii=False),flush=True)
