#!/usr/bin/env python3
"""Run the explicit synthetic HTTP.sys loopback probe; never configure TLS/LAN.

Uses only an owned remote root/prefix, preserves SSH authorization, and reuses the
SSH/SCP implementation from windows-probe.py. This script is a runner, not proof
until its remote result and native completion counts have been inspected.
"""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import subprocess
import sys

sys.dont_write_bytecode = True
import uuid

_spec = importlib.util.spec_from_file_location("fc_windows_probe", Path(__file__).with_name("windows-probe.py"))
_helpers = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_helpers)
Remote, ps_literal = _helpers.Remote, _helpers.ps_literal

POWERSHELL = r'''
param([Parameter(Mandatory=$true)][string]$Root,[Parameter(Mandatory=$true)][string]$ExeName,[Parameter(Mandatory=$true)][string]$Marker)
$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
[Console]::OutputEncoding=[System.Text.UTF8Encoding]::new($false)
$OutputEncoding=[Console]::OutputEncoding
$env:TEMP=Join-Path $Root 'temp'
$env:TMP=$env:TEMP
New-Item -ItemType Directory -Force -Path $env:TEMP|Out-Null
$steps=[Collections.Generic.List[object]]::new()
$server=$null
$serverHandle=[IntPtr]::Zero
$hold=$null
$prefix=$null
$aclCreated=$false
$result=@{scope='HTTP-only synthetic loopback; no TLS/campus/browser/authentication evidence';tls='NOT_RUN';win7='NOT_RUN';win10='NOT_RUN';status='RUNNING';p6_11='NOT_RUN';firstConnectionHeaderDeadline='NOT_VALIDATED'}
$script:overall=[Diagnostics.Stopwatch]::StartNew()
$script:phase='initializing'
function Assert([bool]$Condition,[string]$Message){if(!$Condition){throw $Message}}
function Phase([string]$Name,$Details=$null){
  $script:phase=$Name
  $entry=@{phase=$Name;elapsedMilliseconds=$script:overall.ElapsedMilliseconds;utc=[DateTime]::UtcNow.ToString('o');details=$Details}
  $line=$entry|ConvertTo-Json -Depth 6 -Compress
  [IO.File]::WriteAllText((Join-Path $Root '.http-progress.json'),$line,[Text.UTF8Encoding]::new($false))
  [IO.File]::AppendAllText((Join-Path $Root '.http-progress.jsonl'),$line+"`n",[Text.UTF8Encoding]::new($false))
}
function Budget([int]$Maximum){
  $remaining=120000-$script:overall.ElapsedMilliseconds
  Assert ($remaining-gt 0) ("internal 120-second test deadline exceeded phase="+$script:phase)
  return [int][Math]::Min($Maximum,$remaining)
}
# File-provider Get-Content strings carry PSPath/PSProvider graphs. Only plain
# System.IO.File strings may enter result/checkpoint JSON (including failures).
function Checkpoint {
  $result.phase=$script:phase;$result.steps=$steps.ToArray();$result.checkpoint=$true
  [IO.File]::WriteAllText((Join-Path $Root '.http-result.json'),($result|ConvertTo-Json -Depth 8 -Compress),[Text.UTF8Encoding]::new($false))
}
function Step($Value){$steps.Add($Value);Phase ("step-complete/"+$Value.case) $Value;Checkpoint}
Phase 'script-start'
Checkpoint
function Tcp {
  $c=[Net.Sockets.TcpClient]::new()
  $c.ReceiveTimeout=7000;$c.SendTimeout=5000
  $ar=$null;$wait=$null
  try{
    $ar=$c.BeginConnect([Net.IPAddress]::Loopback,$script:port,$null,$null);$wait=$ar.AsyncWaitHandle
    if(!$wait.WaitOne((Budget 1000))){$c.Close();throw ('bounded loopback connect timed out phase='+$script:phase)}
    $c.EndConnect($ar)
    $stream=$c.GetStream();$stream.ReadTimeout=7000;$stream.WriteTimeout=5000
    return $c
  }catch{$c.Dispose();throw}
  finally{if($ar-and $ar.IsCompleted-and $wait){$wait.Dispose()}}
}
function Write-Ascii($Stream,[string]$Text){$Stream.WriteTimeout=(Budget 5000);$b=[Text.Encoding]::ASCII.GetBytes($Text);$Stream.Write($b,0,$b.Length)}
function Read-Reply($Client,[bool]$AllowReset=$false){
  $stream=$Client.GetStream();$stream.ReadTimeout=7000;$stream.WriteTimeout=5000
  $m=[IO.MemoryStream]::new();$buffer=[byte[]]::new(8192);$timedOut=$false;$reset=$false;$eof=$false
  $watch=[Diagnostics.Stopwatch]::StartNew();$readPhase=$script:phase
  try{
    while($true){
      $remaining=[int][Math]::Min((7000-$watch.ElapsedMilliseconds),(Budget 7000))
      if($remaining-le 0){$timedOut=$true;$Client.Close();if(!$AllowReset){throw ('absolute 7000ms response deadline exceeded phase='+$readPhase)};break}
      Phase ($readPhase+'/read-await') @{receivedBytes=$m.Length;remainingMilliseconds=$remaining}
      $ar=$null;$wait=$null
      try{
        $ar=$stream.BeginRead($buffer,0,$buffer.Length,$null,$null);$wait=$ar.AsyncWaitHandle
        if(!$wait.WaitOne($remaining)){
          # Close cancels this managed read. Keep its IAsyncResult/event owned by
          # the runtime until completion; do not block EndRead or dispose its
          # event before completion just to finish the observation window.
          $timedOut=$true;$Client.Close()
          if(!$AllowReset){throw ('absolute 7000ms response deadline exceeded phase='+$readPhase)}
          break
        }
        $n=$stream.EndRead($ar)
      }catch{
        if($timedOut){if(!$AllowReset){throw};break}
        if($AllowReset){$reason=$_.Exception.GetBaseException();if($reason-is [Net.Sockets.SocketException]-and $reason.SocketErrorCode-eq [Net.Sockets.SocketError]::TimedOut){$timedOut=$true}else{$reset=$true};break}
        throw
      }finally{if($ar-and $ar.IsCompleted-and $wait){$wait.Dispose()}}
      if($n-eq 0){$eof=$true;break}
      Assert (($m.Length+$n)-le 524288) 'response allocation bound exceeded'
      $m.Write($buffer,0,$n)
    }
    $all=$m.ToArray();$text=[Text.Encoding]::ASCII.GetString($all);$split=$text.IndexOf("`r`n`r`n");$code=0;$body=[byte[]]::new(0)
    if($split-ge 0){$first=$text.Substring(0,$text.IndexOf("`r`n"));$code=[int]($first.Split(' ')[1]);$length=$all.Length-($split+4);$body=[byte[]]::new($length);[Array]::Copy($all,$split+4,$body,0,$length)}
    Phase ($readPhase+'/read-complete') @{receivedBytes=$all.Length;elapsedMilliseconds=$watch.ElapsedMilliseconds;timedOut=$timedOut;reset=$reset;eof=$eof;statusCode=$code}
    $headers=if($split-ge 0){$text.Substring(0,$split)}else{''}
    return [pscustomobject]@{code=$code;body=$body;headers=$headers;bytes=$all.Length;timedOut=$timedOut;reset=$reset;eof=$eof;observedMilliseconds=$watch.ElapsedMilliseconds}
  }finally{$watch.Stop();$m.Dispose();$Client.Dispose()}
}
function Get-Route([string]$Route){Phase ("cycle-$script:cycle/get-$Route");$c=Tcp;Write-Ascii ($c.GetStream()) ("GET /$Marker/$Route HTTP/1.1`r`nHost: 127.0.0.1:$script:port`r`nConnection: close`r`n`r`n");return Read-Reply $c}
function Healthy {
  $until=[DateTime]::UtcNow.AddSeconds(10)
  while([DateTime]::UtcNow-lt $until){$script:server.Refresh();if($script:server.HasExited){throw ('server exited: '+([IO.File]::ReadAllText((Join-Path $Root 'http.stderr'))))};try{$r=Get-Route 'health';if($r.code-eq 200){return}}catch{};Start-Sleep -Milliseconds 50}
  throw 'owned loopback listener did not become ready'
}
function Fixed-Echo([int]$Length){
  Phase ("cycle-$script:cycle/fixed-echo")
  $c=Tcp;$s=$c.GetStream();Write-Ascii $s ("POST /$Marker/echo HTTP/1.1`r`nHost: 127.0.0.1:$script:port`r`nContent-Length: $Length`r`nConnection: close`r`n`r`n")
  $body=[byte[]]::new($Length);for($i=0;$i-lt $Length;$i++){$body[$i]=90};$s.Write($body,0,$body.Length);$r=Read-Reply $c
  Assert ($r.code-eq 200) 'fixed echo failed';$expected="received=$Length;sum=$([long]$Length*90)";Assert ([Text.Encoding]::ASCII.GetString($r.body)-eq $expected) 'fixed echo body/truncation mismatch'
  Step (@{case='fixed-body-crosses-64KiB';bytes=$Length;status='PASS'})
}
function Chunked-Echo {
  Phase ("cycle-$script:cycle/chunked-echo")
  $c=Tcp;$s=$c.GetStream();Write-Ascii $s ("POST /$Marker/echo HTTP/1.1`r`nHost: 127.0.0.1:$script:port`r`nTransfer-Encoding: chunked`r`nConnection: close`r`n`r`n")
  $total=0;foreach($length in @(17,65536,29)){$body=[byte[]]::new($length);for($i=0;$i-lt $length;$i++){$body[$i]=90};Write-Ascii $s ($length.ToString('x')+"`r`n");$s.Write($body,0,$body.Length);Write-Ascii $s "`r`n";$total+=$length};Write-Ascii $s "0`r`n`r`n"
  $r=Read-Reply $c;Assert ($r.code-eq 200) 'chunked echo failed';Assert ([Text.Encoding]::ASCII.GetString($r.body)-eq "received=$total;sum=$([long]$total*90)") 'chunked echo body mismatch';Step (@{case='chunked-body-explicit-EOF';bytes=$total;status='PASS'})
}
function Download {
  Phase ("cycle-$script:cycle/stream-download")
  $r=Get-Route 'download';Assert ($r.code-eq 200) 'stream download failed';Assert ($r.body.Length-eq 196608) 'response did not contain exactly three chunks';Assert (@($r.body|Where-Object{$_-ne 90}).Count-eq 0) 'stream response bytes changed';Step (@{case='streamed-three-response-chunks';bytes=$r.body.Length;status='PASS'})
}
function Cached {
  $r=Get-Route 'cached';Assert ($r.code-eq 304) 'conditional response status mismatch';Assert ($r.body.Length-eq 0) '304 unexpectedly contained a body';Assert ($r.headers-notmatch '(?im)^Content-Length:') '304 emitted wire body length as representation length';Assert ($r.headers-match '(?im)^ETag: "synthetic"') '304 validator missing';Step (@{case='304-omits-content-length-and-body';status='PASS';headers=$r.headers})
}
function Hold-Body {
  Phase ("cycle-$script:cycle/hold-partial-body")
  $c=Tcp;$s=$c.GetStream();Write-Ascii $s ("POST /$Marker/echo HTTP/1.1`r`nHost: 127.0.0.1:$script:port`r`nContent-Length: 2097152`r`nConnection: close`r`n`r`nZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ");return $c
}
try{
  Phase 'environment-and-prefix-check'
  $identity=[Security.Principal.WindowsIdentity]::GetCurrent()
  $admin=([Security.Principal.WindowsPrincipal]::new($identity)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
  $portSource=[Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback,0);$portSource.Start();$script:port=([Net.IPEndPoint]$portSource.LocalEndpoint).Port;$portSource.Stop()
  $prefix="http://127.0.0.1:$script:port/$Marker/"
  $aclBefore=(& netsh.exe http show urlacl "url=$prefix" 2>&1|Out-String)
  Assert (!$aclBefore.Contains($prefix)) 'random owned prefix unexpectedly has an existing reservation'
  $result.prefix=$prefix;$result.administratorToken=$admin;$result.urlAclBefore='ABSENT';@{prefix=$prefix;mayCreate=$false;marker=$Marker}|ConvertTo-Json -Compress|Set-Content -Encoding UTF8 -LiteralPath (Join-Path $Root '.http-urlacl.json')
  # Prefer an administrator token directly; no persistent OS mutation is needed.
  # If this token can delegate a reservation despite not reporting the role,
  # attempt ONLY our exact owned prefix/current user, and record it before use.
  if(!$admin){@{prefix=$prefix;mayCreate=$true;marker=$Marker}|ConvertTo-Json -Compress|Set-Content -Encoding UTF8 -LiteralPath (Join-Path $Root '.http-urlacl.json');$aclOut=(& netsh.exe http add urlacl "url=$prefix" ("user="+$identity.Name) listen=yes delegate=no 2>&1|Out-String);Assert ($LASTEXITCODE-eq 0) 'missing administrator/exact URL reservation prerequisite';$aclCreated=$true;$result.urlAclMode='EXACT_OWNED_TEMPORARY';$aclCheck=(& netsh.exe http show urlacl "url=$prefix" 2>&1|Out-String);Assert ($aclCheck.Contains($prefix)) 'owned reservation readback failed'}else{$result.urlAclMode='ADMIN_DIRECT_NO_MUTATION'}
  $exe=Join-Path $Root $ExeName
  Phase 'start-native-server'
  $script:server=Start-Process -FilePath $exe -ArgumentList @('--http-iocp-probe',"$script:port",$Marker,'60') -WindowStyle Hidden -WorkingDirectory $Root -RedirectStandardOutput (Join-Path $Root 'http.stdout') -RedirectStandardError (Join-Path $Root 'http.stderr') -PassThru
  # Force Process to retain its original native process handle before the child
  # can exit. WaitForExit alone may otherwise leave Start-Process's ExitCode
  # unset in Windows PowerShell. Keep this Process alive through exit readback.
  $script:serverHandle=$script:server.Handle
  Assert ($script:serverHandle-ne [IntPtr]::Zero) 'owned native process handle missing'
  $result.serverHandleRetained=$true
  $result.serverPid=$server.Id
  for($cycle=1;$cycle-le 2;$cycle++){
    Healthy
    Fixed-Echo 65537
    Chunked-Echo
    Download
    Cached
    if($cycle-eq 1){
      Phase 'eight-concurrent-requests'
      $clients=[Collections.Generic.List[object]]::new();for($i=0;$i-lt 8;$i++){$c=Tcp;Write-Ascii ($c.GetStream()) ("GET /$Marker/health HTTP/1.1`r`nHost: 127.0.0.1:$script:port`r`nConnection: close`r`n`r`n");$clients.Add($c)};$replyIndex=0;foreach($c in $clients){Phase ('concurrent-response-'+$replyIndex);$r=Read-Reply $c;$replyIndex++;Assert ($r.code-eq 200) 'bounded concurrent health failed'};Step (@{case='eight-concurrent-requests';count=8;status='PASS'})
      Phase 'overlong-header'
      $c=Tcp;Write-Ascii ($c.GetStream()) ("GET /$Marker/health HTTP/1.1`r`nHost: 127.0.0.1:$script:port`r`nX-Overlong: "+('Z'*20000)+"`r`nConnection: close`r`n`r`n");$r=Read-Reply $c $true;Assert ($r.code-ne 200) 'overlong header was accepted';Step (@{case='overlong-header-rejected-kernel-or-adapter';statusCode=$r.code;status='PASS'})
      Phase 'forbidden-synthetic-authorization'
      $c=Tcp;Write-Ascii ($c.GetStream()) ("GET /$Marker/health HTTP/1.1`r`nHost: 127.0.0.1:$script:port`r`nAuthorization: Synthetic-Probe-Rejected`r`nConnection: close`r`n`r`n");$r=Read-Reply $c;Assert ($r.code-eq 400) 'probe accepted forbidden authorization header';Step (@{case='probe-cannot-authenticate';status='PASS'})
      $c=Hold-Body;$c.Dispose();Start-Sleep -Milliseconds 100;Healthy;Step (@{case='client-aborted-pending-body';status='PASS'})
      $c=Tcp;Write-Ascii ($c.GetStream()) ("GET /$Marker/download HTTP/1.1`r`nHost: 127.0.0.1:$script:port`r`nConnection: close`r`n`r`n");$c.Dispose();Start-Sleep -Milliseconds 100;Healthy;Step (@{case='client-aborted-stream-response';status='PASS'})
      Phase 'first-partial-header-observation'
      $c=Tcp;Write-Ascii ($c.GetStream()) ("POST /$Marker/echo HTTP/1.1`r`nHost: 127.0.0.1:$script:port`r`nX-Wait: ");$watch=[Diagnostics.Stopwatch]::StartNew();$r=Read-Reply $c $true;$watch.Stop();Assert ($r.code-ne 200) 'partial header unexpectedly succeeded';Step (@{case='first-connection-partial-header-five-second-policy';configuredApplicationSeconds=5;observationWindowMilliseconds=7000;observedMilliseconds=$watch.ElapsedMilliseconds;statusCode=$r.code;closedDuringObservation=($r.eof-or $r.reset);clientReadTimedOut=$r.timedOut;status='NOT_VALIDATED';reason='HTTP.sys cannot associate the first connection with this URL group/request queue until headers are complete; application HeaderWait does not override the driver default; global HTTP configuration was not changed';source='https://learn.microsoft.com/en-us/windows/win32/api/http/ns-http-http_timeout_limit_info'})
    }
    $hold=Hold-Body;Start-Sleep -Milliseconds 50;$stop=Get-Route 'stop';Assert ($stop.code-eq 200) 'explicit stop response failed';$hold.Dispose();$hold=$null;Step (@{case='stop-with-body-and-head-operations-pending';cycle=$cycle;status='PASS'})
  }
  Phase 'native-process-final-drain'
  Assert ($server.WaitForExit((Budget 10000))) 'native drain/second cycle process did not finish'
  $nativeExitCode=$server.ExitCode
  $result.nativeExitCode=$nativeExitCode
  $result.nativeExitCodeType=if($null-eq $nativeExitCode){'null'}else{$nativeExitCode.GetType().FullName}
  Phase 'native-process-exit-readback' @{exitCode=$result.nativeExitCode;exitCodeType=$result.nativeExitCodeType;handleRetained=$result.serverHandleRetained}
  Checkpoint
  Assert ($nativeExitCode-is [int]) 'owned native process actual Int32 exit code missing'
  Assert ($nativeExitCode-eq 0) ('native probe exit failed: code='+$nativeExitCode+'; '+([IO.File]::ReadAllText((Join-Path $Root 'http.stderr'))))
  $stdout=@([IO.File]::ReadAllLines((Join-Path $Root 'http.stdout')));$native=@($stdout|Where-Object{$_.StartsWith('HTTP_IOCP_PASS ')}|ForEach-Object{$_.Substring(15)|ConvertFrom-Json})
  Assert ($native.Count-eq 2) 'two same-process native cycle reports missing'
  foreach($n in $native){Assert ($n.active_slots-eq 0-and $n.in_flight-eq 0) 'native ownership was not fully returned';Assert ($n.completions-gt 0-and $n.pending-ge 2) 'actual IOCP pending completions missing';Assert ($n.submitted-eq ($n.completions+$n.immediate_errors)) 'accepted/immediate/error completion accounting mismatch';Assert ($n.explicit_stop) 'native server timed out instead of explicit stop';Assert ($n.peak_in_flight-le 16) 'fixed original+cancel operation count exceeded bound';Assert ($n.native_allocation_bytes-le 2097152) 'native slot allocation budget exceeded'}
  Assert ($native[1].generation-gt $native[0].generation-and $native[1].stale_token_rejected) 'old generation accepted by actual second host'
  $result.native=$native;$result.stdout=$stdout;$result.stderr=@([IO.File]::ReadAllLines((Join-Path $Root 'http.stderr')));$result.synchronousSuccessObserved=(@($native|Where-Object{$_.immediate_success-gt 0}).Count-gt 0);$result.steps=$steps.ToArray();$result.completionLifecycle='PASS';$result.status='PARTIAL'
}catch{$result.status='FAIL';Phase 'caught-failure' @{message=$_.Exception.Message};$result.error=$_.Exception.Message;$result.steps=$steps.ToArray();if(Test-Path -LiteralPath (Join-Path $Root 'http.stdout')){$result.stdout=@([IO.File]::ReadAllLines((Join-Path $Root 'http.stdout')))};if(Test-Path -LiteralPath (Join-Path $Root 'http.stderr')){$result.stderr=@([IO.File]::ReadAllLines((Join-Path $Root 'http.stderr')))}}
finally{
  Phase 'finally/checkpoint-before-cleanup'
  Checkpoint
  if($hold){
    Phase 'finally/close-owned-client-socket'
    $hold.Dispose();$hold=$null
    Phase 'finally/owned-client-socket-closed'
  }
  # Do not Refresh/Dispose/WaitForExit a Process object or run netsh here. A
  # native failure can leave process/redirect teardown pending. The independent
  # Python owner first captures every checkpoint/log, then kills only its owned
  # native/client processes and removes only its journaled URL reservation/root.
  Phase 'finally/defer-process-and-urlacl-to-independent-owner' @{serverPid=$result.serverPid;temporaryUrlAcl=$aclCreated}
  $result.ownedProcessCleanup='DEFERRED_TO_PARENT'
  $result.urlAclCleanup='DEFERRED_TO_PARENT'
  Checkpoint
  Phase 'finally/complete'
}
$result.checkpoint=$false;$result.phase='finished'
[IO.File]::WriteAllText((Join-Path $Root '.http-result.json'),($result|ConvertTo-Json -Depth 8 -Compress),[Text.UTF8Encoding]::new($false))
Phase 'finished' @{status=$result.status}
$result|ConvertTo-Json -Depth 8 -Compress
'''


