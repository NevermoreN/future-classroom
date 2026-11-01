param(
    [Parameter(Mandatory=$true)][ValidateSet('Run','Snapshot','Read','Cleanup')][string]$Mode,
    [Parameter(Mandatory=$true)][string]$Root,
    [string]$RelativeFile,
    [string]$ExpectedProofSha256,
    [ValidateRange(1,120000)][int]$AvailableMilliseconds=120000
)
$ErrorActionPreference='Stop'
Set-StrictMode -Version Latest
$clock=[Diagnostics.Stopwatch]::StartNew()
$nonce='ae966fa5484f4235b154b7b874ff728f'
$product='{60E68721-62F0-5656-B5BE-144E6B889AB6}'
$marker="future-classroom-owned-r75-registry-keypath/v1`n"
$expected=@{
 'fixture.msi'='d3c6d5e4937d5dd673e9835a7fbc8d5c7d03af8b0527e1a12d565da1a0e4e248'
 'fixture-spec.json'='a41186d39d1ed187bfdbe99b0d94cb2e4eefc6fc36866257bddaaff34edd643f'
 'query-registry-x86.ps1'='0d52ab605178fb80e52aba9ff6ae8d4329daf6f57049f458dafdd43c29c75af4'
}
function Digest([string]$file) { return (Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash.ToLowerInvariant() }
function Require([bool]$condition,[string]$code) { if(-not $condition){throw $code} }
function Write-New([string]$path,[byte[]]$bytes) {
 $stream=[IO.File]::Open($path,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::Read)
 try{$stream.Write($bytes,0,$bytes.Length);$stream.Flush()}finally{$stream.Dispose()}
}
function Json-New([string]$path,$value) {
 Write-New $path ((New-Object Text.UTF8Encoding $false).GetBytes(($value|ConvertTo-Json -Depth 12)+"`n"))
}
function Remaining { return [Math]::Max(0,$AvailableMilliseconds-$clock.ElapsedMilliseconds) }
function Quote([string]$value) {
 Require (-not $value.Contains('"') -and -not $value.EndsWith('\') -and -not $value.Contains("`n")) 'ARG_ENCODING'
 return '"'+$value+'"'
}
function Validate-Root {
 Require ($Root -ceq [IO.Path]::GetFullPath($Root)) 'ROOT_ALIAS'
 Require ([IO.Path]::GetFileName($Root) -ceq ('FutureClassroom-r75-registry-keypath-'+$nonce)) 'ROOT_NAME'
 $cursor=$Root
 while($null -ne $cursor -and $cursor.Length -gt 0){
  $item=Get-Item -LiteralPath $cursor -Force
  Require ($item.PSIsContainer -and -not ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) 'ROOT_REPARSE'
  $cursor=[IO.Path]::GetDirectoryName($cursor)
 }
 $owner=Join-Path $Root '.owner'
 Require (-not ((Get-Item -LiteralPath $owner -Force).Attributes -band [IO.FileAttributes]::ReparsePoint)) 'OWNER_REPARSE'
 Require ([IO.File]::ReadAllText($owner) -ceq $marker) 'OWNER_BYTES'
 foreach($name in $expected.Keys){
  $file=Join-Path $Root $name
  Require (-not ((Get-Item -LiteralPath $file -Force).Attributes -band [IO.FileAttributes]::ReparsePoint)) 'INPUT_REPARSE'
  Require ((Digest $file) -ceq $expected[$name]) 'INPUT_SHA'
 }
 $spec=([IO.File]::ReadAllText((Join-Path $Root 'fixture-spec.json'))|ConvertFrom-Json)
 Require ($spec.nonce -ceq $nonce -and $spec.productCode -ceq $product -and $spec.scope -ceq 'TEST_ONLY_REGISTRY_KEYPATH_FIXTURE') 'SPEC_IDENTITY'
}
function Rows {
 $files=New-Object Collections.Generic.List[object]
 function Walk([string]$directory){
  foreach($entry in @(Get-ChildItem -LiteralPath $directory -Force|Sort-Object FullName)){
   Require (-not ($entry.Attributes -band [IO.FileAttributes]::ReparsePoint)) 'MEMBER_REPARSE'
   if($entry.PSIsContainer){
    $directoryRelative=$entry.FullName.Substring($Root.Length+1).Replace('\','/')
    Require ($directoryRelative.Split('/')[0] -cin @('query-temp-Before','query-temp-Installed','query-temp-After')) 'UNKNOWN_DIRECTORY'
    Walk $entry.FullName
   }else{
    if($entry.FullName -ceq (Join-Path $Root 'cleanup-proof.json')){Require ($Mode -ceq 'Cleanup') 'UNEXPECTED_PROOF';continue}
    Require ($entry.Length -le 16777216) 'FILE_BOUND'
    $relative=$entry.FullName.Substring($Root.Length+1).Replace('\','/')
    $segments=$relative.Split('/')
    if($segments.Length -gt 1){Require ($segments[0] -cin @('query-temp-Before','query-temp-Installed','query-temp-After')) 'UNKNOWN_MEMBER_DIRECTORY'}
    else{Require ($relative -cin @('.owner','fixture.msi','fixture-spec.json','query-registry-x86.ps1','controller.ps1','controller-report.json','Before.json','Installed.json','After.json','install.log','uninstall.log','query-Before.stdout.log','query-Before.stderr.log','query-Installed.stdout.log','query-Installed.stderr.log','query-After.stdout.log','query-After.stderr.log','install.stdout.log','install.stderr.log','uninstall.stdout.log','uninstall.stderr.log')) 'UNKNOWN_ROOT_MEMBER'}
    $files.Add([ordered]@{path=$relative;bytes=$entry.Length;sha256=(Digest $entry.FullName)})
   }
  }
 }
 Walk $Root
 Require ($files.Count -le 128 -and (($files|Measure-Object bytes -Sum).Sum -le 67108864)) 'TREE_BOUND'
 return @($files.ToArray())
}
function No-ActiveOwnedProcess {
 $active=@(Get-CimInstance Win32_Process|Where-Object{
  $_.ProcessId -ne $PID -and $_.CommandLine -and $_.CommandLine.IndexOf($Root,[StringComparison]::OrdinalIgnoreCase) -ge 0
 })
 Require ($active.Count -eq 0) 'ACTIVE_OWNED_PROCESS_RETAINS_ROOT'
}
function Safe-CleanupReport($report) {
 if(-not ($report.identityVerified -and $report.beforeAbsent -and $report.installAttempted -and $report.afterAbsent)){return $false}
 $uninstall=@($report.commands|Where-Object{$_.name -ceq 'uninstall'})
 if($uninstall.Count -ne 1 -or $uninstall[0].exitCode -notin @(0,1605)){return $false}
 foreach($command in $report.commands){if(-not $command.actualExit -or -not $command.pipesComplete -or $command.timedOut){return $false}}
 return $true
}
Validate-Root
if($Mode -ceq 'Read'){
 Require ($RelativeFile -cmatch '^[A-Za-z0-9_.\-/]+$' -and -not $RelativeFile.StartsWith('/') -and -not ($RelativeFile.Split('/') -contains '..')) 'READ_PATH'
 $current=@(Rows|Where-Object{$_.path -ceq $RelativeFile})
 Require ($current.Count -eq 1) 'READ_NOT_SNAPSHOT_MEMBER'
 $file=Join-Path $Root ($RelativeFile.Replace('/','\'))
 [Convert]::ToBase64String([IO.File]::ReadAllBytes($file));exit 0
}
if($Mode -ceq 'Snapshot'){
 No-ActiveOwnedProcess
 @{status='SNAPSHOT_ONLY';root=$Root;nonce=$nonce;files=@(Rows)}|ConvertTo-Json -Depth 5 -Compress
 exit 0
}
if($Mode -ceq 'Cleanup'){
 Require ($ExpectedProofSha256 -cmatch '^[0-9a-f]{64}$') 'PROOF_SHA_REQUIRED'
 $proofFile=Join-Path $Root 'cleanup-proof.json'
 Require (-not ((Get-Item -LiteralPath $proofFile -Force).Attributes -band [IO.FileAttributes]::ReparsePoint)) 'PROOF_REPARSE'
 Require ((Digest $proofFile) -ceq $ExpectedProofSha256) 'PROOF_IDENTITY'
 $proof=([IO.File]::ReadAllText($proofFile)|ConvertFrom-Json)
 Require ($proof.status -ceq 'PASS_LOCAL_REOPENED_ORIGINAL_BYTES' -and $proof.root -ceq $Root -and $proof.nonce -ceq $nonce) 'RETENTION_PROOF'
 $now=@(Rows);Require ($now.Count -eq $proof.files.Count) 'CLEANUP_UNKNOWN_DELTA'
 foreach($row in $now){
  $match=@($proof.files|Where-Object{$_.path -ceq $row.path})
  Require ($match.Count -eq 1 -and $match[0].bytes -eq $row.bytes -and $match[0].sha256 -ceq $row.sha256) 'CLEANUP_CHANGED_BYTES'
 }
 $report=([IO.File]::ReadAllText((Join-Path $Root 'controller-report.json'))|ConvertFrom-Json)
 Require (Safe-CleanupReport $report) 'EFFECT_OR_OWNER_NOT_RETURNED'
 No-ActiveOwnedProcess
 Remove-Item -LiteralPath $Root -Recurse -Force
 Require (-not (Test-Path -LiteralPath $Root)) 'ROOT_REMAINS'
 @{status='REMOVED_EXACT_OWNED_NONCE_ROOT';root=$Root;nonce=$nonce;retentionProofSha256=$ExpectedProofSha256}|ConvertTo-Json -Compress
 exit 0
}
# Run mode: one origin for 120s, including identity/child/capture work. Keep 55s
# for the formal uninstall and final absence query; never reset this clock.
$initial=@(Get-ChildItem -LiteralPath $Root -Force)
Require ($initial.Count -eq 5 -and (@($initial|Where-Object{$_.PSIsContainer}).Count -eq 0)) 'ROOT_NOT_FRESH'
foreach($item in $initial){Require ($item.Name -in @('.owner','fixture.msi','fixture-spec.json','query-registry-x86.ps1','controller.ps1')) 'UNKNOWN_INITIAL_MEMBER'}
$report=[ordered]@{status='FAIL_RUNTIME';nonce=$nonce;root=$Root;runtimePid=$PID;originalBudgetSeconds=120;availableMilliseconds=$AvailableMilliseconds;
 identityVerified=$true;beforeAbsent=$false;installAttempted=$false;afterAbsent=$false;measurementPassed=$false;commands=@()}
function Invoke-Owned([string]$name,[string]$file,[string]$arguments,[int]$capSeconds,[int]$reserveSeconds){
 $row=[ordered]@{name=$name;file=$file;arguments=$arguments;pid=$null;actualExit=$false;pipesComplete=$false;timedOut=$false;exitCode=$null}
 $report.commands+=,$row
 $budget=[int][Math]::Min($capSeconds*1000,[Math]::Max(0,(Remaining)-$reserveSeconds*1000))
 Require ($budget -gt 0) 'RESERVED_CLEANUP_BUDGET'
 $child=New-Object Diagnostics.Process
 $child.StartInfo.FileName=$file;$child.StartInfo.Arguments=$arguments;$child.StartInfo.WorkingDirectory=$Root
 $child.StartInfo.UseShellExecute=$false;$child.StartInfo.CreateNoWindow=$true
 $child.StartInfo.RedirectStandardInput=$true;$child.StartInfo.RedirectStandardOutput=$true;$child.StartInfo.RedirectStandardError=$true
 $out=[IO.File]::Open((Join-Path $Root ($name+'.stdout.log')),[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::Read)
 $err=[IO.File]::Open((Join-Path $Root ($name+'.stderr.log')),[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::Read)
 try{
  Require ($child.Start()) 'CHILD_START_FALSE';$row.pid=$child.Id;$row['creationFileTimeUtc']=$child.StartTime.ToUniversalTime().ToFileTimeUtc();$child.StandardInput.Close()
  $ot=$child.StandardOutput.BaseStream.CopyToAsync($out);$et=$child.StandardError.BaseStream.CopyToAsync($err)
  if(-not $child.WaitForExit($budget)){
   $row.timedOut=$true;$child.Kill()
   [void]$child.WaitForExit([int][Math]::Min(2000,(Remaining)))
  }
  $row.actualExit=$child.HasExited
  if($row.actualExit){$row.exitCode=$child.ExitCode}
  $row.pipesComplete=[Threading.Tasks.Task]::WaitAll([Threading.Tasks.Task[]]@($ot,$et),[int][Math]::Min(2000,(Remaining)))
  Require ($row.actualExit -and $row.pipesComplete -and -not $row.timedOut) 'OWNED_CHILD_RETURN_NOT_PROVED'
 }finally{$out.Dispose();$err.Dispose();$child.Dispose()}
 return $row
}
$system=[Environment]::GetFolderPath([System.Environment+SpecialFolder]::Windows)
$queryPs=if([Environment]::Is64BitOperatingSystem){Join-Path $system 'SysWOW64\WindowsPowerShell\v1.0\powershell.exe'}else{Join-Path $system 'System32\WindowsPowerShell\v1.0\powershell.exe'}
$msiexec=Join-Path $system 'System32\msiexec.exe'
function Query-Phase([string]$phase,[int]$reserve){
 $queryArguments='-NoProfile -NonInteractive -ExecutionPolicy Bypass -File '+(Quote (Join-Path $Root 'query-registry-x86.ps1'))+
  ' -Phase '+$phase+' -SpecPath '+(Quote (Join-Path $Root 'fixture-spec.json'))+' -ExpectedSpecSha256 '+$expected['fixture-spec.json']+' -OutputRoot '+(Quote $Root)
 $row=Invoke-Owned ('query-'+$phase) $queryPs $queryArguments 10 $reserve
 Require ($row.exitCode -eq 0) 'QUERY_CHILD_FAILED'
 $value=([IO.File]::ReadAllText((Join-Path $Root ($phase+'.json')))|ConvertFrom-Json)
 Require ($value.status -ceq 'PASS_QUERY_PHASE_RAW_KEYPATH_RETAINED' -and $value.phase -ceq $phase -and $value.nonce -ceq $nonce -and $value.processBits -eq 32) 'QUERY_RECEIPT'
 return $value
}
try{
 [void](Query-Phase 'Before' 55);$report.beforeAbsent=$true
 $report.installAttempted=$true
 $install=Invoke-Owned 'install' $msiexec ('/i '+(Quote (Join-Path $Root 'fixture.msi'))+' /qn /norestart /L*v '+(Quote (Join-Path $Root 'install.log'))) 35 55
 Require ($install.exitCode -eq 0) 'INSTALL_NOT_EXIT0'
 [void](Query-Phase 'Installed' 55);$report.measurementPassed=$true
}catch{$report['measurementFailureType']=$_.Exception.GetType().FullName}
finally{
 if($report.beforeAbsent -and $report.installAttempted){
  try{
   # Only the exact preflight-proven fixture ProductCode. No registry/cache
   # deletion, transform, alternate package or production helper fallback.
   $uninstall=Invoke-Owned 'uninstall' $msiexec ('/x '+$product+' /qn /norestart /L*v '+(Quote (Join-Path $Root 'uninstall.log'))) 45 10
   Require ($uninstall.exitCode -in @(0,1605)) 'FORMAL_UNINSTALL_FAILED'
  }catch{$report['uninstallFailureType']=$_.Exception.GetType().FullName}
  try{[void](Query-Phase 'After' 0);$report.afterAbsent=$true}catch{$report['absenceFailureType']=$_.Exception.GetType().FullName}
 }
 $report['elapsedMilliseconds']=$clock.ElapsedMilliseconds
 $report['cleanupSafe']=(Safe-CleanupReport $report)
 if($report.measurementPassed -and $report.cleanupSafe){$report.status='PASS_MEASURED_AND_FORMALLY_REMOVED'}
 Json-New (Join-Path $Root 'controller-report.json') $report
}
$report|ConvertTo-Json -Depth 10 -Compress
if($report.status -cne 'PASS_MEASURED_AND_FORMALLY_REMOVED'){exit 1}
exit 0
