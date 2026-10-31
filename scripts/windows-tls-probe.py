#!/usr/bin/env python3
"""Exercise the explicit Schannel loopback diagnostic through an owned SSH tunnel.

Creates one disposable CurrentUser MY certificate and CNG key, never changes ROOT,
HTTP.sys, firewall, Run or SSH configuration. SSH-forwarded timings are diagnostic
observations, not evidence for the native strict TCP-to-header five-second gate.
The New-SelfSignedCertificate fixture requires a modern Windows test host; it does
not establish Windows 7 support for either the fixture cmdlet or the application.
"""
import argparse
import base64
import concurrent.futures
import hashlib
import importlib.util
import json
from pathlib import Path
import queue
import re
import socket
import ssl
import subprocess
import sys
import tempfile
import threading
import time
import uuid

sys.dont_write_bytecode = True
_spec = importlib.util.spec_from_file_location("fc_windows_probe", Path(__file__).with_name("windows-probe.py"))
_helpers = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_helpers)
ps_literal = _helpers.ps_literal

MAX_LOG_BYTES = 512 * 1024
MAX_RESPONSE_BYTES = 64 * 1024
RAW_NAMES = ("tls.stdout", "tls.stderr", ".tls-process.json", ".tls-supervisor.json", ".tls-setup-process.json", ".tls-fixture.json", "tls-public.pem")
READY = "TLS_FRONTDOOR_READY "
PASS = "TLS_FRONTDOOR_PASS "
EXIT = "TLS_FRONTDOOR_PROCESS_EXIT "

COMMON = r'''
$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
$OutputEncoding=[Console]::OutputEncoding
function Assert([bool]$Condition,[string]$Message){if(!$Condition){throw $Message}}
function Write-Json([string]$Path,$Value){[IO.File]::WriteAllText($Path,($Value|ConvertTo-Json -Depth 8 -Compress),[Text.UTF8Encoding]::new($false))}
function Store-Snapshot([string]$Exclude=''){
  $entries=@(Get-ChildItem -LiteralPath 'Cert:\CurrentUser\My'|Where-Object{$_.Thumbprint-ne $Exclude}|ForEach-Object{
    $cert=$_;$sha=[Security.Cryptography.SHA256]::Create()
    try{$der=([BitConverter]::ToString($sha.ComputeHash($cert.RawData))).Replace('-','').ToLowerInvariant()}finally{$sha.Dispose()}
    $cert.Thumbprint.ToUpperInvariant()+':'+$der+':'+[Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes([string]$cert.FriendlyName))+':'+$cert.HasPrivateKey
  })
  [Array]::Sort($entries,[StringComparer]::Ordinal)
  $sha=[Security.Cryptography.SHA256]::Create()
  try{$digest=([BitConverter]::ToString($sha.ComputeHash([Text.Encoding]::UTF8.GetBytes(($entries-join "`n"))))).Replace('-','').ToLowerInvariant()}finally{$sha.Dispose()}
  return @{count=$entries.Count;sha256=$digest}
}
'''

SETUP = r'''
$marker=__MARKER__;$root=__ROOT__
Assert (([IO.File]::ReadAllText((Join-Path $root '.tls-probe-root')))-eq $marker) 'TLS root marker mismatch'
$setupProcess=Get-Process -Id $PID
Write-Json (Join-Path $root '.tls-setup-process.json') @{marker=$marker;pid=$PID;startTicks=$setupProcess.StartTime.ToUniversalTime().Ticks.ToString();exe=$setupProcess.Path}
$setupProcess.Dispose()
$provider=[Security.Cryptography.CngProvider]::MicrosoftSoftwareKeyStorageProvider
$keyName=$marker+'-key'
$before=Store-Snapshot
Assert (![Security.Cryptography.CngKey]::Exists($keyName,$provider)) 'unique TLS key unexpectedly exists'
Assert (@(Get-ChildItem -LiteralPath 'Cert:\CurrentUser\My'|Where-Object{$_.FriendlyName-eq $marker}).Count-eq 0) 'unique TLS certificate unexpectedly exists'
$manifest=@{marker=$marker;keyName=$keyName;provider=$provider.Provider;keyAbsentBefore=$true;before=$before;thumbprint=$null}
Write-Json (Join-Path $root '.tls-fixture.json') $manifest
$cert=New-SelfSignedCertificate -Type SSLServerAuthentication -Subject 'CN=localhost' -TextExtension @('2.5.29.17={text}DNS=localhost&IPAddress=127.0.0.1') -CertStoreLocation 'Cert:\CurrentUser\My' -Provider $provider.Provider -Container $keyName -KeyAlgorithm RSA -KeyLength 2048 -HashAlgorithm SHA256 -KeyExportPolicy NonExportable -KeyUsage DigitalSignature,KeyEncipherment -NotBefore (Get-Date).AddMinutes(-5) -NotAfter (Get-Date).AddDays(1) -FriendlyName $marker
$manifest.thumbprint=$cert.Thumbprint.ToUpperInvariant()
Write-Json (Join-Path $root '.tls-fixture.json') $manifest
$rsa=$null
try{
  $rsa=[Security.Cryptography.X509Certificates.RSACertificateExtensions]::GetRSAPrivateKey($cert)
  Assert ($rsa-is [Security.Cryptography.RSACng]) 'TLS fixture key is not CNG'
  Assert ($rsa.Key.KeyName-eq $keyName-and $rsa.Key.Provider.Provider-eq $provider.Provider) 'TLS fixture key ownership mismatch'
}finally{if($rsa){$rsa.Dispose()}}
Assert ([Security.Cryptography.CngKey]::Exists($keyName,$provider)) 'TLS fixture key missing after creation'
$unrelated=Store-Snapshot $manifest.thumbprint
Assert ($unrelated.count-eq $before.count-and $unrelated.sha256-eq $before.sha256) 'unrelated MY certificates changed during fixture creation'
$der=$cert.RawData
$pem="-----BEGIN CERTIFICATE-----`n"+[Convert]::ToBase64String($der,[Base64FormattingOptions]::InsertLineBreaks).Replace("`r",'')+"`n-----END CERTIFICATE-----`n"
[IO.File]::WriteAllText((Join-Path $root 'tls-public.pem'),$pem,[Text.ASCIIEncoding]::new())
$os=Get-CimInstance Win32_OperatingSystem
@{thumbprint=$manifest.thumbprint;keyName=$keyName;provider=$provider.Provider;myBefore=$before;publicPem=$pem;privateKeyExported=$false;san=(@($cert.Extensions|Where-Object{$_.Oid.Value-eq '2.5.29.17'}|ForEach-Object{$_.Format($false)})-join ';');platform=@{caption=$os.Caption;version=$os.Version;build=$os.BuildNumber;osArchitecture=$os.OSArchitecture}}|ConvertTo-Json -Depth 5 -Compress
'''

