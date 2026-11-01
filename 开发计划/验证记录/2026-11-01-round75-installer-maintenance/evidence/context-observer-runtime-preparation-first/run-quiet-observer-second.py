"""Prepared R75 exact final test-only MSI quiet observer. Root alone --execute.
One120s budget reserves20s cleanup. No production MSI/Begin/gate/Pending/Run,
foreground/UI/task access, cache/registration deletion or unknown-process kill.
"""
from pathlib import Path
import argparse, base64, datetime, gzip, hashlib, json, re, subprocess, sys, time
sys.dont_write_bytecode=True
R=Path('/tmp/fc-round75-installer-maintenance');E=R/'evidence';D=E/'context-observer-runtime-preparation-first'
STATIC=E/'context-probe-build-second-preparation/build-export-second.json';STATIC_SHA='b4a009bb953ae829d5eb8c564017f2d5ecc0140e33b88a7d4b29c38f41b5ae0a'
MSI=E/'context-probe-build-second-preparation/authored-third/context-probe-final.msi';MSI_SHA='0a1ec454c5ef4f2f760fced6a1cdcb5aaffaffacddee5e1ddeaa3d94cd1ebd43'
CLI=E/'gui-cli-list-first.json';CLI_SHA='f823b6e55eee0733b5d32ebeea2bdc2d9b3bfeda66186cdbd749c838a15a0875'
LEDGER=R/'gui-test-fixture-f8bd373d36224240a4aeb4ae49b76f96/fixture-ledger.json';LEDGER_SHA='fa372949bb74b30b540d122af552ebf4c2006e88a6e771ddd7bef76c57ab8799'
PRODUCT='{4BFAFA4A-6E0D-5FAB-90F7-4C9967F2ED46}'
HOST='elior@192.168.10.212';IDENTITY='/config/.local/share/future-classroom-test-access/win11-192.168.10.212'
MARKER='future-classroom-owned-r75-parent-observer/v1\n';ABORT='Owned readonly context probe intentionally stops before InstallInitialize.'
COMMON=r"""$ErrorActionPreference='Stop';[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
function Facts { $a=@();foreach($p in [Diagnostics.Process]::GetProcessesByName('msiexec')){try{$a+=@{pid=$p.Id;creationFiletime=[string]$p.StartTime.ToUniversalTime().ToFileTimeUtc()}}finally{$p.Dispose()}};return ,$a }
function Registration { $i=$null;$x=$null;try{$i=New-Object -ComObject WindowsInstaller.Installer;$x=$i.GetType().InvokeMember('ProductsEx',[Reflection.BindingFlags]::GetProperty,$null,$i,@($c.product,'',7));$n=$x.Count;$s=$i.GetType().InvokeMember('ProductState',[Reflection.BindingFlags]::GetProperty,$null,$i,@($c.product));return @{status='ACTUAL_READONLY_CALLER_AND_MACHINE_CONTEXTS';product=$c.product;count=$n;productState=$s;contexts=7;userSidArgument='';otherUsers='NOT_QUERIED';cache='NOT_QUERIED'}}finally{if($null-ne$x){[void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($x)};if($null-ne$i){[void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($i)}} }
function CheckRoot { if($c.root -cne (Join-Path ([IO.Path]::GetFullPath($env:TEMP)) ('FutureClassroom-r75-parent-observer-'+$c.nonce))){throw 'ROOT_PATH_MISMATCH'};if(([IO.File]::ReadAllText((Join-Path $c.root '.owner'))) -cne $c.marker){throw 'OWNER_MISMATCH'};if(([IO.File]::GetAttributes($c.root)-band[IO.FileAttributes]::ReparsePoint)-ne 0){throw 'ROOT_REPARSE'} }
"""
CREATE=r"""if($PSVersionTable.PSVersion.Major-ne 5 -or $PSVersionTable.PSVersion.Minor-ne 1){throw 'EXPECTED_DESKTOP51'}
if($c.root -cne (Join-Path ([IO.Path]::GetFullPath($env:TEMP)) ('FutureClassroom-r75-parent-observer-'+$c.nonce)) -or (Test-Path -LiteralPath $c.root)){throw 'FRESH_EXACT_ROOT_REQUIRED'}
$before=Registration;if($before.count-ne 0 -or $before.productState-ne -1){throw 'BEFORE_PRODUCT_NOT_ABSENT'}
$baseline=Facts
[IO.Directory]::CreateDirectory($c.root)|Out-Null;$acl=[Security.AccessControl.DirectorySecurity]::new();$acl.SetAccessRuleProtection($true,$false)
$id=[Security.Principal.WindowsIdentity]::GetCurrent();try{$sid=$id.User}finally{$id.Dispose()}
foreach($s in @($sid,[Security.Principal.SecurityIdentifier]::new('S-1-5-18'),[Security.Principal.SecurityIdentifier]::new('S-1-5-32-544'))){$acl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new($s,'FullControl','ContainerInherit,ObjectInherit','None','Allow'))}
[IO.Directory]::SetAccessControl($c.root,$acl);[IO.File]::WriteAllBytes((Join-Path $c.root '.owner'),[Text.Encoding]::ASCII.GetBytes($c.marker))
@{status='CREATED_BEFORE_ABSENT';root=$c.root;nonce=$c.nonce;marker=$c.marker;beforeProduct=$before;msiexecBaseline=$baseline;powershell=$PSVersionTable.PSVersion.ToString()}|ConvertTo-Json -Depth 8 -Compress
"""
RUN=r"""CheckRoot;$msi=Join-Path $c.root 'context-probe.msi';$log=Join-Path $c.root 'msiexec.log';$context=Join-Path $c.root 'context.json'
if((Get-FileHash -LiteralPath $msi -Algorithm SHA256).Hash.ToLowerInvariant()-cne $c.sha -or [IO.File]::Exists($log) -or [IO.File]::Exists($context)){throw 'FINAL_MSI_PIN_OR_FRESH_REPORTS'}
$p=[Diagnostics.Process]::new();$p.StartInfo.FileName=(Join-Path $env:WINDIR 'System32\msiexec.exe');$p.StartInfo.Arguments=$c.commandLine;$p.StartInfo.WorkingDirectory=$c.root;$p.StartInfo.UseShellExecute=$false;$p.StartInfo.CreateNoWindow=$true
$result=@{status='MSI_EXIT_OR_PROCESS_UNKNOWN';root=$c.root;nonce=$c.nonce}
try{if(-not $p.Start()){throw 'MSI_START_FALSE'};$result.client=@{pid=$p.Id;creationFiletime=[string]$p.StartTime.ToUniversalTime().ToFileTimeUtc()};if(-not $p.WaitForExit(35000)){$result.status='MSI_CLIENT_STILL_LIVE_NO_KILL'}else{$result.status='MSI_CLIENT_ACTUAL_EXIT';$result.actualExit=$p.HasExited;$result.exitCode=$p.ExitCode}}finally{$p.Dispose()}
if([IO.File]::Exists($context)){$data=[IO.File]::ReadAllBytes($context);if($data.Length-gt 8192){throw 'CONTEXT_BOUND'};$text=[Text.Encoding]::UTF8.GetString($data);$m=[regex]::Match($text,'"ownIdentity":\{"status":"OK","processId":([0-9]+),"creationFiletime":([0-9]+)\}');if(-not $m.Success){throw 'OBSERVER_IDENTITY_MISSING'};$result.observerExpected=@{pid=[int]$m.Groups[1].Value;creationFiletime=$m.Groups[2].Value};$result.preliminaryContextSha256=(Get-FileHash -LiteralPath $context -Algorithm SHA256).Hash.ToLowerInvariant()}
$result.logPresent=[IO.File]::Exists($log)
$result.msiSha256=(Get-FileHash -LiteralPath $msi -Algorithm SHA256).Hash.ToLowerInvariant();$result|ConvertTo-Json -Depth 8 -Compress
"""
AFTER=r"""CheckRoot
$state='UNKNOWN';$live=@();$limit=[DateTime]::UtcNow.AddSeconds(8)
do{$live=@();foreach($x in (Facts)){if(-not @($c.baseline|Where-Object{$_.pid-eq $x.pid -and $_.creationFiletime-ceq $x.creationFiletime}).Count){$live+=$x}}
 $state='ABSENT';if($null-ne$c.observer){$p=$null;try{$p=[Diagnostics.Process]::GetProcessById([int]$c.observer.pid);$actual=[string]$p.StartTime.ToUniversalTime().ToFileTimeUtc();$state=if($actual-ceq $c.observer.creationFiletime){'ORIGINAL_STILL_LIVE'}else{'PID_REUSED_ORIGINAL_GONE'}}catch [ArgumentException]{$state='ABSENT'}finally{if($null-ne$p){$p.Dispose()}}}
 if($live.Count-eq 0 -and $state-ne 'ORIGINAL_STILL_LIVE'){break};[Threading.Thread]::Sleep(100)
}while([DateTime]::UtcNow-lt$limit)
$registration=Registration
@{status='ACTUAL_READONLY_AFTER';root=$c.root;nonce=$c.nonce;newMsiexec=$live;newMsiexecCount=$live.Count;observerOriginalInstance=$state;afterProduct=$registration;processesQuiescent=($live.Count-eq 0 -and $state-ne 'ORIGINAL_STILL_LIVE');baselineExistingNeverKilled=$true}|ConvertTo-Json -Depth 8 -Compress
"""
CAPTURE=r"""CheckRoot
$new=@();foreach($x in (Facts)){if(-not @($c.baseline|Where-Object{$_.pid-eq $x.pid -and $_.creationFiletime-ceq $x.creationFiletime}).Count){$new+=$x}};if($new.Count-ne 0){throw 'CAPTURE_NEW_MSI_STILL_LIVE'}
$p=$null;try{$p=[Diagnostics.Process]::GetProcessById([int]$c.observer.pid);if(([string]$p.StartTime.ToUniversalTime().ToFileTimeUtc())-ceq$c.observer.creationFiletime){throw 'CAPTURE_OBSERVER_STILL_LIVE'}}catch [ArgumentException]{}finally{if($null-ne$p){$p.Dispose()}}
$context=Join-Path $c.root 'context.json';$log=Join-Path $c.root 'msiexec.log'
foreach($f in @($context,$log)){if(([IO.File]::GetAttributes($f)-band[IO.FileAttributes]::ReparsePoint)-ne 0){throw 'CAPTURE_REPARSE'}}
$cb=[IO.File]::ReadAllBytes($context);$lb=[IO.File]::ReadAllBytes($log);if($cb.Length-gt 8192 -or $lb.Length-gt 2097152){throw 'CAPTURE_SIZE_BOUND'}
@{status='ACTUAL_FINAL_BYTES_AFTER_QUIESCENT';nonce=$c.nonce;contextBase64=[Convert]::ToBase64String($cb);logBase64=[Convert]::ToBase64String($lb)}|ConvertTo-Json -Compress
"""
CLEANUP=r"""if(-not [IO.Directory]::Exists($c.root)){@{status='ABSENT';nonce=$c.nonce}|ConvertTo-Json -Compress;exit 0};CheckRoot
if($c.safeToDelete-ne $true){throw 'UNKNOWN_LIVE_OR_REGISTRATION_RETAIN_ROOT_NO_DELETE'}
$new=@();foreach($x in (Facts)){if(-not @($c.baseline|Where-Object{$_.pid-eq $x.pid -and $_.creationFiletime-ceq $x.creationFiletime}).Count){$new+=$x}};if($new.Count-ne 0){throw 'NEW_MSI_INSTANCE_RECHECK_RETAIN_ROOT'}
if($null-ne$c.observer){$p=$null;try{$p=[Diagnostics.Process]::GetProcessById([int]$c.observer.pid);if(([string]$p.StartTime.ToUniversalTime().ToFileTimeUtc())-ceq$c.observer.creationFiletime){throw 'OBSERVER_STILL_LIVE'}}catch [ArgumentException]{}finally{if($null-ne$p){$p.Dispose()}}}
$a=Registration;if($a.count-ne 0 -or $a.productState-ne -1){throw 'REGISTRATION_UNKNOWN_RETAIN_ROOT'}
if((Get-FileHash -LiteralPath (Join-Path $c.root 'context.json') -Algorithm SHA256).Hash.ToLowerInvariant()-cne$c.contextSha -or (Get-FileHash -LiteralPath (Join-Path $c.root 'msiexec.log') -Algorithm SHA256).Hash.ToLowerInvariant()-cne$c.logSha){throw 'CAPTURED_ORIGINAL_CHANGED_RETAIN_ROOT'}
$allowed=@('.owner','context-probe.msi','msiexec.log','context.json');foreach($file in [IO.Directory]::GetFileSystemEntries($c.root)){if([IO.Path]::GetFileName($file)-cnotin$allowed -or ([IO.File]::GetAttributes($file)-band[IO.FileAttributes]::ReparsePoint)-ne 0){throw 'UNEXPECTED_MEMBER_RETAIN_ROOT'}}
$msi=Join-Path $c.root 'context-probe.msi';if([IO.File]::Exists($msi) -and (Get-FileHash -LiteralPath $msi -Algorithm SHA256).Hash.ToLowerInvariant()-cne$c.sha){throw 'UPLOADED_MSI_IDENTITY_UNKNOWN_RETAIN_ROOT'}
foreach($name in @('context.json','msiexec.log','context-probe.msi','.owner')){$file=Join-Path $c.root $name;if([IO.File]::Exists($file)){[IO.File]::Delete($file)}};[IO.Directory]::Delete($c.root,$false)
@{status='REMOVED';root=$c.root;nonce=$c.nonce;marker=$c.marker;absent=(-not [IO.Directory]::Exists($c.root));registrationRecheck=$a}|ConvertTo-Json -Depth 8 -Compress
"""
def need(v,c):
    if not v:raise AssertionError(c)
