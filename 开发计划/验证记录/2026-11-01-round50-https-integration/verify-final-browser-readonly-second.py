"""Local read-only recomputation. No SSH, test, browser, Cargo or cleanup runs."""
from pathlib import Path
from urllib.parse import urlsplit
from collections import Counter
import ast,base64,datetime,hashlib,json,re,sys
sys.dont_write_bytecode=True
EV=Path('/tmp/fc-round50-https-integration-evidence')
R46=Path('/tmp/fc-round46-ci-evidence')
R51=Path('/tmp/fc-round51-browser-diagnostics-evidence')
SCRIPTS=Path('/tmp/fc-round51-browser-diagnostics-worktree/scripts')
OUT=EV/'browser-readonly-independent-second.json'
sha=lambda b:hashlib.sha256(b).hexdigest()
inputs={};report={'schemaVersion':1,'status':'RUNNING','checks':[],'flows':[]}
TAG=''
def check(name,condition):
 report['checks'].append({'flow':TAG,'name':name,'status':'PASS' if condition else 'FAIL'})
 if not condition:raise AssertionError(name)
def input_bytes(path):
 p=Path(path);data=p.read_bytes();row={'path':str(p),'bytes':len(data),'sha256':sha(data)}
 if str(p) in inputs:check('input bytes unchanged '+p.name,inputs[str(p)]==row)
 else:inputs[str(p)]=row
 return data
def json_input(p):return json.loads(input_bytes(p).decode('utf-8-sig'))
def encoded_stream(row,label):
 data=base64.b64decode(row['base64'],validate=True)
 check(label+' complete bytes SHA',row['bytes']==len(data) and row['sha256']==sha(data) and row.get('captureScope','COMPLETE')=='COMPLETE' and row.get('observedBytes',len(data))==len(data))
 return data
def raw_files(owner,label):
 preservation=owner['rawPreservation'];check(label+' preservation PASS',preservation['status']=='PASS');result={}
 for name,row in preservation['files'].items():
  data=input_bytes(row['artifact']);expected=row.get('downloaded',row)
  check(label+' raw accepted '+name,row['status']=='PASS' and expected['bytes']==len(data) and expected['sha256']==sha(data))
  if 'remote' in row:check(label+' remote/download '+name,row['remote']==expected)
  if 'base64' in row:check(label+' encoded/file '+name,base64.b64decode(row['base64'],validate=True)==data)
  result[name]=data
 return result
def recursive_raw(x,label='raw'):
 count=0
 if isinstance(x,dict):
  if {'bytes','sha256','base64'}<=x.keys():encoded_stream(x,label);count+=1
  for k,v in x.items():count+=recursive_raw(v,label+'/'+k)
 elif isinstance(x,list):
  for i,v in enumerate(x):count+=recursive_raw(v,label+'/'+str(i))
 return count

def validate_wire(network):
 flags=['servedFromCache','fromDiskCache','fromServiceWorker','fromPrefetchCache']
 if not all(x['finished'] is True and x['bodyVerified'] is True for x in network):raise ValueError('unfinished')
 return [x for x in network if x['wireStatus'] is not None and (x['wireStatus']==304 or not any(x[f] for f in flags))]
def validate_readonly(q,base,raw_observed):
 if q['status']!='PASS' or q['integrityStatus']!='PASS' or q['ssh']['exitCode']!=0:raise ValueError('readonly terminal')
 for k,v in raw_observed.items():
  if q[k]!=v:raise ValueError('raw readonly mismatch')
 if q['sshPublicAuthorizationSha256']!=base['sshPublicAuthorizationSha256'] or q['projectRunReadOnlySnapshot']!=base['projectRunReadOnlySnapshot']:raise ValueError('baseline changed')
 if q['sshd']!='Running' or q['persistentKeyMode']!='0o600' or q['persistentKeyMetadata']['contentsRead'] is not False:raise ValueError('ssh invariant')
 if any(q[k] for k in ['scratchDirectories','ownedProcesses','ownedTasks','activityObserved']):raise ValueError('residual')
 if not all(q[k] is True for k in ['publicAuthorizationUnchanged','projectRunUnchanged']):raise ValueError('integrity')
 views=q['projectRunReadOnlySnapshot']['views']
 if len(views)!=2 or {x['view'] for x in views}!={'Registry32','Registry64'}:raise ValueError('views')

def validate_no_failure(n):
 if n.get('status')!='PASS' or 'error' in n or any('fail' in k.lower() for k in n):raise ValueError('unexpected failure diagnostics')
