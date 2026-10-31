from pathlib import Path
import re,json,hashlib,base64,difflib,subprocess
root=Path('/tmp/fc-round30-evidence');p=Path('scripts/windows-msvc-ci.ps1');raw=p.read_bytes();source=raw.decode()
before=json.loads((root/'ci-process-identity-startup-stage3-before-source.json').read_text());old=base64.b64decode(before['base64']).decode()
pattern=r'(?ms)^function Get-ActiveProcessIdentity\([^\n]*\) \{\n.*?^\}\n'
helper=re.search(pattern,source).group();oldhelper=re.search(pattern,old).group()
prior_source=base64.b64decode(json.loads((root/'ci-process-identity-startup-before-source.json').read_text())['base64']).decode()
prior_helper=re.search(pattern,prior_source).group()
probe=r'''# Read-only source AST + in-memory startup states. No process launch or file writes.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$source = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('__SOURCE__'))
$tokens = $null; $errors = $null
$null = [Management.Automation.Language.Parser]::ParseInput($source, [ref]$tokens, [ref]$errors)
if (@($errors).Count -ne 0) { throw 'Full CI source AST failed.' }
__HELPER__
__OLD_HELPER__
__STAGE2_HELPER__
$cases = [Collections.Generic.List[object]]::new()
function New-StartupProcess([string[]]$States, [int]$ExitAfterModuleReads = -1, [bool]$ExitedInitially = $false) {
    $fake = [pscustomobject]@{
        Id = 12345; StartTime = [DateTime]::new(2026,10,31,12,0,0,[DateTimeKind]::Utc)
        States = $States; ModuleReads = 0; Refreshes = 0; ExitAfterModuleReads = $ExitAfterModuleReads; ExitedInitially = $ExitedInitially
    }
    $fake | Add-Member -MemberType ScriptProperty -Name HasExited -Value {
        return ($this.ExitedInitially -or ($this.ExitAfterModuleReads -ge 0 -and $this.ModuleReads -ge $this.ExitAfterModuleReads))
    }
    $fake | Add-Member -MemberType ScriptProperty -Name MainModule -Value {
        $state = $this.States[[Math]::Min($this.Refreshes, $this.States.Length - 1)]
        $this.ModuleReads = $this.ModuleReads + 1
        switch ($state) {
            'null' { return $null }
            'valid' { return [pscustomobject]@{FileName='C:\Synthetic\tool.exe'} }
            'empty' { return [pscustomobject]@{FileName=''} }
            'throw' { throw [InvalidOperationException]::new('Synthetic module getter exception.') }
            default { throw 'Invalid memory fixture state.' }
        }
    }
    $fake | Add-Member -MemberType ScriptMethod -Name Refresh -Value {
        $this.Refreshes = $this.Refreshes + 1
    }
    return $fake
}
$prior = New-StartupProcess @('null')
$priorRejected = $false
try { $null = Get-PriorActiveProcessIdentity $prior } catch {
    $priorRejected = $_.Exception.Message -eq 'Live process main module is unavailable.'
}
if (-not $priorRejected -or $prior.ModuleReads -ne 1) { throw 'Prior startup-null failure was not reproduced.' }
$cases.Add([ordered]@{name='prior-live-null-reproduces-original-failure'; passed=$true})
$stage2Rejected = $false
try { $null = Get-Stage2ActiveProcessIdentity (New-StartupProcess @('empty')) ([DateTime]::UtcNow.AddSeconds(2)) } catch {
    $stage2Rejected = $_.Exception.Message -eq 'Live process image is unavailable.'
}
if (-not $stage2Rejected) { throw 'Stage2 live-empty failure was not reproduced.' }
$cases.Add([ordered]@{name='stage2-live-empty-reproduces-original-failure'; passed=$true})
function Assert-Identity([string]$Label, $Fake, [int]$Reads) {
    $identity = @(Get-ActiveProcessIdentity $Fake ([DateTime]::UtcNow.AddSeconds(2)))
    if ($identity.Count -ne 1 -or $identity[0].Count -ne 3 -or $identity[0].pid -ne $Fake.Id -or
        $identity[0].start_ticks -ne $Fake.StartTime.ToUniversalTime().Ticks -or
        $identity[0].image -cne 'C:\Synthetic\tool.exe' -or $Fake.ModuleReads -ne $Reads -or $Fake.Refreshes -ne ($Reads - 1)) {
        throw "Incorrect exact identity: $Label"
    }
    $cases.Add([ordered]@{name=$Label; passed=$true})
}
Assert-Identity 'startup-cached-null-refresh-then-valid-preserves-identity' (New-StartupProcess @('null','valid')) 2
Assert-Identity 'startup-cached-empty-refresh-then-valid-preserves-identity' (New-StartupProcess @('empty','valid')) 2
Assert-Identity 'live-valid-preserves-identity' (New-StartupProcess @('valid')) 1
foreach ($case in @(
    @{label='already-exited-no-module-query'; fake=(New-StartupProcess @('throw') -1 $true); reads=0},
    @{label='startup-null-then-exit'; fake=(New-StartupProcess @('null') 1); reads=1},
    @{label='valid-query-then-exit'; fake=(New-StartupProcess @('valid') 1); reads=1},
    @{label='getter-exception-then-exit'; fake=(New-StartupProcess @('throw') 1); reads=1}
)) {
    $identity = Get-ActiveProcessIdentity $case.fake ([DateTime]::UtcNow.AddSeconds(2))
    if ($null -ne $identity -or $case.fake.ModuleReads -ne $case.reads) { throw "Unexpected exit result: $($case.label)" }
    $cases.Add([ordered]@{name=$case.label; passed=$true})
}
foreach ($case in @(
    @{label='still-live-getter-exception-fails-closed'; state='throw'}
)) {
    $rejected = $false
    try { $null = Get-ActiveProcessIdentity (New-StartupProcess @($case.state)) ([DateTime]::UtcNow.AddSeconds(2)) } catch { $rejected = $true }
    if (-not $rejected) { throw "Unverified live image admitted: $($case.label)" }
    $cases.Add([ordered]@{name=$case.label; passed=$true})
}
$expired = New-StartupProcess @('null')
$deadlineRejected = $false
try { $null = Get-ActiveProcessIdentity $expired ([DateTime]::UtcNow.AddSeconds(-1)) } catch {
    $deadlineRejected = $_.Exception.Message -eq 'Live process identity deadline expired.'
}
if (-not $deadlineRejected -or $expired.ModuleReads -ne 0 -or $expired.Refreshes -ne 0) { throw 'Pre-exhausted absolute deadline was renewed.' }
$cases.Add([ordered]@{name='persistent-null-pre-exhausted-deadline-rejects-without-renewal'; passed=$true})
$emptyExpired = New-StartupProcess @('empty')
$emptyRejected = $false
try { $null = Get-ActiveProcessIdentity $emptyExpired ([DateTime]::UtcNow.AddSeconds(-1)) } catch {
    $emptyRejected = $_.Exception.Message -eq 'Live process identity deadline expired.'
}
if (-not $emptyRejected -or $emptyExpired.ModuleReads -ne 0 -or $emptyExpired.Refreshes -ne 0) {
    throw 'Pre-exhausted empty-image deadline was renewed.'
}
$cases.Add([ordered]@{name='persistent-empty-pre-exhausted-deadline-rejects-without-renewal'; passed=$true})
[ordered]@{
    status='PASS'; sourceSha256='__SHA__'; priorSourceSha256='__OLD_SHA__'; parserErrors=@($errors).Count
    powershell=$PSVersionTable.PSVersion.ToString(); caseCount=$cases.Count; cases=$cases.ToArray()
    scope='READONLY_AST_AND_IN_MEMORY_STARTUP_STATES_NO_PROCESS_LAUNCH_OR_FILE_WRITES'
} | ConvertTo-Json -Depth 8 -Compress
'''.replace('__SOURCE__',base64.b64encode(raw).decode()).replace('__HELPER__',helper).replace('__OLD_HELPER__',prior_helper.replace('function Get-ActiveProcessIdentity','function Get-PriorActiveProcessIdentity')).replace('__STAGE2_HELPER__',oldhelper.replace('function Get-ActiveProcessIdentity','function Get-Stage2ActiveProcessIdentity')).replace('__SHA__',hashlib.sha256(raw).hexdigest()).replace('__OLD_SHA__',before['sha256'])
probe_path=root/'ci-process-identity-startup-stage3-readonly-probe.ps1'
with probe_path.open('x') as f:f.write(probe)
oldcall='$identity = Get-ActiveProcessIdentity $process'
newcall='$identity = Get-ActiveProcessIdentity $process ($started.AddSeconds([Math]::Min(5, $TimeoutSeconds)))'
assert old.replace(oldhelper,helper)==source
assert hashlib.sha256(old.encode()).hexdigest()==before['sha256']=='db5509538852e784099a2bf67a40d7bc74c81866895083a79cad72b4c6b3f121'
assert helper.count('Start-Sleep -Milliseconds 1')==1
assert helper.count('$Process.Refresh()')==1
assert '$Deadline =' not in helper and '.AddSeconds' not in helper
# Pure Python state model only: explicit finite absolute deadline, no PowerShell runtime claim.
def model(states,exit_after=-1,exited=False,budget=5):
    reads=0;ticks=0
    def done():return exited or (exit_after>=0 and reads>=exit_after)
    if done():return None,reads
    while True:
        if done():return None,reads
        if ticks>=budget:raise TimeoutError('identity deadline')
        state=states[min(reads,len(states)-1)];reads+=1
        if state=='throw':
            if done():return None,reads
            raise ValueError('getter')
        if state=='valid':
            if done():return None,reads
            return ('exact-pid-start-image',reads)
        if done():return None,reads
        ticks+=1
