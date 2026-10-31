import json,runpy,subprocess,stat,sys
from pathlib import Path
namespace=runpy.run_path('scripts/windows-probe.py')
identity=Path('/config/.local/share/future-classroom-test-access/win11-192.168.10.212')
remote=namespace['Remote']('elior@192.168.10.212',identity)
pub=' '.join(subprocess.check_output(['ssh-keygen','-y','-f',str(identity)],text=True).split()[:2])
script=r"""
$pub=EXPECTED_PUB;
$tasks=@(Get-ScheduledTask|Where-Object{$_.TaskName-like 'FutureClassroom-RustProbe-*'});
$directories=@(Get-ChildItem -LiteralPath $env:TEMP -Directory -ErrorAction SilentlyContinue|Where-Object{$_.Name-like 'FutureClassroom-RustProbe-*'});
$processes=@(Get-CimInstance Win32_Process|Where-Object{$_.ExecutablePath-and $_.ExecutablePath-like '*\FutureClassroom-RustProbe-*\*'});
$scratch=@(Get-ChildItem -LiteralPath 'HKCU:\Software'|Where-Object{$_.PSChildName-like 'FutureClassroom-TestRun-R21-*'});
$classes=@(Get-ChildItem -LiteralPath 'HKCU:\Software\Classes'|Where-Object{$_.PSChildName-like 'FutureClassroom.Test.*'});
$run=Get-ItemProperty -LiteralPath 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' -Name FutureClassroom -ErrorAction SilentlyContinue;
$keys=Get-Content -LiteralPath (Join-Path $env:ProgramData 'ssh\administrators_authorized_keys');
@{testTaskCount=$tasks.Count;testDirectoryCount=$directories.Count;testProcessCount=$processes.Count;scratchRunLeafCount=$scratch.Count;testAssociationCount=$classes.Count;realRunPresent=($null-ne $run);sshd=(Get-Service sshd).Status.ToString();persistentPublicKeyRetained=(@($keys|Where-Object{$_.Trim()-eq $pub-or $_.Trim().StartsWith($pub+' ')}).Count-gt 0)}|ConvertTo-Json -Compress
""".replace('EXPECTED_PUB',namespace['ps_literal'](pub))
data=json.loads(remote.powershell(script));data['persistentIdentityAuthenticated']=True;data['privateKeyMode']=oct(stat.S_IMODE(identity.stat().st_mode));data['scope']='Read-only project test residue and persistent SSH audit; no mutation or cleanup performed'
assert all(data[k]==0 for k in ['testTaskCount','testDirectoryCount','testProcessCount','scratchRunLeafCount','testAssociationCount']),data
assert data['sshd']=='Running' and data['persistentPublicKeyRetained'] and data['privateKeyMode']=='0o600',data
data['status']='PASS';Path(sys.argv[1]).write_text(json.dumps(data,ensure_ascii=False,indent=2)+'\n');print(json.dumps(data))