def validate_api(network,n):
 rows=[x for x in network if urlsplit(x['url']).path.startswith('/fc/api/')]
 contents={urlsplit(x['url']).path for x in rows if re.fullmatch(r'/fc/api/v1/content/homework/1/1/1/[0-9]{4}-[0-9]{2}-[0-9]{2}',urlsplit(x['url']).path)}
 if len(contents)!=1:raise ValueError('scope')
 content=next(iter(contents));routes=Counter((x['method'],urlsplit(x['url']).path) for x in rows)
 expected=Counter({('GET','/fc/api/v1/status'):2,('GET','/fc/api/v1/session'):2,('GET','/fc/api/v1/content/homework/scopes/0'):2,('GET',content):2,('POST',content):1,('GET',content+'/visual'):4})
 if routes!=expected or len(rows)!=n['apiResponses'] or n['contentPosts']!=1:raise ValueError('api counts')
 flags=['servedFromCache','fromDiskCache','fromServiceWorker','fromPrefetchCache']
 if any(x['wireStatus']!=200 or x['status']!=200 or any(x[f] for f in flags) for x in rows):raise ValueError('cached API')
 return rows

def check_control_scripts(r):
 tree=ast.parse(input_bytes(SCRIPTS/'windows-tls-probe.py').decode())
 keys={'COMMON','SETUP','LAUNCH','STOP','CAPTURE','CLEANUP'}
 literals={t.id:ast.literal_eval(row.value) for row in tree.body if isinstance(row,ast.Assign) for t in row.targets if isinstance(t,ast.Name) and t.id in keys}
 for name in ['LAUNCH','STOP']:literals[name]=literals[name].replace('--tls-lifecycle-probe','--tls-business-probe')
 check('exact underlying remote templates digest',sha(''.join(literals[k] for k in ['COMMON','SETUP','LAUNCH','STOP','CAPTURE','CLEANUP']).encode())==r['remoteScriptsSha256'])
 for name,key in [('cleanup','CLEANUP'),('stop','STOP'),('capture','CAPTURE'),('setup','SETUP'),('launch','LAUNCH')]:
  rendered=literals[key]
  replacements={'__ROOT__':r['remoteRoot'],'__MARKER__':r['marker'],'__EXENAME__':'fc-bootstrap.exe','__THUMBPRINT__':r['fixture']['thumbprint'] if name=='launch' else ''}
  for token,value in replacements.items():rendered=rendered.replace(token,"'"+value.replace("'","''")+"'")
  exact=(literals['COMMON']+rendered).encode('utf-8-sig')
  check('regenerated exact uploaded native control '+name,sha(exact)==r['uploadedScriptSha256'][name])

