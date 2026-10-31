from pathlib import Path
p=Path('/tmp/fc-round33-win7-worktree/scripts/windows-msvc-ci.ps1');s=p.read_text()
def once(old,new):
 global s
 assert s.count(old)==1,(old[:100],s.count(old));s=s.replace(old,new,1)
once('    [string]$ExpectedSourceSha\n)','    [string]$ExpectedSourceSha,\n    [ValidateSet(\'Modern\', \'Win7\')][string]$Baseline = \'Modern\'\n)')
once("$Toolchain = '1.97.1-x86_64-pc-windows-msvc'\n$Target = 'i686-pc-windows-msvc'",'''# Pure configuration/argument/header helpers can be AST-extracted without the
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
    if ($Command -notin @('metadata','fmt','test','build','clippy')) { throw 'Unsupported internal Cargo command.' }
    if ($Name -eq 'Win7' -and $Command -in @('test','build','clippy')) {
        $targetIndex = [Array]::IndexOf($Arguments, '--target')
        if ($targetIndex -lt 0 -or $targetIndex + 1 -ge $Arguments.Length -or
            $Arguments[$targetIndex + 1] -cne 'i686-win7-windows-msvc') { throw 'Win7 compilation requires its explicit target.' }
        # No explicit crate list: cargo test must include the test crate too.
        return ,(@('-Z','build-std',$Command) + $Arguments)
    }
    # Metadata/fmt do not compile; do not forward unstable flags to cargo-fmt.
    return ,(@($Command) + $Arguments)
}
function Test-MsvcExecutableHeader([byte[]]$Bytes,
    [ValidateSet('Modern', 'Win7')][string]$Name = 'Modern') {
    if ($Bytes.Length -lt 256 -or $Bytes[0] -ne 0x4d -or $Bytes[1] -ne 0x5a) { throw 'Not a complete PE executable.' }
    $pe = [BitConverter]::ToInt32($Bytes, 0x3c)
    if ($pe -lt 0 -or $pe -gt $Bytes.Length - 26 -or [BitConverter]::ToUInt32($Bytes, $pe) -ne 0x4550 -or
        [BitConverter]::ToUInt16($Bytes, $pe + 4) -ne 0x14c -or [BitConverter]::ToUInt16($Bytes, $pe + 24) -ne 0x10b) {
        throw 'Executable is not Windows x86 PE32.'
    }
    if ($Name -eq 'Win7') {
        # PE32 OptionalHeader starts at PE+24; subsystem version is +48/+50.
        if ($pe -gt $Bytes.Length - 76 -or
            [BitConverter]::ToUInt16($Bytes, $pe + 72) -ne 6 -or
            [BitConverter]::ToUInt16($Bytes, $pe + 74) -ne 1) { throw 'Win7 candidate must declare PE32 subsystem version 6.1.' }
        return '6.1'
    }
    return $null
}
$BaselineConfiguration = Get-MsvcBaseline $Baseline
$Toolchain = $BaselineConfiguration.toolchain
$Target = $BaselineConfiguration.target''')
once('"future-classroom-msvc-$env:GITHUB_RUN_ID-$env:GITHUB_RUN_ATTEMPT"','"$($BaselineConfiguration.root_prefix)-$env:GITHUB_RUN_ID-$env:GITHUB_RUN_ATTEMPT"')
once("[IO.File]::ReadAllText((Join-Path $RunRoot '.owner')) -cne 'future-classroom-msvc-ci/v1'","[IO.File]::ReadAllText((Join-Path $RunRoot '.owner')) -cne $BaselineConfiguration.marker")
once("function Write-CrtFlagState([string]$Phase) {\n", "function Write-CrtFlagState([string]$Phase, [string]$ExpectedRustFlags = '-C target-feature=+crt-static') {\n    if ($ExpectedRustFlags -cnotin @('-C target-feature=+crt-static', '-C target-feature=+crt-static -C link-arg=/SUBSYSTEM:CONSOLE,6.01')) { throw 'Invalid required CRT flags.' }\n")
once("requirement = 'Encoded variable absent; RUSTFLAGS exactly -C target-feature=+crt-static.'",'requirement = "Encoded variable absent; RUSTFLAGS exactly $ExpectedRustFlags."')
once("$flags['RUSTFLAGS'].value -cne '-C target-feature=+crt-static'","$flags['RUSTFLAGS'].value -cne $ExpectedRustFlags")
old='''    if ($bytes.Length -lt 256 -or $bytes[0] -ne 0x4d -or $bytes[1] -ne 0x5a) { throw 'Not a complete PE executable.' }
    $pe = [BitConverter]::ToInt32($bytes, 0x3c)
    if ($pe -lt 0 -or $pe -gt $bytes.Length - 26 -or [BitConverter]::ToUInt32($bytes, $pe) -ne 0x4550 -or
        [BitConverter]::ToUInt16($bytes, $pe + 4) -ne 0x14c -or [BitConverter]::ToUInt16($bytes, $pe + 24) -ne 0x10b) {
        throw 'Executable is not Windows x86 PE32.'
    }'''
