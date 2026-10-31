#requires -Version 7.2
<#
Manual GitHub-hosted build evidence only. No compiled test EXE is executed here.
Official references checked for the initial workflow:
https://github.com/actions/runner-images/blob/main/images/windows/Windows2022-Readme.md
https://github.com/actions/checkout/releases/tag/v7.0.1
https://github.com/actions/upload-artifact/releases/tag/v7.0.2
https://github.com/rust-lang/rust/releases/tag/1.97.1
https://doc.rust-lang.org/rustc/platform-support/windows-msvc.html
https://doc.rust-lang.org/reference/linkage.html#static-and-dynamic-c-runtimes
https://doc.rust-lang.org/cargo/reference/external-tools.html#json-messages
https://learn.microsoft.com/en-us/dotnet/framework/tools/developer-command-prompt-for-vs
#>
[CmdletBinding()]
param(
    [ValidateSet('Build', 'Cleanup')][string]$Mode = 'Build',
    [Parameter(Mandatory)][string]$RunRoot,
    [string]$ExpectedSourceSha
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false
$Toolchain = '1.97.1-x86_64-pc-windows-msvc'
$Target = 'i686-pc-windows-msvc'
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
    $expected = [IO.Path]::GetFullPath((Join-Path $env:RUNNER_TEMP "future-classroom-msvc-$env:GITHUB_RUN_ID-$env:GITHUB_RUN_ATTEMPT"))
    if (-not [string]::Equals([IO.Path]::GetFullPath($RunRoot), $expected, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'Refusing a directory outside this exact run-id/attempt root.'
    }
    $item = Get-Item -LiteralPath $RunRoot
    if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0 -or
        [IO.File]::ReadAllText((Join-Path $RunRoot '.owner')) -cne 'future-classroom-msvc-ci/v1') {
        throw 'Run directory ownership marker/reparse check failed.'
    }
}
Assert-OwnedRoot
$Artifact = Join-Path $RunRoot 'artifact'
$Logs = Join-Path $Artifact 'logs'
$Steps = [Collections.Generic.List[object]]::new()

