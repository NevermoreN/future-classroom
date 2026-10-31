from pathlib import Path
import json,hashlib,re,datetime,base64
repo=Path.cwd();root=Path('/tmp/fc-round34-accept-evidence');dest=repo/'开发计划/验证记录';prefix='2026-11-01-round34-accept';target=dest/prefix
summary_path=dest/'2026-11-01-round34-accept-stage-summary.json';index_path=dest/'2026-11-01-round34-stage-progress-index.json'
assert not any(p.exists() for p in [target,summary_path,index_path])
sha=lambda b:hashlib.sha256(b).hexdigest()
load=lambda n:json.loads((root/n).read_bytes())
binding=load('final-source-and-runtime-binding.json');build=load('windows-affected-first.json');native=load('windows-native-first.json');tls=load('windows-tls-continuous-first.json');after=load('windows-after.json');pkg=load('development-package-first.json');pure=load('win7-ci-powershell-first.json')['result'];ci=load('ci-terminal-observations.json');repro=load('clippy-repro-conclusion-first.json')
assert binding['status']=='PASS_AFFECTED_WINDOWS_SOURCE_AND_RUNTIME_BOUND'
assert build['status']==native['status']==native['cleanup']==tls['status']==tls['cleanup']==after['status']==pure['status']=='PASS'
assert pkg['status']=='PASS_DEVELOPMENT_PACKAGE_ONLY' and pkg['temporaryPackagingRootsRemoved']
cleanup=load('local-build-and-test-cleanup.json');assert cleanup['status']=='PASS' and len(cleanup['removed'])==3 and all(x['removed'] for x in cleanup['removed'])
modern=ci['jobs']['modern']['value']['structuredContent']['jobs'][0];win7=ci['jobs']['win7']['value']['structuredContent']['jobs'][0]
assert modern['status']==win7['status']=='completed' and modern['conclusion']=='success' and win7['conclusion']=='failure'
assert repro['globalPosition']['exitCode']==101 and repro['afterSubcommandPosition']['exitCode']==0
old_indexes={'2026-10-31-round30-stage-progress-index.json':'7ad3e92fa452e00b3060a1eb1149934d84625abef0657372bde008edab600dfa','2026-10-31-round31-stage-progress-index.json':'10bbb497944bbd2a509622261d4e29f9ef5ed4240a21ec2bfb7bcf71b665b25e','2026-10-31-round32-stage-progress-index.json':'891087fb677173b8cfec9b0c43961ba28f6ba6ac5b95f8e9e43f24726e4385e7','2026-10-31-round30-late-originals-index.json':'75e94198db491ec4d89dc4c173ff906078073257b75b8f54a67b49b899c4ccd4','2026-10-31-round33-stage-progress-index.json':'45f9dde0abbae849308d430c50a17f40a74a5904c6ebf66fec0068f4d0d26a08'}
for name,h in old_indexes.items():assert sha((dest/name).read_bytes())==h
private=re.compile(rb'-----BEGIN (?:[A-Z]+ )?PRIVATE KEY-----\s+[A-Za-z0-9+/=\r\n]{32,}-----END (?:[A-Z]+ )?PRIVATE KEY-----')
credentials=re.compile(rb'(?:gh[pousr]_[A-Za-z0-9_]{25,}|github_pat_[A-Za-z0-9_]{30,}|sk-proj-[A-Za-z0-9_-]{25,}|[?&](?:sig|X-Amz-Signature)=[A-Za-z0-9%+/=]{20,})')
def scan(data):
 assert not private.search(data) and not credentials.search(data)
 try:o=json.loads(data.decode('utf-8-sig'))
 except (UnicodeError,ValueError):return
 def nested(o):
  if isinstance(o,dict):
   for k,v in o.items():
    if k=='base64' and isinstance(v,str):
     raw=base64.b64decode(v,validate=True);assert not raw.startswith((b'MZ',b'PK\x03\x04'));assert not private.search(raw) and not credentials.search(raw)
    nested(v)
  elif isinstance(o,list):
   for v in o:nested(v)
 nested(o)
rows=[]
for p in sorted(root.rglob('*')):
 assert not p.is_symlink()
 if p.is_dir():continue
 assert p.is_file() and p.suffix.lower() not in ['.exe','.zip','.pyc','.pfx','.p12','.dll','.pdb','.obj','.lib']
 data=p.read_bytes();assert len(data)<=32*1024*1024;data.decode('utf-8-sig');scan(data)
 relative=p.relative_to(root);rows.append({'source':str(p),'path':prefix+'/'+relative.as_posix(),'bytes':len(data),'sha256':sha(data)})
