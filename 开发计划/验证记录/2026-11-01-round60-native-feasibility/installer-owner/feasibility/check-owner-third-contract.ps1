# Pure in-memory evaluation of the exact bounded result block. Never execute the
# installer entrypoint, Add-Type, native calls, processes, registry or files.
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$TextSource,
    [Parameter(Mandatory)][ValidatePattern('\A[0-9a-f]{64}\z')][string]$ExpectedSourceSha256
)
Set-StrictMode -Version Latest
$ErrorActionPreference='Stop'
$utf8=[Text.UTF8Encoding]::new($false)
$sha=[Security.Cryptography.SHA256]::Create()
try{$actual=([BitConverter]::ToString($sha.ComputeHash($utf8.GetBytes($TextSource)))).Replace('-','').ToLowerInvariant()}finally{$sha.Dispose()}
if($actual -cne $ExpectedSourceSha256){throw 'Exact source digest differs.'}
$tokens=$null;$errors=$null
$ast=[Management.Automation.Language.Parser]::ParseInput($TextSource,[ref]$tokens,[ref]$errors)
if(@($errors).Count -ne 0){throw 'Exact source failed ParseInput.'}
$start=$TextSource.IndexOf('    $contract=$true')
$end=$TextSource.IndexOf("`n}catch{",$start)
if($start -lt 0 -or $end -le $start -or $TextSource.IndexOf('    $contract=$true',$start+1) -ge 0){throw 'Missing or repeated exact result block.'}
$block=$TextSource.Substring($start,$end-$start)
$subtokens=$null;$suberrors=$null
$subast=[Management.Automation.Language.Parser]::ParseInput($block,[ref]$subtokens,[ref]$suberrors)
if(@($suberrors).Count -ne 0){throw 'Result block failed ParseInput.'}
# Result evaluation has language expressions/methods over our own dictionaries,
# not any command invocation; reject expansion into calls before evaluation.
$commands=@($subast.FindAll({param($n)$n -is [Management.Automation.Language.CommandAst]},$true))
if($commands.Count -ne 0){throw 'Result block contains a command invocation.'}
$evaluate=[scriptblock]::Create($block)
$checks=[Collections.Generic.List[object]]::new()
function New-Observation([string]$Name,[int]$Code){
 $installed=$Name -eq 'install-commit' -or $Name -eq 'repair-commit'
 return @{name=$Name;decision=$(if($Name -eq 'install-rollback'){0}else{1});msiExitCode=$Code;
  guardianEvidence=@([ordered]@{JoinReturn=0;EndReturn=0;EndReturned=1;AboutToReturnCode=0});
  guardianObserved=@(@{actualExit=$true;exitCode=0});productAfter=@{productState=$(if($installed){5}else{-1})};
  payloadAfter=@{valuePresent=$installed;kind='DWord';value=1}}
}
foreach($variant in @('normal','install-nonzero','repair-nonzero','uninstall-nonzero','guardian-failed','end-failed')){
 $script:records=[Collections.Generic.List[object]]::new()
 foreach($name in @('install-commit','repair-commit','uninstall-commit','install-rollback')){
  $code=if($name -eq 'install-rollback'){1603}else{0}
  $script:records.Add((New-Observation $name $code))
 }
 switch($variant){
  'install-nonzero'{$script:records[0].msiExitCode=1603}
  'repair-nonzero'{$script:records[1].msiExitCode=1603}
  'uninstall-nonzero'{$script:records[2].msiExitCode=1603}
  'guardian-failed'{$script:records[2].guardianObserved[0].exitCode=1}
  'end-failed'{$script:records[2].guardianEvidence[0].EndReturn=1603}
 }
 $report=[ordered]@{}
 $noise=@(. $evaluate)
 $expected=$variant -eq 'normal'
 if($noise.Count -ne 0 -or $contract -ne $expected -or $report.directSinglePackageContractSatisfied -ne $expected){throw ('Incorrect result for '+$variant)}
 if($script:records[3].msiExitCode -ne 1603){throw 'Rollback raw code was rewritten.'}
 $checks.Add(@{name=$variant;status='PASS';expectedAccepted=$expected;rollbackOriginalCode=1603})
}
@{status='PASS_PURE_RESULT_CONTRACT_ONLY';sourceSha256=$actual;parseErrors=@($errors).Count;checks=$checks.ToArray();
 scope='Synthetic result records run through the exact result block only; no native MSI/Join/End/cleanup/runtime evidence.';
 nativeExecuted=$false;processStarted=$false;registryOrFileWritten=$false}|ConvertTo-Json -Depth 6 -Compress