LAUNCH = r'''
$root=__ROOT__;$marker=__MARKER__;$thumbprint=__THUMBPRINT__;$exe=Join-Path $root __EXENAME__
Assert (([IO.File]::ReadAllText((Join-Path $root '.tls-probe-root')))-eq $marker) 'TLS launch root marker mismatch'
$env:TEMP=Join-Path $root 'temp';$env:TMP=$env:TEMP
New-Item -ItemType Directory -Path $env:TEMP|Out-Null
$child=$null;$handle=[IntPtr]::Zero;$seen=0;$watch=[Diagnostics.Stopwatch]::StartNew()
$supervisor=Get-Process -Id $PID
Write-Json (Join-Path $root '.tls-supervisor.json') @{marker=$marker;pid=$PID;startTicks=$supervisor.StartTime.ToUniversalTime().Ticks.ToString();exe=$supervisor.Path}
$supervisor.Dispose()
function Forward-Lines {
  $path=Join-Path $root 'tls.stdout'
  if(Test-Path -LiteralPath $path){
    # Start-Process retains a redirected writer while the child is running.
    # Permit that writer explicitly; ReadAllText's sharing mode conflicts with
    # it. Keep both byte checks and a fixed, capped character allocation.
    $stream=[IO.File]::Open($path,[IO.FileMode]::Open,[IO.FileAccess]::Read,([IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete))
    $reader=$null
    try{
      Assert ($stream.Length-le 524288) 'native TLS stdout exceeded diagnostic bound'
      $reader=[IO.StreamReader]::new($stream,[Text.UTF8Encoding]::new($false,$true),$false,4096,$true)
      $chars=[char[]]::new(524289)
      $length=$reader.ReadBlock($chars,0,$chars.Length)
      Assert ($length -le 524288 -and $stream.Length -le 524288) 'native TLS stdout grew beyond diagnostic bound'
      $text=[string]::new($chars,0,$length)
    }finally{if($reader){$reader.Dispose()};$stream.Dispose()}
    $lines=$text.Split([char]10)
    $complete=$lines.Length-1
    while($script:seen-lt $complete){[Console]::WriteLine($lines[$script:seen].TrimEnd([char]13));$script:seen++}
    [Console]::Out.Flush()
  }
}
try{
  $child=Start-Process -FilePath $exe -ArgumentList @('--tls-lifecycle-probe',$thumbprint,$marker) -WindowStyle Hidden -WorkingDirectory $root -RedirectStandardOutput (Join-Path $root 'tls.stdout') -RedirectStandardError (Join-Path $root 'tls.stderr') -PassThru
  $handle=$child.Handle
  Assert ($handle-ne [IntPtr]::Zero) 'owned native TLS process handle missing'
  $entry=@{marker=$marker;pid=$child.Id;startTicks=$child.StartTime.ToUniversalTime().Ticks.ToString();exe=$exe}
  Write-Json (Join-Path $root '.tls-process.json') $entry
  while(!$child.WaitForExit(10)){
    Forward-Lines
    Assert ($watch.ElapsedMilliseconds-lt 90000) 'native TLS diagnostic exceeded its independent 90-second observation limit'
  }
  Forward-Lines
  Assert ($child.ExitCode-is [int]) 'actual native TLS Int32 exit code missing'
  [Console]::WriteLine('TLS_FRONTDOOR_PROCESS_EXIT '+(@{code=$child.ExitCode}|ConvertTo-Json -Compress));[Console]::Out.Flush()
}finally{
  if($child){
    if(!$child.HasExited){$child.Kill();Assert ($child.WaitForExit(5000)) 'owned TLS child did not terminate after diagnostic failure'}
    $child.Dispose()
  }
}
'''

CAPTURE = r'''
$root=__ROOT__;$marker=__MARKER__
Assert (([IO.File]::ReadAllText((Join-Path $root '.tls-probe-root')))-eq $marker) 'TLS capture root marker mismatch'
$data=@{}
foreach($name in @('tls.stdout','tls.stderr','.tls-process.json','.tls-supervisor.json','.tls-setup-process.json','.tls-fixture.json','tls-public.pem')){
  $path=Join-Path $root $name
  if(Test-Path -LiteralPath $path){
    # The process is physically stopped before this script runs. Hash a stream,
    # never load a potentially oversized original failure log into memory/JSON.
    $stream=[IO.File]::Open($path,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::Read)
    $sha=[Security.Cryptography.SHA256]::Create()
    try{
      $length=$stream.Length
      $digest=([BitConverter]::ToString($sha.ComputeHash($stream))).Replace('-','').ToLowerInvariant()
      Assert ($stream.Length-eq $length) 'TLS raw evidence length changed while hashing'
      $data[$name]=@{bytes=$length;sha256=$digest}
    }finally{$sha.Dispose();$stream.Dispose()}
  }
}
$data|ConvertTo-Json -Depth 4 -Compress
'''

