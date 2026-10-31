# Only owned hidden cmd children; no file writes or external environment changes.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
function Get-ActiveProcessIdentity($Process) {
    if ($Process.HasExited) { return $null }
    try {
        $processId = $Process.Id
        $startTicks = $Process.StartTime.ToUniversalTime().Ticks
        $module = $Process.MainModule
        if ($null -eq $module) { throw 'Live process main module is unavailable.' }
        $image = [string]$module.FileName
        if ([string]::IsNullOrWhiteSpace($image)) { throw 'Live process image is unavailable.' }
    } catch {
        # The same process may exit between HasExited and the module query.
        if ($Process.HasExited) { return $null }
        throw
    }
    if ($Process.HasExited) { return $null }
    return [ordered]@{ pid = $processId; start_ticks = $startTicks; image = $image }
}


$cases = [Collections.Generic.List[object]]::new()
$cmd = Join-Path $env:SystemRoot 'System32\cmd.exe'
$oldFailures = 0
$startedCount = 0; $exitedCount = 0
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
for ($i=0; $i -lt 8; $i++) {
    $child = New-OwnedCmd '/d /q /c exit 0'; $startedCount++
    try {
        if (-not $child.WaitForExit(5000) -or $child.ExitCode -ne 0) { throw 'Owned fast child failed to exit.' }
        $oldRejected = $false
        try { $null = $child.MainModule.FileName } catch { $oldRejected = $true; $oldFailures++ }
        $identity = Get-ActiveProcessIdentity $child
        if ($null -ne $identity) { throw 'Exited owned child produced a live identity.' }
        $cases.Add([ordered]@{ name="actual-exited-child-$i"; passed=$true; oldReadRejected=$oldRejected; pid=$child.Id; exitCode=$child.ExitCode })
    } finally {
        if (-not $child.HasExited) { $child.Kill(); $null=$child.WaitForExit(5000) }
        if ($child.HasExited) { $exitedCount++ } else { throw 'Owned child retained.' }
        $child.Dispose()
    }
}
$child = New-OwnedCmd '/d /q /c set /p fcProbe='; $startedCount++
try {
    $identity = Get-ActiveProcessIdentity $child
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
    $child.Dispose()
}
if ($startedCount -ne 9 -or $exitedCount -ne 9 -or $oldFailures -lt 1) { throw 'Owned-process regression coverage incomplete.' }
[ordered]@{status='PASS'; powershell=$PSVersionTable.PSVersion.ToString(); sourceSha256='13e990098c3d2b100383ca80e5f2991df4bb96175884d46fa4391429b28ed4fb'; scope='OWNED_HIDDEN_CMD_CHILDREN_ONLY_NO_FILES_OR_PERSISTENT_CHANGES'; caseCount=$cases.Count; started=$startedCount; actualExited=$exitedCount; allHandlesDisposed=$true; oldExitedReadFailures=$oldFailures; cases=$cases.ToArray()} | ConvertTo-Json -Depth 8 -Compress
