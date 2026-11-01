#!/usr/bin/env python3
"""One authorized Win11 headless Edge profile, held by an exclusive kernel Job.

run --spki SHA256_BASE64 --report fresh.json prints CONTROL then READY, and keeps
the SAME SSH supervisor alive until stop --state <control.json> or its 180s cap.
No browser PID/name sweep, certificate creation, trust-store edits, microphone,
visible UI, foreground action or system/SSH configuration change is performed.

Promoted from the actually exercised windows-edge-cache-owner.py prototype
(SHA256 fa6a06d8d81869151cef54f8d12e7b0d9cea8749fcd8f17b7eb8a96533daf2e0).
The native C#, supervisor, capture and cleanup blocks remain byte-for-byte.
STOP now publishes only a fully closed temporary file with a non-overwriting
same-directory move; matching concurrent requests remain idempotent. The installed
Edge SHA/version pin remains deliberate: an Edge update requires a new reviewed
freeze and actual Windows evidence, rather than silently accepting a new binary.
"""
import argparse
import ast
import base64
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import queue
import re
import subprocess
import sys
import tempfile
import time
import uuid

sys.dont_write_bytecode = True
EDGE = r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe"
EDGE_SHA = "d35d48b11f0439eeeddbbff0bf54856e922f5e1506a2f3098b5a808fcec4068b"
EDGE_VERSION = "156.0.4314.58"
TESTED_PROTOTYPE_SHA256 = "fa6a06d8d81869151cef54f8d12e7b0d9cea8749fcd8f17b7eb8a96533daf2e0"
RAW_NAMES = ("edge.stdout", "edge.stderr", ".edge-process.json", ".edge-assigned.json",
             ".edge-supervisor.json", ".edge-ready.json", "edge-devtools.raw", ".edge-outcome.json")
LOG_LIMIT = 512 * 1024

