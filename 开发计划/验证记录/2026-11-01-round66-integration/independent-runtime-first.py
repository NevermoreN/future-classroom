#!/usr/bin/env python3
"""Local-only, exclusive-output audit of R66 terminal original receipts.
Never launches a compiler, test, subprocess, network client or Windows operation.
Missing/running inputs return2 WITHOUT writing a report. A failed real header run
is retained independently of the workspace/native/business outcomes.
"""
from pathlib import Path
from collections import Counter
import argparse, ast, base64, datetime, hashlib, json, re, struct, sys

E = Path('/tmp/fc-round66-integration/evidence')
MAIN = Path('/workspace/共享/Eliora/未来教室计划')
SUMMARY = re.compile(r'test result: (ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out; finished in ([0-9.]+)s')
ITEM = re.compile(r'test (.+) \.\.\. (ok|FAILED|ignored(?:,.*)?)')
REQUIRED = ['implementation-freeze-first.json', 'input-scope-qualification-first.json',
            'compiled-program-identities-first.json', 'release-tests-first.json',
            'release-product-first.json', 'release-clippy-first.json',
            'windows-release-first.json', 'windows-release-operation-first.json',
            'windows-release-extra-flows-first.json', 'windows-release-visual-first.json',
            'windows-release-repair-first.json', 'windows-release-tls-series-first.json',
            'windows-release-business-continuous-browser-first.json',
            'windows-release-header-deadline-first.json']
INPUTS = {}

def require(value, code):
    if not value:
        raise ValueError(code)

def sha(data):
    return hashlib.sha256(data).hexdigest()

def read(path):
    p = Path(path)
    require(p.is_file() and not p.is_symlink(), 'REGULAR_INPUT_REQUIRED')
    data = p.read_bytes()
    fact = {'path': str(p), 'bytes': len(data), 'sha256': sha(data)}
    require(str(p) not in INPUTS or INPUTS[str(p)] == fact, 'INPUT_CHANGED_DURING_AUDIT')
    INPUTS[str(p)] = fact
    return data

def js(path):
    return json.loads(read(path).decode('utf-8-sig'))

def fact(path):
    read(path)
    return INPUTS[str(Path(path))]

def text_lines(value):
    require(isinstance(value, list) and all(isinstance(x, str) for x in value), 'OUTPUT_MUST_BE_NORMALIZED_LINE_ARRAY')
    return value

def stream(value):
    data = base64.b64decode(value['base64'], validate=True)
    require(len(data) == value['bytes'] == value.get('observedBytes', len(data)) and sha(data) == value['sha256'] and value.get('captureScope', 'COMPLETE') == 'COMPLETE', 'RAW_STREAM_IDENTITY')
    return data

def controller_logs(stem, receipt, nested=False):
    result = {}
    for kind in ('stdout', 'stderr'):
        p = E / (stem + '.' + kind + '.log')
        f = fact(p)
        claim = receipt[kind] if nested else {'bytes': receipt[kind + 'Bytes'], 'sha256': receipt[kind + 'Sha256']}
        require(f['bytes'] == claim['bytes'] and f['sha256'] == claim['sha256'], 'CONTROLLER_RAW_LOG_IDENTITY')
        result[kind] = f
    return {'capture': 'controller subprocess complete raw stdout/stderr; NOT per-harness line inventory', **result}

def artifacts(path):
    rows = []
    for line in read(path).decode('utf-8').splitlines():
        try:
            d = json.loads(line)
        except ValueError:
            continue
        if d.get('reason') == 'compiler-artifact' and d.get('executable'):
            rows.append(d)
    return rows

def upload_identity(report, expected):
    uploaded = report['uploadedArtifacts']
    require(len(uploaded) == len(expected) and len({x['name'] for x in uploaded}) == len(uploaded), 'UPLOAD_UNIQUE_COUNT')
    require({x['name']: x['sha256'] for x in uploaded} == expected, 'UPLOAD_BINARY_IDENTITY')

def terminal_status(report):
    require(report.get('status') not in (None, 'RUNNING', 'IN_PROGRESS'), 'NONTERMINAL_REPORT')
    return {'status': report['status'], 'cleanup': report.get('cleanup')}