STOP = r'''
$root=__ROOT__;$marker=__MARKER__
Assert ((Split-Path -Leaf $root)-eq $marker) 'TLS cleanup basename mismatch'
Assert (([IO.File]::ReadAllText((Join-Path $root '.tls-probe-root')))-eq $marker) 'TLS cleanup root marker mismatch'
$killed=0
# Stop the recorded supervisor first so it cannot create another child after
# the native-child audit. A process object retains its original handle to exit.
foreach($name in @('.tls-setup-process.json','.tls-supervisor.json','.tls-process.json')){
  $processFile=Join-Path $root $name
  if(Test-Path -LiteralPath $processFile){
    $entry=[IO.File]::ReadAllText($processFile)|ConvertFrom-Json
    Assert ($entry.marker-eq $marker-and [int]$entry.pid-ne $PID) 'TLS process journal ownership mismatch'
    if($name-eq '.tls-process.json'){Assert ($entry.exe-eq (Join-Path $root __EXENAME__)) 'TLS native process path mismatch'}
    else{Assert ((Split-Path -Leaf $entry.exe)-eq 'powershell.exe') 'TLS supervisor path mismatch'}
    $ownedProcess=Get-Process -Id ([int]$entry.pid) -ErrorAction SilentlyContinue
    if($ownedProcess){
      $retainedHandle=$ownedProcess.Handle
      Assert ($retainedHandle-ne [IntPtr]::Zero) 'owned TLS process handle unavailable during cleanup'
      Assert ($ownedProcess.Path-eq $entry.exe-and $ownedProcess.StartTime.ToUniversalTime().Ticks.ToString()-eq $entry.startTicks) 'TLS process identity changed; refusing to kill reused PID'
      $ownedProcess.Kill();Assert ($ownedProcess.WaitForExit(5000)) 'owned TLS process cleanup timeout';$killed++;$ownedProcess.Dispose()
    }
  }
}
# Recover the narrow Start-Process / process-journal gap, never a process-name
# sweep. Both the unique uploaded path and the exact nonce argument must match.
$recovered=0
$remaining=@(Get-CimInstance Win32_Process|Where-Object{$_.ExecutablePath-and $_.ExecutablePath-eq (Join-Path $root __EXENAME__)})
foreach($candidate in $remaining){
  Assert ($candidate.CommandLine-and $candidate.CommandLine.Contains('--tls-lifecycle-probe')-and $candidate.CommandLine.Contains($marker)) 'unjournaled executable is not this owned diagnostic'
  $ownedProcess=Get-Process -Id $candidate.ProcessId
  $retainedHandle=$ownedProcess.Handle
  Assert ($retainedHandle-ne [IntPtr]::Zero) 'recovered TLS process handle unavailable during cleanup'
  Assert ($ownedProcess.Path-eq (Join-Path $root __EXENAME__)) 'unjournaled diagnostic path changed'
  $ownedProcess.Kill();Assert ($ownedProcess.WaitForExit(5000)) 'recovered owned TLS process cleanup timeout';$recovered++;$ownedProcess.Dispose()
}
$remaining=@(Get-CimInstance Win32_Process|Where-Object{$_.ExecutablePath-and $_.ExecutablePath-eq (Join-Path $root __EXENAME__)})
Assert ($remaining.Count-eq 0) 'owned TLS executable still running or process journal missing'
@{status='PASS';ownedProcessesRemaining=0;ownedProcessesKilled=$killed;recoveredNativeProcesses=$recovered}|ConvertTo-Json -Compress
'''

CLEANUP = r'''
$root=__ROOT__;$marker=__MARKER__
Assert ((Split-Path -Leaf $root)-eq $marker) 'TLS cleanup basename mismatch'
Assert (([IO.File]::ReadAllText((Join-Path $root '.tls-probe-root')))-eq $marker) 'TLS cleanup root marker mismatch'
Assert (@(Get-CimInstance Win32_Process|Where-Object{$_.ExecutablePath-and $_.ExecutablePath-eq (Join-Path $root __EXENAME__)}).Count-eq 0) 'TLS executable still running before key cleanup'
$fixtureFile=Join-Path $root '.tls-fixture.json';$certificateRemoved=0;$keyExists=$false;$after=$null;$restored=$null;$deleteKeyFallback=$false
if(Test-Path -LiteralPath $fixtureFile){
  $entry=[IO.File]::ReadAllText($fixtureFile)|ConvertFrom-Json
  Assert ($entry.marker-eq $marker-and $entry.keyName-eq ($marker+'-key')-and $entry.provider-eq 'Microsoft Software Key Storage Provider'-and $entry.keyAbsentBefore-eq $true) 'TLS fixture cleanup ownership mismatch'
  $provider=[Security.Cryptography.CngProvider]::new($entry.provider)
  $certificates=@(Get-ChildItem -LiteralPath 'Cert:\CurrentUser\My'|Where-Object{$_.FriendlyName-eq $marker})
  Assert ($certificates.Count-le 1) 'unexpected duplicate owned TLS certificates; refusing broad cleanup'
  foreach($cert in $certificates){
    Assert ((!$entry.thumbprint)-or $cert.Thumbprint-eq $entry.thumbprint) 'owned TLS certificate thumbprint changed'
    $rsa=$null
    try{
      $rsa=[Security.Cryptography.X509Certificates.RSACertificateExtensions]::GetRSAPrivateKey($cert)
      Assert ($rsa-is [Security.Cryptography.RSACng]-and $rsa.Key.KeyName-eq $entry.keyName-and $rsa.Key.Provider.Provider-eq $entry.provider) 'owned TLS certificate private key mismatch'
    }finally{if($rsa){$rsa.Dispose()}}
    $certPath='Cert:\CurrentUser\My\'+$cert.Thumbprint
    try{Remove-Item -Path $certPath -DeleteKey}
    catch{
      $deleteKeyFallback=$true
      # Some Certificate-provider versions cannot perform DeleteKey. The exact
      # existing named CNG key was checked above; never enumerate/delete keys.
      if([Security.Cryptography.CngKey]::Exists($entry.keyName,$provider)){
        $key=[Security.Cryptography.CngKey]::Open($entry.keyName,$provider)
        try{$key.Delete()}finally{$key.Dispose()}
      }
      if(Test-Path -LiteralPath $certPath){Remove-Item -Path $certPath}
    }
    $certificateRemoved++
  }
  # A creation failure can leave just our previously absent, exact named key.
  # Delete it through CNG only after the immutable ownership manifest checks.
  if([Security.Cryptography.CngKey]::Exists($entry.keyName,$provider)){
    $key=[Security.Cryptography.CngKey]::Open($entry.keyName,$provider)
    try{$key.Delete()}finally{$key.Dispose()}
  }
  $keyExists=[Security.Cryptography.CngKey]::Exists($entry.keyName,$provider)
  Assert (!$keyExists) 'owned TLS CNG key still exists'
  Assert (@(Get-ChildItem -LiteralPath 'Cert:\CurrentUser\My'|Where-Object{$_.FriendlyName-eq $marker}).Count-eq 0) 'owned TLS certificate still exists'
  $after=Store-Snapshot
  $restored=($after.count-eq $entry.before.count-and $after.sha256-eq $entry.before.sha256)
  Assert $restored 'CurrentUser MY collection digest not restored; preserving owned evidence root'
}
Remove-Item -LiteralPath $root -Recurse -Force
Assert (!(Test-Path -LiteralPath $root)) 'owned TLS root remains'
@{status='PASS';rootAbsent=$true;ownedProcessesRemaining=0;certificatesRemoved=$certificateRemoved;deleteKeyFallbackUsed=$deleteKeyFallback;cngKeyExists=$keyExists;myAfter=$after;myDigestRestored=$restored;sshConfiguration='UNTOUCHED';rootTrustStore='UNTOUCHED';httpSys='UNTOUCHED';firewall='UNTOUCHED'}|ConvertTo-Json -Depth 5 -Compress
'''