# The Job handle is noninheritable; only the three explicit standard handles
# enter CreateProcess. Suspension closes the child-spawn/assignment race.
CSHARP = r'''
using System;
using System.ComponentModel;
using System.Diagnostics;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;
public sealed class FcEdgeJob {
  [StructLayout(LayoutKind.Sequential)] struct SA { public int Size; public IntPtr Descriptor; [MarshalAs(UnmanagedType.Bool)] public bool Inherit; }
  [StructLayout(LayoutKind.Sequential)] struct SI {
    public int Size; public string Reserved, Desktop, Title;
    public uint X,Y,XSize,YSize,XChars,YChars,Fill,Flags; public ushort Show,Reserved2;
    public IntPtr ReservedBytes,Input,Output,Error;
  }
  [StructLayout(LayoutKind.Sequential)] struct SIX { public SI Start; public IntPtr Attributes; }
  [StructLayout(LayoutKind.Sequential)] struct PI { public IntPtr Process,Thread; public uint Pid,Tid; }
  [StructLayout(LayoutKind.Sequential)] struct BasicLimit {
    public long ProcessTime,JobTime; public uint Flags; public UIntPtr MinWorking,MaxWorking;
    public uint ActiveLimit; public UIntPtr Affinity; public uint Priority,Scheduling;
  }
  [StructLayout(LayoutKind.Sequential)] struct Io { public ulong A,B,C,D,E,F; }
  [StructLayout(LayoutKind.Sequential)] struct Extended {
    public BasicLimit Basic; public Io Io; public UIntPtr ProcessMemory,JobMemory,PeakProcess,PeakJob;
  }
  [StructLayout(LayoutKind.Sequential)] struct Accounting {
    public long User,Kernel,PeriodUser,PeriodKernel; public uint Faults,Total,Active,Terminated;
  }
  [StructLayout(LayoutKind.Sequential)] struct FT { public uint Low,High; public long Value { get { return ((long)High<<32)|Low; } } }
  [DllImport("kernel32.dll",EntryPoint="SetLastError")] static extern void ClearLastError(uint error);
  [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern IntPtr CreateJobObjectW(IntPtr attributes,string name);
  [DllImport("kernel32.dll",SetLastError=true)] static extern bool SetInformationJobObject(IntPtr job,int kind,IntPtr info,uint size);
  [DllImport("kernel32.dll",SetLastError=true)] static extern bool QueryInformationJobObject(IntPtr job,int kind,out Accounting info,uint size,IntPtr returned);
  [DllImport("kernel32.dll",SetLastError=true)] static extern bool AssignProcessToJobObject(IntPtr job,IntPtr process);
  [DllImport("kernel32.dll",SetLastError=true)] static extern IntPtr OpenProcess(uint access,bool inherit,uint pid);
  [DllImport("kernel32.dll",SetLastError=true)] static extern bool IsProcessInJob(IntPtr process,IntPtr job,out bool member);
  [DllImport("kernel32.dll",SetLastError=true)] static extern bool TerminateJobObject(IntPtr job,uint code);
  [DllImport("kernel32.dll",SetLastError=true)] static extern bool TerminateProcess(IntPtr process,uint code);
  [DllImport("kernel32.dll",SetLastError=true)] static extern bool GetProcessTimes(IntPtr process,out FT created,out FT exited,out FT kernel,out FT user);
  [DllImport("kernel32.dll",SetLastError=true)] static extern bool GetExitCodeProcess(IntPtr process,out uint code);
  [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool QueryFullProcessImageNameW(IntPtr process,uint flags,StringBuilder image,ref uint size);
  [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool CreateProcessW(string exe,StringBuilder command,IntPtr pa,IntPtr ta,bool inherit,uint flags,IntPtr environment,string directory,ref SIX startup,out PI process);
  [DllImport("kernel32.dll",SetLastError=true)] static extern bool InitializeProcThreadAttributeList(IntPtr list,int count,int flags,ref IntPtr size);
  [DllImport("kernel32.dll",SetLastError=true)] static extern bool UpdateProcThreadAttribute(IntPtr list,uint flags,IntPtr attribute,IntPtr value,IntPtr size,IntPtr oldValue,IntPtr returned);
  [DllImport("kernel32.dll")] static extern void DeleteProcThreadAttributeList(IntPtr list);
  [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern IntPtr CreateFileW(string path,uint access,uint share,ref SA sa,uint creation,uint flags,IntPtr template);
  [DllImport("kernel32.dll",SetLastError=true)] static extern uint ResumeThread(IntPtr thread);
  [DllImport("kernel32.dll",SetLastError=true)] static extern uint WaitForSingleObject(IntPtr handle,uint milliseconds);
  [DllImport("kernel32.dll",SetLastError=true)] static extern bool CloseHandle(IntPtr handle);
  IntPtr job,process,thread,output,error,input,attributes,handles;
  bool attributesReady;
  public bool Assigned { get; private set; }
  public uint ProcessId { get; private set; }
  public long StartTicks { get; private set; }
  public string ImagePath { get; private set; }
  public bool PrimaryExited { get; private set; }
  public uint PrimaryExitCode { get; private set; }
  public static bool FailureCleanupComplete { get; private set; }
  public static bool AttemptCreatedProcess { get; private set; }
  static readonly UTF8Encoding Utf8 = new UTF8Encoding(false);
  static void Check(bool ok,string stage) { if(!ok) throw new Win32Exception(Marshal.GetLastWin32Error(),stage); }
  static void ValidHandle(IntPtr value,string stage) { Check(value!=IntPtr.Zero && value!=new IntPtr(-1),stage); }
  static string Json(string value) {
    StringBuilder result=new StringBuilder("\"");
    foreach(char c in value) { if(c=='"'||c=='\\') result.Append('\\').Append(c); else if(c<32) result.Append("\\u").Append(((int)c).ToString("x4")); else result.Append(c); }
    return result.Append('"').ToString();
  }
  static string Quote(string value) {
    StringBuilder result=new StringBuilder("\""); int slashes=0;
    foreach(char c in value) {
      if(c=='\\') { slashes++; continue; }
      if(c=='"') result.Append('\\',slashes*2+1).Append('"');
      else result.Append('\\',slashes).Append(c);
      slashes=0;
    }
    return result.Append('\\',slashes*2).Append('"').ToString();
  }
  static void NewText(string path,string text) {
    using(FileStream file=new FileStream(path,FileMode.CreateNew,FileAccess.Write,FileShare.Read)) {
      byte[] bytes=Utf8.GetBytes(text); file.Write(bytes,0,bytes.Length);
    }
  }
  public FcEdgeJob(string exe,string[] args,string root,string profile,string marker,string expectedSha,string jobName) {
    FailureCleanupComplete=false; AttemptCreatedProcess=false;
    try {
      ClearLastError(0); job=CreateJobObjectW(IntPtr.Zero,jobName); int jobError=Marshal.GetLastWin32Error(); ValidHandle(job,"Create exclusive Job");
      if(jobError==183) { CloseHandle(job); job=IntPtr.Zero; throw new InvalidOperationException("Job name already exists; refusing adoption"); }
      Extended limits=new Extended(); limits.Basic.Flags=0x2008; limits.Basic.ActiveLimit=64;
      IntPtr native=Marshal.AllocHGlobal(Marshal.SizeOf(typeof(Extended)));
      try { Marshal.StructureToPtr(limits,native,false); Check(SetInformationJobObject(job,9,native,(uint)Marshal.SizeOf(typeof(Extended))),"Set no-breakaway/kill-on-close/64process Job limits"); }
      finally { Marshal.FreeHGlobal(native); }
      SA sa=new SA(); sa.Size=Marshal.SizeOf(typeof(SA)); sa.Inherit=true;
      output=CreateFileW(Path.Combine(root,"edge.stdout"),0x40000000,1,ref sa,1,0x80,IntPtr.Zero); ValidHandle(output,"Create exclusive stdout");
      error=CreateFileW(Path.Combine(root,"edge.stderr"),0x40000000,1,ref sa,1,0x80,IntPtr.Zero); ValidHandle(error,"Create exclusive stderr");
      input=CreateFileW("NUL",0x80000000,3,ref sa,3,0x80,IntPtr.Zero); ValidHandle(input,"Open NUL stdin");
      IntPtr size=IntPtr.Zero; InitializeProcThreadAttributeList(IntPtr.Zero,1,0,ref size);
      Check(size!=IntPtr.Zero,"Size handle inheritance list");
      attributes=Marshal.AllocHGlobal(size); Check(InitializeProcThreadAttributeList(attributes,1,0,ref size),"Initialize handle inheritance list"); attributesReady=true;
      handles=Marshal.AllocHGlobal(IntPtr.Size*3); Marshal.WriteIntPtr(handles,0,input); Marshal.WriteIntPtr(handles,IntPtr.Size,output); Marshal.WriteIntPtr(handles,IntPtr.Size*2,error);
      Check(UpdateProcThreadAttribute(attributes,0,new IntPtr(0x20002),handles,new IntPtr(IntPtr.Size*3),IntPtr.Zero,IntPtr.Zero),"Restrict inherited handles to stdio only");
      SIX startup=new SIX(); startup.Start.Size=Marshal.SizeOf(typeof(SIX)); startup.Start.Flags=0x101; startup.Start.Show=0;
      startup.Start.Input=input; startup.Start.Output=output; startup.Start.Error=error; startup.Attributes=attributes;
      StringBuilder command=new StringBuilder(Quote(exe)); foreach(string arg in args) command.Append(' ').Append(Quote(arg));
      PI created; Check(CreateProcessW(exe,command,IntPtr.Zero,IntPtr.Zero,true,0x08080004,IntPtr.Zero,root,ref startup,out created),"Create exact Edge suspended/hidden");
      process=created.Process; thread=created.Thread; ProcessId=created.Pid; AttemptCreatedProcess=true;
      FT born,ended,kernel,user; Check(GetProcessTimes(process,out born,out ended,out kernel,out user),"Query retained process creation time"); StartTicks=DateTime.FromFileTimeUtc(born.Value).Ticks;
      StringBuilder image=new StringBuilder(32768); uint length=32768; Check(QueryFullProcessImageNameW(process,0,image,ref length),"Query retained process image"); ImagePath=image.ToString();
      if(!String.Equals(ImagePath,exe,StringComparison.OrdinalIgnoreCase)) throw new InvalidOperationException("Suspended process image mismatch");
      NewText(Path.Combine(root,".edge-process.json"),"{\"marker\":"+Json(marker)+",\"profile\":"+Json(profile)+",\"pid\":"+ProcessId+",\"startTicks\":"+Json(StartTicks.ToString())+",\"exe\":"+Json(ImagePath)+",\"exeSha256\":"+Json(expectedSha)+",\"jobName\":"+Json(jobName)+",\"createdSuspended\":true}");
      Check(AssignProcessToJobObject(job,process),"Assign original suspended Edge to exclusive Job"); Assigned=true;
      NewText(Path.Combine(root,".edge-assigned.json"),"{\"marker\":"+Json(marker)+",\"pid\":"+ProcessId+",\"startTicks\":"+Json(StartTicks.ToString())+",\"assignedBeforeResume\":true,\"breakawayAllowed\":false,\"killOnJobClose\":true}");
    } catch {
      try { FailureCleanupComplete=StopAndWait(20000); } catch { FailureCleanupComplete=false; }
      bool closed=false; try { closed=Close(); } finally { FailureCleanupComplete=FailureCleanupComplete && closed; }
      throw;
    }
  }
  public uint ActiveProcesses() {
    if(job==IntPtr.Zero) return 0;
    Accounting info; Check(QueryInformationJobObject(job,1,out info,(uint)Marshal.SizeOf(typeof(Accounting)),IntPtr.Zero),"Query original Job accounting"); return info.Active;
  }
  public bool ContainsProcess(uint pid) {
    IntPtr candidate=OpenProcess(0x1000,false,pid); ValidHandle(candidate,"Open DevTools listener only for exact Job membership");
    try { bool member; Check(IsProcessInJob(candidate,job,out member),"Query DevTools listener membership in original Job"); return member; }
    finally { Check(CloseHandle(candidate),"Close listener query handle"); }
  }
  public void Resume() { Check(Assigned,"Original process not Job-bound"); Check(ResumeThread(thread)!=0xffffffff,"Resume original assigned process"); }
  public bool StopAndWait(int milliseconds) {
    Stopwatch watch=Stopwatch.StartNew();
    if(Assigned) { if(ActiveProcesses()!=0) Check(TerminateJobObject(job,1),"Terminate only original exclusive Job"); }
    else if(process!=IntPtr.Zero && WaitForSingleObject(process,0)==258) Check(TerminateProcess(process,1),"Terminate original unresumed process handle");
    while(Assigned && ActiveProcesses()!=0) { if(watch.ElapsedMilliseconds>=milliseconds) return false; Thread.Sleep(25); }
    if(process!=IntPtr.Zero) {
      int remaining=Math.Max(0,milliseconds-(int)watch.ElapsedMilliseconds);
      if(WaitForSingleObject(process,(uint)remaining)!=0) return false;
      uint code; Check(GetExitCodeProcess(process,out code),"Get actual original process exit"); PrimaryExited=true; PrimaryExitCode=code;
    }
    return true;
  }
  bool CloseOne(ref IntPtr handle) { IntPtr saved=handle; handle=IntPtr.Zero; return saved==IntPtr.Zero || saved==new IntPtr(-1) || CloseHandle(saved); }
  public bool Close() {
    bool ok=true;
    if(attributesReady) { DeleteProcThreadAttributeList(attributes); attributesReady=false; }
    if(attributes!=IntPtr.Zero) { Marshal.FreeHGlobal(attributes); attributes=IntPtr.Zero; }
    if(handles!=IntPtr.Zero) { Marshal.FreeHGlobal(handles); handles=IntPtr.Zero; }
    ok=CloseOne(ref thread)&&ok; ok=CloseOne(ref process)&&ok;
    ok=CloseOne(ref input)&&ok; ok=CloseOne(ref output)&&ok; ok=CloseOne(ref error)&&ok;
    // Last close has KILL_ON_JOB_CLOSE even on abrupt supervisor process exit.
    ok=CloseOne(ref job)&&ok; return ok;
  }
}
'''

