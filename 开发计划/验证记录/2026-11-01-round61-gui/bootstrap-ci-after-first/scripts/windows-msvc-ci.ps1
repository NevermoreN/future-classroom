#requires -Version 7.2
<#
Manual GitHub-hosted build evidence only. No compiled test EXE is executed here.
Official references checked for the initial workflow:
https://github.com/actions/runner-images/blob/main/images/windows/Windows2022-Readme.md
https://github.com/actions/checkout/releases/tag/v7.0.1
https://github.com/actions/upload-artifact/releases/tag/v7.0.2
https://github.com/rust-lang/rust/releases/tag/1.97.1
https://doc.rust-lang.org/rustc/platform-support/windows-msvc.html
https://doc.rust-lang.org/rustc/platform-support/win7-windows-msvc.html
https://doc.rust-lang.org/cargo/reference/unstable.html#build-std
https://doc.rust-lang.org/reference/linkage.html#static-and-dynamic-c-runtimes
https://doc.rust-lang.org/cargo/reference/external-tools.html#json-messages
https://learn.microsoft.com/en-us/dotnet/framework/tools/developer-command-prompt-for-vs
#>
[CmdletBinding()]
param(
    [ValidateSet('Build', 'Cleanup')][string]$Mode = 'Build',
    [Parameter(Mandatory)][string]$RunRoot,
    [string]$ExpectedSourceSha,
    [ValidateSet('Modern', 'Win7')][string]$Baseline = 'Modern'
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false
# Pure configuration/argument/header helpers can be AST-extracted without the
# hosted-only setup or any environment, process, file or CI side effects.
function Get-MsvcBaseline([ValidateSet('Modern', 'Win7')][string]$Name = 'Modern') {
    if ($Name -eq 'Modern') {
        return [ordered]@{
            name = 'Modern'; toolchain = '1.97.1-x86_64-pc-windows-msvc'; target = 'i686-pc-windows-msvc'
            root_prefix = 'future-classroom-msvc'; marker = 'future-classroom-msvc-ci/v1'
            role = 'modern-msvc-compilation-only'; rustflags = '-C target-feature=+crt-static'
            linker_environment = 'CARGO_TARGET_I686_PC_WINDOWS_MSVC_LINKER'; cc_environment = 'CC_i686_pc_windows_msvc'
            cflags_environment = $null; cflags = $null; expected_rust_commit = $null
            install_args = @('toolchain','install','1.97.1-x86_64-pc-windows-msvc','--profile','minimal','--component','clippy','--component','rustfmt','--target','i686-pc-windows-msvc')
        }
    }
    return [ordered]@{
        name = 'Win7'; toolchain = 'nightly-2026-10-29-x86_64-pc-windows-msvc'; target = 'i686-win7-windows-msvc'
        root_prefix = 'future-classroom-win7-msvc'; marker = 'future-classroom-win7-msvc-ci/v1'
        role = 'win7-msvc-candidate-compilation-only'
        rustflags = '-C target-feature=+crt-static -C link-arg=/SUBSYSTEM:CONSOLE,6.01'
        linker_environment = 'CARGO_TARGET_I686_WIN7_WINDOWS_MSVC_LINKER'; cc_environment = 'CC_i686_win7_windows_msvc'
        cflags_environment = 'CFLAGS_i686_win7_windows_msvc'; cflags = '/D_WIN32_WINNT=0x0601 /DWINVER=0x0601'
        expected_rust_commit = '92ffebeceaa0a071066f233873c2438e73d70fe7'
        # Tier3 has no rustup --target distribution. rust-src belongs to this same fixed toolchain.
        install_args = @('toolchain','install','nightly-2026-10-29-x86_64-pc-windows-msvc','--profile','minimal','--component','clippy','--component','rustfmt','--component','rust-src')
    }
}
function Get-BaselineCargoArguments([string]$Command, [string[]]$Arguments,
    [ValidateSet('Modern', 'Win7')][string]$Name = 'Modern') {
    if ($Command -notin @('metadata','fmt','test','build','rustc','clippy')) { throw 'Unsupported internal Cargo command.' }
    if ($Command -eq 'rustc') {
        $packageIndex = [Array]::IndexOf($Arguments, '-p')
        $binaryIndex = [Array]::IndexOf($Arguments, '--bin')
        if ($packageIndex -lt 0 -or $packageIndex + 1 -ge $Arguments.Length -or
            $Arguments[$packageIndex + 1] -cne 'fc-bootstrap' -or
            $binaryIndex -lt 0 -or $binaryIndex + 1 -ge $Arguments.Length -or
            $Arguments[$binaryIndex + 1] -cne 'fc-bootstrap' -or
            @($Arguments | Where-Object { $_ -ceq '-p' }).Count -ne 1 -or
            @($Arguments | Where-Object { $_ -ceq '--bin' }).Count -ne 1 -or
            $Arguments -contains '--' -or $Arguments -contains '--all-targets' -or
            $Arguments -contains '--tests' -or $Arguments -contains '--bins') {
            throw 'Product rustc requires only the explicit fc-bootstrap binary.'
        }
    }
    if ($Name -eq 'Win7' -and $Command -in @('test','build','rustc','clippy')) {
        $targetIndex = [Array]::IndexOf($Arguments, '--target')
        if ($targetIndex -lt 0 -or $targetIndex + 1 -ge $Arguments.Length -or
            $Arguments[$targetIndex + 1] -cne 'i686-win7-windows-msvc') { throw 'Win7 compilation requires its explicit target.' }
        # No explicit crate list: cargo test must include the test crate too.
        # Clippy is an external Cargo command; forward build-std to its inner check.
        if ($Command -eq 'clippy') { return ,(@($Command,'-Z','build-std') + $Arguments) }
        if ($Command -eq 'rustc') {
            # Only the selected final product gets this tail. Dependency/test
            # flags stay CONSOLE6.01; actual product GUI/version is checked below.
            return ,(@('-Z','build-std',$Command) + $Arguments + @('--','-C','link-arg=/SUBSYSTEM:WINDOWS,6.01'))
        }
        return ,(@('-Z','build-std',$Command) + $Arguments)
    }
    # Metadata/fmt do not compile; do not forward unstable flags to cargo-fmt.
    return ,(@($Command) + $Arguments)
}
function Test-MsvcExecutableHeader([byte[]]$Bytes,
    [ValidateSet('Modern', 'Win7')][string]$Name = 'Modern',
    [ValidateSet('tests', 'product')][string]$Role = 'tests') {
    if ($Bytes.Length -lt 256 -or $Bytes[0] -ne 0x4d -or $Bytes[1] -ne 0x5a) { throw 'Not a complete PE executable.' }
    $pe = [BitConverter]::ToInt32($Bytes, 0x3c)
    if ($pe -lt 0 -or $pe -gt $Bytes.Length - 26 -or [BitConverter]::ToUInt32($Bytes, $pe) -ne 0x4550 -or
        [BitConverter]::ToUInt16($Bytes, $pe + 4) -ne 0x14c -or [BitConverter]::ToUInt16($Bytes, $pe + 24) -ne 0x10b) {
        throw 'Executable is not Windows x86 PE32.'
    }
    # Subsystem is OptionalHeader+68. Check the actual complete field for ALL
    # baselines, rather than trusting an argument or accepting either subsystem.
    $optionalLength = [BitConverter]::ToUInt16($Bytes, $pe + 20)
    if ($pe -gt $Bytes.Length - 94 -or $optionalLength -lt 70 -or
        $optionalLength -gt $Bytes.Length - $pe - 24) { throw 'Truncated PE32 optional header.' }
    $actualSubsystem = [BitConverter]::ToUInt16($Bytes, $pe + 92)
    $expectedSubsystem = if ($Role -eq 'product') { 2 } else { 3 }
    if ($actualSubsystem -ne $expectedSubsystem) { throw 'PE subsystem differs from exact product GUI or test CUI role.' }
    if ($Name -eq 'Win7') {
        # PE32 OptionalHeader starts at PE+24; subsystem version is +48/+50.
        if ([BitConverter]::ToUInt16($Bytes, $pe + 72) -ne 6 -or
            [BitConverter]::ToUInt16($Bytes, $pe + 74) -ne 1) { throw 'Win7 candidate must declare PE32 subsystem version 6.1.' }
        return '6.1'
    }
    return $null
}
$BaselineConfiguration = Get-MsvcBaseline $Baseline
$Toolchain = $BaselineConfiguration.toolchain
$Target = $BaselineConfiguration.target
$LogLimit = 16MB # Per stdout/stderr stream: preserve original prefix, fail on overflow.
$Utf8 = [Text.UTF8Encoding]::new($false)
$SourceRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))

