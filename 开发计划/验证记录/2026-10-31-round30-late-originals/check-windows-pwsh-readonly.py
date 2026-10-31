from pathlib import Path
import subprocess,json,base64,hashlib,datetime
out=Path('/tmp/fc-round30-evidence/windows-pwsh-availability-readonly.json');assert not out.exists()
source="$ErrorActionPreference='Stop';[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false);$cmd=Get-Command pwsh.exe -CommandType Application -ErrorAction SilentlyContinue|Select-Object -First 1;@{windowsPowerShell=$PSVersionTable.PSVersion.ToString();pwshPath=$(if($cmd){$cmd.Source}else{$null});scope='Read-only command resolution only; no upload, process environment/user environment mutation, compiler or test execution'}|ConvertTo-Json -Compress"
encoded=base64.b64encode(source.encode('utf-16-le')).decode();cmd=['ssh','-o','BatchMode=yes','-o','ConnectTimeout=8','future-classroom-win11','powershell.exe -NoProfile -NonInteractive -EncodedCommand '+encoded]
x=subprocess.run(cmd,capture_output=True,timeout=30)
def raw(b):return {'bytes':len(b),'sha256':hashlib.sha256(b).hexdigest(),'base64':base64.b64encode(b).decode()}
r={'command':cmd,'powershellSource':source,'exitCode':x.returncode,'stdout':raw(x.stdout),'stderr':raw(x.stderr)}
if x.returncode==0:r['observed']=json.loads(x.stdout.decode('utf-8-sig'))
out.write_text(json.dumps(r,indent=2)+'\n');print(json.dumps({'exitCode':x.returncode,'observed':r.get('observed')},ensure_ascii=False))
raise SystemExit(x.returncode)
