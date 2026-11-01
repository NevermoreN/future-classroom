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
$expectedRegistry = 'Software\FutureClassroom-MsiGuardian-Test-' + $fixture.nonce
if ($fixture.registryRoot -cne $expectedRegistry -or $fixture.payloadKey -cne ($expectedRegistry+'\Payload') -or $fixture.payloadValue -cne 'OwnedPayload') { throw 'Registry ownership scope differs.' }
if ($fixture.productCode -cnotmatch '^\{[A-F0-9-]{36}\}$' -or $fixture.upgradeCode -cnotmatch '^\{[A-F0-9-]{36}\}$') { throw 'Invalid fixed product identity.' }
$msi = Join-Path $root 'fixture.msi'; $guardianExe = Join-Path $root 'guardian.exe'
foreach($file in @($msi,$guardianExe)) { if ((Get-Item -LiteralPath $file -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Probe input cannot be reparse.' } }
if ((Get-FileHash -LiteralPath $msi -Algorithm SHA256).Hash.ToLowerInvariant() -cne $ExpectedMsiSha256 -or (Get-FileHash -LiteralPath $guardianExe -Algorithm SHA256).Hash.ToLowerInvariant() -cne $ExpectedGuardianSha256) { throw 'Exact pinned probe input hash differs.' }
$reportPath = Join-Path $root 'windows-observations-first.json'
if (Test-Path -LiteralPath $reportPath) { throw 'Exclusive original report already exists.' }
$script:ownerKey = $null; $script:ownerHandle = $null; $script:ownerNew = $false; $script:markersWritten = $false
$script:unknown = $false; $script:records = [Collections.Generic.List[object]]::new()
$sid = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
$base = [Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::CurrentUser,[Microsoft.Win32.RegistryView]::Registry32)
$report = [ordered]@{schemaVersion=1;status='RUNNING';scope='Synthetic per-user single-MSI Join/End feasibility only, not product lifecycle acceptance';nonce=$fixture.nonce;productCode=$fixture.productCode;upgradeCode=$fixture.upgradeCode;registryRoot=$expectedRegistry;registryView='Registry32';expectedSid=$sid;msiSha256=$ExpectedMsiSha256;guardianSha256=$ExpectedGuardianSha256;operations=$script:records;cleanup='NOT_ATTEMPTED';ordinaryProductStartup='NOT_RUN'}
# Native disposition, not a prior Absent observation, establishes this fixture's NEW root.
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;
namespace FcMsiFixtureOwnerV2 {
 public sealed class Creation {
  public int ParentOpenError, CreateError, ParentCloseError;
  public uint Disposition;
  public SafeRegistryHandle Key;
 }
 public static class Registry {
  [DllImport("advapi32.dll", CharSet=CharSet.Unicode, ExactSpelling=true)]
  static extern int RegOpenKeyExW(IntPtr key, string subkey, uint options, uint access, out IntPtr result);
  [DllImport("advapi32.dll", CharSet=CharSet.Unicode, ExactSpelling=true)]
  static extern int RegCreateKeyExW(IntPtr key, string subkey, uint reserved, string cls, uint options, uint access, IntPtr security, out IntPtr result, out uint disposition);
  [DllImport("advapi32.dll", ExactSpelling=true)] static extern int RegCloseKey(IntPtr key);
  public static Creation Create(string leaf) {
   if (leaf == null || !System.Text.RegularExpressions.Regex.IsMatch(leaf, @"\AFutureClassroom-MsiGuardian-Test-[a-f0-9]{32}\z")) throw new ArgumentException("Invalid fixture leaf.");
   Creation r = new Creation(); IntPtr parent;
   r.ParentOpenError = RegOpenKeyExW(new IntPtr(unchecked((int)0x80000001)), "Software", 0, 0x2001f | 0x200, out parent);
   if (r.ParentOpenError != 0) return r;
   try {
    IntPtr child; uint disposition;
    r.CreateError = RegCreateKeyExW(parent, leaf, 0, null, 0, 0x2001f | 0x200, IntPtr.Zero, out child, out disposition);
    r.Disposition = disposition;
    if (r.CreateError == 0) r.Key = new SafeRegistryHandle(child, true);
   } finally { r.ParentCloseError = RegCloseKey(parent); }
   return r;
  }
 }
}
'@
function Assert-OriginalsReturned {
    if($script:unknown){throw 'Physical operation unresolved.'}
    foreach($r in $script:records){
        if($r.completion -cne 'OBSERVED_ORIGINALS_ONLY' -or -not $r.physicalMsiExit -or $r.guardianObserved.Count -ne 1){throw 'Original MSI/guardian not fully observed.'}
        $g=$r.guardianObserved[0]
        if(-not $g.ContainsKey('actualExit') -or -not $g.actualExit){throw 'Original guardian did not physically return.'}
    }
}
function Assert-KnownOwnerTree {
    if(-not $script:ownerNew -or -not $script:markersWritten -or $null -eq $script:ownerKey){throw 'No fully marked original NEW root.'}
    $k=$script:ownerKey
    foreach($n in @('ProbeNonce','OwnerSid')){if($k.GetValueKind($n) -ne [Microsoft.Win32.RegistryValueKind]::String){throw 'Owner marker kind differs.'}}
    if($k.GetValue('ProbeNonce') -cne $fixture.nonce -or $k.GetValue('OwnerSid') -cne $sid){throw 'Original owner markers differ.'}
    foreach($n in @($k.GetValueNames())){if(@('ProbeNonce','OwnerSid','CurrentOperation','Decision') -cnotcontains $n){throw 'Unknown root value; retain scope.'}}
    if($script:records.Count -gt 0){
        $last=$script:records[$script:records.Count-1]
        if($k.GetValueKind('CurrentOperation') -ne [Microsoft.Win32.RegistryValueKind]::String -or $k.GetValue('CurrentOperation') -cne $last.operation -or $k.GetValueKind('Decision') -ne [Microsoft.Win32.RegistryValueKind]::DWord -or $k.GetValue('Decision') -ne $last.decision){throw 'Operation control differs from original record.'}
    }
    foreach($n in @($k.GetSubKeyNames())){if(@('Evidence','Payload') -cnotcontains $n){throw 'Unknown root child; retain scope.'}}
    $payload=$k.OpenSubKey('Payload',$false)
    if($payload){try{if(@($payload.GetSubKeyNames()).Count -ne 0){throw 'Unknown payload child.'};foreach($n in @($payload.GetValueNames())){if($n -cne 'OwnedPayload' -or $payload.GetValueKind($n) -ne [Microsoft.Win32.RegistryValueKind]::DWord -or $payload.GetValue($n) -ne 1){throw 'Unknown payload content.'}}}finally{$payload.Dispose()}}
    $ev=$k.OpenSubKey('Evidence',$false)
    if($ev){try{
        if(@($ev.GetValueNames()).Count -ne 0){throw 'Unknown evidence-parent values.'}
        $names=@($ev.GetSubKeyNames());if($names.Count -gt 5){throw 'Evidence count exceeded.'}
        foreach($name in $names){
            $rows=@($script:records|Where-Object{$_.operation -ceq $name});if($rows.Count -ne 1){throw 'Unknown evidence operation.'}
            $op=$ev.OpenSubKey($name,$false)
            try{
                if(@($op.GetValueNames()).Count -ne 0){throw 'Unknown operation values.'}
                $kids=@($op.GetSubKeyNames());if($kids.Count -ne 1 -or $rows[0].guardianObserved.Count -ne 1 -or $kids[0] -cne ('Guardian-'+$rows[0].guardianObserved[0].pid)){throw 'Unknown evidence process.'}
                $g=$op.OpenSubKey($kids[0],$false)
                try{
                    if(@($g.GetSubKeyNames()).Count -ne 0){throw 'Unknown guardian child.'}
                    $dwords=@('Pid','ArgCount','TransactionId','RequestedDecision','EventOpenError','ObserveAckWait','JoinExportPresent','EndExportPresent','ExportLookupError','JoinReturn','OwnerEventBeforeEnd','EndEntered','EndReturn','EndReturned','AboutToReturnCode')
                    foreach($n in @($g.GetValueNames())){
                        $kind=$g.GetValueKind($n);$v=$g.GetValue($n)
                        if($dwords -ccontains $n){if($kind -ne [Microsoft.Win32.RegistryValueKind]::DWord){throw 'Guardian DWORD kind differs.'}}
                        elseif($n -ceq 'CreationFileTime'){if($kind -ne [Microsoft.Win32.RegistryValueKind]::QWord){throw 'Guardian creation kind differs.'}}
                        elseif($n -ceq 'ActualSid'){if($kind -ne [Microsoft.Win32.RegistryValueKind]::String -or $v -cne $sid){throw 'Guardian SID differs.'}}
                        elseif($n -ceq 'Stage'){if($kind -ne [Microsoft.Win32.RegistryValueKind]::String -or @('Entered','Joined','EndReturned') -cnotcontains $v){throw 'Unknown guardian stage.'}}
                        else{throw 'Unknown guardian value.'}
                    }
                }finally{$g.Dispose()}
            }finally{$op.Dispose()}
        }
    }finally{$ev.Dispose()}}
}
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
    Assert-OriginalsReturned
    if(-not $script:ownerNew -or -not $script:markersWritten){throw 'No fully marked NEW owner; no MSI allowed.'}
    $operation=[Guid]::NewGuid().ToString('N');$control=$script:ownerKey
    $control.SetValue('CurrentOperation',$operation,[Microsoft.Win32.RegistryValueKind]::String);$control.SetValue('Decision',$decision,[Microsoft.Win32.RegistryValueKind]::DWord)
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
        if($null -eq $guardian){$script:unknown=$true;$row.completion='UNKNOWN_GUARDIAN_NOT_RETAINED';throw 'Exact original guardian was not retained; no further MSI or cleanup.'}
        $row.guardianEvidence=@(Snapshot-Evidence $operation);$row.productAfter=Read-ProductState;$row.payloadAfter=Read-Payload;$row.elapsedMillis=$watch.ElapsedMilliseconds
        $row.logBytes=if(Test-Path -LiteralPath $log){(Get-Item -LiteralPath $log).Length}else{0};$row.logSha256=if(Test-Path -LiteralPath $log){(Get-FileHash -LiteralPath $log -Algorithm SHA256).Hash.ToLowerInvariant()}else{$null}
        foreach($entry in $row.guardianEvidence){
            if($entry.Contains('EndEntered') -and ($entry.EndEntered -eq 1) -and (-not $entry.Contains('EndReturned') -or $entry.EndReturned -ne 1 -or -not $entry.Contains('EndReturn') -or $entry.EndReturn -ne 0)){$script:unknown=$true;$row.completion='UNKNOWN_TRANSACTION_END';throw 'End transaction not confirmed; preserve scope without cleanup.'}
        }
        if($row.guardianEvidence.Count -ne 1 -or -not $row.guardianEvidence[0].Contains('EndReturned') -or $row.guardianEvidence[0].EndReturned -ne 1 -or -not $row.guardianEvidence[0].Contains('EndReturn') -or $row.guardianEvidence[0].EndReturn -ne 0){$script:unknown=$true;throw 'Exact transaction End success not observed; no next MSI.'}
        $row.completion='OBSERVED_ORIGINALS_ONLY';return $row
    }catch{
        if($procStarted){$script:unknown=$true;$row.completion='UNKNOWN_ORIGINAL_PROCESS_RETAINED'}
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
    $created=[FcMsiFixtureOwnerV2.Registry]::Create('FutureClassroom-MsiGuardian-Test-'+$fixture.nonce)
    $script:ownerHandle=$created.Key
    $report.registryCreation=[ordered]@{parentOpenError=$created.ParentOpenError;createError=$created.CreateError;parentCloseError=$created.ParentCloseError;disposition=$created.Disposition;actualNew=$false;nonceWritten=$false;sidWritten=$false;markersComplete=$false;handleDisposed=$false}
    if($created.ParentOpenError -ne 0 -or $created.CreateError -ne 0){throw 'Native root creation failed; no ownership claimed.'}
    if($created.Disposition -ne 1){throw 'Native disposition is not NEW; existing root is never written or deleted.'}
    $script:ownerNew=$true;$report.registryCreation.actualNew=$true
    if($created.ParentCloseError -ne 0){throw 'Parent close failed; retain created root without MSI.'}
    $script:ownerKey=[Microsoft.Win32.RegistryKey]::FromHandle($script:ownerHandle,[Microsoft.Win32.RegistryView]::Registry32)
    $script:ownerKey.SetValue('ProbeNonce',$fixture.nonce,[Microsoft.Win32.RegistryValueKind]::String);$report.registryCreation.nonceWritten=$true
    $script:ownerKey.SetValue('OwnerSid',$sid,[Microsoft.Win32.RegistryValueKind]::String);$report.registryCreation.sidWritten=$true
    $script:markersWritten=$true;$report.registryCreation.markersComplete=$true
    $null=Invoke-OwnedMsi 'install-commit' @('/i',$msi) 1
    $null=Invoke-OwnedMsi 'repair-commit' @('/famus',$fixture.productCode) 1
    $null=Invoke-OwnedMsi 'uninstall-commit' @('/x',$fixture.productCode) 1
    $null=Invoke-OwnedMsi 'install-rollback' @('/i',$msi) 0
    $contract=$true
    foreach($observed in @($script:records)){
        if($observed.guardianEvidence.Count -ne 1 -or $observed.guardianObserved.Count -ne 1){$contract=$false;continue}
        $g=$observed.guardianEvidence[0]
        foreach($field in @('JoinReturn','EndReturn','EndReturned','AboutToReturnCode')){if(-not $g.Contains($field)){$contract=$false}}
        if(-not $contract){continue}
        if($g.JoinReturn -ne 0 -or $g.EndReturn -ne 0 -or $g.EndReturned -ne 1 -or $g.AboutToReturnCode -ne 0 -or -not $observed.guardianObserved[0].actualExit -or $observed.guardianObserved[0].exitCode -ne 0){$contract=$false}
        if($observed.name -eq 'uninstall-commit' -and $observed.msiExitCode -ne 0){$contract=$false}
        if($observed.name -eq 'install-commit' -or $observed.name -eq 'repair-commit'){
            if($observed.msiExitCode -ne 0 -or $observed.productAfter.productState -ne 5 -or -not $observed.payloadAfter.valuePresent -or $observed.payloadAfter.kind -cne 'DWord' -or $observed.payloadAfter.value -ne 1){$contract=$false}
        }elseif($observed.productAfter.productState -ne -1 -or $observed.payloadAfter.valuePresent){$contract=$false}
    }
    $report.directSinglePackageContractSatisfied=$contract
    $report.rollbackInstallerExitCode='Recorded verbatim; no predeclared native code substituted for original outcome.'
    $report.status=if($contract){'OBSERVED_FOUR_DIRECT_OPERATIONS_NOT_PRODUCT_ACCEPTANCE'}else{'FAIL_FEASIBILITY_CONTRACT_RETAINED'}
}catch{
    $report.status='FAIL_OR_UNKNOWN_RETAINED';$report.errorKind=$_.Exception.GetType().Name;$report.errorMessage='Owned MSI feasibility did not complete; original scoped observations/logs retained.'
}finally{
    # Never compensate an unknown transaction or delete another product/key.
    if($script:ownerNew -and $script:markersWritten -and -not $script:unknown){
        try{
            Assert-OriginalsReturned
            Assert-KnownOwnerTree
            $state=Read-ProductState
            if($state.productState -eq 5){$null=Invoke-OwnedMsi 'cleanup-uninstall' @('/x',$fixture.productCode) 1;$state=Read-ProductState}
            $report.finalProductState=$state
            if($state.productState -eq -1){
                $report.evidenceBeforeOwnRegistryCleanup=@($script:records|ForEach-Object{@{name=$_.name;operation=$_.operation;evidence=@(Snapshot-Evidence $_.operation)}})
                Assert-OriginalsReturned
                Assert-KnownOwnerTree
                $base.DeleteSubKeyTree($expectedRegistry,$true)
                $script:ownerKey.Dispose();$script:ownerKey=$null
                $script:ownerHandle.Dispose();$report.registryCreation.handleDisposed=$true
                $remaining=$base.OpenSubKey($expectedRegistry,$false)
                if($remaining){$remaining.Dispose();throw 'Root still present after original owner handles closed.'}
                $report.cleanup='PASS_OWN_PRODUCT_ABSENT_AND_NONCE_REGISTRY_REMOVED'
            }else{$report.cleanup='RETAINED_PRODUCT_STATE_NOT_ABSENT'}
        }catch{$report.cleanup='FAIL_OR_UNKNOWN_RETAINED'}
    }elseif($script:unknown){$report.cleanup='UNKNOWN_LIVE_OPERATION_RETAINED_NO_CLEANUP'}
    elseif($script:ownerNew){$report.cleanup='RETAINED_ACTUAL_NEW_ROOT_MARKERS_INCOMPLETE_NO_MSI_CLEANUP'}
    else{$report.cleanup='NOT_OWNED_NO_ROOT_OR_PRODUCT_MUTATION_OR_CLEANUP'}
    if($script:ownerKey){$script:ownerKey.Dispose();$script:ownerKey=$null}
    if($script:ownerHandle){$script:ownerHandle.Dispose();if($report.Contains('registryCreation')){$report.registryCreation.handleDisposed=$true}}
    $report.unknownPhysicalOperation=$script:unknown;$base.Dispose()
    $json=$report|ConvertTo-Json -Depth 16
    [IO.File]::WriteAllText($reportPath,$json,[Text.UTF8Encoding]::new($false))
}
Write-Output ('MSI_FEASIBILITY_REPORT '+$reportPath)
if($report.status -ne 'OBSERVED_FOUR_DIRECT_OPERATIONS_NOT_PRODUCT_ACCEPTANCE' -or $report.cleanup -ne 'PASS_OWN_PRODUCT_ABSENT_AND_NONCE_REGISTRY_REMOVED'){exit 1}