def sha(data):return hashlib.sha256(data).hexdigest()
def scoped(path):
    p=Path(path);need(p.is_absolute() and p.resolve().is_relative_to(R) and not any(x.is_symlink() for x in [p,*p.parents] if x==R or R in x.parents),'OWNED_R75_SCOPE');need(p.suffix.lower() not in {'.key','.pfx','.p12'},'NO_PRIVATE_CONTENT');return p
def fact(path):
    p=scoped(path);need(p.is_file(),'REGULAR_INPUT');h=hashlib.sha256();size=0
    with p.open('rb') as f:
        while b:=f.read(1024*1024):h.update(b);size+=len(b)
    return dict(path=str(p),bytes=size,sha256=h.hexdigest())
def remote(script,config):
    if script!=RUN:config={k:v for k,v in config.items() if k!='commandLine'}
    packed=json.dumps(config,separators=(',',':')).replace("'","''")
    source=("$ErrorActionPreference='Stop';$c='"+packed+"'|ConvertFrom-Json;"+COMMON+script).encode('utf-8')
    bootstrap="$ErrorActionPreference='Stop';[Console]::InputEncoding=[Text.UTF8Encoding]::new($false);[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false);$s=[Console]::In.ReadToEnd();& ([ScriptBlock]::Create($s))"
    encoded=base64.b64encode(bootstrap.encode('utf-16-le')).decode();need(len(encoded)<=1200,'SHORT_STDIN_BOOTSTRAP')
    return dict(command=['ssh','-i',IDENTITY,'-o','BatchMode=yes','-o','ConnectTimeout=8',HOST,'powershell.exe','-NoLogo','-NoProfile','-NonInteractive','-EncodedCommand',encoded],input=source)
