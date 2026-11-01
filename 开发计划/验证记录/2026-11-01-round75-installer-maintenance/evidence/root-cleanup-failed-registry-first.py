"""Bounded cleanup of a captured fixture whose original MSI was never invoked."""
from pathlib import Path
import base64,hashlib,json,subprocess
E=Path('/tmp/fc-round75-installer-maintenance/evidence')
src=E/'root-registry-recovery-readonly-first.json';old=json.loads(src.read_bytes())
root=old['root'];files=[{k:x[k] for k in ('path','bytes','sha256')} for x in old['files']]
script=r'''$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';$root=$c.root
if([IO.File]::ReadAllText((Join-Path $root '.owner')) -cne "future-classroom-owned-r75-registry-keypath/v1`n"){throw 'OWNER'}
if([IO.Path]::GetFileName($root) -cne 'FutureClassroom-r75-registry-keypath-ae966fa5484f4235b154b7b874ff728f'){throw 'ROOT'}
$actual=@(Get-ChildItem -LiteralPath $root -Recurse -Force);$regular=@($actual|Where-Object{-not $_.PSIsContainer});if($regular.Count -ne $c.files.Count){throw 'MEMBER_COUNT'}
foreach($x in $actual){if($x.Attributes -band [IO.FileAttributes]::ReparsePoint){throw 'REPARSE'};if($x.PSIsContainer -and $x.FullName -cne (Join-Path $root 'query-temp-Before')){throw 'UNKNOWN_DIRECTORY'}}
foreach($x in $regular){$name=$x.FullName.Substring($root.Length+1).Replace('\','/');$expect=@($c.files|Where-Object{$_.path -ceq $name});if($expect.Count -ne 1 -or $x.Length -ne $expect[0].bytes -or (Get-FileHash -LiteralPath $x.FullName -Algorithm SHA256).Hash.ToLowerInvariant() -cne $expect[0].sha256){throw 'CHANGED_BYTES'}}
$report=[IO.File]::ReadAllText((Join-Path $root 'controller-report.json'))|ConvertFrom-Json
if(-not $report.identityVerified -or $report.installAttempted -or $report.commands.Count -ne 1 -or $report.commands[0].name -cne 'query-Before' -or -not $report.commands[0].actualExit -or -not $report.commands[0].pipesComplete -or $report.commands[0].timedOut){throw 'NO_INSTALL_AND_OWNER_NOT_PROVEN'}
$active=@(Get-CimInstance Win32_Process|Where-Object{$_.ProcessId -ne $PID -and $_.CommandLine -and $_.CommandLine.IndexOf($root,[StringComparison]::OrdinalIgnoreCase) -ge 0});if($active.Count){throw 'ACTIVE'}
Remove-Item -LiteralPath $root -Recurse -Force
if(Test-Path -LiteralPath $root){throw 'ROOT_REMAINS'}
@{status='REMOVED_OWNED_FAILED_READONLY_PREFLIGHT_ROOT';root=$root;files=$regular.Count;msiEverExecuted=$false;activeOwnedBefore=0;absent=$true}|ConvertTo-Json -Compress'''
config=base64.b64encode(json.dumps({'root':root,'files':files},separators=(',',':')).encode()).decode()
prefix="$c=[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('"+config+"'))|ConvertFrom-Json;"
wrapper="[Console]::InputEncoding=[Text.Encoding]::UTF8;[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false);$s=[Console]::In.ReadToEnd();& ([ScriptBlock]::Create($s));"
encoded=base64.b64encode(wrapper.encode('utf-16-le')).decode()
cmd=['ssh','-i','/config/.local/share/future-classroom-test-access/win11-192.168.10.212','-o','BatchMode=yes','-o','ConnectTimeout=8','elior@192.168.10.212','powershell.exe -NoProfile -NonInteractive -EncodedCommand '+encoded]
p=E/'root-registry-failed-preflight-cleanup-first.json';out=E/'root-registry-failed-preflight-cleanup-first.stdout.log';err=E/'root-registry-failed-preflight-cleanup-first.stderr.log';assert not any(x.exists() for x in [p,out,err])
c=subprocess.run(cmd,input=(prefix+script).encode(),capture_output=True,timeout=25);out.write_bytes(c.stdout);err.write_bytes(c.stderr)
v={'status':'PASS' if c.returncode==0 else 'FAIL','actualExit':True,'exitCode':c.returncode,'command':cmd,'stdinScript':prefix+script,'readonlySourceSha256':hashlib.sha256(src.read_bytes()).hexdigest(),'scope':'Cleanup only after original bytes retained and code/report prove no MSI installation was attempted. Does not amend original FAIL or run uninstall for a product never installed.'}
for k,f in [('stdout',out),('stderr',err)]:v[k]={'path':str(f),'bytes':f.stat().st_size,'sha256':hashlib.sha256(f.read_bytes()).hexdigest()}
if c.returncode==0:v['actual']=json.loads(c.stdout)
p.write_text(json.dumps(v,indent=2)+'\n');print(json.dumps({k:v.get(k) for k in ('status','actualExit','exitCode','actual')}));raise SystemExit(c.returncode)
