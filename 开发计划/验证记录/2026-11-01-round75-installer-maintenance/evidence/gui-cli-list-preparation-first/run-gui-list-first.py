"""R75 pinned GUI test fixture --list only. Root alone explicitly --execute.
Preserve six intended observer arguments plus libtest --list. Never run tests,
observer, MSI, lifecycle product/gate/Pending or browser. Own exact temp cleanup.
"""
from pathlib import Path, PureWindowsPath
import argparse, base64, datetime, gzip, hashlib, json, re, subprocess, sys, time, uuid
sys.dont_write_bytecode=True
ROOT=Path('/tmp/fc-round75-installer-maintenance');E=ROOT/'evidence';D=E/'gui-cli-list-preparation-first'
LEDGER=ROOT/'gui-test-fixture-f8bd373d36224240a4aeb4ae49b76f96/fixture-ledger.json'
LEDGER_SHA='fa372949bb74b30b540d122af552ebf4c2006e88a6e771ddd7bef76c57ab8799'
FIXTURE_SHA='5ae22a321c0c8fc564da8567d341ca93946219330d61f31291997a422a579b39'
NAME='installer_lifecycle::native::broker::context_probe::engine_parent_context_observer'
MARKER='future-classroom-owned-r75-parent-observer/v1\n'
HOST='elior@192.168.10.212';IDENTITY='/config/.local/share/future-classroom-test-access/win11-192.168.10.212'
CREATE=r"""$ErrorActionPreference='Stop';$r=Join-Path ([IO.Path]::GetFullPath($env:TEMP)) ('FutureClassroom-r75-parent-observer-'+$c.nonce)
if($r.Length -gt 240 -or (Test-Path -LiteralPath $r)){throw 'ROOT_PREFLIGHT_NOT_FRESH'}
[IO.Directory]::CreateDirectory($r)|Out-Null
$acl=New-Object Security.AccessControl.DirectorySecurity
$acl.SetAccessRuleProtection($true,$false)
$sid=[Security.Principal.WindowsIdentity]::GetCurrent().User
foreach($s in @($sid,[Security.Principal.SecurityIdentifier]::new('S-1-5-18'),[Security.Principal.SecurityIdentifier]::new('S-1-5-32-544'))){
 $rule=[Security.AccessControl.FileSystemAccessRule]::new($s,'FullControl','ContainerInherit,ObjectInherit','None','Allow');$acl.AddAccessRule($rule)
}
[IO.Directory]::SetAccessControl($r,$acl)
[IO.File]::WriteAllBytes((Join-Path $r '.owner'),[Text.Encoding]::ASCII.GetBytes($c.marker))
@{status='CREATED';root=$r;marker=$c.marker;nonce=$c.nonce}|ConvertTo-Json -Compress
"""
RUN=r"""$ErrorActionPreference='Stop';$r=$c.root;$file=Join-Path $r 'observer-harness.exe';$owner=Join-Path $r '.owner'
if(([IO.File]::ReadAllText($owner)) -cne $c.marker){throw 'OWNER_MISMATCH'}
if(([IO.File]::GetAttributes($r) -band [IO.FileAttributes]::ReparsePoint) -ne 0){throw 'ROOT_REPARSE'}
if((Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash.ToLowerInvariant() -ne $c.sha){throw 'FIXTURE_SHA_MISMATCH'}
if([IO.File]::Exists((Join-Path $r 'context.json'))){throw 'UNEXPECTED_OBSERVER_REPORT'}
$p=New-Object Diagnostics.Process;$p.StartInfo.FileName=$file;$p.StartInfo.Arguments=$c.commandLine;$p.StartInfo.WorkingDirectory=$r
$p.StartInfo.UseShellExecute=$false;$p.StartInfo.CreateNoWindow=$true;$p.StartInfo.RedirectStandardInput=$true;$p.StartInfo.RedirectStandardOutput=$true;$p.StartInfo.RedirectStandardError=$true
$o=New-Object IO.MemoryStream;$e=New-Object IO.MemoryStream
try{
 if(-not $p.Start()){throw 'PROCESS_START_FALSE'};$pidFact=$p.Id;$p.StandardInput.Close()
 $ot=$p.StandardOutput.BaseStream.CopyToAsync($o);$et=$p.StandardError.BaseStream.CopyToAsync($e)
 if(-not $p.WaitForExit(20000)){$p.Kill();if(-not $p.WaitForExit(5000)){throw 'OWNED_PROCESS_EXIT_UNKNOWN'};throw 'ORIGINAL_LIST20S_TIMEOUT'}
 if(-not [Threading.Tasks.Task]::WaitAll([Threading.Tasks.Task[]]@($ot,$et),5000)){throw 'PIPE_COMPLETION_TIMEOUT'}
 if($o.Length -gt 65536 -or $e.Length -gt 65536){throw 'CAPTURE_BOUND'}
 $answer=@{status='ACTUAL_LIST_PROCESS_RETURNED';pid=$pidFact;exitCode=$p.ExitCode;actualExit=$p.HasExited;stdoutBase64=[Convert]::ToBase64String($o.ToArray());stderrBase64=[Convert]::ToBase64String($e.ToArray());contextReportAbsent=(-not [IO.File]::Exists((Join-Path $r 'context.json')));fixtureSha256=(Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash.ToLowerInvariant();nonce=$c.nonce}
}finally{$o.Dispose();$e.Dispose();$p.Dispose()}
$answer|ConvertTo-Json -Compress
"""
CLEANUP=r"""$ErrorActionPreference='Stop';$r=Join-Path ([IO.Path]::GetFullPath($env:TEMP)) ('FutureClassroom-r75-parent-observer-'+$c.nonce)
if(-not [IO.Directory]::Exists($r)){@{status='ABSENT';root=$r;nonce=$c.nonce}|ConvertTo-Json -Compress;exit 0}
if(([IO.File]::GetAttributes($r) -band [IO.FileAttributes]::ReparsePoint) -ne 0){throw 'CLEANUP_ROOT_REPARSE'}
$owner=Join-Path $r '.owner';$file=Join-Path $r 'observer-harness.exe'
if(([IO.File]::ReadAllText($owner)) -cne $c.marker){throw 'CLEANUP_OWNER_MISMATCH'}
$members=@([IO.Directory]::GetFileSystemEntries($r));foreach($m in $members){if($m -cne $owner -and $m -cne $file){throw 'UNEXPECTED_ROOT_MEMBER'};if(([IO.File]::GetAttributes($m) -band [IO.FileAttributes]::ReparsePoint) -ne 0){throw 'CLEANUP_MEMBER_REPARSE'}}
if([IO.File]::Exists($file)){if((Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash.ToLowerInvariant() -ne $c.sha){throw 'CLEANUP_FIXTURE_IDENTITY_UNKNOWN'};[IO.File]::Delete($file)}
[IO.File]::Delete($owner);[IO.Directory]::Delete($r,$false)
@{status='REMOVED';root=$r;marker=$c.marker;nonce=$c.nonce;absent=(-not [IO.Directory]::Exists($r))}|ConvertTo-Json -Compress
"""
def need(v,c):
    if not v:raise AssertionError(c)
