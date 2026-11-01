
$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
$OutputEncoding=[Console]::OutputEncoding
function Assert([bool]$Condition,[string]$Message){if(!$Condition){throw $Message}}
function Write-Json([string]$Path,$Value){[IO.File]::WriteAllText($Path,($Value|ConvertTo-Json -Depth 8 -Compress),[Text.UTF8Encoding]::new($false))}
function Store-Snapshot([string]$Exclude=''){
  $entries=@(Get-ChildItem -LiteralPath 'Cert:\CurrentUser\My'|Where-Object{$_.Thumbprint-ne $Exclude}|ForEach-Object{
    $cert=$_;$sha=[Security.Cryptography.SHA256]::Create()
    try{$der=([BitConverter]::ToString($sha.ComputeHash($cert.RawData))).Replace('-','').ToLowerInvariant()}finally{$sha.Dispose()}
    $cert.Thumbprint.ToUpperInvariant()+':'+$der+':'+[Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes([string]$cert.FriendlyName))+':'+$cert.HasPrivateKey
  })
  [Array]::Sort($entries,[StringComparer]::Ordinal)
  $sha=[Security.Cryptography.SHA256]::Create()
  try{$digest=([BitConverter]::ToString($sha.ComputeHash([Text.Encoding]::UTF8.GetBytes(($entries-join "`n"))))).Replace('-','').ToLowerInvariant()}finally{$sha.Dispose()}
  return @{count=$entries.Count;sha256=$digest}
}

$root='C:\Users\Elior\AppData\Local\Temp\FutureClassroom-Test-TLS-1bb08e0ce22c435680d8c118776d12e1';$marker='FutureClassroom-Test-TLS-1bb08e0ce22c435680d8c118776d12e1'
Assert ((Split-Path -Leaf $root)-eq $marker) 'TLS cleanup basename mismatch'
Assert (([IO.File]::ReadAllText((Join-Path $root '.tls-probe-root')))-eq $marker) 'TLS cleanup root marker mismatch'
$killed=0
# Stop the recorded supervisor first so it cannot create another child after
# the native-child audit. A process object retains its original handle to exit.
foreach($name in @('.tls-setup-process.json','.tls-supervisor.json','.tls-process.json')){
  $processFile=Join-Path $root $name
  if(Test-Path -LiteralPath $processFile){
    $entry=[IO.File]::ReadAllText($processFile)|ConvertFrom-Json
    Assert ($entry.marker-eq $marker-and [int]$entry.pid-ne $PID) 'TLS process journal ownership mismatch'
    if($name-eq '.tls-process.json'){Assert ($entry.exe-eq (Join-Path $root 'fc-bootstrap.exe')) 'TLS native process path mismatch'}
    else{Assert ((Split-Path -Leaf $entry.exe)-eq 'powershell.exe') 'TLS supervisor path mismatch'}
    $ownedProcess=Get-Process -Id ([int]$entry.pid) -ErrorAction SilentlyContinue
    if($ownedProcess){
      $retainedHandle=$ownedProcess.Handle
      Assert ($retainedHandle-ne [IntPtr]::Zero) 'owned TLS process handle unavailable during cleanup'
      Assert ($ownedProcess.Path-eq $entry.exe-and $ownedProcess.StartTime.ToUniversalTime().Ticks.ToString()-eq $entry.startTicks) 'TLS process identity changed; refusing to kill reused PID'
      $ownedProcess.Kill();Assert ($ownedProcess.WaitForExit(5000)) 'owned TLS process cleanup timeout';$killed++;$ownedProcess.Dispose()
    }
  }
}
# Recover the narrow Start-Process / process-journal gap, never a process-name
# sweep. Both the unique uploaded path and the exact nonce argument must match.
$recovered=0
$remaining=@(Get-CimInstance Win32_Process|Where-Object{$_.ExecutablePath-and $_.ExecutablePath-eq (Join-Path $root 'fc-bootstrap.exe')})
foreach($candidate in $remaining){
  Assert ($candidate.CommandLine-and $candidate.CommandLine.Contains('--https-settings-probe')-and $candidate.CommandLine.Contains($marker)) 'unjournaled executable is not this owned diagnostic'
  $ownedProcess=Get-Process -Id $candidate.ProcessId
  $retainedHandle=$ownedProcess.Handle
  Assert ($retainedHandle-ne [IntPtr]::Zero) 'recovered TLS process handle unavailable during cleanup'
  Assert ($ownedProcess.Path-eq (Join-Path $root 'fc-bootstrap.exe')) 'unjournaled diagnostic path changed'
  $ownedProcess.Kill();Assert ($ownedProcess.WaitForExit(5000)) 'recovered owned TLS process cleanup timeout';$recovered++;$ownedProcess.Dispose()
}
$remaining=@(Get-CimInstance Win32_Process|Where-Object{$_.ExecutablePath-and $_.ExecutablePath-eq (Join-Path $root 'fc-bootstrap.exe')})
Assert ($remaining.Count-eq 0) 'owned TLS executable still running or process journal missing'
@{status='PASS';ownedProcessesRemaining=0;ownedProcessesKilled=$killed;recoveredNativeProcesses=$recovered}|ConvertTo-Json -Compress
