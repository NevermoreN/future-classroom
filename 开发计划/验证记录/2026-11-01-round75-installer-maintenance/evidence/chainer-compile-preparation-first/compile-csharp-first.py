"""R75 root-only Windows csc compile/download/cleanup controller; never run EXE.
Explicit source/SHA/nonce/expected userSID.120s includes20s cleanup reserve.
No MSI/registry/productRoot/gate/Run/foreground/task/global-policy operations.
"""
from pathlib import Path, PurePosixPath
import argparse, base64, datetime, hashlib, json, re, struct, subprocess, sys, time
sys.dont_write_bytecode=True
ROOT=Path('/tmp/fc-round75-installer-maintenance');E=ROOT/'evidence';D=E/'chainer-compile-preparation-first'
HOST='elior@192.168.10.212';KEY='/config/.local/share/future-classroom-test-access/win11-192.168.10.212'
BOOTSTRAP="$ErrorActionPreference='Stop';[Console]::InputEncoding=[Text.UTF8Encoding]::new($false);[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false);$s=[Console]::In.ReadToEnd();& ([ScriptBlock]::Create($s))"
COMMON=r"""$ErrorActionPreference='Stop';[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
function Identity { $i=[Security.Principal.WindowsIdentity]::GetCurrent();try{return $i.User}finally{$i.Dispose()} }
function DirAcl($p,$sid){$a=[Security.AccessControl.DirectorySecurity]::new();$a.SetOwner($sid);$a.SetAccessRuleProtection($true,$false);foreach($s in @($sid,[Security.Principal.SecurityIdentifier]::new('S-1-5-18'))){$a.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new($s,'FullControl','ContainerInherit,ObjectInherit','None','Allow'))};[IO.Directory]::SetAccessControl($p,$a)}
function FileOwner($p,$sid){$a=[Security.AccessControl.FileSecurity]::new();$a.SetOwner($sid);$a.SetAccessRuleProtection($true,$false);foreach($s in @($sid,[Security.Principal.SecurityIdentifier]::new('S-1-5-18'))){$a.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new($s,'FullControl','Allow'))};[IO.File]::SetAccessControl($p,$a)}
function Fact($p){if(([IO.File]::GetAttributes($p)-band[IO.FileAttributes]::ReparsePoint)-ne 0){throw 'REPARSE_FILE'};return @{bytes=[Int64]([IO.FileInfo]::new($p).Length);sha256=(Get-FileHash -LiteralPath $p -Algorithm SHA256).Hash.ToLowerInvariant()} }
function RootCheck { $sid=Identity;if($sid.Value-cne$c.sid){throw 'CURRENT_USER_SID_MISMATCH'};if($c.root-cne(Join-Path ([IO.Path]::GetFullPath($env:TEMP)) ('FutureClassroom-r75-chainer-build-'+$c.nonce))){throw 'ROOT_PATH_MISMATCH'};if(([IO.File]::GetAttributes($c.root)-band[IO.FileAttributes]::ReparsePoint)-ne 0){throw 'ROOT_REPARSE'};$a=[IO.Directory]::GetAccessControl($c.root);if($a.GetOwner([Security.Principal.SecurityIdentifier]).Value-cne$c.sid -or -not $a.AreAccessRulesProtected){throw 'OWNER_OR_PROTECTED_DACL_MISMATCH'};$rules=@($a.GetAccessRules($true,$true,[Security.Principal.SecurityIdentifier]));if($rules.Count-ne 2 -or @($rules|Where-Object{$_.IdentityReference.Value-notin@($c.sid,'S-1-5-18') -or $_.AccessControlType-ne 'Allow' -or $_.FileSystemRights-ne[Security.AccessControl.FileSystemRights]::FullControl}).Count-ne 0){throw 'ROOT_USER_SYSTEM_ONLY_DACL'};if([IO.File]::ReadAllText((Join-Path $c.root '.owner'))-cne$c.marker){throw 'ROOT_MARKER_MISMATCH'} }
function ProcessGone($p){$x=$null;try{$x=[Diagnostics.Process]::GetProcessById([int]$p.pid);return (([string]$x.StartTime.ToUniversalTime().ToFileTimeUtc())-cne$p.creationFiletime)}catch [ArgumentException]{return $true}finally{if($null-ne$x){$x.Dispose()}} }
"""
CREATE=r"""if($PSVersionTable.PSVersion.Major-ne 5 -or $PSVersionTable.PSVersion.Minor-ne 1){throw 'EXPECTED_DESKTOP51'};$sid=Identity;if($sid.Value-cne$c.sid){throw 'EXPECTED_USER_SID_MISMATCH'}
$r=Join-Path ([IO.Path]::GetFullPath($env:TEMP)) ('FutureClassroom-r75-chainer-build-'+$c.nonce);if(Test-Path -LiteralPath $r){throw 'ROOT_NOT_FRESH'}
[IO.Directory]::CreateDirectory($r)|Out-Null;DirAcl $r $sid;$tmp=Join-Path $r 'compile-temp';[IO.Directory]::CreateDirectory($tmp)|Out-Null;DirAcl $tmp $sid
$owner=Join-Path $r '.owner';[IO.File]::WriteAllBytes($owner,[Text.Encoding]::ASCII.GetBytes($c.marker));FileOwner $owner $sid
@{status='CREATED';root=$r;nonce=$c.nonce;marker=$c.marker;actualOwnerSid=$sid.Value;protectedDacl=$true;allowedSids=@($sid.Value,'S-1-5-18');powershell=$PSVersionTable.PSVersion.ToString()}|ConvertTo-Json -Depth 6 -Compress
"""
COMPILE=r"""RootCheck;$sid=Identity;$cs=Join-Path $c.root 'ChainerProbe.cs';$exe=Join-Path $c.root 'ChainerProbe.exe';$temp=Join-Path $c.root 'compile-temp'
$before=Fact $cs;if($before.sha256-cne$c.sourceSha -or $before.bytes-ne$c.sourceBytes){throw 'UPLOADED_SOURCE_IDENTITY_MISMATCH'};FileOwner $cs $sid
$compiler=[IO.Path]::GetFullPath((Join-Path $env:WINDIR 'Microsoft.NET\Framework\v4.0.30319\csc.exe'));$compilerFact=Fact $compiler;$version=[Diagnostics.FileVersionInfo]::GetVersionInfo($compiler).FileVersion
$compilerArgs=@($compiler,'/nologo','/target:winexe','/platform:x86','/optimize+','/warnaserror+',('/out:'+$exe),$cs)
$p=[Diagnostics.Process]::new();$p.StartInfo.FileName=$compiler;$p.StartInfo.Arguments=$c.arguments;$p.StartInfo.WorkingDirectory=$c.root;$p.StartInfo.UseShellExecute=$false;$p.StartInfo.CreateNoWindow=$true;$p.StartInfo.RedirectStandardInput=$true;$p.StartInfo.RedirectStandardOutput=$true;$p.StartInfo.RedirectStandardError=$true;$p.StartInfo.EnvironmentVariables['TMP']=$temp;$p.StartInfo.EnvironmentVariables['TEMP']=$temp
$o=[IO.MemoryStream]::new();$e=[IO.MemoryStream]::new();$timed=$false;$physical=$false;$code=$null;$proc=$null;$watch=[Diagnostics.Stopwatch]::StartNew()
try{if(-not $p.Start()){throw 'CSC_START_FALSE'};$proc=@{pid=$p.Id;creationFiletime=[string]$p.StartTime.ToUniversalTime().ToFileTimeUtc()};$p.StandardInput.Close();$ot=$p.StandardOutput.BaseStream.CopyToAsync($o);$et=$p.StandardError.BaseStream.CopyToAsync($e)
 if(-not $p.WaitForExit(20000)){$timed=$true;$p.Kill();if(-not $p.WaitForExit(5000)){throw 'ORIGINAL_CSC_EXIT_UNKNOWN'}};$physical=$p.HasExited;$code=$p.ExitCode
 if(-not [Threading.Tasks.Task]::WaitAll([Threading.Tasks.Task[]]@($ot,$et),5000)){throw 'CSC_PIPE_COMPLETION_UNKNOWN'};if($o.Length-gt 1048576 -or $e.Length-gt 1048576){throw 'COMPILER_LOG_BOUND'}
 $stdout=Join-Path $c.root 'compiler.stdout.log';$stderr=Join-Path $c.root 'compiler.stderr.log';[IO.File]::WriteAllBytes($stdout,$o.ToArray());[IO.File]::WriteAllBytes($stderr,$e.ToArray());FileOwner $stdout $sid;FileOwner $stderr $sid
}finally{$watch.Stop();$p.Dispose();$o.Dispose();$e.Dispose()}
$after=Fact $cs;$same=($before.bytes-eq$after.bytes -and $before.sha256-ceq$after.sha256);$output=$null;if([IO.File]::Exists($exe)){FileOwner $exe $sid;$output=Fact $exe}
$again=Fact $compiler;if($again.sha256-cne$compilerFact.sha256 -or $again.bytes-ne$compilerFact.bytes){throw 'SYSTEM_COMPILER_CHANGED'}
$receipt=@{status=$(if($physical -and $code-eq 0 -and -not $timed -and $same -and $null-ne$output -and $watch.Elapsed.TotalSeconds-le 20){'PASS_CSC_ACTUAL_EXIT0'}else{'FAIL_CSC_OR_SOURCE'});actualExit=$physical;exitCode=$code;timedOut=$timed;elapsedSeconds=$watch.Elapsed.TotalSeconds;originalCompileBudgetSeconds=20;sourceBefore=$before;sourceAfter=$after;sourceUnchanged=$same;exe=$output;nonce=$c.nonce;commandArgs=$compilerArgs;compilerPath=$compiler;compilerSha256=$compilerFact.sha256;compilerBytes=$compilerFact.bytes;compilerVersion=$version;originalCompilerProcess=$proc;expectedOwnerSid=$c.sid;windowsSourcePath=$cs;windowsExePath=$exe;childTmp=$temp;childTemp=$temp;generatedExeExecuted=$false}
$receiptFile=Join-Path $c.root 'compiler-receipt.json';[IO.File]::WriteAllText($receiptFile,($receipt|ConvertTo-Json -Depth 8 -Compress),[Text.UTF8Encoding]::new($false));FileOwner $receiptFile $sid
@{status='COMPILER_RECEIPT_WRITTEN';receipt=$receipt;receiptFile=(Fact $receiptFile)}|ConvertTo-Json -Depth 10 -Compress
"""
COLLECT=r"""RootCheck;if(-not (ProcessGone $c.process)){throw 'ORIGINAL_CSC_STILL_LIVE'}
$files=@();$dirs=@();$queue=[Collections.Generic.Queue[string]]::new();$queue.Enqueue($c.root);$total=[Int64]0
while($queue.Count-gt 0){$dir=$queue.Dequeue();foreach($p in [IO.Directory]::GetFileSystemEntries($dir)){if(([IO.File]::GetAttributes($p)-band[IO.FileAttributes]::ReparsePoint)-ne 0){throw 'SNAPSHOT_REPARSE'};$rel=$p.Substring($c.root.Length+1).Replace('\','/');if($rel-match '(?i)\.(key|pfx|p12)$'){throw 'PRIVATE_SUFFIX_NOT_READ'};if(([IO.File]::GetAttributes($p)-band[IO.FileAttributes]::Directory)-ne 0){if($rel-ne 'compile-temp' -and -not $rel.StartsWith('compile-temp/')){throw 'UNEXPECTED_DIRECTORY'};$dirs+=$rel;$queue.Enqueue($p)}else{if($rel-notin@('.owner','ChainerProbe.cs','ChainerProbe.exe','compiler.stdout.log','compiler.stderr.log','compiler-receipt.json') -and -not $rel.StartsWith('compile-temp/')){throw 'UNEXPECTED_ROOT_FILE'};$f=Fact $p;$total+=[Int64]$f.bytes;if($total-gt 33554432 -or $files.Count-ge 128){throw 'SNAPSHOT_BOUND'};$files+=@{path=$rel;bytes=$f.bytes;sha256=$f.sha256}}}}
@{status='ACTUAL_CSC_EXITED_SNAPSHOT';root=$c.root;nonce=$c.nonce;compilerOriginalGone=$true;files=$files;directories=$dirs;totalBytes=$total}|ConvertTo-Json -Depth 8 -Compress
"""
CLEANUP=r"""if(-not [IO.Directory]::Exists($c.root)){@{status='ABSENT';nonce=$c.nonce}|ConvertTo-Json -Compress;exit 0};RootCheck
if($c.downloadsAcknowledged-ne $true -or -not (ProcessGone $c.process)){throw 'UNKNOWN_ORIGINAL_PROCESS_OR_DOWNLOADS_RETAIN_ROOT'}
$known=@{};foreach($f in $c.files){$known[$f.path]=$f};$seen=@();$dirs=@();$queue=[Collections.Generic.Queue[string]]::new();$queue.Enqueue($c.root)
while($queue.Count-gt 0){$dir=$queue.Dequeue();foreach($p in [IO.Directory]::GetFileSystemEntries($dir)){if(([IO.File]::GetAttributes($p)-band[IO.FileAttributes]::ReparsePoint)-ne 0){throw 'CLEANUP_REPARSE'};$rel=$p.Substring($c.root.Length+1).Replace('\','/');if(([IO.File]::GetAttributes($p)-band[IO.FileAttributes]::Directory)-ne 0){if($rel-notin$c.directories){throw 'UNKNOWN_DIRECTORY'};$dirs+=$rel;$queue.Enqueue($p)}else{if(-not $known.ContainsKey($rel)){throw 'UNKNOWN_FILE'};$f=Fact $p;if($f.bytes-ne$known[$rel].bytes -or $f.sha256-cne$known[$rel].sha256){throw 'FILE_CHANGED_RETAIN_ROOT'};$seen+=$rel}}}
if($seen.Count-ne$known.Count -or $dirs.Count-ne$c.directories.Count){throw 'MEMBERSHIP_CHANGED_RETAIN_ROOT'}
foreach($f in $c.files){[IO.File]::Delete((Join-Path $c.root ($f.path.Replace('/','\'))))};foreach($dir in @($dirs|Sort-Object Length -Descending)){[IO.Directory]::Delete((Join-Path $c.root ($dir.Replace('/','\'))),$false)};[IO.Directory]::Delete($c.root,$false)
@{status='REMOVED';nonce=$c.nonce;root=$c.root;files=$seen.Count;directories=$dirs.Count;absent=(-not [IO.Directory]::Exists($c.root));originalCscReaped=$true;downloadAcknowledgmentRequired=$true}|ConvertTo-Json -Compress
"""
def need(v,c):
    if not v:raise AssertionError(c)