once(old,'    $subsystemVersion = Test-MsvcExecutableHeader $bytes $Baseline')
once("pe_machine = '0x014c'; pe_optional_magic = '0x010b'; executed = $false","pe_machine = '0x014c'; pe_optional_magic = '0x010b'; pe_subsystem_version = $subsystemVersion; executed = $false")
once("'CARGO_ENCODED_RUSTFLAGS','RUSTFLAGS','CARGO_TARGET_I686_PC_WINDOWS_MSVC_LINKER','CC_i686_pc_windows_msvc','TEMP','TMP'","'CARGO_ENCODED_RUSTFLAGS','RUSTFLAGS','CARGO_TARGET_I686_PC_WINDOWS_MSVC_LINKER','CC_i686_pc_windows_msvc',\n    'CARGO_TARGET_I686_WIN7_WINDOWS_MSVC_LINKER','CC_i686_win7_windows_msvc','CFLAGS_i686_win7_windows_msvc','TEMP','TMP'")
once("rustflags = '-C target-feature=+crt-static'; tests = 'COMPILE_ONLY_NO_RUN'","rustflags = $BaselineConfiguration.rustflags; baseline = $Baseline; role = $BaselineConfiguration.role; tests = 'COMPILE_ONLY_NO_RUN'")
once("$env:RUSTFLAGS = '-C target-feature=+crt-static'","$env:RUSTFLAGS = $BaselineConfiguration.rustflags")
once("Require-Step (Invoke-Logged 'install-fixed-rust' $rustup @('toolchain','install',$Toolchain,'--profile','minimal','--component','clippy','--component','rustfmt','--target',$Target) 600)","Require-Step (Invoke-Logged 'install-fixed-rust' $rustup $BaselineConfiguration.install_args 600)")
once("Require-Step (Invoke-Logged 'rust-version' $rustup @('run',$Toolchain,'rustc','-vV'))\n    Require-Step (Invoke-Logged 'cargo-version' $cargo @('--version','--verbose'))",'''$rustVersion = Invoke-Logged 'rust-version' $rustup @('run',$Toolchain,'rustc','-vV')
    Require-Step $rustVersion
    Require-Step (Invoke-Logged 'cargo-version' $cargo @('--version','--verbose'))
    if ($Baseline -eq 'Win7') {
        $rustDetails = Read-Output $rustVersion
        if ($rustDetails -notmatch '(?m)^commit-hash: 92ffebeceaa0a071066f233873c2438e73d70fe7\\r?$' -or
            $rustDetails -notmatch '(?m)^release: 1\\.101\\.0-nightly\\r?$' -or
            $rustDetails -notmatch '(?m)^host: x86_64-pc-windows-msvc\\r?$') { throw 'Win7 compiler differs from fixed nightly commit/host/release.' }
        Require-Step (Invoke-Logged 'rust-src-components' $rustup @('component','list','--installed','--toolchain',$Toolchain))
        $sysrootStep = Invoke-Logged 'rust-sysroot' $rustup @('run',$Toolchain,'rustc','--print','sysroot')
        Require-Step $sysrootStep
        $sysroot = [IO.Path]::GetFullPath((Read-Output $sysrootStep).Trim())
        $expectedSysroot = [IO.Path]::GetFullPath((Join-Path $env:RUSTUP_HOME "toolchains\\$Toolchain"))
        if (-not [string]::Equals($sysroot, $expectedSysroot, [StringComparison]::OrdinalIgnoreCase)) { throw 'rust-src sysroot escapes the exact owned toolchain.' }
        $sourceFacts = @('Cargo.toml','Cargo.lock','core/src/lib.rs','alloc/src/lib.rs','std/src/lib.rs','test/src/lib.rs') | ForEach-Object {
            $sourceFile = Join-Path $sysroot "lib\\rustlib\\src\\rust\\library\\$_"
            $sourceItem = Get-Item -LiteralPath $sourceFile
            if ($sourceItem.Length -gt 16MB -or ($sourceItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { throw 'rust-src source bound/reparse rejected.' }
            File-Fact $sourceFile
        }
        Write-NewJson (Join-Path $Artifact 'rust-src-provenance.json') ([ordered]@{
            toolchain = $Toolchain; rustc_commit = $BaselineConfiguration.expected_rust_commit; sysroot = $sysroot
            source_component = 'rust-src installed by the same fixed rustup toolchain invocation'; files = @($sourceFacts)
            scope = 'Selected actual installed source/lock hashes; not a complete toolchain/SDK provenance or Win7 runtime certification.'
        })
    }''')
for phase in ['before-vs','after-vs','before-cargo']:
 once("Write-CrtFlagState '"+phase+"'","Write-CrtFlagState '"+phase+"' $BaselineConfiguration.rustflags")
