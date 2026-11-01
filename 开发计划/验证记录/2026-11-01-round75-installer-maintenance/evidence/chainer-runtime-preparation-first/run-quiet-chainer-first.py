#!/usr/bin/env python3
"""Independent exact chainer quiet fixture. Root alone executes Windows.
One120s clock,20s cleanup reserve. No production Rust/gate/Pending/Run changes.
"""
from pathlib import Path
import argparse,base64,datetime,hashlib,json,re,subprocess,sys,time
sys.dont_write_bytecode=True
R=Path('/tmp/fc-round75-installer-maintenance');E=R/'evidence';D=E/'chainer-runtime-preparation-first'
HOST='elior@192.168.10.212';KEY='/config/.local/share/future-classroom-test-access/win11-192.168.10.212'
BOOTSTRAP="$ErrorActionPreference='Stop';[Console]::InputEncoding=[Text.UTF8Encoding]::new($false);[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false);$s=[Console]::In.ReadToEnd();& ([ScriptBlock]::Create($s))"
COMMON=r'''$ErrorActionPreference='Stop';[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
function Sid { $x=[Security.Principal.WindowsIdentity]::GetCurrent();try{return $x.User}finally{$x.Dispose()} }
function Fact($p){if(([IO.File]::GetAttributes($p)-band[IO.FileAttributes]::ReparsePoint)-ne 0){throw 'REPARSE_FILE'};return @{bytes=[Int64]([IO.FileInfo]::new($p).Length);sha256=(Get-FileHash -LiteralPath $p -Algorithm SHA256).Hash.ToLowerInvariant()} }
function Facts { $a=@();foreach($p in [Diagnostics.Process]::GetProcessesByName('msiexec')){try{$a+=@{pid=$p.Id;creationFiletime=[string]$p.StartTime.ToUniversalTime().ToFileTimeUtc()}}finally{$p.Dispose()}};return ,$a }
function Registration { $i=$null;$x=$null;$related=$null;try{$i=New-Object -ComObject WindowsInstaller.Installer;$x=$i.GetType().InvokeMember('ProductsEx',[Reflection.BindingFlags]::GetProperty,$null,$i,@($c.product,'',7));$n=$x.GetType().InvokeMember('Count',[Reflection.BindingFlags]::GetProperty,$null,$x,@());if($n -isnot [Int32] -or $n-lt 0){throw 'PRODUCTS_COUNT_TYPE'};$s=$i.GetType().InvokeMember('ProductState',[Reflection.BindingFlags]::GetProperty,$null,$i,@($c.product));$related=$i.GetType().InvokeMember('RelatedProducts',[Reflection.BindingFlags]::GetProperty,$null,$i,@($c.upgrade));$rn=$related.GetType().InvokeMember('Count',[Reflection.BindingFlags]::GetProperty,$null,$related,@());if($rn -isnot [Int32] -or $rn-lt 0){throw 'RELATED_COUNT_TYPE'};return @{status='ACTUAL_CALLER_PERUSER_AND_MACHINE';product=$c.product;upgrade=$c.upgrade;count=$n;productState=$s;relatedCount=$rn;contexts=7;sidArgument='';otherUsers='NOT_QUERIED'}}finally{foreach($o in @($related,$x,$i)){if($null-ne$o){[void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($o)}}} }
function ProcessState($x){if($null-eq$x){return 'UNKNOWN'};$p=$null;try{$p=[Diagnostics.Process]::GetProcessById([int]$x.pid);if(([string]$p.StartTime.ToUniversalTime().ToFileTimeUtc())-ceq([string]$x.creationFiletime)){return 'ORIGINAL_STILL_LIVE'};return 'PID_REUSED_ORIGINAL_GONE'}catch [ArgumentException]{return 'ABSENT'}catch{return 'UNKNOWN_QUERY_FAILED'}finally{if($null-ne$p){$p.Dispose()}} }
function CheckRoot { $sid=Sid;if($sid.Value-cne$c.sid){throw 'CURRENT_USER_SID'};if($c.root-cne(Join-Path ([IO.Path]::GetFullPath($env:TEMP)) ('FutureClassroom-r75-chainer-probe-'+$c.nonce))){throw 'EXACT_ROOT_PATH'};if(([IO.File]::GetAttributes($c.root)-band[IO.FileAttributes]::ReparsePoint)-ne 0){throw 'ROOT_REPARSE'};$a=[IO.Directory]::GetAccessControl($c.root);if($a.GetOwner([Security.Principal.SecurityIdentifier]).Value-cne$c.sid -or -not $a.AreAccessRulesProtected){throw 'EXACT_OWNER_PROTECTED_DACL'};$rules=@($a.GetAccessRules($true,$true,[Security.Principal.SecurityIdentifier]));if($rules.Count-ne 2 -or @($rules|Where-Object{$_.IdentityReference.Value-notin@($c.sid,'S-1-5-18') -or $_.AccessControlType-ne'Allow' -or $_.FileSystemRights-ne[Security.AccessControl.FileSystemRights]::FullControl}).Count-ne 0){throw 'ONLY_USER_SYSTEM_DACL'};if([IO.File]::ReadAllText((Join-Path $c.root '.owner'))-cne$c.marker){throw 'EXACT_MARKER'} }
function NewMsi($arguments){$p=[Diagnostics.Process]::new();$p.StartInfo.FileName=Join-Path $env:WINDIR 'System32\msiexec.exe';$p.StartInfo.Arguments=$arguments;$p.StartInfo.WorkingDirectory=$c.root;$p.StartInfo.UseShellExecute=$false;$p.StartInfo.CreateNoWindow=$true;return $p}
'''
CREATE=r'''if($PSVersionTable.PSVersion.Major-ne 5 -or $PSVersionTable.PSVersion.Minor-ne 1){throw 'DESKTOP51_REQUIRED'};$sid=Sid;if($sid.Value-cne$c.sid){throw 'EXPECTED_USER'}
if($c.root-cne(Join-Path ([IO.Path]::GetFullPath($env:TEMP)) ('FutureClassroom-r75-chainer-probe-'+$c.nonce)) -or (Test-Path -LiteralPath $c.root)){throw 'FRESH_EXACT_ROOT'}
$before=Registration;if($before.count-ne 0 -or $before.productState-ne -1 -or $before.relatedCount-ne 0){throw 'PRODUCT_OR_RELATED_ALREADY_PRESENT'};$baseline=Facts
[IO.Directory]::CreateDirectory($c.root)|Out-Null;$a=[Security.AccessControl.DirectorySecurity]::new();$a.SetOwner($sid);$a.SetAccessRuleProtection($true,$false);foreach($s in @($sid,[Security.Principal.SecurityIdentifier]::new('S-1-5-18'))){$a.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new($s,'FullControl','ContainerInherit,ObjectInherit','None','Allow'))};[IO.Directory]::SetAccessControl($c.root,$a)
[IO.Directory]::CreateDirectory((Join-Path $c.root 'runtime-temp'))|Out-Null
$p=Join-Path $c.root '.owner';$f=[IO.File]::Open($p,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::Read);try{$b=[Text.Encoding]::ASCII.GetBytes($c.marker);$f.Write($b,0,$b.Length);$f.Flush($true)}finally{$f.Dispose()};CheckRoot
@{status='CREATED_EXACT_OWNER_BEFORE_ABSENT';root=$c.root;nonce=$c.nonce;ownerSid=$sid.Value;before=$before;baseline=$baseline;rules='USER_AND_SYSTEM_ONLY';powershell=$PSVersionTable.PSVersion.ToString()}|ConvertTo-Json -Depth 8 -Compress
'''
NATIVE=r'''
using System; using System.Runtime.InteropServices; using System.Text;
public static class FcChainerRuntime {
 [DllImport("kernel32.dll",SetLastError=true)] public static extern IntPtr OpenProcess(uint rights,bool inherit,uint pid);
 [DllImport("kernel32.dll",SetLastError=true)] public static extern bool GetProcessTimes(IntPtr h,out long c,out long e,out long k,out long u);
 [DllImport("kernel32.dll",CharSet=CharSet.Unicode,SetLastError=true)] public static extern bool QueryFullProcessImageName(IntPtr h,uint flags,StringBuilder name,ref uint size);
 [DllImport("kernel32.dll",SetLastError=true)] public static extern bool GetExitCodeProcess(IntPtr h,out uint code);
 [DllImport("kernel32.dll",SetLastError=true)] public static extern uint WaitForSingleObject(IntPtr h,uint ms);
 [DllImport("kernel32.dll",SetLastError=true)] public static extern bool TerminateProcess(IntPtr h,uint code);
 [DllImport("kernel32.dll",SetLastError=true)] public static extern bool CloseHandle(IntPtr h);
}
'''
RUN=r'''CheckRoot;$msi=Join-Path $c.root 'probe.msi';if((Fact $msi).sha256-cne$c.sha){throw 'FINAL_MSI_SHA'}
$temp=Join-Path $c.root 'runtime-temp';$oldTmp=$env:TMP;$oldTemp=$env:TEMP;$env:TMP=$temp;$env:TEMP=$temp
try{Add-Type -TypeDefinition $c.native -Language CSharp -ErrorAction Stop}finally{$env:TMP=$oldTmp;$env:TEMP=$oldTemp}
function HandleIdentity($h){$cc=[Int64]0;$ee=[Int64]0;$kk=[Int64]0;$uu=[Int64]0;if(-not [FcChainerRuntime]::GetProcessTimes($h,[ref]$cc,[ref]$ee,[ref]$kk,[ref]$uu)){throw 'HELPER_TIMES'};$s=[Text.StringBuilder]::new(1024);$n=[uint32]1024;if(-not [FcChainerRuntime]::QueryFullProcessImageName($h,0,$s,[ref]$n)){throw 'HELPER_IMAGE'};return @{creationFiletime=[string]$cc;image=$s.ToString()} }
$p=NewMsi $c.installArgs;$capture=@{handle=[IntPtr]::Zero;attempted=$false;pre=$null;verified=$false};$killed=$false;$watch=[Diagnostics.Stopwatch]::StartNew()
$r=@{status='ACTUAL_NATIVE_AND_CLIENT_FACTS';nonce=$c.nonce;clientActualExit=$false;clientExitCode=$null;helperHandleCaptured=$false;helperExitEvidence='UNKNOWN_NOT_CAPTURED';helperExitCode=$null;negativeOwnerDeathAttempted=$false;helperHandleClose=$null}
function CaptureHelper {
 if($capture.attempted){return};$name=Join-Path $c.root 'pre-join.json';if(-not [IO.File]::Exists($name)){return};$capture.attempted=$true
 try{$raw=[IO.File]::ReadAllBytes($name);if($raw.Length-gt 8192){throw 'PREJOIN_BOUND'};$capture.pre=[Text.Encoding]::UTF8.GetString($raw)|ConvertFrom-Json;$pre=$capture.pre
 if($pre.scope-cne'PRE_JOIN_IDENTITY_ONLY_NOT_TRANSACTION_SUCCESS' -or $pre.nonce-cne$c.nonce -or $pre.argv.Count-ne 4 -or $pre.argv[1]-cne'probe' -or $pre.argv[2]-cne$c.nonce -or $pre.argv[3]-cne$c.root -or $pre.own.status-cne'OK'){throw 'PREJOIN_CLOSED_IDENTITY'}
 $pre=$capture.pre;$r.preJoinSha256=(Fact $name).sha256;$r.helperExpected=@{pid=[int]$pre.own.pid;creationFiletime=[string]$pre.own.creationFiletime;image=[string]$pre.own.image};$capture.handle=[FcChainerRuntime]::OpenProcess(0x101000,$false,[uint32]$pre.own.pid)
 if($capture.handle-eq[IntPtr]::Zero){$r.helperCaptureWin32=[Runtime.InteropServices.Marshal]::GetLastWin32Error();$r.helperOriginalInstance=ProcessState $r.helperExpected;return}
 $r.helperHandleCaptured=$true;$id=HandleIdentity $capture.handle;if($id.creationFiletime-cne$r.helperExpected.creationFiletime -or $id.image-cne$r.helperExpected.image){throw 'ORIGINAL_HELPER_INSTANCE_CHANGED'}
 $image=Fact $id.image;$r.helperActualImage=$image;if($image.sha256-cne$c.exeSha){throw 'HELPER_EXACT_COMPILED_IMAGE_SHA'};$capture.verified=$true;$r.helperExactInstanceAndBinary=$true
 }catch{$r.helperCaptureError=$_.Exception.GetType().FullName;$r.helperCaptureStageError=[string]$_}
}
try{if(-not $p.Start()){throw 'MSI_START_FALSE'};$r.client=@{pid=$p.Id;creationFiletime=[string]$p.StartTime.ToUniversalTime().ToFileTimeUtc()}
 while(-not $p.HasExited -and $watch.ElapsedMilliseconds-lt$c.runMs){CaptureHelper;[Threading.Thread]::Sleep(20)};CaptureHelper
 if(-not $p.HasExited -and $capture.verified -and $capture.handle-ne[IntPtr]::Zero -and [FcChainerRuntime]::WaitForSingleObject($capture.handle,0)-eq 258){
   $r.negativeOwnerDeathAttempted=$true;$kill=[FcChainerRuntime]::OpenProcess(0x101001,$false,[uint32]$r.helperExpected.pid)
   if($kill-ne[IntPtr]::Zero){try{$id=HandleIdentity $kill;if($id.creationFiletime-ceq$r.helperExpected.creationFiletime -and $id.image-ceq$r.helperExpected.image -and (Fact $id.image).sha256-ceq$c.exeSha){$killed=[FcChainerRuntime]::TerminateProcess($kill,1603);$r.negativeOwnerDeathNativeResult=$killed;if(-not $killed){$r.negativeOwnerDeathWin32=[Runtime.InteropServices.Marshal]::GetLastWin32Error()};[void][FcChainerRuntime]::WaitForSingleObject($capture.handle,3000)}}finally{$ok=[FcChainerRuntime]::CloseHandle($kill);$r.negativeTerminateHandleClose=@{ok=$ok;win32=$(if($ok){0}else{[Runtime.InteropServices.Marshal]::GetLastWin32Error()})}}}else{$r.negativeOwnerDeathOpenWin32=[Runtime.InteropServices.Marshal]::GetLastWin32Error()}
   [void]$p.WaitForExit(3000)
 }
 if($p.HasExited){$r.clientActualExit=$true;$r.clientExitCode=$p.ExitCode}else{$r.clientStillLive=$true}
 if($capture.handle-ne[IntPtr]::Zero -and $capture.verified){$wait=[FcChainerRuntime]::WaitForSingleObject($capture.handle,0);$r.helperWaitResult=$wait;if($wait-eq 0){$code=[uint32]0;if([FcChainerRuntime]::GetExitCodeProcess($capture.handle,[ref]$code)){$r.helperExitCode=$code;$r.helperExitEvidence='ACTUAL_HELD_PROCESS_GETEXITCODE';$r.helperOriginalInstance='HELD_ORIGINAL_EXITED'}else{$r.helperExitWin32=[Runtime.InteropServices.Marshal]::GetLastWin32Error()}}elseif($wait-eq 258){$r.helperOriginalInstance='ORIGINAL_STILL_LIVE'}}
}catch{$r.runError=[string]$_}finally{if($capture.handle-ne[IntPtr]::Zero){$ok=[FcChainerRuntime]::CloseHandle($capture.handle);$r.helperHandleClose=@{ok=$ok;win32=$(if($ok){0}else{[Runtime.InteropServices.Marshal]::GetLastWin32Error()})}};$r.elapsedMs=$watch.ElapsedMilliseconds;$p.Dispose()}
$r|ConvertTo-Json -Depth 10 -Compress
'''
SETTLE=r'''CheckRoot;$before=Registration;$uninstall=@{attempted=$false;actualExit=$false;exitCode=$null}
try{if($before.count-gt 0 -or $before.productState-ne -1){$uninstall.attempted=$true;$p=NewMsi $c.uninstallArgs;try{if(-not $p.Start()){throw 'UNINSTALL_START_FALSE'};$uninstall.client=@{pid=$p.Id;creationFiletime=[string]$p.StartTime.ToUniversalTime().ToFileTimeUtc()};if($p.WaitForExit([int]$c.settleMs)){$uninstall.actualExit=$true;$uninstall.exitCode=$p.ExitCode}else{$uninstall.stillLive=$true}}finally{$p.Dispose()}}}catch{$uninstall.error=[string]$_}finally{$after=Registration}
$new=@();foreach($x in (Facts)){if(-not @($c.baseline|Where-Object{$_.pid-eq$x.pid -and $_.creationFiletime-ceq$x.creationFiletime}).Count){$new+=$x}}
$helper=ProcessState $c.helper;$client=ProcessState $c.client
@{status='ACTUAL_FORMAL_X_FINALLY_AND_ABSENCE';nonce=$c.nonce;beforeFinally=$before;formalX=$uninstall;after=$after;helperOriginalInstance=$helper;clientOriginalInstance=$client;newMsiexec=$new;newMsiexecCount=$new.Count;baselineServiceNeverKilled=$true}|ConvertTo-Json -Depth 10 -Compress
'''
CAPTURE=r'''CheckRoot;$files=@();$directories=@();$payloads=@{};$queue=[Collections.Generic.Queue[string]]::new();$queue.Enqueue($c.root);$total=[Int64]0
while($queue.Count-gt 0){$dir=$queue.Dequeue();foreach($p in [IO.Directory]::GetFileSystemEntries($dir)){if(([IO.File]::GetAttributes($p)-band[IO.FileAttributes]::ReparsePoint)-ne 0){throw 'CAPTURE_REPARSE'};$rel=$p.Substring($c.root.Length+1).Replace('\','/');if(([IO.File]::GetAttributes($p)-band[IO.FileAttributes]::Directory)-ne 0){if($rel-ne'runtime-temp' -and -not $rel.StartsWith('runtime-temp/')){throw 'UNKNOWN_DIRECTORY'};$directories+=$rel;$queue.Enqueue($p)}else{if($rel-notin@('.owner','probe.msi','install.log','uninstall.log','pre-join.json','result.json','pre-join.tmp','result.tmp') -and -not $rel.StartsWith('runtime-temp/')){throw 'UNKNOWN_FILE'};if($rel-match'(?i)\.(key|pfx|p12)$'){throw 'PRIVATE_SUFFIX_NOT_READ'};$f=Fact $p;$total+=$f.bytes;if($files.Count-ge 128 -or $total-gt 16777216){throw 'CAPTURE_BOUND'};$files+=@{path=$rel;bytes=$f.bytes;sha256=$f.sha256};if($rel-in@('install.log','uninstall.log','pre-join.json','result.json','pre-join.tmp','result.tmp')){if($f.bytes-gt$(if($rel.EndsWith('.log')){2097152}else{8192})){throw 'RAW_LOG_JSON_BOUND'};$raw=[IO.File]::ReadAllBytes($p);$again=Fact $p;if($again.sha256-cne$f.sha256){throw 'CAPTURE_FILE_CHANGED'};$payloads[$rel]=[Convert]::ToBase64String($raw)}}}}
@{status='ACTUAL_OWNED_ORIGINAL_BYTES';nonce=$c.nonce;files=$files;directories=$directories;payloads=$payloads;derivedCompilerCache='Exact SHA metadata only, not product/source evidence'}|ConvertTo-Json -Depth 8 -Compress
'''
DELETE=r'''if(-not [IO.Directory]::Exists($c.root)){@{status='ABSENT';nonce=$c.nonce}|ConvertTo-Json -Compress;exit 0};CheckRoot
if($c.downloadsAcknowledged-ne$true -or $c.safeToDelete-ne$true){throw 'UNKNOWN_PROCESS_OR_RAW_DOWNLOAD_RETAIN_ROOT'}
$a=Registration;if($a.count-ne 0 -or $a.productState-ne -1 -or $a.relatedCount-ne 0){throw 'REGISTRATION_RECHECK_RETAIN_ROOT'}
foreach($x in @($c.helper,$c.client)){if((ProcessState $x)-notin@('ABSENT','PID_REUSED_ORIGINAL_GONE')){throw 'ORIGINAL_INSTANCE_UNKNOWN_RETAIN_ROOT'}}
$new=@();foreach($x in (Facts)){if(-not @($c.baseline|Where-Object{$_.pid-eq$x.pid -and $_.creationFiletime-ceq$x.creationFiletime}).Count){$new+=$x}};if($new.Count-ne 0){throw 'NEW_MSI_RECHECK_RETAIN_ROOT'}
$known=@{};foreach($f in $c.files){$known[$f.path]=$f};$seen=@();$dirs=@();$queue=[Collections.Generic.Queue[string]]::new();$queue.Enqueue($c.root)
while($queue.Count-gt 0){$dir=$queue.Dequeue();foreach($p in [IO.Directory]::GetFileSystemEntries($dir)){if(([IO.File]::GetAttributes($p)-band[IO.FileAttributes]::ReparsePoint)-ne 0){throw 'DELETE_REPARSE'};$rel=$p.Substring($c.root.Length+1).Replace('\','/');if(([IO.File]::GetAttributes($p)-band[IO.FileAttributes]::Directory)-ne 0){if($rel-notin$c.directories){throw 'DIRECTORY_CHANGED'};$dirs+=$rel;$queue.Enqueue($p)}else{if(-not $known.ContainsKey($rel)){throw 'FILE_CHANGED'};$f=Fact $p;if($f.sha256-cne$known[$rel].sha256 -or $f.bytes-ne$known[$rel].bytes){throw 'ORIGINAL_BYTES_CHANGED'};$seen+=$rel}}}
if($seen.Count-ne$known.Count -or $dirs.Count-ne$c.directories.Count){throw 'MEMBERSHIP_CHANGED'}
foreach($f in $c.files){if($f.path-cne'.owner'){[IO.File]::Delete((Join-Path $c.root ($f.path.Replace('/','\'))))}};foreach($dir in @($dirs|Sort-Object Length -Descending)){[IO.Directory]::Delete((Join-Path $c.root ($dir.Replace('/','\'))),$false)};[IO.File]::Delete((Join-Path $c.root '.owner'));[IO.Directory]::Delete($c.root,$false)
@{status='REMOVED';nonce=$c.nonce;absent=(-not [IO.Directory]::Exists($c.root));registration=$a}|ConvertTo-Json -Depth 8 -Compress
'''