def source_build():
    f = js(E / 'implementation-freeze-first.json')
    q = js(E / 'input-scope-qualification-first.json')
    freeze_sha = fact(E / 'implementation-freeze-first.json')['sha256']
    rows = f['allRustCargoInputs']
    require(len(rows) == len({x['path'] for x in rows}) == f['sourceInputCount'] == q['frozenRows'], 'FREEZE_INVENTORY')
    require(q['freezeSha256'] == freeze_sha and f['mainWorktreeCompiledInputsIdentical'] is True, 'FREEZE_QUALIFICATION')
    history = [x for x in rows if x['path'].startswith('开发计划/')]
    require(len(history) == q['historicalSourceSnapshotsNotCompilerInputs'] and len(rows)-len(history) == q['crateRustAndCargoConfigurationRows'], 'CURRENT_VS_HISTORICAL_SCOPE')
    for root in (MAIN, Path(f['worktree'])):
        for x in rows:
            data = read(root / x['path'])
            require(len(data) == x['bytes'] and sha(data) == x['sha256'], 'FROZEN_SOURCE_CURRENT_BYTES')
    for x in f['files']:
        require(read(E / 'source-after' / x['path']) == read(MAIN / x['path']), 'CHANGED_SOURCE_SNAPSHOT')
    builds = []
    for name in ('release-tests-first', 'release-product-first', 'release-clippy-first'):
        d = js(E / (name + '.json'))
        require(d['status'] == 'PASS' and d['exitCode'] == 0 and d['sourceUnchanged'] is True and d['sourceFreezeSha256'] == freeze_sha, 'BUILD_NOT_PASS_OR_SOURCE_CHANGED')
        require(d['originalBudgetSeconds'] == 900 and d['elapsedSeconds'] <= 900, 'BUILD_ORIGINAL_BUDGET')
        command = d['command']
        require('--release' in command and '--locked' in command and command[command.index('--target')+1] == 'i686-pc-windows-gnu', 'BUILD_PROFILE_TARGET')
        if name == 'release-tests-first':
            require(all(x in command for x in ('--workspace', '--all-targets', '--no-run', '--message-format=json')), 'FULL_TEST_COMPILE_SCOPE')
        if name == 'release-clippy-first':
            require(command[-3:] == ['--', '-D', 'warnings'] and '--workspace' in command and '--all-targets' in command, 'STRICT_LINT_SCOPE')
        builds.append({'record': fact(E / (name+'.json')), 'status': d['status'], 'command': command, 'elapsedSeconds': d['elapsedSeconds'], 'originalBudgetSeconds': 900, 'raw': controller_logs(name, d)})
    return {'freeze': fact(E/'implementation-freeze-first.json'), 'changedFiles': len(f['files']), 'frozenRows':len(rows), 'currentCrateRustCargoConfigurationRows':len(rows)-len(history), 'historicalSnapshotsNotCompilerInputs':len(history), 'worktreeAndMainAllRowsMatch':True, 'builds':builds}

def programs():
    test_artifacts = artifacts(E/'release-tests-first.stdout.log')
    tests = [x for x in test_artifacts if x['profile']['test']]
    products = [x for x in artifacts(E/'release-product-first.stdout.log') if x['target']['name']=='fc-bootstrap' and not x['profile']['test']]
    require(len(tests)==21 and len({x['executable'] for x in tests})==21 and len(products)==1, 'PRODUCER_UNIQUE_21_PLUS_NORMAL')
    prod = products[0]
    operation = js(E/'windows-release-operation-first.json')
    identities = js(E/'compiled-program-identities-first.json')
    require(identities['status']=='PASS', 'COMPILED_IDENTITY_REPORT_STATUS')
    indexed = {}
    for x in identities['programRows']:
        if x['path'] in indexed:
            require(x==indexed[x['path']], 'DUPLICATE_PROGRAM_DIFFERENT_IDENTITY')
        indexed[x['path']]=x
    require(set(indexed)=={x['executable'] for x in tests+[prod]}, 'COMPILED_IDENTITY_SET')
    facts=[]
    for x in tests+[prod]:
        p=Path(x['executable']);data=read(p);identity=indexed[str(p)]
        require(len(data)==identity['bytes'] and sha(data)==identity['sha256'] and identity['test']==x['profile']['test'], 'ACTUAL_EXE_PRODUCER_BYTES')
        off=struct.unpack_from('<I',data,0x3c)[0]
        require(data[:2]==b'MZ' and data[off:off+4]==b'PE\0\0', 'ACTUAL_PE_SIGNATURE')
        machine=struct.unpack_from('<H',data,off+4)[0];magic=struct.unpack_from('<H',data,off+24)[0];subsystem=struct.unpack_from('<H',data,off+24+68)[0]
        require(machine==identity['machine']==0x14c and magic==0x10b and subsystem==identity['subsystem']==(3 if x['profile']['test'] else 2), 'ACTUAL_PE_ROLE')
        facts.append({**fact(p),'name':p.name,'test':x['profile']['test'],'target':x['target']['name'],'subsystem':subsystem})
    require(operation['normal']=={k:fact(Path(prod['executable']))[k] for k in ('path','bytes','sha256')}, 'OPERATION_NORMAL_EXACT')
    require({x['path']:(x['bytes'],x['sha256']) for x in operation['tests']}=={x['path']:(x['bytes'],x['sha256']) for x in facts if x['test']}, 'OPERATION_ALL_TEST_IDENTITIES')
    require(operation['metadataSha256']==fact(E/'release-tests-first.stdout.log')['sha256'] and operation['sourceFreezeSha256']==fact(E/'implementation-freeze-first.json')['sha256'], 'OPERATION_METADATA_FREEZE')
    return facts,operation