function Assert-OwnedRoot {
    if (-not $IsWindows -or $env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted') {
        throw 'This script only changes a fresh GitHub-hosted Windows runner.'
    }
    if ($env:GITHUB_REPOSITORY -cne 'NevermoreN/future-classroom' -or
        $env:GITHUB_RUN_ID -notmatch '^\d+$' -or $env:GITHUB_RUN_ATTEMPT -notmatch '^\d+$') {
        throw 'Unexpected repository/run identity.'
    }
    $expected = [IO.Path]::GetFullPath((Join-Path $env:RUNNER_TEMP "$($BaselineConfiguration.root_prefix)-$env:GITHUB_RUN_ID-$env:GITHUB_RUN_ATTEMPT"))
    if (-not [string]::Equals([IO.Path]::GetFullPath($RunRoot), $expected, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'Refusing a directory outside this exact run-id/attempt root.'
    }
    $item = Get-Item -LiteralPath $RunRoot
    if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0 -or
        [IO.File]::ReadAllText((Join-Path $RunRoot '.owner')) -cne $BaselineConfiguration.marker) {
        throw 'Run directory ownership marker/reparse check failed.'
    }
}
Assert-OwnedRoot
$Artifact = Join-Path $RunRoot 'artifact'
$Logs = Join-Path $Artifact 'logs'
$Steps = [Collections.Generic.List[object]]::new()

function Write-NewJson([string]$Path, $Value, [long]$MaximumBytes = 0) {
    $bytes = $Utf8.GetBytes(($Value | ConvertTo-Json -Depth 30) + "`n")
    if ($MaximumBytes -gt 0 -and $bytes.LongLength -gt $MaximumBytes) { throw 'Source manifest JSON exceeds its byte bound.' }
    $file = [IO.File]::Open($Path, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::Read)
    try { $file.Write($bytes, 0, $bytes.Length) } finally { $file.Dispose() }
}
function Write-Checksums {
    $entries = @(Get-ChildItem -LiteralPath $Artifact -File -Recurse |
        Where-Object { $_.Name -ne 'artifact-sha256.json' } | Sort-Object FullName | ForEach-Object {
            [ordered]@{
                path = [IO.Path]::GetRelativePath($Artifact, $_.FullName).Replace('\', '/')
                bytes = $_.Length; sha256 = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
            }
        })
    Write-NewJson (Join-Path $Artifact 'artifact-sha256.json') $entries
}

if ($Mode -eq 'Cleanup') {
    $outcomes = [Collections.Generic.List[object]]::new()
    $retained = $false
    # Only exact PID+start-time+image matches started by this run may be killed.
    foreach ($record in Get-ChildItem -LiteralPath $Logs -Filter '*.active.json' -File) {
        try { $saved = Get-Content -LiteralPath $record.FullName -Raw | ConvertFrom-Json } catch {
            $retained = $true
            $outcomes.Add([ordered]@{ process_record = $record.Name; status = 'UNREADABLE'; reason = $_.Exception.Message })
            continue
        }
        try { $process = [Diagnostics.Process]::GetProcessById([int]$saved.pid) } catch { continue }
        try {
            if ($process.StartTime.ToUniversalTime().Ticks -eq [long]$saved.start_ticks -and
                [string]::Equals($process.MainModule.FileName, $saved.image, [StringComparison]::OrdinalIgnoreCase)) {
                $process.Kill($true)
                if (-not $process.WaitForExit(10000)) { $retained = $true }
            }
        } catch { $retained = $true } finally { $process.Dispose() }
    }
    foreach ($name in @('target', 'scratch', 'cargo', 'rustup')) {
        $path = Join-Path $RunRoot $name
        try {
            if (Test-Path -LiteralPath $path) {
                foreach ($item in @((Get-Item -LiteralPath $path)) + @(Get-ChildItem -LiteralPath $path -Recurse -Force)) {
                    if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
                        throw 'Refuse recursive deletion through a reparse point.'
                    }
                }
                Remove-Item -LiteralPath $path -Recurse -Force
            }
            $outcomes.Add([ordered]@{ root = $name; removed = -not (Test-Path -LiteralPath $path) })
        } catch {
            $retained = $true
            $outcomes.Add([ordered]@{ root = $name; removed = $false; reason = $_.Exception.Message })
        }
    }
    Write-NewJson (Join-Path $Artifact 'cleanup.json') ([ordered]@{
        status = $(if ($retained) { 'RETAINED' } else { 'OWNED_BUILD_ROOTS_REMOVED' })
        roots = $outcomes.ToArray(); artifact_lifetime = 'Retained for always-upload; hosted VM disposal removes remaining local evidence.'
        scope = 'Only this exact RunnerTemp run-id/attempt root; no user-machine cleanup or runtime tests.'
    })
    Write-Checksums
    if ($retained) { exit 1 } else { exit 0 }
}