def audit(browser_path,readonly_path,exe,provenance):
    global TAG
    TAG=browser_path.name
    r=json_input(browser_path);b=r['browser'];npath=Path(b['nodeReportPath']);n=json_input(npath)
    check('actual completed browser PASS cleanupPASS',r['status']==r['cleanup']==b['status']==b['cleanup']==n['status']=='PASS')
    check('exact independently audited executable SHA',r['exeSha256']==exe)
    check('production and strict deadline gates unchanged',r['browserContentKind']=='HOMEWORK' and r['continuousAcceptEnabled'] is True and r['strictP0']=='NOT_VALIDATED' and r['productionListener']=='NOT_ENABLED' and r['forwardTimingIsNativeDeadlineEvidence'] is False)
    check('node original complete report bytes SHA and embedded equality',n==b['result'] and inputs[str(npath)]['bytes']==b['nodeReport']['bytes'] and inputs[str(npath)]['sha256']==b['nodeReport']['sha256'])
    freeze=json_input(R51/'implementation-freeze-second.json')
    check('second frozen diagnostic closed whitelist26',freeze['closedNames']['count']==len(freeze['closedNames']['names'])==26 and freeze['closedNames']['regexFallback'] is False)
    names={'observer':'windows-native-browser-observer.py','nodeHarness':'windows-native-browser-harness.mjs','edgeOwner':'windows-test-edge-owner.py','tlsOwnershipHelpers':'windows-tls-probe.py'}
    for key,name in names.items():check('source-bound actual '+key,sha(input_bytes(SCRIPTS/name))==b['sourceSha256'][key])
    for row in freeze['ownedFiles'][:2]:check('exact frozen second '+row['path'],b['sourceSha256']['nodeHarness' if row['path'].endswith('.mjs') else 'observer']==row['sha256'])
    validate_no_failure(n);validate_api(n['network'],n);check_control_scripts(r)
    check('successful PASS contains no failure diagnostics',not any('fail' in k.lower() for k in n) and 'error' not in n and 'error' not in r)
    runner_raw=input_bytes(SCRIPTS/'windows-tls-business-probe.py')
    check('source-bound actual runner',sha(runner_raw)==r['runnerSourceSha256']=='0cb59751f0bd1e79dd86de9eb26ac780e264a969d74cbdbe074be988210c7208')
    tree=ast.parse(runner_raw.decode());csp=next(ast.literal_eval(s.value) for s in tree.body if isinstance(s,ast.Assign) and any(isinstance(t,ast.Name) and t.id=='CSP' for t in s.targets))
    check('strict exact CSP digest',sha(csp.encode())==n['expectedCspSha256'])
    check('same origin marker generation and original12s deadline',n['marker']==r['marker'] and n['generation']==r['ready'][0]['generation'] and n['origin']==r['ready'][0]['origin'] and 0<n['maxMillis']<=12000 and n['elapsedMillis']<=n['maxMillis'])
    check('original180s browser owner observation215 budgets',b['edgeSessionCapSeconds']==180 and b['ownerExitObservationSeconds']==215)
    for a in n['assets']:
     want=next(x for x in r['staticPackage']['assets'] if x['path']==a['path'])
     check('source-admitted asset hash '+a['path'],a['verified'] is True and a['bytes']==want['sizeBytes'] and a['sha256']==want['sha256'])
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
    check('independent conservation rawHTTP plus actual browser wire equals native',len(r['events'])==len(http)+len(nonhttp) and final['heads']==final['responses']==len(http)+len(wire))
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

    check('independent actual wire classification',validate_wire(n['network'])==wire)
    for owner_obj,label in [(r,'native'),(b,'observer'),(owner,'Edge')]:
     for run in owner_obj.get('remoteControlRunsRaw',[]):check(label+' original control exit0',run['exitCode']==0)
    check('actual remote scripts parsed before use',r['controlScriptsParse']==owner['controlScriptsParse']=='PASS')
    rawcount=recursive_raw(r,'browserRunner')+recursive_raw(owner,'edgeOwner')
    check('raw owner journal uses exact actual original edge job',process['executablePath'].lower()==assigned['executablePath'].lower() if 'executablePath' in process and 'executablePath' in assigned else process['pid']==assigned['pid'])
    q=json_input(readonly_path);base=json_input(q['baseline']['path'])
    check('readonly baseline exact SHA',sha(input_bytes(q['baseline']['path']))==q['baseline']['sha256'])
    for k in ['auditSource','remoteAdapterSource']:check('readonly source '+k,sha(input_bytes(q[k]['path']))==q[k]['sha256'])
    ps=q['powershell'];check('readonly original stdin source UTF8/UTF16 bindings',ps['fullSource']==ps['preamble']+ps['script'] and sha(ps['fullSource'].encode())==ps['sourceSha256Utf8'] and sha(ps['fullSource'].encode('utf-16-le'))==ps['sourceSha256Utf16Le'] and base64.b64decode(ps['encodedCommand'],validate=True)==ps['encodedWrapperSource'].encode('utf-16-le'))
    check('readonly actual original source delivered as stdin',encoded_stream(ps['stdin'],'readonly stdin')==ps['fullSource'].encode())
    observed=json.loads(encoded_stream(q['ssh']['stdout'],'readonly stdout').decode('utf-8-sig'));encoded_stream(q['ssh']['stderr'],'readonly stderr')
    validate_readonly(q,base,observed)
    check('fresh readonly is after browser and observed complete',readonly_path.stat().st_mtime_ns>=browser_path.stat().st_mtime_ns and datetime.datetime.fromisoformat(q['completedAtUtc'])>=datetime.datetime.fromisoformat(q['observedAtUtc']))
    report['flows'].append({'browser':str(browser_path),'readonlyReport':str(readonly_path),'exeSha256':exe,'artifactProvenance':provenance,'status':'PASS_FINITE_ACTUAL_BROWSER_AND_FRESH_READONLY','sourceSha256':b['sourceSha256'],'counts':{'rawEvents':len(r['events']),'rawHTTP':len(http),'browserObserved':len(network),'browserWire':len(wire),'browserCacheOnly':len(network)-len(wire),'nativeHeads':final['heads'],'nativeResponses':final['responses'],'api':len(api),'posts':n['contentPosts'],'visualReads':n['visualReads'],'shortcutClicks':n['reminderShortcutClicks'],'recursiveRawContainers':rawcount},'voiceCapabilities':observation,'edgePhysicalOutcome':{k:outcome[k] for k in ['originalPrimaryExited','originalPrimaryExitCode','actualJobProcessesZero','originalHandlesClosed','reason','failure']},'readonly':{'authorizationUnchanged':q['publicAuthorizationUnchanged'],'projectRunUnchanged':q['projectRunUnchanged'],'sshd':q['sshd'],'ownedResiduals':0},'budgets':{'nodeMaxMillis':n['maxMillis'],'nodeElapsedMillis':n['elapsedMillis'],'edgeSessionCapSeconds':b['edgeSessionCapSeconds']}})