def runtime(facts,operation):
    d=js(E/'windows-release-first.json');state=terminal_status(d)
    require(operation['status']!='RUNNING' and operation.get('sourceUnchanged') is True and operation.get('scriptUnchanged') is True, 'RUNTIME_CONTROLLER_SOURCE')
    require(d['exeSha256']==operation['normal']['sha256'], 'RUNTIME_NORMAL')
    expected={x['name']:x['sha256'] for x in facts};upload_identity(d,expected)
    by_name={x['name']:x for x in facts if x['test']};names=set();counts=Counter();results=[]
    for test in d['tests']:
        name=test['name'];require(name in by_name and name not in names and test['binarySha256']==by_name[name]['sha256'], 'RUNTIME_TEST_BINARY');names.add(name)
        lines=text_lines(test['output']);summaries=[(i,x,SUMMARY.fullmatch(x)) for i,x in enumerate(lines,1) if SUMMARY.fullmatch(x)]
        require(len(summaries)==1, 'EXACT_ONE_HARNESS_SUMMARY');i,line,m=summaries[0]
        values=dict(zip(('passed','failed','ignored','measured','filtered'),map(int,m.groups()[1:6])))
        require(values['measured']==values['filtered']==0 and test['timeoutSeconds']==900, 'UNFILTERED_ORIGINAL_TEST_BUDGET')
        inventory=[{'line':n,'name':z[1],'status':z[2]} for n,x in enumerate(lines,1) if (z:=ITEM.fullmatch(x))]
        require(len({x['name'] for x in inventory})==len(inventory), 'DUPLICATE_HARNESS_TEST_NAME')
        tally=Counter('ignored' if x['status'].startswith('ignored') else x['status'] for x in inventory)
        require(tally['ok']==values['passed'] and tally['FAILED']==values['failed'] and tally['ignored']==values['ignored'], 'SUMMARY_VS_ACTUAL_TEST_LINES')
        run=[int(m[1]) for x in lines if (m:=re.fullmatch(r'running (\d+) tests?',x))]
        require(run==[sum(values[k] for k in ('passed','failed','ignored'))], 'RUNNING_VS_SUMMARY_INVENTORY')
        require((test['exitCode']==0)==(values['failed']==0), 'HARNESS_EXIT_SUMMARY_CONSISTENCY')
        counts.update(values);results.append({'name':name,'binarySha256':test['binarySha256'],'exitCode':test['exitCode'],'normalizedOutputLineCount':len(lines),'normalizedOutputCanonicalSha256':sha(json.dumps(lines,ensure_ascii=False,separators=(',',':')).encode()),'summaryLine':i,'summary':line,**values,'inventory':inventory})
    if d['status']=='PASS':
        require(len(names)==21 and counts['failed']==0 and operation['status']=='PASS_COMMAND' and operation['exitCode']==0, 'CLAIMED_RUNTIME_PASS_INCOMPLETE')
    return {**state,'report':fact(E/'windows-release-first.json'),'controllerStatus':operation['status'],'controllerExitCode':operation['exitCode'],'controllerRaw':controller_logs('windows-release-first',operation,True),'executedHarnesses':len(names),'expectedHarnesses':21,'completeInventory':len(names)==21,'counts':dict(counts),'harnesses':results}

