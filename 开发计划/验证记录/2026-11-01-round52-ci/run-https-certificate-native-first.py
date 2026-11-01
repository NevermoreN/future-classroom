#!/usr/bin/env python3
"""Root-operated owned CurrentUser MY native query; no network listener/browser.

Preparation is local. Real execution requires explicit --execute and root's
exclusive Windows slot. No private key is read/exported; --identity is passed
as an existing path to ssh/scp only. Complete controls and originals persist.
"""
import argparse
import base64
import hashlib
import importlib.util
import json
from pathlib import Path
import re
import subprocess
import sys
import uuid

sys.dont_write_bytecode = True
ROOT = Path('/workspace/共享/Eliora/未来教室计划')
PREPARER = Path('/tmp/fc-round52-ci-evidence/certificate-fixture-preparation-first/prepare-certificate-fixture-third.py')
SOURCES = {
    'windows-tls-business-probe.py': '0cb59751f0bd1e79dd86de9eb26ac780e264a969d74cbdbe074be988210c7208',
    'windows-tls-probe.py': '1a91ac72ccb3fff0a08d9aa98222a82019af86a8d1b903e9fb10eb69ad8d94df',
    'windows-probe.py': 'f176a022bf90565be7074f53a2d502d6519afd1748cdd2022f608a217d485c14',
}
PREPARER_SHA = 'b652832f1ca4fe8c86f7e6fb105e232cc042f4a02d2df27e8493427365240df3'


def require(value, message):
    if not value:
        raise RuntimeError(message)


def sha(data):
    return hashlib.sha256(data).hexdigest()


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def dependencies():
    for name, expected in SOURCES.items():
        require(sha((ROOT/'scripts'/name).read_bytes()) == expected, 'main ownership source changed: '+name)
    require(sha(PREPARER.read_bytes()) == PREPARER_SHA, 'frozen third preparer SHA changed')
    preparer = load('fc_certificate_preparer_third', PREPARER)
    business = load('fc_certificate_existing_cleanup', ROOT/'scripts/windows-tls-business-probe.py')
    ownership, _ = preparer.load_ownership()
    require(ownership.COMMON == business.COMMON and ownership.SETUP == business.SETUP
            and ownership.CAPTURE == business.CAPTURE and ownership.CLEANUP == business.CLEANUP,
            'frozen certificate ownership differs from current reused owner')
    return preparer, business


def bytes_fact(data):
    return {'bytes': len(data), 'sha256': sha(data), 'base64': base64.b64encode(data).decode('ascii')}


def validate_ast(result, expected):
    require(isinstance(result,dict) and set(result)=={'status','scope','scripts'}
            and result['status']=='PASS' and result['scope']=='AST_ONLY_NO_FIXTURE_OR_EXE_EXECUTION', 'AST result shape/scope failed')
    rows=result['scripts']; require(isinstance(rows,list) and len(rows)==len(expected), 'AST script count changed')
    actual={}
    for row in rows:
        require(isinstance(row,dict) and set(row)=={'name','errorCount','sha256'},'AST row shape failed')
        require(row['name'] not in actual and type(row['errorCount'])is int and row['errorCount']==0,'duplicate AST row/error')
        actual[row['name']]=row['sha256']
    require(actual=={name:entry['sha256']for name,entry in expected.items()}, 'remote script source SHA mismatch')


