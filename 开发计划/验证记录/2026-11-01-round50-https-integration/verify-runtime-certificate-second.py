#!/usr/bin/env python3
"""Independent local R50 compiled-input, Win11 and certificate wire audit."""
from pathlib import Path
import base64, hashlib, json, re, struct, subprocess, sys, datetime
E=Path('/tmp/fc-round50-https-integration-evidence')
R=Path('/tmp/fc-round47-https-settings-worktree')
NORMAL='b6368400a58b87d4725cbd664b5bde88a4f51927a74656e5c566010e8bbf6930'
CONTRACT=Path('/tmp/fc-round49-https-settings-ui-evidence/native-probe-contract-final-first.json')
CORRECTED='48fa60b98669f9b10c7ead070a519b7d3564349950d762fc9ac4c8aa4a7aa071'
HISTORICAL='feaac367ea006f5333d040b267f4269c26e442a2034f662f69d3c54ff89ce296'
SUMMARY=re.compile(r'^test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out; finished in ([0-9.]+)s$',re.M)

def need(ok,label):
    if not ok:raise ValueError(label)
def sha(b):return hashlib.sha256(b).hexdigest()
def load(p):return json.loads(Path(p).read_text(encoding='utf-8-sig'))
def fact(p):
    p=Path(p);need(p.is_file() and not p.is_symlink(),'missing ordinary original: '+p.name)
    b=p.read_bytes();return {'path':str(p),'bytes':len(b),'sha256':sha(b)}
def exact(p,want):
    got=fact(p);need(all(got[k]==want[k] for k in ['sha256']+(['bytes'] if 'bytes'in want else [])),'original bytes/SHA mismatch: '+Path(p).name);return got

def snapshot():
    names=set(subprocess.check_output(['git','ls-files','--cached','--others','--exclude-standard','-z'],cwd=R).decode().split('\0'))
    return {n:{k:v for k,v in fact(R/n).items() if k!='path'} for n in sorted(names) if n and ((n.startswith('crates/') and (n.endswith('.rs') or n.endswith('/Cargo.toml'))) or n in ['Cargo.toml','Cargo.lock','rust-toolchain.toml','.cargo/config.toml'])}

def validate_summary(t):
    need(t['exitCode']==0 and t['timeoutSeconds']==900,'test did not exit0 with original900 budget')
    out=t['output'];need(isinstance(out,list) and all(isinstance(v,str) for v in out),'complete test output missing')
    text='\n'.join(out);matches=list(SUMMARY.finditer(text));need(len(matches)==1,'exactly one libtest summary required')
    n,failed,ignored,measured,filtered=map(int,matches[0].groups()[:5]);need((failed,ignored,measured,filtered)==(0,0,0,0),'nonzero failed/ignored/measured/filtered')
    need(re.findall(r'^running (\d+) tests?$',text,re.M)==[str(n)],'announced test count mismatch')
    return {'name':t['name'],'sha256':t['binarySha256'],'passed':n,'failed':failed,'ignored':ignored,'filtered':filtered,'summary':matches[0].group(),'outputUtf8Bytes':len(text.encode()),'outputUtf8Sha256':sha(text.encode())}

