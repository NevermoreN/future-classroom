from pathlib import Path
import runpy,json,stat,sys
repo=Path.cwd(); p=Path('/tmp/fc-round29-evidence');sys.dont_write_bytecode=True
m=runpy.run_path(str(repo/'scripts/windows-probe.py'));key=Path('/config/.local/share/future-classroom-test-access/win11-192.168.10.212');r=m['Remote']('elior@192.168.10.212',key)
script=r'''$dirs=@(Get-ChildItem -LiteralPath $env:TEMP -Directory |Where-Object{$_.Name -match '^FutureClassroom-(RustProbe-|Test-)'}|Select-Object -ExpandProperty Name);$processes=@(Get-CimInstance Win32_Process|Where-Object{($_.ExecutablePath-and $_.ExecutablePath-match 'FutureClassroom-(RustProbe-|Test-)')-or ($_.Name-like 'fc-bootstrap*')}|ForEach-Object{@{name=$_.Name;pid=$_.ProcessId}});$tasks=@(Get-ScheduledTask|Where-Object{$_.TaskName-like 'FutureClassroom-RustProbe-*'}|Select-Object -ExpandProperty TaskName);$os=Get-CimInstance Win32_OperatingSystem;@{sshd=(Get-Service sshd).Status.ToString();build=$os.BuildNumber;os=$os.Caption;sshPublicAuthorizationSha256=(Get-FileHash -LiteralPath "$env:ProgramData\ssh\administrators_authorized_keys" -Algorithm SHA256).Hash.ToLowerInvariant();scratchDirectories=$dirs;ownedProcesses=$processes;ownedTasks=$tasks}|ConvertTo-Json -Depth 5 -Compress'''
report=json.loads(r.powershell(script));report['projectRunReadOnlySnapshot']=json.loads(r.powershell(m['project_run_snapshot_script']()));report['persistentKeyMode']=oct(stat.S_IMODE(key.stat().st_mode));report['authentication']='PASS'
assert report['sshd']=='Running' and report['persistentKeyMode']=='0o600'
assert not report['scratchDirectories'] and not report['ownedProcesses'] and not report['ownedTasks']
name=sys.argv[1] if len(sys.argv)>1 else 'windows-before.json'
if name!='windows-before.json':
 before=json.loads((p/'windows-before.json').read_text());assert report['projectRunReadOnlySnapshot']==before['projectRunReadOnlySnapshot'] and report['sshPublicAuthorizationSha256']==before['sshPublicAuthorizationSha256'];report['publicAuthorizationUnchanged']=report['projectRunUnchanged']=True
report['status']='PASS';(p/name).write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n');print(json.dumps(report,ensure_ascii=False))