def main():
 if OUT.exists():raise FileExistsError('create-new only')
 try:
  prior=json_input(EV/'runtime-certificate-independent-second.json');check('prior R50 independent actual runtime PASS',prior['status'].startswith('PASS'))
  audit46=json_input(R46/'modern-original-independent-first.json');check('original modern artifact audit PASS static only',audit46['status']=='PASS_STATIC_ONLY_RUNTIME_NOT_RUN')
  check('modern normal source artifact exact SHA',audit46['normalExecutable']['producer']['sha256']=='bc8f224a97797311e76182486bfa6502f8c11110e3a7151893f5212ff9d911aa')
  check('R50 normal actual429-source runtime exact SHA',prior['normalSha256']=='b6368400a58b87d4725cbd664b5bde88a4f51927a74656e5c566010e8bbf6930' and prior['sourceCount']==429)
  audit(R46/'modern-msvc-default-browser-continuous-final-third.json',R46/'windows-readonly-final-second.json','bc8f224a97797311e76182486bfa6502f8c11110e3a7151893f5212ff9d911aa',{'audit':str(R46/'modern-original-independent-first.json'),'binding':'Original exact Modern MSVC artifact; separately completed original runtime review'})
  audit(EV/'windows-default-browser-continuous-first.json',EV/'windows-readonly-final-first.json','b6368400a58b87d4725cbd664b5bde88a4f51927a74656e5c566010e8bbf6930',{'audit':str(EV/'runtime-certificate-independent-second.json'),'binding':'R50 actual429-input compiled normal and runtime22EXE audit'})
  first=json_input(R46/'modern-msvc-default-browser-continuous-first.json')
  check('first real R46 FAIL retained cleanupPASS',first['status']=='FAIL' and first['cleanup']=='PASS')
  firstnode=json_input(first['browser']['nodeReportPath'])
  check('first real network failure remains unresolved',firstnode['status']=='FAIL' and firstnode['error']['code']=='NETWORK_LOAD_FAILED')
  report['history']={'firstActualR46':'FAIL_NETWORK_LOAD_FAILED_ORIGINAL_UNCHANGED','rootCause':'UNKNOWN','subsequentPassMeaning':'Finite successful later observations only; no proven correction of original failure.'}
  check('all reviewed input bytes remain unchanged',all(sha(Path(x['path']).read_bytes())==x['sha256'] for x in inputs.values()))
  report['status']='PASS_TWO_FINITE_ACTUAL_BROWSERS_AND_FRESH_READONLY'
 except BaseException as error:
  report['status']='FAIL';report['error']={'type':type(error).__name__,'check':str(error)[:250]}
 report.update(inputs=list(inputs.values()),helper={'path':str(Path(__file__)),'bytes':Path(__file__).stat().st_size,'sha256':sha(Path(__file__).read_bytes())},limitations=['Only local evidence recomputation, no network/browser/tests/build/cleanup run.','Original R46 NETWORK_LOAD_FAILED remains unknown and unchanged. Later PASS does not prove a fix.','Actual observed capabilities UNVERIFIED; no inference of native query, cache efficacy, current output or audible playback.','Normal PASS has no failure diagnostics; negative diagnostic mechanisms are separately modeled, not exercised by this success.','R50 full Host all-target matrix is separate and not promoted by this runtime review.','StrictP0 firstTCP5s, ordinary production listener, Win7/10 actual OS, visible Seewo, campus trust and72h remain separate gates.','Original Edge primary exit1 on deliberate capability-stop is preserved; supervisor exit0, Job0, handles closed and profile/root absent prove cleanup.'])
 with OUT.open('x') as f:json.dump(report,f,ensure_ascii=False,indent=2);f.write('\n')
 print(json.dumps({'status':report['status'],'report':str(OUT),'sha256':sha(OUT.read_bytes()),'flows':len(report['flows']),'checks':len(report['checks']),'error':report.get('error')},ensure_ascii=False))
 return 0 if report['status'].startswith('PASS') else 1
if __name__=='__main__':raise SystemExit(main())
