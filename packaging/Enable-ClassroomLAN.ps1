param(
    [string]$ProgramPath = (Join-Path (Split-Path -Parent $MyInvocation.MyCommand.Path) 'fc-bootstrap.exe'),
    [ValidateRange(1, 65535)][int]$HttpsPort = 8443
)

$ErrorActionPreference = 'Stop'
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = New-Object Security.Principal.WindowsPrincipal($identity)
if (!$principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw 'Run this installation helper in an administrator PowerShell.'
}
$program = (Resolve-Path -LiteralPath $ProgramPath).Path
if ([IO.Path]::GetFileName($program) -ne 'fc-bootstrap.exe') {
    throw 'Select the installed fc-bootstrap.exe, not another program.'
}
$sha = [Security.Cryptography.SHA256]::Create()
try {
    $digest = $sha.ComputeHash([Text.Encoding]::UTF8.GetBytes($program.ToLowerInvariant()))
    $suffix = ([BitConverter]::ToString($digest)).Replace('-', '').Substring(0, 12)
} finally { $sha.Dispose() }
foreach ($rule in @(@('HTTPS', 'TCP', $HttpsPort), @('Discovery', 'UDP', 38443))) {
    $name = 'FutureClassroom-LAN-' + $suffix + '-' + $rule[0]
    # Only replace this installation path's product-owned rule; leave all
    # unrelated rules, SSH authorization and the firewall itself unchanged.
    & netsh.exe advfirewall firewall delete rule "name=$name" "program=$program" | Out-Null
    & netsh.exe advfirewall firewall add rule "name=$name" dir=in action=allow `
        "program=$program" "protocol=$($rule[1])" "localport=$($rule[2])" `
        'remoteip=10.0.0.0/8,172.16.0.0/12,192.168.0.0/16' 'profile=private,domain' enable=yes | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "Unable to prepare LAN rule: $name" }
}
Write-Output 'Classroom LAN rules prepared for private/domain networks only. HTTPS and UDP discovery still require the running application.'
