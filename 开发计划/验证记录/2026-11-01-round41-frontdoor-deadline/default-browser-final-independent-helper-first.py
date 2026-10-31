from pathlib import Path
from urllib.parse import urlsplit
from collections import Counter
import ast, base64, datetime, hashlib, json, re

EV = Path('/tmp/fc-round41-frontdoor-deadline-evidence')
ROOT = Path('/workspace/共享/Eliora/未来教室计划')
INPUT = EV / 'windows-default-browser-continuous-final.json'
OUT = EV / 'default-browser-final-independent-first.json'
SOURCE = 'ab5c403fa4f2cd90a19e3be52d7307ea408465d3'
EXE = 'b1f25364f488d6e8ffdb45ef53d5e99ff7fa717e3a226dd9ddf75d48bf7dee6f'
sha = lambda b: hashlib.sha256(b).hexdigest()
assert not OUT.exists(), 'refuse existing report'
inputs = {}
report = {'schemaVersion': 1, 'status': 'RUNNING', 'gitBase': SOURCE, 'exeSha256': EXE,
          'scope': 'Independent read-only recomputation of completed R41 GNU actual finite TLS/continuous/HOMEWORK browser evidence. Original first operational FAIL and separate atomic-stop file fixtures remain unchanged. Original runner, standalone Node/Edge-owner files and exact raw bytes are checked without executing tests/network. Final R41 source is a compiled delta on pinned ab5 base including the scoped Win7 attributes, not represented as already in that Git commit. No Cookie/CSRF/body/control capability copied.',
          'checks': [], 'inputs': []}

def check(name, condition):
    report['checks'].append({'name': name, 'status': 'PASS' if condition else 'FAIL'})
    if not condition:
        raise AssertionError(name)

def input_bytes(path):
    path = Path(path)
    check('input contained in exact evidence directory: ' + path.name,
          path.is_absolute() and path.is_relative_to(EV) and path.resolve().is_relative_to(EV))
    data = path.read_bytes()
    row = {'path': str(path), 'bytes': len(data), 'sha256': sha(data)}
    if str(path) in inputs:
        check('repeated input remains unchanged: ' + path.name, inputs[str(path)] == row)
    else:
        inputs[str(path)] = row
    return data

def json_input(path):
    return json.loads(input_bytes(path).decode('utf-8-sig'))

def encoded_stream(row, label):
    data = base64.b64decode(row['base64'], validate=True)
    check(label + ' exact complete retained bytes and SHA',
          row['bytes'] == len(data) and row['sha256'] == sha(data)
          and row.get('captureScope', 'COMPLETE') == 'COMPLETE'
          and row.get('observedBytes', len(data)) == len(data))
    return data

def raw_files(owner, label):
    preservation = owner['rawPreservation']
    check(label + ' original raw preservation PASS', preservation['status'] == 'PASS')
    result = {}
    for name, row in preservation['files'].items():
        data = input_bytes(row['artifact'])
        check(label + ' raw file accepted: ' + name, row['status'] == 'PASS')
        expected = row.get('downloaded', row)
        check(label + ' raw file byte hash: ' + name,
              expected['bytes'] == len(data) and expected['sha256'] == sha(data))
        if 'remote' in row:
            check(label + ' remote/download agreement: ' + name, row['remote'] == expected)
        if 'base64' in row:
            check(label + ' retained base64/file agreement: ' + name,
                  base64.b64decode(row['base64'], validate=True) == data)
        result[name] = data
    return result

from zipfile import ZipFile

def ast_strings(path):
    module=ast.parse(input_bytes(path).decode())
    return {statement.targets[0].id:ast.literal_eval(statement.value)
            for statement in module.body if isinstance(statement,ast.Assign)
            and len(statement.targets)==1 and isinstance(statement.targets[0],ast.Name)
            and statement.targets[0].id in ['CSHARP','COMMON','SUPERVISE','CAPTURE','CLEANUP','STOP']}

