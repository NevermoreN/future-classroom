from pathlib import Path
import json,hashlib,subprocess,re,runpy,sys,stat
sys.dont_write_bytecode=True
root=Path('/tmp/fc-round31-device-worktree');ev=Path('/tmp/fc-round31-device-evidence');out=ev/'final-source-and-runtime-binding.json';assert not out.exists()
def sha(b):return hashlib.sha256(b).hexdigest()
host=json.loads((ev/'host-workspace-third.json').read_bytes());build=json.loads((ev/'windows-workspace-first.json').read_bytes())
runtime=json.loads((ev/'windows-full-and-voice-output-first.json').read_bytes());old=json.loads((ev/'windows-voice-settings-regression-first.json').read_bytes())
assert host['status']==build['status']==runtime['status']==old['status']=='PASS'
assert host['sourceBefore']==host['sourceAfter']==build['sourceBefore']==build['sourceAfter']
for name,row in build['sourceAfter'].items():
    data=(root/name).read_bytes();assert len(data)==row['bytes'] and sha(data)==row['sha256'],name
programs={Path(row['path']).name:row for row in build['artifacts']}
for row in build['artifacts']:
    data=Path(row['path']).read_bytes();assert len(data)==row['bytes'] and sha(data)==row['sha256']
normal=next(row for row in build['artifacts'] if not row['test'])
assert normal['sha256']==runtime['exeSha256']==old['exeSha256']=='81ae6c690b4e8ac2516f76a6b70cea55adc8ddef79ac621fc2813759abfeb3d3'
assert runtime['cleanup']==old['cleanup']=='PASS'
assert len(runtime['tests'])==21
groups=[]
for row in runtime['tests']:
    assert row['exitCode']==0 and row['binarySha256']==programs[row['name']]['sha256']
    results=re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;', '\n'.join(row['output']))
    assert len(results)==1
    values=tuple(map(int,results[0]));assert values[1:]==(0,0);groups.append({'program':row['name'],'passed':values[0],'failed':0,'ignored':0})
assert sum(x['passed'] for x in groups)==1307
helper=runpy.run_path(str(root/'scripts/windows-probe.py'))
observation=helper['validate_voice_output_output'](runtime['native']['voiceOutputExitCode'],runtime['native']['voiceOutputOutput'])
settings=helper['validate_voice_settings_output'](old['native']['voiceSettingsExitCode'],old['native']['voiceSettingsOutput'])
assert observation['steps']==15 and observation['elapsedMs']==30391 and settings['steps']==36
before=json.loads((ev/'windows-before.json').read_bytes());after=json.loads((ev/'windows-after.json').read_bytes())
assert before['status']==after['status']=='PASS'
assert before['sshPublicAuthorizationSha256']==after['sshPublicAuthorizationSha256']=='d669ba58f88179b9962b2021332141d0fb0fc84478b694491155a1f184d432b5'
assert before['projectRunReadOnlySnapshot']==after['projectRunReadOnlySnapshot']
assert after['sshd']=='Running' and after['persistentKeyMode']=='0o600' and not any(after[k] for k in ['scratchDirectories','ownedProcesses','ownedTasks'])
assert stat.S_IMODE(Path('/config/.local/share/future-classroom-test-access/win11-192.168.10.212').stat().st_mode)==0o600
package=json.loads((ev/'development-package-first.json').read_bytes());assert package['status']=='PASS_DEVELOPMENT_PACKAGE_ONLY' and package['repackByteIdentical'] and package['temporaryPackagingRootsRemoved']
assert sha(Path(package['package']['path']).read_bytes())==package['package']['sha256']
changed=subprocess.check_output(['git','diff','--name-only','-z'],cwd=root).decode().split('\0')
untracked=subprocess.check_output(['git','ls-files','--others','--exclude-standard','-z'],cwd=root).decode().split('\0')
paths=sorted(set(filter(None,changed+untracked)));source=[]
patch=subprocess.check_output(['git','diff','--binary'],cwd=root)
for name in paths:
    p=root/name;assert p.is_file() and not p.is_symlink()
    data=p.read_bytes();data.decode('utf-8');assert p.suffix.lower() not in ['.exe','.dll','.zip','.pdb','.lib','.obj']
    source.append({'path':name,'bytes':len(data),'sha256':sha(data)})
for name in filter(None,untracked):
    result=subprocess.run(['git','diff','--no-index','--','/dev/null',name],cwd=root,capture_output=True);assert result.returncode==1;patch+=result.stdout
(ev/'final-feature-working-diff.patch').write_bytes(patch)
r={'status':'PASS_CURRENT_FEATURE_BOUND_TO_ACTUAL_WIN11_GNU','scope':'All unchanged385 Rust/Cargo input bytes match successful Host and GNU build; per-EXE SHA matches actual21 Win11 harnesses plus15-step native output and36-step policy regression. No actual Win7/Win10, full product, audible playback, Seewo or feature MSVC claim.','gitBaseBeforeFeatureCommit':subprocess.check_output(['git','rev-parse','HEAD'],cwd=root).decode().strip(),'rustCargoInputs':build['sourceAfter'],'changedSourceBeforeEvidenceArchive':source,'completeWorkingDiffSha256':sha(patch),'host':{'passed':host['totalPassed'],'groups':len(host['testGroups']),'testExecutables':len(host['artifacts']),'strictClippy':'PASS'},'windows':{'platform':runtime['platform'],'target':'i686-pc-windows-gnu','groups':groups,'totalPassed':1307,'testExecutables':21,'strictClippy':'PASS','normal':normal,'outputObservation':observation,'originalSettingsRegression':settings,'physicalPlayback':'NOT_EXERCISED_BY_SETTINGS_OUTPUT_PROBES'},'programs':build['artifacts'],'windowsCleanupAndPersistentAccess':{'status':after['status'],'ownedDirectories':0,'ownedProcesses':0,'ownedTasks':0,'sshd':after['sshd'],'publicAuthorizationSha256':after['sshPublicAuthorizationSha256'],'runBothViewsUnchanged':True,'privateKeyMode':'0o600','privateKeyContentsRead':False},'developmentPackage':package['package'],'packageStatus':'NOT_VALIDATED','postBinaryBuildChanges':'Package feature metadata/README and evidence line-ending attributes only; no Rust/Cargo input changes.','fullObjective':'ACTIVE_NOT_COMPLETE'}
out.write_text(json.dumps(r,indent=2)+'\n');print(json.dumps({'status':r['status'],'sourceInputs':len(r['rustCargoInputs']),'changedSourceFiles':len(source),'hostPassed':859,'windowsPassed':1307,'outputSteps':15,'oldSettingsSteps':36,'normalSha256':normal['sha256']}),flush=True)
