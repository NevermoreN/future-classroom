# Pure checks of the exact injected CI source. This never invokes the hosted
# entrypoint, Cargo, a linker, a real executable, or filesystem/environment writes.
[CmdletBinding()]
param(
    [Parameter(Mandatory)][ValidateNotNullOrEmpty()][string]$TextSource,
    [Parameter(Mandatory)][ValidatePattern('\A[0-9a-f]{64}\z')][string]$ExpectedSourceSha256
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$script:Assertions = 0
$Utf8 = [Text.UTF8Encoding]::new($false)
$cases = [Collections.Generic.List[object]]::new()
$bindings = [Collections.Generic.List[object]]::new()
function Text-Sha256([string]$Text) {
    $hash = [Security.Cryptography.SHA256]::Create()
    try { return [BitConverter]::ToString($hash.ComputeHash($Utf8.GetBytes($Text))).Replace('-', '').ToLowerInvariant() }
    finally { $hash.Dispose() }
}
function Require([bool]$Condition, [string]$Label) {
    $script:Assertions += 1
    if (-not $Condition) { throw $Label }
}
function Reject([scriptblock]$Action, [string]$ExpectedMessage) {
    $caught = $null
    try { $discard = @(& $Action) } catch { $caught = $_ }
    Require ($null -ne $caught) 'Expected rejection did not occur.'
    Require ($caught.Exception.Message.Contains($ExpectedMessage)) 'An unrelated exception hid the expected guard.'
}
function Equal-Arguments([string[]]$Actual, [string[]]$Expected) {
    Require ($Actual.Count -eq $Expected.Count) 'Cargo argument count differs.'
    for ($index = 0; $index -lt $Actual.Count; $index++) {
        Require ($Actual[$index] -ceq $Expected[$index]) "Cargo argument at $index differs."
    }
}
function Check([string]$Name, [scriptblock]$Action) {
    $before = $script:Assertions
    try {
        $noise = @(& $Action)
        Require ($noise.Count -eq 0) 'Unexpected pipeline output.'
        $cases.Add([ordered]@{ name = $Name; status = 'PASS'; assertions = $script:Assertions - $before })
    } catch {
        $cases.Add([ordered]@{ name = $Name; status = 'FAIL'; assertions = $script:Assertions - $before;
            message = $_.Exception.Message; position = [string]$_.InvocationInfo.PositionMessage })
    }
}
# Synthetic bytes ONLY: no file is produced or executed. Header fields are the
# published PE32 offsets, deliberately independent of the extracted validator.
function New-PeFixture([UInt16]$Subsystem, [UInt16]$Minor = 1) {
    $bytes = [byte[]]::new(512)
    $bytes[0] = 0x4d; $bytes[1] = 0x5a
    [BitConverter]::GetBytes([int]128).CopyTo($bytes, 60)
    [BitConverter]::GetBytes([UInt32]0x4550).CopyTo($bytes, 128)
    [BitConverter]::GetBytes([UInt16]0x14c).CopyTo($bytes, 132)
    [BitConverter]::GetBytes([UInt16]224).CopyTo($bytes, 148)
    [BitConverter]::GetBytes([UInt16]0x10b).CopyTo($bytes, 152)
    [BitConverter]::GetBytes([UInt16]6).CopyTo($bytes, 200)
    [BitConverter]::GetBytes($Minor).CopyTo($bytes, 202)
    [BitConverter]::GetBytes($Subsystem).CopyTo($bytes, 220)
    return ,$bytes
}
$parseErrors = $null
$fatal = $null
$sourceSha = Text-Sha256 $TextSource
try {
    Require ($sourceSha -ceq $ExpectedSourceSha256) 'Injected source SHA differs.'
    $tokens = $null
    $ast = [Management.Automation.Language.Parser]::ParseInput($TextSource, [ref]$tokens, [ref]$parseErrors)
    Require (@($parseErrors).Count -eq 0) 'Whole CI source failed ParseInput.'
    foreach ($name in @('Get-MsvcBaseline', 'Get-BaselineCargoArguments', 'Get-ProductRustFlags', 'Test-MsvcExecutableHeader')) {
        $definitions = @($ast.FindAll({ param($node)
            $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -ceq $name
        }, $true))
        Require ($definitions.Count -eq 1) 'Missing/duplicate whitelisted pure helper.'
        $definition = [string]$definitions[0].Extent.Text
        $bindings.Add([ordered]@{ name = $name; sha256 = Text-Sha256 $definition; utf8Bytes = $Utf8.GetByteCount($definition) })
        $noise = @(Invoke-Expression $definition)
        Require ($noise.Count -eq 0) 'Function definition produced output.'
    }
    Check 'fixed baselines retain CRT flags and test-only console version' {
        $modern = Get-MsvcBaseline Modern
        $win7 = Get-MsvcBaseline Win7
        Require ($modern.target -ceq 'i686-pc-windows-msvc') 'Modern target changed.'
        Require ($modern.rustflags -ceq '-C target-feature=+crt-static') 'Modern shared flags changed.'
        Require ($win7.target -ceq 'i686-win7-windows-msvc') 'Win7 target changed.'
        Require ($win7.rustflags -ceq '-C target-feature=+crt-static -C link-arg=/SUBSYSTEM:CONSOLE,6.01') 'Win7 dependencies/tests must retain old console flags.'
        Require ($win7.toolchain -ceq 'nightly-2026-10-29-x86_64-pc-windows-msvc') 'Win7 pinned toolchain changed.'
    }
    Check 'normal rustc selects each closed product without a conflicting final subsystem override' {
        foreach ($binaryName in @('fc-bootstrap','fc-installer-lifecycle')) {
            $modernArgs = [string[]]@('-vv','--release','--locked','--target','i686-pc-windows-msvc','-p','fc-bootstrap','--bin',$binaryName,'--message-format=json')
            $win7Args = [string[]]@('-vv','--release','--locked','--target','i686-win7-windows-msvc','-p','fc-bootstrap','--bin',$binaryName,'--message-format=json')
            Equal-Arguments (Get-BaselineCargoArguments rustc $modernArgs Modern) (@('rustc') + $modernArgs)
            Equal-Arguments (Get-BaselineCargoArguments rustc $win7Args Win7) (@('-Z','build-std','rustc') + $win7Args)
        }
    }
    Check 'product-only environment and finally restore are wired around the real command' {
        Require ((Get-ProductRustFlags Modern) -ceq '-C target-feature=+crt-static') 'Modern product flags changed.'
        Require ((Get-ProductRustFlags Win7) -ceq '-C target-feature=+crt-static -C link-arg=/SUBSYSTEM:WINDOWS,6.01') 'Win7 product environment must be GUI6.01.'
        Require ((Get-MsvcBaseline Win7).rustflags -ceq '-C target-feature=+crt-static -C link-arg=/SUBSYSTEM:CONSOLE,6.01') 'Win7 workspace flags must stay console.'
        $productScopes = @($ast.FindAll({ param($node)
            $node -is [Management.Automation.Language.TryStatementAst] -and
            $null -ne $node.Finally -and
            $node.Finally.Extent.Text.Contains('$env:RUSTFLAGS = $originalProductRustFlags') -and
            $node.Body.Extent.Text.Contains("Invoke-Logged 'product-compile'")
        }, $true))
        Require ($productScopes.Count -eq 1) 'Product command must have exactly one containing try/finally.'
        $body = [string]$productScopes[0].Body.Extent.Text
        $final = [string]$productScopes[0].Finally.Extent.Text
        Require ($body.IndexOf('$env:RUSTFLAGS = $productRustFlags') -ge 0 -and
            $body.IndexOf('$env:RUSTFLAGS = $productRustFlags') -lt $body.IndexOf("Write-CrtFlagState 'before-product'") -and
            $body.IndexOf("Write-CrtFlagState 'before-product'") -lt $body.IndexOf("Invoke-Logged 'product-compile'")) 'Product environment is not set and captured before the command.'
        Require ($final.IndexOf('$env:RUSTFLAGS = $originalProductRustFlags') -ge 0 -and
            $final.IndexOf('$env:RUSTFLAGS = $originalProductRustFlags') -lt $final.IndexOf("Write-CrtFlagState 'after-product'")) 'Finally must restore before capturing original flags.'
        Require ($TextSource.Contains('$originalProductRustFlags = $env:RUSTFLAGS')) 'Original product environment was not captured.'
        Require ($TextSource.Contains('product_rustflags = Get-ProductRustFlags $Baseline')) 'Metadata lost product-only flag exception.'
    }
    Check 'test build clippy metadata and fmt argument contracts remain unchanged' {
        $compileArgs = [string[]]@('--workspace','--all-targets','--release','--target','i686-win7-windows-msvc')
        foreach ($command in @('test','build')) {
            Equal-Arguments (Get-BaselineCargoArguments $command $compileArgs Win7) (@('-Z','build-std',$command) + $compileArgs)
            Equal-Arguments (Get-BaselineCargoArguments $command $compileArgs Modern) (@($command) + $compileArgs)
        }
        $clippyArgs = $compileArgs + @('--','-D','warnings')
        Equal-Arguments (Get-BaselineCargoArguments clippy $clippyArgs Win7) (@('clippy','-Z','build-std') + $clippyArgs)
        Equal-Arguments (Get-BaselineCargoArguments clippy $clippyArgs Modern) (@('clippy') + $clippyArgs)
        Equal-Arguments (Get-BaselineCargoArguments metadata @('--locked','--no-deps') Win7) @('metadata','--locked','--no-deps')
        Equal-Arguments (Get-BaselineCargoArguments fmt @('--all','--','--check') Win7) @('fmt','--all','--','--check')
    }
    Check 'mis-scoped final rustc calls and wrong Win7 target fail closed' {
        $good = [string[]]@('--target','i686-win7-windows-msvc','-p','fc-bootstrap','--bin','fc-bootstrap')
        foreach ($bad in @(@('--target','i686-win7-windows-msvc'),
            @('-p','other','--bin','fc-bootstrap'), @('-p','fc-bootstrap','--bin','other'),
            ($good + @('-p','fc-bootstrap')), ($good + @('--bin','fc-bootstrap')),
            ($good + @('--')), ($good + @('--all-targets')), ($good + @('--tests')), ($good + @('--bins')))) {
            Reject { Get-BaselineCargoArguments rustc $bad Win7 } 'Product rustc requires one explicit closed fc-bootstrap product binary.'
        }
        Reject { Get-BaselineCargoArguments rustc @('--target','i686-pc-windows-msvc','-p','fc-bootstrap','--bin','fc-bootstrap') Win7 } 'Win7 compilation requires its explicit target.'
        Reject { Get-BaselineCargoArguments test @('--workspace') Win7 } 'Win7 compilation requires its explicit target.'
    }
    Check 'actual PE32 GUI product and CUI tests accepted with role-specific version' {
        Require ($null -eq (Test-MsvcExecutableHeader (New-PeFixture 2) Modern product)) 'Modern GUI product rejected.'
        Require ($null -eq (Test-MsvcExecutableHeader (New-PeFixture 3) Modern tests)) 'Modern CUI test rejected.'
        Require ((Test-MsvcExecutableHeader (New-PeFixture 2) Win7 product) -ceq '6.1') 'Win7 GUI product rejected.'
        Require ((Test-MsvcExecutableHeader (New-PeFixture 3) Win7 tests) -ceq '6.1') 'Win7 CUI test rejected.'
    }
    Check 'GUI tests and console product cannot pass by valid version alone' {
        foreach ($baseline in @('Modern','Win7')) {
            Reject { Test-MsvcExecutableHeader (New-PeFixture 3) $baseline product } 'PE subsystem differs'
            Reject { Test-MsvcExecutableHeader (New-PeFixture 2) $baseline tests } 'PE subsystem differs'
            Reject { Test-MsvcExecutableHeader (New-PeFixture 0) $baseline product } 'PE subsystem differs'
        }
        Reject { Test-MsvcExecutableHeader (New-PeFixture 2 0) Win7 product } 'must declare PE32 subsystem version 6.1'
        Reject { Test-MsvcExecutableHeader (New-PeFixture 3 0) Win7 tests } 'must declare PE32 subsystem version 6.1'
    }
    Check 'malformed PE32 headers cannot bypass actual subsystem admission' {
        Reject { Test-MsvcExecutableHeader ([byte[]]::new(255)) Modern product } 'Not a complete PE'
        $bad = New-PeFixture 2
        [BitConverter]::GetBytes([int]500).CopyTo($bad, 60)
        Reject { Test-MsvcExecutableHeader $bad Modern product } 'not Windows x86 PE32'
        $bad = New-PeFixture 2
        [BitConverter]::GetBytes([UInt16]0x8664).CopyTo($bad, 132)
        Reject { Test-MsvcExecutableHeader $bad Modern product } 'not Windows x86 PE32'
        $bad = New-PeFixture 2
        [BitConverter]::GetBytes([UInt16]0x20b).CopyTo($bad, 152)
        Reject { Test-MsvcExecutableHeader $bad Modern product } 'not Windows x86 PE32'
        $bad = New-PeFixture 2
        [BitConverter]::GetBytes([UInt16]68).CopyTo($bad, 148)
        Reject { Test-MsvcExecutableHeader $bad Modern product } 'Truncated PE32 optional header'
        $bad = New-PeFixture 2
        [BitConverter]::GetBytes([UInt16]512).CopyTo($bad, 148)
        Reject { Test-MsvcExecutableHeader $bad Modern product } 'Truncated PE32 optional header'
    }
    Check 'both real normal invocations and copy gate bind the closed targets' {
        $productCalls = @($ast.FindAll({ param($node)
            $node -is [Management.Automation.Language.CommandAst] -and $node.GetCommandName() -ceq 'Invoke-Logged' -and
            $node.CommandElements.Count -gt 1 -and $node.CommandElements[1].Extent.Text -ceq "'product-compile'"
        }, $true))
        Require ($productCalls.Count -eq 1) 'Expected exactly one normal product compilation.'
        Require ($productCalls[0].Extent.Text.Contains("Get-BaselineCargoArguments 'rustc'")) 'Product does not use final-target rustc.'
        Require ($productCalls[0].Extent.Text.Contains("'--bin','fc-bootstrap'")) 'Product is not explicitly selected.'
        $helperCalls = @($ast.FindAll({ param($node)
            $node -is [Management.Automation.Language.CommandAst] -and $node.GetCommandName() -ceq 'Invoke-Logged' -and
            $node.CommandElements.Count -gt 1 -and $node.CommandElements[1].Extent.Text -ceq "'lifecycle-compile'"
        }, $true))
        Require ($helperCalls.Count -eq 1) 'Expected exactly one normal lifecycle compilation.'
        Require ($helperCalls[0].Extent.Text.Contains("Get-BaselineCargoArguments 'rustc'")) 'Lifecycle helper does not use final-target rustc.'
        Require ($helperCalls[0].Extent.Text.Contains("'--bin','fc-installer-lifecycle'")) 'Lifecycle helper is not explicitly selected.'
        Require ($TextSource.Contains('foreach ($build in @($tests, $binary, $lifecycle))')) 'Actual normal helper Cargo messages are not collected.'
        Require ($TextSource.Contains('cargo_profile_test = [bool]$Message.profile.test')) 'Metadata lost actual Cargo test/non-test distinction.'
        Require ($TextSource.Contains('Test-MsvcExecutableHeader $bytes $Baseline $Role')) 'Copy admission lost actual role.'
        Require ($TextSource.Contains('pe_subsystem = $actualSubsystem')) 'Metadata lost actual subsystem.'
        Require ($TextSource.Contains('$LogLimit = 16MB')) 'Original stdout/stderr cap changed.'
    }
} catch {
    $fatal = [ordered]@{ kind = $_.Exception.GetType().Name; message = $_.Exception.Message;
        position = [string]$_.InvocationInfo.PositionMessage }
}
$failed = @($cases | Where-Object { $_.status -cne 'PASS' })
$status = 'PASS_PURE_POWERSHELL_GUI_CONTRACT_ONLY'
if ($null -ne $fatal -or $failed.Count -gt 0) { $status = 'FAIL_PURE_POWERSHELL_GUI_CONTRACT_ONLY' }
[ordered]@{ status = $status; powershell = $PSVersionTable.PSVersion.ToString(); sourceSha256 = $sourceSha;
    sourceExpectedSha256 = $ExpectedSourceSha256; fullSourceParseErrors = @($parseErrors).Count;
    functionBindings = $bindings.ToArray(); checks = $cases.ToArray(); assertions = $script:Assertions; fatal = $fatal;
    scope = 'AST-extracted real argument and PE admission helpers, synthetic PE32 header bytes only; no actual linker, executable, Rust build, GUI or console runtime proof.';
    ciEntrypointExecuted = $false; filesystemOrEnvironmentWritten = $false; processOrThreadStarted = $false;
    windowsRuntime = 'NOT_RUN'; actualProductPe = 'NOT_RUN'; actualRedirectedDiagnostic = 'NOT_RUN'
} | ConvertTo-Json -Depth 12 -Compress
if ($status -cne 'PASS_PURE_POWERSHELL_GUI_CONTRACT_ONLY') { throw 'Pure GUI contract checks failed; original JSON above is retained.' }