COMMON = r'''
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false);$OutputEncoding=[Console]::OutputEncoding
function Assert([bool]$Condition,[string]$Message){if(!$Condition){throw $Message}}
function New-Text([string]$Path,[string]$Text){
  $file=[IO.File]::Open($Path,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::Read)
  try{$bytes=[Text.UTF8Encoding]::new($false).GetBytes($Text);$file.Write($bytes,0,$bytes.Length)}finally{$file.Dispose()}
}
function New-Json([string]$Path,$Value){New-Text $Path ($Value|ConvertTo-Json -Depth 8 -Compress)}
function Assert-Root([string]$Root,[string]$Marker,[string]$OriginalTemp){
  Assert ($Marker-match '^FutureClassroom-Test-Browser-[0-9a-f]{32}$') 'browser marker shape mismatch'
  Assert ([String]::Equals([IO.Path]::GetFullPath($Root),[IO.Path]::GetFullPath((Join-Path $OriginalTemp $Marker)),[StringComparison]::OrdinalIgnoreCase)) 'browser root outside original TEMP'
  Assert (([IO.File]::ReadAllText((Join-Path $Root '.browser-marker')))-ceq $Marker) 'browser marker mismatch'
  Assert (((Get-Item -LiteralPath $Root -Force).Attributes-band [IO.FileAttributes]::ReparsePoint)-eq 0) 'browser root is reparse point'
}
'''
SUPERVISE = r'''
$root=__ROOT__;$marker=__MARKER__;$capability=__CAPABILITY__;$spki=__SPKI__;$exe=__EDGE__;$expectedSha=__EDGE_SHA__
$originalTemp=$env:TEMP;$originalTmp=$env:TMP;Assert-Root $root $marker $originalTemp
$owner=$null;$compiled=$false;$stopped=$false;$primaryExited=$false;$handlesClosed=$false;$ready=$false;$failure=$null;$reason='startup-failed'
$watch=[Diagnostics.Stopwatch]::StartNew()
$supervisor=Get-Process -Id $PID
New-Json (Join-Path $root '.edge-supervisor.json') @{marker=$marker;pid=$PID;startTicks=$supervisor.StartTime.ToUniversalTime().Ticks.ToString();exe=$supervisor.Path}
$supervisor.Dispose()
try{
  $manifest=[IO.File]::ReadAllText((Join-Path $root '.browser-owner.json'))|ConvertFrom-Json
  $sha=[Security.Cryptography.SHA256]::Create()
  try{$capHash=([BitConverter]::ToString($sha.ComputeHash([Text.Encoding]::ASCII.GetBytes($capability)))).Replace('-','').ToLowerInvariant()}finally{$sha.Dispose()}
  Assert ($manifest.marker-eq $marker-and $manifest.capabilitySha256-eq $capHash-and $manifest.exe-eq $exe) 'browser stop capability/owner mismatch'
  Assert ((Get-FileHash -Algorithm SHA256 -LiteralPath $exe).Hash.ToLowerInvariant()-ceq $expectedSha) 'frozen installed Edge executable SHA changed'
  $versionInfo=(Get-Item -LiteralPath $exe).VersionInfo
  $version=$versionInfo.ProductVersion
  Assert ($version-eq __EDGE_VERSION__) 'frozen installed Edge executable version changed'
  $profile=Join-Path $root 'profile'
  Assert (!(Test-Path -LiteralPath $profile)) 'browser profile already exists; refusing reuse'
  New-Item -ItemType Directory -Path $profile|Out-Null
  New-Item -ItemType Directory -Path (Join-Path $root 'temp')|Out-Null
  # Compile and child runtime temporary files stay under this owned root.
  $env:TEMP=Join-Path $root 'temp';$env:TMP=$env:TEMP
  Add-Type -TypeDefinition @'
__CSHARP__
'@
  $compiled=$true
  $arguments=[string[]]@('--headless=new',('--user-data-dir='+$profile),'--remote-debugging-address=127.0.0.1','--remote-debugging-port=0',('--ignore-certificate-errors-spki-list='+$spki),'--no-first-run','--no-default-browser-check','--disable-background-networking','--disable-sync','--disable-extensions','--mute-audio','about:blank')
  $owner=[FcEdgeJob]::new($exe,$arguments,$root,$profile,$marker,$expectedSha,$manifest.jobName)
  Assert ($owner.Assigned-and [String]::Equals($owner.ImagePath,$exe,[StringComparison]::OrdinalIgnoreCase)) 'original Edge handle/image not assigned'
  $owner.Resume()
  $activePort=Join-Path $profile 'DevToolsActivePort'
  while($watch.ElapsedMilliseconds-lt 180000){
    Assert ($owner.ActiveProcesses()-le 64) 'exclusive Edge Job process bound exceeded'
    if($owner.ActiveProcesses()-eq 0){throw 'exclusive Edge Job exited before stop request'}
    foreach($name in @('edge.stdout','edge.stderr')){Assert ((Get-Item -LiteralPath (Join-Path $root $name)).Length-le 524288) 'Edge original log diagnostic limit exceeded'}
    if(!$ready-and (Test-Path -LiteralPath $activePort)){
      $bytes=$null
      try{$bytes=[IO.File]::ReadAllBytes($activePort)}catch [IO.IOException]{}
      if($bytes){
        Assert ($bytes.Length-le 4096) 'DevToolsActivePort original exceeded bound'
        $text=[Text.UTF8Encoding]::new($false,$true).GetString($bytes)
        $lines=@($text.TrimEnd([char]10,[char]13).Split([char]10)|ForEach-Object{$_.TrimEnd([char]13)})
        if($lines.Count-eq 2-and $lines[0]-match '^[1-9][0-9]{0,4}$'-and $lines[1]-match '^/devtools/browser/[0-9a-fA-F-]{16,80}$'){
          $port=[int]$lines[0];Assert ($port-le 65535) 'DevTools loopback port invalid'
          $listeners=@(Get-NetTCPConnection -State Listen -LocalPort $port -ErrorAction Stop)
          Assert ($listeners.Count-gt 0) 'DevTools listener missing'
          foreach($listener in $listeners){Assert ($listener.LocalAddress-in @('127.0.0.1','::1')) 'DevTools escaped loopback';Assert ($owner.ContainsProcess([uint32]$listener.OwningProcess)) 'DevTools listener belongs to a foreign kernel Job'}
          $raw=[IO.File]::Open((Join-Path $root 'edge-devtools.raw'),[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::Read)
          try{$raw.Write($bytes,0,$bytes.Length)}finally{$raw.Dispose()}
          $entry=@{marker=$marker;port=$port;webSocketPath=$lines[1];pid=$owner.ProcessId;startTicks=$owner.StartTicks.ToString();exe=$owner.ImagePath;profile=$profile;jobName=$manifest.jobName;headless=$true;muteAudio=$true;productVersion=$versionInfo.ProductVersion;fileVersion=$versionInfo.FileVersion;boundLoopback=$true;readyOriginalBytes=$bytes.Length;readyOriginalSha256=(Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $root 'edge-devtools.raw')).Hash.ToLowerInvariant()}
          New-Json (Join-Path $root '.edge-ready.json') $entry
          [Console]::WriteLine('EDGE_CACHE_READY '+($entry|ConvertTo-Json -Depth 5 -Compress));[Console]::Out.Flush();$ready=$true
        }
      }
    }
    $stop=Join-Path $root '.request-stop'
    if(Test-Path -LiteralPath $stop){Assert (([IO.File]::ReadAllText($stop))-ceq $capability) 'browser stop capability file mismatch';$reason='capability-stop';break}
    Start-Sleep -Milliseconds 100
  }
  if($reason-ne 'capability-stop'){$reason='180s-supervisor-limit'}
}catch{$failure=$_.Exception.Message}
finally{
  if($owner){
    try{$stopped=$owner.StopAndWait(20000);$primaryExited=$owner.PrimaryExited;$exitCode=$owner.PrimaryExitCode}
    catch{$failure=([string]$failure)+'; actual original Job stop failed: '+$_.Exception.Message}
    try{$handlesClosed=$owner.Close()}catch{$failure=([string]$failure)+'; original handles close failed: '+$_.Exception.Message}
  }elseif($compiled){$stopped=[FcEdgeJob]::FailureCleanupComplete;$primaryExited=(-not [FcEdgeJob]::AttemptCreatedProcess) -or $stopped;$handlesClosed=$stopped}
  else{$stopped=$true;$primaryExited=$true;$handlesClosed=$true}
  $outcome=@{marker=$marker;ready=$ready;reason=$reason;failure=$failure;actualJobProcessesZero=$stopped;originalPrimaryExited=$primaryExited;originalHandlesClosed=$handlesClosed;originalPrimaryExitCode=$exitCode;elapsedMillis=$watch.ElapsedMilliseconds;jobBoundBeforeResume=$([bool]$owner);maximumJobProcesses=64;stopRequestOnly=$true;existingBrowserProcesses='NOT_TOUCHED'}
  New-Json (Join-Path $root '.edge-outcome.json') $outcome
  [Console]::WriteLine('EDGE_CACHE_STOPPED '+($outcome|ConvertTo-Json -Depth 5 -Compress));[Console]::Out.Flush()
  $env:TEMP=$originalTemp;$env:TMP=$originalTmp
}
if($failure-or !$ready-or !$stopped-or !$primaryExited-or !$handlesClosed){exit 1}
'''
STOP = r'''
function Publish-StopRequest([string]$Root,[string]$Capability){
  $path=Join-Path $Root '.request-stop'
  if([IO.File]::Exists($path)){
    Assert (([IO.File]::ReadAllText($path))-ceq $Capability) 'different stop capability already present'
    return
  }
  $pending=Join-Path $Root ('.request-stop.pending-'+[Guid]::NewGuid().ToString('N'))
  $owned=$false
  try{
    $file=$null
    try{
      $file=[IO.File]::Open($pending,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::Read)
      $owned=$true
      $bytes=[Text.UTF8Encoding]::new($false).GetBytes($Capability)
      $file.Write($bytes,0,$bytes.Length)
    }finally{if($file){$file.Dispose()}}
    # The final name is not visible until the complete original file is closed.
    # File.Move is non-overwriting and both names are in the same owned directory.
    try{[IO.File]::Move($pending,$path);$owned=$false}
    catch{
      $cause=$_.Exception.GetBaseException();$code=$cause.HResult-band 65535
      if(($cause-is [IO.IOException])-and ($code-in @(80,183))-and [IO.File]::Exists($path)){
        Assert (([IO.File]::ReadAllText($path))-ceq $Capability) 'different stop capability won publication'
      }else{throw}
    }
  }finally{
    # A failed CreateNew never grants ownership of an existing candidate.
    if($owned-and [IO.File]::Exists($pending)){[IO.File]::Delete($pending)}
  }
}
$root=__ROOT__;$marker=__MARKER__;$capability=__CAPABILITY__
if(!(Test-Path -LiteralPath $root)){@{status='ROOT_ALREADY_ABSENT';physicalExit='CHECK_FINAL_RUN_REPORT'}|ConvertTo-Json -Compress;exit 0}
Assert-Root $root $marker $env:TEMP
$entry=[IO.File]::ReadAllText((Join-Path $root '.browser-owner.json'))|ConvertFrom-Json
$sha=[Security.Cryptography.SHA256]::Create()
try{$hash=([BitConverter]::ToString($sha.ComputeHash([Text.Encoding]::ASCII.GetBytes($capability)))).Replace('-','').ToLowerInvariant()}finally{$sha.Dispose()}
Assert ($entry.marker-eq $marker-and $entry.capabilitySha256-ceq $hash-and $entry.exe-ceq __EDGE__) 'stop ownership/capability mismatch'
$path=Join-Path $root '.request-stop'
Publish-StopRequest $root $capability
@{status='STOP_REQUESTED';marker=$marker;physicalExit='WAIT_FOR_ORIGINAL_SUPERVISOR_REPORT'}|ConvertTo-Json -Compress
'''
CAPTURE = r'''
$root=__ROOT__;$marker=__MARKER__
Assert-Root $root $marker $env:TEMP
$result=@{}
foreach($name in @('edge.stdout','edge.stderr','.edge-process.json','.edge-assigned.json','.edge-supervisor.json','.edge-ready.json','edge-devtools.raw','.edge-outcome.json')){
  $path=Join-Path $root $name
  if(Test-Path -LiteralPath $path){
    $file=[IO.File]::Open($path,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::Read)
    $sha=[Security.Cryptography.SHA256]::Create()
    try{$length=$file.Length;$hash=([BitConverter]::ToString($sha.ComputeHash($file))).Replace('-','').ToLowerInvariant();Assert ($file.Length-eq $length) 'Edge original raw file changed during capture';$result[$name]=@{bytes=$length;sha256=$hash}}
    finally{$sha.Dispose();$file.Dispose()}
  }
}
$result|ConvertTo-Json -Depth 4 -Compress
'''
CLEANUP = r'''
$root=__ROOT__;$marker=__MARKER__
Assert-Root $root $marker $env:TEMP
$outcome=[IO.File]::ReadAllText((Join-Path $root '.edge-outcome.json'))|ConvertFrom-Json
Assert ($outcome.marker-eq $marker-and $outcome.actualJobProcessesZero-eq $true-and $outcome.originalPrimaryExited-eq $true-and $outcome.originalHandlesClosed-eq $true) 'original Job actual zero/primary exit/handles close unproven; retain profile'
foreach($item in @(Get-ChildItem -LiteralPath $root -Recurse -Force)){Assert (($item.Attributes-band [IO.FileAttributes]::ReparsePoint)-eq 0) 'refuse browser root deletion through reparse point'}
Remove-Item -LiteralPath $root -Recurse -Force
Assert (!(Test-Path -LiteralPath $root)) 'owned browser profile/root remains'
@{status='PASS';rootAbsent=$true;actualJobProcessesZero=$true;originalPrimaryExited=$true;existingBrowsers='UNTOUCHED';sshConfiguration='UNTOUCHED';systemVolume='UNTOUCHED';rootTrustStore='UNTOUCHED'}|ConvertTo-Json -Compress
'''


