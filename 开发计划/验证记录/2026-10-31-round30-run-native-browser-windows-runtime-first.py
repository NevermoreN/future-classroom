from pathlib import Path
import subprocess,sys,json,hashlib,base64,time
root=Path.cwd();ev=Path('/tmp/fc-round30-evidence');frozen=Path('/tmp/fc-round30-native-browser-windows-workspace-snapshot');binding=json.loads((ev/'native-browser-windows-workspace-frozen.json').read_text())
def digest(p):return hashlib.sha256(p.read_bytes()).hexdigest()
for p,sha in binding['sourceAfter'].items():assert digest(root/p)==sha,p
for row in binding['programs']:assert digest(Path(row['frozen']))==row['sha256']
assert digest(frozen/'fc-bootstrap.exe')==binding['normal']['sha256']
command=[sys.executable,'-B','scripts/windows-probe.py','--host','elior@192.168.10.212','--identity','/config/.local/share/future-classroom-test-access/win11-192.168.10.212','--exe',str(frozen/'fc-bootstrap.exe'),'--test-metadata',str(frozen/'tests.jsonl'),'--test-timeout-seconds','180','--report',str(ev/'native-browser-windows-runtime-first.json')]
start=time.monotonic();source_before=digest(root/'scripts/windows-probe.py')
with (ev/'native-browser-windows-runtime-first.stdout.log').open('xb') as out,(ev/'native-browser-windows-runtime-first.stderr.log').open('xb') as err:
 r=subprocess.run(command,stdout=out,stderr=err)
report={'schemaVersion':1,'scope':'All twenty exact current GNU x86 test executables run on authorized Windows11 only; formal Win7/Win10/MSVC NOT_VALIDATED','command':command,'exitCode':r.returncode,'elapsedSeconds':time.monotonic()-start,'runnerSourceBefore':source_before,'runnerSourceAfter':digest(root/'scripts/windows-probe.py'),'frozenBindingSha256':digest(ev/'native-browser-windows-workspace-frozen.json'),'inputChanges':[p for p,sha in binding['sourceAfter'].items() if digest(root/p)!=sha],'artifactChanges':[row['frozen'] for row in binding['programs'] if digest(Path(row['frozen']))!=row['sha256']],'streams':{}}
for kind in ['stdout','stderr']:
 p=ev/('native-browser-windows-runtime-first.'+kind+'.log');b=p.read_bytes();report['streams'][kind]={'path':str(p),'bytes':len(b),'sha256':hashlib.sha256(b).hexdigest(),'base64':base64.b64encode(b).decode()}
with (ev/'native-browser-windows-runtime-first-invocation.json').open('x') as f:json.dump(report,f,indent=2);f.write('\n')
print(json.dumps({k:report[k] for k in ['exitCode','elapsedSeconds','inputChanges','artifactChanges']}),flush=True)
raise SystemExit(r.returncode)
