
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
Assert ((Split-Path -Leaf $root)-eq $marker) 'TLS cleanup basename mismatch'
Assert (([IO.File]::ReadAllText((Join-Path $root '.tls-probe-root')))-eq $marker) 'TLS cleanup root marker mismatch'
Assert (@(Get-CimInstance Win32_Process|Where-Object{$_.ExecutablePath-and $_.ExecutablePath-eq (Join-Path $root 'fc-bootstrap.exe')}).Count-eq 0) 'TLS executable still running before key cleanup'
$fixtureFile=Join-Path $root '.tls-fixture.json';$certificateRemoved=0;$keyExists=$false;$after=$null;$restored=$null;$deleteKeyFallback=$false
if(Test-Path -LiteralPath $fixtureFile){
  $entry=[IO.File]::ReadAllText($fixtureFile)|ConvertFrom-Json
  Assert ($entry.marker-eq $marker-and $entry.keyName-eq ($marker+'-key')-and $entry.provider-eq 'Microsoft Software Key Storage Provider'-and $entry.keyAbsentBefore-eq $true) 'TLS fixture cleanup ownership mismatch'
  $provider=[Security.Cryptography.CngProvider]::new($entry.provider)
  $certificates=@(Get-ChildItem -LiteralPath 'Cert:\CurrentUser\My'|Where-Object{$_.FriendlyName-eq $marker})
  Assert ($certificates.Count-le 1) 'unexpected duplicate owned TLS certificates; refusing broad cleanup'
  foreach($cert in $certificates){
    Assert ((!$entry.thumbprint)-or $cert.Thumbprint-eq $entry.thumbprint) 'owned TLS certificate thumbprint changed'
    $rsa=$null
    try{
      $rsa=[Security.Cryptography.X509Certificates.RSACertificateExtensions]::GetRSAPrivateKey($cert)
      Assert ($rsa-is [Security.Cryptography.RSACng]-and $rsa.Key.KeyName-eq $entry.keyName-and $rsa.Key.Provider.Provider-eq $entry.provider) 'owned TLS certificate private key mismatch'
    }finally{if($rsa){$rsa.Dispose()}}
    $certPath='Cert:\CurrentUser\My\'+$cert.Thumbprint
    try{Remove-Item -Path $certPath -DeleteKey}
    catch{
      $deleteKeyFallback=$true
      # Some Certificate-provider versions cannot perform DeleteKey. The exact
      # existing named CNG key was checked above; never enumerate/delete keys.
      if([Security.Cryptography.CngKey]::Exists($entry.keyName,$provider)){
        $key=[Security.Cryptography.CngKey]::Open($entry.keyName,$provider)
        try{$key.Delete()}finally{$key.Dispose()}
      }
      if(Test-Path -LiteralPath $certPath){Remove-Item -Path $certPath}
    }
    $certificateRemoved++
  }
  # A creation failure can leave just our previously absent, exact named key.
  # Delete it through CNG only after the immutable ownership manifest checks.
  if([Security.Cryptography.CngKey]::Exists($entry.keyName,$provider)){
    $key=[Security.Cryptography.CngKey]::Open($entry.keyName,$provider)
    try{$key.Delete()}finally{$key.Dispose()}
  }
  $keyExists=[Security.Cryptography.CngKey]::Exists($entry.keyName,$provider)
  Assert (!$keyExists) 'owned TLS CNG key still exists'
  Assert (@(Get-ChildItem -LiteralPath 'Cert:\CurrentUser\My'|Where-Object{$_.FriendlyName-eq $marker}).Count-eq 0) 'owned TLS certificate still exists'
  $after=Store-Snapshot
  $restored=($after.count-eq $entry.before.count-and $after.sha256-eq $entry.before.sha256)
  Assert $restored 'CurrentUser MY collection digest not restored; preserving owned evidence root'
}
Remove-Item -LiteralPath $root -Recurse -Force
Assert (!(Test-Path -LiteralPath $root)) 'owned TLS root remains'
@{status='PASS';rootAbsent=$true;ownedProcessesRemaining=0;certificatesRemoved=$certificateRemoved;deleteKeyFallbackUsed=$deleteKeyFallback;cngKeyExists=$keyExists;myAfter=$after;myDigestRestored=$restored;sshConfiguration='UNTOUCHED';rootTrustStore='UNTOUCHED';httpSys='UNTOUCHED';firewall='UNTOUCHED'}|ConvertTo-Json -Depth 5 -Compress
