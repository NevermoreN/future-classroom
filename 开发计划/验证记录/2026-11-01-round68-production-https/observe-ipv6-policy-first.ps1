param(
    [ValidateRange(5, 120)][int]$ObservationBudgetSeconds = 45,
    [ValidateRange(1, 512)][int]$MaximumBlockRules = 128
)

# Read-only local observation. The parent controller must impose its own hard
# process timeout: elapsed checks below bound work between local OS queries,
# but cannot interrupt an individual stalled CIM/registry call.
$ErrorActionPreference = 'Stop'
$timer = [System.Diagnostics.Stopwatch]::StartNew()
function Get-ObservationError($record) {
    # Exception messages may contain local paths/identities; retain codes only.
    return [ordered]@{
        type = $record.Exception.GetType().FullName
        hresult = $record.Exception.HResult
    }
}
function Test-ObservationBudget {
    return $timer.Elapsed.TotalSeconds -lt $ObservationBudgetSeconds
}
function Get-BoundedAddressText($value) {
    $text = [string]$value
    if ($text.Length -gt 96) { return $text.Substring(0, 96) + '[truncated]' }
    return $text
}

$result = [ordered]@{
    schema = 1
    scope = 'Read-only bounded local IPv6 policy observations; no cause verdict'
    startedUtc = [DateTime]::UtcNow.ToString('o')
    process = [ordered]@{
        is64BitProcess = [Environment]::Is64BitProcess
        is64BitOperatingSystem = [Environment]::Is64BitOperatingSystem
        osVersion = [Environment]::OSVersion.Version.ToString()
        powerShellVersion = $PSVersionTable.PSVersion.ToString()
        osSupportsIPv6 = [System.Net.Sockets.Socket]::OSSupportsIPv6
    }
    disabledComponents = [ordered]@{ status = 'NOT_QUERIED' }
    loopbackRoute = [ordered]@{ status = 'NOT_QUERIED' }
    firewallProfiles = [ordered]@{ status = 'NOT_QUERIED' }
    outboundBlockRules = [ordered]@{ status = 'NOT_QUERIED' }
    observationBudgetSeconds = $ObservationBudgetSeconds
    budgetScope = 'Elapsed checks between local queries; parent hard process timeout required'
    limitations = @(
        'No mutation or socket connection is performed by this script.',
        'Rule count/address observations are not an effective Windows Filtering Platform decision trace.',
        'No observed Windows Firewall block rule does not exclude third-party WFP providers or callouts.',
        'A successful or empty query does not by itself explain native error10013.',
        'No program paths, rule names, user network inventory or VPN configuration are collected.'
    )
}