def literal_tuple(script,name):
    for node in ast.parse(read(script)).body:
        if isinstance(node,ast.Assign) and any(isinstance(t,ast.Name) and t.id==name for t in node.targets):
            return ast.literal_eval(node.value)
    raise ValueError('EXPECTED_LITERAL_NOT_FOUND')

def hidden(d,kind,normal):
    require(d['exeSha256']==normal,'FLOW_NORMAL_BINDING')
    if kind!='public':upload_identity(d,{'fc-bootstrap.exe':normal})
    state=terminal_status(d);native=d['native']
    if kind in ('public','visual'):
        prefix,key,constant=(('PUBLIC_DESKTOP_NATIVE','publicDesktop','PUBLIC_DESKTOP_STEPS') if kind=='public' else ('SUBJECT_VISUAL_NATIVE','subjectVisual','SUBJECT_VISUAL_STEPS'))
        names=literal_tuple(MAIN/'scripts/windows-probe.py',constant)
        lines=text_lines(native[key+'Output']);actual=[x for x in lines if x.startswith(prefix+'_STEP')]
        expected=[f'{prefix}_STEP index={i} name={name}' for i,name in enumerate(names)]
        require(actual==expected and native[key+'ExitCode']==0 and d[key+'NativeStatus']=='PASS' and d[key+'NativeStepNames']==list(names), 'HIDDEN_ORDERED_STEPS')
        footer=[x for x in lines if x.startswith(prefix+'_PASS')]
        require(len(footer)==1 and lines.index(footer[0])>lines.index(expected[-1]) and 'shared_workers=2' in footer[0] and 'isolated_cleanup=PASS' in footer[0], 'HIDDEN_PHYSICAL_FINAL_MARKER')
        if kind=='public':require('public_session=NONE' in footer[0] and 'old_scene=REJECTED' in footer[0], 'PUBLIC_NO_ACCOUNT_AND_STALE_REJECT')
        return {**state,'steps':len(names),'orderedNames':list(names),'footer':footer[0]}
    lines=text_lines(native['reinitializationOutput'])
    children=[x for x in lines if x.startswith('REINITIALIZATION_CHILD_PASS stage=')]
    require([x.split('stage=',1)[1].split()[0] for x in children]==['cancel','commit','reopen'],'RESET_CHILD_STAGES')
    nonces=[re.search(r'\bnonce=([0-9a-f]+)\b',x)[1] for x in children];require(len(set(nonces))==1,'RESET_SAME_NONCE')
    footer=[x for x in lines if x.startswith('reinitialization_native=PASS ')]
    required=('child_processes=3','cancel_exact_preservation=PASS','default_and_partial_confirmation_denied=PASS','full_reset=PASS','new_dpapi=PASS','new_installation=PASS','native_setup=PASS','old_account_absent=PASS','old_backup_direct_restore_denied=PASS','external_saved_copy=UNCHANGED','foreground=UNTOUCHED','real_run=UNTOUCHED','isolated_cleanup=PASS')
    require(native['reinitializationExitCode']==0 and len(footer)==1 and all(x in footer[0] for x in required) and d['reinitializationNativeStatus']=='PASS','RESET_ORIGINAL_FINAL_FACTS')
    return {**state,'actualChildStages':['cancel','commit','reopen'],'sameNonce':True,'footer':footer[0]}

def extra(facts):
    normal=next(x['sha256'] for x in facts if not x['test']);d=js(E/'windows-release-first.json')
    public=hidden(d,'public',normal)
    operation=js(E/'windows-release-extra-flows-first.json');require(operation['normal']['sha256']==normal,'EXTRA_FLOW_NORMAL')
    rows=[]
    for kind in ('visual','repair'):
        receipt=next(x for x in operation['operations'] if x['kind']==kind)
        report=js(E/f'windows-release-{kind}-first.json')
        rows.append({'kind':kind,'observed':hidden(report,kind,normal),'controllerExitCode':receipt['exitCode'],'raw':controller_logs(f'windows-release-{kind}-first',receipt,True),'report':fact(E/f'windows-release-{kind}-first.json')})
    return {'public':public,'extraControllerStatus':operation['status'],'extra':rows}