function Resolve-Application([string]$Name) {
    # -All preserves execution precedence; duplicate PATH entries are legitimate.
    $commands = @(Get-Command -Name $Name -CommandType Application -All -ErrorAction Stop |
        Select-Object -First 1)
    if ($commands.Count -ne 1) { throw "Application not found: $Name" }
    $source = [string]$commands[0].Source
    if ([string]::IsNullOrWhiteSpace($source) -or -not [IO.Path]::IsPathRooted($source)) {
        throw "Application path is not absolute: $Name"
    }
    $path = [IO.Path]::GetFullPath($source)
    # IsPathRooted alone also accepts C:relative and \drive-relative on Windows.
    $sourceRoot = [IO.Path]::GetPathRoot($source).Replace('/', '\')
    $fullRoot = [IO.Path]::GetPathRoot($path).Replace('/', '\')
    if (-not [string]::Equals($sourceRoot, $fullRoot, [StringComparison]::OrdinalIgnoreCase) -or
        -not [IO.File]::Exists($path)) {
        throw "Application path is not an existing absolute file: $Name"
    }
    return $path
}

function Write-CrtFlagState([string]$Phase, [string]$ExpectedRustFlags = '-C target-feature=+crt-static') {
    if ($ExpectedRustFlags -cnotin @('-C target-feature=+crt-static', '-C target-feature=+crt-static -C link-arg=/SUBSYSTEM:CONSOLE,6.01')) { throw 'Invalid required CRT flags.' }
    if ($Phase -notin @('before-vs', 'after-vs', 'before-cargo')) { throw 'Invalid CRT flag phase.' }
    $flags = [ordered]@{}
    foreach ($name in @('RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS')) {
        $value = [Environment]::GetEnvironmentVariable($name, 'Process')
        if ($null -ne $value -and $value.Length -gt 4096) { throw 'Compiler flag evidence exceeds bound.' }
        $flags[$name] = [ordered]@{
            present = (Test-Path -LiteralPath "Env:\$name"); value = $value
            length = $(if ($null -eq $value) { $null } else { $value.Length })
        }
    }
    Write-NewJson (Join-Path $Logs "$Phase-crt-flags.json") ([ordered]@{
        phase = $Phase; powershell = $PSVersionTable.PSVersion.ToString()
        dotnet = [Environment]::Version.ToString(); flags = $flags
        requirement = "Encoded variable absent; RUSTFLAGS exactly $ExpectedRustFlags."
        explanation = 'Cargo prefers encoded flags, including an empty value. PowerShell 7.5 API null leaves an empty variable; use the Env provider to remove it.'
        scope = 'Two exact process variables only; no environment dump.'
    })
    if ($flags['CARGO_ENCODED_RUSTFLAGS'].present -or
        $null -ne $flags['CARGO_ENCODED_RUSTFLAGS'].value -or
        $flags['RUSTFLAGS'].value -cne $ExpectedRustFlags) {
        throw 'Actual process compiler flags differ from the required static CRT selection; original flag evidence retained.'
    }
}

function Restore-ProcessEnvironment($Saved) {
    foreach ($name in $Saved.Keys) {
        if ($Saved[$name].present) {
            # The provider preserves an original empty value on PowerShell 7.5+.
            Set-Item -LiteralPath "Env:\$name" -Value $Saved[$name].value
        } else {
            Remove-Item -LiteralPath "Env:\$name" -ErrorAction SilentlyContinue
        }
    }
}

function Get-ActiveProcessIdentity($Process, [DateTime]$Deadline) {
    if ($Process.HasExited) { return $null }
    try {
        $processId = $Process.Id
        $startTicks = $Process.StartTime.ToUniversalTime().Ticks
        while ($true) {
            if ($Process.HasExited) { return $null }
            if ([DateTime]::UtcNow -ge $Deadline) { throw 'Live process identity deadline expired.' }
            $module = $Process.MainModule
            if ($null -ne $module) {
                $image = [string]$module.FileName
                if (-not [string]::IsNullOrWhiteSpace($image)) {
                    if ($Process.HasExited) { return $null }
                    if ([DateTime]::UtcNow -ge $Deadline) { throw 'Live process identity deadline expired.' }
                    return [ordered]@{ pid = $processId; start_ticks = $startTicks; image = $image }
                }
            }
            # Startup can temporarily expose no module or an empty image path.
            if ($Process.HasExited) { return $null }
            if ([DateTime]::UtcNow -ge $Deadline) { throw 'Live process identity deadline expired.' }
            $Process.Refresh()
            Start-Sleep -Milliseconds 1
        }
    } catch {
        # The same process may exit between HasExited and the module query.
        if ($Process.HasExited) { return $null }
        throw
    }
}

function Invoke-Logged {
    param([string]$Name, [string]$Executable, [string[]]$Arguments, [int]$TimeoutSeconds = 120,
        [int[]]$AllowedExitCodes = @(0))
    if ($Name -notmatch '^[a-z0-9-]+$') { throw 'Invalid internal log name.' }
    $info = [Diagnostics.ProcessStartInfo]::new()
    $info.FileName = $Executable; $info.WorkingDirectory = $SourceRoot
    $info.UseShellExecute = $false; $info.CreateNoWindow = $true
    $info.RedirectStandardOutput = $true; $info.RedirectStandardError = $true
    foreach ($argument in $Arguments) { $info.ArgumentList.Add($argument) }
    $stdoutPath = Join-Path $Logs "$Name.stdout.log"
    $stderrPath = Join-Path $Logs "$Name.stderr.log"
    $outFile = [IO.File]::Open($stdoutPath, 'CreateNew', 'Write', 'Read')
    $errFile = [IO.File]::Open($stderrPath, 'CreateNew', 'Write', 'Read')
    $process = [Diagnostics.Process]::new(); $process.StartInfo = $info
    $processStarted = $false
    $started = [DateTime]::UtcNow
    $timedOut = $false; $truncated = $false; $exited = $false; $exitCode = $null
    try {
        if (-not $process.Start()) { throw "Cannot start $Name" }
        $processStarted = $true
        $identity = Get-ActiveProcessIdentity $process ($started.AddSeconds([Math]::Min(5, $TimeoutSeconds)))
        if ($null -ne $identity) { Write-NewJson (Join-Path $Logs "$Name.active.json") $identity }
        $streams = @(
            @{ reader = $process.StandardOutput.BaseStream; file = $outFile; buffer = [byte[]]::new(16384); pending = $null; ended = $false; kept = 0L },
            @{ reader = $process.StandardError.BaseStream; file = $errFile; buffer = [byte[]]::new(16384); pending = $null; ended = $false; kept = 0L }
        )
        foreach ($stream in $streams) { $stream.pending = $stream.reader.ReadAsync($stream.buffer, 0, $stream.buffer.Length) }
        $exitObserved = $null
        while ($true) {
            foreach ($stream in $streams) {
                if (-not $stream.ended -and $stream.pending.IsCompleted) {
                    $count = $stream.pending.GetAwaiter().GetResult()
                    if ($count -eq 0) { $stream.ended = $true; continue }
                    $keep = [int][Math]::Min([long]$count, [long]$LogLimit - $stream.kept)
                    if ($keep -gt 0) { $stream.file.Write($stream.buffer, 0, $keep); $stream.kept += $keep }
                    if ($keep -lt $count) { $truncated = $true }
                    $stream.pending = $stream.reader.ReadAsync($stream.buffer, 0, $stream.buffer.Length)
                }
            }
            $exited = $process.HasExited
            if ($exited -and $null -eq $exitObserved) { $exitObserved = [DateTime]::UtcNow }
            if ($exited -and $streams[0].ended -and $streams[1].ended) { break }
            if (([DateTime]::UtcNow - $started).TotalSeconds -ge $TimeoutSeconds -or $truncated -or
                ($null -ne $exitObserved -and ([DateTime]::UtcNow - $exitObserved).TotalSeconds -ge 10)) {
                $timedOut = $true
                try { $process.Kill($true) } catch { }
                $exited = $process.WaitForExit(10000)
                break
            }
            Start-Sleep -Milliseconds 10
        }
        if ($exited) { $exitCode = $process.ExitCode }
    } finally {
        if ($processStarted -and -not $process.HasExited) { try { $process.Kill($true) } catch { } }
        $outFile.Dispose(); $errFile.Dispose(); $process.Dispose()
    }
    $step = [ordered]@{
        name = $Name; executable = $Executable; arguments = $Arguments
        started_utc = $started.ToString('o'); duration_seconds = ([DateTime]::UtcNow - $started).TotalSeconds
        exit_code = $exitCode; parent_exited = $exited; timed_out_or_pipe_stalled = $timedOut
        log_truncated = $truncated; log_limit_bytes_per_stream = $LogLimit
        stdout = "logs/$Name.stdout.log"; stderr = "logs/$Name.stderr.log"
        passed = (-not $timedOut -and -not $truncated -and $exited -and ($AllowedExitCodes -contains $exitCode))
    }
    $Steps.Add($step)
    Write-NewJson (Join-Path $Logs "$Name.result.json") $step
    Write-Host "$Name`: exit=$exitCode passed=$($step.passed) duration=$([int]$step.duration_seconds)s"
    return $step
}
function Require-Step($Step) { if (-not $Step.passed) { throw "Prerequisite failed: $($Step.name); original logs retained." } }
function Read-Output($Step) { [IO.File]::ReadAllText((Join-Path $Artifact $Step.stdout), $Utf8) }
# Historical evidence is Git-bound data, not a second copy of current build
# inputs. The exact slash-terminated prefix is the ONLY excluded byte domain.
function Get-SourceInputDomain([string]$Relative) {
    if ([string]::IsNullOrEmpty($Relative) -or [IO.Path]::IsPathRooted($Relative) -or
        $Relative -match '[\\:\x00-\x1f]') { throw 'Non-canonical relative Git input.' }
    foreach ($part in $Relative.Split('/')) {
        if ($part -in @('', '.', '..')) { throw 'Non-canonical Git path segment.' }
    }
    if ($Relative.StartsWith('开发计划/验证记录/', [StringComparison]::Ordinal)) { return 'historical_git_archive' }
    return 'sha256_input'
}
function ConvertFrom-SourceGitTree([string]$Raw) {
    # Fixed 20,000-path metadata inventory includes historical evidence; byte budgets stay unchanged.
    # Invoke-Logged also preserves the original prefix and fails on truncation.
    if ($Raw.Length -eq 0 -or $Utf8.GetByteCount($Raw) -gt $LogLimit -or
        $Raw[$Raw.Length - 1] -ne [char]0) { throw 'Git tree metadata empty, truncated or over bound.' }
    $records = $Raw.Split([char[]]@([char]0), 20002, [StringSplitOptions]::None)
    if ($records.Length - 1 -gt 20000) { throw 'Source file inventory exceeds bound.' }
    $entries = [Collections.Generic.List[object]]::new()
    $names = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    for ($number = 0; $number -lt $records.Length - 1; $number++) {
        $record = $records[$number]
        $tab = $record.IndexOf([char]9)
        if ($tab -lt 0) { throw 'Malformed Git tree metadata record.' }
        $header = $record.Substring(0, $tab)
        $relative = $record.Substring($tab + 1)
        if ($header -cnotmatch '\A(100644|100755) blob ([0-9a-f]{40}) +([0-9]+)\z') {
            throw 'Git inputs must be regular blobs with a mode, object id and byte size.'
        }
        $mode = $Matches[1]; $objectId = $Matches[2]; $sizeText = $Matches[3]
        $size = 0L
        if (-not [long]::TryParse($sizeText, [ref]$size) -or $size -gt 64MB) { throw 'Git blob exceeds per-file bound.' }
        $domain = Get-SourceInputDomain $relative
        if (-not $names.Add($relative)) { throw 'Duplicate/case-colliding Git input path.' }
        $entries.Add([ordered]@{
            path = $relative; mode = $mode; type = 'blob'
            git_blob_oid_sha1 = $objectId; git_blob_bytes = $size; domain = $domain
        })
    }
    return ,$entries.ToArray()
}
function Assert-NoHistoricalBuildReference([string]$Relative, [string]$Path) {
    # Conservative explicit-path policy, not a sandbox for arbitrary build.rs IO.
    # Inspect only Rust/Cargo configuration inputs, not prose or this CI script.
    $name = [IO.Path]::GetFileName($Relative)
    if (-not ($Relative.EndsWith('.rs', [StringComparison]::OrdinalIgnoreCase) -or
        $name -ieq 'Cargo.toml' -or $name -ieq 'rust-toolchain.toml' -or
        $Relative.StartsWith('.cargo/', [StringComparison]::Ordinal))) { return }
    $text = [IO.File]::ReadAllText($Path, $Utf8)
    # Both Rust escaped backslashes and ordinary Windows separators normalize.
    $normalized = [regex]::Replace($text.Replace([char]92, [char]47), '/+', '/')
    if ($normalized.IndexOf('开发计划/验证记录/', [StringComparison]::Ordinal) -ge 0) {
        throw 'Explicit build input references historical archive; move it into the SHA256 input domain first.'
    }
}
function Hash-Inputs([string[]]$Paths, [object[]]$GitEntries) {
    if ($Paths.Count -gt 20000 -or $GitEntries.Count -gt 20000 -or $Paths.Count -ne $GitEntries.Count) {
        throw 'Source file inventory count/tree mismatch.'
    }
    $tree = [Collections.Generic.Dictionary[string,object]]::new([StringComparer]::Ordinal)
    foreach ($entry in $GitEntries) { $tree.Add([string]$entry.path, $entry) }
    $names = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    $entries = [Collections.Generic.List[object]]::new()
    $history = [Collections.Generic.List[object]]::new()
    $total = 0L; $historyCheckout = 0L; $historyBlob = 0L
    foreach ($relative in ($Paths | Sort-Object -CaseSensitive)) {
        $domain = Get-SourceInputDomain $relative
        if (-not $names.Add($relative) -or -not $tree.ContainsKey($relative)) { throw 'Tracked paths differ from HEAD tree.' }
        $path = [IO.Path]::GetFullPath((Join-Path $SourceRoot $relative))
        if (-not $path.StartsWith($SourceRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw 'Input escapes source root.' }
        $file = Get-Item -LiteralPath $path -Force
        if ($file -isnot [IO.FileInfo] -or $file.Length -gt 64MB -or
            ($file.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { throw 'Source inventory size/reparse bound exceeded.' }
        # A regular leaf under a reparse directory must not bypass the guard.
        $parent = $file.Directory
        while ($null -ne $parent) {
            if (($parent.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { throw 'Source parent is a reparse point.' }
            if ([string]::Equals($parent.FullName, $SourceRoot, [StringComparison]::OrdinalIgnoreCase)) { break }
            $parent = $parent.Parent
        }
        if ($null -eq $parent) { throw 'Input ancestor escaped source root.' }
        if ($domain -eq 'historical_git_archive') {
            # FileInfo/reparse checks above are retained. No Get-FileHash/read
            # of historical payload occurs here; Git status may independently
            # read it while checking that the checkout is clean.
            $historyCheckout += $file.Length
            $historyBlob += [long]$tree[$relative].git_blob_bytes
            $history.Add([ordered]@{ path = $relative; checkout_bytes = $file.Length })
        } else {
            $total += $file.Length
            if ($total -gt 256MB) { throw 'SHA256 input inventory exceeds total byte bound.' }
            Assert-NoHistoricalBuildReference $relative $path
            $entries.Add([ordered]@{ path = $relative; bytes = $file.Length
                sha256 = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant() })
        }
    }
    return [ordered]@{
        sha256_files = $entries.ToArray(); sha256_checkout_bytes = $total
        historical_checkout = $history.ToArray(); historical_checkout_bytes = $historyCheckout
        historical_git_blob_bytes = $historyBlob
    }
}
function New-SourceDomainSummary($Inventory, [string]$HeadSha, [string]$TreeId) {
    return [ordered]@{
        schema = 'future-classroom.ci-input-domains/v1'; source_sha = $HeadSha; root_tree_oid_sha1 = $TreeId
        tracked_file_count = $Inventory.sha256_files.Count + $Inventory.historical_checkout.Count
        explicit_build_reference_policy = 'Rust/Cargo input text may not explicitly reference the historical prefix, including Windows separators. This guard is not hermetic and cannot prove absence of arbitrary dynamic build.rs IO.'
        limits = [ordered]@{ paths = 20000; per_file_bytes = 64MB; sha256_input_total_bytes = 256MB; metadata_output_bytes = $LogLimit }
        sha256_input = [ordered]@{
            manifest = 'input-sha256.json'; file_count = $Inventory.sha256_files.Count
            checkout_bytes_hashed = $Inventory.sha256_checkout_bytes
            scope = 'Every tracked file outside the exact historical prefix; actual checkout SHA256 before and after.'
        }
        historical_git_archive = [ordered]@{
            exact_prefix = '开发计划/验证记录/'; file_count = $Inventory.historical_checkout.Count
            git_blob_bytes = $Inventory.historical_git_blob_bytes; checkout_metadata_bytes = $Inventory.historical_checkout_bytes
            git_manifest = 'input-git-tree.json'; checkout_metadata = $Inventory.historical_checkout
            content_sha256 = 'NOT_READ_BY_INPUT_HASHER'
            scope = 'HEAD tree/blob mode/type/object SHA1/size binding; current file metadata/reparse and clean Git guards. Git blob sizes/ids are not checkout SHA256; CRLF may change checkout bytes.'
        }
    }
}
function File-Fact([string]$Path) {
    $file = Get-Item -LiteralPath $Path
    [ordered]@{ path = $file.FullName; bytes = $file.Length;
        file_version = $file.VersionInfo.FileVersion;
        sha256 = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant() }
}
function Copy-Executable($Message, [string]$Role, [string]$PackageName, [string]$PackagePath) {
    if ($Role -notin @('tests','product') -or $PackageName -notmatch '^[a-z0-9-]+$') { throw 'Invalid internal artifact namespace.' }
    $source = [IO.Path]::GetFullPath([string]$Message.executable)
    $targetRoot = [IO.Path]::GetFullPath((Join-Path $RunRoot 'target'))
    if (-not $source.StartsWith($targetRoot + '\', [StringComparison]::OrdinalIgnoreCase) -or
        [IO.Path]::GetExtension($source) -ine '.exe') { throw 'Cargo executable escaped the owned build root.' }
    $item = Get-Item -LiteralPath $source
    if ($item.Length -gt 128MB -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { throw 'Executable bound/reparse rejected.' }
    if ($script:PayloadBytes + $item.Length -gt 512MB) { throw 'Executable package byte bound exceeded before copying.' }
    $bytes = [IO.File]::ReadAllBytes($source)
    $subsystemVersion = Test-MsvcExecutableHeader $bytes $Baseline $Role
    $actualSubsystem = [BitConverter]::ToUInt16($bytes, [BitConverter]::ToInt32($bytes, 0x3c) + 92)
    $relative = "$Role/$PackageName/$($item.Name)"
    $destination = Join-Path $Artifact $relative
    New-Item -ItemType Directory -Path ([IO.Path]::GetDirectoryName($destination)) -Force | Out-Null
    if (Test-Path -LiteralPath $destination) { throw 'Duplicate artifact path.' }
    Copy-Item -LiteralPath $source -Destination $destination
    $script:PayloadBytes += $item.Length
    [ordered]@{
        role = $Role; package = $PackageName; target = $Message.target.name
        kinds = $Message.target.kind; relative_path = $relative; package_working_directory = $PackagePath
        bytes = $item.Length; sha256 = (Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash.ToLowerInvariant()
        pe_machine = '0x014c'; pe_optional_magic = '0x010b'; pe_subsystem = $actualSubsystem; pe_subsystem_version = $subsystemVersion; executed = $false
    }
}

$savedEnvironment = @{}
foreach ($name in @('RUSTUP_HOME','CARGO_HOME','RUSTUP_TOOLCHAIN','CARGO_TARGET_DIR','CARGO_BUILD_JOBS','CARGO_TERM_COLOR',
    'CARGO_ENCODED_RUSTFLAGS','RUSTFLAGS','CARGO_TARGET_I686_PC_WINDOWS_MSVC_LINKER','CC_i686_pc_windows_msvc',
    'CARGO_TARGET_I686_WIN7_WINDOWS_MSVC_LINKER','CC_i686_win7_windows_msvc','CFLAGS_i686_win7_windows_msvc','TEMP','TMP')) {
    $savedEnvironment[$name] = @{
        present = (Test-Path -LiteralPath "Env:\$name")
        value = [Environment]::GetEnvironmentVariable($name, 'Process')
    }
}
$failure = $null
$executables = [Collections.Generic.List[object]]::new()
$script:PayloadBytes = 0L
$inputDigest = $null
$gitTreeDigest = $null
$inputDomainsDigest = $null
$inputDomainSummary = $null
Write-NewJson (Join-Path $Artifact 'run-start.json') ([ordered]@{
    status = 'STARTED_NOT_VALIDATED'; utc = [DateTime]::UtcNow.ToString('o'); expected_source_sha = $ExpectedSourceSha
    dispatch_source_sha = $env:GITHUB_SHA
    workflow_sha = $env:GITHUB_WORKFLOW_SHA; target = $Target; toolchain = $Toolchain
    rustflags = $BaselineConfiguration.rustflags; baseline = $Baseline; role = $BaselineConfiguration.role; tests = 'COMPILE_ONLY_NO_RUN'
    windows7 = 'NOT_RUN'; windows10 = 'NOT_RUN'; windows11 = 'NOT_RUN'; seewo_device = 'NOT_RUN'
})
try {
    if ($ExpectedSourceSha -notmatch '\A[0-9a-fA-F]{40}\z' -or
        $env:GITHUB_SHA -notmatch '\A[0-9a-fA-F]{40}\z') {
        throw 'Expected and dispatch source SHA must each be exactly 40 hexadecimal characters.'
    }
    if ($ExpectedSourceSha -ine $env:GITHUB_SHA) {
        throw 'Dispatch source differs from requested source_sha; no build admitted.'
    }
    $git = Resolve-Application 'git.exe'
    $sha = Invoke-Logged 'source-sha' $git @('rev-parse','HEAD')
    Require-Step $sha
    $checkedOutSha = (Read-Output $sha).Trim()
    if ($checkedOutSha -notmatch '\A[0-9a-fA-F]{40}\z' -or
        $checkedOutSha -ine $ExpectedSourceSha -or $checkedOutSha -ine $env:GITHUB_SHA) {
        throw 'Checked-out source differs from requested/dispatch Git SHA; no build admitted.'
    }
    $rustup = Resolve-Application 'rustup.exe'
    $cargo = Resolve-Application 'cargo.exe'
    $python = Resolve-Application 'python.exe'
    $clean = Invoke-Logged 'source-clean-before' $git @('status','--porcelain','--untracked-files=all')
    Require-Step $clean
    if ((Read-Output $clean).Length -ne 0) { throw 'Source is not a clean checkout.' }
    $tracked = Invoke-Logged 'source-input-paths' $git @('ls-files','-z')
    Require-Step $tracked
    $paths = (Read-Output $tracked).Split([char[]]@([char]0), 20001, [StringSplitOptions]::RemoveEmptyEntries)
    $treeStep = Invoke-Logged 'source-git-tree-before' $git @('ls-tree','-r','-z','-l','--full-tree','HEAD')
    Require-Step $treeStep
    $gitEntries = ConvertFrom-SourceGitTree (Read-Output $treeStep)
    $treeIdStep = Invoke-Logged 'source-git-tree-id-before' $git @('rev-parse','HEAD^{tree}')
    Require-Step $treeIdStep
    $treeId = (Read-Output $treeIdStep).Trim()
    if ($treeId -cnotmatch '\A[0-9a-f]{40}\z') { throw 'Invalid HEAD root tree object id.' }
    $inventory = Hash-Inputs $paths $gitEntries
    $inputs = $inventory.sha256_files
    $gitManifest = [ordered]@{ schema = 'future-classroom.ci-git-tree-inputs/v1'
        source_sha = $ExpectedSourceSha; root_tree_oid_sha1 = $treeId; files = $gitEntries }
    $inputDomainSummary = New-SourceDomainSummary $inventory $ExpectedSourceSha $treeId
    Write-NewJson (Join-Path $Artifact 'input-sha256.json') $inputs $LogLimit
    Write-NewJson (Join-Path $Artifact 'input-git-tree.json') $gitManifest $LogLimit
    Write-NewJson (Join-Path $Artifact 'input-domains.json') $inputDomainSummary $LogLimit
    $inputDigest = (Get-FileHash -LiteralPath (Join-Path $Artifact 'input-sha256.json')).Hash.ToLowerInvariant()
    $gitTreeDigest = (Get-FileHash -LiteralPath (Join-Path $Artifact 'input-git-tree.json')).Hash.ToLowerInvariant()
    $inputDomainsDigest = (Get-FileHash -LiteralPath (Join-Path $Artifact 'input-domains.json')).Hash.ToLowerInvariant()
    if ((Get-Content -LiteralPath (Join-Path $SourceRoot 'rust-toolchain.toml') -Raw) -notmatch 'channel\s*=\s*"1\.97\.1"') {
        throw 'Repository toolchain does not match fixed CI Rust 1.97.1.'
    }
    $os = Get-CimInstance Win32_OperatingSystem
    Write-NewJson (Join-Path $Artifact 'host.json') ([ordered]@{
        caption = $os.Caption; version = $os.Version; build = $os.BuildNumber; architecture = $os.OSArchitecture
        powershell = $PSVersionTable.PSVersion.ToString(); image = $env:ImageOS; image_version = $env:ImageVersion
        runner_environment = $env:RUNNER_ENVIRONMENT; compiler_host = 'x86_64-pc-windows-msvc'
        source_sha = $ExpectedSourceSha; input_manifest_sha256 = $inputDigest
        git_tree_manifest_sha256 = $gitTreeDigest; input_domains_manifest_sha256 = $inputDomainsDigest
    })
    foreach ($name in @('target','scratch','cargo','rustup')) { New-Item -ItemType Directory -Path (Join-Path $RunRoot $name) | Out-Null }
    $env:RUSTUP_HOME = Join-Path $RunRoot 'rustup'; $env:CARGO_HOME = Join-Path $RunRoot 'cargo'
    $env:RUSTUP_TOOLCHAIN = $Toolchain; $env:CARGO_TARGET_DIR = Join-Path $RunRoot 'target'
    $env:CARGO_BUILD_JOBS = '2'; $env:CARGO_TERM_COLOR = 'never'
    # The API null setter leaves an empty value in PowerShell 7.5, which Cargo
    # would prefer over RUSTFLAGS. Provider null truly removes the variable.
    $env:CARGO_ENCODED_RUSTFLAGS = $null
    $env:RUSTFLAGS = $BaselineConfiguration.rustflags
    $env:TEMP = Join-Path $RunRoot 'scratch'; $env:TMP = $env:TEMP
    Require-Step (Invoke-Logged 'rustup-version' $rustup @('--version'))
    Require-Step (Invoke-Logged 'install-fixed-rust' $rustup $BaselineConfiguration.install_args 600)
    $rustVersion = Invoke-Logged 'rust-version' $rustup @('run',$Toolchain,'rustc','-vV')
    Require-Step $rustVersion
    Require-Step (Invoke-Logged 'cargo-version' $cargo @('--version','--verbose'))
    if ($Baseline -eq 'Win7') {
        $rustDetails = Read-Output $rustVersion
        if ($rustDetails -notmatch '(?m)^commit-hash: 92ffebeceaa0a071066f233873c2438e73d70fe7\r?$' -or
            $rustDetails -notmatch '(?m)^release: 1\.101\.0-nightly\r?$' -or
            $rustDetails -notmatch '(?m)^host: x86_64-pc-windows-msvc\r?$') { throw 'Win7 compiler differs from fixed nightly commit/host/release.' }
        Require-Step (Invoke-Logged 'rust-src-components' $rustup @('component','list','--installed','--toolchain',$Toolchain))
        $sysrootStep = Invoke-Logged 'rust-sysroot' $rustup @('run',$Toolchain,'rustc','--print','sysroot')
        Require-Step $sysrootStep
        $sysroot = [IO.Path]::GetFullPath((Read-Output $sysrootStep).Trim())
        $expectedSysroot = [IO.Path]::GetFullPath((Join-Path $env:RUSTUP_HOME "toolchains\$Toolchain"))
        if (-not [string]::Equals($sysroot, $expectedSysroot, [StringComparison]::OrdinalIgnoreCase)) { throw 'rust-src sysroot escapes the exact owned toolchain.' }
        $sourceFacts = @('Cargo.toml','Cargo.lock','core/src/lib.rs','alloc/src/lib.rs','std/src/lib.rs','test/src/lib.rs') | ForEach-Object {
            $sourceFile = Join-Path $sysroot "lib\rustlib\src\rust\library\$_"
            $sourceItem = Get-Item -LiteralPath $sourceFile
            if ($sourceItem.Length -gt 16MB -or ($sourceItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { throw 'rust-src source bound/reparse rejected.' }
            File-Fact $sourceFile
        }
        Write-NewJson (Join-Path $Artifact 'rust-src-provenance.json') ([ordered]@{
            toolchain = $Toolchain; rustc_commit = $BaselineConfiguration.expected_rust_commit; sysroot = $sysroot
            source_component = 'rust-src installed by the same fixed rustup toolchain invocation'; files = @($sourceFacts)
            scope = 'Selected actual installed source/lock hashes; not a complete toolchain/SDK provenance or Win7 runtime certification.'
        })
    }
    $cfg = Invoke-Logged 'rust-target-cfg' $rustup @('run',$Toolchain,'rustc','--target',$Target,'-C','target-feature=+crt-static','--print','cfg')
    Require-Step $cfg
    if ((Read-Output $cfg) -notmatch '(?m)^target_feature="crt-static"\r?$') { throw 'Requested target did not enable crt-static.' }
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    $vs = Invoke-Logged 'visual-studio' $vswhere @('-latest','-version','[17.0,18.0)','-products','*','-requires','Microsoft.VisualStudio.Component.VC.Tools.x86.x64','-format','json')
    Require-Step $vs
    $instances = @((Read-Output $vs) | ConvertFrom-Json)
    if ($instances.Count -ne 1) { throw 'No unique installed Visual Studio 2022 x86/x64 toolchain.' }
    $vsPath = [string]$instances[0].installationPath
    Write-CrtFlagState 'before-vs' $BaselineConfiguration.rustflags
    & (Join-Path $vsPath 'Common7\Tools\Launch-VsDevShell.ps1') -Arch x86 -HostArch amd64 -SkipAutomaticLocation | Out-Null
    Write-CrtFlagState 'after-vs' $BaselineConfiguration.rustflags
    $cl = Resolve-Application 'cl.exe'
    $link = Resolve-Application 'link.exe'
    $dumpbin = Resolve-Application 'dumpbin.exe'
    foreach ($tool in @($cl,$link,$dumpbin)) {
        if (-not $tool.StartsWith($vsPath + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Wrong MSVC tool selected.' }
    }
    if ($env:VSCMD_ARG_TGT_ARCH -ne 'x86' -or $env:VSCMD_ARG_HOST_ARCH -notin @('x64','amd64')) { throw 'Wrong compiler target/host architecture.' }
    Set-Item -LiteralPath "Env:\$($BaselineConfiguration.linker_environment)" -Value $link
    Set-Item -LiteralPath "Env:\$($BaselineConfiguration.cc_environment)" -Value $cl
    if ($Baseline -eq 'Win7') {
        Set-Item -LiteralPath "Env:\$($BaselineConfiguration.cflags_environment)" -Value $BaselineConfiguration.cflags
    }
    $sdkVersion = $env:WindowsSDKVersion.TrimEnd('\')
    Write-NewJson (Join-Path $Artifact 'msvc-sdk.json') ([ordered]@{
        visual_studio_version = $instances[0].installationVersion; cl = File-Fact $cl; linker = File-Fact $link
        sdk_version = $sdkVersion; sdk_root = $env:WindowsSdkDir; vc_tools_version = $env:VCToolsVersion
        static_vc_runtime = File-Fact (Join-Path $env:VCToolsInstallDir 'lib\x86\libcmt.lib')
        static_ucrt = File-Fact (Join-Path $env:WindowsSdkDir "Lib\$sdkVersion\ucrt\x86\libucrt.lib")
        sdk_kernel32 = File-Fact (Join-Path $env:WindowsSdkDir "Lib\$sdkVersion\um\x86\kernel32.lib")
    })
    $probe = Join-Path $RunRoot 'scratch\compiler-version.c'
    $probeSource = 'int fc_compiler_probe(void) { return 0; }'
    $probeArgs = @('/nologo','/Bv','/MT','/c',"/Fo$(Join-Path $RunRoot 'scratch\compiler-version.obj')")
    if ($Baseline -eq 'Win7') {
        $probeSource = "#include <windows.h>`n#if _WIN32_WINNT != 0x0601 || WINVER != 0x0601`n#error Expected Win7 target macros`n#endif`n" + $probeSource
        $probeArgs += @('/D_WIN32_WINNT=0x0601','/DWINVER=0x0601')
    }
    [IO.File]::WriteAllText($probe, $probeSource, $Utf8)
    Require-Step (Invoke-Logged 'cl-version-probe' $cl ($probeArgs + @($probe)))
    # Version/path/hash are already recorded in msvc-sdk.json and cl /Bv.
    # link /? may return nonzero after help; actual Cargo links still require exit 0.
    $metadataStep = Invoke-Logged 'cargo-metadata' $cargo (Get-BaselineCargoArguments 'metadata' @('--locked','--format-version','1','--no-deps') $Baseline)
    Require-Step $metadataStep
    $metadata = (Read-Output $metadataStep) | ConvertFrom-Json
    $packages = @{}
    foreach ($package in $metadata.packages) {
        if ($metadata.workspace_members -contains $package.id) { $packages[$package.id] = $package }
    }
    # Independent checks retain separate first/final command evidence. No tests run.
    Invoke-Logged 'fmt' $cargo (Get-BaselineCargoArguments 'fmt' @('--all','--','--check') $Baseline) 300 | Out-Null
    Invoke-Logged 'architecture' $python @('scripts/check-architecture.py') 120 | Out-Null
    Write-CrtFlagState 'before-cargo' $BaselineConfiguration.rustflags
    $tests = Invoke-Logged 'workspace-test-compile' $cargo (Get-BaselineCargoArguments 'test' @('-vv','--workspace','--all-targets','--release','--locked','--target',$Target,'--no-run','--message-format=json') $Baseline) 2400
    $binary = Invoke-Logged 'product-compile' $cargo (Get-BaselineCargoArguments 'rustc' @('-vv','--release','--locked','--target',$Target,'-p','fc-bootstrap','--bin','fc-bootstrap','--message-format=json') $Baseline) 1200
    Invoke-Logged 'clippy' $cargo (Get-BaselineCargoArguments 'clippy' @('-vv','--workspace','--all-targets','--release','--locked','--target',$Target,'--','-D','warnings') $Baseline) 1500 | Out-Null
    $seen = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    foreach ($build in @($tests, $binary)) {
        foreach ($line in [IO.File]::ReadLines((Join-Path $Artifact $build.stdout), $Utf8)) {
            if (-not $line.StartsWith('{')) { continue }
            $message = $line | ConvertFrom-Json
            if ($message.reason -ne 'compiler-artifact' -or -not $message.executable -or -not $packages.ContainsKey($message.package_id)) { continue }
            $role = if ($message.profile.test) { 'tests' } elseif ($build.name -eq 'product-compile' -and $message.target.kind -contains 'bin') { 'product' } else { continue }
            if (-not $seen.Add([string]$message.executable)) { continue }
            if ($executables.Count -ge 128) { throw 'Executable count bound exceeded.' }
            $package = $packages[$message.package_id]
            $packagePath = [IO.Path]::GetRelativePath($SourceRoot, [IO.Path]::GetDirectoryName($package.manifest_path)).Replace('\','/')
            $executables.Add((Copy-Executable $message $role $package.name $packagePath))
        }
    }
    if (@($executables | Where-Object { $_.role -eq 'tests' }).Count -lt 12 -or
        @($executables | Where-Object { $_.role -eq 'product' }).Count -ne 1) { throw 'Required workspace test/product executables missing.' }
    [long]$payloadBytes = 0
    foreach ($executable in $executables) {
        [long]$size = $executable['bytes']
        if ($size -lt 256 -or $size -gt 128MB -or $payloadBytes -gt 512MB - $size) {
            throw 'Executable package byte bound exceeded.'
        }
        $payloadBytes += $size
    }
    if ($payloadBytes -ne $script:PayloadBytes) { throw 'Executable package byte ledger mismatch.' }
    $number = 0
    foreach ($exe in $executables) {
        $imports = Invoke-Logged "pe-imports-$number" $dumpbin @('/DEPENDENTS',(Join-Path $Artifact $exe.relative_path))
        Require-Step $imports
        if ((Read-Output $imports) -match '(?im)^\s*(?:msvcr(?:t|\d+)[a-z0-9_]*|vcruntime[a-z0-9_]*|msvcp[a-z0-9_]*|ucrtbased?|api-ms-win-crt-[^\s]+)\.dll\s*$') {
            throw 'Direct dynamic CRT import found despite explicit crt-static; original PE report retained.'
        }
        $number++
    }
    Write-NewJson (Join-Path $Artifact 'executables.json') $executables.ToArray()
    foreach ($notice in @('LICENSE','packaging/THIRD-PARTY-NOTICES.txt')) {
        Copy-Item -LiteralPath (Join-Path $SourceRoot $notice) -Destination (Join-Path $Artifact ([IO.Path]::GetFileName($notice)))
    }
    $shaAfter = Invoke-Logged 'source-sha-after' $git @('rev-parse','HEAD')
    Require-Step $shaAfter
    if ((Read-Output $shaAfter).Trim() -ine $ExpectedSourceSha -or $env:GITHUB_SHA -ine $ExpectedSourceSha) { throw 'Build changed the exact checked-out/dispatch source SHA.' }
    $trackedAfter = Invoke-Logged 'source-input-paths-after' $git @('ls-files','-z')
    Require-Step $trackedAfter
    $pathsAfter = (Read-Output $trackedAfter).Split([char[]]@([char]0), 20001, [StringSplitOptions]::RemoveEmptyEntries)
    $treeAfterStep = Invoke-Logged 'source-git-tree-after' $git @('ls-tree','-r','-z','-l','--full-tree','HEAD')
    Require-Step $treeAfterStep
    $gitEntriesAfter = ConvertFrom-SourceGitTree (Read-Output $treeAfterStep)
    $treeIdAfterStep = Invoke-Logged 'source-git-tree-id-after' $git @('rev-parse','HEAD^{tree}')
    Require-Step $treeIdAfterStep
    $treeIdAfter = (Read-Output $treeIdAfterStep).Trim()
    if ($treeIdAfter -cnotmatch '\A[0-9a-f]{40}\z') { throw 'Invalid final HEAD root tree object id.' }
    $inventoryAfter = Hash-Inputs $pathsAfter $gitEntriesAfter
    $gitManifestAfter = [ordered]@{ schema = 'future-classroom.ci-git-tree-inputs/v1'
        source_sha = $ExpectedSourceSha; root_tree_oid_sha1 = $treeIdAfter; files = $gitEntriesAfter }
    $domainsAfter = New-SourceDomainSummary $inventoryAfter $ExpectedSourceSha $treeIdAfter
    Write-NewJson (Join-Path $Artifact 'input-sha256-after.json') $inventoryAfter.sha256_files $LogLimit
    Write-NewJson (Join-Path $Artifact 'input-git-tree-after.json') $gitManifestAfter $LogLimit
    Write-NewJson (Join-Path $Artifact 'input-domains-after.json') $domainsAfter $LogLimit
    $cleanAfter = Invoke-Logged 'source-clean-after' $git @('status','--porcelain','--untracked-files=all')
    Require-Step $cleanAfter
    if ((Read-Output $cleanAfter).Length -ne 0) { throw 'Build left changes inside source checkout.' }
    if (($inputs | ConvertTo-Json -Depth 10 -Compress) -cne ($inventoryAfter.sha256_files | ConvertTo-Json -Depth 10 -Compress)) { throw 'Build changed hashed source inputs.' }
    if (($gitManifest | ConvertTo-Json -Depth 10 -Compress) -cne ($gitManifestAfter | ConvertTo-Json -Depth 10 -Compress) -or
        ($inputDomainSummary | ConvertTo-Json -Depth 10 -Compress) -cne ($domainsAfter | ConvertTo-Json -Depth 10 -Compress)) { throw 'Build changed Git metadata or input domain binding.' }
} catch {
    $failure = $_.Exception.Message
    Write-NewJson (Join-Path $Artifact 'failure.json') ([ordered]@{ message = $failure; utc = [DateTime]::UtcNow.ToString('o') })
    Write-Host "Compilation evidence failed: $failure"
} finally {
    Restore-ProcessEnvironment $savedEnvironment
}
$passed = ($null -eq $failure -and $Steps.Count -gt 0 -and @($Steps | Where-Object { -not $_.passed }).Count -eq 0)
Write-NewJson (Join-Path $Artifact 'result.json') ([ordered]@{
    status = $(if ($passed) { 'COMPILE_CHECKS_PASSED_RUNTIME_NOT_RUN' } else { 'FAILED_OR_INCOMPLETE' })
    source_sha = $ExpectedSourceSha; input_manifest_sha256 = $inputDigest; toolchain = $Toolchain; target = $Target; baseline = $Baseline
    dispatch_source_sha = $env:GITHUB_SHA
    input_schema = 'future-classroom.ci-input-domains/v1'
    git_tree_manifest_sha256 = $gitTreeDigest; input_domains_manifest_sha256 = $inputDomainsDigest
    input_domain_summary = $(if ($null -eq $inputDomainSummary) { $null } else { [ordered]@{
        manifest = 'input-domains.json'; tracked_file_count = $inputDomainSummary.tracked_file_count
        sha256_file_count = $inputDomainSummary.sha256_input.file_count
        sha256_checkout_bytes_hashed = $inputDomainSummary.sha256_input.checkout_bytes_hashed
        historical_file_count = $inputDomainSummary.historical_git_archive.file_count
        historical_git_blob_bytes = $inputDomainSummary.historical_git_archive.git_blob_bytes
        historical_checkout_metadata_bytes = $inputDomainSummary.historical_git_archive.checkout_metadata_bytes
        historical_content_sha256 = 'NOT_READ_BY_INPUT_HASHER'
    } })
    commands = $Steps.ToArray(); executable_count = $executables.Count
    windows7 = 'NOT_RUN'; windows10 = 'NOT_RUN'; windows11 = 'NOT_RUN'; seewo_device = 'NOT_RUN'
    purpose = $(if ($Baseline -eq 'Win7') { 'Win7 MSVC candidate COMPILE_ONLY with rebuilt std/test and PE32 subsystem6.1; not Win7 API, SDK/CRT compatibility or runtime certification.' } else { 'Modern formal MSVC build evidence and portable test executables, not a product installer or runtime certification.' })
    later_test_arguments = @('--test-threads=2'); test_working_directories = 'Package paths in executables.json; choose an isolated controlled root.'
    no_toolchain_payloads = $true; no_environment_dump = $true
    limitations = @('Static CRT does not prove OS API compatibility.', 'Already terminated/cancelled hosted jobs may prevent always-upload.',
        'Doctests are not compiled into the collected no-run harness executables.', 'Actual Windows 7/10/11 and classroom-device runs require separate evidence.')
})
if ($passed) { exit 0 } else { exit 1 }
