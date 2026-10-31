from pathlib import Path
import subprocess,sys,json,hashlib,base64,time
root=Path.cwd(); ev=Path('/tmp/fc-round30-evidence')
inputs=['scripts/windows-tls-business-probe.py','scripts/windows-native-browser-observer.py','scripts/windows-native-browser-harness.mjs','scripts/windows-test-edge-owner.py','scripts/windows-tls-probe.py','scripts/windows-probe.py']
def sha(p): return hashlib.sha256(p.read_bytes()).hexdigest()
def snapshot(): return {name:{'bytes':(root/name).stat().st_size,'sha256':sha(root/name)} for name in inputs}
before=snapshot(); normal=Path('/tmp/fc-round30-windows-head-stage-snapshot/fc-bootstrap.exe'); bundle=Path('/tmp/fc-round30-native-browser-web.zip')
assert sha(normal)=='bbfa4b3818ed4aa15abdfb879e4d2f8339f73b36730b8e8e83b6d8d926574d6a'
assert sha(bundle)=='eee678d5313f56900b246486b839e280b6c04da3c760721a121ecec4205290c9'
assert before['scripts/windows-native-browser-harness.mjs']['sha256']=='6db58138366e67ba3b785541d975b5a24372ca2ab38ebda2182ddc76e26e907e'
command=[sys.executable,'-B','scripts/windows-tls-business-probe.py','--ssh-target','future-classroom-win11','--identity','/config/.local/share/future-classroom-test-access/win11-192.168.10.212','--exe',str(normal),'--web-bundle',str(bundle),'--browser','--report',str(ev/'native-browser-tls-second.json')]
start=time.monotonic()
with (ev/'native-browser-tls-second.stdout.log').open('xb') as out,(ev/'native-browser-tls-second.stderr.log').open('xb') as err:
 result=subprocess.run(command,stdout=out,stderr=err)
after=snapshot(); report={'schemaVersion':1,'scope':'Actual isolated Windows Edge UI against existing frozen native Rust EXE; original second attempt bytes retained','command':command,'exitCode':result.returncode,'elapsedSeconds':time.monotonic()-start,'sourceBefore':before,'sourceAfter':after,'changedInputs':[p for p in before if before[p]!=after[p]],'exeSha256Before': 'bbfa4b3818ed4aa15abdfb879e4d2f8339f73b36730b8e8e83b6d8d926574d6a','exeSha256After':sha(normal),'bundleSha256After':sha(bundle),'streams':{}}
for label in ['stdout','stderr']:
 p=ev/('native-browser-tls-second.'+label+'.log'); raw=p.read_bytes();report['streams'][label]={'path':str(p),'bytes':len(raw),'sha256':hashlib.sha256(raw).hexdigest(),'base64':base64.b64encode(raw).decode()}
with (ev/'native-browser-tls-second-invocation.json').open('x') as f:json.dump(report,f,ensure_ascii=False,indent=2);f.write('\n')
print(json.dumps({'exitCode':result.returncode,'elapsedSeconds':report['elapsedSeconds'],'changedInputs':report['changedInputs'],'report':str(ev/'native-browser-tls-second.json')}),flush=True)
raise SystemExit(result.returncode)