def safe_failure(error, phase):
    if isinstance(error, subprocess.TimeoutExpired):
        return {"kind": "timeout", "phase": phase, "timeoutSeconds": error.timeout}
    return {"kind": type(error).__name__, "phase": phase, "message": str(error)[-3000:]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", required=True)
    parser.add_argument("--identity", type=Path, required=True)
    parser.add_argument("--exe", type=Path, required=True)
    parser.add_argument("--report", type=Path, required=True)
    args = parser.parse_args()
    if args.host.startswith("-") or "@" not in args.host or any(c.isspace() for c in args.host):
        parser.error("host must be user@hostname")
    if not args.exe.is_file() or args.exe.suffix.lower() != ".exe" or not args.identity.is_file():
        parser.error("exe and private identity files must exist")
    remote = Remote(args.host, args.identity)
    phase = "create-owned-root"
    token = uuid.uuid4().hex
    marker = "FutureClassroom-Test-Http-" + token
    directory = remote.powershell("$root=Join-Path $env:TEMP " + ps_literal(marker) + ";New-Item -ItemType Directory -Path $root|Out-Null;Set-Content -LiteralPath (Join-Path $root '.http-probe-root') -Value " + ps_literal(token) + ";$root")
    root = ps_literal(directory)
    failure = None
    report = {"schemaVersion": 1, "scope": "HTTP.sys actual IOCP; two same-process synthetic loopback cycles only", "tls": "NOT_RUN", "cleanup": "NOT_RUN", "status": "FAIL", "runnerSourceSha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(), "remoteClientScriptSha256": hashlib.sha256(POWERSHELL.encode("utf-8-sig")).hexdigest()}
    try:
        phase = "read-platform"
        report["platform"] = json.loads(remote.powershell("$os=Get-CimInstance Win32_OperatingSystem;@{caption=$os.Caption;version=$os.Version;build=$os.BuildNumber;osArchitecture=$os.OSArchitecture}|ConvertTo-Json -Compress"))
        phase = "upload-owned-executable"
        remote.upload(args.exe, directory)
        digest = hashlib.sha256(args.exe.read_bytes()).hexdigest()
        report["exeSha256"] = digest
        phase = "verify-remote-executable"
        actual = remote.powershell("(Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path " + root + " " + ps_literal(args.exe.name) + ")).Hash.ToLowerInvariant()")
        if actual != digest:
            raise RuntimeError("remote executable digest mismatch")
        with tempfile.TemporaryDirectory(prefix="fc-http-probe-script-") as local:
            script = Path(local) / "http-probe.ps1"
            script.write_text(POWERSHELL, encoding="utf-8-sig")
            phase = "upload-owned-client-script"
            remote.upload(script, directory)
        phase = "parse-uploaded-client-script"
        parse_script = "$tokens=$null;$errors=$null;[System.Management.Automation.Language.Parser]::ParseFile((Join-Path " + root + " 'http-probe.ps1'),[ref]$tokens,[ref]$errors)|Out-Null;if($errors.Count-ne 0){throw ('owned HTTP client syntax errors: '+(($errors|ForEach-Object{$_.Message})-join '; '))};'CLIENT_SCRIPT_PARSE_PASS'"
        if remote.powershell(parse_script, timeout=15) != "CLIENT_SCRIPT_PARSE_PASS":
            raise RuntimeError("owned HTTP PowerShell client parse result missing")
        report["clientScriptParse"] = "PASS"
        # Bypass applies only to this child PowerShell process and our owned
        # script. Never change the machine/user execution policy.
        command = "$ErrorActionPreference='Continue';$childOutput=@(& powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File (Join-Path " + root + " 'http-probe.ps1') -Root " + root + " -ExeName " + ps_literal(args.exe.name) + " -Marker " + ps_literal(marker) + " 2>&1|ForEach-Object{\"$_\"});$childExit=$LASTEXITCODE;$ErrorActionPreference='Stop';@{exitCode=$childExit;output=$childOutput}|ConvertTo-Json -Depth 4 -Compress"
        phase = "http-native-client-and-two-cycle-drain"
        runner = json.loads(remote.powershell(command, timeout=160).lstrip("\ufeff"))
        report["runner"] = runner
        if runner.get("exitCode") != 0:
            raise RuntimeError("owned HTTP PowerShell child failed: " + "\n".join(runner.get("output", []))[-3000:])
        result = json.loads("\n".join(runner.get("output", [])).lstrip("\ufeff"))
        report["native"] = result
        if result.get("status") not in ("PASS", "PARTIAL"):
            raise RuntimeError(result.get("error", "native HTTP probe failed"))
        report["status"] = result["status"]
        report["p6_11"] = "NOT_RUN"
        report["firstConnectionHeaderDeadline"] = "NOT_VALIDATED"
    except Exception as error:
        failure = safe_failure(error, phase)
        report["error"] = failure
    finally:
        # Preserve checkpoints even when the SSH/PowerShell call timed out. Do
        # not rely on the remote client reaching its final JSON output/finally.
        capture = r"""$root=__ROOT__;$data=@{};foreach($name in @('http.stdout','http.stderr','.http-progress.json','.http-progress.jsonl','.http-result.json')){$path=Join-Path $root $name;if(Test-Path -LiteralPath $path){try{$text=(@([IO.File]::ReadLines($path)|Select-Object -Last 128)-join "`n");if($text.Length-gt 65536){$text=$text.Substring($text.Length-65536)};$data[$name]=$text}catch{$data[$name]='CAPTURE_FAILED'}}};$data|ConvertTo-Json -Depth 3 -Compress""".replace("__ROOT__", root)
        try:
            report["diagnosticsBeforeCleanup"] = json.loads(remote.powershell(capture, timeout=15).lstrip("\ufeff"))
            current = report["diagnosticsBeforeCleanup"].get(".http-progress.json")
            if current:
                try:
                    report["lastRemoteProgress"] = json.loads(current)
                except (TypeError, json.JSONDecodeError):
                    pass
        except Exception as error:
            report["diagnosticsCaptureError"] = safe_failure(error, "collect-owned-progress-and-native-logs")
        cleanup = "$root=" + root + ";if(([IO.File]::ReadAllText((Join-Path $root '.http-probe-root'))).Trim()-ne " + ps_literal(token) + "){throw 'HTTP cleanup marker mismatch'};Get-CimInstance Win32_Process|Where-Object{($_.ExecutablePath-and $_.ExecutablePath.StartsWith($root+'\\',[StringComparison]::OrdinalIgnoreCase))-or ($_.Name-eq 'powershell.exe'-and $_.CommandLine-and $_.CommandLine.IndexOf((Join-Path $root 'http-probe.ps1'),[StringComparison]::OrdinalIgnoreCase)-ge 0)}|ForEach-Object{Stop-Process -Id $_.ProcessId -Force};if(Test-Path -LiteralPath (Join-Path $root '.http-urlacl.json')){$entry=[IO.File]::ReadAllText((Join-Path $root '.http-urlacl.json'))|ConvertFrom-Json;if($entry.marker-ne " + ps_literal(marker) + "-or !$entry.prefix.StartsWith('http://127.0.0.1:',[StringComparison]::Ordinal)-or !$entry.prefix.EndsWith('/'+$entry.marker+'/')){throw 'owned URL reservation cleanup marker mismatch'};if($entry.mayCreate){$reservation=(& netsh.exe http show urlacl ('url='+$entry.prefix) 2>&1|Out-String);if($reservation.Contains($entry.prefix)){& netsh.exe http delete urlacl ('url='+$entry.prefix)|Out-Null;if($LASTEXITCODE-ne 0){throw 'owned URL reservation recovery cleanup failed'}}};$remaining=(& netsh.exe http show urlacl ('url='+$entry.prefix) 2>&1|Out-String);if($remaining.Contains($entry.prefix)){throw 'owned URL reservation remains'}};Remove-Item -LiteralPath $root -Recurse -Force;if(Test-Path -LiteralPath $root){throw 'HTTP probe root remains'};'CLEANUP_PASS'"
        try:
            outcome = remote.powershell(cleanup, timeout=30)
            report["cleanup"] = "PASS" if outcome == "CLEANUP_PASS" else "FAIL"
            if report["cleanup"] == "PASS":
                report["ownedProcessCleanup"] = "PASS"
                report["urlAclAfter"] = "ABSENT"
        except Exception as error:
            report["cleanup"] = "FAIL"
            report["cleanupError"] = safe_failure(error, "cleanup-owned-processes-root-and-url-reservation")
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
        if report["cleanup"] != "PASS":
            raise RuntimeError("HTTP probe cleanup failed; inspect report") from None
    if failure is not None:
        raise RuntimeError("HTTP probe failed; " + json.dumps(failure, ensure_ascii=False)) from None


if __name__ == "__main__":
    main()