def digest(data):
    return hashlib.sha256(data).hexdigest()


def require(value, message):
    if not value:
        raise RuntimeError(message)


def load_helper(_root):
    # Keep --project-root accepted for prototype CLI compatibility, but never
    # import executable Python from the caller cwd or a supplied package root.
    source = Path(__file__).resolve().with_name("windows-tls-probe.py")
    require(source.is_file(), "sibling windows-tls-probe.py helper missing")
    spec = importlib.util.spec_from_file_location("fc_edge_owned_tls_helpers", source)
    require(spec is not None and spec.loader is not None, "cannot load sibling TLS helpers")
    helper = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(helper)
    return helper


def audited_remote(helper, target, identity):
    class AuditedRemote(helper.Remote):
        def powershell(self, script, timeout=60):
            preamble = ("$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';"
                        "[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false);$OutputEncoding=[Console]::OutputEncoding;")
            raw = (preamble + script).encode("utf-16le")
            encoded = base64.b64encode(raw).decode("ascii")
            require(len(encoded) < 7500, "bounded SSH encoded control command too long; use owned script file")
            command = ["ssh", *self.options, self.host,
                       "powershell.exe -NoProfile -NonInteractive -EncodedCommand " + encoded]
            entry = {"kind": "inline-owner-control", "scriptUtf16Sha256": digest(raw)}
            try:
                result = subprocess.run(command, capture_output=True, timeout=timeout)
                entry["exitCode"] = result.returncode
                streams = (("stdout", result.stdout), ("stderr", result.stderr))
            except subprocess.TimeoutExpired as error:
                streams = (("stdout", error.stdout or b""), ("stderr", error.stderr or b""))
                entry["status"] = "TIMEOUT"
                for name, data in streams:
                    entry[name] = {"bytes": len(data), "sha256": digest(data), "base64": base64.b64encode(data).decode("ascii")}
                self.script_runs.append(entry)
                raise
            for name, data in streams:
                entry[name] = {"bytes": len(data), "sha256": digest(data), "base64": base64.b64encode(data).decode("ascii")}
            self.script_runs.append(entry)
            require(result.returncode == 0, "owned inline browser control failed: " + result.stderr.decode("utf-8", "replace")[-3000:])
            return result.stdout.decode("utf-8", "strict").strip().lstrip("\ufeff")
    return AuditedRemote(target, identity)