class Remote(_helpers.Remote):
    def __init__(self, target, identity):
        self.host = target
        self.options = ["-o", "BatchMode=yes", "-o", "ConnectTimeout=8"]
        if identity:
            self.options += ["-i", str(identity), "-o", "IdentitiesOnly=yes"]
        self.script_runs = []

    def command(self, script):
        encoded = base64.b64encode((COMMON + script).encode("utf-16-le")).decode("ascii")
        return ["ssh", *self.options, self.host, "powershell.exe -NoProfile -NonInteractive -EncodedCommand " + encoded]

    def command_file(self, path):
        return self.command("& powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File " + ps_literal(path) + ";exit $LASTEXITCODE")

    def run_file(self, path, timeout):
        # Keep the SSH/cmd.exe command short. Large UTF-16 EncodedCommand strings
        # can exceed the Windows default SSH shell's command-line limit.
        entry = {"script": path.rsplit("\\", 1)[-1]}
        try:
            result = subprocess.run(self.command_file(path), capture_output=True, timeout=timeout)
            entry["exitCode"] = result.returncode
            streams = (("stdout", result.stdout), ("stderr", result.stderr))
        except subprocess.TimeoutExpired as error:
            entry["status"] = "TIMEOUT"
            streams = (("stdout", error.stdout or b""), ("stderr", error.stderr or b""))
            for name, data in streams:
                entry[name] = {"bytes": len(data), "sha256": digest(data), "base64": base64.b64encode(data).decode("ascii")}
            self.script_runs.append(entry)
            raise
        for name, data in streams:
            entry[name] = {"bytes": len(data), "sha256": digest(data), "base64": base64.b64encode(data).decode("ascii")}
        self.script_runs.append(entry)
        if result.returncode:
            raise RuntimeError("owned TLS control script failed: " + result.stderr.decode("utf-8", "replace")[-3000:])
        return result.stdout.decode("utf-8", "strict").strip().lstrip("\ufeff")

    def download(self, remote_path, local_path):
        result = subprocess.run(["scp", "-q", *self.options, self.host + ":" + remote_path.replace("\\", "/"), str(local_path)], capture_output=True, timeout=60)
        if result.returncode:
            raise RuntimeError("owned TLS raw artifact download failed: " + result.stderr.decode("utf-8", "replace")[-3000:])


def digest(data):
    return hashlib.sha256(data).hexdigest()


def file_identity(path):
    length = 0
    sha = hashlib.sha256()
    with path.open("rb") as stream:
        while chunk := stream.read(1024 * 1024):
            length += len(chunk)
            sha.update(chunk)
    return {"bytes": length, "sha256": sha.hexdigest()}


def preserve_raw(remote, directory, report_path, token, metadata, report):
    """Keep complete immutable artifacts before allowing their remote deletion."""
    if not isinstance(metadata, dict) or any(name not in RAW_NAMES for name in metadata):
        raise RuntimeError("unexpected remote TLS raw evidence metadata names")
    artifact_root = report_path.parent / (report_path.stem + "-" + token + "-raw")
    artifact_root.mkdir(parents=True, exist_ok=False)
    report["rawPreservation"] = {"status": "INCOMPLETE", "directory": str(artifact_root.resolve()), "files": {}}
    (artifact_root / "remote-metadata.json").write_text(json.dumps(metadata, sort_keys=True, indent=2) + "\n", encoding="utf-8")
    for name, expected in metadata.items():
        if (not isinstance(expected, dict) or type(expected.get("bytes")) is not int or expected["bytes"] < 0
                or not isinstance(expected.get("sha256"), str) or not re.fullmatch(r"[0-9a-f]{64}", expected["sha256"])):
            raise RuntimeError("invalid remote TLS raw evidence byte/hash identity")
        partial = artifact_root / (name + ".partial")
        final = artifact_root / name
        entry = {"status": "DOWNLOADING", "remote": expected, "partialArtifact": str(partial.resolve())}
        report["rawPreservation"]["files"][name] = entry
        try:
            remote.download(directory + "\\" + name, partial)
            actual = file_identity(partial)
            entry["downloaded"] = actual
            if actual != {"bytes": expected["bytes"], "sha256": expected["sha256"]}:
                raise RuntimeError("downloaded TLS raw artifact byte/hash mismatch: " + name)
            partial.rename(final)
            entry.update({"status": "PASS", "artifact": str(final.resolve())})
            del entry["partialArtifact"]
            if actual["bytes"] <= MAX_LOG_BYTES:
                entry["base64"] = base64.b64encode(final.read_bytes()).decode("ascii")
            else:
                entry["inlineCapture"] = "OMITTED/complete original retained in verified artifact"
        except Exception:
            entry["status"] = "FAIL"
            if partial.is_file():
                entry["partialDownloaded"] = file_identity(partial)
            raise
    report["rawPreservation"]["status"] = "PASS"


def failure(error, phase):
    if isinstance(error, subprocess.TimeoutExpired):
        return {"kind": "timeout", "phase": phase, "seconds": error.timeout}
    return {"kind": type(error).__name__, "phase": phase, "message": str(error)[-3000:]}


def script(template, directory, marker, exe_name, thumbprint=""):
    replacements = {"__ROOT__": directory, "__MARKER__": marker, "__EXENAME__": exe_name, "__THUMBPRINT__": thumbprint}
    for key, value in replacements.items():
        template = template.replace(key, ps_literal(value))
    return template


