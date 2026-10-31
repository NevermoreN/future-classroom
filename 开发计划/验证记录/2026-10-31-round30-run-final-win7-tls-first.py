from pathlib import Path
import subprocess,json,hashlib,base64,time,sys
root=Path.cwd();base=Path('/tmp/fc-round30-evidence');source=json.loads((base/'final-modern-package-first.json').read_text())['sourceAfter']
def digest(p):return hashlib.sha256(p.read_bytes()).hexdigest()
for name,row in source.items():assert digest(root/name)==row['sha256'],name
exe=Path('/tmp/fc-round30-final-win7-snapshot/fc-bootstrap.exe');assert digest(exe)=='67c5cbf7d27995e843f2a9ebc8e488ce48dc5e585b71bed045c8d66115f21e1b'
package=Path('/mnt/data/FutureClassroom-0.1.0-streaming-tls-dev.zip');assert digest(package)=='03f1f4ca2fa098688dedad32c5e3a274a21baba313ad2d1ff29c089b141caad6'
command=[sys.executable,'-B','scripts/windows-tls-business-probe.py','--ssh-target','future-classroom-win11','--identity','/config/.local/share/future-classroom-test-access/win11-192.168.10.212','--exe',str(exe),'--web-bundle',str(package),'--report',str(base/'final-win7-tls-on-win11-first.json')]
start=time.monotonic()
with (base/'final-win7-tls-first.stdout.log').open('xb') as out,(base/'final-win7-tls-first.stderr.log').open('xb') as err:r=subprocess.run(command,stdout=out,stderr=err)
report={'schemaVersion':1,'scope':'Supplemental GNU Win7 target actual finite TLS business run on the SAME Win11 machine only; actual Windows7/10 NOT_VALIDATED','command':command,'exitCode':r.returncode,'elapsedSeconds':time.monotonic()-start,'sourceReferenceSha256':digest(base/'final-modern-package-first.json'),'changedInputs':[n for n,row in source.items() if digest(root/n)!=row['sha256']],'exeSha256After':digest(exe),'webBundleSha256After':digest(package),'streams':{}}
for label in ['stdout','stderr']:
 p=base/('final-win7-tls-first.'+label+'.log');b=p.read_bytes();report['streams'][label]={'path':str(p),'bytes':len(b),'sha256':hashlib.sha256(b).hexdigest(),'base64':base64.b64encode(b).decode()}
with (base/'final-win7-tls-first-invocation.json').open('x') as f:json.dump(report,f,indent=2);f.write('\n')
print(json.dumps({k:report[k] for k in ['exitCode','elapsedSeconds','changedInputs']}),flush=True)
raise SystemExit(r.returncode)
