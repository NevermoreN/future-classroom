# Pure PS7 collection reproduction only: no CI, process, build, PE, or cleanup operations.
# Root may execute on an isolated controlled host and retain the actual report.
# Usage: pwsh -NoProfile -File <this-script> -ReportPath <fresh-private-json>
#requires -Version 7.2
[CmdletBinding()]
param([Parameter(Mandatory)][string]$ReportPath)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
function Observe([string]$Name, [scriptblock]$Action) {
    try {
        $observed = @(& $Action)
        return [ordered]@{ name = $Name; outcome = 'RETURNED'; values = $observed }
    } catch {
        return [ordered]@{
            name = $Name; outcome = 'THREW'
            exception = $_.Exception.ToString()
            error_record = $_.ToString()
            category = [string]$_.CategoryInfo
            fully_qualified_error_id = [string]$_.FullyQualifiedErrorId
            position = [string]$_.InvocationInfo.PositionMessage
            script_stack_trace = [string]$_.ScriptStackTrace
        }
    }
}
$list = [Collections.Generic.List[object]]::new()
$list.Add([ordered]@{ role = 'tests'; bytes = 1L })
$list.Add([ordered]@{ role = 'product'; bytes = 2L })
$array = @([ordered]@{ role = 'tests'; bytes = 1L }, [ordered]@{ role = 'product'; bytes = 2L })
$objects = @([pscustomobject]@{ role = 'tests'; bytes = 1L }, [pscustomobject]@{ role = 'product'; bytes = 2L })
$cases = [Collections.Generic.List[object]]::new()
$cases.Add((Observe 'exact_list_ordereddictionary_measure_property' { ($list | Measure-Object bytes -Sum).Sum }))
$cases.Add((Observe 'array_ordereddictionary_measure_property' { ($array | Measure-Object bytes -Sum).Sum }))
$cases.Add((Observe 'array_pscustomobject_measure_property_control' { ($objects | Measure-Object bytes -Sum).Sum }))
$cases.Add((Observe 'existing_where_dictionary_access_control' {
    [ordered]@{ tests = @($list | Where-Object { $_.role -eq 'tests' }).Count; product = @($list | Where-Object { $_.role -eq 'product' }).Count }
}))
$cases.Add((Observe 'explicit_dictionary_long_sum_control' {
    [long]$total = 0
    foreach ($entry in $list) { $total += [long]$entry['bytes'] }
    $total
}))
$cases.Add((Observe 'project_values_before_measure_control' { ($list | ForEach-Object { [long]$_['bytes'] } | Measure-Object -Sum).Sum }))
$report = [ordered]@{
    schema = 'future-classroom-service-msvc-measure-readonly/v1'
    scope = 'Synthetic collection probe only; no Cargo/Windows native/CI execution or artifact changes.'
    source_sha256 = (Get-FileHash -LiteralPath $PSCommandPath -Algorithm SHA256).Hash.ToLowerInvariant()
    ps_version = $PSVersionTable.PSVersion.ToString()
    fixture_list_type = $list.GetType().FullName
    fixture_entry_type = $list[0].GetType().FullName
    expected_synthetic_sum = 3L
    observations = $cases.ToArray()
}
$json = ($report | ConvertTo-Json -Depth 20) + "`n"
$utf8 = [Text.UTF8Encoding]::new($false)
$bytes = $utf8.GetBytes($json)
$file = [IO.File]::Open($ReportPath, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::Read)
try { $file.Write($bytes, 0, $bytes.Length) } finally { $file.Dispose() }
Write-Output $json