def sha(data):return hashlib.sha256(data).hexdigest()
def scoped(path):
    p=Path(path);need(p.is_absolute() and p.resolve().is_relative_to(ROOT) and not any(x.is_symlink() for x in [p,*p.parents] if x==ROOT or ROOT in x.parents),'OWNED_R75_REGULAR_SCOPE');need(p.suffix.lower() not in {'.key','.pfx','.p12'},'PRIVATE_CONTENT_NOT_READ');return p
def fact(path):
    p=scoped(path);need(p.is_file(),'REGULAR_INPUT');data=p.read_bytes();return dict(path=str(p),bytes=len(data),sha256=sha(data))
def remote_command(script,config):
    packed=base64.b64encode(json.dumps(config,separators=(',',':')).encode()).decode()
    prefix="$ErrorActionPreference='Stop';[Console]::OutputEncoding=New-Object Text.UTF8Encoding($false);if($PSVersionTable.PSVersion.Major -ne 5 -or $PSVersionTable.PSVersion.Minor -ne 1){throw 'EXPECTED_DESKTOP51'};$c=[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('"+packed+"'))|ConvertFrom-Json;"
    # Lossless in-memory script compression keeps the encoded SSH command below
    # cmd.exe limits even with a 240-UTF16-unit owned root; no uploaded script.
    body=base64.b64encode(gzip.compress((prefix+script).encode(),mtime=0)).decode()
    decoder="$b=[Convert]::FromBase64String('"+body+"');$m=[IO.MemoryStream]::new();$m.Write($b,0,$b.Length);$m.Position=0;$g=[IO.Compression.GZipStream]::new($m,[IO.Compression.CompressionMode]::Decompress);$r=[IO.StreamReader]::new($g,[Text.Encoding]::UTF8);try{$s=$r.ReadToEnd()}finally{$r.Dispose();$g.Dispose();$m.Dispose()};& ([ScriptBlock]::Create($s))"
    encoded=base64.b64encode(decoder.encode('utf-16-le')).decode();need(len(encoded)<=7500,'BOUNDED_ENCODED_POWERSHELL_COMMAND')
    return ['ssh','-i',IDENTITY,'-o','BatchMode=yes','-o','ConnectTimeout=8',HOST,'powershell.exe','-NoLogo','-NoProfile','-NonInteractive','-EncodedCommand',encoded]
