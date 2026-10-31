from pathlib import Path
import subprocess,json,hashlib,base64
base=Path('/tmp/fc-round30-evidence');out=base/'ci-crt-env-windows-probe.json';assert not out.exists()
source=(base/'ci-crt-env-fix-ast-restore-probe.ps1').read_bytes()+b'\n'+(base/'ci-crt-env-flag-guard-extra.ps1').read_bytes()
preamble=b"$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false);"
full=preamble+source
frozen=base/'ci-crt-env-windows-stdin-probe.ps1';assert not frozen.exists();frozen.write_bytes(full)
wrapper="[Console]::InputEncoding=[Text.UTF8Encoding]::new($false);$probe=[Console]::In.ReadToEnd();& ([ScriptBlock]::Create($probe));"
encoded=base64.b64encode(wrapper.encode('utf-16-le')).decode()
command=['ssh','-o','BatchMode=yes','-o','ConnectTimeout=8','future-classroom-win11','powershell.exe -NoProfile -NonInteractive -EncodedCommand '+encoded]
x=subprocess.run(command,input=full,capture_output=True,timeout=40)
def raw(b):return {'bytes':len(b),'sha256':hashlib.sha256(b).hexdigest(),'base64':base64.b64encode(b).decode(),'scope':'COMPLETE'}
r={'scope':'Actual authorized Windows PS5.1 source AST plus process-only environment helpers. No CI, PS7, Rust/compiler/test EXE, persistent environment, file or registry mutation.','command':command,'stdinSource':{'path':str(frozen),'bytes':len(full),'sha256':hashlib.sha256(full).hexdigest()},'exitCode':x.returncode,'stdout':raw(x.stdout),'stderr':raw(x.stderr)}
if x.returncode==0:r['observations']=[json.loads(line) for line in x.stdout.decode('utf-8-sig').splitlines() if line.strip()]
out.write_text(json.dumps(r,indent=2)+'\n')
assert x.returncode==0, 'Inspect complete saved original stderr'
assert len(r['observations'])==2 and all(o['status']=='PASS' for o in r['observations'])
print(json.dumps({'exitCode':x.returncode,'restoreCases':r['observations'][0]['caseCount'],'flagGuardCases':r['observations'][1]['caseCount'],'sourceParserErrors':r['observations'][0]['parserErrors'],'emptyPS7Behavior':r['observations'][0]['emptyRestoration'],'processFlagsRestored':r['observations'][1]['processFlagsRestored']}),flush=True)