try:
    r=json_input(INPUT);b=r['browser']
    check('final actual full runner terminal PASS cleanupPASS',r['status']==r['cleanup']=='PASS')
    check('actual exact GNU normal b1f finite HOMEWORK continuous gates unchanged',r['exeSha256']==EXE and r['browserContentKind']=='HOMEWORK' and r['continuousAcceptEnabled'] is True and r['strictP0']=='NOT_VALIDATED' and r['productionListener']=='NOT_ENABLED' and r['forwardTimingIsNativeDeadlineEvidence'] is False)
    runner_path=EV/'windows-tls-business-probe.header-deadline-frozen-third.py'
    edge_path=EV/'windows-test-edge-owner.atomic-stop-frozen-first.py'
    before_edge=EV/'windows-test-edge-owner.before-atomic-stop.py'
    runner_raw=input_bytes(runner_path);edge_raw=input_bytes(edge_path)
    check('actual third runner0cb597 exact source',sha(runner_raw)==r['runnerSourceSha256']=='0cb59751f0bd1e79dd86de9eb26ac780e264a969d74cbdbe074be988210c7208')
    check('actual Edge atomic helper e093 source distinct from top native ownership helper',sha(edge_raw)==b['sourceSha256']['edgeOwner']=='e0930f19dc6fabf490774b2bd354605fb285d195dbaae32fdffd1e41f4f775f5' and r['ownershipHelperSourceSha256']==b['sourceSha256']['tlsOwnershipHelpers']=='1a91ac72ccb3fff0a08d9aa98222a82019af86a8d1b903e9fb10eb69ad8d94df' and r['ownershipHelperSourceSha256']!=b['sourceSha256']['edgeOwner'])
    for key,relative in [('observer','windows-native-browser-observer.py'),('nodeHarness','windows-native-browser-harness.mjs'),('tlsOwnershipHelpers','windows-tls-probe.py')]:
        check('unchanged actual source provenance '+key,sha((ROOT/'scripts'/relative).read_bytes())==b['sourceSha256'][key])
    programs=json_input(EV/'programs-independent-third.json')
    build_path=EV/programs['buildReport'];build=json_input(build_path)
    check('final exact original build report SHA',inputs[str(build_path)]['sha256']==programs['buildReportSha256'])
    check('final build original source before after410 unchanged',build['status']=='PASS' and build['changedInputs']==[] and build['gitHeadBefore']==build['gitHeadAfter']==SOURCE and build['sourceBefore']==build['sourceAfter']==programs['sourceBinding'] and len(programs['sourceBinding'])==410)
    check('GNU b1f actual source artifact bound before run',programs['status']=='PASS_STATIC_PROGRAM_AND_FINAL_RUST_INPUT_AUDIT' and programs['normalExecutable']['sha256']==EXE and programs['normalExecutable']['bytes']==12922615 and programs['counts']['executables']==22 and programs['counts']['tests']==21 and programs['runtimeExecutedByThisAudit'] is False)
    probe_source=Path('/tmp/fc-round41-frontdoor-deadline-worktree/crates/fc-bootstrap/src/application/http_business_probe.rs')
    probe=probe_source.read_bytes();compiled_inputs=programs['sourceBinding']
    wanted=compiled_inputs['crates/fc-bootstrap/src/application/http_business_probe.rs']
    check('finite native original45s source exact compiled binding',len(probe)==wanted['bytes'] and sha(probe)==wanted['sha256'] and re.search(rb'const PROBE_SECONDS: u64 = 45;',probe) is not None and b'Instant::now() + Duration::from_secs(PROBE_SECONDS)' in probe)
    tree=ast.parse(runner_raw.decode());csp=None;client_budget=False
    for statement in tree.body:
        if isinstance(statement,ast.Assign) and any(isinstance(x,ast.Name) and x.id=='CSP' for x in statement.targets):csp=ast.literal_eval(statement.value)
        if isinstance(statement,ast.ClassDef) and statement.name=='Client':
            for node in ast.walk(statement):
                if isinstance(node,ast.Assign) and any(isinstance(x,ast.Attribute) and x.attr=='deadline' for x in node.targets):
                    value=node.value
                    client_budget=isinstance(value,ast.BinOp) and isinstance(value.op,ast.Add) and isinstance(value.right,ast.Constant) and value.right.value==30
    check('unchanged original client30s source and strict CSP',client_budget and isinstance(csp,str))
    check('finite Edge original180s and actual observer215s exit observation unchanged',b['edgeSessionCapSeconds']==180 and b['ownerExitObservationSeconds']==215)
    npath=Path(b['nodeReportPath']);n=json_input(npath);meta=inputs[str(npath)]
    check('standalone actual Node original size SHA matches observer',meta['bytes']==b['nodeReport']['bytes'] and meta['sha256']==b['nodeReport']['sha256'])
    check('Node result equals embedded actual report',n==b['result'])
    check('actual Node HOMEWORK same service and original max12s',n['status']=='PASS' and n['contentKind']=='HOMEWORK' and n['marker']==r['marker'] and n['generation']==r['ready'][0]['generation'] and n['origin']==r['ready'][0]['origin'] and n['maxMillis']<=12000 and n['elapsedMillis']<=n['maxMillis'])
    check('Node frozen source and original strict CSP bound',n['sourceSha256']==b['sourceSha256']['nodeHarness'] and sha(csp.encode())==n['expectedCspSha256'])
    package=Path('/mnt/data/FutureClassroom-0.1.0-voice-capability-dev.zip')
    check('intentional unchanged static package SHA',sha(package.read_bytes())==r['webBundleSha256']=='5e280a84e726f0366db3e0ea1bd704158e26ac1b3fdb4e542f397b48fc1706c0')
    with ZipFile(package) as z:web_manifest=json.loads(z.read('web/fc-assets.json'))
    check('Node admitted canonical static manifest matches package',sha(json.dumps(web_manifest,separators=(',',':'),ensure_ascii=False).encode())==n['assetManifestCanonicalSha256'])
    by_asset={x['path']:x for x in web_manifest['assets']}
    for asset in n['assets']:
        expected=by_asset.get(asset['path'])
        check('actual visited resource precise hash '+asset['path'],expected is not None and asset['verified'] is True and asset['bytes']==expected['sizeBytes'] and asset['sha256']==expected['sha256'])
    network=n['network'];flags=['servedFromCache','fromDiskCache','fromServiceWorker','fromPrefetchCache']
    check('actual all browser observations physically finished/bodyVerified',all(x['finished'] is True and x['bodyVerified'] is True for x in network))
    wire=[x for x in network if x['wireStatus'] is not None and (x['wireStatus']==304 or not any(x[f] for f in flags))]
    check('independent wire count excludes cached200 keeps actual304',wire==n['wireResponses'] and len(wire)==n['wireResponseCount']==b['wireResponseCount'])
    check('observed and cache-only responses independent accounting',len(network)==n['observedRequestCount']==n['observedResponseCount'] and len(network)-len(wire)==n['cacheOnlyResponseCount'])
    for row in network:
        parsed=urlsplit(row['url'])
        check('browser exact same HTTPS origin and no secret query',parsed.scheme=='https' and parsed.netloc==urlsplit(n['origin']).netloc and not parsed.query and not parsed.fragment and not parsed.username and not parsed.password)
        if not parsed.path.startswith('/fc/api/'):
            check('actual static CSP exact '+parsed.path,row.get('csp')==csp)
    api=[x for x in network if urlsplit(x['url']).path.startswith('/fc/api/')]
    content_paths={urlsplit(x['url']).path for x in api if re.fullmatch(r'/fc/api/v1/content/homework/1/1/1/[0-9]{4}-[0-9]{2}-[0-9]{2}',urlsplit(x['url']).path)}
    check('one actual homework scope class subject owner and day route',len(content_paths)==1)
    content=next(iter(content_paths));datetime.date.fromisoformat(content.rsplit('/',1)[1]);visual=content+'/visual'
    routes=Counter((x['method'],urlsplit(x['url']).path) for x in api)
    expected=Counter({('GET','/fc/api/v1/status'):2,('GET','/fc/api/v1/session'):2,('GET','/fc/api/v1/content/homework/scopes/0'):2,('GET',content):2,('POST',content):1,('GET',visual):4})
    check('exact independently derived13 API with4visual and1POST',routes==expected and len(api)==n['apiResponses']==13 and n['contentPosts']==1 and type(n['contentPosts']) is int and sum(x['method']=='POST' for x in network)==1)
    check('all13API responses real200 no cache',all(x['wireStatus']==x['status']==200 and not any(x[f] for f in flags) for x in api))
    operations={'statusGets':2,'sessionGets':2,'scopeGets':2,'contentGets':2,'contentPosts':1,'visualGets':4}
    check('derived operations equal report with no reminder shortcut substitution',n['apiOperations']==operations and n['reminderShortcutClicks']==0 and n['reminderObservations']==[])
    observations=n['visualObservations']
    check('four scoped readonly visual observations exact lifecycle',n['visualReads']==len(observations)==4 and [x['stage'] for x in observations]==['initial','draft-refresh','after-save','after-reload'] and [x['readNumber'] for x in observations]==[1,2,3,4])
    expected_visual={'subjectRevision':'1','displayRevision':'0','rulesVersion':1,'effectiveRgb':'#88A6C4','mode':'FILLED','homeworkFillPercent':3,'courseFillPercent':20,'homeworkComponentOpacityPercent':100,'courseComponentOpacityPercent':100,'homeworkVisible':True,'courseVisible':True}
    check('four visual payload observations expected default fixture',r['visualFixture']=='FILLED_DEFAULT' and all(x['visual']==expected_visual for x in observations))
    check('four repeated actual visual body digests stable',len({x['responseBodySha256'] for x in observations})==1)
    for x in observations:
        dom=x['dom'];stage=x['stage']
        check('actual '+stage+' visual revisions and draft retained',dom['subjectRevision']=='1' and dom['displayRevision']=='0' and dom['rulesVersion']=='1' and x['draftPreserved'] is True and dom['draftMatches'] is True and dom['bottomReminderAvailable'] is True and dom['refreshOpacity']==dom['helpOpacity']==1)
        for kind,percent in [('homework',3),('course',20)]:
            block=dom[kind];color=re.fullmatch(r'rgba\((\d+), (\d+), (\d+), ([0-9.]+)\)',block['backgroundColor'])
            check('actual computed '+stage+' '+kind+' fill vs group/text/controls',block['mode']=='FILLED' and block['fillPercent']==str(percent) and block['effectiveRgb']=='#88A6C4' and block['visible']=='true' and block['componentOpacity']==block['pillOpacity']==block['effectiveOpacity']==1 and block['borderWidth']==0 and block['borderStyle']=='none' and block['textAlign']=='center' and block['textMatches'] is True and block['textColorUnchanged'] is True and color is not None and tuple(map(int,color.groups()[:3]))==(136,166,196) and abs(float(color[4])-percent/100)<1e-9)
    check('actual COMMITTED original CSRF and no page/console errors',n['writeOutcome']=='COMMITTED' and n['sessionCsrfMatched'] is True and n['contentRevision']=='1' and n['pageErrorCount']==n['consoleErrorCount']==0)
    stdout=encoded_stream(b['nodeRaw']['stdout'],'Node stdout');encoded_stream(b['nodeRaw']['stderr'],'Node stderr')
    messages=[json.loads(line) for line in stdout.decode().splitlines()]
    check('original Node READY RESULT actual complete report binding',[x['type'] for x in messages]==['READY','RESULT'] and all(x['marker']==r['marker'] for x in messages) and messages[1]['status']=='PASS' and messages[1]['wireResponseCount']==len(wire) and messages[1]['contentPosts']==1 and messages[1]['contextDisposed'] is True and messages[1]['report']==str(npath))
    check('Node actualexit0 context disposed CDP disconnected',b['nodeActualExitCode']==0 and n['contextDisposed'] is True and n['cdpDisconnected'] is True)
    native_raw=raw_files(r,'native');lines=native_raw['tls.stdout'].decode().splitlines()
    passes=[json.loads(x.split(' ',1)[1]) for x in lines if x.startswith('TLS_BUSINESS_PASS ')]
    progress=[json.loads(x.split(' ',1)[1]) for x in lines if x.startswith('TLS_BUSINESS_PROGRESS ')]
    check('native actual original PASS and progress exact copies',passes==[r['native']]==r['nativeReports'] and progress==r['nativeProgress'])
    final=passes[0];last=progress[-1]
    check('original2workers actual join native drain Hubs/quarantine0',final['physicalNativeDrain'] is True and final['sharedWorkers']==2 and final['workerJoin'] is True and final['setupCleanup'] is True and final['hubPending']==final['downloadHubPending']==final['quarantined']==0)
    check('actual final native sockets pendingIO and jobs0',last['acceptance']['native_pending']==last['acceptance']['owned_sockets']==last['pendingJobs']==last['jsonBodyRecords']==0 and last['requests']==[])
    debug=[x for x in lines if x.startswith('TLS_BUSINESS_REPORT ')]
    check('raw final native report actual resource drain',len(debug)==1 and all(re.search(p,debug[0]) for p in [r'active_records: 0,',r'pending_jobs: 0,',r'cleanup_pending: 0,',r'retained_cleanup: 0,',r'native_drained: true,']))
    http=[x for x in r['events'] if type(x.get('httpStatus')) is int];nonhttp=[x for x in r['events'] if type(x.get('httpStatus')) is not int]
    check('all raw client records PASS with canonical events hash',all(x['status']=='PASS' for x in r['events']) and sha(json.dumps(r['events'],sort_keys=True,separators=(',',':')).encode())==r['clientEventsSha256'])
    check('all64 raw HTTP actualTLS12 exact cert and clean peerTLS EOF',all(x['tlsVersion']=='TLSv1.2' and x['serverHostname']=='localhost' and x['peerDerSha256']==http[0]['peerDerSha256'] and x['closeNotify']=='PASS: actual clean TLS EOF' for x in http))
    check('four nonHTTP cases are actual capacity peerEOF and cert rejection',{x['case'] for x in nonhttp}=={'continuous-capacity-reject-1','continuous-capacity-reject-2','wrong-ca-rejected','wrong-hostname-rejected'} and all(x['transportRejection'] in ['EOF','ConnectionResetError','SSLEOFError'] for x in nonhttp if x['case'].startswith('continuous-capacity')) and all(x['rejectionType']=='SSLCertVerificationError' for x in nonhttp if x['case'].startswith('wrong-')))
    check('derive exact68 raw events64HTTP plus18browser wire equals82native',len(r['events'])==68 and len(http)==64 and len(wire)==18 and final['heads']==final['responses']==len(http)+len(wire)==82)
    capabilities=[x for x in http if x.get('route')=='voice/capabilities'];observation=r['voiceCapabilitiesObservation']
    check('actual three no-body capabilities GET401200200',len(capabilities)==3 and [x['httpStatus'] for x in capabilities]==[401,200,200] and all(x['method']=='GET' and x['requestBodyBytes']==0 for x in capabilities))
    check('actual two authorized capabilities raw body size and SHA stable',capabilities[1]['responseBodyBytes']==capabilities[2]['responseBodyBytes'] and capabilities[1]['responseBodySha256']==capabilities[2]['responseBodySha256'])
    check('no fake availability/querycount/cache observation inferred',observation['status']=='PASS' and observation['deviceAvailability']=='UNVERIFIED' and observation['nativeQueryCount']=='NOT_OBSERVED' and observation['cacheBehavior']=='NOT_DETERMINED_BY_GET' and observation['stableFieldsExcludingServerNow']=='MATCH')
    capability_keys=None
    for statement in tree.body:
        if isinstance(statement,ast.FunctionDef) and statement.name=='validate_voice_capabilities':
            for node in statement.body:
                if isinstance(node,ast.Assign) and any(isinstance(t,ast.Name) and t.id=='keys' for t in node.targets):capability_keys=ast.literal_eval(node.value)
    check('successful actual runner exact20field validator source bound',isinstance(capability_keys,set) and len(capability_keys)==20)
    continuous=r['business']['continuousAcceptance']
    check('original4 request exact immutable120s intervals retained',len(continuous['originalIntervals'])==4 and len({(x['generation'],x['slot'],x['serial']) for x in continuous['originalIntervals']})==4 and all(x['deadlineNs']-x['admittedAtNs']==120_000_000_000 for x in continuous['originalIntervals']) and continuous['capacityCloses']==2 and continuous['sameOriginalReceipts']==4 and continuous['binaryUploadPermitsUsedByStage']==0)
    check('actual idle application accept return/repost after5.05 without kernel claim',r['continuousIdle']['noClientSeconds']>=5.05 and r['continuousIdle']['idleReturns']>=1 and r['continuousIdle']['repostedSequence']>r['continuousIdle']['baselineSequence'])
    owner_path=Path(b['ownerReportPath']);owner=json_input(owner_path);identity=inputs[str(owner_path)]
    check('Edge original standalone file exact bytes SHA',identity['bytes']==b['ownerReport']['bytes'] and identity['sha256']==b['ownerReport']['sha256'])
    check('Edge e093 exact original helper source binding not top native helper',owner['sourceSha256']==b['sourceSha256']['edgeOwner']=='e0930f19dc6fabf490774b2bd354605fb285d195dbaae32fdffd1e41f4f775f5')
    check('Edge owner and observer actualexit0/physicalcleanup PASS',owner['status']==owner['cleanup']==b['status']==b['cleanup']==b['ownerPhysicalCleanup']=='PASS' and b['ownerActualExitCode']==0 and b['errors']==[] and b['ownerProcessRetained'] is False)
    edge_raw=raw_files(owner,'Edge owner');outcome=json.loads(edge_raw['.edge-outcome.json'].decode('utf-8-sig'));process=json.loads(edge_raw['.edge-process.json'].decode('utf-8-sig'));assigned=json.loads(edge_raw['.edge-assigned.json'].decode('utf-8-sig'))
    check('raw Edge outcome equals original report with actual physical exit',outcome==owner['nativeOutcome'] and outcome['originalPrimaryExited'] is True and outcome['actualJobProcessesZero'] is True and outcome['originalHandlesClosed'] is True and outcome['failure'] is None and outcome['reason']=='capability-stop')
    check('Edge actual original job bound while suspended before resume',process['createdSuspended'] is True and assigned['assignedBeforeResume'] is True and assigned['breakawayAllowed'] is False and assigned['killOnJobClose'] is True and process['pid']==assigned['pid'] and process['startTicks']==assigned['startTicks'] and process['marker']==assigned['marker']==owner['marker'])
    check('Edge exact profile/root absent other browsers/settings untouched',owner['cleanupDetails']['rootAbsent'] is True and owner['cleanupDetails']['actualJobProcessesZero'] is True and owner['cleanupDetails']['existingBrowsers']==owner['cleanupDetails']['sshConfiguration']==owner['cleanupDetails']['rootTrustStore']=='UNTOUCHED')
    for kind in ['stdout','stderr']:encoded_stream(b['ownerSupervisorRaw'][kind],'Edge owner original supervisor '+kind)
    check('native actualexit0 original tunnel cleanup zero remaining process',r['nativeProcessExit']=={'code':0} and r['nativeProcessExits']==[{'code':0}] and r['ownedTunnelCleanup']=='PASS' and r['ownedProcessCleanup']['status']=='PASS' and r['ownedProcessCleanup']['ownedProcessesRemaining']==0)
    cleanup=r['cleanupDetails']
    check('native synthetic MY/CNG/root actual cleanup original settings preserved',cleanup['status']=='PASS' and cleanup['rootAbsent'] is True and cleanup['cngKeyExists'] is False and cleanup['myDigestRestored'] is True and cleanup['ownedProcessesRemaining']==0 and all(cleanup[k]=='UNTOUCHED' for k in ['sshConfiguration','rootTrustStore','firewall','httpSys']))
    first=json_input(EV/'windows-default-browser-continuous-first.json');first_owner=json_input(Path(first['browser']['ownerReportPath']));freeze=json_input(EV/'editor-edge-atomic-stop-freeze-first.json')
    check('actual first old GNU full browser operational FAIL retained same runner',first['status']=='FAIL' and first['cleanup']=='PASS' and first['exeSha256']=='3ac0f3728f819c610de16fb0e2b79df5d9e6b0b2992fa151429168ba92957937' and first['exeSha256']!=r['exeSha256'] and first['runnerSourceSha256']==r['runnerSourceSha256'] and first['browser']['nodeActualExitCode']==0 and first['browser']['ownerActualExitCode']==1 and first['browser']['status']=='FAIL')
    check('first owner actual sharing violation while stop file read remains failure',first_owner['status']=='FAIL' and first_owner['nativeOutcome']['failure'] is not None and 'ReadAllText' in first_owner['nativeOutcome']['failure'] and '.request-stop' in first_owner['nativeOutcome']['failure'] and 'being used by another process' in first_owner['nativeOutcome']['failure'])
    old_strings=ast_strings(before_edge);new_strings=ast_strings(edge_path)
    for key in ['CSHARP','COMMON','SUPERVISE','CAPTURE','CLEANUP']:
        check('original Edge native/process ownership block unchanged '+key,old_strings[key]==new_strings[key] and any(x['name']==key and x['sha256']==sha(new_strings[key].encode()) and x['unchanged'] is True for x in freeze['nativeBlocks']))
    stop=new_strings['STOP']
    check('atomic STOP complete closed unique CreateNew same-dir nonoverwrite publication source',"function Publish-StopRequest" in stop and '[IO.FileMode]::CreateNew' in stop and '$file.Write($bytes,0,$bytes.Length)' in stop and stop.index('$file.Dispose()')<stop.index('[IO.File]::Move($pending,$path)') and "('.request-stop.pending-'+[Guid]::NewGuid().ToString('N'))" in stop and 'if($owned-and [IO.File]::Exists($pending)){[IO.File]::Delete($pending)}' in stop and 'New-Text $path $capability' not in stop)
    first_fixture=json_input(EV/'editor-edge-atomic-stop-windows-first.json');fixture=json_input(EV/'editor-edge-atomic-stop-windows-second.json')
    check('preserved first synthetic fixture instrumentation failure not silently passed',first_fixture['status']=='FAIL' and first_fixture['exitCode']==1 and sha(input_bytes(EV/'editor-edge-atomic-stop-windows-first.json'))==freeze['preservedFailedFixture']['sha256'])
    check('actual second synthetic file-only fixture original proof bound',fixture['status']=='PASS_ACTUAL_WINDOWS_SYNTHETIC_FILES' and fixture['exitCode']==0 and fixture['astErrors']==0 and sha(input_bytes(EV/'editor-edge-atomic-stop-windows-second.json'))==freeze['actualWindows']['sha256'])
    for value,label in [(first_fixture,'first'),(fixture,'second')]:
        for key in ['source','stdout','stderr']:
            data=encoded_stream(value[key],'actual file fixture '+label+' '+key)
            if 'path' in value[key]:check('file fixture '+label+' '+key+' original bytes match',input_bytes(Path(value[key]['path']))==data)
    result=fixture['result']
    check('actual file-only children4 each expected exit physical return disposed rootremoved',result['status']=='PASS' and result['failure'] is None and result['started']==result['actualExited']==result['disposed']==4 and result['rootRemoved'] is True and len(result['children'])==4 and all(x['actualExited'] is True and x['exitCode']==x['expectedExitCode'] for x in result['children']))
    check('samecap concurrent normal source and differentcap negative retained',len(result['checks'])==6 and [x['exitCode'] for x in result['children']]==[0,0,0,1] and 'Fixture-only instrumentation' in result['halfWriteSource'])
    check('all input original bytes remain unchanged',all(Path(x['path']).stat().st_size==x['bytes'] and sha(Path(x['path']).read_bytes())==x['sha256'] for x in inputs.values()))
    report.update(status='PASS_FINITE_GNU_HOMEWORK_TLS_BROWSER_ORIGINAL_GATES_UNCHANGED',contentKind='HOMEWORK',exeSha256=EXE,runnerSha256=r['runnerSourceSha256'],edgeOwnerSha256=owner['sourceSha256'],topNativeOwnershipHelperSha256=r['ownershipHelperSourceSha256'],
        counts={'rawEvents':len(r['events']),'rawHttpResponses':len(http),'browserObservedResponses':len(network),'browserWireResponses':len(wire),'browserCacheOnlyResponses':len(network)-len(wire),'nativeHeads':final['heads'],'nativeResponses':final['responses'],'apiResponses':len(api),'apiOperations':operations,'posts':n['contentPosts'],'visualReads':n['visualReads'],'reminderShortcutClicks':n['reminderShortcutClicks'],'capabilityGets':len(capabilities),'capabilityStatuses':[x['httpStatus'] for x in capabilities],'sharedWorkers':final['sharedWorkers'],'hubPending':final['hubPending'],'downloadHubPending':final['downloadHubPending'],'quarantined':final['quarantined']},
        budgets={'clientSecondsSource':30,'nativeProbeSecondsCompiledSource':45,'nodeMaxMillis':n['maxMillis'],'nodeElapsedMillis':n['elapsedMillis'],'edgeCapSeconds':180,'ownerExitObservationSeconds':215,'scope':'Original fixed source/report budgets; no timer reset or new workflow execution by audit'},
        nativeFinalReceipt=final,capabilityObservation=observation,capabilityValidator={'fieldCount':20,'fieldNames':sorted(capability_keys),'scope':'Actually successful exact runner source validator; absent actual response bodies are not independently reconstructed.'},visualObservations=observations,
        edgePhysicalOutcome={k:outcome[k] for k in ['originalPrimaryExited','originalPrimaryExitCode','actualJobProcessesZero','originalHandlesClosed','jobBoundBeforeResume','reason','failure']},
        history={'firstActualBrowser':{'path':str(EV/'windows-default-browser-continuous-first.json'),'status':'FAIL_UNCHANGED','exeSha256':first['exeSha256'],'sameGnuAsFinal':False,'sameRunnerAsFinal':True,'reason':'Original owner sharing violation reading .request-stop; Node already exit0, operational owner failure remains FAIL.'},'firstFileFixture':{'status':'FAIL_INSTRUMENTATION_UNCHANGED','reason':freeze['preservedFailedFixture']['cause']},'secondFileFixture':{'status':fixture['status'],'checks':result['checks'],'childExitCodes':[x['exitCode'] for x in result['children']],'scope':result['scope'],'halfWriteScope':result['halfWriteSource']},'meaning':'Synthetic file fixtures directly test source publication/cleanup; the separately audited final full actual Edge run proves original browser owner completed with fixed helper. File fixtures alone are not browser/TLS/product evidence.'},
        inputs=list(inputs.values()),helper={'path':str(Path(__file__)),'bytes':Path(__file__).stat().st_size,'sha256':sha(Path(__file__).read_bytes())},
        limitations=['No tests, Windows, network, Cargo, Git or source mutation performed by checker.','The top ownershipHelperSourceSha2561a91 is windows-tls-probe native ownership code, NOT Edge helper; actual Edge e093 is bound through browser.sourceSha256 and standalone owner report.','Actual capabilities UNVERIFIED; three GETs do not reveal native query count/cache or guarantee current device/audibility. Full response bodies intentionally omitted, exact20field validation belongs to source-bound actual runner.','Edge primary actual exit1 after capability-stop is preserved; original owner supervisor actualexit0/job0/handles/profile removal prove physical completion.','Original first full actual browser FAIL and first synthetic fixture FAIL remain unchanged; expected different-cap child exit1 is correct rejection, not a false exit0 claim.','HOMEWORK four actual computed-CSS/draft observations and network13API are instrumented original Node evidence, no retained raw CDP-frame claim.','Full kernel ESTABLISHED/backlog/strictP0 not proved; ordinary listener remains disabled, strict old firstTCP5s gate unchanged.','Win7/Win10/visibleSeewo/mobile/campusTLS/physical microphone/audibility remain separate gates.'])
except BaseException as error:
    report.update(status='FAIL_INDEPENDENT_AUDIT',errorKind=type(error).__name__,failedCheck=str(error) if isinstance(error,AssertionError) else 'Original evidence invariant or shape failed',inputs=list(inputs.values()))
with OUT.open('x') as f:f.write(json.dumps(report,ensure_ascii=False,indent=2)+'\n')
print(json.dumps({'status':report['status'],'checks':len(report['checks']),'counts':report.get('counts'),'report':{'path':str(OUT),'bytes':OUT.stat().st_size,'sha256':sha(OUT.read_bytes())}},ensure_ascii=False))
raise SystemExit(0 if report['status']=='PASS_FINITE_GNU_HOMEWORK_TLS_BROWSER_ORIGINAL_GATES_UNCHANGED' else 1)
