
$root='C:\Users\Elior\AppData\Local\Temp\FutureClassroom-GuiDiagnostics-3194e0137f7249fd825708f2081f26f8';
$exe=Join-Path $root 'fc-bootstrap.exe';
if((Get-FileHash -Algorithm SHA256 -LiteralPath $exe).Hash.ToLowerInvariant() -cne '9095d86eeb7d7c1e4c82dd4a278a6b33fbe71af1454ba14ca7270a24a97797e4'){throw 'GUI input SHA mismatch'}
$results=@();
foreach($argument in @('--version','--future-classroom-invalid-launch-probe')){
 $info=New-Object System.Diagnostics.ProcessStartInfo;
 $info.FileName=$exe;$info.Arguments=$argument;$info.WorkingDirectory=$root;
 $info.UseShellExecute=$false;$info.CreateNoWindow=$false;
 $info.RedirectStandardOutput=$true;$info.RedirectStandardError=$true;
 $info.StandardOutputEncoding=[Text.UTF8Encoding]::new($false);$info.StandardErrorEncoding=[Text.UTF8Encoding]::new($false);
 $process=New-Object System.Diagnostics.Process;$process.StartInfo=$info;
 $began=$false;$timedOut=$false;
 try{
  $began=$process.Start();if(-not $began){throw 'GUI child did not start'}
  $originalPid=$process.Id;
  if(-not $process.WaitForExit(5000)){$timedOut=$true;$process.Kill();if(-not $process.WaitForExit(5000)){throw 'Original owned diagnostic did not return'}}
  $process.WaitForExit();
  $stdout=$process.StandardOutput.ReadToEnd();$stderr=$process.StandardError.ReadToEnd();
  if($stdout.Length -gt 8192 -or $stderr.Length -gt 8192){throw 'Diagnostic output overflow'}
  $results+=@{argument=$argument;pid=$originalPid;actualExit=$true;exitCode=$process.ExitCode;timedOut=$timedOut;stdout=$stdout;stderr=$stderr;createNoWindow=$false;useShellExecute=$false;redirectedPipes=$true};
 }finally{
  if($began -and -not $process.HasExited){$process.Kill();$null=$process.WaitForExit(5000)}
  if(-not $began -or $process.HasExited){$process.Dispose()}
 }
}
@{operations=$results;exeSha256=(Get-FileHash -Algorithm SHA256 -LiteralPath $exe).Hash.ToLowerInvariant();noConsoleApisCalled=$true}|ConvertTo-Json -Depth 8 -Compress