class Logs:
    """Keep bounded exact bytes while continuously draining both child pipes."""
    def __init__(self, process):
        self.stdout = bytearray()
        self.stderr = bytearray()
        self.lines = queue.Queue(maxsize=128)
        self.overflow = threading.Event()
        self.observed_bytes = {"stdout": 0, "stderr": 0}
        self.threads = []
        for pipe, target, emit in ((process.stdout, self.stdout, True), (process.stderr, self.stderr, False)):
            thread = threading.Thread(target=self._read, args=(pipe, target, emit), daemon=True)
            thread.start()
            self.threads.append(thread)

    def _read(self, pipe, target, emit):
        partial = bytearray()
        stream_name = "stdout" if emit else "stderr"
        try:
            while chunk := pipe.read1(4096):
                self.observed_bytes[stream_name] += len(chunk)
                available = MAX_LOG_BYTES - len(target)
                target.extend(chunk[:available])
                if len(chunk) > available:
                    self.overflow.set()
                if emit and not self.overflow.is_set():
                    partial.extend(chunk)
                    while b"\n" in partial:
                        line, _, rest = partial.partition(b"\n")
                        partial = bytearray(rest)
                        try:
                            self.lines.put_nowait(line.rstrip(b"\r").decode("utf-8", "strict").lstrip("\ufeff"))
                        except (queue.Full, UnicodeDecodeError):
                            self.overflow.set()
        finally:
            pipe.close()

    def join(self):
        for thread in self.threads:
            thread.join(timeout=5)
        if any(thread.is_alive() for thread in self.threads):
            raise RuntimeError("owned SSH pipe reader still alive")
        if self.overflow.is_set():
            raise RuntimeError("owned SSH diagnostic log/line bound exceeded or invalid UTF-8")

    def report(self):
        return {name: {"bytes": len(data), "observedBytes": self.observed_bytes[name], "captureScope": "PREFIX_ONLY" if self.observed_bytes[name] > len(data) else "COMPLETE", "sha256Scope": "captured bytes only", "sha256": digest(data), "base64": base64.b64encode(data).decode("ascii")}
                for name, data in (("stdout", self.stdout), ("stderr", self.stderr))}


