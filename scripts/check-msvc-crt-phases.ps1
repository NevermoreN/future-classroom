# Executes the real CRT evidence function in this isolated PowerShell process.
# Its JSON writer is replaced with an in-memory sink. Only this process's two
# compiler flag variables change, and finally restores them; no files/builds.
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$TextSource,
    [Parameter(Mandatory)][ValidatePattern('\A[0-9a-f]{64}\z')][string]$ExpectedSourceSha256
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$sha = [Security.Cryptography.SHA256]::Create()
try { $digest = [BitConverter]::ToString($sha.ComputeHash([Text.Encoding]::UTF8.GetBytes($TextSource))).Replace('-', '').ToLowerInvariant() }
finally { $sha.Dispose() }
if ($digest -cne $ExpectedSourceSha256) { throw 'Injected CI source digest differs.' }
$tokens = $null; $errors = $null
$ast = [Management.Automation.Language.Parser]::ParseInput($TextSource, [ref]$tokens, [ref]$errors)
if (@($errors).Count -ne 0) { throw 'CI source parse failed.' }
foreach ($name in @('Get-MsvcBaseline', 'Get-ProductRustFlags', 'Get-CrtPhaseRustFlags', 'Write-CrtFlagState')) {
    $nodes = @($ast.FindAll({ param($node)
        $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -ceq $name
    }, $true))
    if ($nodes.Count -ne 1) { throw 'Missing or duplicated exact CRT helper.' }
    Invoke-Expression $nodes[0].Extent.Text
}
$script:Captured = [Collections.Generic.List[object]]::new()
$checks = [Collections.Generic.List[object]]::new()
$Logs = 'C:\future-classroom-memory-only'
function Write-NewJson($Path, $Value) { $script:Captured.Add(@{path=$Path;value=$Value}) }
function Reject([scriptblock]$Action, [string]$Message) {
    $caught = $null
    try { $discard = @(& $Action) } catch { $caught = $_ }
    if ($null -eq $caught -or -not $caught.Exception.Message.Contains($Message)) { throw 'Expected exact CRT rejection was not observed.' }
}
$saved = @{}
foreach ($name in @('RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS')) {
    $saved[$name] = @{present=(Test-Path -LiteralPath "Env:\$name");value=[Environment]::GetEnvironmentVariable($name,'Process')}
}
try {
    Remove-Item Env:\CARGO_ENCODED_RUSTFLAGS -ErrorAction SilentlyContinue
    foreach ($selected in @('Modern','Win7')) {
        $script:Baseline = $selected
        foreach ($phase in @('before-vs','after-vs','before-cargo','before-product','after-product')) {
            # Independent expected constants, not the new phase helper.
            $expected = '-C target-feature=+crt-static'
            if ($selected -ceq 'Win7') {
                $expected += $(if ($phase -ceq 'before-product') {' -C link-arg=/SUBSYSTEM:WINDOWS,6.01'} else {' -C link-arg=/SUBSYSTEM:CONSOLE,6.01'})
            }
            $env:RUSTFLAGS = $expected
            $before = $script:Captured.Count
            $noise = @(Write-CrtFlagState $phase $expected)
            if ($noise.Count -ne 0 -or $script:Captured.Count -ne $before + 1) { throw 'CRT evidence function did not return exactly one captured record.' }
            $value = $script:Captured[$before].value
            if ($value.phase -cne $phase -or $value.flags.RUSTFLAGS.value -cne $expected -or $value.flags.CARGO_ENCODED_RUSTFLAGS.present) { throw 'Captured actual flags differ.' }
            $checks.Add(@{baseline=$selected;phase=$phase;status='PASS_REAL_FUNCTION_MEMORY_SINK'})
        }
    }
    $script:Baseline = 'Win7'
    $console = '-C target-feature=+crt-static -C link-arg=/SUBSYSTEM:CONSOLE,6.01'
    $gui = '-C target-feature=+crt-static -C link-arg=/SUBSYSTEM:WINDOWS,6.01'
    $before = $script:Captured.Count
    Reject { Write-CrtFlagState 'unknown' $console } 'Invalid CRT flag phase.'
    Reject { Write-CrtFlagState 'BEFORE-product' $gui } 'Invalid CRT flag phase.'
    Reject { Write-CrtFlagState 'before-vs' $gui } 'Invalid required CRT flags for baseline and phase.'
    Reject { Write-CrtFlagState 'before-product' $console } 'Invalid required CRT flags for baseline and phase.'
    Reject { Write-CrtFlagState 'after-product' $gui } 'Invalid required CRT flags for baseline and phase.'
    if ($script:Captured.Count -ne $before) { throw 'Invalid phase/expectation produced a trusted record.' }
    $checks.Add(@{name='five_invalid_phase_or_expected_flag_calls';status='PASS_REJECTED'})
    $env:RUSTFLAGS = $console
    Reject { Write-CrtFlagState 'before-product' $gui } 'Actual process compiler flags differ'
    if ($script:Captured.Count -ne $before + 1 -or $script:Captured[$before].value.flags.RUSTFLAGS.value -cne $console) { throw 'Actual flag mismatch evidence lost.' }
    $env:RUSTFLAGS = $gui
    $env:CARGO_ENCODED_RUSTFLAGS = '-C opt-level=0'
    Reject { Write-CrtFlagState 'before-product' $gui } 'Actual process compiler flags differ'
    if ($script:Captured.Count -ne $before + 2 -or -not $script:Captured[$before+1].value.flags.CARGO_ENCODED_RUSTFLAGS.present) { throw 'Encoded override evidence lost.' }
    $checks.Add(@{name='actual_flag_and_encoded_override';status='PASS_REJECTED_WITH_ORIGINAL_RECORDS'})
} finally {
    foreach ($name in $saved.Keys) {
        if ($saved[$name].present) { Set-Item -LiteralPath "Env:\$name" -Value $saved[$name].value }
        else { Remove-Item -LiteralPath "Env:\$name" -ErrorAction SilentlyContinue }
    }
}
@{status='PASS_REAL_CRT_FUNCTION_MEMORY_SINK';sourceSha256=$digest;powershell=$PSVersionTable.PSVersion.ToString();checks=$checks.ToArray();acceptedPhaseCalls=10;rejectedCalls=7;capturedOriginalRecords=$script:Captured.Count;filesystemWritten=$false;childProcessStarted=$false;compilerFlagsScope='Only current isolated process, restored in finally';productCompilation='NOT_RUN'}|ConvertTo-Json -Depth 8 -Compress