def recorded_remote(tls, host, identity):
    class RecordedRemote(tls.Remote):
        def __init__(self):
            super().__init__(host, identity)
            self.transport_runs = []

        def invoke(self, command, timeout, source=None):
            entry = {'command': command, 'timeoutSeconds': timeout}
            if source is not None:
                entry['powershellSourceUtf8'] = bytes_fact(source.encode('utf-8'))
            try:
                result = subprocess.run(command, capture_output=True, timeout=timeout)
                entry.update(exitCode=result.returncode, stdout=bytes_fact(result.stdout), stderr=bytes_fact(result.stderr))
            except subprocess.TimeoutExpired as error:
                entry.update(status='TIMEOUT', stdout=bytes_fact(error.stdout or b''), stderr=bytes_fact(error.stderr or b''))
                self.transport_runs.append(entry)
                raise
            except OSError as error:
                entry.update(status='LOCAL_TRANSPORT_START_FAILED', errorKind=type(error).__name__)
                self.transport_runs.append(entry)
                raise
            self.transport_runs.append(entry)
            require(result.returncode == 0, 'recorded remote invocation failed; full original streams retained')
            return result

        def powershell(self, source, timeout=60):
            return self.invoke(self.command(source), timeout, tls.COMMON+source).stdout.decode('utf-8','strict').strip().lstrip('\ufeff')

        def run_file(self, path, timeout):
            # Preserve original exact run_file byte/exit handling and its own
            # script_runs; additionally retain complete command/source binding.
            before = len(self.script_runs)
            try:
                return super().run_file(path, timeout)
            finally:
                if len(self.script_runs) > before:
                    self.script_runs[-1]['command'] = self.command_file(path)
                    self.script_runs[-1]['timeoutSeconds'] = timeout

        def upload(self, local, directory):
            require(local.is_file(), 'owned upload missing')
            self.invoke(['scp','-q',*self.options,str(local),self.host+':'+directory.replace('\\','/')+'/'+local.name],60)

        def download(self, remote_path, local):
            self.invoke(['scp','-q',*self.options,self.host+':'+remote_path.replace('\\','/'),str(local)],60)

    return RecordedRemote()


