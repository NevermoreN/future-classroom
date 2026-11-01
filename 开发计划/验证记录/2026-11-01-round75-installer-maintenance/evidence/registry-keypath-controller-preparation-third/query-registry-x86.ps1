param(
    [Parameter(Mandatory=$true)][ValidateSet('Before','Installed','After')][string]$Phase,
    [Parameter(Mandatory=$true)][string]$SpecPath,
    [Parameter(Mandatory=$true)][ValidatePattern('^[0-9a-f]{64}$')][string]$ExpectedSpecSha256,
    [Parameter(Mandatory=$true)][string]$OutputRoot
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
# Root starts this script directly in its original owned x86 PS5.1 child with a
# finite parent deadline. This script never starts msiexec or touches Pending.
if ([Environment]::Is64BitProcess) { throw 'QUERY_REQUIRES_X86_CHILD' }
$clock = [Diagnostics.Stopwatch]::StartNew()
$specBytes = [IO.File]::ReadAllBytes($SpecPath)
if ($specBytes.Length -gt 16384) { throw 'SPEC_BOUND' }
$hasher = [Security.Cryptography.SHA256]::Create()
try { $specSha = ([BitConverter]::ToString($hasher.ComputeHash($specBytes))).Replace('-','').ToLowerInvariant() }
finally { $hasher.Dispose() }
if ($specSha -cne $ExpectedSpecSha256) { throw 'SPEC_IDENTITY' }
$spec = ([Text.Encoding]::UTF8.GetString($specBytes) | ConvertFrom-Json)
if ($spec.schemaVersion -ne 1 -or $spec.scope -cne 'TEST_ONLY_REGISTRY_KEYPATH_FIXTURE' -or
    $spec.nonce -cnotmatch '^[0-9a-f]{32}$' -or $spec.registryView -cne 'Registry32' -or
    $spec.registryKey -cne ('Software\FutureClassroom-Test-RegistryKeyPath-' + $spec.nonce) -or
    $spec.registryValue -cne 'OwnedMarker' -or $spec.registryData -ne 1 -or $spec.version -cne '0.0.1') { throw 'CLOSED_SPEC' }
foreach ($field in @('productCode','upgradeCode','componentCode')) {
    if ($spec.$field -cnotmatch '^\{[0-9A-F]{8}-[0-9A-F]{4}-[0-9A-F]{4}-[0-9A-F]{4}-[0-9A-F]{12}\}$') { throw 'GUID_SPEC' }
}
$root = [IO.Path]::GetFullPath($OutputRoot)
if ($root -cne $OutputRoot -or [IO.Path]::GetFileName($root) -cne ('FutureClassroom-r75-registry-keypath-' + $spec.nonce)) { throw 'OWNED_ROOT_NAME' }
$cursor = $root
while ($null -ne $cursor -and $cursor.Length -gt 0) {
    $item = Get-Item -LiteralPath $cursor -Force
    if (-not $item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'ROOT_REPARSE' }
    $cursor = [IO.Path]::GetDirectoryName($cursor)
}
$ownerPath = Join-Path $root '.owner'
if ((Get-Item -LiteralPath $ownerPath -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'OWNER_REPARSE' }
$expectedOwner = [Text.Encoding]::UTF8.GetBytes("future-classroom-owned-r75-registry-keypath/v1`n")
if ([Convert]::ToBase64String([IO.File]::ReadAllBytes($ownerPath)) -cne [Convert]::ToBase64String($expectedOwner)) { throw 'OWNER_MARKER' }
$childTemp = Join-Path $root ('query-temp-' + $Phase)
if (Test-Path -LiteralPath $childTemp) { throw 'TEMP_NOT_FRESH' }
[void][IO.Directory]::CreateDirectory($childTemp)
$env:TEMP = $childTemp
$env:TMP = $childTemp
# The environment change is only in this dedicated child. Root removes this
# exact temp subtree after this child and its compiler have physically exited.
Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class R75RegistryKeypathQuery {
    [DllImport("msi.dll", CharSet=CharSet.Unicode, ExactSpelling=true)]
    public static extern int MsiGetComponentPathExW(string product, string component, string sid,
        uint context, StringBuilder path, ref uint capacity);
    [DllImport("msi.dll", CharSet=CharSet.Unicode, ExactSpelling=true)]
    public static extern uint MsiGetProductInfoExW(string product, string sid, uint context,
        string property, StringBuilder value, ref uint capacity);
    // PowerShell-to-managed string binding is not the native NULL boundary.
    // The managed wrapper chooses the literal NULL required for MACHINE (4).
    public static int ComponentPath(string product, string component, string sid,
        uint context, StringBuilder path, ref uint capacity) {
        return MsiGetComponentPathExW(product, component, context == 4 ? null : sid,
            context, path, ref capacity);
    }
    public static uint ProductInfo(string product, string sid, uint context,
        string property, StringBuilder value, ref uint capacity) {
        return MsiGetProductInfoExW(product, context == 4 ? null : sid,
            context, property, value, ref capacity);
    }
    [DllImport("msi.dll", CharSet=CharSet.Unicode, ExactSpelling=true)]
    public static extern uint MsiEnumRelatedProductsW(string family, uint reserved, uint index,
        StringBuilder product);
}
'@
$report = [ordered]@{ schemaVersion=1; status='FAIL_QUERY'; phase=$Phase; pid=$PID; processBits=32;
    osIs64Bit=[Environment]::Is64BitOperatingSystem; specSha256=$specSha; nonce=$spec.nonce;
    productCode=$spec.productCode; componentCode=$spec.componentCode; queries=@();
    keypathInterpretation='RAW_ONLY_NO_01_OR_21_ASSUMPTION'; registry=$null; related=@() }
try {
    $currentSid = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
    $report['currentSid'] = $currentSid
    foreach ($context in @(1,2,4)) {
        $sid = if ($context -eq 4) { $null } else { $currentSid }
        $path = New-Object Text.StringBuilder 4096
        [uint32]$pathCapacity = 4096
        $state = [R75RegistryKeypathQuery]::ComponentPath($spec.productCode, $spec.componentCode, $sid, $context, $path, [ref]$pathCapacity)
        $version = New-Object Text.StringBuilder 4096
        [uint32]$versionCapacity = 4096
        $code = [R75RegistryKeypathQuery]::ProductInfo($spec.productCode, $sid, $context, 'VersionString', $version, [ref]$versionCapacity)
        $raw = $path.ToString()
        $report.queries += [ordered]@{ context=$context; componentState=$state; componentPath=$raw;
            componentPathUtf16Hex=([BitConverter]::ToString([Text.Encoding]::Unicode.GetBytes($raw))).Replace('-','');
            pathCapacity=4096; pathReturnedLength=$pathCapacity; productVersionCode=$code;
            productVersion=$version.ToString(); versionCapacity=4096; versionReturnedLength=$versionCapacity }
        if ($state -eq -3 -or $code -eq 234 -or ($state -eq 3 -and $pathCapacity -ge 4096) -or ($code -eq 0 -and $versionCapacity -ge 4096)) { throw 'BOUNDED_QUERY_OVERFLOW' }
    }
    for ([uint32]$index=0; $index -lt 2; $index++) {
        $product = New-Object Text.StringBuilder 39
        $code = [R75RegistryKeypathQuery]::MsiEnumRelatedProductsW($spec.upgradeCode, 0, $index, $product)
        $report.related += [ordered]@{ index=$index; code=$code; productCode=$product.ToString() }
        if ($code -ne 0) { break }
    }
    $base = [Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::CurrentUser, [Microsoft.Win32.RegistryView]::Registry32)
    try {
        $key = $base.OpenSubKey($spec.registryKey, $false)
        try {
            $registry = [ordered]@{ exists=($null -ne $key); valueCount=0; subkeyCount=0; markerKind=$null; markerValue=$null }
            if ($null -ne $key) {
                $registry.valueCount = $key.ValueCount
                $registry.subkeyCount = $key.SubKeyCount
                if ($key.ValueCount -eq 1 -and $key.GetValueNames()[0] -ceq 'OwnedMarker') {
                    $registry.markerKind = $key.GetValueKind('OwnedMarker').ToString()
                    if ($registry.markerKind -ceq 'DWord') { $registry.markerValue = $key.GetValue('OwnedMarker') }
                }
            }
            $report.registry = $registry
        } finally { if ($null -ne $key) { $key.Dispose() } }
    } finally { $base.Dispose() }
    foreach ($entry in $report.queries) {
        if ($Phase -eq 'Installed' -and $entry.context -eq 2) {
            if ($entry.productVersionCode -ne 0 -or $entry.productVersion -cne '0.0.1' -or $entry.componentState -ne 3) { throw 'EXPECTED_INSTALLED_CONTEXT' }
        } elseif ($entry.productVersionCode -ne 1605 -or $entry.componentState -ne -1) { throw 'EXPECTED_CONTEXT_ABSENCE' }
    }
    if ($Phase -eq 'Installed') {
        if (-not $report.registry.exists -or $report.registry.valueCount -ne 1 -or $report.registry.subkeyCount -ne 0 -or
            $report.registry.markerKind -cne 'DWord' -or $report.registry.markerValue -ne 1 -or
            $report.related.Count -ne 2 -or $report.related[0].code -ne 0 -or $report.related[0].productCode -cne $spec.productCode -or $report.related[1].code -ne 259) { throw 'EXPECTED_EXACT_FIXTURE_EFFECTS' }
    } elseif ($report.registry.exists -or $report.related.Count -ne 1 -or $report.related[0].code -ne 259) { throw 'EXPECTED_COMPLETE_FIXTURE_ABSENCE' }
    $report.status = 'PASS_QUERY_PHASE_RAW_KEYPATH_RETAINED'
} catch {
    $report['failureType'] = $_.Exception.GetType().FullName
} finally {
    $report['elapsedMilliseconds'] = $clock.ElapsedMilliseconds
    $bytes = (New-Object Text.UTF8Encoding $false).GetBytes(($report | ConvertTo-Json -Depth 8) + "`n")
    if ($bytes.Length -gt 32768) { throw 'REPORT_BOUND' }
    $output = [IO.File]::Open((Join-Path $root ($Phase + '.json')), [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::Read)
    try { $output.Write($bytes, 0, $bytes.Length); $output.Flush() } finally { $output.Dispose() }
}
if ($report.status -cne 'PASS_QUERY_PHASE_RAW_KEYPATH_RETAINED') { exit 1 }
exit 0