assert sum(r['bytes'] for r in rows)<64*1024*1024
summary={'status':'PASS_STAGE_PROGRESS_NOT_FULL_PRODUCT','recordedUtc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'goal':'ACTIVE: 按照计划完成全部开发。','gitBaselineReference':build['gitHeadBefore'],'sourceIdentity':'Tested modified source is383 exact input byte/SHA map in final binding, not falsely attributed to baseline Git commit. New synchronized SHA and CI dispatch are later receipts.','sourceBinding':{'path':prefix+'/final-source-and-runtime-binding.json','sha256':sha((root/'final-source-and-runtime-binding.json').read_bytes())},'scope':'Separate bare AcceptEx ownership for isolated HTTP candidate, affected GNU release/native Win11 and real Schannel continuous acceptance, actual prior shared-helper CI diagnosis and minimal Win7 Clippy fix. Production listener stays off.','windows':{'os':after['os'],'build':after['build'],'architecture':after['osArchitecture'],'programTarget':'i686-pc-windows-gnu x86/WOW64','harnesses':1,'passed':177,'failed':0,'ignored':0,'newStreamTests':9,'normalExeSha256':native['exeSha256'],'fullWorkspace1313Rerun':False,'cleanup':'PASS'},'tls':{'events':len(tls['events']),'native':tls['native'],'idle':tls['continuousIdle'],'capacity':tls['business']['continuousAcceptance'],'snapshotCount':len(tls['nativeProgress']),'lastSnapshot':tls['nativeProgress'][-1],'observedApplicationMaxima':binding['realTls']['applicationObservedMaxima'],'cleanup':tls['cleanupDetails']},'priorCi':{'sourceSha':'93414d5f26c829db552f8434433f6ad0684971f6','modern':modern,'win7':win7,'win7WholeArtifactHash':'API/upload metadata only; selected16 nonbinary members verified via CRC and original ledger; no whole ZIP/executable runtime claim','selectedOriginalsReport':prefix+'/win7-selected-originals-first.json'},'win7ClippyCorrection':{'cause':repro['status'],'sameFixedNightly':repro['rustcCommit'],'globalArgumentExit':101,'afterClippyArgumentExit':0,'pureCases':pure['cases'],'pureAssertions':pure['assertions'],'fullNewSourceHostedCi':'NOT_RUN at archive time; exactly once per baseline after final ordinary sync','actualWin7Runtime':'NOT_RUN'},'developmentPackage':pkg['package'],'developmentPackageStatus':pkg['manifest']['status'],'preservedFailures':['First native static review blocked a controlled-close-failure test destructor; test-only fixture correction reviewed before any compile','Fixed-nightly minimal repro first missing Clippy component; original failure retained; official matching Clippy installed as continuing development tool','Actual original Win7 CI Clippy exit101/E0463; overall FAILED_OR_INCOMPLETE retained','Actual same-source local global-position diagnostic exit101; after-subcommand position exit0; production full build-std preserved'],'localCompilerBudget':'2 jobs, no concurrent Cargo builds; actual native tests2threads; original business pool2workers','windowsFinalAudit':{'status':after['status'],'path':prefix+'/windows-after.json','sha256':sha((root/'windows-after.json').read_bytes()),'RunAndSshAuthorizationUnchanged':True,'ownedRemainingDirectoriesProcessesTasks':0,'persistentSshServiceAndKey':'KEPT, private key contents not read, mode0600'},'localBuildAndTestCleanup':{'path':prefix+'/local-build-and-test-cleanup.json','status':cleanup['status'],'removed':cleanup['removed'],'retained':cleanup['retained']},'unchangedOldIndexes':old_indexes,'remainingGates':binding['remainingGates'],'sourceMapping':rows}
for row in rows:
 p=Path(row['source']);data=p.read_bytes();assert len(data)==row['bytes'] and sha(data)==row['sha256'];q=dest/row['path'];q.parent.mkdir(parents=True,exist_ok=True)
 with q.open('xb') as f:f.write(data)
 assert q.read_bytes()==data
with summary_path.open('x') as f:f.write(json.dumps(summary,ensure_ascii=False,indent=2)+'\n')
entries=[{k:r[k] for k in ['path','bytes','sha256']} for r in rows];data=summary_path.read_bytes();entries.append({'path':summary_path.name,'bytes':len(data),'sha256':sha(data)})
index={'status':'PASS_IMMUTABLE_STAGE_INDEX','scope':'Original Round34 implementation reviews/failures/builds/Win11/TLS/CI diagnosis and unvalidated packaging plus summary. No EXE/ZIP/private credentials. Later sync/cleanup/CI-dispatch receipts stored separately.','files':entries,'fileCount':len(entries),'logicalBytes':sum(r['bytes'] for r in entries),'oldIndexes':old_indexes}
with index_path.open('x') as f:f.write(json.dumps(index,ensure_ascii=False,indent=2)+'\n')
print(json.dumps({'status':summary['status'],'files':len(entries),'bytes':index['logicalBytes'],'indexSha256':sha(index_path.read_bytes()),'windowsTests':177,'tlsEvents':55,'package':pkg['package']},ensure_ascii=False),flush=True)
