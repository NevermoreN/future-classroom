param([Parameter(Mandatory=$true)][string]$OwnedScriptRoot)
$ErrorActionPreference='Stop'
$result=@()
foreach($name in @('setup','launch','stop','capture','cleanup')){
 $path=Join-Path $OwnedScriptRoot ('tls-'+$name+'.ps1')
 $tokens=$null;$errors=$null
 [Management.Automation.Language.Parser]::ParseFile($path,[ref]$tokens,[ref]$errors)|Out-Null
 $result+=@{name=$name;errorCount=$errors.Count;sha256=(Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()}
 if($errors.Count-ne 0){throw ('Owned control AST failed: '+$name)}
}
@{status='PASS';scope='AST_ONLY_NO_FIXTURE_OR_EXE_EXECUTION';scripts=$result}|ConvertTo-Json -Depth 5 -Compress