def sha(b):return hashlib.sha256(b).hexdigest()
def scoped(path):
    p=Path(path);need(p.is_absolute() and p.resolve().is_relative_to(ROOT) and not any(x.is_symlink() for x in [p,*p.parents] if x==ROOT or ROOT in x.parents),'OWNED_R75_NONLINK_SCOPE');need(p.suffix.lower() not in {'.key','.pfx','.p12'},'PRIVATE_CONTENT_NOT_READ');return p
def fact(path):
    p=scoped(path);need(p.is_file(),'REGULAR_ORIGINAL');b=p.read_bytes();return dict(path=str(p),bytes=len(b),sha256=sha(b))
def relative(name):
    p=PurePosixPath(name);need(name==p.as_posix() and not p.is_absolute() and '..' not in p.parts,'EXACT_RELATIVE_ARTIFACT');return name
def inspect_clr(data):
    need(len(data)>=64 and data[:2]==b'MZ','ACTUAL_PE_DOS');u16=lambda p:struct.unpack_from('<H',data,p)[0];u32=lambda p:struct.unpack_from('<I',data,p)[0]
    pe=u32(60);need(64<=pe<=len(data)-24 and data[pe:pe+4]==b'PE\0\0' and u16(pe+4)==0x14c and u16(pe+22)&0x2 and not u16(pe+22)&0x2000,'ACTUAL_I386_EXE_NOT_DLL');opt=pe+24;size=u16(pe+20);need(size>=216 and opt+size<=len(data) and u16(opt)==0x10b and u16(opt+68)==2,'ACTUAL_PE32_GUI')
    need(u32(opt+92)>=15,'CLR_DATA_DIRECTORY_PRESENT');rva=u32(opt+208);clr_size=u32(opt+212);need(rva>0 and clr_size>=72,'CLR_COR20_HEADER_PRESENT')
    table=opt+size;sections=u16(pe+6);need(1<=sections<=96 and table+40*sections<=len(data),'ACTUAL_SECTION_TABLE');mapped=[]
    for i in range(sections):
        s=table+40*i;start=u32(s+12);rawsize=u32(s+16);raw=u32(s+20)
        if start<=rva and rva-start+72<=rawsize:mapped.append(raw+(rva-start))
    need(len(mapped)==1 and mapped[0]+72<=len(data),'CLR_RVA_UNIQUE_RAW_MAPPING');at=mapped[0];need(u32(at)>=72,'CLR_HEADER_SIZE');flags=u32(at+16);need(flags&0x3==0x3,'CLR_ILONLY_AND_32BITREQUIRED')
    return dict(machine='0x014c',optionalMagic='0x010b',subsystem=2,clrRva=rva,clrFileOffset=at,clrFlags=flags,ilOnly=True,bit32Required=True,generatedExeExecuted=False)
