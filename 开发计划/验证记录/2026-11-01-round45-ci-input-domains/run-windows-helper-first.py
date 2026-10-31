#!/usr/bin/env python3
"""Run only the frozen R45 helper fixture, in-memory stdin; never CI entrypoint."""
import base64,datetime,hashlib,json,pathlib,subprocess,time,uuid
EV=pathlib.Path('/tmp/fc-round45-ci-input-domains-evidence')
fixture=EV/'input-domain-powershell-fixture-first.ps1'; data=fixture.read_bytes()
assert hashlib.sha256(data).hexdigest()=='ab2942c7793d004719cd457055163a9868b7d6b995a81b861407d2d73ee9d849'
nonce='r45-input-helpers-'+uuid.uuid4().hex
wrapper=r'''
$ErrorActionPreference='Stop'; $ProgressPreference='SilentlyContinue';
[Console]::InputEncoding=[Text.UTF8Encoding]::new($false);[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false);$OutputEncoding=[Console]::OutputEncoding;
$nonce='__NONCE__';$self=[Diagnostics.Process]::GetCurrentProcess();
$identity=@{nonce=$nonce;pid=$PID;startTicks=$self.StartTime.ToUniversalTime().Ticks;image=$self.MainModule.FileName};
[Console]::WriteLine('OWNER '+($identity|ConvertTo-Json -Compress));$self.Dispose();
$ok=$false;
try {$source=[Console]::In.ReadToEnd(); & ([ScriptBlock]::Create($source)); $ok=$true} catch {[Console]::Error.WriteLine($_.ToString());[Console]::Error.WriteLine($_.ScriptStackTrace)}
finally {[Console]::WriteLine('EXIT '+(@{nonce=$nonce;pid=$PID;completed=$true;fixtureSucceeded=$ok}|ConvertTo-Json -Compress))};
if($ok){exit 0}else{exit 1}
'''.replace('__NONCE__',nonce)
encoded=base64.b64encode(wrapper.encode('utf-16le')).decode()
command=['ssh','-T','-o','BatchMode=yes','-o','ConnectTimeout=10','future-classroom-win11','powershell.exe -NoLogo -NoProfile -NonInteractive -EncodedCommand '+encoded]
report={'status':'STARTED_NOT_VALIDATED','nonce':nonce,'fixture':{'path':str(fixture),'bytes':len(data),'sha256':hashlib.sha256(data).hexdigest()},'command':command,'wrapper':wrapper,'stdin':{'bytes':len(data),'sha256':hashlib.sha256(data).hexdigest()},'timeoutSeconds':60,'startedUtc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'ciEntrypointExecuted':False,'compilerExecuted':False}
reportpath=EV/'windows-input-helpers-first.json'
if reportpath.exists():raise RuntimeError('report already exists')
start=time.monotonic()
with (EV/'windows-input-helpers-first.stdout.log').open('xb') as out,(EV/'windows-input-helpers-first.stderr.log').open('xb') as err:
    p=subprocess.Popen(command,stdin=subprocess.PIPE,stdout=out,stderr=err)
    try:
        p.communicate(data,timeout=60)
        report['exitCode']=p.returncode
    except subprocess.TimeoutExpired:
        report['timeout']=True;p.kill();p.wait();report['exitCode']=p.returncode
report['durationSeconds']=time.monotonic()-start
for name in ['stdout','stderr']:
    raw=(EV/f'windows-input-helpers-first.{name}.log').read_bytes()
    report[name]={'bytes':len(raw),'sha256':hashlib.sha256(raw).hexdigest(),'base64':base64.b64encode(raw).decode(),'utf8':raw.decode('utf8','replace')}
try:
    lines=report['stdout']['utf8'].splitlines()
    owners=[json.loads(x[6:]) for x in lines if x.startswith('OWNER ')]; exits=[json.loads(x[5:]) for x in lines if x.startswith('EXIT ')]
    report['processOwner']=owners[0] if len(owners)==1 else None
    report['remoteExitRecord']=exits[0] if len(exits)==1 else None
    payload=[json.loads(x) for x in lines if x.startswith('{')]
    report['result']=payload[0] if len(payload)==1 else None
    report['status']='PASS' if report['exitCode']==0 and not report.get('timeout') and len(exits)==1 and exits[0]['nonce']==nonce and exits[0]['fixtureSucceeded'] and report['result']['status']=='PASS_POWERSHELL_HELPERS_ONLY' and report['result']['root_removed'] else 'FAIL'
except Exception as error:report['status']='FAIL';report['parseError']=type(error).__name__
with reportpath.open('x') as f:json.dump(report,f,ensure_ascii=False,indent=2);f.write('\n')
print(json.dumps({'status':report['status'],'exitCode':report['exitCode'],'durationSeconds':report['durationSeconds'],'report':str(reportpath),'tests':len(report.get('result',{}).get('checks',[])) if report.get('result') else None},ensure_ascii=False))