def raw_native(d):
    require(d['rawPreservation']['status']=='PASS','RAW_NATIVE_PRESERVATION')
    result={}
    for name,claim in d['rawPreservation']['files'].items():
        p=Path(claim['artifact']);require(p.is_relative_to(E) and p.resolve().is_relative_to(E),'RAW_PATH_OUTSIDE_R66')
        data=read(p);require(claim['status']=='PASS' and claim['remote']==claim['downloaded'] and claim['remote']['bytes']==len(data) and claim['remote']['sha256']==sha(data) and base64.b64decode(claim['base64'],validate=True)==data,'RAW_NATIVE_FILE_IDENTITY')
        result[name]=data
    return result

def tls_base(name,normal):
    d=js(E/(name+'.json'));state=terminal_status(d)
    require(d['exeSha256']==normal,'TLS_NORMAL_BINDING')
    raw=raw_native(d);lines=raw['tls.stdout'].decode('utf-8').splitlines()
    passes=[json.loads(x.split(' ',1)[1]) for x in lines if x.startswith('TLS_BUSINESS_PASS ')]
    progress=[json.loads(x.split(' ',1)[1]) for x in lines if x.startswith('TLS_BUSINESS_PROGRESS ')]
    require(passes==d['nativeReports'] and (not passes or passes==[d['native']]) and progress==d['nativeProgress'],'TLS_ORIGINAL_NATIVE_COPIES')
    for p in passes:
        require(p['sharedWorkers']==2 and p['workerJoin'] is True and p['physicalNativeDrain'] is True and p['setupCleanup'] is True and all(p[k]==0 for k in ('hubPending','downloadHubPending','quarantined','approvals')),'TLS_PHYSICAL_WORKER_NATIVE_HUB_CLEANUP')
    if d['status']=='PASS':require(len(passes)==1,'PASS_REQUIRES_ACTUAL_NATIVE_FOOTER')
    require(d['clientEventsSha256']==sha(json.dumps(d['events'],sort_keys=True,separators=(',',':')).encode()),'CLIENT_EVENTS_IDENTITY')
    if d['cleanup']=='PASS':
        c=d['cleanupDetails'];require(c['status']=='PASS' and c['rootAbsent'] is True and c['cngKeyExists'] is False and c['myDigestRestored'] is True and c['ownedProcessesRemaining']==0,'TLS_EXACT_FIXTURE_CLEANUP')
        require(d['ownedProcessCleanup']['status']=='PASS' and d['ownedProcessCleanup']['ownedProcessesRemaining']==0 and d['ownedTunnelCleanup']=='PASS','TLS_OWNED_PROCESS_CLEANUP')
    if d['status']=='PASS':require(d.get('nativeProcessExit')=={'code':0},'TLS_PASS_REQUIRES_ACTUAL_NATIVE_EXIT0')
    return d,{**state,'report':fact(E/(name+'.json')),'nativeFooter':passes,'actualNativeProcessExit':d.get('nativeProcessExit'),'rawNativeFiles':{k:{'bytes':len(v),'sha256':sha(v)} for k,v in raw.items()},'originalStrictP0':d.get('strictP0'),'productionListener':d.get('productionListener')}