def render(helper, template, state):
    values = {"__ROOT__": state["remoteRoot"], "__MARKER__": state["marker"], "__CAPABILITY__": state["stopCapability"],
              "__SPKI__": state.get("spki", ""), "__EDGE__": EDGE, "__EDGE_SHA__": EDGE_SHA, "__EDGE_VERSION__": EDGE_VERSION}
    for key, value in values.items():
        template = template.replace(key, helper.ps_literal(value))
    return COMMON + template.replace("__CSHARP__", CSHARP)


def new_json(path, value, private=False):
    path.parent.mkdir(parents=True, exist_ok=True)
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600 if private else 0o644)
    with os.fdopen(descriptor, "w", encoding="utf-8") as file:
        json.dump(value, file, ensure_ascii=False, indent=2)
        file.write("\n")


def validate_state(state):
    require(isinstance(state, dict) and state.get("schemaVersion") == 1
            and isinstance(state.get("marker"), str) and re.fullmatch(r"FutureClassroom-Test-Browser-[0-9a-f]{32}", state["marker"])
            and isinstance(state.get("stopCapability"), str) and re.fullmatch(r"[0-9a-f]{64}", state["stopCapability"])
            and state.get("edgeExe") == EDGE and state.get("edgeSha256") == EDGE_SHA
            and isinstance(state.get("remoteRoot"), str) and "\n" not in state["remoteRoot"]
            and isinstance(state.get("sshTarget"), str) and re.fullmatch(r"[A-Za-z0-9_.@:-]+", state["sshTarget"])
            and not state["sshTarget"].startswith("-"), "malformed local browser stop capability")


def preserve(helper, remote, state, report_path, metadata, report):
    require(isinstance(metadata, dict) and set(metadata) <= set(RAW_NAMES), "unexpected browser raw metadata names")
    destination = report_path.parent / (report_path.stem + "-" + state["marker"] + "-raw")
    destination.mkdir(parents=True, exist_ok=False)
    report["rawPreservation"] = {"status": "INCOMPLETE", "directory": str(destination.resolve()), "files": {}}
    for name, expected in metadata.items():
        require(isinstance(expected, dict) and type(expected.get("bytes")) is int and expected["bytes"] >= 0
                and isinstance(expected.get("sha256"), str) and re.fullmatch(r"[0-9a-f]{64}", expected["sha256"]), "invalid browser raw identity")
        partial, final = destination / (name + ".partial"), destination / name
        remote.download(state["remoteRoot"] + "\\" + name, partial)
        actual = helper.file_identity(partial)
        require(actual == expected, "complete original browser raw byte/SHA mismatch: " + name)
        partial.rename(final)
        record = {"status": "PASS", "bytes": actual["bytes"], "sha256": actual["sha256"], "artifact": str(final.resolve())}
        if actual["bytes"] <= LOG_LIMIT:
            raw = final.read_bytes()
            record["base64"] = base64.b64encode(raw).decode("ascii")
            require(base64.b64decode(record["base64"], validate=True) == raw, "browser original base64 mismatch")
        else:
            record["inline"] = "OMITTED: complete original preserved in verified file"
        report["rawPreservation"]["files"][name] = record
    report["rawPreservation"]["status"] = "PASS"
    return destination


