# Only owned hidden cmd children; no file writes or external environment changes.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
function Get-ActiveProcessIdentity($Process, [DateTime]$Deadline) {
    if ($Process.HasExited) { return $null }
    try {
        $processId = $Process.Id
        $startTicks = $Process.StartTime.ToUniversalTime().Ticks
        while ($true) {
            if ($Process.HasExited) { return $null }
            if ([DateTime]::UtcNow -ge $Deadline) { throw 'Live process identity deadline expired.' }
            $module = $Process.MainModule
            if ($null -ne $module) {
                $image = [string]$module.FileName
                if (-not [string]::IsNullOrWhiteSpace($image)) {
                    if ($Process.HasExited) { return $null }
                    if ([DateTime]::UtcNow -ge $Deadline) { throw 'Live process identity deadline expired.' }
                    return [ordered]@{ pid = $processId; start_ticks = $startTicks; image = $image }
                }
            }
            # Startup can temporarily expose no module or an empty image path.
            if ($Process.HasExited) { return $null }
            if ([DateTime]::UtcNow -ge $Deadline) { throw 'Live process identity deadline expired.' }
            $Process.Refresh()
            Start-Sleep -Milliseconds 1
        }
    } catch {
        # The same process may exit between HasExited and the module query.
        if ($Process.HasExited) { return $null }
        throw
    }
}


$cases = [Collections.Generic.List[object]]::new()
$cmd = Join-Path $env:SystemRoot 'System32\cmd.exe'
$oldFailures = 0
$startedCount = 0; $exitedCount = 0; $disposedCount = 0; $probeFailure = $null
function New-OwnedCmd([string]$Arguments) {
    $info = [Diagnostics.ProcessStartInfo]::new()
    $info.FileName = $cmd; $info.Arguments = $Arguments
    $info.UseShellExecute = $false; $info.CreateNoWindow = $true
    $info.RedirectStandardInput = $true
    $info.RedirectStandardOutput = $true; $info.RedirectStandardError = $true
    $child = [Diagnostics.Process]::new(); $child.StartInfo = $info
    if (-not $child.Start()) { $child.Dispose(); throw 'Owned cmd did not start.' }
    return $child
}
$immediateNulls=0; $immediateIdentities=0
try {
for ($i=0; $i -lt 8; $i++) {
    $child = New-OwnedCmd '/d /q /c exit 0'; $startedCount++
    try {
        if (-not $child.WaitForExit(5000) -or $child.ExitCode -ne 0) { throw 'Owned fast child failed to exit.' }
        $oldRejected = $false
        try { $null = $child.MainModule.FileName } catch { $oldRejected = $true; $oldFailures++ }
        $identity = Get-ActiveProcessIdentity $child ([DateTime]::UtcNow.AddSeconds(5))
        if ($null -ne $identity) { throw 'Exited owned child produced a live identity.' }
        $cases.Add([ordered]@{ name="actual-exited-child-$i"; passed=$true; oldReadRejected=$oldRejected; pid=$child.Id; exitCode=$child.ExitCode })
    } finally {
        if (-not $child.HasExited) { $child.Kill(); $null=$child.WaitForExit(5000) }
        if ($child.HasExited) { $exitedCount++ } else { throw 'Owned child retained.' }
        $child.Dispose(); $disposedCount++
    }
}

$immediateNulls=0; $immediateIdentities=0
for ($i=0; $i -lt 16; $i++) {
    $originalStarted=[DateTime]::UtcNow
    $child=New-OwnedCmd '/d /q /c exit 0'; $startedCount++
    try {
        $identity=Get-ActiveProcessIdentity $child ($originalStarted.AddSeconds(5))
        if ($null -ne $identity) {
            if ($identity.pid -ne $child.Id -or $identity.start_ticks -ne $child.StartTime.ToUniversalTime().Ticks -or
                -not [string]::Equals($identity.image,$cmd,[StringComparison]::OrdinalIgnoreCase)) { throw 'Immediate owned child identity mismatch.' }
            $immediateIdentities++
        } else {
            if (-not $child.HasExited) { throw 'Immediate null identity without observed process exit.' }
            $immediateNulls++
        }
        if (-not $child.WaitForExit(5000) -or $child.ExitCode -ne 0) { throw 'Immediate child did not exit successfully.' }
        $cases.Add([ordered]@{name="actual-immediate-child-$i"; passed=$true; identityPresent=($null -ne $identity); pid=$child.Id; exitCode=$child.ExitCode})
    } finally {
        if (-not $child.HasExited) { $child.Kill(); $null=$child.WaitForExit(5000) }
        if ($child.HasExited) { $exitedCount++ } else { throw 'Owned immediate child retained.' }
        $child.Dispose(); $disposedCount++
    }
}

$child = New-OwnedCmd '/d /q /c set /p fcProbe='; $startedCount++
try {
    $identity = Get-ActiveProcessIdentity $child ([DateTime]::UtcNow.AddSeconds(5))
    if ($null -eq $identity -or $identity.pid -ne $child.Id -or
        $identity.start_ticks -ne $child.StartTime.ToUniversalTime().Ticks -or
        -not [string]::Equals($identity.image, $cmd, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'Owned live child identity mismatch.'
    }
    $child.StandardInput.Close()
    if (-not $child.WaitForExit(5000)) { throw 'Owned input-close child did not exit.' }
    $cases.Add([ordered]@{name='actual-live-child-exact-identity-and-input-close-exit'; passed=$true; pid=$identity.pid; start_ticks=$identity.start_ticks; image=$identity.image; exitCode=$child.ExitCode})
} finally {
    if (-not $child.HasExited) { $child.Kill(); $null=$child.WaitForExit(5000) }
    if ($child.HasExited) { $exitedCount++ } else { throw 'Owned child retained.' }
    $child.Dispose(); $disposedCount++
}
if ($startedCount -ne 25 -or $exitedCount -ne 25 -or $oldFailures -lt 1) { throw 'Owned-process regression coverage incomplete.' }

} catch { $probeFailure=$_.Exception.Message } finally {
[ordered]@{status=$(if ($null -eq $probeFailure) {'PASS'} else {'FAIL'}); failure=$probeFailure; powershell=$PSVersionTable.PSVersion.ToString(); sourceSha256='0cb7c52757fcf22bced4af6c25ae1d52ac957907200e5e322b24e86315690508'; scope='OWNED_HIDDEN_CMD_CHILDREN_ONLY_NO_FILES_OR_PERSISTENT_CHANGES'; caseCount=$cases.Count; started=$startedCount; actualExited=$exitedCount; allHandlesDisposed=($disposedCount -eq $startedCount); disposedCount=$disposedCount; oldExitedReadFailures=$oldFailures; immediateNulls=$immediateNulls; immediateIdentities=$immediateIdentities; cases=$cases.ToArray()} | ConvertTo-Json -Depth 8 -Compress

}
if ($null -ne $probeFailure) { throw $probeFailure }
