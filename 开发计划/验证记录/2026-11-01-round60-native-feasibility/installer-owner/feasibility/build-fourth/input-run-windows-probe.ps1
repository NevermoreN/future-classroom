[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$FixtureDirectory,
    [Parameter(Mandatory = $true)][ValidatePattern('^[a-f0-9]{64}$')][string]$ExpectedMsiSha256,
    [Parameter(Mandatory = $true)][ValidatePattern('^[a-f0-9]{64}$')][string]$ExpectedGuardianSha256,
    [switch]$ExecuteOwnedProbe
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
# Explicit opt-in only. This file has NOT been executed on Windows by its author.
if (-not $ExecuteOwnedProbe) { throw 'No installation performed: ExecuteOwnedProbe is required after independent review.' }
$root = [IO.Path]::GetFullPath($FixtureDirectory)
$fixture = Get-Content -LiteralPath (Join-Path $root 'fixture.json') -Raw | ConvertFrom-Json
if ($fixture.nonce -cnotmatch '^[a-f0-9]{32}$') { throw 'Invalid owned fixture nonce.' }
$expectedRoot = Join-Path ([IO.Path]::GetTempPath()) ('FutureClassroomMsiOwner-' + $fixture.nonce)
if (-not [String]::Equals($root.TrimEnd('\'),[IO.Path]::GetFullPath($expectedRoot).TrimEnd('\'),[StringComparison]::OrdinalIgnoreCase)) { throw 'Only the exact nonce-owned TEMP directory is allowed.' }
if ((Get-Item -LiteralPath $root -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Owned fixture directory cannot be a reparse point.' }
$expectedRegistry = 'Software\FutureClassroom\Tests\MsiOwnerFeasibility\' + $fixture.nonce
if ($fixture.registryRoot -cne $expectedRegistry -or $fixture.payloadKey -cne ($expectedRegistry+'\Payload') -or $fixture.payloadValue -cne 'OwnedPayload') { throw 'Registry ownership scope differs.' }
if ($fixture.productCode -cnotmatch '^\{[A-F0-9-]{36}\}$' -or $fixture.upgradeCode -cnotmatch '^\{[A-F0-9-]{36}\}$') { throw 'Invalid fixed product identity.' }
$msi = Join-Path $root 'fixture.msi'; $guardianExe = Join-Path $root 'guardian.exe'
foreach($file in @($msi,$guardianExe)) { if ((Get-Item -LiteralPath $file -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Probe input cannot be reparse.' } }
if ((Get-FileHash -LiteralPath $msi -Algorithm SHA256).Hash.ToLowerInvariant() -cne $ExpectedMsiSha256 -or (Get-FileHash -LiteralPath $guardianExe -Algorithm SHA256).Hash.ToLowerInvariant() -cne $ExpectedGuardianSha256) { throw 'Exact pinned probe input hash differs.' }
$reportPath = Join-Path $root 'windows-observations-first.json'
if (Test-Path -LiteralPath $reportPath) { throw 'Exclusive original report already exists.' }
$script:unknown = $false; $script:records = [Collections.Generic.List[object]]::new()
$sid = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
$base = [Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::CurrentUser,[Microsoft.Win32.RegistryView]::Registry32)
$existing = $base.OpenSubKey($expectedRegistry,$false)
if ($existing) { $existing.Dispose();$base.Dispose();throw 'Original owned HKCU root already exists; refusing adoption.' }
$report = [ordered]@{schemaVersion=1;status='RUNNING';scope='Synthetic per-user single-MSI Join/End feasibility only, not product lifecycle acceptance';nonce=$fixture.nonce;productCode=$fixture.productCode;upgradeCode=$fixture.upgradeCode;registryRoot=$expectedRegistry;registryView='Registry32';expectedSid=$sid;msiSha256=$ExpectedMsiSha256;guardianSha256=$ExpectedGuardianSha256;operations=$script:records;cleanup='NOT_ATTEMPTED';ordinaryProductStartup='NOT_RUN'}
function Read-ProductState {
    $psi=[Diagnostics.ProcessStartInfo]::new();$psi.FileName=$guardianExe;$psi.Arguments='--product-state';$psi.UseShellExecute=$false;$psi.CreateNoWindow=$true;$psi.RedirectStandardOutput=$true;$psi.RedirectStandardError=$true
    $proc=[Diagnostics.Process]::new();$proc.StartInfo=$psi
    if(-not $proc.Start()){throw 'Owned read-only product query did not start.'}
    if(-not $proc.WaitForExit(5000)){$script:unknown=$true;throw 'Owned product query did not physically return; preserve all owned state.'}
    $stdout=$proc.StandardOutput.ReadToEnd();$stderr=$proc.StandardError.ReadToEnd()
    if($stdout.Length -gt 256 -or $stderr.Length -gt 1024){throw 'Owned query output exceeded bound.'}
    $row=@{pid=$proc.Id;actualExit=$true;exitCode=$proc.ExitCode;stdout=$stdout;stderr=$stderr};$proc.Dispose()
    if($row.exitCode -ne 0){throw 'Owned product-state query failed.'}
    $value=$stdout | ConvertFrom-Json
    if(@($value.PSObject.Properties.Name).Count -ne 3 -or $null -eq $value.productState -or $value.productCode -cne $fixture.productCode -or $value.nonce -cne $fixture.nonce){throw 'Compiled guardian product/nonce identity differs; no MSI mutation authorized.'}
    $row.productState=[int]$value.productState;return $row
}
function Read-Payload {
    $key=$base.OpenSubKey($fixture.payloadKey,$false)
    if(-not $key){return @{keyPresent=$false;valuePresent=$false}}
    try{$names=@($key.GetValueNames());$present=$names -ccontains 'OwnedPayload';if(-not $present){return @{keyPresent=$true;valuePresent=$false}};return @{keyPresent=$true;valuePresent=$true;kind=$key.GetValueKind('OwnedPayload').ToString();value=$key.GetValue('OwnedPayload')}}finally{$key.Dispose()}
}
function Snapshot-Evidence([string]$operation) {
    $key=$base.OpenSubKey($expectedRegistry+'\Evidence\'+$operation,$false);$rows=@()
    if(-not $key){return}
    try{
        $names=@($key.GetSubKeyNames());if($names.Count -gt 4){throw 'Too many guardian invocations for one operation.'}
        foreach($name in $names){
            if($name -cnotmatch '^Guardian-[0-9]{1,10}$'){throw 'Unexpected scoped evidence key.'}
            $child=$key.OpenSubKey($name,$false)
            try{$values=[ordered]@{key=$name};$vn=@($child.GetValueNames());if($vn.Count -gt 24){throw 'Too many guardian scalars.'};foreach($n in $vn){$v=$child.GetValue($n);if($v -is [string] -and $v.Length -gt 256){throw 'Guardian text exceeded bound.'};$values[$n]=$v};$rows+=@($values)}finally{$child.Dispose()}
        }
    }finally{$key.Dispose()};return $rows
}
function Invoke-OwnedMsi([string]$name,[string[]]$arguments,[int]$decision){
    if($script:unknown){throw 'Previous physical operation is unresolved; no new install or cleanup allowed.'}
    $operation=[Guid]::NewGuid().ToString('N');$control=$base.OpenSubKey($expectedRegistry,$true)
    try{$control.SetValue('CurrentOperation',$operation,[Microsoft.Win32.RegistryValueKind]::String);$control.SetValue('Decision',$decision,[Microsoft.Win32.RegistryValueKind]::DWord)}finally{$control.Dispose()}
    $ready=$null;$ack=$null
    $log=Join-Path $root ($name+'.msi.log');if(Test-Path -LiteralPath $log){throw 'Exclusive MSI log exists.'}
    $beforeState=Read-ProductState;$beforePayload=Read-Payload
    $row=[ordered]@{name=$name;operation=$operation;decision=$decision;productBefore=$beforeState;payloadBefore=$beforePayload;guardianObserved=@();physicalMsiExit=$false;completion='RUNNING';log=$log};$script:records.Add($row)
    $watch=[Diagnostics.Stopwatch]::StartNew();$deadlineMillis=60000;$guardian=$null;$proc=$null;$procStarted=$false
    try{
    $createdReady=$false;$createdAck=$false
    $ready=[Threading.EventWaitHandle]::new($false,[Threading.EventResetMode]::ManualReset,('Global\FCMSI-'+$fixture.nonce+'-'+$operation+'-ready'),[ref]$createdReady)
    $ack=[Threading.EventWaitHandle]::new($false,[Threading.EventResetMode]::ManualReset,('Global\FCMSI-'+$fixture.nonce+'-'+$operation+'-ack'),[ref]$createdAck)
    if(-not $createdReady -or -not $createdAck){throw 'Refusing preexisting event ownership.'}
        $all=@($arguments)+@('/qn','/norestart','REBOOT=ReallySuppress','/l*v',$log)
        foreach($arg in $all){if($arg.Contains('"') -or $arg.Contains("`r") -or $arg.Contains("`n")){throw 'Unsafe argument representation.'}}
        $psi=[Diagnostics.ProcessStartInfo]::new();$psi.FileName=Join-Path ([Environment]::GetFolderPath('System')) 'msiexec.exe';$psi.Arguments=(($all|ForEach-Object{'"'+$_+'"'}) -join ' ');$psi.UseShellExecute=$false;$psi.CreateNoWindow=$true
        $proc=[Diagnostics.Process]::new();$proc.StartInfo=$psi;if(-not $proc.Start()){throw 'Owned msiexec start failed.'};$procStarted=$true;$row.msiPid=$proc.Id;$row.msiStartFileTime=$proc.StartTime.ToUniversalTime().ToFileTimeUtc()
        while(-not $proc.HasExited -and $watch.ElapsedMilliseconds -lt $deadlineMillis){
            if((Test-Path -LiteralPath $log) -and (Get-Item -LiteralPath $log).Length -gt 16MB){$script:unknown=$true;throw 'MSI log exceeded original finite bound; unresolved process preserved.'}
            if($null -eq $guardian -and $ready.WaitOne(0)){
                $entries=@(Snapshot-Evidence $operation);if($entries.Count -ne 1){throw 'Ready did not identify exactly one original guardian.'};$entry=$entries[0]
                if($entry.ActualSid -cne $sid){throw 'Guardian token SID differs; no ACK.'}
                $guardian=[Diagnostics.Process]::GetProcessById([int]$entry.Pid);$null=$guardian.Handle
                if($guardian.StartTime.ToUniversalTime().ToFileTimeUtc() -ne [long]$entry.CreationFileTime){throw 'Guardian creation identity differs.'}
                $image=$guardian.MainModule.FileName;if([string]::IsNullOrWhiteSpace($image)){throw 'Guardian image unknown.'}
                if((Get-FileHash -LiteralPath $image -Algorithm SHA256).Hash.ToLowerInvariant() -cne $ExpectedGuardianSha256){throw 'Actual embedded guardian bytes differ.'}
                $row.guardianObserved=@(@{pid=$guardian.Id;creationFileTime=[long]$entry.CreationFileTime;originalProcessHandleRetained=$true;actualImageSha256=$ExpectedGuardianSha256})
                $null=$ack.Set()
            }
            Start-Sleep -Milliseconds 10
        }
        if(-not $proc.HasExited){$script:unknown=$true;$row.completion='UNKNOWN_MSI_STILL_RUNNING';throw 'Original msiexec did not physically return; no cleanup/retry.'}
        $proc.WaitForExit();$row.physicalMsiExit=$true;$row.msiExitCode=$proc.ExitCode
        if($guardian){$remaining=[Math]::Max(0,$deadlineMillis-[int]$watch.ElapsedMilliseconds);if(-not $guardian.WaitForExit($remaining)){$script:unknown=$true;throw 'Original guardian did not physically return.'};$row.guardianObserved[0].actualExit=$true;$row.guardianObserved[0].exitCode=$guardian.ExitCode}
        $row.guardianEvidence=@(Snapshot-Evidence $operation);$row.productAfter=Read-ProductState;$row.payloadAfter=Read-Payload;$row.elapsedMillis=$watch.ElapsedMilliseconds
        $row.logBytes=if(Test-Path -LiteralPath $log){(Get-Item -LiteralPath $log).Length}else{0};$row.logSha256=if(Test-Path -LiteralPath $log){(Get-FileHash -LiteralPath $log -Algorithm SHA256).Hash.ToLowerInvariant()}else{$null}
        foreach($entry in $row.guardianEvidence){
            if($entry.Contains('EndEntered') -and ($entry.EndEntered -eq 1) -and (-not $entry.Contains('EndReturned') -or $entry.EndReturned -ne 1 -or -not $entry.Contains('EndReturn') -or $entry.EndReturn -ne 0)){$script:unknown=$true;$row.completion='UNKNOWN_TRANSACTION_END';throw 'End transaction not confirmed; preserve scope without cleanup.'}
        }
        $row.completion='OBSERVED_ORIGINALS_ONLY';return $row
    }catch{
        if(($procStarted -and -not $proc.HasExited) -or ($guardian -and -not $guardian.HasExited)){$script:unknown=$true;$row.completion='UNKNOWN_ORIGINAL_PROCESS_RETAINED'}
        throw
    }finally{
        if($procStarted -and $proc.HasExited){$proc.Dispose()}
        if($guardian -and $guardian.HasExited){$guardian.Dispose()}
        if($ready){$ready.Dispose()};if($ack){$ack.Dispose()}
    }
}
try{
    $initial=Read-ProductState;$report.productBefore=$initial
    if($initial.productState -ne -1){throw 'Synthetic random product already known; refusing adoption.'}
    $control=$base.CreateSubKey($expectedRegistry);try{$control.SetValue('ProbeNonce',$fixture.nonce,[Microsoft.Win32.RegistryValueKind]::String);$control.SetValue('OwnerSid',$sid,[Microsoft.Win32.RegistryValueKind]::String)}finally{$control.Dispose()}
    $null=Invoke-OwnedMsi 'install-commit' @('/i',$msi) 1
    $null=Invoke-OwnedMsi 'repair-commit' @('/famus',$fixture.productCode) 1
    $null=Invoke-OwnedMsi 'uninstall-commit' @('/x',$fixture.productCode) 1
    $null=Invoke-OwnedMsi 'install-rollback' @('/i',$msi) 0
    $report.status='OBSERVED_FOUR_DIRECT_OPERATIONS_NOT_PRODUCT_ACCEPTANCE'
}catch{
    $report.status='FAIL_OR_UNKNOWN_RETAINED';$report.errorKind=$_.Exception.GetType().Name;$report.errorMessage='Owned MSI feasibility did not complete; original scoped observations/logs retained.'
}finally{
    # Never compensate an unknown transaction or delete another product/key.
    if(-not $script:unknown){
        try{
            $state=Read-ProductState
            if($state.productState -eq 5){$null=Invoke-OwnedMsi 'cleanup-uninstall' @('/x',$fixture.productCode) 1;$state=Read-ProductState}
            $report.finalProductState=$state
            if($state.productState -eq -1){
                $report.evidenceBeforeOwnRegistryCleanup=@($script:records|ForEach-Object{@{name=$_.name;operation=$_.operation;evidence=@(Snapshot-Evidence $_.operation)}})
                $owned=$base.OpenSubKey($expectedRegistry,$false)
                if($owned){try{if($owned.GetValue('ProbeNonce') -cne $fixture.nonce -or $owned.GetValue('OwnerSid') -cne $sid){throw 'Ownership marker differs.'}}finally{$owned.Dispose()};$base.DeleteSubKeyTree($expectedRegistry,$false)}
                $report.cleanup='PASS_OWN_PRODUCT_ABSENT_AND_NONCE_REGISTRY_REMOVED'
            }else{$report.cleanup='RETAINED_PRODUCT_STATE_NOT_ABSENT'}
        }catch{$report.cleanup='FAIL_OR_UNKNOWN_RETAINED'}
    }else{$report.cleanup='UNKNOWN_LIVE_OPERATION_RETAINED_NO_CLEANUP'}
    $report.unknownPhysicalOperation=$script:unknown;$base.Dispose()
    $json=$report|ConvertTo-Json -Depth 16
    [IO.File]::WriteAllText($reportPath,$json,[Text.UTF8Encoding]::new($false))
}
Write-Output ('MSI_FEASIBILITY_REPORT '+$reportPath)
if($report.status -ne 'OBSERVED_FOUR_DIRECT_OPERATIONS_NOT_PRODUCT_ACCEPTANCE' -or $report.cleanup -ne 'PASS_OWN_PRODUCT_ABSENT_AND_NONCE_REGISTRY_REMOVED'){exit 1}