def need(v,c):
    if not v:raise AssertionError(c)
def sha(b):return hashlib.sha256(b).hexdigest()
def scoped(path):
    p=Path(path);need(p.is_absolute() and p.resolve().is_relative_to(R) and not any(x.is_symlink() for x in [p,*p.parents] if x==R or R in x.parents),'OWNED_R75_NONLINK_SCOPE');need(p.suffix.lower() not in {'.key','.pfx','.p12'},'PRIVATE_CONTENT_NOT_READ');return p
def fact(path):
    p=scoped(path);need(p.is_file(),'REGULAR_INPUT');b=p.read_bytes();return dict(path=str(p),bytes=len(b),sha256=sha(b))
def remote(script,c):
    source=("$c='"+json.dumps(c,separators=(',',':')).replace("'","''")+"'|ConvertFrom-Json;"+COMMON+script).encode('utf-8');encoded=base64.b64encode(BOOTSTRAP.encode('utf-16-le')).decode();need(len(encoded)<=1200,'SHORT_UTF8_STDIN_BOOTSTRAP')
    return {'command':['ssh','-i',KEY,'-o','BatchMode=yes','-o','ConnectTimeout=8',HOST,'powershell.exe','-NoLogo','-NoProfile','-NonInteractive','-EncodedCommand',encoded],'input':source}