def business(normal):
    name='windows-release-business-continuous-browser-first';d,out=tls_base(name,normal);b=d['browser']
    node=js(Path(b['nodeReportPath']));require(sha(read(Path(b['nodeReportPath'])))==b['nodeReport']['sha256'],'NODE_REPORT_BINDING')
    owner=js(Path(b['ownerReportPath']));require(sha(read(Path(b['ownerReportPath'])))==b['ownerReport']['sha256'],'EDGE_OWNER_REPORT_BINDING')
    require(b['nodeActualExitCode']==b['ownerActualExitCode']==0 and b['cleanup']=='PASS' and b['ownerPhysicalCleanup']=='PASS' and node['contextDisposed'] is True and node['cdpDisconnected'] is True and owner['status']==owner['cleanup']=='PASS','BROWSER_PHYSICAL_OWNER_END')
    for pair in (b['nodeRaw'],b['ownerSupervisorRaw']):
        for k in ('stdout','stderr'):stream(pair[k])
    network=node['network'];flags=('servedFromCache','fromDiskCache','fromServiceWorker','fromPrefetchCache')
    require(all(x['finished'] is True and x['bodyVerified'] is True for x in network),'BROWSER_FINISHED_BODY_VERIFIED')
    wire=[x for x in network if x['wireStatus'] is not None and (x['wireStatus']==304 or not any(x[f] for f in flags))]
    require(wire==node['wireResponses'] and len(wire)==node['wireResponseCount']==b['wireResponseCount'],'BROWSER_WIRE_NOT_CACHE')
    http=[x for x in d['events'] if type(x.get('httpStatus')) is int]
    native=d['native'];require(native['heads']==native['responses']==len(http)+len(wire),'NATIVE_RAW_PLUS_BROWSER_EXACT_ACCOUNTING')
    require(node['status']=='PASS' and node['contentPosts']==1 and node['sessionCsrfMatched'] is True and node['writeOutcome']=='COMMITTED' and node['pageErrorCount']==node['consoleErrorCount']==0,'BROWSER_ACTUAL_COMMIT')
    interval=d['business']['continuousAcceptance'];xs=interval['originalIntervals'];require(len(xs)==4 and len({(x['generation'],x['slot'],x['serial']) for x in xs})==4 and all(x['deadlineNs']-x['admittedAtNs']==120_000_000_000 for x in xs) and interval['sameOriginalReceipts']==4 and interval['capacityCloses']==2,'CONTINUOUS_EXACT_ORIGINAL_INTERVALS')
    out.update(rawClientEvents=len(d['events']),rawHttp=len(http),browserWire=len(wire),nativeHeads=native['heads'],nativeResponses=native['responses'],apiResponses=node['apiResponses'],contentPosts=node['contentPosts'],visualReads=node.get('visualReads'),reminderShortcutClicks=node.get('reminderShortcutClicks'),nodeStatus=node['status'],nodeActualExitCode=b['nodeActualExitCode'],ownerActualExitCode=b['ownerActualExitCode'],ownerStatus=owner['status'],continuousOriginalIntervals=xs)
    return out

def header(normal):
    d,out=tls_base('windows-release-header-deadline-first',normal);events=d['events'];timing=[]
    for x in events:
        require(type(x['beforeConnectNs']) is int and type(x['peerCloseNs']) is int and x['elapsedNs']==x['peerCloseNs']-x['beforeConnectNs']>=0,'HEADER_RAW_INTERVAL_MATH')
        require(x['peerCloseObserved'] is True and x['localCloseUsedAsPeerEvidence'] is False and x['beforeConnectNs']<=x['localConnectReturnedNs']<=x['peerCloseNs']<=x['localCloseBeganNs']<=x['localCloseReturnedNs'],'HEADER_PEER_NOT_LOCAL_CLOSE')
        timing.append({'case':x['case'],'elapsedNs':x['elapsedNs'],'withinOriginal5e9':x['elapsedNs']<=5_000_000_000,'status':x['status']})
    expected=['header-deadline-tls-'+str(i) for i in range(1,5)]+['header-deadline-raw-tcp']
    actual=[x['case'] for x in events]
    require(actual==expected[:len(actual)],'HEADER_DISTINCT_ORIGINAL_CASE_ORDER')
    complete=actual==expected
    obs=d.get('headerDeadlineObservation')
    if obs is not None:require(obs['thresholdNs']==5_000_000_000 and obs['epoch']=='PER_CONNECTION_BEFORE_LOCAL_TCP_CONNECT_CALL','HEADER_ORIGINAL_THRESHOLD_EPOCH')
    within=all(x['withinOriginal5e9'] for x in timing)
    if d['status']=='PASS':require(complete and within and obs is not None,'CLAIMED_HEADER_PASS_INCOMPLETE_OR_ABOVE_THRESHOLD')
    out.update(samples=timing,completeFiveSamples=complete,allObservedSamplesWithinOriginalThreshold=within,thresholdNs=5_000_000_000,scope='Finite pre-local-connect through observed peer-close upper interval; no kernel ESTABLISHED/backlog, arbitrary-scheduling hard-real-time, per-client/native-slot association or production P0 promotion.')
    return out


