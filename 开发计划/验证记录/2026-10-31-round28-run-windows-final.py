from pathlib import Path
import subprocess,json,hashlib,os,re
p=Path('/tmp/fc-round28-evidence');repo=Path('/workspace/共享/Eliora/未来教室计划');env={**os.environ,'CARGO_BUILD_JOBS':'2'};exe=Path('/tmp/fc-round28-win/i686-pc-windows-gnu/release/fc-bootstrap.exe')
base=['cargo','test','--workspace','--release','--locked','--no-run','--target','i686-pc-windows-gnu','--target-dir','/tmp/fc-round28-win','--message-format=json']
def run(args,out,err=None):
 with (p/out).open('w') as o:
  if err:
   with (p/err).open('w') as e:r=subprocess.run(args,stdout=o,stderr=e,env=env,cwd=repo)
  else:r=subprocess.run(args,stdout=o,stderr=subprocess.STDOUT,env=env,cwd=repo)
 if r.returncode:raise SystemExit(r.returncode)
def items(name):
 rows=[json.loads(l) for l in (p/name).read_text().splitlines()];return [r for r in rows if r.get('reason')=='compiler-artifact' and r.get('profile',{}).get('test') and r.get('executable')]
run(base,'windows-test-artifacts-final.jsonl','windows-build-final.log')
a=items('windows-test-artifacts-final.jsonl');assert len(a)==20
before={Path(r['executable']).name:hashlib.sha256(Path(r['executable']).read_bytes()).hexdigest() for r in a};assert len(before)==20
normal=hashlib.sha256(exe.read_bytes()).hexdigest();(p/'windows-artifacts-before-run.json').write_text(json.dumps({'exeSha256':normal,'tests':before},indent=2)+'\n')
run(base,'windows-test-artifacts-confirmed.jsonl','windows-build-confirmed.log');confirmed=items('windows-test-artifacts-confirmed.jsonl');assert len(confirmed)==20 and all(r['fresh'] for r in confirmed)
assert before=={Path(r['executable']).name:hashlib.sha256(Path(r['executable']).read_bytes()).hexdigest() for r in confirmed}
probe=['python3',str(repo/'scripts/windows-probe.py'),'--host','elior@192.168.10.212','--identity','/config/.local/share/future-classroom-test-access/win11-192.168.10.212','--exe',str(exe),'--native-seconds','1']
run(probe+['--test-metadata',str(p/'windows-test-artifacts-confirmed.jsonl'),'--test-timeout-seconds','180','--voice-settings','--report',str(p/'windows-full-final.json')],'windows-full-final.log')
r=json.loads((p/'windows-full-final.json').read_text());assert r['status']==r['cleanup']=='PASS' and r['exeSha256']==normal and r['voiceSettingsNativeSteps']==36
assert len(r['tests'])==20 and all(t['exitCode']==0 for t in r['tests']);actual={t['name']:t['binarySha256'] for t in r['tests']};assert actual==before
uploaded={x['name']:x['sha256'] for x in r['uploadedArtifacts']};assert all(uploaded[k]==v for k,v in before.items())
passed=sum(int(m) for t in r['tests'] for m in re.findall(r'test result: ok\. (\d+) passed;', '\n'.join(t['output'])))
run(probe+['--audio-silent','--web-voice','--report',str(p/'modern-audio-final.json')],'modern-audio-final.log')
audio=json.loads((p/'modern-audio-final.json').read_text());assert all(audio[k]=='PASS' for k in ['status','cleanup','audioStatus','voicePolicyRuntimeStatus','webVoiceStatus']);assert audio['exeSha256']==normal==hashlib.sha256(exe.read_bytes()).hexdigest()
assert before=={Path(t['executable']).name:hashlib.sha256(Path(t['executable']).read_bytes()).hexdigest() for t in confirmed}
binding={'status':'PASS','testExecutables':20,'passed':passed,'exeSha256':normal,'tests':before,'inputSha256':json.loads((p/'source-inputs-final.json').read_text())['inputSha256'],'allFreshConfirmedBeforeExecution':True,'compiledUploadedExecutedCurrentSha256':'all equal','native36':'PASS','actualZeroPcmPlayingPolicyRetirementAndFreshExplicitJob':'PASS','scope':'all actual runtime evidence on Win11; Win7/Win10/MSVC/visible Seewo and production TLS remain separate'}
(p/'windows-artifact-binding-final.json').write_text(json.dumps(binding,indent=2)+'\n');print(json.dumps({k:v for k,v in binding.items() if k!='tests'}))