def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--report',type=Path,required=True);p.add_argument('--execute',action='store_true');a=p.parse_args()
    need((ROOT/'.owner').read_bytes()==b'future-classroom-owned-r75-installer-maintenance/v1\n' and (D/'.owner').read_bytes()==b'future-classroom-owned-r75-gui-cli-list-preparation/v1\n','EXACT_OWNERS')
    report=scoped(a.report);need(report.parent.resolve().is_relative_to(E) and not report.exists(),'EXCLUSIVE_NEW_EVIDENCE_REPORT')
    if not a.execute:print(json.dumps(dict(status='PREPARED_GUI_FIXTURE_LIST_ONLY_NOT_EXECUTED',fixtureLedgerSha256=LEDGER_SHA,fixtureSha256=FIXTURE_SHA,scope='Six intended observer args plus --list; only selected test listing, not observer execution.',originalOverallBudgetSeconds=120)));return 0
    ledger_fact=fact(LEDGER);need(ledger_fact['sha256']==LEDGER_SHA,'EXACT_ACTUAL_CLONE_LEDGER');ledger=json.loads(LEDGER.read_bytes());fixture=Path(ledger['guiFixture']['path'])
    need(ledger['status']=='PASS_EXACT_CUI3_TO_GUI2_TEST_FIXTURE_CLONE' and ledger['profileTest'] is True and ledger['normalProduct'] is False and fact(fixture)==ledger['guiFixture'] and ledger['guiFixture']['sha256']==FIXTURE_SHA,'SAME_ORIGINAL_PINNED_GUI_TEST_FIXTURE')
    nonce=uuid.uuid4().hex;config=dict(nonce=nonce,marker=MARKER,sha=FIXTURE_SHA)
    result=dict(status='RUNNING',startedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),originalOverallBudgetSeconds=120,originalListProcessBudgetSeconds=20,fixtureLedger=ledger_fact,guiFixture=fact(fixture),nonce=nonce,marker=MARKER,operations=[],cleanup='NOT_RUN',observerExecuted=False,msiExecuted=False,sourceGitBase=ledger['sourceGitBase'],candidateSourceProducerSha256=ledger['candidateSourceProducerSha256'],candidateSourceFreezeSha256=ledger['candidateSourceFreezeSha256'],scope='Same test-only GUI clone list admission/capture only. No observation of actual parent context, MSI, gate, Pending, product or normal helper execution.')
    with report.open('x') as f:json.dump(result,f,indent=2);f.write('\n')
    began=time.monotonic();steps_done=False;created=False
    def call(label,cmd,timeout):
        need(time.monotonic()-began<120,'ORIGINAL120S_OVERALL_DEADLINE');t=time.monotonic();code=None;out=b'';err=b'';timed=False
        try:c=subprocess.run(cmd,capture_output=True,timeout=min(timeout,max(0.1,120-(time.monotonic()-began))));code=c.returncode;out=c.stdout;err=c.stderr
        except subprocess.TimeoutExpired as error:out=error.stdout or b'';err=error.stderr or b'';timed=True
        row=dict(label=label,command=cmd,exitCode=code,actualLocalParentReturned=not timed,timedOut=timed,elapsedSeconds=time.monotonic()-t)
        for key,data in (('stdout',out),('stderr',err)):
            path=report.with_name(report.stem+'-'+label+'.'+key+'.log')
            with path.open('xb') as f:f.write(data)
            row[key]=dict(**fact(path),base64=base64.b64encode(data).decode())
        result['operations'].append(row);need(code==0 and not timed,'ACTUAL_OPERATION_EXIT0_'+label);return out
    try:
        made=json.loads(call('01-create-owned-root',remote_command(CREATE,config),20));created=True
        remote_root=made['root'];path=PureWindowsPath(remote_root)
        need(made['status']=='CREATED' and made['marker']==MARKER and made['nonce']==nonce and path.is_absolute() and str(path)==remote_root and path.name=='FutureClassroom-r75-parent-observer-'+nonce and len(remote_root.encode('utf-16-le'))<=480,'EXACT_ACTUAL_OWNED_ROOT')
        six=['--exact',NAME,'--test-threads=1','--nocapture','--skip','fc-owned-parent-observer-v1:'+nonce+':'+remote_root];argv=six+['--list'];command_line=subprocess.list2cmdline(argv)
        result.update(remoteRoot=remote_root,observerSixArguments=six,actualListArguments=argv,actualListCommandLine=command_line)
        destination=HOST+':'+remote_root.replace('\\','/')+'/observer-harness.exe'
        call('02-upload-same-clone',['scp','-i',IDENTITY,'-o','BatchMode=yes','-o','ConnectTimeout=8',str(fixture),destination],30)
        run_config=dict(config,root=remote_root,commandLine=command_line)
        actual=json.loads(call('03-list-only',remote_command(RUN,run_config),40));result['actualList']=actual
        need(actual['status']=='ACTUAL_LIST_PROCESS_RETURNED' and actual['exitCode']==0 and actual['actualExit'] is True and actual['nonce']==nonce and actual['fixtureSha256']==FIXTURE_SHA and actual['contextReportAbsent'] is True,'ACTUAL_SAME_CLONE_LIST_EXIT0_NO_OBSERVER_REPORT')
        raw=base64.b64decode(actual['stdoutBase64'],validate=True);err=base64.b64decode(actual['stderrBase64'],validate=True);need(len(raw)<=65536 and len(err)<=65536,'EXACT_BOUNDED_NATIVE_PIPE_CAPTURE')
        lines=[line for line in raw.decode('utf-8-sig').splitlines() if line];need(lines==[NAME+': test','1 test, 0 benchmarks'] and err==b'','EXACT_ONE_TEST_LIST_CAPTURE_NO_TEST_EXECUTION')
        result['listCapture']=dict(stdout=dict(bytes=len(raw),sha256=sha(raw),base64=actual['stdoutBase64']),stderr=dict(bytes=len(err),sha256=sha(err),base64=actual['stderrBase64']),nonemptyLines=lines,capture='Exact bytes from .NET redirected native BaseStream memory buffers, not re-encoded StandardOutput strings.')
        need(fact(LEDGER)==ledger_fact and fact(fixture)==ledger['guiFixture'],'LOCAL_CLONE_LEDGER_AND_EXE_UNCHANGED');steps_done=True
    except Exception as error:result.update(status='FAIL_GUI_FIXTURE_LIST_OR_CAPTURE',error=dict(kind=type(error).__name__,code=str(error) if isinstance(error,AssertionError) else 'TRANSPORT_JSON_OR_CAPTURE_FAILURE'))
    finally:
        try:
            cleaned=json.loads(call('99-owned-finally-cleanup',remote_command(CLEANUP,config),20));result['cleanupActual']=cleaned
            need(cleaned['nonce']==nonce and cleaned['status'] in ('REMOVED','ABSENT'),'OWNED_CLEANUP_RESULT')
            if created:need(cleaned['status']=='REMOVED' and cleaned['marker']==MARKER and cleaned['absent'] is True,'CREATED_ROOT_ACTUALLY_REMOVED')
            result['cleanup']='PASS'
        except Exception as error:result.update(cleanup='FAIL_OR_UNKNOWN',cleanupError=dict(kind=type(error).__name__,code=str(error) if isinstance(error,AssertionError) else 'OWNER_REVIEW_REQUIRED_NO_UNBOUNDED_CLEANUP'))
    if steps_done and result['cleanup']=='PASS':result.update(status='ACTUAL_HARNESS_CLI_PARSE_PASS_NOT_OBSERVER_RUNTIME',observerSha256=FIXTURE_SHA,actualExit=True,exitCode=0,commandArgs=argv,selectedTests=[NAME])
    elif result['status']=='RUNNING':result['status']='FAIL_GUI_LIST_CLEANUP_OR_UNKNOWN'
    result.update(actualControllerReturned=True,elapsedSeconds=time.monotonic()-began,completedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),qualification='Six intended observer parameters were passed with an additional --list libtest selector. No observer function/request parser ran; actual six-argv observer admission and native parent observation require a separate root action.')
    report.write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(dict(status=result['status'],cleanup=result['cleanup'],report=fact(report))));return 0 if result['status']=='ACTUAL_HARNESS_CLI_PARSE_PASS_NOT_OBSERVER_RUNTIME' else 1
if __name__=='__main__':raise SystemExit(main())