def registration_absent(row):need(row['status']=='ACTUAL_READONLY_CALLER_AND_MACHINE_CONTEXTS' and row['product']==PRODUCT and row['count']==0 and row['productState']==-1 and row['contexts']==7 and row['userSidArgument']=='' and row['otherUsers']==row['cache']=='NOT_QUERIED','LIMITED_ACTUAL_PRODUCT_ABSENCE')
def main():
    controller_began=time.monotonic()
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--report',type=Path,required=True);p.add_argument('--execute',action='store_true');a=p.parse_args()
    need((R/'.owner').read_bytes()==b'future-classroom-owned-r75-installer-maintenance/v1\n' and (D/'.owner').read_bytes()==b'future-classroom-owned-r75-context-observer-runtime-preparation/v1\n','EXACT_OWNERS');report=scoped(a.report);need(report.parent.resolve().is_relative_to(E) and not report.exists(),'EXCLUSIVE_EVIDENCE_OUTPUT')
    if not a.execute:print(json.dumps(dict(status='PREPARED_QUIET_TEST_ONLY_MSI_NOT_EXECUTED',finalMsiSha256=MSI_SHA,overallBudgetSeconds=120,cleanupReserveSeconds=20,expectedExit=1603,productionOrObserverRuntime='NOT_RUN')));return 0
    need(fact(STATIC)['sha256']==STATIC_SHA and fact(MSI)['sha256']==MSI_SHA and fact(CLI)['sha256']==CLI_SHA and fact(LEDGER)['sha256']==LEDGER_SHA,'EXACT_ACTUAL_STATIC_CLI_LEDGER_FINAL_MSI')
    static=json.loads(STATIC.read_bytes());cli=json.loads(CLI.read_bytes());ledger=json.loads(LEDGER.read_bytes());need(static['status']=='PASS_LOCAL_CONTEXT_TYPE19_IDT_BUILD_EXPORT_ONLY_NOT_WINDOWS' and static['tableCheck']['status']=='PASS_EXPORTED_PROBE_TOPOLOGY_ONLY' and static['msi']==fact(MSI) and static['originalsUnchanged'] is True and cli['status']=='ACTUAL_HARNESS_CLI_PARSE_PASS_NOT_OBSERVER_RUNTIME' and cli['cleanup']=='PASS','ADMITTED_ACTUAL_STATIC_FINAL_AND_LIST')
    inputs={str(x):fact(x) for x in (STATIC,MSI,CLI,LEDGER)}
    for row in static['sourceAfter']:
        path=R/'worktree'/row['path'];f=fact(path);need(f['bytes']==row['bytes'] and f['sha256']==row['sha256'],'EIGHT_SOURCE_GUARDS_UNCHANGED');inputs[str(path)]=f
    need(len(static['sourceAfter'])==8,'EIGHT_SOURCE_GUARDS')
    for row in ledger['originalArtifactIdentities']:
        need(fact(row['path'])==row,'ORIGINAL24_PE_UNCHANGED');inputs[row['path']]=row
    need(len(ledger['originalArtifactIdentities'])==24,'ALL24_ORIGINAL_PE')
    config=dict(root=cli['remoteRoot'],nonce=cli['nonce'],marker=MARKER,sha=MSI_SHA,product=PRODUCT);remote_msi=config['root']+'\\context-probe.msi';remote_log=config['root']+'\\msiexec.log';msi_args=['/i',remote_msi,'/qn','/norestart','/L*v',remote_log];config['commandLine']=subprocess.list2cmdline(msi_args)
    value=dict(status='RUNNING',startedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),originalOverallBudgetSeconds=120,cleanupReservedSeconds=20,originalClientWaitSeconds=35,expectedExitCode=1603,finalMsi=fact(MSI),fixtureLedger=fact(LEDGER),cliProof=fact(CLI),sourceGitBase=ledger['sourceGitBase'],candidateSourceProducerSha256=ledger['candidateSourceProducerSha256'],candidateSourceFreezeSha256=ledger['candidateSourceFreezeSha256'],inputIdentities=inputs,remoteRoot=config['root'],nonce=config['nonce'],marker=MARKER,msiArguments=msi_args,operations=[],cleanup='NOT_RUN',scope='Owned test-only final observer MSI quiet Type2 then intentional Type19 abort before Initialize. Not product installer/gate/Pending/Run or production authorization. Exact source/Binary provenance already static verified. No cache/registration removal or unknown process kill.')
    with report.open('x') as f:json.dump(value,f,indent=2);f.write('\n')
    began=controller_began;safe=False;created=False;passed=False;baseline=[];observer=None;context_raw=None;log_raw=None
    def call(label,cmd,limit,cleanup=False):
        available=120-(time.monotonic()-began)-(0 if cleanup else 20);need(available>0,'ORIGINAL_BUDGET_WITH_CLEANUP_RESERVE');t=time.monotonic();code=None;out=b'';err=b'';timed=False
        command=cmd['command'] if isinstance(cmd,dict) else cmd;feed=cmd['input'] if isinstance(cmd,dict) else None
        try:r=subprocess.run(command,input=feed,capture_output=True,timeout=min(limit,available));code=r.returncode;out=r.stdout;err=r.stderr
        except subprocess.TimeoutExpired as error:out=error.stdout or b'';err=error.stderr or b'';timed=True
        step=dict(label=label,command=command,actualLocalParentReturned=not timed,exitCode=code,timedOut=timed,elapsedSeconds=time.monotonic()-t)
        if feed is not None:step['stdin']=dict(bytes=len(feed),sha256=sha(feed),base64=base64.b64encode(feed).decode())
        for name,data in (('stdout',out),('stderr',err)):
            file=report.with_name(report.stem+'-'+label+'.'+name+'.log')
            with file.open('xb') as f:f.write(data)
            step[name]=dict(**fact(file),base64=base64.b64encode(data).decode())
        value['operations'].append(step);need(code==0 and not timed,'ACTUAL_TRANSPORT_EXIT0_'+label);return json.loads(out) if label!='02-scp-final-msi' else None
    try:
        before=call('01-create-before',remote(CREATE,config),20);created=True;value['before']=before;registration_absent(before['beforeProduct']);baseline=before['msiexecBaseline'];need(before['root']==config['root'] and before['nonce']==config['nonce'] and before['marker']==MARKER and before['status']=='CREATED_BEFORE_ABSENT','EXACT_FRESH_OBSERVER_ROOT')
        call('02-scp-final-msi',['scp','-i',IDENTITY,'-o','BatchMode=yes','-o','ConnectTimeout=8',str(MSI),HOST+':'+remote_msi.replace('\\','/')],25)
        actual=call('03-quiet-final-msi',remote(RUN,config),50);value['actualMsi']=actual;observer=actual.get('observerExpected')
        after=call('04-readonly-after',remote(AFTER,dict(config,baseline=baseline,observer=observer)),15);value['after']=after;registration_absent(after['afterProduct'])
        need(after['processesQuiescent'] is True and after['newMsiexecCount']==0 and after['newMsiexec']==[] and after['observerOriginalInstance'] in ('ABSENT','PID_REUSED_ORIGINAL_GONE'),'ORIGINAL_OBSERVER_AND_NEW_MSI_PROCESSES_RETURNED')
        safe=True
        need(actual['status']=='MSI_CLIENT_ACTUAL_EXIT' and actual['actualExit'] is True and actual['exitCode']==1603 and actual['msiSha256']==MSI_SHA and actual['nonce']==config['nonce'],'ACTUAL_EXPECTED_QUIET_ABORT1603')
        need(observer is not None,'ACTUAL_OBSERVER_IDENTITY_REQUIRED_BEFORE_CAPTURE')
        captured=call('05-capture-after-quiescent',remote(CAPTURE,dict(config,baseline=baseline,observer=observer)),10);need(captured['status']=='ACTUAL_FINAL_BYTES_AFTER_QUIESCENT' and captured['nonce']==config['nonce'],'FINAL_QUIESCENT_CAPTURE')
        context_raw=base64.b64decode(captured['contextBase64'],validate=True);log_raw=base64.b64decode(captured['logBase64'],validate=True);need(len(context_raw)<=8192 and len(log_raw)<=2*1024*1024 and sha(context_raw)==actual['preliminaryContextSha256'],'EXACT_BOUNDED_FINAL_CONTEXT_AND_LOG')
        for suffix,data,key in (('-context.json',context_raw,'contextOriginal'),('-msiexec.log',log_raw,'msiLogOriginal')):
            file=report.with_name(report.stem+suffix)
            with file.open('xb') as f:f.write(data)
            value[key]=fact(file)
        need(context_raw is not None and log_raw is not None and observer is not None,'ACTUAL_CONTEXT_AND_VERBOSE_LOG_REQUIRED');context=json.loads(context_raw);value['contextFacts']=context
        need(context['scope']=='READ_ONLY_TEST_FACTS_NOT_INSTALLER_ACCEPTANCE' and context['nonce']==config['nonce'] and context['ownIdentity']['status']=='OK' and context['ownIdentity']['processId']==observer['pid'] and str(context['ownIdentity']['creationFiletime'])==observer['creationFiletime'] and context['ownProcessToken']['status']=='OK' and context['ownImage']['status']=='OK' and context['releaseProof']=='REQUIRES_ACTUAL_TEST_PROCESS_EXIT0','EXACT_ACTUAL_OBSERVER_FACTS_AND_ORIGINAL_IDENTITY')
        need(re.fullmatch('[0-9a-f]+',context['ownProcessToken']['tokenUserSidHex']) and context['parent'].get('status')=='OPENED_LIMITED_QUERY_SYNC' and context['parent']['identity']['status']=='OK' and context['parent']['processToken']['status']=='OK' and context['parent']['image']['status']=='OK' and context['consoleWindowPresent'] is False,'ACTUAL_PARENT_PID_CREATION_IMAGE_SID_FACTS_AND_NO_CONSOLE')
        text=log_raw.decode('utf-16') if log_raw.startswith((b'\xff\xfe',b'\xfe\xff')) else log_raw.decode('utf-8-sig')
        observer_ends=list(re.finditer(r'(?m)^Action ended [^\r\n]*: FcObserveParent\. Return value 1\.[ \t]*\r?$',text));abort_ends=list(re.finditer(r'(?m)^Action ended [^\r\n]*: FcAbortBeforeInitialize\. Return value 3\.[ \t]*\r?$',text))
        need(len(observer_ends)==len(abort_ends)==1 and observer_ends[0].start()<abort_ends[0].start() and ABORT in text,'ACTUAL_MSI_OBSERVER_SUCCESS_THEN_INTENTIONAL_TYPE19_ABORT')
        need(not re.search(r'(?m)^Action start [^\r\n]*: (?:InstallInitialize|WriteRegistryValues|RegisterProduct|PublishProduct|InstallFiles|StartServices)\.',text),'NO_INITIALIZE_OR_EFFECT_ACTION_STARTED')
        need(inputs=={name:fact(name) for name in inputs},'SOURCE24_ARTIFACTS_AND_STATIC_INPUTS_UNCHANGED');value['inputsUnchanged']=True;passed=True
    except Exception as error:value.update(status='FAIL_QUIET_OBSERVER_OR_EVIDENCE',error=dict(kind=type(error).__name__,code=str(error) if isinstance(error,AssertionError) else 'TRANSPORT_CAPTURE_OR_COM_API_FAILURE'))
    finally:
        try:
            cleaned=call('99-owned-cleanup',remote(CLEANUP,dict(config,baseline=baseline,observer=observer,safeToDelete=safe and context_raw is not None and log_raw is not None,contextSha=sha(context_raw) if context_raw is not None else '',logSha=sha(log_raw) if log_raw is not None else '')),20,cleanup=True);value['cleanupActual']=cleaned
            need(cleaned['nonce']==config['nonce'] and cleaned['status'] in ('ABSENT','REMOVED'),'OWNED_CLEANUP_RESULT')
            if created:need(cleaned['status']=='REMOVED' and cleaned['absent'] is True and cleaned['marker']==MARKER,'CREATED_ROOT_ACTUALLY_REMOVED')
            value['cleanup']='PASS'
        except Exception as error:value.update(cleanup='FAIL_OR_UNKNOWN_ROOT_RETAINED',cleanupError=dict(kind=type(error).__name__,code=str(error) if isinstance(error,AssertionError) else 'LIMITED_ROOT_OWNER_REVIEW_REQUIRED_NO_BLIND_DELETE'))
    elapsed=time.monotonic()-began
    if passed and value['cleanup']=='PASS' and elapsed<=120:value['status']='PASS_R75_READONLY_CONTEXT_MSI_EXPECTED_ABORT_ONLY'
    elif value['status']=='RUNNING':value['status']='FAIL_QUIET_OBSERVER_CLEANUP_OR_TOTAL_BUDGET'
    value.update(elapsedSeconds=elapsed,completedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),actualControllerReturned=True,qualification='MSI engine verbose action-success evidence and original observer instance disappearance support this test-only run; no raw independent observer process exit-code API capture is invented. Reported parent/SID topology remains observed facts, never production auth/Begin/End acceptance. Product absence limited to caller per-user contexts plus machine; other users/cache not queried or deleted.')
    report.write_text(json.dumps(value,indent=2)+'\n');print(json.dumps(dict(status=value['status'],cleanup=value['cleanup'],report=fact(report))));return 0 if value['status']=='PASS_R75_READONLY_CONTEXT_MSI_EXPECTED_ABORT_ONLY' else 1
if __name__=='__main__':raise SystemExit(main())