def absent(row,c):return row['status']=='ACTUAL_CALLER_PERUSER_AND_MACHINE' and row['product']==c['product'] and row['upgrade']==c['upgrade'] and row['count']==0 and row['productState']==-1 and row['relatedCount']==0 and row['contexts']==7 and row['sidArgument']=='' and row['otherUsers']=='NOT_QUERIED'
def original_gone(state):return state in ('ABSENT','PID_REUSED_ORIGINAL_GONE')
def qualify(pre,result,actual,c):
    need(pre['scope']=='PRE_JOIN_IDENTITY_ONLY_NOT_TRANSACTION_SUCCESS' and pre['nonce']==result['nonce']==c['nonce'],'SOURCE_BOUND_PREJOIN_RESULT_NONCE')
    need(pre['argv']==result['argv'] and len(pre['argv'])==4 and pre['argv'][1:]==['probe',c['nonce'],c['root']] and re.fullmatch('[0-9]{1,10}',pre['argv'][0]) and 0<int(pre['argv'][0])<=0xffffffff and result['transaction']==int(pre['argv'][0]),'EXACT_ENGINE_ARGV_AND_TRANSACTION')
    need(pre['pointerBytes']==result['pointerBytes']==4 and pre['own']==result['own'] and pre['parent']==result['parent'] and pre['own']['status']==pre['parent']['status']=='OK' and pre['consoleWindowPresent'] is False and result['consoleWindowPresent'] is False,'ACTUAL_GUI_X86_OWN_PARENT_FACTS')
    need(result['scope']=='INDEPENDENT_CHAINER_API_FIXTURE_NOT_RUST_PRODUCT_ACCEPTANCE' and result['status']=='JOIN_END_ROLLBACK_RETURNED' and result['joinReturn']==result['endRollbackReturn']==0 and result['changeOwnerEventReturned'] is True and result['failure'] is None,'ACTUAL_JOIN_END_ROLLBACK_RETURNED')
    closes=result['checkedClose'];need(closes and all(x['ok'] is True and x['win32']==0 for x in closes) and sum(x['role']=='returned-change-owner-event' for x in closes)==1,'CHECKED_ONCE_RETURNED_EVENT_AND_CONTROLS')
    held=actual.get('helperExpected');need(held and held['pid']==pre['own']['pid'] and held['creationFiletime']==str(pre['own']['creationFiletime']) and held['image']==pre['own']['image'],'ORIGINAL_HELPER_INSTANCE_BOUND')
    # JSON releaseProof is intentionally ignored: only actual held OS handle counts.
    need(actual.get('helperExitEvidence')=='ACTUAL_HELD_PROCESS_GETEXITCODE' and actual.get('helperExactInstanceAndBinary') is True and actual.get('helperExitCode')==0 and actual.get('helperHandleClose',{}).get('ok') is True and actual.get('negativeOwnerDeathAttempted') is False,'ACTUAL_ORIGINAL_HELPER_EXIT0_REQUIRED_OTHERWISE_UNKNOWN')
    need(actual.get('clientActualExit') is True and isinstance(actual.get('clientExitCode'),int),'ACTUAL_CLIENT_INTEGER_EXIT_NO_PREDICTED_SUCCESS_CODE')

