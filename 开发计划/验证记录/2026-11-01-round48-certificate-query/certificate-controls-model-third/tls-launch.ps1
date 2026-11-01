
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

$root='C:\Synthetic-Not-Executed\FutureClassroom-Test-TLS-50505050505050505050505050505050';$marker='FutureClassroom-Test-TLS-50505050505050505050505050505050';$thumbprint='5050505050505050505050505050505050505050';$exe=Join-Path $root 'fc-bootstrap.exe'
Assert (([IO.File]::ReadAllText((Join-Path $root '.tls-probe-root')))-eq $marker) 'TLS launch root marker mismatch'
$env:TEMP=Join-Path $root 'temp';$env:TMP=$env:TEMP
New-Item -ItemType Directory -Path $env:TEMP|Out-Null
$child=$null;$handle=[IntPtr]::Zero;$seen=0;$watch=[Diagnostics.Stopwatch]::StartNew()
$supervisor=Get-Process -Id $PID
Write-Json (Join-Path $root '.tls-supervisor.json') @{marker=$marker;pid=$PID;startTicks=$supervisor.StartTime.ToUniversalTime().Ticks.ToString();exe=$supervisor.Path}
$supervisor.Dispose()
function Forward-Lines {
  $path=Join-Path $root 'tls.stdout'
  if(Test-Path -LiteralPath $path){
    # Start-Process retains a redirected writer while the child is running.
    # Permit that writer explicitly; ReadAllText's sharing mode conflicts with
    # it. Keep both byte checks and a fixed, capped character allocation.
    $stream=[IO.File]::Open($path,[IO.FileMode]::Open,[IO.FileAccess]::Read,([IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete))
    $reader=$null
    try{
      Assert ($stream.Length-le 524288) 'native TLS stdout exceeded diagnostic bound'
      $reader=[IO.StreamReader]::new($stream,[Text.UTF8Encoding]::new($false,$true),$false,4096,$true)
      $chars=[char[]]::new(524289)
      $length=$reader.ReadBlock($chars,0,$chars.Length)
      Assert ($length -le 524288 -and $stream.Length -le 524288) 'native TLS stdout grew beyond diagnostic bound'
      $text=[string]::new($chars,0,$length)
    }finally{if($reader){$reader.Dispose()};$stream.Dispose()}
    $lines=$text.Split([char]10)
    $complete=$lines.Length-1
    while($script:seen-lt $complete){[Console]::WriteLine($lines[$script:seen].TrimEnd([char]13));$script:seen++}
    [Console]::Out.Flush()
  }
}
try{
  $child=Start-Process -FilePath $exe -ArgumentList @('--https-settings-probe','--certificate-thumbprint',$thumbprint) -WindowStyle Hidden -WorkingDirectory $root -RedirectStandardOutput (Join-Path $root 'tls.stdout') -RedirectStandardError (Join-Path $root 'tls.stderr') -PassThru
  $handle=$child.Handle
  Assert ($handle-ne [IntPtr]::Zero) 'owned native TLS process handle missing'
  $entry=@{marker=$marker;pid=$child.Id;startTicks=$child.StartTime.ToUniversalTime().Ticks.ToString();exe=$exe}
  Write-Json (Join-Path $root '.tls-process.json') $entry
  while(!$child.WaitForExit(10)){
    Forward-Lines
    Assert ($watch.ElapsedMilliseconds-lt 90000) 'native TLS diagnostic exceeded its independent 90-second observation limit'
  }
  Forward-Lines
  Assert ($child.ExitCode-is [int]) 'actual native TLS Int32 exit code missing'
  [Console]::WriteLine('HTTPS_CERTIFICATE_PROCESS_EXIT '+(@{code=$child.ExitCode}|ConvertTo-Json -Compress));[Console]::Out.Flush()
}finally{
  if($child){
    if(!$child.HasExited){$child.Kill();Assert ($child.WaitForExit(5000)) 'owned TLS child did not terminate after diagnostic failure'}
    $child.Dispose()
  }
}