function Write-NewJson([string]$Path, $Value) {
    $bytes = $Utf8.GetBytes(($Value | ConvertTo-Json -Depth 30) + "`n")
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
function Hash-Inputs([string[]]$Paths) {
    $entries = [Collections.Generic.List[object]]::new()
    $total = 0L
    if ($Paths.Count -gt 10000) { throw 'Source file inventory exceeds bound.' }
    foreach ($relative in ($Paths | Sort-Object -CaseSensitive)) {
        if ([IO.Path]::IsPathRooted($relative)) { throw 'Non-relative Git input.' }
        $path = [IO.Path]::GetFullPath((Join-Path $SourceRoot $relative))
        if (-not $path.StartsWith($SourceRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw 'Input escapes source root.' }
        $file = Get-Item -LiteralPath $path
        $total += $file.Length
        if ($file.Length -gt 64MB -or $total -gt 256MB -or
            ($file.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { throw 'Source inventory size/reparse bound exceeded.' }
        $entries.Add([ordered]@{ path = $relative.Replace('\', '/'); bytes = $file.Length;
            sha256 = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant() })
    }
    return ,$entries.ToArray()
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
    if ($bytes.Length -lt 256 -or $bytes[0] -ne 0x4d -or $bytes[1] -ne 0x5a) { throw 'Not a complete PE executable.' }
    $pe = [BitConverter]::ToInt32($bytes, 0x3c)
    if ($pe -lt 0 -or $pe -gt $bytes.Length - 26 -or [BitConverter]::ToUInt32($bytes, $pe) -ne 0x4550 -or
        [BitConverter]::ToUInt16($bytes, $pe + 4) -ne 0x14c -or [BitConverter]::ToUInt16($bytes, $pe + 24) -ne 0x10b) {
        throw 'Executable is not Windows x86 PE32.'
    }
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
        pe_machine = '0x014c'; pe_optional_magic = '0x010b'; executed = $false
    }
}

$savedEnvironment = @{}
foreach ($name in @('RUSTUP_HOME','CARGO_HOME','RUSTUP_TOOLCHAIN','CARGO_TARGET_DIR','CARGO_BUILD_JOBS','CARGO_TERM_COLOR',
    'CARGO_ENCODED_RUSTFLAGS','RUSTFLAGS','CARGO_TARGET_I686_PC_WINDOWS_MSVC_LINKER','CC_i686_pc_windows_msvc','TEMP','TMP')) {
    $savedEnvironment[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
}
$failure = $null
$executables = [Collections.Generic.List[object]]::new()
$script:PayloadBytes = 0L
$inputDigest = $null
Write-NewJson (Join-Path $Artifact 'run-start.json') ([ordered]@{
    status = 'STARTED_NOT_VALIDATED'; utc = [DateTime]::UtcNow.ToString('o'); expected_source_sha = $ExpectedSourceSha
    dispatch_source_sha = $env:GITHUB_SHA
    workflow_sha = $env:GITHUB_WORKFLOW_SHA; target = $Target; toolchain = $Toolchain
    rustflags = '-C target-feature=+crt-static'; tests = 'COMPILE_ONLY_NO_RUN'
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
    $paths = (Read-Output $tracked).Split([char]0, [StringSplitOptions]::RemoveEmptyEntries)
    $inputs = Hash-Inputs $paths
    Write-NewJson (Join-Path $Artifact 'input-sha256.json') $inputs
    $inputDigest = (Get-FileHash -LiteralPath (Join-Path $Artifact 'input-sha256.json')).Hash.ToLowerInvariant()
    if ((Get-Content -LiteralPath (Join-Path $SourceRoot 'rust-toolchain.toml') -Raw) -notmatch 'channel\s*=\s*"1\.97\.1"') {
        throw 'Repository toolchain does not match fixed CI Rust 1.97.1.'
    }
    $os = Get-CimInstance Win32_OperatingSystem
    Write-NewJson (Join-Path $Artifact 'host.json') ([ordered]@{
        caption = $os.Caption; version = $os.Version; build = $os.BuildNumber; architecture = $os.OSArchitecture
        powershell = $PSVersionTable.PSVersion.ToString(); image = $env:ImageOS; image_version = $env:ImageVersion
        runner_environment = $env:RUNNER_ENVIRONMENT; compiler_host = 'x86_64-pc-windows-msvc'
        source_sha = $ExpectedSourceSha; input_manifest_sha256 = $inputDigest
    })
    foreach ($name in @('target','scratch','cargo','rustup')) { New-Item -ItemType Directory -Path (Join-Path $RunRoot $name) | Out-Null }
    $env:RUSTUP_HOME = Join-Path $RunRoot 'rustup'; $env:CARGO_HOME = Join-Path $RunRoot 'cargo'
    $env:RUSTUP_TOOLCHAIN = $Toolchain; $env:CARGO_TARGET_DIR = Join-Path $RunRoot 'target'
    $env:CARGO_BUILD_JOBS = '2'; $env:CARGO_TERM_COLOR = 'never'
    [Environment]::SetEnvironmentVariable('CARGO_ENCODED_RUSTFLAGS', $null, 'Process')
    $env:RUSTFLAGS = '-C target-feature=+crt-static'
    $env:TEMP = Join-Path $RunRoot 'scratch'; $env:TMP = $env:TEMP
    Require-Step (Invoke-Logged 'rustup-version' $rustup @('--version'))
    Require-Step (Invoke-Logged 'install-fixed-rust' $rustup @('toolchain','install',$Toolchain,'--profile','minimal','--component','clippy','--component','rustfmt','--target',$Target) 600)
    Require-Step (Invoke-Logged 'rust-version' $rustup @('run',$Toolchain,'rustc','-vV'))
    Require-Step (Invoke-Logged 'cargo-version' $cargo @('--version','--verbose'))
    $cfg = Invoke-Logged 'rust-target-cfg' $rustup @('run',$Toolchain,'rustc','--target',$Target,'-C','target-feature=+crt-static','--print','cfg')
    Require-Step $cfg
    if ((Read-Output $cfg) -notmatch '(?m)^target_feature="crt-static"\r?$') { throw 'Requested target did not enable crt-static.' }
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    $vs = Invoke-Logged 'visual-studio' $vswhere @('-latest','-version','[17.0,18.0)','-products','*','-requires','Microsoft.VisualStudio.Component.VC.Tools.x86.x64','-format','json')
    Require-Step $vs
    $instances = @((Read-Output $vs) | ConvertFrom-Json)
    if ($instances.Count -ne 1) { throw 'No unique installed Visual Studio 2022 x86/x64 toolchain.' }
    $vsPath = [string]$instances[0].installationPath
    & (Join-Path $vsPath 'Common7\Tools\Launch-VsDevShell.ps1') -Arch x86 -HostArch amd64 -SkipAutomaticLocation | Out-Null
    $cl = Resolve-Application 'cl.exe'
    $link = Resolve-Application 'link.exe'
    $dumpbin = Resolve-Application 'dumpbin.exe'
    foreach ($tool in @($cl,$link,$dumpbin)) {
        if (-not $tool.StartsWith($vsPath + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Wrong MSVC tool selected.' }
    }
    if ($env:VSCMD_ARG_TGT_ARCH -ne 'x86' -or $env:VSCMD_ARG_HOST_ARCH -notin @('x64','amd64')) { throw 'Wrong compiler target/host architecture.' }
    $env:CARGO_TARGET_I686_PC_WINDOWS_MSVC_LINKER = $link
    [Environment]::SetEnvironmentVariable('CC_i686_pc_windows_msvc', $cl, 'Process')
    $sdkVersion = $env:WindowsSDKVersion.TrimEnd('\')
    Write-NewJson (Join-Path $Artifact 'msvc-sdk.json') ([ordered]@{
        visual_studio_version = $instances[0].installationVersion; cl = File-Fact $cl; linker = File-Fact $link
        sdk_version = $sdkVersion; sdk_root = $env:WindowsSdkDir; vc_tools_version = $env:VCToolsVersion
        static_vc_runtime = File-Fact (Join-Path $env:VCToolsInstallDir 'lib\x86\libcmt.lib')
        static_ucrt = File-Fact (Join-Path $env:WindowsSdkDir "Lib\$sdkVersion\ucrt\x86\libucrt.lib")
        sdk_kernel32 = File-Fact (Join-Path $env:WindowsSdkDir "Lib\$sdkVersion\um\x86\kernel32.lib")
    })
    $probe = Join-Path $RunRoot 'scratch\compiler-version.c'
    [IO.File]::WriteAllText($probe, 'int fc_compiler_probe(void) { return 0; }', $Utf8)
    Require-Step (Invoke-Logged 'cl-version-probe' $cl @('/nologo','/Bv','/MT','/c',"/Fo$(Join-Path $RunRoot 'scratch\compiler-version.obj')",$probe))
    # Version/path/hash are already recorded in msvc-sdk.json and cl /Bv.
    # link /? may return nonzero after help; actual Cargo links still require exit 0.
    $metadataStep = Invoke-Logged 'cargo-metadata' $cargo @('metadata','--locked','--format-version','1','--no-deps')
    Require-Step $metadataStep
    $metadata = (Read-Output $metadataStep) | ConvertFrom-Json
    $packages = @{}
    foreach ($package in $metadata.packages) {
        if ($metadata.workspace_members -contains $package.id) { $packages[$package.id] = $package }
    }
    # Independent checks retain separate first/final command evidence. No tests run.
    Invoke-Logged 'fmt' $cargo @('fmt','--all','--','--check') 300 | Out-Null
    Invoke-Logged 'architecture' $python @('scripts/check-architecture.py') 120 | Out-Null
    $tests = Invoke-Logged 'workspace-test-compile' $cargo @('test','--workspace','--all-targets','--release','--locked','--target',$Target,'--no-run','--message-format=json') 2400
    $binary = Invoke-Logged 'product-compile' $cargo @('build','--release','--locked','--target',$Target,'-p','fc-bootstrap','--bin','fc-bootstrap','--message-format=json') 1200
    Invoke-Logged 'clippy' $cargo @('clippy','--workspace','--all-targets','--release','--locked','--target',$Target,'--','-D','warnings') 1500 | Out-Null
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
    $after = Hash-Inputs $paths
    if (($inputs | ConvertTo-Json -Depth 10 -Compress) -cne ($after | ConvertTo-Json -Depth 10 -Compress)) { throw 'Build changed hashed source inputs.' }
    $cleanAfter = Invoke-Logged 'source-clean-after' $git @('status','--porcelain','--untracked-files=all')
    Require-Step $cleanAfter
    if ((Read-Output $cleanAfter).Length -ne 0) { throw 'Build left changes inside source checkout.' }
} catch {
    $failure = $_.Exception.Message
    Write-NewJson (Join-Path $Artifact 'failure.json') ([ordered]@{ message = $failure; utc = [DateTime]::UtcNow.ToString('o') })
    Write-Host "Compilation evidence failed: $failure"
} finally {
    foreach ($name in $savedEnvironment.Keys) { [Environment]::SetEnvironmentVariable($name, $savedEnvironment[$name], 'Process') }
}
$passed = ($null -eq $failure -and $Steps.Count -gt 0 -and @($Steps | Where-Object { -not $_.passed }).Count -eq 0)
Write-NewJson (Join-Path $Artifact 'result.json') ([ordered]@{
    status = $(if ($passed) { 'COMPILE_CHECKS_PASSED_RUNTIME_NOT_RUN' } else { 'FAILED_OR_INCOMPLETE' })
    source_sha = $ExpectedSourceSha; input_manifest_sha256 = $inputDigest; toolchain = $Toolchain; target = $Target
    dispatch_source_sha = $env:GITHUB_SHA
    commands = $Steps.ToArray(); executable_count = $executables.Count
    windows7 = 'NOT_RUN'; windows10 = 'NOT_RUN'; windows11 = 'NOT_RUN'; seewo_device = 'NOT_RUN'
    purpose = 'Modern formal MSVC build evidence and portable test executables, not a product installer or runtime certification.'
    later_test_arguments = @('--test-threads=2'); test_working_directories = 'Package paths in executables.json; choose an isolated controlled root.'
    no_toolchain_payloads = $true; no_environment_dump = $true
    limitations = @('Static CRT does not prove OS API compatibility.', 'Already terminated/cancelled hosted jobs may prevent always-upload.',
        'Doctests are not compiled into the collected no-run harness executables.', 'Actual Windows 7/10/11 and classroom-device runs require separate evidence.')
})
if ($passed) { exit 0 } else { exit 1 }