def main():
    began=time.monotonic();p=argparse.ArgumentParser(description=__doc__)
    for name in ['spec','msi','static-report','report']:p.add_argument('--'+name,type=Path,required=True)
    for name in ['spec-sha256','msi-sha256','static-report-sha256']:p.add_argument('--'+name,required=True)
    p.add_argument('--execute',action='store_true');a=p.parse_args()
    need((R/'.owner').read_bytes()==b'future-classroom-owned-r75-installer-maintenance/v1\n' and (D/'.owner').read_bytes()==b'future-classroom-owned-r75-chainer-runtime-preparation-first/v1\n','EXACT_CONTROLLER_OWNERS')
    for x in [a.spec_sha256,a.msi_sha256,a.static_report_sha256]:need(re.fullmatch('[0-9a-f]{64}',x),'EXPLICIT_SHA256')
    paths=[scoped(x) for x in [a.spec,a.msi,a.static_report]];report=scoped(a.report);need(report.parent.is_dir() and report.parent.resolve().is_relative_to(E) and not report.exists(),'NEW_EVIDENCE_REPORT')
    need([fact(x)['sha256'] for x in paths]==[a.spec_sha256,a.msi_sha256,a.static_report_sha256],'EXPLICIT_IMMUTABLE_INPUTS')
    v=json.loads(a.spec.read_bytes());static=json.loads(a.static_report.read_bytes());need(static['status']=='PASS_EXACT_STATIC_CHAINER_FIXTURE_ONLY' and static['runtime']=='NOT_RUN' and static['productGuardian']=='NOT_RUN' and static['specSha256']==a.spec_sha256 and static['finalMsi']=={'bytes':fact(a.msi)['bytes'],'sha256':a.msi_sha256},'ACTUAL_EXACT_FINAL_STATIC_FIXTURE')
    need(re.fullmatch('[0-9a-f]{32}',v['nonce']) and v['runtimeBudgetSeconds']==120 and v['requiredMarker']=='future-classroom-owned-r75-chainer-probe/v1\n','SPEC_BOUNDED_FIXED_ROLE')
    if not a.execute:print(json.dumps(dict(status='PREPARED_QUIET_CHAINER_NOT_EXECUTED',specSha256=a.spec_sha256,finalMsiSha256=a.msi_sha256,overallBudgetSeconds=120,cleanupReserveSeconds=20,clientExitExpectation='RAW_FACTS_ONLY',helperExitRequirement='ACTUAL_HELD_GETEXITCODE_OR_UNKNOWN')));return 0
    c=dict(root=v['windowsReportRoot'],nonce=v['nonce'],sid=v['expectedOwnerSid'],marker=v['requiredMarker'],product=v['productCode'],upgrade=v['upgradeCode'],sha=a.msi_sha256,exeSha=static['probe']['sha256'])
    remote_msi=c['root']+'\\probe.msi';c['installArgs']=subprocess.list2cmdline(['/i',remote_msi,'/qn','/norestart','REBOOT=ReallySuppress','MSIRESTARTMANAGERCONTROL=DisableShutdown','/L*v',c['root']+'\\install.log']);c['uninstallArgs']=subprocess.list2cmdline(['/x',c['product'],'/qn','/norestart','REBOOT=ReallySuppress','MSIRESTARTMANAGERCONTROL=DisableShutdown','/L*v',c['root']+'\\uninstall.log'])
    inputs={str(x):fact(x) for x in paths};value=dict(status='RUNNING',startedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),overallBudgetSeconds=120,cleanupReserveSeconds=20,inputs=inputs,nonce=c['nonce'],remoteRoot=c['root'],operations=[],cleanup='NOT_RUN',scope='Independent rollback-only .NET chainer fixture. Not product Rust guardian or transaction/session binding acceptance. No expected MSI exit code. Other user registrations/cache not enumerated or deleted.')
    with report.open('x') as f:json.dump(value,f,indent=2);f.write('\n')
    created=False;actual={};baseline=[];snapshot=None;downloaded=False;safe=False;passed=False;raws={};client=None;helper=None
    def call(label,cmd,limit,cleanup=False):
        remaining=120-(time.monotonic()-began)-(0 if cleanup else 20);need(remaining>0,'ORIGINAL120_WITH20_CLEANUP_RESERVE');started=time.monotonic();code=None;stdout=b'';stderr=b'';timed=False;argv=cmd['command'] if isinstance(cmd,dict) else cmd;feed=cmd.get('input') if isinstance(cmd,dict) else None
        try:r=subprocess.run(argv,input=feed,capture_output=True,timeout=min(remaining,limit));code=r.returncode;stdout=r.stdout;stderr=r.stderr
        except subprocess.TimeoutExpired as e:stdout=e.stdout or b'';stderr=e.stderr or b'';timed=True
        row=dict(label=label,command=argv,actualLocalParentReturned=not timed,exitCode=code,timedOut=timed,elapsedSeconds=time.monotonic()-started)
        if feed is not None:row['stdin']=dict(bytes=len(feed),sha256=sha(feed),base64=base64.b64encode(feed).decode())
        for name,b in [('stdout',stdout),('stderr',stderr)]:
            x=report.with_name(report.stem+'-'+label+'.'+name+'.log')
            with x.open('xb') as f:f.write(b)
            row[name]=dict(**fact(x),base64=base64.b64encode(b).decode())
        value['operations'].append(row);need(code==0 and not timed,'ACTUAL_TRANSPORT_EXIT0_'+label);return json.loads(stdout) if label!='02-upload-final-msi' else None
    try:
        before=call('01-create-before',remote(CREATE,c),15);created=True;value['before']=before;baseline=before['baseline'];need(absent(before['before'],c) and before['nonce']==c['nonce'] and before['ownerSid']==c['sid'],'EXACT_BEFORE_ABSENCE_OWNER')
        call('02-upload-final-msi',['scp','-i',KEY,'-o','BatchMode=yes','-o','ConnectTimeout=8',str(a.msi),HOST+':'+remote_msi.replace('\\','/')],15)
        remaining=120-(time.monotonic()-began)-20;c['runMs']=min(45000,max(1000,int((remaining-15)*1000)));need(remaining>17,'RUNTIME_NEGATIVE_CLEANUP_HEADROOM')
        actual=call('03-quiet-chainer',remote(RUN,dict(c,native=NATIVE)),c['runMs']/1000+12);value['actual']=actual;client=actual.get('client');helper=actual.get('helperExpected')
    except Exception as e:value['runError']=dict(kind=type(e).__name__,code=str(e) if isinstance(e,AssertionError) else 'RUN_TRANSPORT_OR_INPUT_FAILURE')
    finally:
        if created:
            try:
                remaining=120-(time.monotonic()-began);need(remaining>8,'ORIGINAL_FINALLY_BUDGET');settled=call('90-formal-x-finally',remote(SETTLE,dict(c,baseline=baseline,client=client,helper=helper,settleMs=min(8000,max(1000,int((remaining-7)*1000))))),min(12,remaining-4),cleanup=True);value['settled']=settled
                safe=absent(settled['after'],c) and settled['newMsiexecCount']==0 and original_gone(settled['helperOriginalInstance']) and original_gone(settled['clientOriginalInstance'])
                x=settled['formalX'];need(not x['attempted'] or (x['actualExit'] is True and isinstance(x['exitCode'],int)),'FORMAL_X_ACTUAL_TERMINAL_IF_NEEDED')
                snapshot=call('91-capture-originals',remote(CAPTURE,c),8,cleanup=True);value['snapshot']=snapshot;need(snapshot['nonce']==c['nonce'] and snapshot['status']=='ACTUAL_OWNED_ORIGINAL_BYTES','ACTUAL_CAPTURE')
                records={r['path']:r for r in snapshot['files']};need(len(records)==len(snapshot['files']) and records['probe.msi']['sha256']==c['sha'],'UNIQUE_MEMBERS_FINAL_MSI')
                for name,encoded in snapshot['payloads'].items():
                    need(name in ['install.log','uninstall.log','pre-join.json','result.json','pre-join.tmp','result.tmp'],'CLOSED_RAW_PAYLOAD_NAME');b=base64.b64decode(encoded,validate=True);need(len(b)==records[name]['bytes'] and sha(b)==records[name]['sha256'],'EXACT_ORIGINAL_BYTES');dest=report.with_name(report.stem+'-'+name)
                    with dest.open('xb') as f:f.write(b)
                    raws[name]=b;value.setdefault('downloadedOriginals',[]).append(fact(dest))
                downloaded=True
                cleaned=call('99-owned-root-cleanup',remote(DELETE,dict(c,baseline=baseline,client=client,helper=helper,safeToDelete=safe,downloadsAcknowledged=downloaded,files=snapshot['files'],directories=snapshot['directories'])),8,cleanup=True);value['cleanupActual']=cleaned;need(cleaned['status']=='REMOVED' and cleaned['nonce']==c['nonce'] and cleaned['absent'] is True,'ACTUAL_EXACT_ROOT_REMOVED');value['cleanup']='PASS'
            except Exception as e:value.update(cleanup='FAIL_OR_UNKNOWN_RETAINED',cleanupError=dict(kind=type(e).__name__,code=str(e) if isinstance(e,AssertionError) else 'OWNER_REVIEW_REQUIRED_NO_BLIND_DELETE'))
        else:value['cleanup']='ROOT_NOT_CONFIRMED_CREATED_NO_DELETE_CLAIM'
    try:
        need('pre-join.json' in raws and 'result.json' in raws,'ORIGINAL_PREJOIN_AND_RESULT_REQUIRED');pre=json.loads(raws['pre-join.json']);result=json.loads(raws['result.json']);value['preJoinFacts']=pre;value['nativeFacts']=result
        qualify(pre,result,actual,c);need(safe and value['cleanup']=='PASS','ORIGINAL_INSTANCES_GONE_REGISTRATION_AND_ROOT_CLEANUP')
        need(inputs=={name:fact(name) for name in inputs},'ORIGINAL_INPUTS_UNCHANGED');value['inputsUnchanged']=True;passed=True
    except Exception as e:value['qualificationError']=dict(kind=type(e).__name__,code=str(e) if isinstance(e,AssertionError) else 'RAW_FACTS_NOT_ENOUGH_FOR_ACCEPTANCE')
    if 'install.log' in raws:
        log=raws['install.log'];text=log.decode('utf-16') if log.startswith((b'\xff\xfe',b'\xfe\xff')) else log.decode('utf-8-sig',errors='replace')
        value['verboseChainerCandidateLines']=[s for s in text.splitlines() if re.search('(?i)chainer',s)][:128]
        value['verboseExitQualification']='RAW_CANDIDATE_LINES_FOR_INDEPENDENT_REVIEW_ONLY_NO_GUESSED_EXIT_REGEX; JSON_releaseProof_is_not_OS_exit'
    elapsed=time.monotonic()-began;value.update(status='PASS_R75_INDEPENDENT_CHAINER_JOIN_ROLLBACK_ACTUAL_HELPER_EXIT0_ONLY' if passed and elapsed<=120 else 'FAIL_OR_UNKNOWN_INDEPENDENT_CHAINER_EVIDENCE',elapsedSeconds=elapsed,completedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),actualControllerReturned=True)
    report.write_text(json.dumps(value,indent=2)+'\n');print(json.dumps(dict(status=value['status'],cleanup=value['cleanup'],report=fact(report))));return 0 if value['status'].startswith('PASS_') else 1
if __name__=='__main__':raise SystemExit(main())