class Tunnel:
    """One SSH master, with per-batch forwarding commands that spend no slot."""
    def __init__(self, remote, local_root):
        self.remote = remote
        self.control = str(Path(local_root) / "ssh-control")
        self.forward = None
        self.process = subprocess.Popen(["ssh", *remote.options, "-M", "-S", self.control, "-N", "-o", "ControlPersist=no", "-o", "ExitOnForwardFailure=yes", remote.host], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        self.logs = Logs(self.process)
        until = time.monotonic() + 12
        try:
            while time.monotonic() < until:
                if self.process.poll() is not None:
                    raise RuntimeError("owned SSH tunnel master exited during startup")
                if Path(self.control).exists():
                    check = self._control("check", timeout=2)
                    if check.returncode == 0:
                        return
                time.sleep(0.02)
            raise RuntimeError("owned SSH tunnel master readiness timeout")
        except BaseException:
            self.close()
            raise

    def _control(self, operation, *extra, timeout=5):
        return subprocess.run(["ssh", *self.remote.options, "-S", self.control, "-O", operation, *extra, self.remote.host], capture_output=True, timeout=timeout)

    def open(self, remote_port):
        self.cancel()
        with socket.socket() as reserved:
            reserved.bind(("127.0.0.1", 0))
            local_port = reserved.getsockname()[1]
        self.forward = f"127.0.0.1:{local_port}:127.0.0.1:{remote_port}"
        opened = self._control("forward", "-o", "ExitOnForwardFailure=yes", "-L", self.forward)
        if opened.returncode:
            raise RuntimeError("owned SSH forward failed: " + opened.stderr.decode("utf-8", "replace")[-1000:])
        # A successful mux forward reply acknowledges listener establishment.
        # Never connect merely to probe readiness: every connection spends one
        # of the diagnostic server's four physical acceptance slots.
        return local_port

    def cancel(self):
        if self.forward:
            canceled = self._control("cancel", "-L", self.forward)
            self.forward = None
            if canceled.returncode:
                raise RuntimeError("owned SSH forward cancellation failed")

    def close(self):
        try:
            self.cancel()
        finally:
            if self.process.poll() is None:
                self.process.terminate()
                try:
                    self.process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    self.process.kill()
                    self.process.wait(timeout=5)
            self.logs.join()


def context(cafile):
    value = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
    value.minimum_version = ssl.TLSVersion.TLSv1_2
    value.maximum_version = ssl.TLSVersion.TLSv1_2
    value.check_hostname = True
    value.verify_mode = ssl.CERT_REQUIRED
    value.load_verify_locations(cafile=str(cafile))
    if not value.check_hostname or value.verify_mode != ssl.CERT_REQUIRED or value.cert_store_stats()["x509"] != 1:
        raise RuntimeError("isolated client certificate verification configuration changed")
    return value


def untrusted_context(expected_der):
    """Normal client trust roots, without adding our disposable self-signed leaf."""
    value = ssl.create_default_context(ssl.Purpose.SERVER_AUTH)
    value.minimum_version = ssl.TLSVersion.TLSv1_2
    value.maximum_version = ssl.TLSVersion.TLSv1_2
    value.check_hostname = True
    value.verify_mode = ssl.CERT_REQUIRED
    if not value.check_hostname or value.verify_mode != ssl.CERT_REQUIRED or expected_der in value.get_ca_certs(binary_form=True):
        raise RuntimeError("wrong-CA client unexpectedly trusts the isolated fixture")
    return value


def exercise_batch(local_port, marker, cycle, tls_context, wrong_ca_context, expected_der):
    """Exactly four connections per cycle; neither health probes nor retries."""
    start = time.monotonic_ns()
    barrier = threading.Barrier(4)

    def client(case):
        event = {"cycle": cycle, "case": case, "status": "FAIL"}
        connection = None
        try:
            barrier.wait(timeout=3)
            connection = socket.create_connection(("127.0.0.1", local_port), timeout=3)
            event["forwardConnectNanos"] = time.monotonic_ns() - start
            connection.settimeout(8)
            if case == "partial-tls-record":
                connection.sendall(b"\x16\x03\x03\x00\x20\x01")
            else:
                negative = case in ("wrong-hostname-rejected", "wrong-ca-rejected")
                client_context = wrong_ca_context if case == "wrong-ca-rejected" else tls_context
                hostname = "wrong.invalid" if case == "wrong-hostname-rejected" else "localhost"
                event["serverHostname"] = hostname
                event["trustSource"] = "default client trust roots" if case == "wrong-ca-rejected" else "only owned public PEM"
                try:
                    connection = client_context.wrap_socket(connection, server_hostname=hostname, suppress_ragged_eofs=False)
                except ssl.SSLCertVerificationError as error:
                    if not negative:
                        raise
                    event["rejectionType"] = "SSLCertVerificationError"
                    event["verifyCode"] = error.verify_code
                    event["verifyMessage"] = error.verify_message
                    event["status"] = "PASS"
                    return event
                if negative:
                    raise RuntimeError("invalid certificate/hostname unexpectedly passed actual TLS verification")
                event["tlsVersion"] = connection.version()
                event["cipher"] = connection.cipher()
                event["peerDerSha256"] = digest(connection.getpeercert(binary_form=True))
                if connection.version() != "TLSv1.2" or connection.getpeercert(binary_form=True) != expected_der:
                    raise RuntimeError("TLS protocol or exact fixture certificate mismatch")
                if case == "tls-abrupt-close":
                    event["status"] = "PASS"
                    return event
                request = f"GET /fc/tls-probe/{marker} HTTP/1.1\r\nHost: localhost\r\n\r\n".encode("ascii")
                if case == "fragmented-https-response":
                    for chunk in (request[:7], request[7:-3], request[-3:-1], request[-1:]):
                        connection.sendall(chunk)
                        time.sleep(0.015)
                else:
                    # A strict prefix, not an unknown header that the narrow
                    # diagnostic could reject immediately for another reason.
                    connection.sendall(request[:-1])
            received = bytearray()
            close_kind = None
            deadline = time.monotonic() + 8
            while time.monotonic() < deadline:
                connection.settimeout(max(0.01, deadline - time.monotonic()))
                try:
                    part = connection.recv(4096)
                except (ConnectionResetError, ConnectionAbortedError, ssl.SSLEOFError) as error:
                    close_kind = type(error).__name__
                    break
                if not part:
                    close_kind = "EOF"
                    break
                received.extend(part)
                if len(received) > MAX_RESPONSE_BYTES:
                    raise RuntimeError("TLS diagnostic response exceeded bound")
                if case == "fragmented-https-response" and b"\r\n\r\n" in received:
                    head, body = bytes(received).split(b"\r\n\r\n", 1)
                    if len(body) >= len(marker):
                        if body != marker.encode("ascii") or not head.startswith(b"HTTP/1.1 200 "):
                            raise RuntimeError("TLS diagnostic exact status/body mismatch")
                        lengths = re.findall(br"(?im)^Content-Length:\s*([0-9]+)\s*$", head)
                        if lengths != [str(len(marker)).encode("ascii")]:
                            raise RuntimeError("TLS diagnostic exact response length missing")
                        event["responseSha256"] = digest(bytes(received))
                        event["responseBytes"] = len(received)
                        event["status"] = "PASS"
                        return event
            if case == "fragmented-https-response":
                raise RuntimeError("TLS diagnostic response incomplete")
            if close_kind is None:
                raise RuntimeError("partial TLS/header connection not closed in diagnostic observation window")
            if received:
                raise RuntimeError("incomplete TLS/header unexpectedly produced application bytes")
            event["closeKind"] = close_kind
            event["status"] = "PASS"
        except Exception as error:
            event["error"] = failure(error, f"client/{case}")
        finally:
            if connection:
                connection.close()
            event["forwardObservationNanos"] = time.monotonic_ns() - start
        return event

    cases = (("fragmented-https-response", "partial-http-header", "partial-tls-record", "tls-abrupt-close") if cycle == 1
             else ("fragmented-https-response", "wrong-hostname-rejected", "wrong-ca-rejected", "partial-tls-record"))
    with concurrent.futures.ThreadPoolExecutor(max_workers=4, thread_name_prefix="fc-tls-owned-client") as pool:
        events = list(pool.map(client, cases))
    return events


def validate_native(value):
    if (value.get("cycles") != 2 or value.get("accepted") != 8 or type(value.get("handshakes")) is not int
            or value["handshakes"] < 4 or value.get("responses") != 2 or value.get("cleanupComplete") is not True
            or value.get("strictP0") != "NOT_VALIDATED"):
        raise RuntimeError("native TLS lifecycle final scope/count/cleanup contract failed")
    if type(value.get("ioSubmissions")) is not int or value["ioSubmissions"] <= 0 or value.get("ioCompletions") != value["ioSubmissions"]:
        raise RuntimeError("native TLS IOCP operation ledger mismatch")
    if (type(value.get("sspiSubmissions")) is not int or value["sspiSubmissions"] <= 0
            or value.get("sspiCompletions") != value["sspiSubmissions"]
            or value.get("returnedFrames") != value["sspiSubmissions"] or value.get("slotFramesAtDrain") != 8):
        raise RuntimeError("native TLS SSPI/frame ownership ledger mismatch")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ssh-target", default="future-classroom-win11")
    parser.add_argument("--identity", type=Path)
    parser.add_argument("--exe", type=Path, required=True)
    parser.add_argument("--report", type=Path, required=True)
    args = parser.parse_args()
    if not re.fullmatch(r"[A-Za-z0-9_.@:-]+", args.ssh_target) or args.ssh_target.startswith("-"):
        parser.error("ssh-target must be an SSH alias or user@host, without whitespace/options")
    if not args.exe.is_file() or args.exe.suffix.lower() != ".exe" or (args.identity and not args.identity.is_file()):
        parser.error("exe and optional private identity must exist")
    executable = args.exe.read_bytes()
    pe = int.from_bytes(executable[60:64], "little") if len(executable) >= 64 else 0
    if executable[:2] != b"MZ" or executable[pe:pe + 6] != b"PE\0\0\x4c\x01":
        parser.error("supplied executable must be a real Windows x86 PE")
    token = uuid.uuid4().hex
    marker = "FutureClassroom-Test-TLS-" + token
    report = {"schemaVersion": 1, "status": "FAIL", "scope": "Explicit finite-batch Schannel/IOCP lifecycle; Python TLS1.2 via owned SSH loopback forward", "strictP0": "NOT_VALIDATED", "productionListener": "NOT_ENABLED", "win7": "NOT_VALIDATED", "win10": "NOT_VALIDATED", "forwardTimingIsNativeDeadlineEvidence": False, "marker": marker, "exeSha256": digest(executable), "runnerSourceSha256": digest(Path(__file__).read_bytes()), "remoteScriptsSha256": digest((COMMON + SETUP + LAUNCH + STOP + CAPTURE + CLEANUP).encode("utf-8")), "events": [], "cleanup": "NOT_RUN"}
    remote = Remote(args.ssh_target, args.identity)
    directory = None
    launcher = None
    launcher_logs = None
    tunnel = None
    local_owner = None
    uploaded_scripts = set()
    phase = "create-owned-root"
    try:
        directory = remote.powershell("$root=Join-Path $env:TEMP " + ps_literal(marker) + ";New-Item -ItemType Directory -Path $root|Out-Null;[IO.File]::WriteAllText((Join-Path $root '.tls-probe-root')," + ps_literal(marker) + ");$root")
        report["remoteRoot"] = directory
        local_owner = tempfile.TemporaryDirectory(prefix="fc-tls-probe-")
        phase = "upload-owned-control-scripts"
        report["uploadedScriptSha256"] = {}
        for name, template in (("cleanup", CLEANUP), ("stop", STOP), ("capture", CAPTURE), ("setup", SETUP)):
            source = Path(local_owner.name) / ("tls-" + name + ".ps1")
            source.write_text(COMMON + script(template, directory, marker, args.exe.name), encoding="utf-8-sig")
            report["uploadedScriptSha256"][name] = digest(source.read_bytes())
            remote.upload(source, directory)
            uploaded_scripts.add(name)
        parsed = remote.powershell("$files=Get-ChildItem -LiteralPath " + ps_literal(directory) + " -Filter 'tls-*.ps1';foreach($file in $files){$tokens=$null;$errors=$null;[Management.Automation.Language.Parser]::ParseFile($file.FullName,[ref]$tokens,[ref]$errors)|Out-Null;if($errors.Count-ne 0){throw (($errors|ForEach-Object{$_.Message})-join '; ')}};'TLS_CONTROL_SCRIPTS_PARSE_PASS'", timeout=15)
        if parsed != "TLS_CONTROL_SCRIPTS_PARSE_PASS":
            raise RuntimeError("owned TLS control scripts parse result missing")
        report["controlScriptsParse"] = "PASS"
        phase = "create-owned-certificate-and-cng-key"
        fixture = json.loads(remote.run_file(directory + "\\tls-setup.ps1", timeout=45))
        report["fixture"] = {key: value for key, value in fixture.items() if key != "publicPem"}
        launch_source = Path(local_owner.name) / "tls-launch.ps1"
        launch_source.write_text(COMMON + script(LAUNCH, directory, marker, args.exe.name, fixture["thumbprint"]), encoding="utf-8-sig")
        report["uploadedScriptSha256"]["launch"] = digest(launch_source.read_bytes())
        remote.upload(launch_source, directory)
        phase = "upload-and-verify-owned-executable"
        remote.upload(args.exe, directory)
        remote_digest = remote.powershell("(Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path " + ps_literal(directory) + " " + ps_literal(args.exe.name) + ")).Hash.ToLowerInvariant()")
        if remote_digest != report["exeSha256"] or digest(args.exe.read_bytes()) != report["exeSha256"]:
            raise RuntimeError("supplied/uploaded executable changed after identity freeze")
        # Keep the control socket's directory alive until finally closes its
        # actual SSH master, even when a client or supervisor fails midway.
        if local_owner:
            local = local_owner.name
            pem = Path(local) / "tls-public.pem"
            pem.write_text(fixture["publicPem"], encoding="ascii")
            expected_der = ssl.PEM_cert_to_DER_cert(fixture["publicPem"])
            tls_context = context(pem)
            wrong_ca_context = untrusted_context(expected_der)
            report["clientVerification"] = {"checkHostname": tls_context.check_hostname, "verifyMode": "CERT_REQUIRED", "minTls": "TLSv1.2", "maxTls": "TLSv1.2", "publicCertificateSha256": digest(expected_der), "trustSource": "only owned public PEM; no system trust mutation", "wrongCaNegative": "PENDING/actual certificate rejection required", "wrongHostnameNegative": "PENDING/actual certificate rejection required", "wrongCaTrustSource": "default client trust roots; isolated fixture not added", "closeNotify": "NOT_VALIDATED; exact HTTP Content-Length completion is checked independently", "openssl": ssl.OPENSSL_VERSION}
            phase = "start-owned-ssh-master"
            tunnel = Tunnel(remote, local)
            phase = "start-native-tls-diagnostic"
            launcher = subprocess.Popen(remote.command_file(directory + "\\tls-launch.ps1"), stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            launcher_logs = Logs(launcher)
            deadline = time.monotonic() + 100
            ready = []
            native = []
            exits = []
            report["ready"] = ready
            report["nativeReports"] = native
            report["nativeProcessExits"] = exits
            while time.monotonic() < deadline:
                try:
                    line = launcher_logs.lines.get(timeout=0.1)
                except queue.Empty:
                    if launcher.poll() is not None and not any(t.is_alive() for t in launcher_logs.threads):
                        break
                    continue
                if line.startswith(READY):
                    phase = "ready-forward-and-four-clients"
                    item = json.loads(line[len(READY):])
                    expected_cycle = len(ready) + 1
                    if (type(item.get("port")) is not int or not 1 <= item["port"] <= 65535
                            or item.get("marker") != marker or item.get("cycle") != expected_cycle or expected_cycle > 2
                            or type(item.get("generation")) is not int or item["generation"] <= 0
                            or (ready and item["generation"] <= ready[-1]["generation"])):
                        raise RuntimeError("native TLS READY identity/port/cycle/generation invalid")
                    ready.append(item)
                    local_port = tunnel.open(item["port"])
                    report["events"].extend(exercise_batch(local_port, marker, expected_cycle, tls_context, wrong_ca_context, expected_der))
                    tunnel.cancel()
                elif line.startswith(PASS):
                    native.append(json.loads(line[len(PASS):]))
                elif line.startswith(EXIT):
                    exits.append(json.loads(line[len(EXIT):]))
                else:
                    report.setdefault("otherNativeLines", []).append(line)
            phase = "actual-native-and-ssh-exit"
            launcher.wait(timeout=max(0.01, deadline - time.monotonic()))
            launcher_logs.join()
            if launcher.returncode != 0 or len(exits) != 1 or exits[0].get("code") != 0:
                raise RuntimeError("native TLS child or its SSH supervisor did not exit successfully")
            if len(ready) != 2 or len(native) != 1 or len(report["events"]) != 8 or any(item["status"] != "PASS" for item in report["events"]):
                raise RuntimeError("two TLS batches/four client cases/native summary incomplete")
            validate_native(native[0])
            report["ready"] = ready
            report["native"] = native[0]
            report["nativeProcessExit"] = exits[0]
            report["status"] = "PASS"
    except Exception as error:
        report["error"] = failure(error, phase)
    finally:
        if launcher:
            if launcher.poll() is None:
                launcher.terminate()
                try:
                    launcher.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    launcher.kill()
                    launcher.wait(timeout=5)
            if launcher_logs:
                try:
                    launcher_logs.join()
                except Exception as error:
                    report["launcherDrainError"] = failure(error, "drain-owned-ssh-pipes")
                    report["status"] = "FAIL"
                report["sshLauncherRaw"] = launcher_logs.report()
        if tunnel:
            try:
                tunnel.close()
                report["ownedTunnelCleanup"] = "PASS"
            except Exception as error:
                report["ownedTunnelCleanup"] = "FAIL"
                report["tunnelCleanupError"] = failure(error, "stop-owned-ssh-tunnel")
                report["status"] = "FAIL"
            report["sshTunnelRaw"] = tunnel.logs.report()
        if local_owner:
            try:
                local_owner.cleanup()
            except Exception as error:
                report["localCleanupError"] = failure(error, "cleanup-owned-local-cert-and-control-socket-directory")
                report["status"] = "FAIL"
        if directory:
            stopped = False
            try:
                if "stop" in uploaded_scripts:
                    report["ownedProcessCleanup"] = json.loads(remote.run_file(directory + "\\tls-stop.ps1", timeout=25))
                    stopped = report["ownedProcessCleanup"].get("status") == "PASS"
                else:
                    # No fixture or launcher runs until every control script
                    # has uploaded, so this early failure has no owned process.
                    stopped = launcher is None
            except Exception as error:
                report["processCleanupError"] = failure(error, "stop-owned-native-and-supervisor-processes")
                report["status"] = "FAIL"
            # No original native/fixture files exist before setup is uploaded
            # and invoked. Every later path requires verified full artifacts.
            raw_preserved = "setup" not in uploaded_scripts and launcher is None
            try:
                if "capture" in uploaded_scripts:
                    if not stopped:
                        raise RuntimeError("physical process stop unproven; refusing to capture mutable original logs")
                    metadata = json.loads(remote.run_file(directory + "\\tls-capture.ps1", timeout=30))
                    report["remoteRawEvidence"] = metadata
                    preserve_raw(remote, directory, args.report, token, metadata, report)
                    raw_preserved = True
            except Exception as error:
                report["remoteCaptureError"] = failure(error, "preserve-complete-owned-native-original-artifacts")
                report["status"] = "FAIL"
            try:
                if not stopped:
                    raise RuntimeError("physical owned-process cleanup unproven; preserving certificate/key/root evidence")
                if not raw_preserved:
                    raise RuntimeError("complete original artifact preservation unproven; retaining remote root for evidence recovery")
                if "cleanup" in uploaded_scripts:
                    report["cleanupDetails"] = json.loads(remote.run_file(directory + "\\tls-cleanup.ps1", timeout=30))
                else:
                    outcome = remote.powershell("$root=" + ps_literal(directory) + ";if(([IO.File]::ReadAllText((Join-Path $root '.tls-probe-root')))-ne " + ps_literal(marker) + "){throw 'TLS early cleanup marker mismatch'};Remove-Item -LiteralPath $root -Recurse -Force;if(Test-Path -LiteralPath $root){throw 'TLS early root remains'};'TLS_EARLY_CLEANUP_PASS'", timeout=15)
                    report["cleanupDetails"] = {"status": "PASS" if outcome == "TLS_EARLY_CLEANUP_PASS" else "FAIL", "scope": "pre-fixture upload failure; no certificate/key/process created"}
                report["cleanup"] = report["cleanupDetails"]["status"]
            except Exception as error:
                report["cleanup"] = "FAIL"
                report["cleanupError"] = failure(error, "cleanup-owned-process-certificate-key-root")
                report["retainedRemoteEvidence"] = {"root": directory, "processStopProven": stopped, "completeLocalArtifactsProven": raw_preserved, "cleanupIncomplete": True, "nextAction": "recover and verify complete original artifacts, then run this root's exact tls-cleanup.ps1; preserve SSH authorization"}
                report["status"] = "FAIL"
        report["clientEventsSha256"] = digest(json.dumps(report["events"], sort_keys=True, separators=(",", ":")).encode("utf-8"))
        if "clientVerification" in report:
            for name, case in (("wrongCaNegative", "wrong-ca-rejected"), ("wrongHostnameNegative", "wrong-hostname-rejected")):
                matches = [event for event in report["events"] if event["case"] == case]
                report["clientVerification"][name] = ({"status": "PASS", "cycle": matches[0]["cycle"], "rejectionType": matches[0]["rejectionType"], "verifyCode": matches[0]["verifyCode"], "verifyMessage": matches[0]["verifyMessage"]}
                    if len(matches) == 1 and matches[0]["status"] == "PASS" and matches[0].get("rejectionType") == "SSLCertVerificationError"
                    else {"status": "FAIL" if matches else "NOT_RUN", "evidence": matches})
        report["remoteControlRunsRaw"] = remote.script_runs
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    if report["status"] != "PASS" or report["cleanup"] != "PASS":
        raise SystemExit("TLS lifecycle diagnostic failed; inspect " + str(args.report))
    print("TLS_LIFECYCLE_RUNNER_PASS " + json.dumps({"report": str(args.report), "exeSha256": report["exeSha256"], "cleanup": report["cleanup"], "strictP0": "NOT_VALIDATED"}, separators=(",", ":")))


if __name__ == "__main__":
    main()
