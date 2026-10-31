# In-memory pure helper checks only. The caller injects the exact UTF-8 helper
# source through TextSource; no file, environment, process, build or CI writes.
# ParseInput parses the whole script but does NOT execute its top-level/#requires.
[CmdletBinding()]
param([Parameter(Mandatory)][ValidateNotNullOrEmpty()][string]$TextSource)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$script:AssertionCount = 0
$cases = [Collections.Generic.List[object]]::new()
$rejections = [Collections.Generic.List[object]]::new()
$functionFacts = [Collections.Generic.List[object]]::new()
$expectedSourceSha = '91e79de38f7a90e8d7976d6551b614f8fdc790590e226980971fa0f2303bc315'
function Text-Sha256([string]$Text) {
    $hash = [Security.Cryptography.SHA256]::Create()
    try {
        $bytes = [Text.UTF8Encoding]::new($false).GetBytes($Text)
        return [BitConverter]::ToString($hash.ComputeHash($bytes)).Replace('-', '').ToLowerInvariant()
    } finally { $hash.Dispose() }
}
function Assert-That([bool]$Condition, [string]$Message) {
    $script:AssertionCount += 1
    if (-not $Condition) { throw $Message }
}
function Assert-FlatStrings([object]$Value, [string[]]$Expected) {
    Assert-That ($Value -is [Array]) 'Helper did not return an array.'
    Assert-That ($Value.Count -eq $Expected.Count) 'Unexpected argument count.'
    for ($index = 0; $index -lt $Expected.Count; $index++) {
        Assert-That ($Value[$index] -is [string]) 'Nested/non-string argument leaked.'
        Assert-That ($Value[$index] -ceq $Expected[$index]) "Argument differs at index $index."
    }
}
function Assert-Rejected([string]$Label, [scriptblock]$Action,
    [string]$ExpectedMessage = '', [switch]$ParameterValidation) {
    $caught = $null
    try { $discard = @(& $Action) } catch { $caught = $_ }
    Assert-That ($null -ne $caught) "Expected rejection: $Label"
    if ($ParameterValidation) {
        Assert-That ($caught.FullyQualifiedErrorId -match 'ParameterArgumentValidation') "Not ValidateSet rejection: $Label"
    }
    if ($ExpectedMessage.Length -gt 0) {
        Assert-That ($caught.Exception.Message.Contains($ExpectedMessage)) "Wrong rejection: $Label"
    }
    $rejections.Add([ordered]@{
        name = $Label; error_type = $caught.Exception.GetType().FullName
        error_id = $caught.FullyQualifiedErrorId; message = $caught.Exception.Message
    })
}
function Check-Case([string]$Name, [scriptblock]$Action) {
    $start = $script:AssertionCount
    try {
        $noise = @(& $Action)
        Assert-That ($noise.Count -eq 0) "Unexpected pipeline output in $Name."
        $cases.Add([ordered]@{ name = $Name; status = 'PASS'; assertions = $script:AssertionCount - $start })
    } catch {
        $cases.Add([ordered]@{
            name = $Name; status = 'FAIL'; assertions = $script:AssertionCount - $start
            error = $_.Exception.Message; position = [string]$_.InvocationInfo.PositionMessage
            stack = [string]$_.ScriptStackTrace
        })
    }
}
function New-SyntheticPe32([int]$Length = 512, [int]$PeOffset = 128,
    [uint16]$Machine = 0x14c, [uint16]$OptionalLength = 224,
    [uint16]$Major = 6, [uint16]$Minor = 1) {
    $bytes = [byte[]]::new($Length)
    $bytes[0] = 0x4d; $bytes[1] = 0x5a
    [BitConverter]::GetBytes([int]$PeOffset).CopyTo($bytes, 0x3c)
    [BitConverter]::GetBytes([uint32]0x4550).CopyTo($bytes, $PeOffset)
    [BitConverter]::GetBytes($Machine).CopyTo($bytes, $PeOffset + 4)
    [BitConverter]::GetBytes($OptionalLength).CopyTo($bytes, $PeOffset + 20)
    [BitConverter]::GetBytes([uint16]0x10b).CopyTo($bytes, $PeOffset + 24)
    if ($PeOffset + 76 -le $Length) {
        [BitConverter]::GetBytes($Major).CopyTo($bytes, $PeOffset + 72)
        [BitConverter]::GetBytes($Minor).CopyTo($bytes, $PeOffset + 74)
    }
    return ,$bytes
}
$sourceSha = Text-Sha256 $TextSource
$fatal = $null
$parseErrorCount = 0
try {
    Assert-That ($sourceSha -ceq $expectedSourceSha) 'Injected helper differs from the reviewed frozen source SHA.'
    $tokens = $null; $parseErrors = $null
    $ast = [System.Management.Automation.Language.Parser]::ParseInput($TextSource, [ref]$tokens, [ref]$parseErrors)
    if ($null -ne $parseErrors) { $parseErrorCount = $parseErrors.Count }
    Assert-That ($parseErrorCount -eq 0) 'Whole helper source failed ParseInput.'
    $allowed = @('Get-MsvcBaseline', 'Get-BaselineCargoArguments', 'Test-MsvcExecutableHeader')
    foreach ($name in $allowed) {
        $matches = @($ast.FindAll({
            param($node)
            $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -ceq $name
        }, $true))
        Assert-That ($matches.Count -eq 1) "Expected exactly one pure function: $name"
        $definition = [string]$matches[0].Extent.Text
        $functionFacts.Add([ordered]@{ name = $name; utf8_bytes = [Text.Encoding]::UTF8.GetByteCount($definition); sha256 = Text-Sha256 $definition })
        # Only this whitelisted FunctionDefinitionAst extent is invoked. The
        # entire helper AST, top-level expressions and #requires are never run.
        $noise = @(Invoke-Expression $definition)
        Assert-That ($noise.Count -eq 0) "Unexpected output defining $name."
    }
    $modern = Get-MsvcBaseline
    $explicitModern = Get-MsvcBaseline -Name Modern
    $win7 = Get-MsvcBaseline -Name Win7
    Check-Case 'modern-default-explicit-entire-configuration-equivalent' {
        Assert-That (($modern | ConvertTo-Json -Depth 8 -Compress) -ceq ($explicitModern | ConvertTo-Json -Depth 8 -Compress)) 'Modern default changed.'
        Assert-That ($modern.name -ceq 'Modern') 'Wrong Modern name.'
        Assert-That ($modern.toolchain -ceq '1.97.1-x86_64-pc-windows-msvc') 'Wrong fixed Modern compiler.'
        Assert-That ($modern.target -ceq 'i686-pc-windows-msvc') 'Wrong Modern target.'
        Assert-That ($modern.rustflags -ceq '-C target-feature=+crt-static') 'Modern CRT flags changed.'
    }
    Check-Case 'modern-original-install-array-exact' {
        Assert-FlatStrings $modern.install_args @('toolchain','install','1.97.1-x86_64-pc-windows-msvc','--profile','minimal','--component','clippy','--component','rustfmt','--target','i686-pc-windows-msvc')
    }
    Check-Case 'win7-fixed-nightly-target-rust-src-and-separate-root' {
        Assert-That ($win7.name -ceq 'Win7') 'Wrong Win7 name.'
        Assert-That ($win7.toolchain -ceq 'nightly-2026-10-29-x86_64-pc-windows-msvc') 'Wrong fixed Win7 compiler.'
        Assert-That ($win7.expected_rust_commit -ceq '92ffebeceaa0a071066f233873c2438e73d70fe7') 'Wrong nightly commit.'
        Assert-That ($win7.target -ceq 'i686-win7-windows-msvc') 'Wrong Win7 target.'
        Assert-That ($win7.root_prefix -cne $modern.root_prefix) 'Roots must remain separate.'
        Assert-That ($win7.marker -cne $modern.marker) 'Owner markers must remain separate.'
        Assert-That ($win7.rustflags -ceq '-C target-feature=+crt-static -C link-arg=/SUBSYSTEM:CONSOLE,6.01') 'Win7 CRT/subsystem flags changed.'
        Assert-That ($win7.cflags -ceq '/D_WIN32_WINNT=0x0601 /DWINVER=0x0601') 'Wrong C target macros.'
        Assert-FlatStrings $win7.install_args @('toolchain','install','nightly-2026-10-29-x86_64-pc-windows-msvc','--profile','minimal','--component','clippy','--component','rustfmt','--component','rust-src')
        Assert-That ([Array]::IndexOf($win7.install_args, '--target') -eq -1) 'Tier3 install must not request a rustup target distribution.'
    }
    foreach ($command in @('metadata','fmt')) {
        $arguments = if ($command -eq 'metadata') { @('--locked','--format-version','1','--no-deps') } else { @('--all','--','--check') }
        foreach ($baselineName in @('Modern','Win7')) {
            Check-Case "$baselineName-$command-flat-without-unstable-flags" {
                $actual = Get-BaselineCargoArguments -Command $command -Arguments $arguments -Name $baselineName
                Assert-FlatStrings $actual (@($command) + $arguments)
                Assert-That ([Array]::IndexOf($actual, '-Z') -eq -1) 'Noncompile command gained -Z.'
            }
        }
    }
    foreach ($command in @('test','build','clippy')) {
        $tail = switch ($command) {
            'test' { @('--no-run','--message-format=json') }
            'build' { @('-p','fc-bootstrap','--bin','fc-bootstrap','--message-format=json') }
            'clippy' { @('--','-D','warnings') }
        }
        $common = @('-vv','--release','--locked','--target')
        $modernArguments = $common + @('i686-pc-windows-msvc') + $tail
        $win7Arguments = $common + @('i686-win7-windows-msvc') + $tail
        Check-Case "modern-$command-default-explicit-flat-original-order" {
            $default = Get-BaselineCargoArguments -Command $command -Arguments $modernArguments
            $explicit = Get-BaselineCargoArguments -Command $command -Arguments $modernArguments -Name Modern
            Assert-FlatStrings $default (@($command) + $modernArguments)
            Assert-FlatStrings $explicit (@($command) + $modernArguments)
        }
        Check-Case "win7-$command-flat-build-std-prefix-original-order" {
            $actual = Get-BaselineCargoArguments -Command $command -Arguments $win7Arguments -Name Win7
            Assert-FlatStrings $actual (@('-Z','build-std',$command) + $win7Arguments)
            Assert-That ($actual[0] -ceq '-Z' -and $actual[1] -ceq 'build-std') 'Missing build-std prefix.'
            Assert-That ($actual -cnotcontains 'build-std=std,panic_abort') 'Explicit crate subset would omit test crate.'
        }
        Check-Case "win7-$command-reject-missing-trailing-or-wrong-target" {
            Assert-Rejected "$command-wrong-target" { Get-BaselineCargoArguments -Command $command -Arguments $modernArguments -Name Win7 } 'Win7 compilation requires its explicit target.'
            Assert-Rejected "$command-missing-target" { Get-BaselineCargoArguments -Command $command -Arguments @('--release') -Name Win7 } 'Win7 compilation requires its explicit target.'
            Assert-Rejected "$command-trailing-target" { Get-BaselineCargoArguments -Command $command -Arguments @('--target') -Name Win7 } 'Win7 compilation requires its explicit target.'
        }
    }
    Check-Case 'invalid-baseline-values-are-ValidateSet-rejections-for-all-three-functions' {
        Assert-Rejected 'config-invalid-baseline' { Get-MsvcBaseline -Name Other } -ParameterValidation
        Assert-Rejected 'cargo-invalid-baseline' { Get-BaselineCargoArguments -Command fmt -Arguments @('--all') -Name Other } -ParameterValidation
        $validPe = New-SyntheticPe32
        Assert-Rejected 'pe-invalid-baseline' { Test-MsvcExecutableHeader -Bytes $validPe -Name Other } -ParameterValidation
    }
    Check-Case 'unsupported-cargo-command-rejected' {
        Assert-Rejected 'unsupported-command' { Get-BaselineCargoArguments -Command clean -Arguments @('--all') -Name Win7 } 'Unsupported internal Cargo command.'
    }
    Check-Case 'modern-pe32-default-explicit-do-not-impose-win7-subsystem' {
        foreach ($minor in @(0,1,2)) {
            $bytes = New-SyntheticPe32 -Major 6 -Minor $minor
            Assert-That ($null -eq (Test-MsvcExecutableHeader -Bytes $bytes)) 'Modern default imposed a subsystem return value.'
            Assert-That ($null -eq (Test-MsvcExecutableHeader -Bytes $bytes -Name Modern)) 'Modern explicit imposed a subsystem return value.'
        }
    }
    Check-Case 'win7-valid-pe32-subsystem-6-1' {
        $bytes = New-SyntheticPe32
        Assert-That ((Test-MsvcExecutableHeader -Bytes $bytes -Name Win7) -ceq '6.1') 'Valid Win7 subsystem rejected.'
    }
    foreach ($minor in @(0,2)) {
        Check-Case "win7-reject-subsystem-6-$minor" {
            $bytes = New-SyntheticPe32 -Major 6 -Minor $minor
            Assert-Rejected "win7-subsystem-6-$minor" { Test-MsvcExecutableHeader -Bytes $bytes -Name Win7 } 'subsystem version 6.1'
        }
    }
    Check-Case 'both-baselines-reject-truncated-pe32' {
        $bytes = New-SyntheticPe32 -Length 255
        foreach ($baselineName in @('Modern','Win7')) {
            Assert-Rejected "$baselineName-truncated" { Test-MsvcExecutableHeader -Bytes $bytes -Name $baselineName } 'Not a complete PE executable.'
        }
    }
    Check-Case 'win7-reject-truncated-subsystem-fields-before-reading' {
        $bytes = New-SyntheticPe32 -Length 256 -PeOffset 216 -OptionalLength 52
        Assert-Rejected 'win7-subsystem-fields-outside-buffer' { Test-MsvcExecutableHeader -Bytes $bytes -Name Win7 } 'subsystem version 6.1'
    }
    Check-Case 'win7-reject-short-or-overflowing-declared-optional-header' {
        foreach ($length in @(51,65535)) {
            $bytes = New-SyntheticPe32 -OptionalLength $length
            Assert-Rejected "win7-optional-length-$length" { Test-MsvcExecutableHeader -Bytes $bytes -Name Win7 } 'subsystem version 6.1'
        }
    }
    Check-Case 'both-baselines-reject-wrong-machine-and-pe32-plus' {
        foreach ($baselineName in @('Modern','Win7')) {
            $bytes = New-SyntheticPe32 -Machine 0x8664
            Assert-Rejected "$baselineName-machine-x64" { Test-MsvcExecutableHeader -Bytes $bytes -Name $baselineName } 'not Windows x86 PE32'
            $bytes = New-SyntheticPe32
            [BitConverter]::GetBytes([uint16]0x20b).CopyTo($bytes, 128 + 24)
            Assert-Rejected "$baselineName-pe32-plus" { Test-MsvcExecutableHeader -Bytes $bytes -Name $baselineName } 'not Windows x86 PE32'
        }
    }
    Check-Case 'both-baselines-reject-negative-or-outside-pe-offset' {
        foreach ($baselineName in @('Modern','Win7')) {
            foreach ($offset in @(-1,[int]::MaxValue)) {
                $bytes = New-SyntheticPe32
                [BitConverter]::GetBytes([int]$offset).CopyTo($bytes, 0x3c)
                Assert-Rejected "$baselineName-pe-offset-$offset" { Test-MsvcExecutableHeader -Bytes $bytes -Name $baselineName } 'not Windows x86 PE32'
            }
        }
    }
    Check-Case 'both-baselines-reject-bad-mz-and-pe-signatures' {
        foreach ($baselineName in @('Modern','Win7')) {
            $bytes = New-SyntheticPe32
            $bytes[0] = 0
            Assert-Rejected "$baselineName-bad-mz" { Test-MsvcExecutableHeader -Bytes $bytes -Name $baselineName } 'Not a complete PE executable.'
            $bytes = New-SyntheticPe32
            $bytes[128] = 0
            Assert-Rejected "$baselineName-bad-pe" { Test-MsvcExecutableHeader -Bytes $bytes -Name $baselineName } 'not Windows x86 PE32'
        }
    }
} catch {
    $fatal = [ordered]@{ message = $_.Exception.Message; position = [string]$_.InvocationInfo.PositionMessage; stack = [string]$_.ScriptStackTrace }
}
$failed = @($cases | Where-Object { $_.status -eq 'FAIL' }).Count
$passed = @($cases | Where-Object { $_.status -eq 'PASS' }).Count
$status = if ($null -eq $fatal -and $failed -eq 0 -and $passed -gt 0) { 'PASS' } else { 'FAIL' }
[ordered]@{
    schema = 'future-classroom-win7-msvc-three-pure-functions/v1'; status = $status
    scope = 'In-memory ParseInput plus three whitelisted function extents and synthetic bytes only; no top-level/helper #requires, environment/file/process/native/CI operations.'
    powershell = $PSVersionTable.PSVersion.ToString(); dotnet = [Environment]::Version.ToString()
    text_source_sha256 = $sourceSha; expected_text_source_sha256 = $expectedSourceSha
    whole_script_parse_error_count = $parseErrorCount; functions = $functionFacts.ToArray()
    cases = $cases.Count; passed = $passed; failed = $failed; assertions = $script:AssertionCount
    checks = $cases.ToArray(); rejections = $rejections.ToArray(); fatal = $fatal
    full_script_requires = '7.2 remains; AST-only PS5.1 checks do not certify full-script PS5.1 support.'
    compilation = 'NOT_RUN'; windows7_runtime = 'NOT_RUN'; hosted_ci = 'NOT_RUN'
} | ConvertTo-Json -Depth 16 -Compress
if ($status -ne 'PASS') { throw 'Pure helper check failed; original JSON observations retained in stdout.' }
