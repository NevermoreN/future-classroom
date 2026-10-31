import base64,hashlib,pathlib
root=pathlib.Path('/tmp/fc-round45-ci-input-domains-worktree')
ev=pathlib.Path('/tmp/fc-round45-ci-input-domains-evidence')
data=(root/'scripts/windows-msvc-ci.ps1').read_bytes()
header='''# R45 isolated helper fixture. Does not dot-source or execute the CI entrypoint.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$SourceBytes = [Convert]::FromBase64String('__SOURCE__')
$ExpectedSourceSha256 = '__HASH__'
'''.replace('__SOURCE__',base64.b64encode(data).decode()).replace('__HASH__',hashlib.sha256(data).hexdigest())
tail=r'''
$sha = [Security.Cryptography.SHA256]::Create()
try { $actualSourceSha = [BitConverter]::ToString($sha.ComputeHash($SourceBytes)).Replace('-','').ToLowerInvariant() } finally { $sha.Dispose() }
if ($actualSourceSha -cne $ExpectedSourceSha256) { throw 'Fixture source binding differs.' }
$Utf8 = [Text.UTF8Encoding]::new($false)
$LogLimit = 16MB
$tokens = $null; $parseErrors = $null
$source = $Utf8.GetString($SourceBytes)
$ast = [Management.Automation.Language.Parser]::ParseInput($source, [ref]$tokens, [ref]$parseErrors)
if ($parseErrors.Count -ne 0) { throw 'Full original CI source AST has errors.' }
$names = @('Get-SourceInputDomain','ConvertFrom-SourceGitTree','Assert-NoHistoricalBuildReference','Hash-Inputs','New-SourceDomainSummary','Write-NewJson')
$functionBindings = @()
foreach ($name in $names) {
    $found = @($ast.FindAll({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -ceq $name }.GetNewClosure(), $true))
    if ($found.Count -ne 1) { throw "Expected exact single helper: $name" }
    $definition = $found[0].Extent.Text
    . ([ScriptBlock]::Create($definition))
    $functionBindings += [ordered]@{ name = $name; characters = $definition.Length }
}
$tests = [Collections.Generic.List[object]]::new()
function Require([bool]$Condition, [string]$Message) { if (-not $Condition) { throw $Message } }
function Check([string]$Name, [ScriptBlock]$Body) { & $Body; $tests.Add([ordered]@{ name = $Name; passed = $true }) }
function Reject([string]$Name, [ScriptBlock]$Body) {
    $rejected = $false
    try { & $Body | Out-Null } catch { $rejected = $true }
    Require $rejected "Did not reject: $Name"
    $tests.Add([ordered]@{ name = $Name; passed = $true })
}
function TreeRecord([string]$Relative, [long]$Size, [string]$Mode = '100644', [string]$Type = 'blob', [string]$Oid = ('a' * 40)) {
    return "$Mode $Type $Oid $Size`t$Relative`0"
}
$root = Join-Path ([IO.Path]::GetTempPath()) ('FutureClassroom-Test-CIInputs-' + [Guid]::NewGuid().ToString('N'))
$marker = 'future-classroom-r45-ci-input-fixture/v1:' + [Guid]::NewGuid().ToString('N')
if (Test-Path -LiteralPath $root) { throw 'Fixture root already exists.' }
[IO.Directory]::CreateDirectory($root) | Out-Null
$markerPath = Join-Path $root '.owner'
[IO.File]::WriteAllText($markerPath, $marker, $Utf8)
$SourceRoot = Join-Path $root 'checkout'
[IO.Directory]::CreateDirectory($SourceRoot) | Out-Null
$script:HashReads = [Collections.Generic.List[string]]::new()
# Instrument only this isolated PowerShell invocation. Historical payloads must
# not reach the actual Get-FileHash cmdlet even while FileInfo checks run.
function Get-FileHash {
    param([string]$LiteralPath, [string]$Algorithm)
    $script:HashReads.Add($LiteralPath)
    Microsoft.PowerShell.Utility\Get-FileHash -LiteralPath $LiteralPath -Algorithm $Algorithm
}
$junction = $null
$outcome = $null
try {
    Check 'exact prefix' { Require ((Get-SourceInputDomain '开发计划/验证记录/a.rs') -ceq 'historical_git_archive') 'Exact prefix not isolated.' }
    foreach ($path in @('开发计划/验证记录','开发计划/验证记录-旧/a','开发计划/验证记录x/a','a/开发计划/验证记录/x')) {
        Check "near prefix stays SHA256: $path" { Require ((Get-SourceInputDomain $path) -ceq 'sha256_input') 'Near prefix escaped.' }
    }
    foreach ($path in @('', '../x', 'a/../x', 'a//x', 'a/./x', '/x', 'C:/x', 'a\x', "a`0x", "a`tx")) {
        Reject "noncanonical: $($path.Replace([char]0, [char]63))" { Get-SourceInputDomain $path }
    }
    Check 'metadata mode/type/oid/size preserved' {
        $entry = (ConvertFrom-SourceGitTree (TreeRecord 'src/a.rs' 13))[0]
        Require ($entry.mode -ceq '100644' -and $entry.type -ceq 'blob' -and $entry.git_blob_oid_sha1 -ceq ('a'*40) -and $entry.git_blob_bytes -eq 13) 'Metadata differs.'
    }
    Reject 'missing final NUL' { ConvertFrom-SourceGitTree '100644 blob a 1' }
    Reject 'symlink mode' { ConvertFrom-SourceGitTree (TreeRecord 'x' 1 '120000') }
    Reject 'gitlink' { ConvertFrom-SourceGitTree (TreeRecord 'x' 1 '160000' 'commit') }
    Reject 'bad SHA1' { ConvertFrom-SourceGitTree (TreeRecord 'x' 1 '100644' 'blob' ('z'*40)) }
    Reject 'blob individual limit' { ConvertFrom-SourceGitTree (TreeRecord 'x' (64MB+1)) }
    Reject 'case collision' { ConvertFrom-SourceGitTree ((TreeRecord 'A' 1) + (TreeRecord 'a' 1)) }
    Reject 'duplicate path' { ConvertFrom-SourceGitTree ((TreeRecord 'a' 1) + (TreeRecord 'a' 1)) }
    Reject '10001 metadata paths' {
        $many = [Text.StringBuilder]::new()
        for ($i=0; $i -lt 10001; $i++) { [void]$many.Append((TreeRecord "f$i" 1)) }
        ConvertFrom-SourceGitTree $many.ToString()
    }
    Reject 'metadata stdout bound' { ConvertFrom-SourceGitTree (('x' * (16MB+1)) + "`0") }
    $historyDir = Join-Path $SourceRoot '开发计划/验证记录'
    [IO.Directory]::CreateDirectory($historyDir) | Out-Null
    $archive = Join-Path $historyDir 'a.txt'
    [IO.File]::WriteAllText($archive, "legacy`r`n", $Utf8)
    $normal = Join-Path $SourceRoot 'ordinary.txt'
    [IO.File]::WriteAllText($normal, 'fresh', $Utf8)
    $paths = @('ordinary.txt','开发计划/验证记录/a.txt')
    # Deliberate CRLF checkout vs LF blob byte-count difference is not conflated.
    $tree = ConvertFrom-SourceGitTree ((TreeRecord 'ordinary.txt' 5) + (TreeRecord '开发计划/验证记录/a.txt' 7))
    $locked = [IO.File]::Open($archive, 'Open', 'Read', 'None')
    try { $inventory = Hash-Inputs $paths $tree } finally { $locked.Dispose() }
    Check 'history is not content-read or hashed, normal file hashed' {
        Require ($script:HashReads.Count -eq 1 -and $script:HashReads[0] -ceq $normal) 'Historical payload hashed.'
        Require ($inventory.sha256_files.Count -eq 1 -and $inventory.sha256_checkout_bytes -eq 5) 'Normal input missing.'
        Require ($inventory.historical_git_blob_bytes -eq 7 -and $inventory.historical_checkout_bytes -eq 8) 'Blob size confused with CRLF checkout bytes.'
    }
    Check 'summary honestly separates binding types' {
        $summary = New-SourceDomainSummary $inventory ('b'*40) ('c'*40)
        Require ($summary.tracked_file_count -eq 2 -and $summary.historical_git_archive.content_sha256 -ceq 'NOT_READ_BY_INPUT_HASHER') 'Missing classification.'
        Require ($summary.limits.sha256_input_total_bytes -eq 256MB -and $summary.limits.paths -eq 10000 -and $summary.limits.per_file_bytes -eq 64MB) 'Limits changed.'
    }
    [IO.File]::WriteAllText($normal, 'stale', $Utf8)
    $changed = Hash-Inputs $paths $tree
    Check 'same-length ordinary content changes SHA256' {
        Require ($inventory.sha256_files[0].sha256 -cne $changed.sha256_files[0].sha256) 'Same-length content mutation missed.'
    }
    Reject 'tracked/HEAD count mismatch' { Hash-Inputs @('ordinary.txt') $tree }
    Reject 'unknown tracked path' { Hash-Inputs @('other.txt','开发计划/验证记录/a.txt') $tree }
    Reject 'duplicate tracked path' { Hash-Inputs @('ordinary.txt','ordinary.txt') $tree }
    Reject 'tracked count bound' { Hash-Inputs (@('ordinary.txt') * 10001) @() }
    $rs = Join-Path $SourceRoot 'build.rs'
    foreach ($reference in @('include_bytes!("../开发计划/验证记录/x")', '"开发计划\\验证记录\\x"', '"开发计划\验证记录\x"')) {
        [IO.File]::WriteAllText($rs, $reference, $Utf8)
        Reject 'explicit historical Rust reference rejected' { Assert-NoHistoricalBuildReference 'build.rs' $rs }
    }
    [IO.File]::WriteAllText($rs, 'build = "开发计划/验证记录/x"', $Utf8)
    Reject 'Cargo reference rejected' { Assert-NoHistoricalBuildReference 'Cargo.toml' $rs }
    Reject 'Cargo config reference rejected' { Assert-NoHistoricalBuildReference '.cargo/config.toml' $rs }
    [IO.File]::WriteAllText($rs, 'include_bytes!("../tests/fixtures/minimal.docx")', $Utf8)
    Check 'normal fixture include retained' { Assert-NoHistoricalBuildReference 'build.rs' $rs }
    $large = Join-Path $historyDir 'too-large.bin'
    $stream = [IO.File]::Open($large, 'CreateNew', 'Write', 'None')
    try { $stream.SetLength(64MB+1) } finally { $stream.Dispose() }
    Reject 'historical checkout individual bound' { Hash-Inputs @('开发计划/验证记录/too-large.bin') (ConvertFrom-SourceGitTree (TreeRecord '开发计划/验证记录/too-large.bin' 1)) }
    [IO.File]::Delete($large)
    $target = Join-Path $root 'junction-target'
    [IO.Directory]::CreateDirectory($target) | Out-Null
    [IO.File]::WriteAllText((Join-Path $target 'x'), 'x', $Utf8)
    $junction = Join-Path $SourceRoot 'junction'
    New-Item -ItemType Junction -Path $junction -Target $target | Out-Null
    Reject 'regular leaf under junction rejected' { Hash-Inputs @('junction/x') (ConvertFrom-SourceGitTree (TreeRecord 'junction/x' 1)) }
    $json = Join-Path $root 'over-bound.json'
    Reject 'manifest write bounded before CreateNew' { Write-NewJson $json ('x'*100) 5 }
    Require (-not (Test-Path -LiteralPath $json)) 'Oversize manifest was created.'
    $outcome = [ordered]@{ status='PASS_POWERSHELL_HELPERS_ONLY'; powershell=$PSVersionTable.PSVersion.ToString(); source_sha256=$actualSourceSha; full_source_ast_errors=$parseErrors.Count; extracted_functions=$functionBindings; checks=$tests.ToArray(); ci_entrypoint_executed=$false; compiler_executed=$false; runtime_or_ci_compile='NOT_RUN'; large_aggregate_limit='Python model + original constant/static check; no 256MiB allocation in PS fixture'; root_removed=$false }
} finally {
    if ([IO.File]::ReadAllText($markerPath, $Utf8) -cne $marker -or ((Get-Item -LiteralPath $root).Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { throw 'Fixture owner invalid; refusing cleanup.' }
    if ($null -ne $junction -and (Test-Path -LiteralPath $junction)) { [IO.Directory]::Delete($junction) }
    [IO.Directory]::Delete($root, $true)
    if (Test-Path -LiteralPath $root) { throw 'Fixture root remains.' }
}
$outcome.root_removed = $true
$outcome | ConvertTo-Json -Depth 12 -Compress
'''
with (ev/'input-domain-powershell-fixture-first.ps1').open('x') as f:f.write(header+tail)
print(hashlib.sha256((header+tail).encode()).hexdigest())
