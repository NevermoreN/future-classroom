
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

$root='C:\Users\Elior\AppData\Local\Temp\FutureClassroom-Test-TLS-405d1305f81f4c5e94eb5390b1ed8fbf';$marker='FutureClassroom-Test-TLS-405d1305f81f4c5e94eb5390b1ed8fbf'
Assert (([IO.File]::ReadAllText((Join-Path $root '.tls-probe-root')))-eq $marker) 'TLS capture root marker mismatch'
$data=@{}
foreach($name in @('tls.stdout','tls.stderr','.tls-process.json','.tls-supervisor.json','.tls-setup-process.json','.tls-fixture.json','tls-public.pem')){
  $path=Join-Path $root $name
  if(Test-Path -LiteralPath $path){
    # The process is physically stopped before this script runs. Hash a stream,
    # never load a potentially oversized original failure log into memory/JSON.
    $stream=[IO.File]::Open($path,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::Read)
    $sha=[Security.Cryptography.SHA256]::Create()
    try{
      $length=$stream.Length
      $digest=([BitConverter]::ToString($sha.ComputeHash($stream))).Replace('-','').ToLowerInvariant()
      Assert ($stream.Length-eq $length) 'TLS raw evidence length changed while hashing'
      $data[$name]=@{bytes=$length;sha256=$digest}
    }finally{$sha.Dispose();$stream.Dispose()}
  }
}
$data|ConvertTo-Json -Depth 4 -Compress