def execute(args, preparer, business):
    tls = business._tls
    exe_fact = tls.file_identity(args.exe)
    require(exe_fact['bytes'] <= 128*1024*1024, 'bounded EXE exceeded128MiB')
    with args.exe.open('rb') as stream:
        head = stream.read(64)
        require(head[:2] == b'MZ' and len(head) == 64, 'missing PE executable')
        stream.seek(int.from_bytes(head[60:64],'little'))
        require(stream.read(6) == b'PE\0\0L\x01', 'diagnostic must be x86 PE')
    token = uuid.uuid4().hex
    marker = 'FutureClassroom-Test-TLS-'+token
    controls = args.report.parent/(args.report.stem+'-'+token+'-controls')
    controls.mkdir(parents=True,exist_ok=False)
    (controls/'.owner.json').write_text(json.dumps({'marker':marker,'purpose':'R50 owned native certificate controls and originals','exe':exe_fact},indent=2)+'\n')
    remote = recorded_remote(tls,args.host,args.identity)
    report = {'schemaVersion':1,'status':'FAIL','cleanup':'NOT_RUN','scope':'Owned localhost certificate inspection via hidden native controls and existing2workers; no listener/TLS/browser',
              'marker':marker,'exe':exe_fact,'runnerSourceSha256':sha(Path(__file__).read_bytes()),'preparerSourceSha256':PREPARER_SHA,
              'ownershipSourceSha256':SOURCES,'contractSha256':preparer.CONTRACT_SHA,'localControls':str(controls),
              'productionListener':'NOT_ENABLED','readiness':'NOT_GRANTED','win7':'NOT_RUN','win10':'NOT_RUN',
              'wrongHost':'NOT_EXERCISED','heldNativeCancellation':'NOT_EXERCISED','missingReference':'NOT_EXERCISED',
              'privateKeyExported':False,'sshPrivateKeyReadByPython':False,'originalSupervisorBudgetMillis':90000}
    directory, uploaded, phase = None,set(),'create-owned-root'
    args.report.parent.mkdir(parents=True,exist_ok=True)
    try:
        literal = tls.ps_literal
        directory = remote.powershell('$root=Join-Path $env:TEMP '+literal(marker)
            +';New-Item -ItemType Directory -Path $root|Out-Null;[IO.File]::WriteAllText((Join-Path $root ".tls-probe-root"),'+literal(marker)+');$root')
        require(directory.endswith('\\'+marker) and '\n' not in directory and '\r' not in directory,'root identity mismatch')
        report['remoteRoot'] = directory
        phase='prepare-and-upload-owned-controls'
        initial = controls/'before-public-reference'
        before_controls=preparer.prepare(directory,marker,args.exe.name,None,initial)
        report['controlsBeforeReference'] = str(initial/'preparation.json')
        for name in ('cleanup','stop','capture','setup'):
            remote.upload(initial/('tls-'+name+'.ps1'),directory); uploaded.add(name)
        remote.upload(initial/'parse-owned-controls.ps1',directory)
        # Parse without running any certificate or app action. No filenames are
        # derived from unrelated certificates or existing user directories.
        phase='ast-before-fixture'
        ast_result = json.loads(remote.powershell('& powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File '+literal(directory+'\\parse-owned-controls.ps1')+' -OwnedScriptRoot '+literal(directory)+";if($LASTEXITCODE-ne 0){throw 'Owned AST parse process failed'}",timeout=15))
        validate_ast(ast_result,before_controls['scripts'])
        report['controlAstBeforeFixture']=ast_result
        phase='upload-source-bound-executable'
        remote.upload(args.exe,directory)
        uploaded_sha=remote.powershell('(Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path '+literal(directory)+' '+literal(args.exe.name)+')).Hash.ToLowerInvariant()')
        require(uploaded_sha==exe_fact['sha256']==tls.file_identity(args.exe)['sha256'],'EXE identity changed')
        phase='create-owned-certificate-and-key'
        fixture=json.loads(remote.run_file(directory+'\\tls-setup.ps1',timeout=45))
        thumbprint=fixture.get('thumbprint')
        require(type(thumbprint)is str and re.fullmatch(r'[A-Fa-f0-9]{40}',thumbprint),'setup public thumbprint invalid')
        require(fixture.get('privateKeyExported')is False and fixture.get('keyName')==marker+'-key'
                and fixture.get('provider')=='Microsoft Software Key Storage Provider','fixture ownership mismatch')
        report['fixture']={key:value for key,value in fixture.items() if key!='publicPem'}
        phase='prepare-and-parse-public-reference-launch'
        final=controls/'after-public-reference'
        after_controls=preparer.prepare(directory,marker,args.exe.name,thumbprint,final)
        report['controlsAfterReference']=str(final/'preparation.json')
        remote.upload(final/'tls-launch.ps1',directory)
        remote.upload(final/'parse-owned-controls.ps1',directory)
        ast_result=json.loads(remote.powershell('& powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File '+literal(directory+'\\parse-owned-controls.ps1')+' -OwnedScriptRoot '+literal(directory)+";if($LASTEXITCODE-ne 0){throw 'Owned AST parse process failed'}",timeout=15))
        validate_ast(ast_result,after_controls['scripts'])
        report['controlAstWithLaunch']=ast_result
        phase='actual-hidden-native-query'
        text=remote.run_file(directory+'\\tls-launch.ps1',timeout=100)
        (controls/'native-forwarded-decoded.txt').write_text(text,encoding='utf-8')
        report['nativeValidation']=preparer.validate_transcript(text)
        report['decodedTranscriptScope']='Normalized UTF8/BOM/edge whitespace for parsing only; exact SSH stdout/stderr in remoteFileScriptRuns, exact child originals captured before cleanup'
        report['status']='PASS'
    except Exception as error:
        report['error']=tls.failure(error,phase)
    finally:
        if directory is not None:
            business.cleanup_owned(remote,directory,marker,uploaded,args.report,token,report)
        report['remoteFileScriptRuns']=remote.script_runs
        report['transportInvocations']=remote.transport_runs
        if report.get('cleanup')!='PASS': report['status']='FAIL'
        if report['status']=='PASS':
            details=report.get('cleanupDetails',{})
            checks=(report.get('ownedProcessCleanup',{}).get('ownedProcessesRemaining')==0,
                    report.get('rawPreservation',{}).get('status')=='PASS',details.get('rootAbsent')is True,
                    details.get('certificatesRemoved')==1,details.get('cngKeyExists')is False,details.get('myDigestRestored')is True,
                    details.get('myAfter')==report.get('fixture',{}).get('myBefore'))
            if not all(checks): report.update(status='FAIL',finalCleanupInvariant='Native/physical/raw/certificate/key/root proofs incomplete')
        # Exclusive write, never rewrite an older actual FAIL or PASS report.
        with args.report.open('x',encoding='utf-8') as stream: json.dump(report,stream,indent=2);stream.write('\n')
    return 0 if report['status']=='PASS' else 1


