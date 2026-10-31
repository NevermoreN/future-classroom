from pathlib import Path
import base64, datetime, hashlib, json, re, stat

BASE=Path('/tmp/fc-round42-ci-evidence')
OUT=BASE/'archive-candidate-manifest-second.json'
sha=lambda b:hashlib.sha256(b).hexdigest()
assert not OUT.exists()

def load(name):return json.loads((BASE/name).read_text(encoding='utf-8-sig'))
checks=[]
def check(name,value):
 checks.append({'name':name,'status':'PASS' if value else 'FAIL'})
 assert value,name
modern=load('modern-programs-independent-first.json');win7=load('win7-original-independent-second.json')
runtime=load('modern-runtime-independent-first.json');runtime_original=load('modern-msvc-runtime-first.json')
public=load('modern-msvc-public-native-first.json');visual=load('modern-msvc-visual-native-first.json')
browser=load('voice-capabilities-browser-independent-first.json');package=load('package-independent-first.json')
source='bc2a86fb4413dcb3e8acf6b0d4bb2dfe601c22dc'
check('exact R42 modern original independent terminal',modern['status']=='PASS' and modern['sourceSha']==source)
check('Win7 independent original valid while CI remains failed',win7['status']=='PASS_INDEPENDENT_ARTIFACT_CHECKS_CI_REMAINS_FAILED' and win7['sourceSha']==source)
check('full runtime1413 no failures or hidden filtered',runtime['status']=='PASS' and runtime['programs']==21 and runtime['counts']=={'passed':1413,'failed':0,'ignored':0,'measured':0,'filtered':0} and runtime['cleanup']=='PASS')
check('runtime original immutable binding',runtime['originalReportSha256']==sha((BASE/'modern-msvc-runtime-first.json').read_bytes()) and runtime_original['status']==runtime_original['cleanup']=='PASS')
check('original local counterguard failure retained',load('runtime-count-extraction-first-failure.json')['status']=='FAIL_LOCAL_GUARD')
check('20 public and22 visual original PASS cleanup',public['status']==public['cleanup']==visual['status']==visual['cleanup']=='PASS' and public['publicDesktopNativeSteps']==20 and visual['subjectVisualNativeSteps']==22)
check('actual voice browser independent163 checks PASS',browser['status']=='PASS' and len(browser['checks'])==163 and all(x['status']=='PASS' for x in browser['checks']))
check('actual voice native counts remain dynamic conservation',browser['counts']['nativeHeads']==browser['counts']['nativeResponses']==browser['counts']['clientHttpResponses']+browser['counts']['browserWireResponses'])
check('actual three GET states and unknown query/cache remain',browser['counts']['capabilityStatuses']==[401,200,200] and browser['capabilityObservation']['deviceAvailability']=='UNVERIFIED' and browser['capabilityObservation']['nativeQueryCount']=='NOT_OBSERVED' and browser['capabilityObservation']['cacheBehavior']=='NOT_DETERMINED_BY_GET')
check('first Win7 independent checker failure retained',load('win7-original-independent-first.json')['status']=='FAILED_INDEPENDENT_CHECK')
check('Win7 three actual missing_const diagnostics and clippy-only failure',win7['counts']['clippyMissingConstDiagnostics']==3 and [x['name'] for x in win7['ciTerminal']['failedCommands']]==['clippy'] and win7['ciTerminal']['failedCommands'][0]['exit_code']==101)
for name in ['windows-before-first.json','windows-after-first.json']:
 x=load(name);check('readonly Windows audit exact state '+name,x['status']=='PASS' and x['sshd']=='Running' and x['persistentKeyMode']=='0o600' and x['publicAuthorizationUnchanged'] is True and x['projectRunUnchanged'] is True and x['scratchDirectories']==x['ownedProcesses']==x['ownedTasks']==[])
external=[]
for p,expected,scope,retain in [
 (Path('/tmp/fc-round42-modern-original.zip'),load('modern-full-download-first.json'),'Official exact original modern CI ZIP','Do not remove during ongoing root verification/archival'),
 (Path('/tmp/fc-round42-win7-original.zip'),load('win7-full-download-first.json'),'Official exact original failed Win7 CI ZIP; executables NOT_RUN','Retain original failure provenance until root separately completes archive/cleanup'),
 (Path('/tmp/fc-round42-package/FutureClassroom-0.1.0-voice-capability-dev.zip'),package,'Development package candidate still used by R41','IN_USE_BY_R41_DO_NOT_REMOVE_OR_MOVE'),
]:
 data=p.read_bytes();check('external original exact size/SHA '+p.name,len(data)==expected['bytes'] and sha(data)==expected['sha256'])
 external.append({'path':str(p),'bytes':len(data),'sha256':sha(data),'scope':scope,'retention':retain,'archiveBinary':False})
