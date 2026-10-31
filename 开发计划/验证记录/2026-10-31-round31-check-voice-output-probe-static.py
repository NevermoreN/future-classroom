from pathlib import Path
import re,json,base64,hashlib,difflib
r=Path('/tmp/fc-round31-device-evidence');base=Path('/workspace/共享/Eliora/未来教室计划')
paths=['crates/fc-bootstrap/src/application/voice_output_probe.rs','crates/fc-bootstrap/src/application/voice_management_probe.rs','crates/fc-bootstrap/src/application.rs','crates/fc-bootstrap/src/main.rs']
name=paths[1];old=(base/name).read_text();now=Path(name).read_text();expected=old
for f in ['checked','require','pump','submit','saved','no_jobs','same_saved','make_app','open','login','open_settings','stop_owned_app']:
 expected,n=re.subn(r'^fn '+f+r'(?=[<(])','pub(super) fn '+f,expected,flags=re.M);assert n==1,(f,n)
for f in ['LOGIN_NAME','PASSWORD_VALUE']:expected=expected.replace('const '+f,'pub(super) const '+f,1)
expected=expected.replace('pub(super) fn same_saved(before: &VoiceManagementSnapshot, after: &VoiceManagementSnapshot) -> Check<()> {','pub(super) fn same_saved(\n    before: &VoiceManagementSnapshot,\n    after: &VoiceManagementSnapshot,\n) -> Check<()> {')
assert now==expected,'Original probe must differ only in requested visibility/required signature formatting.'
probe=Path(paths[0]).read_text();order=re.search(r'const ORDER: \[&str; 15\] = \[(.*?)\];',probe,re.S).group(1);order=re.findall(r'"([a-z0-9-]+)"',order)
assert len(order)==15 and len(set(order))==15
assert re.findall(r'mark\((?:step|&mut step), "([a-z0-9-]+)"\)',probe)==order
assert 'Duration::from_secs(120)' in probe and 'while first.is_fresh()' in probe
assert probe.count('explicit_check(app, deadline)?')==2
for bad in ['set_enabled(', 'set_limits_', 'silent_probe(', 'waveOutWrite(', 'waveIn', 'SetForegroundWindow(', 'Fake']:assert bad not in probe,bad
assert probe.index('stop_owned_app(app, deadline)?;')<probe.index('fs::remove_dir_all(&root)')
entries=[{'path':n,'bytes':len(b:=Path(n).read_bytes()),'sha256':hashlib.sha256(b).hexdigest(),'base64':base64.b64encode(b).decode()} for n in paths]
report={'status':'FROZEN_STATIC_ONLY_NOT_COMPILED_OR_RUN','worktree':'/tmp/fc-round31-device-worktree','files':entries,'checks':{'old36StepProbeOnlyVisibilityAndSignatureWrappingChanged':True,'orderedStepsMatchAll15Marks':True,'queryCountTwoExplicitActionsOnly':True,'realOriginalReceiptTTLWaitNoFakeClock':True,'physicalShutdownBeforeRootRemovalEvenWorkflowError':True,'rustfmtEdition2021Check':'PASS'},'order':order,'summaryMarker':'VOICE_OUTPUT_NATIVE_PASS','fieldContract':'steps=15 shared_workers=2 first_result=<Supported|FormatUnsupported|Unavailable|Failed> second_result=<same enum> ttl_expired=PASS audio_jobs=NONE physical_playback=NOT_EXERCISED foreground=UNTOUCHED isolated_cleanup=PASS elapsed_ms=<integer>','ownedRootPrefix':'FutureClassroom-Test-VoiceOutput-<pid>-<32hex>','clockBudget':'One original120s deadline including actual TTL waiting and shutdown; no deadline renewal.','limits':['No claim of audibility, actual playback, microphone/volume changes or three-OS acceptance.','Native query is synchronous; original shared worker must return before stop/join completes.','Actual Windows root invocation and validator remain parent-owned.'],'originalManagementProbeReference':{'sha256':hashlib.sha256(old.encode()).hexdigest(),'base64':base64.b64encode(old.encode()).decode()},'managementProbeDiff':''.join(difflib.unified_diff(old.splitlines(True),now.splitlines(True),fromfile='main-reference/voice_management_probe.rs',tofile='worktree/voice_management_probe.rs'))}
with (r/'voice-output-probe-frozen-static.json').open('x') as f:json.dump(report,f,indent=2);f.write('\n')
print(json.dumps({'status':report['status'],'order':order,'files':[{k:v for k,v in e.items() if k!='base64'} for e in entries]}))