def tls_controller(normal):
    d=js(E/'windows-release-tls-series-first.json')
    require(d['normal']['sha256']==normal and d['sourceFreezeSha256']==fact(E/'implementation-freeze-first.json')['sha256'],'TLS_CONTROLLER_SOURCE_NORMAL')
    rows=[]
    require([x['kind'] for x in d['operations']]==['business-continuous-browser','header-deadline'],'TLS_SERIES_BOTH_INDEPENDENT_OPERATIONS')
    for x in d['operations']:
        require(x['sourceUnchanged'] is True and x['status']!='RUNNING','TLS_CONTROLLER_TERMINAL_SOURCE')
        original=js(E/('windows-release-'+x['kind']+'-first.json'))
        require(x['resultStatus']==original['status'] and x['cleanup']==original['cleanup'],'TLS_CONTROLLER_ORIGINAL_RESULT_MATCH')
        rows.append({'kind':x['kind'],'status':x['status'],'resultStatus':x['resultStatus'],'cleanup':x['cleanup'],'exitCode':x['exitCode'],'elapsedSeconds':x['elapsedSeconds'],'raw':controller_logs('windows-release-'+x['kind']+'-first',x,True)})
    return {'controllerStatus':d['status'],'operations':rows,'interpretation':'Header failure remains independent; this audit never replaces either original status.'}

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--report',type=Path,default=E/'independent-runtime-first.json')
    args=parser.parse_args()
    require(args.report.parent.resolve()==E.resolve() and not args.report.exists(),'EXCLUSIVE_R66_REPORT_REQUIRED')
    missing=[n for n in REQUIRED if not (E/n).is_file()]
    if missing:
        print(json.dumps({'status':'WAITING_INPUTS_NO_REPORT_WRITTEN','missing':missing}));return 2
    for n in REQUIRED:
        d=json.loads((E/n).read_text())
        if d.get('status') in ('RUNNING','IN_PROGRESS'):
            print(json.dumps({'status':'WAITING_TERMINAL_NO_REPORT_WRITTEN','record':n}));return 2
    report={'schemaVersion':1,'recordedUtc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'helper':fact(Path(__file__)),'scope':'Independent local read of terminal original R66 receipts/artifact bytes. No Windows, compile, network, CI, source edits or historical-result replacement.','sections':{},'auditErrors':[]}
    def section(name,fn):
        try:
            report['sections'][name]={'auditStatus':'CHECKED',**fn()}
        except Exception as exc:
            report['sections'][name]={'auditStatus':'INSUFFICIENT_OR_INVALID_EVIDENCE'}
            report['auditErrors'].append({'section':name,'kind':type(exc).__name__,'code':str(exc) if isinstance(exc,ValueError) and re.fullmatch('[A-Z0-9_]+',str(exc)) else 'CHECK_FAILED_NO_RAW_ERROR_DISCLOSURE'})
    section('sourceAndBuild',source_build)
    try:
        facts,operation=programs();report['programs']=facts
    except Exception as exc:
        report['auditErrors'].append({'section':'programs','kind':type(exc).__name__,'code':'PROGRAM_IDENTITY_CHECK_FAILED'});facts=[];operation={}
    if facts:
        normal=next(x['sha256'] for x in facts if not x['test'])
        section('workspaceRuntime',lambda:runtime(facts,operation))
        section('hiddenNativeFlows',lambda:extra(facts))
        section('tlsController',lambda:tls_controller(normal))
        section('businessBrowser',lambda:business(normal))
        section('headerDeadline',lambda:header(normal))
    report['inputs']=list(INPUTS.values())
    report['status']='REVIEW_COMPLETE_WITH_EVIDENCE_ERRORS' if report['auditErrors'] else 'REVIEW_COMPLETE_SEPARATE_OBSERVED_RESULTS'
    report['limits']=['Actual Windows11/WOW64 GNU release only, not Win7/10/visible Seewo QA.','No fixed passed-test total assumed; summaries are independently reconciled to actual per-test lines.','Controller raw byte streams and normalized per-harness output arrays are distinct evidence objects.','A header FAIL remains in its own section and does not relabel independent successful runtime/native/browser runs.','436 current crate/build inputs and377 historical snapshots are different scopes; current qualification file is authoritative.','No installed-MSI or production-listener readiness claim.']
    with args.report.open('x',encoding='utf-8') as out:json.dump(report,out,ensure_ascii=False,indent=2);out.write('\n')
    print(json.dumps({'status':report['status'],'report':str(args.report),'bytes':args.report.stat().st_size,'sha256':sha(args.report.read_bytes()),'auditErrorCount':len(report['auditErrors'])}))
    return 1 if report['auditErrors'] else 0

if __name__=='__main__':
    sys.exit(main())
