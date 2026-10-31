import json,pathlib,base64,subprocess,hashlib,datetime
EV=pathlib.Path('/tmp/fc-round45-ci-input-domains-evidence'); owner=json.loads((EV/'windows-input-helpers-first.json').read_text())['processOwner']
source=r'''$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false);$expectedPid=__PID__;$expectedStart=__START__; $originalAlive=$false;$p=Get-Process -Id $expectedPid -ErrorAction SilentlyContinue;
if($null-ne$p){try{$originalAlive=($p.StartTime.ToUniversalTime().Ticks-eq$expectedStart)}finally{$p.Dispose()}};
$roots=@(Get-ChildItem -LiteralPath $env:TEMP -Directory -Filter 'FutureClassroom-Test-CIInputs-*' -Force|Select-Object -ExpandProperty Name);
@{originalProcessAlive=$originalAlive;matchingFixtureRootCount=$roots.Count;matchingFixtureRoots=$roots;sshd=(Get-Service sshd).Status.ToString();powershell=$PSVersionTable.PSVersion.ToString()}|ConvertTo-Json -Compress
'''.replace('__PID__',str(owner['pid'])).replace('__START__',str(owner['startTicks']))
cmd=['ssh','-T','-o','BatchMode=yes','-o','ConnectTimeout=10','future-classroom-win11','powershell.exe -NoLogo -NoProfile -NonInteractive -EncodedCommand '+base64.b64encode(source.encode('utf-16le')).decode()]
p=subprocess.run(cmd,capture_output=True,timeout=15)
parsed=json.loads(p.stdout.decode('utf-8-sig'));status='PASS' if p.returncode==0 and not parsed['originalProcessAlive'] and parsed['matchingFixtureRootCount']==0 and parsed['sshd']=='Running' and not p.stderr else 'FAIL'
r={'status':status,'utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'scope':'Read-only verification after actual frozen helper execution; no process termination/directory mutation/system changes.','command':cmd,'source':source,'exitCode':p.returncode,'observed':parsed,'originalProcessOwner':owner,'previousAuditNote':'First post-audit raw JSON already showed process absent, roots0, sshd Running. First local parser used invalid encoding alias then a strict stderr-empty criterion mistook PS module progress CLIXML for failure. Those originals remain. First post-audit exit code was not saved at subprocess point; its later report field exitCode0 was reconstructed, not independent evidence. This fresh audit records actual returncode directly.'}
for name,data in [('stdout',p.stdout),('stderr',p.stderr)]:
    (EV/f'windows-input-helpers-after-second.{name}.log').write_bytes(data)
    r[name]={'bytes':len(data),'sha256':hashlib.sha256(data).hexdigest(),'base64':base64.b64encode(data).decode()}
with (EV/'windows-input-helpers-after-second.json').open('x') as f:json.dump(r,f,ensure_ascii=False,indent=2);f.write('\n')
print(json.dumps({'status':status,'observed':parsed,'exitCode':p.returncode},ensure_ascii=False))
