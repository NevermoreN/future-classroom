# Pure in-memory checks of the exact injected CI source. Never dot-source the
# entrypoint, #requires, filesystem/hash paths, child processes or hosted setup.
[CmdletBinding()]
param(
    [Parameter(Mandatory)][ValidateNotNullOrEmpty()][string]$TextSource,
    [Parameter(Mandatory)][ValidatePattern('\A[0-9a-f]{64}\z')][string]$ExpectedSourceSha256
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$script:Assertions = 0
$cases = [Collections.Generic.List[object]]::new()
$bindings = [Collections.Generic.List[object]]::new()
$Utf8 = [Text.UTF8Encoding]::new($false)
$LogLimit = 16MB
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
    Require ($caught.Exception.Message.Contains($ExpectedMessage)) 'Unexpected rejection instead of the exact admission guard.'
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
function Make-RawTree([int]$Count, [int]$HistoryFrom = -1) {
    $raw = [Text.StringBuilder]::new()
    for ($index = 0; $index -lt $Count; $index++) {
        $name = 'ordinary/f{0:d5}.txt' -f $index
        if ($HistoryFrom -ge 0 -and $index -ge $HistoryFrom) { $name = '开发计划/验证记录/f{0:d5}.json' -f $index }
        [void]$raw.Append('100644 blob ' + ('a' * 40) + "       1`t" + $name + "`0")
    }
    return $raw.ToString()
}
# Read-Output is a memory-only substitute for evaluating the exact two original
# .Split RHS expressions. No original logged-command or file reader is invoked.
function Read-Output($Step) { return $script:SplitRaw }
$parseErrors = $null
$fatal = $null
$sourceSha = Text-Sha256 $TextSource
try {
    Require ($sourceSha -ceq $ExpectedSourceSha256) 'Injected source SHA differs.'
    $tokens = $null
    $ast = [Management.Automation.Language.Parser]::ParseInput($TextSource, [ref]$tokens, [ref]$parseErrors)
    Require (@($parseErrors).Count -eq 0) 'Whole CI source failed ParseInput.'
    $allowed = @('Get-SourceInputDomain', 'ConvertFrom-SourceGitTree', 'Hash-Inputs', 'New-SourceDomainSummary')
    foreach ($name in $allowed) {
        $definitions = @($ast.FindAll({ param($node)
            $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -ceq $name
        }, $true))
        Require ($definitions.Count -eq 1) 'Missing/duplicate whitelisted function.'
        $definition = [string]$definitions[0].Extent.Text
        $bindings.Add([ordered]@{ name = $name; sha256 = Text-Sha256 $definition;
            utf8Bytes = $Utf8.GetByteCount($definition) })
        $noise = @(Invoke-Expression $definition)
        Require ($noise.Count -eq 0) 'Function definition produced output.'
    }
    $splitExpressions = [Collections.Generic.List[object]]::new()
    foreach ($name in @('paths', 'pathsAfter')) {
        $assignments = @($ast.FindAll({ param($node)
            $node -is [Management.Automation.Language.AssignmentStatementAst] -and
            $node.Left -is [Management.Automation.Language.VariableExpressionAst] -and
            $node.Left.VariablePath.UserPath -ceq $name
        }, $true))
        Require ($assignments.Count -eq 1) 'Missing/duplicate original tracked Split assignment.'
        $expression = [string]$assignments[0].Right.Extent.Text
        Require ($expression -match '\.Split\(' -and $expression.Contains('20001, [StringSplitOptions]::RemoveEmptyEntries')) 'Tracked Split finite bound differs.'
        $splitExpressions.Add([scriptblock]::Create($expression))
        $bindings.Add([ordered]@{ name = $name + '-Split'; sha256 = Text-Sha256 $expression;
            utf8Bytes = $Utf8.GetByteCount($expression) })
    }
    $tracked = 'MEMORY_ONLY'
    $trackedAfter = 'MEMORY_ONLY'
    Check 'unchanged byte budgets and source-path policy only' {
        Require ($TextSource.Contains('$LogLimit = 16MB')) '16MiB stdout/metadata bound changed.'
        Require ($TextSource.Contains('if ($total -gt 256MB)')) '256MiB SHA input sum changed.'
        Require ($TextSource.Contains('$file.Length -gt 64MB')) '64MiB file cap changed.'
        Require ($TextSource.Contains('$size -gt 64MB')) '64MiB Git blob cap changed.'
        Require ($TextSource.Contains('Source parent is a reparse point.')) 'Reparse guard changed.'
        Require ($TextSource.Contains('Source manifest JSON exceeds its byte bound.')) 'Output metadata guard missing.'
        Require ($TextSource.Contains('$MaximumBytes -gt 0 -and $bytes.LongLength -gt $MaximumBytes')) 'Output-byte precheck changed.'
        Require ($TextSource.Contains('$inputDomainsDigest')) 'Original manifest binding missing.'
    }
    Check '20,000 full Git rows accepted with exact last row preserved' {
        $script:BoundaryRaw = Make-RawTree 20000 10000
        $script:BoundaryEntries = ConvertFrom-SourceGitTree $script:BoundaryRaw
        Require ($script:BoundaryEntries -is [Array] -and $script:BoundaryEntries.Count -eq 20000) 'Boundary inventory was flattened/truncated.'
        Require ($script:BoundaryEntries[0].path -ceq 'ordinary/f00000.txt') 'First entry changed.'
        Require ($script:BoundaryEntries[9999].path -ceq 'ordinary/f09999.txt') 'Old boundary changed.'
        Require ($script:BoundaryEntries[10000].path -ceq '开发计划/验证记录/f10000.json') 'First new-capacity entry missing.'
        Require ($script:BoundaryEntries[19999].path -ceq '开发计划/验证记录/f19999.json') 'Last entry missing.'
    }
    Check '20,001 and larger complete Git row counts reject before parsing a tail' {
        Reject { ConvertFrom-SourceGitTree (Make-RawTree 20001) } 'Source file inventory exceeds bound.'
        Reject { ConvertFrom-SourceGitTree (Make-RawTree 20002) } 'Source file inventory exceeds bound.'
    }
    Check 'last-entry duplicate/case collision cannot hide beyond the old cap' {
        $last = '100644 blob ' + ('a' * 40) + "       1`tORDINARY/F00000.TXT`0"
        Reject { ConvertFrom-SourceGitTree ((Make-RawTree 19999) + $last) } 'Duplicate/case-colliding Git input path.'
    }
    Check 'both exact production tracked Split expressions preserve 20,000 and expose 20,001' {
        $names = [string[]]@($script:BoundaryEntries | ForEach-Object { $_.path })
        foreach ($expression in $splitExpressions) {
            $script:SplitRaw = [string]::Join("`0", $names) + "`0"
            $parts = @(& $expression)
            Require ($parts.Count -eq 20000 -and $parts[19999] -ceq $names[19999]) 'Tracked boundary last path lost.'
            $script:SplitRaw += "extra.txt`0"
            $parts = @(& $expression)
            Require ($parts.Count -eq 20001 -and $parts[20000] -ceq 'extra.txt') 'Overflow hidden by Split.'
            Reject { Hash-Inputs $parts $script:BoundaryEntries } 'Source file inventory count/tree mismatch.'
        }
    }
    Check 'Hash-Inputs 20,000 count admission reaches a sentinel before any filesystem call' {
        $originalDomain = ${function:Get-SourceInputDomain}
        try {
            Set-Item -LiteralPath 'Function:Get-SourceInputDomain' -Value { param($Relative) throw 'R57_COUNT_ACCEPTED_BEFORE_FILESYSTEM' }
            $names = [string[]]@($script:BoundaryEntries | ForEach-Object { $_.path })
            Reject { Hash-Inputs $names $script:BoundaryEntries } 'R57_COUNT_ACCEPTED_BEFORE_FILESYSTEM'
        } finally { Set-Item -LiteralPath 'Function:Get-SourceInputDomain' -Value $originalDomain }
    }
    Check 'Hash-Inputs count/tree mismatch and both independent count overflows reject first' {
        $names = [string[]]@($script:BoundaryEntries | ForEach-Object { $_.path })
        Reject { Hash-Inputs $names[0..19998] $script:BoundaryEntries } 'Source file inventory count/tree mismatch.'
        Reject { Hash-Inputs ($names + 'extra.txt') ($script:BoundaryEntries + $script:BoundaryEntries[0]) } 'Source file inventory count/tree mismatch.'
        Reject { Hash-Inputs ($names + 'extra.txt') $script:BoundaryEntries } 'Source file inventory count/tree mismatch.'
        Reject { Hash-Inputs $names ($script:BoundaryEntries + $script:BoundaryEntries[0]) } 'Source file inventory count/tree mismatch.'
    }
    Check '20,000-entry exact complete S/H partition and honest summary' {
        $hashRows = [Collections.Generic.List[object]]::new()
        $historyRows = [Collections.Generic.List[object]]::new()
        $allNames = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
        foreach ($entry in $script:BoundaryEntries) {
            Require ($allNames.Add([string]$entry.path)) 'Partition duplicated a path.'
            if ($entry.domain -ceq 'sha256_input') {
                $hashRows.Add([ordered]@{ path = $entry.path; bytes = 1L; sha256 = ('b' * 64) })
            } elseif ($entry.domain -ceq 'historical_git_archive') {
                $historyRows.Add([ordered]@{ path = $entry.path; checkout_bytes = 2L })
            } else { throw 'Unknown source domain.' }
        }
        Require ($hashRows.Count -eq 10000 -and $historyRows.Count -eq 10000 -and $allNames.Count -eq 20000) 'Full partition missing paths.'
        $inventory = [ordered]@{ sha256_files = $hashRows.ToArray(); sha256_checkout_bytes = 10000L;
            historical_checkout = $historyRows.ToArray(); historical_checkout_bytes = 20000L; historical_git_blob_bytes = 10000L }
        $summary = New-SourceDomainSummary $inventory ('c' * 40) ('d' * 40)
        Require ($summary.tracked_file_count -eq 20000 -and $summary.sha256_input.file_count -eq 10000 -and
            $summary.historical_git_archive.file_count -eq 10000) 'Summary counts are incomplete.'
        Require ($summary.limits.paths -eq 20000 -and $summary.limits.per_file_bytes -eq 64MB -and
            $summary.limits.sha256_input_total_bytes -eq 256MB -and $summary.limits.metadata_output_bytes -eq 16MB) 'Summary byte budgets changed.'
        Require ($summary.historical_git_archive.content_sha256 -ceq 'NOT_READ_BY_INPUT_HASHER' -and
            $summary.historical_git_archive.git_blob_bytes -eq 10000L -and
            $summary.historical_git_archive.checkout_metadata_bytes -eq 20000L) 'Metadata-only/CRLF-size distinction changed.'
        Require ($Utf8.GetByteCount(($summary | ConvertTo-Json -Depth 10 -Compress)) -le 16MB) 'Normal complete summary exceeds metadata cap.'
    }
    Check 'exact historical prefix only, no new skip domain' {
        Require ((Get-SourceInputDomain '开发计划/验证记录/x') -ceq 'historical_git_archive') 'History prefix missing.'
        foreach ($path in @('开发计划/验证记录X/x', '开发计划/验证记录', 'nested/开发计划/验证记录/x', 'src/x.rs')) {
            Require ((Get-SourceInputDomain $path) -ceq 'sha256_input') 'An alias escaped hashing.'
        }
        Reject { Get-SourceInputDomain 'a/../x' } 'Non-canonical Git path segment.'
    }
    Check 'metadata UTF8 16MiB accepted by byte guard, plus one byte rejected' {
        # Exact boundary intentionally fails the NEXT syntax guard; success here
        # proves the byte guard did not move, without a filesystem or huge path.
        Reject { ConvertFrom-SourceGitTree (('x' * (16MB - 1)) + "`0") } 'Malformed Git tree metadata record.'
        Reject { ConvertFrom-SourceGitTree (('x' * 16MB) + "`0") } 'Git tree metadata empty, truncated or over bound.'
        Reject { ConvertFrom-SourceGitTree ('中' * 6000000 + "`0") } 'Git tree metadata empty, truncated or over bound.'
        Reject { ConvertFrom-SourceGitTree $script:BoundaryRaw.TrimEnd([char]0) } 'Git tree metadata empty, truncated or over bound.'
    }
} catch {
    $fatal = [ordered]@{ kind = $_.Exception.GetType().Name; message = $_.Exception.Message;
        position = [string]$_.InvocationInfo.PositionMessage }
}
$failed = @($cases | Where-Object { $_.status -ne 'PASS' })
$status = 'PASS_PURE_POWERSHELL_INVENTORY_ONLY'
if ($null -ne $fatal -or $failed.Count -gt 0) { $status = 'FAIL_PURE_POWERSHELL_INVENTORY_ONLY' }
[ordered]@{ status = $status; powershell = $PSVersionTable.PSVersion.ToString(); sourceSha256 = $sourceSha;
    sourceExpectedSha256 = $ExpectedSourceSha256; fullSourceParseErrors = @($parseErrors).Count;
    functionBindings = $bindings.ToArray(); checks = $cases.ToArray(); assertions = $script:Assertions; fatal = $fatal;
    scope = 'Pure AST-extracted parser/summary and Hash-Inputs count-entry checks with a before-filesystem sentinel; complete fake metadata partition. No real file hash, build, CI or filesystem acceptance claim.';
    sourcePathLimit = 20000; metadataByteLimit = 16MB; contentByteLimit = 256MB; perFileByteLimit = 64MB;
    ciEntrypointExecuted = $false; filesystemOrEnvironmentWritten = $false; processOrThreadStarted = $false;
    windowsRuntime = 'NOT_RUN'; oldSourceValidators = 'Unchanged; old 10000-path validators apply only to their original sources.'
} | ConvertTo-Json -Depth 12 -Compress
if ($status -cne 'PASS_PURE_POWERSHELL_INVENTORY_ONLY') { throw 'Pure inventory checks failed; original JSON above is retained.' }
