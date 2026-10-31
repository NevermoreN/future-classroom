from pathlib import Path
import sys,runpy,json,hashlib,subprocess,copy,difflib
sys.dont_write_bytecode=True
root=Path('/tmp/fc-round31-device-worktree');ev=Path('/tmp/fc-round31-device-evidence')
out=ev/'runner-validator-second.json';assert not out.exists()
p=root/'scripts/windows-probe.py';source=p.read_bytes();m=runpy.run_path(str(p));validate=m['validate_voice_output_output']
steps=[f'VOICE_OUTPUT_NATIVE_STEP index={i} name={name}' for i,name in enumerate(m['VOICE_OUTPUT_STEPS'])]
summary='VOICE_OUTPUT_NATIVE_PASS steps=15 shared_workers=2 first_result=Supported second_result=Unavailable ttl_expired=PASS audio_jobs=NONE physical_playback=NOT_EXERCISED foreground=UNTOUCHED isolated_cleanup=PASS elapsed_ms=35000'
valid=steps+[summary]
cases=[]
def check(name,data,accepted,exit_code=0):
    try: result=validate(exit_code,data);actual=True
    except (RuntimeError,ValueError,TypeError): result=None;actual=False
    cases.append({'name':name,'expectedAccepted':accepted,'actualAccepted':actual,'passed':accepted==actual})
    assert accepted==actual,name
for value in ['Supported','FormatUnsupported','Unavailable','Failed']:
    check('valid-'+value,steps+[summary.replace('first_result=Supported','first_result='+value)],True)
check('elapsed-exact-30000',steps+[summary.replace('35000','30000')],True)
check('elapsed-zero',steps+[summary.replace('35000','0')],False)
check('elapsed-below-real-ttl',steps+[summary.replace('35000','29999')],False)
check('summary-before-last-step',steps[:-1]+[summary,steps[-1]],False)
check('summary-before-all-steps',[summary]+steps,False)
check('nonzero-exit',valid,False,1)
check('non-list',summary,False)
check('non-string-line',steps+[None,summary],False)
check('missing-step',steps[:-1]+[summary],False)
check('duplicate-step',steps+[steps[-1],summary],False)
check('reordered-step',[steps[1],steps[0]]+steps[2:]+[summary],False)
check('wrong-step-name',[steps[0].replace('administrator','teacher')]+steps[1:]+[summary],False)
check('wrong-step-index',[steps[0].replace('index=0','index=1')]+steps[1:]+[summary],False)
check('duplicate-summary',valid+[summary],False)
check('wrong-enum',steps+[summary.replace('first_result=Supported','first_result=Audible')],False)
check('wrong-cleanup',steps+[summary.replace('isolated_cleanup=PASS','isolated_cleanup=FAIL')],False)
check('wrong-playback-scope',steps+[summary.replace('physical_playback=NOT_EXERCISED','physical_playback=PASS')],False)
check('malformed-time',steps+[summary.replace('35000','-1')],False)
old_steps=[f'VOICE_SETTINGS_NATIVE_STEP index={i} name={name}' for i,name in enumerate(m['VOICE_SETTINGS_STEPS'])]
old_summary='VOICE_SETTINGS_NATIVE_PASS steps=36 shared_workers=2 database_reopen=PASS limits_reopen=PASS limits_stale_cas=PASS limits_defaults_restored=PASS audio_jobs=NONE physical_playback=NOT_EXERCISED foreground=UNTOUCHED isolated_cleanup=PASS elapsed_ms=100'
assert m['validate_voice_settings_output'](0,old_steps+[old_summary])['steps']==36
cases.append({'name':'old-36-step-validator-unchanged','passed':True})
command=[sys.executable,'-B','scripts/windows-probe.py','--host','elior@192.168.10.212','--identity','/nonexistent','--exe','/nonexistent','--voice-output','--voice-settings','--report','/nonexistent']
x=subprocess.run(command,cwd=root,capture_output=True);assert x.returncode==2 and b'dedicated output query' in x.stderr
before=(ev/'windows-probe.before-timing-guard.py').read_bytes()
(ev/'windows-probe.timing-guard-frozen.py').write_bytes(source)
r={'scope':'LOCAL_SYNTHETIC_VERIFIER_TESTS_NOT_WINDOWS_OR_DEVICE_EVIDENCE','status':'PASS','sourceSha256':hashlib.sha256(source).hexdigest(),'beforeSourceSha256':hashlib.sha256(before).hexdigest(),'changeReason':'Reject a claimed real thirty-second expiry completed in under30000ms, and premature final summary markers. Original successful Windows result remains required separately.','diff':''.join(difflib.unified_diff(before.decode().splitlines(True),source.decode().splitlines(True),fromfile='before',tofile='after')),'cases':cases,'caseCount':len(cases),'cliConflict':{'command':command,'exitCode':x.returncode,'stdout':x.stdout.decode(),'stderr':x.stderr.decode()}}
out.write_text(json.dumps(r,indent=2)+'\n');print(json.dumps({'status':'PASS','caseCount':len(cases),'sourceSha256':r['sourceSha256'],'scope':r['scope']}))