def remote(script,c):
    source=("$c='"+json.dumps(c,separators=(',',':')).replace("'","''")+"'|ConvertFrom-Json;"+COMMON+script).encode();encoded=base64.b64encode(BOOTSTRAP.encode('utf-16-le')).decode();need(len(encoded)<=1200,'SHORT_STDIN_BOOTSTRAP')
    return dict(command=['ssh','-i',KEY,'-o','BatchMode=yes','-o','ConnectTimeout=8',HOST,'powershell.exe','-NoLogo','-NoProfile','-NonInteractive','-EncodedCommand',encoded],input=source)
def main():
    began=time.monotonic();p=argparse.ArgumentParser(description=__doc__);p.add_argument('--cs-source',type=Path,required=True);p.add_argument('--cs-source-sha256',required=True);p.add_argument('--nonce',required=True);p.add_argument('--expected-owner-sid',required=True);p.add_argument('--local-output',type=Path,required=True);p.add_argument('--execute',action='store_true');a=p.parse_args()
    need(re.fullmatch('[0-9a-f]{64}',a.cs_source_sha256) and re.fullmatch('[0-9a-f]{32}',a.nonce) and re.fullmatch('S-1-(?:[0-9]+-){1,14}[0-9]+',a.expected_owner_sid),'EXPLICIT_SOURCE_NONCE_SID')
    source=scoped(a.cs_source);outdir=scoped(a.local_output);need(source.suffix=='.cs' and not outdir.exists() and outdir.parent.is_dir(),'EXPLICIT_CS_AND_FRESH_LOCAL_OUTPUT')
    need((ROOT/'.owner').read_bytes()==b'future-classroom-owned-r75-installer-maintenance/v1\n' and (D/'.owner').read_bytes()==b'future-classroom-owned-r75-chainer-compile-preparation/v1\n','EXACT_OWNERS')
    if not a.execute:print(json.dumps(dict(status='PREPARED_CSC_CONTROLLER_NOT_EXECUTED',sourceSha256=a.cs_source_sha256,nonce=a.nonce,expectedOwnerSid=a.expected_owner_sid,overallSeconds=120,cleanupReserveSeconds=20,generatedExeExecuted=False)));return 0
    original=fact(source);need(original['sha256']==a.cs_source_sha256 and 0<original['bytes']<=1048576,'EXACT_BOUNDED_EXPLICIT_CS_SOURCE');outdir.mkdir();(outdir/'.owner').write_bytes(('future-classroom-owned-r75-chainer-local-compile/v1|'+a.nonce+'\n').encode())
    report=outdir/'compile-operation.json';c=dict(nonce=a.nonce,sid=a.expected_owner_sid,marker='future-classroom-owned-r75-chainer-build/v1|'+a.nonce+'\n',sourceSha=original['sha256'],sourceBytes=original['bytes'])
    value=dict(status='RUNNING',startedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),source=original,nonce=a.nonce,expectedOwnerSid=a.expected_owner_sid,overallBudgetSeconds=120,cleanupReserveSeconds=20,originalCompileBudgetSeconds=20,operations=[],cleanup='NOT_RUN',generatedExeExecuted=False,scope='Fixed-framework csc compile-only test fixture. Exact original source/exe/receipt/log downloads before deletion; no WinExe/MSI/registry/product/gate/Run/foreground/policy action.')
    with report.open('x') as f:json.dump(value,f,indent=2);f.write('\n')
    ready=False;accepted=False;created=False;snapshot=None;process=None
    def call(label,cmd,limit,cleanup=False):
        remaining=120-(time.monotonic()-began)-(0 if cleanup else 20);need(remaining>0,'ORIGINAL120S_WITH20S_CLEANUP_RESERVE');t=time.monotonic();code=None;stdout=b'';stderr=b'';timed=False;argv=cmd['command'] if isinstance(cmd,dict) else cmd;feed=cmd.get('input') if isinstance(cmd,dict) else None
        try:r=subprocess.run(argv,input=feed,capture_output=True,timeout=min(limit,remaining));code=r.returncode;stdout=r.stdout;stderr=r.stderr
        except subprocess.TimeoutExpired as error:stdout=error.stdout or b'';stderr=error.stderr or b'';timed=True
        row=dict(label=label,command=argv,actualLocalParentReturned=not timed,exitCode=code,timedOut=timed,elapsedSeconds=time.monotonic()-t)
        if feed is not None:row['stdin']=dict(bytes=len(feed),sha256=sha(feed),base64=base64.b64encode(feed).decode())
        for name,data in (('stdout',stdout),('stderr',stderr)):
            f=outdir/(label+'.'+name+'.log')
            with f.open('xb') as stream:stream.write(data)
            row[name]=dict(**fact(f),base64=base64.b64encode(data).decode())
        value['operations'].append(row);need(code==0 and not timed,'ACTUAL_TRANSPORT_EXIT0_'+label);return json.loads(stdout) if not label.startswith(('02-','05-')) else None
    try:
        made=call('01-create',remote(CREATE,c),15);created=True;need(made['status']=='CREATED' and made['nonce']==a.nonce and made['actualOwnerSid']==a.expected_owner_sid and made['marker']==c['marker'],'FRESH_SID_OWNED_BUILD_ROOT');c['root']=made['root'];value['remoteRoot']=c['root'];value['createFacts']=made
        windows_cs=c['root']+'\\ChainerProbe.cs';windows_exe=c['root']+'\\ChainerProbe.exe';args=['/nologo','/target:winexe','/platform:x86','/optimize+','/warnaserror+','/out:'+windows_exe,windows_cs];c['arguments']=subprocess.list2cmdline(args)
        call('02-upload-source',['scp','-i',KEY,'-o','BatchMode=yes','-o','ConnectTimeout=8',str(source),HOST+':'+windows_cs.replace('\\','/')],15)
        compiled=call('03-compile-only',remote(COMPILE,c),35);value['compiler']=compiled;receipt=compiled['receipt'];process=receipt['originalCompilerProcess'];need(receipt['actualExit'] is True and process is not None,'ORIGINAL_COMPILER_PHYSICALLY_RETURNED')
        snapshot=call('04-snapshot',remote(COLLECT,dict(c,process=process)),10);value['snapshot']=snapshot;need(snapshot['status']=='ACTUAL_CSC_EXITED_SNAPSHOT' and snapshot['compilerOriginalGone'] is True and snapshot['nonce']==a.nonce,'ORIGINAL_CSC_GONE_AND_SNAPSHOT')
        records={relative(r['path']):r for r in snapshot['files']};need(len(records)==len(snapshot['files'])<=128,'UNIQUE_BOUNDED_SNAPSHOT');required={'ChainerProbe.cs','compiler-receipt.json','compiler.stdout.log','compiler.stderr.log'};need(required<=set(records),'ALL_ORIGINAL_SOURCE_RECEIPT_LOGS_PRESENT')
        downloads=required|({'ChainerProbe.exe'} if 'ChainerProbe.exe' in records else set());downloaded=[]
        for index,name in enumerate(sorted(downloads),1):
            local=outdir/name;call('05-download-'+str(index),['scp','-i',KEY,'-o','BatchMode=yes','-o','ConnectTimeout=8',HOST+':'+c['root'].replace('\\','/')+'/'+name,str(local)],10);got=fact(local);need(got['bytes']==records[name]['bytes'] and got['sha256']==records[name]['sha256'],'DOWNLOADED_ORIGINAL_EXACT_'+name);downloaded.append(got)
        value['downloadedOriginals']=downloaded;raw=(outdir/'compiler-receipt.json').read_bytes();actual=json.loads(raw);need(actual==receipt,'DOWNLOADED_COMPILER_RECEIPT_EXACT_REMOTE_OBJECT');expected=dict(bytes=original['bytes'],sha256=original['sha256']);need(actual['sourceBefore']==actual['sourceAfter']==expected and actual['sourceUnchanged'] is True and fact(source)==original and (outdir/'ChainerProbe.cs').read_bytes()==source.read_bytes(),'SOURCE_PRE_POST_AND_DOWNLOADED_ORIGINAL_IDENTICAL')
        ready=True
        need(actual['status']=='PASS_CSC_ACTUAL_EXIT0' and actual['actualExit'] is True and actual['exitCode']==0 and actual['timedOut'] is False and actual['nonce']==a.nonce and 0<=actual['elapsedSeconds']<=20,'ACTUAL_CSC_SUCCESS_WITHOUT_TIMEOUT')
        need(actual['commandArgs'][1:]==args and actual['compilerPath']==actual['commandArgs'][0] and actual['compilerPath'].lower().endswith('\\windows\\microsoft.net\\framework\\v4.0.30319\\csc.exe') and actual['expectedOwnerSid']==a.expected_owner_sid and actual['childTmp']==actual['childTemp']==c['root']+'\\compile-temp','FIXED_CSC_AND_ALL_ACTUAL_FLAGS_TMP')
        need(re.fullmatch('[0-9a-f]{64}',actual['compilerSha256']) and actual['compilerVersion'] and actual['generatedExeExecuted'] is False,'REAL_COMPILER_IDENTITY_AND_NO_EXE_EXECUTION')
        exe=outdir/'ChainerProbe.exe';f=fact(exe);need(actual['exe']==dict(bytes=f['bytes'],sha256=f['sha256']),'ACTUAL_COMPILED_EXE_IDENTITY');value['actualPe']=inspect_clr(exe.read_bytes());value['compilerReceipt']=fact(outdir/'compiler-receipt.json');value['compiledExe']=f;accepted=True
    except Exception as error:value.update(status='FAIL_CSC_DOWNLOAD_OR_PROOF',error=dict(kind=type(error).__name__,code=str(error) if isinstance(error,AssertionError) else 'COMPILER_TRANSPORT_INPUT_OR_PE_FAILURE'))
    finally:
        if created:
            try:
                cleaned=call('99-cleanup',remote(CLEANUP,dict(c,process=process,downloadsAcknowledged=ready,files=snapshot['files'] if snapshot else [],directories=snapshot['directories'] if snapshot else [])),20,cleanup=True);value['cleanupActual']=cleaned;need(cleaned['status']=='REMOVED' and cleaned['nonce']==a.nonce and cleaned['absent'] is True and cleaned['originalCscReaped'] is True,'OWNED_ROOT_REMOVED_AFTER_DOWNLOAD_AND_REAP');value['cleanup']='PASS'
            except Exception as error:value.update(cleanup='FAIL_OR_UNKNOWN_RETAINED',cleanupError=dict(kind=type(error).__name__,code=str(error) if isinstance(error,AssertionError) else 'OWNER_REVIEW_REQUIRED_NO_BLIND_DELETE'))
        else:value['cleanup']='NOT_CREATED_OR_UNKNOWN; no deletion claimed'
    elapsed=time.monotonic()-began
    if accepted and value['cleanup']=='PASS' and elapsed<=120:value['status']='PASS_R75_CSC_GUI32_COMPILE_DOWNLOAD_ONLY'
    elif value['status']=='RUNNING':value['status']='FAIL_CSC_CLEANUP_OR_TOTAL_BUDGET'
    value.update(elapsedSeconds=elapsed,completedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),actualControllerReturned=True);report.write_text(json.dumps(value,indent=2)+'\n');print(json.dumps(dict(status=value['status'],cleanup=value['cleanup'],report=fact(report))));return 0 if value['status']=='PASS_R75_CSC_GUI32_COMPILE_DOWNLOAD_ONLY' else 1
if __name__=='__main__':raise SystemExit(main())
