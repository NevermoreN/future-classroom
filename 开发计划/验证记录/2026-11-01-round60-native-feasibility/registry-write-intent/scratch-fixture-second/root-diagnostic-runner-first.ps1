[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)][string]$FixtureDirectory,
    [Parameter(Mandatory=$true)][ValidatePattern('^[a-f0-9]{64}$')][string]$ExpectedSha256
)
$ErrorActionPreference='Stop'
Set-StrictMode -Version Latest
# Prepared only; no Windows execution by author. The exact pinned EXE alone
# owns registry setup, per-case jobs, rollback and closed-namespace cleanup.
$root=[IO.Path]::GetFullPath($FixtureDirectory).TrimEnd('\')
$name=[IO.Path]::GetFileName($root)
if($name -cnotmatch '^FutureClassroomRegistryIntent-[a-f0-9]{32}$'){throw 'Only a nonce-named scratch directory is permitted.'}
$allowed=[IO.Path]::GetFullPath((Join-Path ([IO.Path]::GetTempPath()) $name)).TrimEnd('\')
if(-not [String]::Equals($root,$allowed,[StringComparison]::OrdinalIgnoreCase)){throw 'Only the direct owned TEMP child is permitted.'}
$exe=Join-Path $root 'registry-write-intent.exe'
foreach($p in @($root,$exe)){if((Get-Item -LiteralPath $p -Force).Attributes -band [IO.FileAttributes]::ReparsePoint){throw 'Scratch inputs cannot be reparse points.'}}
if($ExpectedSha256 -cne 'fc5868521d3d54130daf855a07ae1b703b33bb8f0c3832252970a49f14c75dfc'){throw 'This runner is bound only to the second frozen scratch executable.'}
if((Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash.ToLowerInvariant() -cne $ExpectedSha256){throw 'Scratch executable differs from frozen source output.'}
$stdoutPath=Join-Path $root 'native-first.stdout.log';$stderrPath=Join-Path $root 'native-first.stderr.log';$reportPath=Join-Path $root 'native-first.json'
foreach($p in @($stdoutPath,$stderrPath,$reportPath)){if(Test-Path -LiteralPath $p){throw 'Original scratch evidence already exists; refusing overwrite.'}}
$report=[ordered]@{schemaVersion=1;status='RUNNING';scope='Eight synthetic HKCU/TxR process races; observations only, never a universal CAS or real Run proof';expectedExeSha256=$ExpectedSha256;originalDeadlineMillis=90000;nativeCaseBudgetMillis=6000;nativeTransactionTimeoutMillis=3000;physicalExit=$false;stdoutPath=$stdoutPath;stderrPath=$stderrPath;cleanup='NOT_OBSERVED';windowsVersion=[Environment]::OSVersion.VersionString;process64Bit=[Environment]::Is64BitProcess;nativeRegistryView='Registry32';registryAccessByRunner='NONE'}
$proc=$null;$started=$false;$stdout=$null;$stderr=$null;$outTask=$null;$errTask=$null;$watch=[Diagnostics.Stopwatch]::StartNew()
try{
    $stdout=[IO.File]::Open($stdoutPath,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::Read)
    $stderr=[IO.File]::Open($stderrPath,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::Read)
    $psi=[Diagnostics.ProcessStartInfo]::new();$psi.FileName=$exe;$psi.WorkingDirectory=$root;$psi.UseShellExecute=$false;$psi.CreateNoWindow=$true;$psi.RedirectStandardOutput=$true;$psi.RedirectStandardError=$true
    $proc=[Diagnostics.Process]::new();$proc.StartInfo=$psi
    if(-not $proc.Start()){throw 'Exact scratch process did not start.'};$started=$true
    # Start draining immediately even if a subsequent original-handle identity
    # getter fails; do not leave the native child blocked on an unconsumed pipe.
    $outTask=$proc.StandardOutput.BaseStream.CopyToAsync($stdout,8192)
    $errTask=$proc.StandardError.BaseStream.CopyToAsync($stderr,8192)
    $null=$proc.Handle
    $report.pid=$proc.Id;$report.creationFileTime=$proc.StartTime.ToUniversalTime().ToFileTimeUtc();$report.originalProcessHandleRetained=$true
    while(-not $proc.WaitForExit(50)){
        if($watch.ElapsedMilliseconds -ge 90000){throw 'Original 90-second observation deadline elapsed; preserve original process identity and state.'}
        if($stdout.Length -gt 1MB -or $stderr.Length -gt 1MB){throw 'Original raw log bound exceeded; do not launch another operation.'}
    }
    if($watch.ElapsedMilliseconds -ge 90000){throw 'Original deadline expired before completion delivery.'}
    $proc.WaitForExit();$report.physicalExit=$true;$report.exitCode=$proc.ExitCode
    $remaining=[Math]::Max(0,90000-[int]$watch.ElapsedMilliseconds)
    if(-not [Threading.Tasks.Task]::WaitAll([Threading.Tasks.Task[]]@($outTask,$errTask),$remaining)){throw 'Original output streams have not returned; preserve partial evidence.'}
    $stdout.Flush();$stderr.Flush();$stdout.Dispose();$stdout=$null;$stderr.Dispose();$stderr=$null
    $raw=[IO.File]::ReadAllBytes($stdoutPath);$err=[IO.File]::ReadAllBytes($stderrPath)
    if($raw.Length -gt 1MB -or $err.Length -gt 1MB){throw 'Original raw log bound exceeded.'}
    foreach($b in $raw){if($b -gt 127){throw 'Unexpected non-ASCII native wire byte.'}}
    $text=[Text.Encoding]::ASCII.GetString($raw);$lines=@($text.Split([char]10)|Where-Object{$_.Length -gt 0})
    if($lines.Count -gt 4096){throw 'Native event count exceeded fixed bound.'}
    $nonces=@($lines|Where-Object{$_ -cmatch '^RWI_OWNED_NONCE [a-f0-9]{32}$'})
    if($nonces.Count -ne 1){throw 'Exactly one original generated scratch nonce is required.'}
    $report.registryNonce=$nonces[0].Substring(16);$report.registryRoot='HKCU32\Software\FutureClassroom-RegistryWriteIntent-Test-'+$report.registryNonce
    $events=[Collections.Generic.List[object]]::new();$values=[Collections.Generic.List[object]]::new()
    foreach($line in $lines){
        if($line.Length -gt 2048){throw 'Native line bound exceeded.'}
        if($line.StartsWith('RWI_OWNED_NONCE ')){continue}
        if($line -cmatch '^RWI_EVENT pid=([0-9]{1,10}) seq=([0-9]{1,10}) tick_ms=([0-9]{1,20}) name=([A-Za-z0-9_]{1,120}) code=([0-9]{1,10}) detail=([0-9]{1,20})$'){
            $events.Add(@{pid=[uint32]$Matches[1];sequence=[uint32]$Matches[2];tickMillis=[uint64]$Matches[3];name=$Matches[4];code=[uint32]$Matches[5];detail=[uint64]$Matches[6]});continue
        }
        if($line -cmatch '^RWI_VALUE pid=([0-9]{1,10}) seq=([0-9]{1,10}) tick_ms=([0-9]{1,20}) type=([0-9]{1,10}) bytes=([0-9]{1,3}) hex=([a-f0-9]{0,512})$'){
            $bytes=[uint32]$Matches[5];if($bytes -gt 256 -or $Matches[6].Length -ne 2*$bytes){throw 'Native successful value byte shape differs.'}
            $values.Add(@{pid=[uint32]$Matches[1];sequence=[uint32]$Matches[2];tickMillis=[uint64]$Matches[3];type=[uint32]$Matches[4];bytes=$bytes;hex=$Matches[6]});continue
        }
        throw 'Unexpected native wire line; preserve original rather than interpret it as success.'
    }
    $report.events=$events;$report.successfulValueBytes=$values
    $report.nativeCases=@($events|Where-Object{$_.pid -eq $report.pid -and $_.name -ceq 'supervisor_case_begin'}|ForEach-Object{[char]$_.detail})
    $report.actualJobZeroObservations=@($events|Where-Object{$_.name -ceq 'supervisor_actual_job_active_zero' -and $_.code -eq 0}).Count
    $report.cleanup=if(@($events|Where-Object{$_.name -ceq 'CLEANUP_COMPLETE_EXACT_OWNED_LEAF_ABSENT' -and $_.code -eq 0}).Count -eq 1){'OBSERVED_NATIVE_EXACT_LEAF_ABSENT'}else{'NOT_CONFIRMED_NO_RUNNER_REGISTRY_CLEANUP'}
    $report.nativeUnknownOutcomes=@($events|Where-Object{$_.name -cmatch '^(INCONCLUSIVE_|CLEANUP_FAIL_|FIXTURE_CLEANUP_FAIL)'})
    $report.sourceScope='Real native call return values and ordering will be in this report only when the EXE is actually run. Same-value no-conflict is not a distinct-value CAS counterexample.'
    if($watch.ElapsedMilliseconds -ge 90000){throw 'Original deadline expired before final observation publication.'}
    $report.status=if($report.exitCode -eq 0 -and $report.cleanup -ceq 'OBSERVED_NATIVE_EXACT_LEAF_ABSENT' -and (($report.nativeCases -join '') -ceq 'ABCDEFGH')){'OBSERVED_EIGHT_CASES_NOT_CAS_QUALIFICATION'}else{'FAIL_OR_INCONCLUSIVE_NATIVE_OBSERVATIONS_RETAINED'}
}catch{
    $report.status='FAIL_OR_UNKNOWN_RETAINED';$report.errorKind=$_.Exception.GetType().Name;$report.errorDetail=$_.Exception.Message;$report.errorLine=$_.InvocationInfo.ScriptLineNumber;$report.errorMessage='Scratch observation incomplete; retain original process/log/registry scope. No automatic second run or registry compensation.'
    if($started -and $proc -and -not $proc.HasExited){$script:RegistryIntentOriginalProcess=$proc;$report.originalProcessStillRunning=$true}
}finally{
    # No kill/PID reacquisition and no registry cleanup here. Native code alone
    # gates its cleanup on exact job ActiveProcesses==0 and original key scope.
    if($stdout -and ($null -eq $outTask -or $outTask.IsCompleted)){$stdout.Dispose()}
    if($stderr -and ($null -eq $errTask -or $errTask.IsCompleted)){$stderr.Dispose()}
    if($started -and $proc -and $proc.HasExited){$report.physicalExit=$true;$report.exitCode=$proc.ExitCode;$proc.Dispose()}
    $report.elapsedMillis=$watch.ElapsedMilliseconds
    foreach($pair in @(@('stdout',$stdoutPath),@('stderr',$stderrPath))){
        if(Test-Path -LiteralPath $pair[1]){
            $report[$pair[0]+'Bytes']=(Get-Item -LiteralPath $pair[1]).Length
            $task=if($pair[0] -ceq 'stdout'){$outTask}else{$errTask}
            if($null -eq $task -or $task.IsCompleted){
                try{$report[$pair[0]+'Sha256']=(Get-FileHash -LiteralPath $pair[1] -Algorithm SHA256).Hash.ToLowerInvariant()}catch{$report[$pair[0]+'HashErrorKind']=$_.Exception.GetType().Name;$report.status='FAIL_OR_UNKNOWN_RETAINED'}
            }else{$report[$pair[0]+'HashStatus']='INCOMPLETE_LIVE_STREAM_NO_FINAL_HASH'}
        }
    }
    $stream=[IO.File]::Open($reportPath,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::Read)
    try{$bytes=[Text.UTF8Encoding]::new($false).GetBytes(($report|ConvertTo-Json -Depth 16));$stream.Write($bytes,0,$bytes.Length)}finally{$stream.Dispose()}
}
Write-Output ('REGISTRY_INTENT_REPORT '+$reportPath)
if($report.status -cne 'OBSERVED_EIGHT_CASES_NOT_CAS_QUALIFICATION'){exit 1}