def early_cleanup(helper, remote, state, report_path):
    # Called only before any Popen attempt. Every native-created path must be
    # absent; unlike normal cleanup this makes no physical-exit claim.
    root = helper.ps_literal(state["remoteRoot"])
    marker = helper.ps_literal(state["marker"])
    expected = helper.ps_literal(digest(state["stopCapability"].encode("ascii")))
    script = ("$root=" + root + ";$marker=" + marker + ";"
              + "if(![String]::Equals([IO.Path]::GetFullPath($root),[IO.Path]::GetFullPath((Join-Path $env:TEMP $marker)),[StringComparison]::OrdinalIgnoreCase)){throw 'prelaunch root mismatch'};"
              + "if(([IO.File]::ReadAllText((Join-Path $root '.browser-marker')))-cne $marker){throw 'prelaunch marker mismatch'};"
              + "$entry=[IO.File]::ReadAllText((Join-Path $root '.browser-owner.json'))|ConvertFrom-Json;"
              + "if($entry.marker-cne $marker-or $entry.capabilitySha256-cne " + expected + "){throw 'prelaunch capability mismatch'};"
              + "$allowed=@('.browser-marker','.browser-owner.json','edge-supervise.ps1','edge-stop.ps1','edge-capture.ps1','edge-cleanup.ps1');$originals=@{};"
              + "foreach($file in @(Get-ChildItem -LiteralPath $root -Force)){if($file.PSIsContainer-or $file.Name-notin $allowed-or ($file.Attributes-band [IO.FileAttributes]::ReparsePoint)-ne 0){throw 'native-created/foreign entry prevents prelaunch cleanup'};"
              + "$bytes=[IO.File]::ReadAllBytes($file.FullName);$originals[$file.Name]=@{bytes=$bytes.Length;sha256=(Get-FileHash -Algorithm SHA256 -LiteralPath $file.FullName).Hash.ToLowerInvariant();base64=[Convert]::ToBase64String($bytes)}};"
              + "if(((Get-Item -LiteralPath $root -Force).Attributes-band [IO.FileAttributes]::ReparsePoint)-ne 0){throw 'prelaunch root reparse'};"
              )
    raw = json.loads(remote.powershell(script + "$originals|ConvertTo-Json -Depth 4 -Compress", timeout=30))
    allowed = {".browser-marker", ".browser-owner.json", "edge-supervise.ps1", "edge-stop.ps1", "edge-capture.ps1", "edge-cleanup.ps1"}
    require(isinstance(raw, dict) and set(raw) <= allowed, "unexpected prelaunch original files")
    destination = report_path.parent / (report_path.stem + "-" + state["marker"] + "-prelaunch-raw")
    destination.mkdir(parents=True, exist_ok=False)
    identities = {}
    for name, item in raw.items():
        data = base64.b64decode(item["base64"], validate=True)
        require(type(item["bytes"]) is int and len(data) == item["bytes"] and digest(data) == item["sha256"], "prelaunch original byte/SHA mismatch")
        (destination / name).write_bytes(data)
        identities[name] = {"bytes": len(data), "sha256": digest(data)}
    # Original bytes are now verified and retained locally BEFORE deletion.
    summary = "\n".join(name + ":" + str(identities[name]["bytes"]) + ":" + identities[name]["sha256"] for name in sorted(identities))
    expected = helper.ps_literal(digest(summary.encode("utf-8")))
    finish = ("$rows=@($originals.Keys|Sort-Object|ForEach-Object{$_+':'+$originals[$_].bytes+':'+$originals[$_].sha256});"
              + "$sha=[Security.Cryptography.SHA256]::Create();try{$hash=([BitConverter]::ToString($sha.ComputeHash([Text.Encoding]::UTF8.GetBytes(($rows-join [char]10))))).Replace('-','').ToLowerInvariant()}finally{$sha.Dispose()};"
              + "if($hash-cne " + expected + "){throw 'prelaunch originals changed'};"
              + "Remove-Item -LiteralPath $root -Recurse -Force;if(Test-Path -LiteralPath $root){throw 'prelaunch root remains'};"
              + "@{status='PASS';mode='BEFORE_ANY_NATIVE_LAUNCH_ATTEMPT';nativeCreatedEntriesAbsent=$true;rootAbsent=$true}|ConvertTo-Json -Compress")
    result = json.loads(remote.powershell(script + finish, timeout=30))
    result["originalRawDirectory"] = str(destination.resolve())
    result["originalFiles"] = identities
    return result