def validate_wire(child,forwarded,contract):
    steps=contract['steps'];need(len(steps)==22 and len(set(steps))==22,'contract exact22 distinct steps')
    lines=child.decode('utf-8-sig').strip().splitlines();expected=[f'HTTPS_SETTINGS_NATIVE_STEP index={i} name={n}' for i,n in enumerate(steps)]
    actual=[v for v in lines if v.startswith('HTTPS_SETTINGS_NATIVE_STEP')];need(actual==expected,'wire ordered22 steps missing/repeated/out of order')
    observed=[v for v in lines if v.startswith('HTTPS_SETTINGS_CERTIFICATE_OBSERVATION ')];need(len(observed)==1,'one observation required')
    obs=json.loads(observed[0].split(' ',1)[1]);need(set(obs)==set(contract['observation']['jsonKeys']),'observation exact14 keys')
    need(obs['scope']=='CurrentUserMyCacheOnly' and obs['readiness']=='NOT_GRANTED','scope/readiness upgraded')
    for k in ['publicationRevision','identityRevision','settingsRevision']:need(type(obs[k])is int and obs[k]>0,'invalid observation revision')
    facts=['found','currentTime','privateKey','serverAuthEku','serverName','sslChainPolicy','cachedRevocation']
    for k in facts:
        v=obs[k];need(isinstance(v,dict) and set(v)=={'check','nativeError'} and v['check'] in ['Satisfied','Rejected','Unverified','NotChecked'],'fact shape/check invalid')
        need(v['nativeError'] is None or (type(v['nativeError'])is int and -(2**31)<=v['nativeError']<2**31),'fact signed error invalid')
    for k in ['found','currentTime','privateKey','serverAuthEku']:need(obs[k]['check']=='Satisfied','owned fixture required fact not satisfied')
    for k in ['chainTrustErrors','sslPolicyError']:need(obs[k] is None or(type(obs[k])is int and 0<=obs[k]<2**32),'unsigned native status invalid')
    footer=[v for v in lines if v.startswith('HTTPS_SETTINGS_NATIVE_PASS')];need(len(footer)==1,'one footer required')
    pattern=r'HTTPS_SETTINGS_NATIVE_PASS steps=22 shared_workers=2 original_result_query=PASS listener=NOT_ENABLED certificate_scope=CurrentUserMyCacheOnly certificate_query_count=1 readiness=NOT_GRANTED physical_shutdown=PASS isolated_cleanup=PASS elapsed_ms=([0-9]+)'
    match=re.fullmatch(pattern,footer[0]);need(match is not None and 0<int(match[1])<=90000,'footer facts/elapsed outside supervisor budget')
    need(lines.index(observed[0])==lines.index(expected[7])+1 and lines.index(observed[0])<lines.index(expected[8]) and lines[-1]==footer[0] and len(lines)==24,'observation/footer position or extra child wire')
    fwd=forwarded.decode('utf-8-sig').strip().splitlines();need(fwd[:-1]==lines,'forwarded normalized child transcript differs')
    process=re.fullmatch(r'HTTPS_CERTIFICATE_PROCESS_EXIT (.+)',fwd[-1]);need(process is not None,'actual process exit marker missing')
    exitdata=json.loads(process[1]);need(set(exitdata)=={'code'} and type(exitdata['code'])is int and exitdata['code']==0,'actual native process exit not0')
    return {'orderedSteps':steps,'exactSteps':22,'actualProcessExit':0,'elapsedMillis':int(match[1]),'observation':obs,'footer':footer[0],'wireChildLines':24,'wireForwardedLines':25,'normalization':'UTF8 optional BOM and edge whitespace/line terminators for parsing only; original raw bytes/SHA remain separately verified.'}

def checked_raw(d,label):
    need(isinstance(d,dict) and set(d)>= {'bytes','sha256','base64'},'raw container incomplete: '+label)
    b=base64.b64decode(d['base64'],validate=True);need(len(b)==d['bytes'] and sha(b)==d['sha256'],'raw Base64 bytes/SHA mismatch: '+label);return b

def completed_command(p):
    x=load(p);need(x.get('status')in ['PASS','PASS_COMMAND'] and x.get('exitCode')==0 and (x.get('actualExit')is True or isinstance(x.get('completedUtc'),str)),'outer command not terminal exit0: '+p.name)
    for k in ['stdout','stderr']:
        v=x[k];want=v if isinstance(v,dict) else {'path':v,'bytes':x[k+'Bytes'],'sha256':x[k+'Sha256']};exact(want['path'],want)
    return fact(p)