try {
    $key = Get-Item -LiteralPath 'HKLM:\SYSTEM\CurrentControlSet\Services\Tcpip6\Parameters'
    try {
        if ($key.GetValueNames() -contains 'DisabledComponents') {
            $kind = $key.GetValueKind('DisabledComponents').ToString()
            if ($kind -eq 'DWord') {
                $value = $key.GetValue('DisabledComponents', $null,
                    [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
                $unsigned = [uint32]([int64]$value -band 4294967295L)
                $result.disabledComponents = [ordered]@{
                    status = 'PRESENT'
                    kind = $kind
                    valueDecimal = [uint64]$unsigned
                    valueHex = ('0x{0:X8}' -f $unsigned)
                }
            } else {
                $result.disabledComponents = [ordered]@{
                    status = 'PRESENT_UNEXPECTED_TYPE'
                    kind = $kind
                }
            }
        } else {
            $result.disabledComponents = [ordered]@{ status = 'VALUE_ABSENT' }
        }
    } finally {
        $key.Close()
    }
} catch {
    $result.disabledComponents = [ordered]@{
        status = 'QUERY_ERROR'
        error = Get-ObservationError $_
    }
}

if (Test-ObservationBudget) {
    try {
        $routes = @(Get-NetRoute -AddressFamily IPv6 -DestinationPrefix '::1/128' -PolicyStore ActiveStore |
            Select-Object -First 9)
        $items = @($routes | Select-Object -First 8 | ForEach-Object {
            [ordered]@{
                destinationPrefix = [string]$_.DestinationPrefix
                nextHop = [string]$_.NextHop
                interfaceIndex = $_.InterfaceIndex
                routeMetric = $_.RouteMetric
                protocol = [string]$_.Protocol
            }
        })
        $result.loopbackRoute = [ordered]@{
            status = 'QUERY_COMPLETED'
            observedCount = $items.Count
            truncated = ($routes.Count -gt 8)
            routes = $items
        }
    } catch {
        $result.loopbackRoute = [ordered]@{
            status = 'QUERY_ERROR'
            error = Get-ObservationError $_
        }
    }
} else { $result.loopbackRoute.status = 'BUDGET_EXHAUSTED' }

if (Test-ObservationBudget) {
    try {
        $profiles = @(Get-NetFirewallProfile -PolicyStore ActiveStore |
            Where-Object { $_.Name -in @('Domain', 'Private', 'Public') } |
            Select-Object -First 3 | ForEach-Object {
                [ordered]@{
                    name = [string]$_.Name
                    enabled = [string]$_.Enabled
                    defaultInboundAction = [string]$_.DefaultInboundAction
                    defaultOutboundAction = [string]$_.DefaultOutboundAction
                }
            })
        $result.firewallProfiles = [ordered]@{
            status = 'QUERY_COMPLETED'
            profiles = $profiles
        }
    } catch {
        $result.firewallProfiles = [ordered]@{
            status = 'QUERY_ERROR'
            error = Get-ObservationError $_
        }
    }
} else { $result.firewallProfiles.status = 'BUDGET_EXHAUSTED' }

if (Test-ObservationBudget) {
    try {
        $rules = @(Get-NetFirewallRule -PolicyStore ActiveStore -Enabled True `
            -Direction Outbound -Action Block |
            Select-Object -First ($MaximumBlockRules + 1))
        $queryTruncated = $rules.Count -gt $MaximumBlockRules
        $rulesToCheck = @($rules | Select-Object -First $MaximumBlockRules)
        $checked = 0
        $addressErrors = 0
        $explicitIPv6Rules = 0
        $ipv6LiteralAddressEntries = 0
        $anyRemoteRules = 0
        $truncatedFilterRules = 0
        $truncatedAddressRules = 0
        $samples = @()
        foreach ($rule in $rulesToCheck) {
            if (-not (Test-ObservationBudget)) { break }
            try {
                $filters = @(Get-NetFirewallAddressFilter -AssociatedNetFirewallRule $rule |
                    Select-Object -First 4)
                if ($filters.Count -gt 3) { $truncatedFilterRules++ }
                $selectedFilters = @($filters | Select-Object -First 3)
                $remoteObserved = @($selectedFilters | ForEach-Object { $_.RemoteAddress } |
                    Select-Object -First 65)
                $localObserved = @($selectedFilters | ForEach-Object { $_.LocalAddress } |
                    Select-Object -First 65)
                if ($remoteObserved.Count -gt 64 -or $localObserved.Count -gt 64) {
                    $truncatedAddressRules++
                }
                $remote = @($remoteObserved | Select-Object -First 64)
                $local = @($localObserved | Select-Object -First 64)
                $ipv6Remote = @($remote | Where-Object { ([string]$_).Contains(':') })
                $ipv6Local = @($local | Where-Object { ([string]$_).Contains(':') })
                if ($remote -contains 'Any') { $anyRemoteRules++ }
                if (($ipv6Remote.Count + $ipv6Local.Count) -gt 0) {
                    $explicitIPv6Rules++
                    $ipv6LiteralAddressEntries += $ipv6Remote.Count + $ipv6Local.Count
                    if ($samples.Count -lt 8) {
                        $samples += [ordered]@{
                            # Only explicitly IPv6 address entries, never rule
                            # identifiers, app paths or unrelated IPv4 inventory.
                            remoteIPv6 = @($ipv6Remote | Select-Object -First 4 |
                                ForEach-Object { Get-BoundedAddressText $_ })
                            localIPv6 = @($ipv6Local | Select-Object -First 4 |
                                ForEach-Object { Get-BoundedAddressText $_ })
                            sampleAddressTruncated = ($ipv6Remote.Count -gt 4 -or $ipv6Local.Count -gt 4)
                        }
                    }
                }
                $checked++
            } catch {
                $addressErrors++
            }
        }
        $allAddressQueriesCompleted = ($checked + $addressErrors) -eq $rulesToCheck.Count
        $exact = -not $queryTruncated -and $allAddressQueriesCompleted -and $addressErrors -eq 0 `
            -and $truncatedFilterRules -eq 0 -and $truncatedAddressRules -eq 0
        $result.outboundBlockRules = [ordered]@{
            status = 'QUERY_COMPLETED'
            enabledOutboundBlockRulesObserved = $rulesToCheck.Count
            ruleCountIsLowerBound = $queryTruncated
            maximumBlockRules = $MaximumBlockRules
            checkedAddressRules = $checked
            addressQueryErrors = $addressErrors
            addressScanBudgetExhausted = (-not $allAddressQueriesCompleted)
            maximumAddressFiltersPerRule = 3
            maximumAddressesPerDirectionPerRule = 64
            truncatedFilterRules = $truncatedFilterRules
            truncatedAddressRules = $truncatedAddressRules
            explicitIPv6RulesObserved = $explicitIPv6Rules
            explicitIPv6CountIsExact = $exact
            ipv6LiteralAddressEntriesObserved = $ipv6LiteralAddressEntries
            anyRemoteRulesObserved = $anyRemoteRules
            sampleRuleLimit = 8
            samples = $samples
            interpretation = 'Explicit means an address filter contains an IPv6 literal or range; Any/keywords may also apply to IPv6. Counts are observations, not proof of a matching block decision.'
        }
    } catch {
        $result.outboundBlockRules = [ordered]@{
            status = 'QUERY_ERROR'
            error = Get-ObservationError $_
        }
    }
} else { $result.outboundBlockRules.status = 'BUDGET_EXHAUSTED' }

$timer.Stop()
$result.endedUtc = [DateTime]::UtcNow.ToString('o')
$result.elapsedSeconds = $timer.Elapsed.TotalSeconds
$result | ConvertTo-Json -Depth 10 -Compress