def request_stop(helper, remote, state):
    # Keep the SSH command short; the uploaded full script performs exact TEMP,
    # root marker and capability checks. Validate the caller capability first.
    root = helper.ps_literal(state["remoteRoot"])
    marker = helper.ps_literal(state["marker"])
    expected = helper.ps_literal(digest(state["stopCapability"].encode("ascii")))
    script = ("$root=" + root + ";if(!(Test-Path -LiteralPath $root)){'ROOT_ALREADY_ABSENT';exit 0};"
              + "$entry=[IO.File]::ReadAllText((Join-Path $root '.browser-owner.json'))|ConvertFrom-Json;"
              + "if($entry.marker-cne " + marker + "-or $entry.capabilitySha256-cne " + expected
              + "){throw 'caller stop capability mismatch'};& powershell.exe -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File (Join-Path $root 'edge-stop.ps1');exit $LASTEXITCODE")
    require(len(script.encode("utf-16le")) * 4 // 3 < 7000, "stop dispatch exceeded short-command bound")
    return remote.powershell(script, timeout=20)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="mode", required=True)
    run = sub.add_parser("run")
    run.add_argument("--spki", required=True)
    run.add_argument("--report", type=Path, required=True)
    run.add_argument("--ssh-target", default="future-classroom-win11")
    run.add_argument("--identity", type=Path)
    run.add_argument("--project-root", type=Path, default=Path.cwd(), help="legacy compatibility; helpers always load from this script directory")
    stop = sub.add_parser("stop")
    stop.add_argument("--state", type=Path, required=True)
    stop.add_argument("--project-root", type=Path, default=Path.cwd(), help="legacy compatibility; helpers always load from this script directory")
    check = sub.add_parser("self-test")
    check.add_argument("--project-root", type=Path, default=Path.cwd(), help="legacy compatibility; helpers always load from this script directory")
    args = parser.parse_args()
    helper = load_helper(args.project_root)
    if args.mode == "self-test":
        ast.parse(Path(__file__).read_text())
        for flag in ("--headless=new", "--user-data-dir=", "--remote-debugging-address=127.0.0.1", "--remote-debugging-port=0", "--ignore-certificate-errors-spki-list=", "--mute-audio"):
            require(flag in SUPERVISE, "required headless isolation flag missing")
        require("0x2008" in CSHARP and "0x08080004" in CSHARP and "AssignProcessToJobObject(job,process)" in CSHARP,
                "no-breakaway kill-on-close suspended assignment contract missing")
        require("StopAndWait(20000)" in SUPERVISE and "actualJobProcessesZero" in CLEANUP, "actual zero cleanup contract missing")
        require("Stop-Process" not in STOP and "Get-Process" not in STOP, "stop CLI may not sweep/kill PIDs")
        require("[IO.File]::Move($pending,$path)" in STOP and "[IO.FileMode]::CreateNew" in STOP
                and STOP.index("$file.Dispose()") < STOP.index("[IO.File]::Move($pending,$path)")
                and "$code-in @(80,183)" in STOP and "if($owned-and [IO.File]::Exists($pending))" in STOP
                and "Publish-StopRequest $root $capability" in STOP and "New-Text $path $capability" not in STOP,
                "closed non-overwriting owned stop publication contract missing")
        print(json.dumps({"status": "PURE_STATIC_CHECKS_PASS", "windows": "NOT_RUN", "csharpCompile": "NOT_RUN", "sourceSha256": digest(Path(__file__).read_bytes()), "testedPrototypeSha256": TESTED_PROTOTYPE_SHA256}))
        return
    if args.mode == "stop":
        state = json.loads(args.state.read_text())
        validate_state(state)
        remote = audited_remote(helper, state["sshTarget"], Path(state["identity"]) if state.get("identity") else None)
        # A file contains the opaque capability, never a PID/name kill command.
        result = request_stop(helper, remote, state)
        evidence = args.state.with_name(args.state.name + ".stop-" + uuid.uuid4().hex + ".json")
        new_json(evidence, {"schemaVersion": 1, "marker": state["marker"], "operation": "CAPABILITY_REQUEST_ONLY", "physicalExit": "SEE_ORIGINAL_RUN_REPORT", "remoteControlRunsRaw": remote.script_runs})
        print("EDGE_CACHE_STOP_REQUEST " + json.dumps({"result": result, "rawReport": str(evidence.resolve())}))
        return
    try:
        public_spki = base64.b64decode(args.spki, validate=True)
    except ValueError:
        parser.error("--spki must be canonical base64 SHA256 of the fixture's PUBLIC SPKI")
    require(len(public_spki) == 32 and base64.b64encode(public_spki).decode("ascii") == args.spki, "SPKI must be one canonical SHA256 base64 value")
    require(not args.report.exists(), "report already exists; preserve original evidence")
    require(re.fullmatch(r"[A-Za-z0-9_.@:-]+", args.ssh_target) and not args.ssh_target.startswith("-"), "invalid SSH target")
    require(args.identity is None or args.identity.is_file(), "optional SSH private identity missing")
    marker = "FutureClassroom-Test-Browser-" + uuid.uuid4().hex
    capability = uuid.uuid4().hex + uuid.uuid4().hex
    state_file = args.report.with_name(args.report.name + ".control.json")
    require(not state_file.exists(), "local stop capability already exists; refuse overwrite")
    state = {"schemaVersion": 1, "marker": marker, "stopCapability": capability, "spki": args.spki,
             "sshTarget": args.ssh_target, "identity": str(args.identity.resolve()) if args.identity else None,
             "edgeExe": EDGE, "edgeSha256": EDGE_SHA}
    report = {"schemaVersion": 1, "status": "FAIL", "cleanup": "NOT_RUN", "marker": marker,
              "scope": "Authorized isolated Win11 headless Edge cache browser; actual browser assertions belong to separate CDP harness",
              "sourceSha256": digest(Path(__file__).read_bytes()), "nativeHelperSha256": digest(CSHARP.encode()),
              "testedPrototypeSha256": TESTED_PROTOTYPE_SHA256,
              "promotionScope": "C# and supervisor/capture/cleanup unchanged; STOP publishes closed temporary files by non-overwriting same-directory move",
              "edgeExe": EDGE, "edgeExpectedSha256": EDGE_SHA, "edgeExpectedVersion": EDGE_VERSION,
              "spkiSha256Base64": args.spki, "profile": "NEW_OWNED_ONLY", "deadlineSeconds": 180,
              "nativeStopGraceSeconds": 20, "muteAudio": True, "existingBrowsers": "UNTOUCHED",
              "microphone": "NOT_REQUESTED", "rootTrustStore": "UNTOUCHED", "sshConfiguration": "UNTOUCHED",
              "cacheAssertions": "NOT_RUN_BY_THIS_OWNER", "uploadedScriptSha256": {}}
    remote = audited_remote(helper, args.ssh_target, args.identity)
    local = None
    launcher = logs = None
    root_created = False
    launch_attempted = False
    phase = "create-owned-browser-root"
    try:
        owner = {"marker": marker, "capabilitySha256": digest(capability.encode("ascii")), "exe": EDGE,
                 "jobName": "Local\\" + marker + "-" + uuid.uuid4().hex, "spkiSha256Base64": args.spki}
        root = remote.powershell("$marker=" + helper.ps_literal(marker)
            + ";$root=Join-Path $env:TEMP $marker;New-Item -ItemType Directory -Path $root|Out-Null;[IO.File]::WriteAllText((Join-Path $root '.browser-marker'),$marker);"
            + "[IO.File]::WriteAllText((Join-Path $root '.browser-owner.json'),"
            + helper.ps_literal(json.dumps(owner, separators=(",", ":"))) + ",[Text.UTF8Encoding]::new($false));$root")
        root_created = True
        state["remoteRoot"] = root
        report["remoteRoot"] = root
        validate_state(state)
        new_json(state_file, state, private=True)
        report["controlFile"] = str(state_file.resolve())
        print("EDGE_CACHE_CONTROL " + json.dumps({"controlFile": str(state_file.resolve()), "marker": marker}), flush=True)
        local = tempfile.TemporaryDirectory(prefix="fc-edge-cache-owner-")
        for name, template in (("supervise", SUPERVISE), ("stop", STOP), ("capture", CAPTURE), ("cleanup", CLEANUP)):
            file = Path(local.name) / ("edge-" + name + ".ps1")
            file.write_text(render(helper, template, state), encoding="utf-8-sig")
            report["uploadedScriptSha256"][name] = digest(file.read_bytes())
            remote.upload(file, root)
        phase = "parse-owned-control-scripts"
        expected_scripts = "@{" + ";".join(helper.ps_literal("edge-" + name + ".ps1") + "=" + helper.ps_literal(sha) for name, sha in report["uploadedScriptSha256"].items()) + "}"
        parsed = remote.powershell("$expected=" + expected_scripts + ";foreach($file in Get-ChildItem -LiteralPath " + helper.ps_literal(root)
            + " -Filter 'edge-*.ps1'){if(!$expected.ContainsKey($file.Name)-or (Get-FileHash -Algorithm SHA256 -LiteralPath $file.FullName).Hash.ToLowerInvariant()-cne $expected[$file.Name]){throw 'uploaded control SHA mismatch'};$tokens=$null;$errors=$null;[Management.Automation.Language.Parser]::ParseFile($file.FullName,[ref]$tokens,[ref]$errors)|Out-Null;"
            + "if($errors.Count){throw (($errors|ForEach-Object{$_.Message})-join '; ')}};'EDGE_CONTROL_PARSE_PASS'", timeout=20)
        require(parsed == "EDGE_CONTROL_PARSE_PASS", "owned controls AST parse missing")
        report["controlScriptsParse"] = "PASS"
        phase = "start-original-job-supervisor"
        launch_attempted = True
        launcher = subprocess.Popen(remote.command_file(root + "\\edge-supervise.ps1"), stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        logs = helper.Logs(launcher)
        until = time.monotonic() + 215
        ready = outcomes = 0
        while time.monotonic() < until:
            try:
                line = logs.lines.get(timeout=0.1)
            except queue.Empty:
                if launcher.poll() is not None and not any(thread.is_alive() for thread in logs.threads):
                    break
                continue
            if line.startswith("EDGE_CACHE_READY "):
                item = json.loads(line[len("EDGE_CACHE_READY "):])
                require(ready == 0 and item.get("marker") == marker and type(item.get("port")) is int
                        and 1 <= item["port"] <= 65535 and re.fullmatch(r"/devtools/browser/[0-9a-fA-F-]{16,80}", item.get("webSocketPath", ""))
                        and item.get("exe") == EDGE and item.get("profile") == root + "\\profile" and item.get("boundLoopback") is True,
                        "actual isolated DevTools readiness malformed")
                ready += 1
                report["ready"] = item
                print("EDGE_CACHE_READY " + json.dumps({**item, "remoteRoot": root, "controlFile": str(state_file.resolve())}), flush=True)
            elif line.startswith("EDGE_CACHE_STOPPED "):
                outcomes += 1
                report["nativeOutcome"] = json.loads(line[len("EDGE_CACHE_STOPPED "):])
            else:
                report.setdefault("otherSupervisorLines", []).append(line)
        launcher.wait(timeout=max(0.01, until - time.monotonic()))
        logs.join()
        require(launcher.returncode == 0 and ready == 1 and outcomes == 1, "original Edge supervisor failed or readiness/exit ledger missing")
        outcome = report["nativeOutcome"]
        require(outcome.get("marker") == marker and outcome.get("actualJobProcessesZero") is True
                and outcome.get("originalPrimaryExited") is True and outcome.get("originalHandlesClosed") is True
                and outcome.get("failure") is None, "actual original kernel Job retirement unproven")
        report["status"] = "PASS"
    except Exception as error:
        report["error"] = helper.failure(error, phase)
    finally:
        # Request negative stop via the capability, never kill any browser PID.
        if root_created and launcher and launcher.poll() is None:
            try:
                report["finallyStopRequest"] = request_stop(helper, remote, state)
                launcher.wait(timeout=25)
            except Exception as error:
                report["supervisorStopError"] = helper.failure(error, "request-original-supervisor-stop")
                report["status"] = "FAIL"
                if launcher.poll() is None:
                    launcher.terminate()
                    try:
                        launcher.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        launcher.kill(); launcher.wait(timeout=5)
                # An abrupt supervisor exit closes the noninherited Job handle
                # with KILL_ON_JOB_CLOSE. Without an actual-zero outcome record
                # we retain the profile rather than pretend exit was observed.
        if logs:
            try:
                logs.join()
            except Exception as error:
                report["logDrainError"] = helper.failure(error, "original-ssh-output-drain")
                report["status"] = "FAIL"
            report["sshSupervisorRaw"] = logs.report()
        raw_saved = False
        if root_created and not launch_attempted:
            try:
                report["earlyCleanup"] = early_cleanup(helper, remote, state, args.report)
                require(report["earlyCleanup"].get("status") == "PASS", "prelaunch own root cleanup did not pass")
                report["cleanup"] = "PASS"
            except Exception as error:
                report["cleanup"] = "FAIL"
                report["cleanupError"] = helper.failure(error, "prelaunch-owned-root-cleanup")
                report["retainedRemoteEvidence"] = {"root": state["remoteRoot"], "controlFile": str(state_file.resolve()), "nativeLaunchAttempted": False}
        elif root_created:
            try:
                metadata = json.loads(remote.run_file(state["remoteRoot"] + "\\edge-capture.ps1", timeout=30))
                report["remoteRawEvidence"] = metadata
                directory = preserve(helper, remote, state, args.report, metadata, report)
                raw_saved = True
                if ".edge-outcome.json" in metadata:
                    outcome = json.loads((directory / ".edge-outcome.json").read_text(encoding="utf-8-sig"))
                    require(outcome.get("marker") == marker and outcome.get("actualJobProcessesZero") is True
                            and outcome.get("originalPrimaryExited") is True and outcome.get("originalHandlesClosed") is True,
                            "original kernel Job actual-zero ledger unavailable; retain owned profile")
                else:
                    require(launcher is None, "supervisor started without physical-exit ledger; retain profile")
                if launcher is not None:
                    require({".edge-supervisor.json", ".edge-outcome.json"} <= metadata.keys(), "original supervisor/exit journals incomplete")
                    if report.get("ready") is not None:
                        require(set(RAW_NAMES) <= metadata.keys(), "original browser logs/journals/readiness incomplete")
                report["cleanupDetails"] = json.loads(remote.run_file(state["remoteRoot"] + "\\edge-cleanup.ps1", timeout=30))
                require(report["cleanupDetails"].get("status") == "PASS", "owned profile/root cleanup did not pass")
                report["cleanup"] = "PASS"
            except Exception as error:
                report.update({"status": "FAIL", "cleanup": "FAIL", "cleanupError": helper.failure(error, "preserve-originals-then-clean-owned-profile"),
                    "retainedRemoteEvidence": {"root": state["remoteRoot"], "completeRawArtifactsProven": raw_saved,
                    "controlFile": str(state_file.resolve()),
                    "nextAction": "request only this supervisor via stop --state; require original actualJobProcessesZero/primary exit ledger before exact cleanup. If SSH died without ledger, recover/query exact named Job; never sweep browser PIDs. Preserve SSH authorization."}})
        if local:
            try:
                local.cleanup()
            except Exception as error:
                report["localCleanupError"] = helper.failure(error, "local-owned-control-files")
                report["status"] = "FAIL"
        report["remoteControlRunsRaw"] = remote.script_runs
        new_json(args.report, report)
    print("EDGE_CACHE_OWNER_RESULT " + json.dumps({"status": report["status"], "cleanup": report["cleanup"], "report": str(args.report),
          "cacheAssertions": "SEE_SEPARATE_CDP_HARNESS"}, separators=(",", ":")), flush=True)
    if report["status"] != "PASS" or report["cleanup"] != "PASS":
        raise SystemExit(1)


if __name__ == "__main__":
    main()
