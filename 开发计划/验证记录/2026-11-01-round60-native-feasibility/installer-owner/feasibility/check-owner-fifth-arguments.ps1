# Evaluate ONLY the exact injected argument guard and joining RHS in memory.
[CmdletBinding()]
param(
 [Parameter(Mandatory)][string]$TextSource,
 [Parameter(Mandatory)][ValidatePattern('\A[0-9a-f]{64}\z')][string]$ExpectedSourceSha256
)
Set-StrictMode -Version Latest
$ErrorActionPreference='Stop'
$utf8=[Text.UTF8Encoding]::new($false);$hash=[Security.Cryptography.SHA256]::Create()
try{$sourceSha=([BitConverter]::ToString($hash.ComputeHash($utf8.GetBytes($TextSource)))).Replace('-','').ToLowerInvariant()}finally{$hash.Dispose()}
if($sourceSha -cne $ExpectedSourceSha256){throw 'Exact source digest differs.'}
$tokens=$null;$errors=$null;$ast=[Management.Automation.Language.Parser]::ParseInput($TextSource,[ref]$tokens,[ref]$errors)
if(@($errors).Count -ne 0){throw 'Source failed ParseInput.'}
$functions=@($ast.FindAll({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -ceq 'Invoke-OwnedMsi'},$true))
if($functions.Count -ne 1){throw 'Missing exact function.'}
$function=$functions[0]
$guards=@($function.FindAll({param($n)$n -is [Management.Automation.Language.ForEachStatementAst] -and $n.Variable.VariablePath.UserPath -ceq 'arg' -and $n.Condition.Extent.Text -ceq '$all'},$true))
$joins=@($function.FindAll({param($n)$n -is [Management.Automation.Language.AssignmentStatementAst] -and $n.Left.Extent.Text -ceq '$psi.Arguments'},$true))
if($guards.Count -ne 1 -or $joins.Count -ne 1){throw 'Exact guard/join not unique.'}
$guard=[scriptblock]::Create($guards[0].Extent.Text)
$join=[scriptblock]::Create($joins[0].Right.Extent.Text)
$checks=[Collections.Generic.List[object]]::new()
$guid='{51C60258-197D-4EA2-8859-F59ABA07AC38}'
foreach($mode in @('install-nospace','install-space','repair','uninstall')){
 $all=if($mode -eq 'install-nospace'){@('/i','C:\Owned\fixture.msi','/qn','/norestart','REBOOT=ReallySuppress','/l*v','C:\Owned\i.msi.log')}
 elseif($mode -eq 'install-space'){@('/i','C:\Owned Space\fixture.msi','/qn','/norestart','REBOOT=ReallySuppress','/l*v','C:\Owned Space\i.msi.log')}
 elseif($mode -eq 'repair'){@('/famus',$guid,'/qn','/norestart','REBOOT=ReallySuppress','/l*v','C:\Owned\f.msi.log')}
 else{@('/x',$guid,'/qn','/norestart','REBOOT=ReallySuppress','/l*v','C:\Owned\x.msi.log')}
 $noise=@(. $guard);if($noise.Count -ne 0){throw 'Argument guard emitted output.'}
 $actual=. $join
 $expected=if($mode -eq 'install-nospace'){'/i C:\Owned\fixture.msi /qn /norestart REBOOT=ReallySuppress /l*v C:\Owned\i.msi.log'}
 elseif($mode -eq 'install-space'){'/i "C:\Owned Space\fixture.msi" /qn /norestart REBOOT=ReallySuppress /l*v "C:\Owned Space\i.msi.log"'}
 elseif($mode -eq 'repair'){'/famus '+$guid+' /qn /norestart REBOOT=ReallySuppress /l*v C:\Owned\f.msi.log'}
 else{'/x '+$guid+' /qn /norestart REBOOT=ReallySuppress /l*v C:\Owned\x.msi.log'}
 if($actual -cne $expected -or $actual.Contains('"/')){throw ('Incorrect actual argument encoding for '+$mode)}
 $checks.Add(@{name=$mode;status='PASS'})
}
foreach($bad in @('C:\Owned"Bad\x.msi',"C:\Owned`rBad\x.msi","C:\Owned`nBad\x.msi")){
 $all=@('/i',$bad,'/qn');$caught=$false
 try{$null=. $guard}catch{if($_.Exception.Message -cne 'Unsafe argument representation.'){throw};$caught=$true}
 if(-not $caught){throw 'Unsafe argument accepted.'};$checks.Add(@{name='reject-quote-or-newline';status='PASS'})
}
@{status='PASS_PURE_ARGUMENT_GUARD_AND_JOIN_ONLY';sourceSha256=$sourceSha;parseErrors=@($errors).Count;checks=$checks.ToArray();
 scope='Exact source guard/RHS over synthetic string arrays only. No msiexec/parser/native/usage-UI behavior proved.';
 nativeExecuted=$false;processStarted=$false;registryOrFileWritten=$false}|ConvertTo-Json -Depth 6 -Compress
