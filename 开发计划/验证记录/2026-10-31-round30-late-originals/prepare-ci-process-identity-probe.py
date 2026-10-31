from pathlib import Path
import re,json,base64,hashlib,difflib,subprocess
root=Path('/tmp/fc-round30-evidence');p=Path('scripts/windows-msvc-ci.ps1');raw=p.read_bytes();source=raw.decode()
helper=re.search(r'(?ms)^function Get-ActiveProcessIdentity\([^\n]*\) \{\n.*?^\}\n',source).group()
probe=r'''# Read-only: full-source AST + in-memory process states; no process launch/file writes.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$source = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('__SOURCE__'))
$tokens = $null; $errors = $null
$null = [Management.Automation.Language.Parser]::ParseInput($source, [ref]$tokens, [ref]$errors)
if (@($errors).Count -ne 0) { throw 'Full CI source AST failed.' }
__HELPER__
$cases = [Collections.Generic.List[object]]::new()
function New-FakeProcess([bool[]]$ExitStates, $Module, [bool]$ThrowModule = $false) {
    $fake = [pscustomobject]@{
        Id = 12345; StartTime = [DateTime]::new(2026, 10, 31, 12, 0, 0, [DateTimeKind]::Utc)
        ExitStates = $ExitStates; ExitReads = 0; ModuleReads = 0; ModuleValue = $Module; ThrowModule = $ThrowModule
    }
    $fake | Add-Member -MemberType ScriptProperty -Name HasExited -Value {
        $value = $this.ExitStates[[Math]::Min($this.ExitReads, $this.ExitStates.Length - 1)]
        $this.ExitReads = $this.ExitReads + 1
        return $value
    }
    $fake | Add-Member -MemberType ScriptProperty -Name MainModule -Value {
        $this.ModuleReads = $this.ModuleReads + 1
        if ($this.ThrowModule) { throw [InvalidOperationException]::new('Synthetic module-query race.') }
        return $this.ModuleValue
    }
    return $fake
}
$module = [pscustomobject]@{FileName = 'C:\Synthetic\tool.exe'}
$old = New-FakeProcess @($true) $null
$oldRejected = $false; $oldMessageMatched = $false
try { $null = $old.MainModule.FileName } catch {
    $oldRejected = $true; $oldMessageMatched = $_.Exception.Message.Contains('FileName')
}
if (-not $oldRejected -or -not $oldMessageMatched) { throw 'Old null.FileName failure was not reproduced.' }
$cases.Add([ordered]@{name='old-null-module-reproduces-original-FileName-error'; passed=$true})
function Assert-Exited([string]$Label, $Fake, [int]$ExpectedModuleReads) {
    $identity = Get-ActiveProcessIdentity $Fake
    if ($null -ne $identity -or $Fake.ModuleReads -ne $ExpectedModuleReads) { throw "Unexpected exit identity: $Label" }
    $cases.Add([ordered]@{name=$Label; passed=$true})
}
Assert-Exited 'already-exited-never-queries-module' (New-FakeProcess @($true) $null $true) 0
Assert-Exited 'null-module-during-exit-skips-journal' (New-FakeProcess @($false, $true) $null) 1
Assert-Exited 'throwing-module-during-exit-skips-journal' (New-FakeProcess @($false, $true) $null $true) 1
Assert-Exited 'exit-after-valid-module-skips-journal' (New-FakeProcess @($false, $true) $module) 1
$live = New-FakeProcess @($false, $false) $module
$identity = @(Get-ActiveProcessIdentity $live)
if ($identity.Count -ne 1 -or $identity[0].pid -ne $live.Id -or
    $identity[0].start_ticks -ne $live.StartTime.ToUniversalTime().Ticks -or
    $identity[0].image -cne $module.FileName -or $identity[0].Count -ne 3) {
    throw 'Live identity lost the exact pid/start/image tuple.'
}
$cases.Add([ordered]@{name='live-valid-preserves-exact-three-part-identity'; passed=$true})
foreach ($bad in @(
    @{label='live-null-module-fails-closed'; value=$null; throws=$false},
    @{label='live-throwing-module-fails-closed'; value=$null; throws=$true},
    @{label='live-empty-image-fails-closed'; value=[pscustomobject]@{FileName=''}; throws=$false}
)) {
    $fake = New-FakeProcess @($false, $false) $bad.value $bad.throws
    $rejected = $false
    try { $null = Get-ActiveProcessIdentity $fake } catch { $rejected = $true }
    if (-not $rejected) { throw "Unverified live identity admitted: $($bad.label)" }
    $cases.Add([ordered]@{name=$bad.label; passed=$true})
}
[ordered]@{
    status='PASS'; sourceSha256='__SHA__'; powershell=$PSVersionTable.PSVersion.ToString(); parserErrors=@($errors).Count
    scope='READONLY_AST_AND_IN_MEMORY_FAKE_PROCESS_NO_PROCESS_LAUNCH_OR_FILE_WRITES'
    caseCount=$cases.Count; cases=$cases.ToArray()
} | ConvertTo-Json -Depth 8 -Compress
'''.replace('__SOURCE__',base64.b64encode(raw).decode()).replace('__HELPER__',helper).replace('__SHA__',hashlib.sha256(raw).hexdigest())
probepath=root/'ci-process-identity-readonly-probe.ps1'
with probepath.open('x') as f:f.write(probe)
before=json.loads((root/'ci-process-identity-before-source.json').read_text());old=base64.b64decode(before['base64']).decode()
oldblock='''        Write-NewJson (Join-Path $Logs "$Name.active.json") ([ordered]@{
            pid = $process.Id; start_ticks = $process.StartTime.ToUniversalTime().Ticks
            image = $process.MainModule.FileName
        })'''