checks=[]
for name,states,exit_after,want in [('null-to-valid',['null','valid'],-1,('exact-pid-start-image',2)),('empty-to-valid',['empty','valid'],-1,('exact-pid-start-image',2)),('null-to-exit',['null'],1,(None,1)),('valid',['valid'],-1,('exact-pid-start-image',1)),('query-exit',['valid'],1,(None,1)),('exception-exit',['throw'],1,(None,1))]:
    assert model(states,exit_after)==want;checks.append({'name':name,'passed':True})
for name,states,budget,error in [('expired-null',['null'],0,TimeoutError),('persistent-null',['null'],3,TimeoutError),('live-getter',['throw'],5,ValueError),('expired-empty',['empty'],0,TimeoutError),('persistent-empty',['empty'],3,TimeoutError)]:
    try:model(states,budget=budget)
    except error:pass
    else:raise AssertionError(name)
    checks.append({'name':name,'passed':True})
assert model(['throw'],exited=True)==(None,0);checks.append({'name':'already-exited','passed':True})
result=subprocess.run(['git','diff','--check','--','scripts/windows-msvc-ci.ps1'],capture_output=True);assert result.returncode==0
binding={'path':str(p),'bytes':len(raw),'sha256':hashlib.sha256(raw).hexdigest(),'base64':base64.b64encode(raw).decode()}
with (root/'ci-process-identity-startup-stage3-after-source.json').open('x') as f:json.dump(binding,f,indent=2);f.write('\n')
report={'status':'LOCAL_STATIC_AND_PYTHON_MODEL_PASS','powershellRuntime':'NOT_RUN_LOCAL_PWSH_UNAVAILABLE','readonlyProbe':'PREPARED_NOT_RUN','beforeSha256':before['sha256'],'afterSha256':binding['sha256'],'probeBinding':{'path':str(probe_path),'bytes':len(probe.encode()),'sha256':hashlib.sha256(probe.encode()).hexdigest()},'exactPatchScope':'Only null/empty-image retry branch inside existing identity helper; call and all other bytes unchanged.','absoluteDeadline':'original started + min(5, TimeoutSeconds); never reset within identity helper; Invoke-Logged timeout unchanged.','pythonStateModelOnly':checks,'diffCheck':{'exitCode':result.returncode,'stdout':result.stdout.decode(),'stderr':result.stderr.decode()},'fullDiff':''.join(difflib.unified_diff(old.splitlines(True),source.splitlines(True),fromfile='before/windows-msvc-ci.ps1',tofile='after/windows-msvc-ci.ps1'))}
with (root/'ci-process-identity-startup-stage3-static.json').open('x') as f:json.dump(report,f,indent=2);f.write('\n')
print(json.dumps({k:v for k,v in report.items() if k not in ('pythonStateModelOnly','fullDiff')}))
