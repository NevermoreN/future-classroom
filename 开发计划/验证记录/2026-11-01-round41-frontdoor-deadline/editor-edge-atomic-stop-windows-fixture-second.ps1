$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false);
$publishSource=[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('CiRFcnJvckFjdGlvblByZWZlcmVuY2U9J1N0b3AnOyRQcm9ncmVzc1ByZWZlcmVuY2U9J1NpbGVudGx5Q29udGludWUnCltDb25zb2xlXTo6T3V0cHV0RW5jb2Rpbmc9W1RleHQuVVRGOEVuY29kaW5nXTo6bmV3KCRmYWxzZSk7JE91dHB1dEVuY29kaW5nPVtDb25zb2xlXTo6T3V0cHV0RW5jb2RpbmcKZnVuY3Rpb24gQXNzZXJ0KFtib29sXSRDb25kaXRpb24sW3N0cmluZ10kTWVzc2FnZSl7aWYoISRDb25kaXRpb24pe3Rocm93ICRNZXNzYWdlfX0KZnVuY3Rpb24gTmV3LVRleHQoW3N0cmluZ10kUGF0aCxbc3RyaW5nXSRUZXh0KXsKICAkZmlsZT1bSU8uRmlsZV06Ok9wZW4oJFBhdGgsW0lPLkZpbGVNb2RlXTo6Q3JlYXRlTmV3LFtJTy5GaWxlQWNjZXNzXTo6V3JpdGUsW0lPLkZpbGVTaGFyZV06OlJlYWQpCiAgdHJ5eyRieXRlcz1bVGV4dC5VVEY4RW5jb2RpbmddOjpuZXcoJGZhbHNlKS5HZXRCeXRlcygkVGV4dCk7JGZpbGUuV3JpdGUoJGJ5dGVzLDAsJGJ5dGVzLkxlbmd0aCl9ZmluYWxseXskZmlsZS5EaXNwb3NlKCl9Cn0KZnVuY3Rpb24gTmV3LUpzb24oW3N0cmluZ10kUGF0aCwkVmFsdWUpe05ldy1UZXh0ICRQYXRoICgkVmFsdWV8Q29udmVydFRvLUpzb24gLURlcHRoIDggLUNvbXByZXNzKX0KZnVuY3Rpb24gQXNzZXJ0LVJvb3QoW3N0cmluZ10kUm9vdCxbc3RyaW5nXSRNYXJrZXIsW3N0cmluZ10kT3JpZ2luYWxUZW1wKXsKICBBc3NlcnQgKCRNYXJrZXItbWF0Y2ggJ15GdXR1cmVDbGFzc3Jvb20tVGVzdC1Ccm93c2VyLVswLTlhLWZdezMyfSQnKSAnYnJvd3NlciBtYXJrZXIgc2hhcGUgbWlzbWF0Y2gnCiAgQXNzZXJ0IChbU3RyaW5nXTo6RXF1YWxzKFtJTy5QYXRoXTo6R2V0RnVsbFBhdGgoJFJvb3QpLFtJTy5QYXRoXTo6R2V0RnVsbFBhdGgoKEpvaW4tUGF0aCAkT3JpZ2luYWxUZW1wICRNYXJrZXIpKSxbU3RyaW5nQ29tcGFyaXNvbl06Ok9yZGluYWxJZ25vcmVDYXNlKSkgJ2Jyb3dzZXIgcm9vdCBvdXRzaWRlIG9yaWdpbmFsIFRFTVAnCiAgQXNzZXJ0ICgoW0lPLkZpbGVdOjpSZWFkQWxsVGV4dCgoSm9pbi1QYXRoICRSb290ICcuYnJvd3Nlci1tYXJrZXInKSkpLWNlcSAkTWFya2VyKSAnYnJvd3NlciBtYXJrZXIgbWlzbWF0Y2gnCiAgQXNzZXJ0ICgoKEdldC1JdGVtIC1MaXRlcmFsUGF0aCAkUm9vdCAtRm9yY2UpLkF0dHJpYnV0ZXMtYmFuZCBbSU8uRmlsZUF0dHJpYnV0ZXNdOjpSZXBhcnNlUG9pbnQpLWVxIDApICdicm93c2VyIHJvb3QgaXMgcmVwYXJzZSBwb2ludCcKfQoKZnVuY3Rpb24gUHVibGlzaC1TdG9wUmVxdWVzdChbc3RyaW5nXSRSb290LFtzdHJpbmddJENhcGFiaWxpdHkpewogICRwYXRoPUpvaW4tUGF0aCAkUm9vdCAnLnJlcXVlc3Qtc3RvcCcKICBpZihbSU8uRmlsZV06OkV4aXN0cygkcGF0aCkpewogICAgQXNzZXJ0ICgoW0lPLkZpbGVdOjpSZWFkQWxsVGV4dCgkcGF0aCkpLWNlcSAkQ2FwYWJpbGl0eSkgJ2RpZmZlcmVudCBzdG9wIGNhcGFiaWxpdHkgYWxyZWFkeSBwcmVzZW50JwogICAgcmV0dXJuCiAgfQogICRwZW5kaW5nPUpvaW4tUGF0aCAkUm9vdCAoJy5yZXF1ZXN0LXN0b3AucGVuZGluZy0nK1tHdWlkXTo6TmV3R3VpZCgpLlRvU3RyaW5nKCdOJykpCiAgJG93bmVkPSRmYWxzZQogIHRyeXsKICAgICRmaWxlPSRudWxsCiAgICB0cnl7CiAgICAgICRmaWxlPVtJTy5GaWxlXTo6T3BlbigkcGVuZGluZyxbSU8uRmlsZU1vZGVdOjpDcmVhdGVOZXcsW0lPLkZpbGVBY2Nlc3NdOjpXcml0ZSxbSU8uRmlsZVNoYXJlXTo6UmVhZCkKICAgICAgJG93bmVkPSR0cnVlCiAgICAgICRieXRlcz1bVGV4dC5VVEY4RW5jb2RpbmddOjpuZXcoJGZhbHNlKS5HZXRCeXRlcygkQ2FwYWJpbGl0eSkKICAgICAgJGZpbGUuV3JpdGUoJGJ5dGVzLDAsJGJ5dGVzLkxlbmd0aCkKICAgIH1maW5hbGx5e2lmKCRmaWxlKXskZmlsZS5EaXNwb3NlKCl9fQogICAgIyBUaGUgZmluYWwgbmFtZSBpcyBub3QgdmlzaWJsZSB1bnRpbCB0aGUgY29tcGxldGUgb3JpZ2luYWwgZmlsZSBpcyBjbG9zZWQuCiAgICAjIEZpbGUuTW92ZSBpcyBub24tb3ZlcndyaXRpbmcgYW5kIGJvdGggbmFtZXMgYXJlIGluIHRoZSBzYW1lIG93bmVkIGRpcmVjdG9yeS4KICAgIHRyeXtbSU8uRmlsZV06Ok1vdmUoJHBlbmRpbmcsJHBhdGgpOyRvd25lZD0kZmFsc2V9CiAgICBjYXRjaHsKICAgICAgJGNhdXNlPSRfLkV4Y2VwdGlvbi5HZXRCYXNlRXhjZXB0aW9uKCk7JGNvZGU9JGNhdXNlLkhSZXN1bHQtYmFuZCA2NTUzNQogICAgICBpZigoJGNhdXNlLWlzIFtJTy5JT0V4Y2VwdGlvbl0pLWFuZCAoJGNvZGUtaW4gQCg4MCwxODMpKS1hbmQgW0lPLkZpbGVdOjpFeGlzdHMoJHBhdGgpKXsKICAgICAgICBBc3NlcnQgKChbSU8uRmlsZV06OlJlYWRBbGxUZXh0KCRwYXRoKSktY2VxICRDYXBhYmlsaXR5KSAnZGlmZmVyZW50IHN0b3AgY2FwYWJpbGl0eSB3b24gcHVibGljYXRpb24nCiAgICAgIH1lbHNle3Rocm93fQogICAgfQogIH1maW5hbGx5ewogICAgIyBBIGZhaWxlZCBDcmVhdGVOZXcgbmV2ZXIgZ3JhbnRzIG93bmVyc2hpcCBvZiBhbiBleGlzdGluZyBjYW5kaWRhdGUuCiAgICBpZigkb3duZWQtYW5kIFtJTy5GaWxlXTo6RXhpc3RzKCRwZW5kaW5nKSl7W0lPLkZpbGVdOjpEZWxldGUoJHBlbmRpbmcpfQogIH0KfQ=='));. ([scriptblock]::Create($publishSource))
$testRoot=Join-Path $env:TEMP ('FutureClassroom-Test-StopPublish-'+[Guid]::NewGuid().ToString('N'))
$checks=[Collections.Generic.List[string]]::new()
$children=[Collections.Generic.List[Diagnostics.Process]]::new()
$childResults=[Collections.Generic.List[object]]::new()
$disposed=0;$actuallyExited=0;$removed=$false;$failure=$null;$created=$false
$capA='f'*64;$capB='b'*64
function Must([bool]$Value,[string]$Message){if(!$Value){throw $Message}}
function CaseDir([string]$Name){$p=Join-Path $testRoot $Name;$null=[IO.Directory]::CreateDirectory($p);return $p}
function Q([string]$Value){return "'"+$Value.Replace("'","''")+"'"}
function AwaitFile([string]$Path){$until=[DateTime]::UtcNow.AddSeconds(12);while(![IO.File]::Exists($Path)){Must ([DateTime]::UtcNow-lt $until) 'fixture fixed file barrier deadline';Start-Sleep -Milliseconds 2}}
function StartOwned([string]$Text){
  $p=[Diagnostics.Process]::new();$si=[Diagnostics.ProcessStartInfo]::new();$si.FileName=Join-Path $PSHOME 'powershell.exe'
  $boot='$ErrorActionPreference=''Stop'';$s=[Console]::In.ReadToEnd();& ([scriptblock]::Create($s))'
  $enc=[Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($boot))
  $si.Arguments='-NoLogo -NoProfile -NonInteractive -EncodedCommand '+$enc;$si.UseShellExecute=$false;$si.CreateNoWindow=$true
  $si.RedirectStandardInput=$true;$si.RedirectStandardOutput=$true;$si.RedirectStandardError=$true;$p.StartInfo=$si
  Must ($p.Start()) 'owned fixture PowerShell start';$children.Add($p)
  $p.StandardInput.Write($Text);$p.StandardInput.Close();return $p
}
function Collect([Diagnostics.Process]$P,[int]$Expected,[string]$Case){
  Must ($P.WaitForExit(15000)) 'owned fixture child exit deadline'
  $out=$P.StandardOutput.ReadToEnd();$err=$P.StandardError.ReadToEnd()
  Must ($out.Length-le 2048-and $err.Length-le 16384) 'fixture output bounds'
  $childResults.Add(@{case=$Case;exitCode=$P.ExitCode;expectedExitCode=$Expected;stdout=$out;stderr=$err;actualExited=$P.HasExited})
  Must ($P.ExitCode-eq $Expected) ('fixture child outcome '+$Case)
}
try{
  Must (![IO.Directory]::Exists($testRoot)) 'fresh fixture root'
  $null=[IO.Directory]::CreateDirectory($testRoot);$created=$true
  [IO.File]::WriteAllText((Join-Path $testRoot '.fixture-owned'),'synthetic-stop-publication-only')
  $one=CaseDir 'normal';$final=Join-Path $one '.request-stop'
  Publish-StopRequest $one $capA
  Must (([IO.File]::ReadAllText($final))-ceq $capA) 'normal final complete'
  $ticks=[IO.File]::GetLastWriteTimeUtc($final).Ticks
  Publish-StopRequest $one $capA
  Must (([IO.File]::GetLastWriteTimeUtc($final).Ticks)-eq $ticks) 'same-cap request must not overwrite final'
  $checks.Add('normal complete publication and same-cap idempotency preserve final timestamp')
  $rejected=$false;try{Publish-StopRequest $one $capB}catch{$rejected=$true}
  Must ($rejected-and ([IO.File]::ReadAllText($final))-ceq $capA) 'different existing capability must fail without overwrite'
  $checks.Add('different existing capability rejected; original final unchanged')
  $foreign=CaseDir 'foreign-pending';$sentinel=Join-Path $foreign '.request-stop.pending-foreign'
  [IO.File]::WriteAllText($sentinel,'foreign-synthetic-sentinel')
  Publish-StopRequest $foreign $capA
  Must (([IO.File]::ReadAllText($sentinel))-ceq 'foreign-synthetic-sentinel') 'publisher touched unowned pending'
  $checks.Add('publisher never removes another pending file')
  $bad=CaseDir 'directory-final';$null=[IO.Directory]::CreateDirectory((Join-Path $bad '.request-stop'))
  $rejected=$false;try{Publish-StopRequest $bad $capA}catch{$rejected=$true}
  Must ($rejected-and [IO.Directory]::Exists((Join-Path $bad '.request-stop'))) 'directory collision is a real IO failure'
  Must (@(Get-ChildItem -LiteralPath $bad -Force -Filter '.request-stop.pending-*').Count-eq 0) 'failed move did not remove own pending'
  $missing=Join-Path $testRoot 'missing-parent';$rejected=$false;try{Publish-StopRequest $missing $capA}catch{$rejected=$true}
  Must ($rejected-and ![IO.Directory]::Exists($missing)) 'missing-parent real IO failure hidden'
  $checks.Add('directory destination and missing parent fail; exact owned pending removed')
  $concurrent=CaseDir 'concurrent'
  $procs=@()
  foreach($index in @(1,2)){
    $script=$publishSource+"`n"+'$caseRoot='+(Q $concurrent)+";`n"+'$cap='+(Q $capA)+";`n"+
      '[IO.File]::WriteAllText((Join-Path $caseRoot ''ready-'+$index+'''),''ready'');'+
      '$until=[DateTime]::UtcNow.AddSeconds(12);while(![IO.File]::Exists((Join-Path $caseRoot ''go''))){if([DateTime]::UtcNow-ge $until){throw ''fixed child barrier''};Start-Sleep -Milliseconds 2};'+
      'Publish-StopRequest $caseRoot $cap;''SYNTHETIC_PUBLISH_PASS'''
    $procs+=StartOwned $script
  }
  AwaitFile (Join-Path $concurrent 'ready-1');AwaitFile (Join-Path $concurrent 'ready-2')
  [IO.File]::WriteAllText((Join-Path $concurrent 'go'),'go')
  foreach($p in $procs){Collect $p 0 'concurrent same-cap unmodified function'}
  Must (([IO.File]::ReadAllText((Join-Path $concurrent '.request-stop')))-ceq $capA) 'concurrent final mismatch'
  Must (@(Get-ChildItem -LiteralPath $concurrent -Force -Filter '.request-stop.pending-*').Count-eq 0) 'concurrent pending leaked'
  $checks.Add('two real separate PowerShell processes concurrently publish same capability using unchanged function; both exit0')
  # Instrument ONLY the one Write in a separate fixture copy: half-write/flush
  # and wait in the test. Production source has no test hook or extra budget.
  $write='$file.Write($bytes,0,$bytes.Length)'
  $functionStart=$publishSource.IndexOf('function Publish-StopRequest(');Must ($functionStart-ge 0) 'fixture publication function location'
  $functionOnly=$publishSource.Substring($functionStart)
  Must ([regex]::Matches($functionOnly,[regex]::Escape($write)).Count-eq 1) 'exact single-write fixture hook'
  $pause='$file.Write($bytes,0,32);$file.Flush();[IO.File]::WriteAllText((Join-Path $Root ''half-ready''),''ready'');'+
    '$until=[DateTime]::UtcNow.AddSeconds(12);while(![IO.File]::Exists((Join-Path $Root ''release''))){if([DateTime]::UtcNow-ge $until){throw ''fixed half-write fixture barrier''};Start-Sleep -Milliseconds 2};$file.Write($bytes,32,$bytes.Length-32)'
  $instrumented=$publishSource.Substring(0,$functionStart)+$functionOnly.Replace($write,$pause)
  foreach($different in @($false,$true)){
    $half=CaseDir ('half-'+$different);$final=Join-Path $half '.request-stop'
    $p=StartOwned ($instrumented+"`n"+'Publish-StopRequest '+(Q $half)+' '+(Q $capA)+"`n"+'''SYNTHETIC_HALF_PUBLISH_PASS''')
    AwaitFile (Join-Path $half 'half-ready')
    Must (![IO.File]::Exists($final)) 'half-written candidate was visible as final stop'
    $pending=@(Get-ChildItem -LiteralPath $half -Force -Filter '.request-stop.pending-*');Must ($pending.Count-eq 1) 'one exact half-written candidate'
    $file=[IO.File]::Open($pending[0].FullName,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::ReadWrite)
    try{Must ($file.Length-eq 32) 'half-write fixture did not really flush32 bytes'}finally{$file.Dispose()}
    $winner=if($different){$capB}else{$capA};[IO.File]::WriteAllText($final,$winner)
    $ticks=[IO.File]::GetLastWriteTimeUtc($final).Ticks
    [IO.File]::WriteAllText((Join-Path $half 'release'),'release')
    $expected=if($different){1}else{0};Collect $p $expected ('controlled half-write raced different='+$different)
    Must (([IO.File]::ReadAllText($final))-ceq $winner) 'raced existing final overwritten'
    Must (([IO.File]::GetLastWriteTimeUtc($final).Ticks)-eq $ticks) 'raced final timestamp changed'
    Must (@(Get-ChildItem -LiteralPath $half -Force -Filter '.request-stop.pending-*').Count-eq 0) 'raced exact own pending leaked'
  }
  $checks.Add('instrumented half-write32 invisible at final; after closed-write Move collision same-cap succeeds and different-cap exits1; final unchanged; own pending cleaned')
}catch{$failure=$_.Exception.GetBaseException().Message}
finally{
  foreach($p in $children){
    try{if(!$p.HasExited){$p.Kill();Must ($p.WaitForExit(5000)) 'owned fixture child final wait'};if($p.HasExited){$actuallyExited++}}
    catch{$failure=([string]$failure)+'; child final cleanup: '+$_.Exception.Message}
    finally{$p.Dispose();$disposed++}
  }
  if($created){
    try{Must (([IO.File]::ReadAllText((Join-Path $testRoot '.fixture-owned')))-ceq 'synthetic-stop-publication-only') 'fixture marker changed';Remove-Item -LiteralPath $testRoot -Recurse -Force;$removed=![IO.Directory]::Exists($testRoot)}
    catch{$failure=([string]$failure)+'; root cleanup: '+$_.Exception.Message}
  }
}
$status=if(!$failure-and $removed-and $children.Count-eq 4-and $actuallyExited-eq 4-and $disposed-eq 4){'PASS'}else{'FAIL'}
@{status=$status;scope='Synthetic files only; no Edge/TLS/control capability read';powerShellVersion=$PSVersionTable.PSVersion.ToString();checks=$checks.ToArray();children=$childResults.ToArray();started=$children.Count;actualExited=$actuallyExited;disposed=$disposed;root=$testRoot;rootRemoved=$removed;failure=$failure;halfWriteSource='Fixture-only instrumentation of one Write; original production function used unchanged for normal/concurrent tests'}|ConvertTo-Json -Depth 7 -Compress
if($status-ne 'PASS'){exit 1}