once("$env:CARGO_TARGET_I686_PC_WINDOWS_MSVC_LINKER = $link\n    [Environment]::SetEnvironmentVariable('CC_i686_pc_windows_msvc', $cl, 'Process')",'''Set-Item -LiteralPath "Env:\\$($BaselineConfiguration.linker_environment)" -Value $link
    Set-Item -LiteralPath "Env:\\$($BaselineConfiguration.cc_environment)" -Value $cl
    if ($Baseline -eq 'Win7') {
        Set-Item -LiteralPath "Env:\\$($BaselineConfiguration.cflags_environment)" -Value $BaselineConfiguration.cflags
    }''')
once("[IO.File]::WriteAllText($probe, 'int fc_compiler_probe(void) { return 0; }', $Utf8)\n    Require-Step (Invoke-Logged 'cl-version-probe' $cl @('/nologo','/Bv','/MT','/c',\"/Fo$(Join-Path $RunRoot 'scratch\\compiler-version.obj')\",$probe))",'''$probeSource = 'int fc_compiler_probe(void) { return 0; }'
    $probeArgs = @('/nologo','/Bv','/MT','/c',"/Fo$(Join-Path $RunRoot 'scratch\\compiler-version.obj')")
    if ($Baseline -eq 'Win7') {
        $probeSource = "#include <windows.h>`n#if _WIN32_WINNT != 0x0601 || WINVER != 0x0601`n#error Expected Win7 target macros`n#endif`n" + $probeSource
        $probeArgs += @('/D_WIN32_WINNT=0x0601','/DWINVER=0x0601')
    }
    [IO.File]::WriteAllText($probe, $probeSource, $Utf8)
    Require-Step (Invoke-Logged 'cl-version-probe' $cl ($probeArgs + @($probe)))''')
replacements=[("@('metadata','--locked','--format-version','1','--no-deps')","(Get-BaselineCargoArguments 'metadata' @('--locked','--format-version','1','--no-deps') $Baseline)"),("@('fmt','--all','--','--check')","(Get-BaselineCargoArguments 'fmt' @('--all','--','--check') $Baseline)"),("@('test','-vv','--workspace','--all-targets','--release','--locked','--target',$Target,'--no-run','--message-format=json')","(Get-BaselineCargoArguments 'test' @('-vv','--workspace','--all-targets','--release','--locked','--target',$Target,'--no-run','--message-format=json') $Baseline)"),("@('build','-vv','--release','--locked','--target',$Target,'-p','fc-bootstrap','--bin','fc-bootstrap','--message-format=json')","(Get-BaselineCargoArguments 'build' @('-vv','--release','--locked','--target',$Target,'-p','fc-bootstrap','--bin','fc-bootstrap','--message-format=json') $Baseline)"),("@('clippy','-vv','--workspace','--all-targets','--release','--locked','--target',$Target,'--','-D','warnings')","(Get-BaselineCargoArguments 'clippy' @('-vv','--workspace','--all-targets','--release','--locked','--target',$Target,'--','-D','warnings') $Baseline)")]
for old,new in replacements:once(old,new)
once("source_sha = $ExpectedSourceSha; input_manifest_sha256 = $inputDigest; toolchain = $Toolchain; target = $Target","source_sha = $ExpectedSourceSha; input_manifest_sha256 = $inputDigest; toolchain = $Toolchain; target = $Target; baseline = $Baseline")
once("purpose = 'Modern formal MSVC build evidence and portable test executables, not a product installer or runtime certification.'","purpose = $(if ($Baseline -eq 'Win7') { 'Win7 MSVC candidate COMPILE_ONLY with rebuilt std/test and PE32 subsystem6.1; not Win7 API, SDK/CRT compatibility or runtime certification.' } else { 'Modern formal MSVC build evidence and portable test executables, not a product installer or runtime certification.' })")
p.write_text(s)
# New manual workflow reuses fixed original actions and helper; it owns a distinct root and queue.
p=Path('/tmp/fc-round33-win7-worktree/.github/workflows/windows-win7-msvc.yml');assert not p.exists();s=p.with_name('windows-msvc.yml').read_text();s=s.replace('name: Windows MSVC compile evidence','name: Windows 7 MSVC candidate COMPILE_ONLY').replace('group: windows-msvc-${{ github.ref }}','group: windows-win7-msvc-${{ github.ref }}').replace('compile-i686:','compile-win7-i686:').replace('future-classroom-msvc-','future-classroom-win7-msvc-').replace("'future-classroom-msvc-ci/v1'","'future-classroom-win7-msvc-ci/v1'").replace("'modern-msvc-compilation-only'","'win7-msvc-candidate-compilation-only'").replace('Compile MSVC workspace and collect original evidence','Compile Win7 MSVC candidate with rebuilt std/test; never execute').replace('-Mode Build -RunRoot','-Baseline Win7 -Mode Build -RunRoot').replace('-Mode Cleanup -RunRoot','-Baseline Win7 -Mode Cleanup -RunRoot')
s=s.replace('# Deliberately manual:', '# Tier3 target: fixed nightly + same-toolchain rust-src; no prebuilt target install.\n# Original135/105minute job/build budgets retained; no speculative extension.\n# Deliberately manual:')
p.write_text(s)
print('updated helper and new workflow only')