def self_test(preparer,business,output):
    output.mkdir(parents=True,exist_ok=False)
    tls=business._tls
    model=preparer.self_test()
    expected={'setup':{'sha256':'1'*64},'cleanup':{'sha256':'2'*64}}
    ast_result={'status':'PASS','scope':'AST_ONLY_NO_FIXTURE_OR_EXE_EXECUTION','scripts':[{'name':name,'errorCount':0,'sha256':entry['sha256']}for name,entry in expected.items()]}
    validate_ast(ast_result,expected)
    ast_negatives=[]
    for mutate in (lambda x:x['scripts'][0].update(sha256='3'*64),lambda x:x['scripts'][0].update(errorCount=False),lambda x:x['scripts'].append(x['scripts'][0]),lambda x:x.update(scope='EXECUTED')):
        bad=json.loads(json.dumps(ast_result));mutate(bad);ast_negatives.append(bad)
    for bad in ast_negatives:
        try:validate_ast(bad,expected)
        except RuntimeError:pass
        else:raise AssertionError('bad remote AST/source model accepted')
    results=[]
    for case in ('success','native-failed','stop-failed','capture-failed','cleanup-failed'):
        data=b'SYNTHETIC LOCAL MODEL ONLY\n'; calls=[]
        class ModelRemote:
            def run_file(self,path,timeout):
                name=path.rsplit('\\',1)[-1];calls.append(name)
                if name=='tls-stop.ps1':return json.dumps({'status':'FAIL' if case=='stop-failed' else 'PASS','ownedProcessesRemaining':0})
                if name=='tls-capture.ps1':
                    if case=='capture-failed':raise RuntimeError('synthetic capture failure')
                    return json.dumps({'tls.stdout':{'bytes':len(data),'sha256':sha(data)}})
                if case=='cleanup-failed':raise RuntimeError('synthetic cleanup failure')
                return json.dumps({'status':'PASS','rootAbsent':True,'certificatesRemoved':1,'cngKeyExists':False,'myDigestRestored':True})
            def download(self,remote_path,local):calls.append('download-verified-original');local.write_bytes(data)
        report={'status':'FAIL' if case=='native-failed' else 'PASS'}
        business.cleanup_owned(ModelRemote(),'C:\\SyntheticNotWindows','MODEL',{'setup','stop','capture','cleanup'},output/(case+'.json'),case,report)
        if case in ('stop-failed','capture-failed'):
            require('tls-cleanup.ps1'not in calls and report['cleanup']=='FAIL' and report['status']=='FAIL','uncertain cleanup owner incorrectly removed')
        elif case=='cleanup-failed':require(report['cleanup']=='FAIL' and report['status']=='FAIL','cleanup failure lost')
        else:
            require(calls==['tls-stop.ps1','tls-capture.ps1','download-verified-original','tls-cleanup.ps1'],'physical/raw/key order changed')
            require(report['cleanup']=='PASS'and report['status']==('FAIL'if case=='native-failed'else'PASS'),'native FAIL overwritten by cleanup')
        (output/(case+'.json')).write_text(json.dumps({'scope':'PURE MOCK; NO SSH/WINDOWS', 'calls':calls,'report':report},indent=2)+'\n')
        results.append(case)
    result={'status':'PURE_LOCAL_MODELS_PASS','wireModels':model,'astSourceModels':{'positive':1,'negative':len(ast_negatives)},'cleanupModels':results,'windows':'NOT_RUN','ssh':'NOT_RUN','native':'NOT_RUN'}
    (output/'result.json').write_text(json.dumps(result,indent=2)+'\n')
    return result


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--execute',action='store_true')
    parser.add_argument('--self-test',action='store_true')
    parser.add_argument('--model-output',type=Path)
    parser.add_argument('--exe',type=Path)
    parser.add_argument('--report',type=Path)
    parser.add_argument('--identity',type=Path)
    parser.add_argument('--host',default='future-classroom-win11')
    args=parser.parse_args()
    preparer,business=dependencies()
    if args.self_test:
        require(not args.execute and args.model_output is not None and not any((args.exe,args.report,args.identity)), 'model must not receive real execution arguments')
        print(json.dumps(self_test(preparer,business,args.model_output),indent=2));return 0
    require(args.execute,'root must explicitly authorize execution with --execute')
    require(args.exe and args.exe.is_file()and args.report and not args.report.exists(),'existingEXE/newreport required')
    require(re.fullmatch(r'[A-Za-z0-9_.-]+\.exe',args.exe.name),'plain executable name required')
    require(re.fullmatch(r'[A-Za-z0-9_.@:-]+',args.host)and not args.host.startswith('-'),'host alias/user@IP required')
    require(args.identity is None or args.identity.is_file(),'optional identity path missing; contents are never read')
    return execute(args,preparer,business)


if __name__=='__main__':
    raise SystemExit(main())
