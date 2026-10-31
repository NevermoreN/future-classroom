$guardTokens=$null;$guardErrors=$null
$guardAst=[Management.Automation.Language.Parser]::ParseInput($source,[ref]$guardTokens,[ref]$guardErrors)
$definition=@($guardAst.FindAll({param($node) $node-is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name-eq 'Write-CrtFlagState'},$true))
if($definition.Count-ne 1){throw 'Exact flag helper missing.'}
. ([ScriptBlock]::Create($definition[0].Extent.Text))
$script:FlagRecords=[Collections.Generic.List[object]]::new()
function Write-NewJson([string]$Path,$Value){$script:FlagRecords.Add($Value)}
$Logs='.'
$savedFlags=@{}
foreach($flagName in @('RUSTFLAGS','CARGO_ENCODED_RUSTFLAGS')){
 $savedFlags[$flagName]=@{present=(Test-Path -LiteralPath "Env:\$flagName");value=[Environment]::GetEnvironmentVariable($flagName,'Process')}
}
$guardCases=[Collections.Generic.List[object]]::new()
function Expect-FlagRejection([string]$Label,[string]$Phase='before-cargo'){
 $threw=$false;try{Write-CrtFlagState $Phase}catch{$threw=$true}
 if(-not $threw){throw "Expected flag rejection: $Label"}
 $guardCases.Add(@{name=$Label;passed=$true})
}
try{
 $env:CARGO_ENCODED_RUSTFLAGS=$null
 $env:RUSTFLAGS='-C target-feature=+crt-static'
 foreach($phase in @('before-vs','after-vs','before-cargo')){Write-CrtFlagState $phase;$guardCases.Add(@{name="canonical-$phase";passed=$true})}
 $env:CARGO_ENCODED_RUSTFLAGS='--cfg'+[char]31+'synthetic_shadow'
 Expect-FlagRejection 'nonempty-encoded-shadows-rustflags'
 $env:CARGO_ENCODED_RUSTFLAGS=$null
 $env:RUSTFLAGS='-C target-feature=-crt-static'
 Expect-FlagRejection 'explicit-dynamic-crt'
 $env:RUSTFLAGS=$null
 Expect-FlagRejection 'missing-rustflags'
 $env:RUSTFLAGS='x'*4097
 Expect-FlagRejection 'overlong-flag-before-evidence-copy'
 $env:RUSTFLAGS='-C target-feature=+crt-static'
 Expect-FlagRejection 'invalid-evidence-phase' 'wrong-phase'
}finally{Restore-ProcessEnvironment $savedFlags}
foreach($flagName in $savedFlags.Keys){
 if((Test-Path -LiteralPath "Env:\$flagName")-ne $savedFlags[$flagName].present -or
    [Environment]::GetEnvironmentVariable($flagName,'Process')-cne $savedFlags[$flagName].value){throw 'Exact process flag restoration failed.'}
}
@{status='PASS';caseCount=$guardCases.Count;cases=$guardCases.ToArray();records=$script:FlagRecords.ToArray();filesWritten=0;externalExecutables=0;processFlagsRestored=$true;scope='Actual extracted helper; mocked JSON sink in memory only. No PS7 empty-variable reproduction claimed.'}|ConvertTo-Json -Depth 12 -Compress
