
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

$marker='FutureClassroom-Test-TLS-50505050505050505050505050505050';$root='C:\Synthetic-Not-Executed\FutureClassroom-Test-TLS-50505050505050505050505050505050'
Assert (([IO.File]::ReadAllText((Join-Path $root '.tls-probe-root')))-eq $marker) 'TLS root marker mismatch'
$setupProcess=Get-Process -Id $PID
Write-Json (Join-Path $root '.tls-setup-process.json') @{marker=$marker;pid=$PID;startTicks=$setupProcess.StartTime.ToUniversalTime().Ticks.ToString();exe=$setupProcess.Path}
$setupProcess.Dispose()
$provider=[Security.Cryptography.CngProvider]::MicrosoftSoftwareKeyStorageProvider
$keyName=$marker+'-key'
$before=Store-Snapshot
Assert (![Security.Cryptography.CngKey]::Exists($keyName,$provider)) 'unique TLS key unexpectedly exists'
Assert (@(Get-ChildItem -LiteralPath 'Cert:\CurrentUser\My'|Where-Object{$_.FriendlyName-eq $marker}).Count-eq 0) 'unique TLS certificate unexpectedly exists'
$manifest=@{marker=$marker;keyName=$keyName;provider=$provider.Provider;keyAbsentBefore=$true;before=$before;thumbprint=$null}
Write-Json (Join-Path $root '.tls-fixture.json') $manifest
$cert=New-SelfSignedCertificate -Type SSLServerAuthentication -Subject 'CN=localhost' -TextExtension @('2.5.29.17={text}DNS=localhost&IPAddress=127.0.0.1') -CertStoreLocation 'Cert:\CurrentUser\My' -Provider $provider.Provider -Container $keyName -KeyAlgorithm RSA -KeyLength 2048 -HashAlgorithm SHA256 -KeyExportPolicy NonExportable -KeyUsage DigitalSignature,KeyEncipherment -NotBefore (Get-Date).AddMinutes(-5) -NotAfter (Get-Date).AddDays(1) -FriendlyName $marker
$manifest.thumbprint=$cert.Thumbprint.ToUpperInvariant()
Write-Json (Join-Path $root '.tls-fixture.json') $manifest
$rsa=$null
try{
  $rsa=[Security.Cryptography.X509Certificates.RSACertificateExtensions]::GetRSAPrivateKey($cert)
  Assert ($rsa-is [Security.Cryptography.RSACng]) 'TLS fixture key is not CNG'
  Assert ($rsa.Key.KeyName-eq $keyName-and $rsa.Key.Provider.Provider-eq $provider.Provider) 'TLS fixture key ownership mismatch'
}finally{if($rsa){$rsa.Dispose()}}
Assert ([Security.Cryptography.CngKey]::Exists($keyName,$provider)) 'TLS fixture key missing after creation'
$unrelated=Store-Snapshot $manifest.thumbprint
Assert ($unrelated.count-eq $before.count-and $unrelated.sha256-eq $before.sha256) 'unrelated MY certificates changed during fixture creation'
$der=$cert.RawData
$pem="-----BEGIN CERTIFICATE-----`n"+[Convert]::ToBase64String($der,[Base64FormattingOptions]::InsertLineBreaks).Replace("`r",'')+"`n-----END CERTIFICATE-----`n"
[IO.File]::WriteAllText((Join-Path $root 'tls-public.pem'),$pem,[Text.ASCIIEncoding]::new())
$os=Get-CimInstance Win32_OperatingSystem
@{thumbprint=$manifest.thumbprint;keyName=$keyName;provider=$provider.Provider;myBefore=$before;publicPem=$pem;privateKeyExported=$false;san=(@($cert.Extensions|Where-Object{$_.Oid.Value-eq '2.5.29.17'}|ForEach-Object{$_.Format($false)})-join ';');platform=@{caption=$os.Caption;version=$os.Version;build=$os.BuildNumber;osArchitecture=$os.OSArchitecture}}|ConvertTo-Json -Depth 5 -Compress