newblock='''        $identity = Get-ActiveProcessIdentity $process
        if ($null -ne $identity) { Write-NewJson (Join-Path $Logs "$Name.active.json") $identity }'''
assert old.replace('function Invoke-Logged {',helper+'\nfunction Invoke-Logged {').replace(oldblock,newblock)==source
assert hashlib.sha256(old.encode()).hexdigest()==before['sha256']=='241b3afe56cd31075dc400e6808615f138b5f2b6c844e9c23a87cdeb0976cd81'
# Explicit Python state-model checks only, not a claim of PowerShell execution.
def identity(states,module=None,throws=False):
    states=iter(states);last=False
    def exited():
        nonlocal last
        last=next(states,last);return last
    if exited():return None
    try:
        if throws or not module:raise ValueError('module unavailable')
    except ValueError:
        if exited():return None
        raise
    if exited():return None
    return {'pid':12345,'start_ticks':123,'image':module}
checks=[]
for label,states,module,throws in [('already-exited',[True],None,True),('null-during-exit',[False,True],None,False),('throw-during-exit',[False,True],None,True),('exit-after-read',[False,True],'path',False)]:
    assert identity(states,module,throws) is None;checks.append({'name':label,'passed':True})
assert identity([False,False],'path')=={'pid':12345,'start_ticks':123,'image':'path'};checks.append({'name':'live-valid','passed':True})
for label,module,throws in [('null',None,False),('throw',None,True),('empty','',False)]:
    try:identity([False,False],module,throws)
    except ValueError:pass
    else:raise AssertionError(label)
    checks.append({'name':'live-'+label+'-rejects','passed':True})
result=subprocess.run(['git','diff','--check','--','scripts/windows-msvc-ci.ps1'],capture_output=True)
assert result.returncode==0
binding={'path':str(p),'bytes':len(raw),'sha256':hashlib.sha256(raw).hexdigest(),'base64':base64.b64encode(raw).decode()}
with (root/'ci-process-identity-after-source.json').open('x') as f:json.dump(binding,f,indent=2);f.write('\n')
report={'status':'LOCAL_STATIC_AND_PYTHON_STATE_MODEL_PASS','powerShellRuntime':'NOT_RUN_LOCAL_PWSH_UNAVAILABLE','readonlyProbe':'PREPARED_NOT_RUN','sourceBeforeSha256':before['sha256'],'sourceAfterSha256':binding['sha256'],'readonlyProbeBinding':{'path':str(probepath),'bytes':len(probe.encode()),'sha256':hashlib.sha256(probe.encode()).hexdigest()},'exactPatchScope':'Only helper insertion and post-Start active-journal identity block; timeout/log streams/finally/cleanup/source resolution all byte-identical.','pythonStateModelOnly':checks,'diffCheck':{'exitCode':result.returncode,'stdout':result.stdout.decode(),'stderr':result.stderr.decode()},'fullDiff':''.join(difflib.unified_diff(old.splitlines(True),source.splitlines(True),fromfile='before/windows-msvc-ci.ps1',tofile='after/windows-msvc-ci.ps1'))}
with (root/'ci-process-identity-static.json').open('x') as f:json.dump(report,f,indent=2);f.write('\n')
print(json.dumps({k:v for k,v in report.items() if k not in ('fullDiff','pythonStateModelOnly')}))