def main():
    out=E/'runtime-certificate-independent-second.json';need(not out.exists(),'fresh report required')
    report={'status':'FAIL','helper':fact(Path(__file__)),'scope':'Independent local-only completed GNU Windows compile/runtime/certificate evidence; not whole-matrix Host PASS. No SSH, Windows EXE, Cargo, Git writes, main changes or cleanup.'}
    try:
        c=load(E/'compiled-programs-preliminary-first.json');need(c['status']=='PASS_COMPLETED_GNU_COMPILE_PRODUCT_STABLE_STRICT_ONLY','preliminary scope altered')
        before=snapshot();need(len(before)==429 and before==c['sourceBefore'],'current429 differs from actually compiled source')
        matrix_bytes=(E/'full-matrix-second.json').read_bytes();matrix=json.loads(matrix_bytes)
        snap_path=E/'runtime-certificate-matrix-snapshot-second.json';need(not snap_path.exists(),'matrix snapshot exists');snap_path.write_bytes(matrix_bytes)
        need(matrix['sourceBefore']==before and not matrix.get('changedInputs'),'matrix source mismatch/changed')
        completed={v['name']:v for v in matrix['commands']}
        for old in c['completedCommands']:need(old==completed[old['name']],'preliminary completed command changed')
        command_facts=[]
        for name in ['windows-compile','windows-product','windows-strict','windows-nightly-strict']:
            v=completed[name];need(v['exitCode']==0 and v['actualExit']is True and v['timedOut']is False,'completed Windows compile/lint failed')
            command_facts.append({'name':name,'stdout':exact(v['stdout']['path'],v['stdout']),'stderr':exact(v['stderr']['path'],v['stderr']),'exitCode':0})
        artifacts=c['artifacts'];need(len(artifacts)==22 and sum(v['test']is True for v in artifacts)==21 and sum(v['test']is False for v in artifacts)==1,'22 artifacts/21 tests required')
        binaries=[];names={};normal=None
        for a in artifacts:
            v=exact(a['path'],a);exact(a['originalPath'],a);need(Path(a['path']).name==Path(a['originalPath']).name,'copied EXE basename mismatch')
            b=Path(a['path']).read_bytes();need(b[:2]==b'MZ','DOS header missing');offset=struct.unpack_from('<I',b,60)[0]
            need(b[offset:offset+4]==b'PE\0\0' and struct.unpack_from('<H',b,offset+4)[0]==0x14c and struct.unpack_from('<H',b,offset+24)[0]==0x10b,'PE32 i386 required')
            name=Path(a['path']).name;need(name not in names,'duplicate EXE basename');names[name]=a;binaries.append(v)
            if not a['test']:normal=a
        need(normal['sha256']==NORMAL and Path(normal['path']).name=='fc-bootstrap.exe','normal EXE does not bind frozen b636 source')
        meta=exact(c['metadata']['path'],c['metadata']);rows=[json.loads(v)for v in Path(meta['path']).read_text().splitlines()];need(len(rows)==21,'metadata21 required')
        expected=[]
        for v in rows:
            need(v.get('reason')=='compiler-artifact' and v['profile']['test']is True,'non-test metadata row')
            name=Path(v['executable']).name;a=names[name];need(a['test']is True and v['executable']==a['path'] and v['target']==a['target'],'metadata identity/target mismatch');expected.append(name)
        runtime=load(E/'windows-gnu-runtime-first.json');need(runtime['status']=='PASS' and runtime['cleanup']=='PASS' and runtime['win7']=='NOT_VALIDATED' and runtime['testTimeoutSeconds']==900 and 'timedOutTest'not in runtime,'runtime not fullPASS original900/cleanup')
        need(runtime['exeSha256']==NORMAL and len(runtime['tests'])==21 and [v['name']for v in runtime['tests']]==expected,'runtime exact metadata21 order mismatch')
        upload={v['name']:v['sha256']for v in runtime['uploadedArtifacts']};need(len(upload)==22 and upload=={n:a['sha256']for n,a in names.items()},'uploaded22 SHA mismatch')
        groups=[]
        for t in runtime['tests']:need(t['binarySha256']==names[t['name']]['sha256'],'test executed different EXE');groups.append(validate_summary(t))
        need(sum(v['passed']for v in groups)==1481,'full runtime count not1481')
        need(runtime['selfCheck']['exitCode']==runtime['system']['exitCode']==runtime['native']['exitCode']==runtime['native']['hostExitCode']==0 and runtime['native']['sessionId']>0,'self/system/hiddennative failed')
        runtime_command=completed_command(E/'windows-gnu-runtime-first.command.json')
        native_flows=[]
        for kind,key,n in [('public','publicDesktopNativeStatus',20),('visual','subjectVisualNativeStatus',22)]:
            p=E/f'windows-{kind}-native-first.json'
            if not p.exists():p=E/f'windows-gnu-{kind}-native-first.json'
            x=load(p);need(x['status']=='PASS' and x['cleanup']=='PASS' and x['exeSha256']==NORMAL and x[key]=='PASS' and x[key.replace('Status','Steps')]==n,'public/visual actual normal flow mismatch')
            native_flows.append({'kind':kind,'steps':n,'report':fact(p),'command':completed_command(p.with_suffix('.command.json'))})
        cert=load(E/'windows-https-certificate-native-first.json');need(cert['status']=='PASS'and cert['cleanup']=='PASS'and cert['exe']['sha256']==NORMAL,'certificate normal/source/status mismatch')
        need(cert['productionListener']=='NOT_ENABLED'and cert['readiness']=='NOT_GRANTED'and cert['privateKeyExported']is False and cert['sshPrivateKeyReadByPython']is False and cert['originalSupervisorBudgetMillis']==90000,'certificate scope upgraded')
        contract=load(CONTRACT);need(fact(CONTRACT)['sha256']==cert['contractSha256'],'historical wire contract SHA mismatch')
        correction=load(E/'ui-probe-serialization-second-integration.json');probe='crates/fc-bootstrap/src/application/https_settings_probe.rs'
        need(contract['source']['sha256']==HISTORICAL and correction['beforeSha256']==HISTORICAL and correction['afterSha256']==CORRECTED and before[probe]['sha256']==CORRECTED,'historical feaa/current48fa source boundary mixed')
        controls=[]
        for key,astkey,expectedn in [('controlsBeforeReference','controlAstBeforeFixture',4),('controlsAfterReference','controlAstWithLaunch',5)]:
            prep=load(cert[key]);need(prep['status']=='PREPARED_NOT_RUN'and prep['r49Contract']==cert['contractSha256']and prep['marker']==cert['marker']and prep['launchBudgetMillis']==90000,'preparation source/scope mismatch')
            ast=cert[astkey];need(ast['status']=='PASS'and ast['scope']=='AST_ONLY_NO_FIXTURE_OR_EXE_EXECUTION'and len(ast['scripts'])==expectedn,'actual AST result mismatch')
            expected_sha={}
            for name,v in prep['scripts'].items():
                raw=base64.b64decode(v['sourceBase64'],validate=True);need(len(raw)==v['bytes']and sha(raw)==v['sha256']and Path(v['path']).read_bytes()==raw,'prepared/uploaded control original bytes mismatch');expected_sha[name]=v['sha256']
            need({v['name']:v['sha256']for v in ast['scripts']}==expected_sha and all(type(v['errorCount'])is int and v['errorCount']==0 for v in ast['scripts']),'remote AST SHA/errors differ from local controls')
            controls.append({'preparation':fact(Path(cert[key])),'actualRemoteAST':ast})
        raws={};rawfacts=[]
        need(cert['rawPreservation']['status']=='PASS'and set(cert['remoteRawEvidence'])==set(cert['rawPreservation']['files']),'complete raw remote set mismatch')
        for name,v in cert['rawPreservation']['files'].items():
            data=checked_raw({**v['remote'],'base64':v['base64']},'child '+name);need(v['status']=='PASS'and v['remote']==v['downloaded']==cert['remoteRawEvidence'][name],'remote/download raw identity mismatch')
            rawfacts.append(exact(v['artifact'],v['remote']));need(Path(v['artifact']).read_bytes()==data,'raw bytes differ from base64original');raws[name]=data
        runs=cert['remoteFileScriptRuns'];need([Path(v['script'].replace('\\','/')).name for v in runs]==['tls-setup.ps1','tls-launch.ps1','tls-stop.ps1','tls-capture.ps1','tls-cleanup.ps1'],'physical/run/raw/keycleanup order changed')
        runfacts=[];forwarded=None;stop=None;clean=None
        for i,v in enumerate(runs):
            need(v['exitCode']==0,'remote file run failed');stdout=checked_raw(v['stdout'],'file run stdout');checked_raw(v['stderr'],'file run stderr');runfacts.append({'index':i,'scriptName':Path(v['script'].replace('\\','/')).name,'exitCode':0,'timeoutSeconds':v['timeoutSeconds'],'stdoutBytes':len(stdout),'stdoutSha256':sha(stdout)})
            if i==1:forwarded=stdout
            if i==2:stop=json.loads(stdout.decode('utf-8-sig').strip())
            if i==4:clean=json.loads(stdout.decode('utf-8-sig').strip())
        wire=validate_wire(raws['tls.stdout'],forwarded,contract)
        need(cert['nativeValidation']['exactSteps']==22 and cert['nativeValidation']['actualProcessExit']==0 and cert['nativeValidation']['facts']=={k:wire['observation'][k]for k in ['found','currentTime','privateKey','serverAuthEku','serverName','sslChainPolicy','cachedRevocation']},'top validation differs from actual original wire')
        transports=[];upload_proof=[]
        for i,v in enumerate(cert['transportInvocations']):
            need(v['exitCode']==0,'transport failed');so=checked_raw(v['stdout'],'transport stdout');checked_raw(v['stderr'],'transport stderr')
            if 'powershellSourceUtf8'in v:
                source=checked_raw(v['powershellSourceUtf8'],'transport source');text=source.decode('utf-8')
                if 'Get-FileHash -Algorithm SHA256' in text and 'fc-bootstrap.exe'in text:
                    need(so.decode('utf-8-sig').strip()==NORMAL,'actual uploaded EXE GetFileHash mismatch');upload_proof.append({'invocation':i,'normalSha256':NORMAL})
            transports.append({'index':i,'program':Path(v['command'][0]).name,'exitCode':0,'timeoutSeconds':v['timeoutSeconds'],'stdoutBytes':len(so)})
        need(len(upload_proof)==1,'one actual remote EXE uploadSHA required')
        details=cert['cleanupDetails'];need(stop==cert['ownedProcessCleanup']and stop['status']=='PASS'and stop['ownedProcessesRemaining']==stop['ownedProcessesKilled']==stop['recoveredNativeProcesses']==0,'actual process stop cleanup mismatch')
        need(clean==details and clean['status']=='PASS'and clean['rootAbsent']is True and clean['cngKeyExists']is False and clean['certificatesRemoved']==1 and clean['myDigestRestored']is True and clean['myAfter']==cert['fixture']['myBefore'],'actual cert/CNG/MY/root cleanup mismatch')
        for name in ['.tls-process.json','.tls-supervisor.json','.tls-setup-process.json','.tls-fixture.json']:need(json.loads(raws[name])['marker']==cert['marker'],'original owner marker mismatch')
        need(snapshot()==before,'current429 changed during independent audit')
        report.update(status='PASS_COMPLETED_WIN11_RUNTIME_AND_CERTIFICATE_ONLY',sourceCount=429,sourceBeforeAfterEqual=True,source=before,normalSha256=NORMAL,binaries=binaries,testMetadata=meta,runtime={'report':fact(E/'windows-gnu-runtime-first.json'),'command':runtime_command,'groups':groups,'passed':1481,'testExecutables':21,'cleanup':'PASS','platform':runtime['platform'],'zeroTestTargets':[v['name']for v in groups if v['passed']==0]},nativeFlows=native_flows,certificate={'report':fact(E/'windows-https-certificate-native-first.json'),'command':completed_command(E/'windows-https-certificate-native-first.command.json'),'wire':wire,'rawOriginals':rawfacts,'remoteFileRuns':runfacts,'transportInvocations':transports,'uploadShaProof':upload_proof,'controlPreparationsAndAST':controls,'cleanup':'PASS','heldNativeCancellation':cert['heldNativeCancellation'],'missingReference':cert['missingReference'],'wrongHost':cert['wrongHost']},matrix={'snapshot':fact(snap_path),'statusAtRead':matrix['status'],'completedWindowsCommands':command_facts,'HostWholeMatrixPromotedToPass':False,'preliminaryEarlierMutableReportHash':c['buildReportCapturedBytesSha256']},sourceCorrection={'proof':fact(E/'ui-probe-serialization-second-integration.json'),'historicalWireSnapshotSha256':HISTORICAL,'actualCompiledProbeSha256':CORRECTED,'contract':fact(CONTRACT)},limitations=['Completed GNU/Win11 evidence only; Host matrix status separately pending/not promoted by this review.','No actual Win7/Win10, formal MSVC, visible Seewo or production listener/P0 upgrade.','Historical inspection facts only, private-key acquisition not signing proof; untrusted chain is preserved Rejected; cache/name not invented positive.','Real native-held cancellation, missing reference and wrong-host scenarios not exercised; App commit-delivery cancellation is distinct.','Two original shared workers physical shutdown proved by exact runtime probe assertions/footer/source binding, not new kernel thread telemetry.'])
    except Exception as error:report['error']={'type':type(error).__name__,'message':str(error)[:1000]}
    report['helperUnchanged']=fact(Path(__file__))==report['helper']
    with out.open('x')as f:json.dump(report,f,ensure_ascii=False,indent=2);f.write('\n')
    print(json.dumps({'status':report['status'],'report':str(out),'sha256':fact(out)['sha256'],'error':report.get('error')}))
    return 0 if report['status'].startswith('PASS_')else 1

if __name__=='__main__':sys.exit(main())