intentional=load('package-intentional-artifact-first.json')
ip=Path(intentional['path']);ib=ip.read_bytes()
check('intentional retained development package copy exact bytes/SHA',intentional['status']=='PASS' and intentional['sourceSha']==source and len(ib)==intentional['bytes']==package['bytes'] and sha(ib)==intentional['sha256']==package['sha256'])
external.append({'path':str(ip),'bytes':len(ib),'sha256':sha(ib),'scope':'Parent actually saved intentional development ZIP, not complete product/Release','retention':'INTENTIONAL_RETAINED_DO_NOT_CLEAN','archiveBinary':False,'evidence':'package-intentional-artifact-first.json'})
programs=[]
for x in modern['artifacts']:
 p=Path(x['localPath']);data=p.read_bytes();check('modern held EXE exact SHA '+p.name,len(data)==x['bytes'] and sha(data)==x['sha256'])
 programs.append({'path':str(p),'bytes':len(data),'sha256':sha(data),'role':x['role']})
web=[]
for x in load('web-assets-handoff-first.json')['assets']:
 p=Path('/tmp/fc-round42-web-assets/dist')/x['path'];data=p.read_bytes();check('held Web artifact exact SHA '+x['path'],len(data)==x['bytes'] and sha(data)==x['sha256'])
 web.append({'path':str(p),'bytes':len(data),'sha256':sha(data)})
patterns={
 'privateKey':re.compile(rb'-----BEGIN (?:[A-Z0-9 ]* )?PRIVATE KEY-----'),
 'githubCredential':re.compile(rb'\b(?:gh[pousr]_[A-Za-z0-9]{30,}|github_pat_[A-Za-z0-9_]{40,})'),
 'openaiCredential':re.compile(rb'\bsk-(?:proj-)?[A-Za-z0-9_-]{40,}'),
 'literalAuthorization':re.compile(rb'(?im)^\s*Authorization:\s*(?:Bearer|Basic)\s+[A-Za-z0-9+/=_-]{20,}'),
 'literalStopCapability':re.compile(rb'"stopCapability"\s*:\s*"[0-9a-f]{64}"'),
}
findings=[];decoded=0

def scan(raw,path,origin):
 for name,pat in patterns.items():
  if pat.search(raw):findings.append({'path':path,'origin':origin,'kind':name})

def decode_fields(obj,path):
 global decoded
 if isinstance(obj,dict):
  for key,value in obj.items():
   if key=='base64' and isinstance(value,str):
    data=base64.b64decode(value,validate=True);decoded+=1;scan(data,path,'decoded JSON base64')
    try:nested=json.loads(data.decode('utf-8-sig'))
    except (ValueError,UnicodeError):nested=None
    if nested is not None:decode_fields(nested,path)
   elif isinstance(value,(dict,list)):decode_fields(value,path)
 elif isinstance(obj,list):
  for value in obj:decode_fields(value,path)
files=[];excluded=[]
for p in sorted(BASE.rglob('*')):
 if p.is_dir():continue
 rel=p.relative_to(BASE).as_posix()
 check('only regular non-link evidence object '+rel,stat.S_ISREG(p.lstat().st_mode) and not p.is_symlink())
 if rel=='.owner':excluded.append({'path':rel,'reason':'Temporary directory ownership marker, not archive evidence'});continue
 check('no active private control capability file '+rel,not p.name.endswith('.control.json'))
 raw=p.read_bytes();check('no native binary inside candidate evidence '+rel,not raw.startswith(b'MZ') and p.suffix.lower() not in ['.exe','.dll','.pdb','.zip'])
 scan(raw,rel,'original bytes')
 if p.suffix=='.json':
  try:obj=json.loads(raw.decode('utf-8-sig'))
  except (ValueError,UnicodeError):obj=None
  if obj is not None:decode_fields(obj,rel)
 files.append({'path':rel,'bytes':len(raw),'sha256':sha(raw)})
