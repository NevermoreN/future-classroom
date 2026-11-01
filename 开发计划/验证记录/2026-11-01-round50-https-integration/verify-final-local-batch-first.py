from pathlib import Path
import hashlib,json,re,subprocess,datetime
root=Path('/workspace/共享/Eliora/未来教室计划');ev=Path('/tmp/fc-round50-https-integration-evidence')
def fact(p):return {'path':str(p),'bytes':p.stat().st_size,'sha256':hashlib.sha256(p.read_bytes()).hexdigest()}
def load(name):p=ev/name;return p,json.loads(p.read_text())
second,s=load('full-matrix-second.json');host,h=load('host-final-third.json');integ,i=load('main-source-integration-first.json');assert s['status']=='FAIL' and s['commands'][-1]['timedOut'] and s['commands'][-1]['actualExit'];assert h['status']=='PASS_ALL_HOST_TARGET_TESTS_AND_STRICT';assert h['sourceBefore']==h['sourceAfter']==s['sourceBefore']==s['sourceAfter'];assert not h['changedInputs'] and not s['changedInputs'];assert len(h['sourceBefore'])==429
for n,info in h['sourceBefore'].items():assert (root/n).stat().st_size==info['bytes'] and fact(root/n)['sha256']==info['sha256']
checks=s['commands'][:-1]+h['commands'];assert len(checks)==10
for row in checks:
 assert row['exitCode']==0 and row.get('actualExit',True) and not row.get('timedOut',False)
 for label in ('stdout','stderr'):
  f=row[label];assert fact(Path(f['path']))==f
stdout=Path(h['commands'][0]['stdout']['path']).read_text();stderr=Path(h['commands'][0]['stderr']['path']).read_text();summaries=re.findall(r'^test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out; finished in .+$',stdout,re.M);running=[x for x in stderr.splitlines() if re.match(r'\s*Running ',x)];assert len(summaries)==len(running)==21
counts=[sum(int(x[j]) for x in summaries) for j in range(5)];assert counts==[973,0,0,0,0]
for row in i['files']:assert fact(root/row['path'])['sha256']==row['sha256']
for name in ('windows-gnu-runtime-first.json','windows-https-certificate-native-first.json','windows-public-native-first.json','windows-visual-native-first.json','windows-default-browser-continuous-first.json','windows-readonly-final-first.json'):
 p,d=load(name);assert d['status']=='PASS';assert d.get('cleanup','PASS')=='PASS'
for name in ('runtime-certificate-independent-second.json','browser-readonly-independent-second.json','gnu-programs-independent-first.json'):
 p,d=load(name);assert str(d['status']).startswith('PASS')
r={'status':'PASS_VALIDATED_LOCAL_BATCH_SOURCE_HOST_GNU_WIN11_FINITE','recordedUtc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'sourceInputs':h['sourceBefore'],'mainRustInputsExact429':True,'mainCodeFiles':i['files'],'sourceIntegration':fact(integ),'windowsCompletedCommandsFromOriginalFailedMatrix':s['commands'][:-1],'hostFinalCompletedCommands':h['commands'],'originalFullMatrixFailure':fact(second),'hostFinal':fact(host),'hostCounts':dict(zip(('passed','failed','ignored','measured','filtered'),counts)),'hostRunningGroups':running,'hostSummaries':summaries,'noOriginalFailureOverwritten':True,'testProgramScope':'Host21 groups973 vs independent actualWin11 GNU21EXE1481; sums not combined into a platform total. Public20/visual22/cert22 and fullbrowser each separate.','productionListener':'NOT_ENABLED','strictOriginalTcp5s':'ORIGINAL_FAIL_NOT_REPLACED','newSourceMsvc':'NOT_RUN_YET','win7Os':'NOT_VALIDATED','win10Os':'NOT_VALIDATED','visibleSeewo':'NOT_VALIDATED','physicalAudio':'NOT_EXERCISED','hermeticity':'Exact known429 Rust/build inputs and tracked33Rust+3test changes; no dynamic build.rs sandbox/hermetic claim.','scratchTimeoutCleanup':fact(ev/'host-timeout-scratch-cleanup-first.json'),'localScratchDirectoriesRemaining':[p.name for p in Path('/tmp').iterdir() if p.is_dir() and p.name.startswith('FutureClassroom-')]}
assert not r['localScratchDirectoriesRemaining'];out=ev/'final-local-batch-first.json';assert not out.exists();out.write_text(json.dumps(r,indent=2)+'\n');print('PASS_FINAL_LOCAL_BATCH_HOST973_WIN111481_SOURCE429_EXACT')