check('literal and decoded private credential scan has no findings',not findings)
check('all evidence original bytes unchanged through snapshot',all((BASE/x['path']).stat().st_size==x['bytes'] and sha((BASE/x['path']).read_bytes())==x['sha256'] for x in files))
assert set(x['path'] for x in files)|{x['path'] for x in excluded}=={p.relative_to(BASE).as_posix() for p in BASE.rglob('*') if p.is_file()}
r={'schemaVersion':1,'status':'READY_FOR_ARCHIVE_REVIEW_NOT_ARCHIVED','observedAtUtc':datetime.datetime.now(datetime.timezone.utc).isoformat(),
 'scope':'Read-only archive candidate inventory and original result binding only. No main repository write, copy, cleanup, Git, Windows, CI or runtime execution. R41 retains package/assets; no binary is selected for source evidence archive.',
 'sourceRoot':str(BASE),'suggestedDestination':'开发计划/验证记录/2026-11-01-round42-ci','sourceSha':source,
 'files':files,'fileCount':len(files),'totalBytes':sum(x['bytes'] for x in files),
 'indexExcludedFromOwnLedger':True,'excluded':excluded,
 'secretScan':{'status':'PASS_LITERAL_AND_JSON_BASE64_SCAN','literalPatterns':list(patterns),'base64FieldsDecodedCumulative':decoded,'matches':len(findings),'limitations':'Pattern scan is bounded hygiene, not a proof that arbitrary data contains no unknown secret; private key files and active control files are outside this inventory.'},
 'requiredResultBindings':{k:{'path':k,'sha256':sha((BASE/k).read_bytes())} for k in [
 'modern-full-download-first.json','modern-programs-independent-first.json','modern-runtime-independent-first.json','runtime-count-extraction-first-failure.json',
 'modern-msvc-runtime-first.json','modern-msvc-public-native-first.json','modern-msvc-visual-native-first.json','modern-msvc-voice-capabilities-browser-first.json',
 'voice-capabilities-browser-independent-first.json','win7-full-download-first.json','win7-failure-diagnosis-first.json','win7-original-independent-first.json','win7-original-independent-second.json',
 'package-independent-first.json','package-intentional-artifact-first.json','windows-before-first.json','windows-after-first.json']},
 'externalRetainedReferences':external,
 'modernPrograms':{'scope':'22 exact CI EXEs retained outside archive; do not execute or delete by inventory','files':programs},
 'webAssets':{'scope':'Held R39 final Web build, still used by R41; not a new Web test execution; DO_NOT_CLEAN','files':web},
 'outcomes':{'modernCi':'SUCCESS','actualWin11':{'programs':21,'passed':1413,'failed':0,'ignored':0,'filtered':0,'publicNativeSteps':20,'visualNativeSteps':22,'cleanup':'PASS'},
 'browser':browser['counts'],'win7Ci':'FAILURE_CLIPPY101_THREE_ACTUAL_TLS_STATIC_DIAGNOSTICS','win7Runtime':'NOT_RUN',
 'localCounterFirst':'FAIL_LOCAL_GUARD_PRESERVED','win7IndependentFirst':'FAIL_DIAGNOSTIC_COUNT_ASSUMPTION_PRESERVED','win7IndependentSecond':'ARTIFACT_CHECKS_PASS_CI_STILL_FAILED'},
 'checks':checks,'archivePerformed':False,'cleanupPerformed':False,'mainRepositoryModified':False,
 'limitations':['Exact source does NOT contain R41 deadline changes.','Strict original first-TCP five-second gate remains FAIL/not validated; ordinary production listener remains disabled.','Win7/Win10 actual runtime, visible Seewo, mobile/campus TLS and microphone/audibility remain independent unpassed gates.','Formal final archive INDEX and any later evidence require parent-owned separate exact-byte copy/hash proof; this is a point-in-time candidate inventory.']}
with OUT.open('x') as f:f.write(json.dumps(r,ensure_ascii=False,indent=2)+'\n')
print(json.dumps({'status':r['status'],'fileCount':r['fileCount'],'totalBytes':r['totalBytes'],'checks':len(checks),'decodedBase64Fields':decoded,'credentialFindings':len(findings),'reportSha256':sha(OUT.read_bytes())},ensure_ascii=False))
