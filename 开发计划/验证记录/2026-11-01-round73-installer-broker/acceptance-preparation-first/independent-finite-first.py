"""Prepared finite R73 evidence audit; root executes after terminal originals.
No Windows/Cargo/PE execution, network, source mutation, cleanup or key read.
Missing/nonterminal required JSON yields WAIT/no report. Only actual finite GNU
Win11 and Desktop5.1 pure contract scope may pass; no MSI/hosted-CI promotion.
"""
from pathlib import Path
import argparse, ast, base64, datetime, hashlib, json, re, runpy, struct, sys
sys.dont_write_bytecode = True
ROOT = Path('/tmp/fc-round73-installer-broker'); EV = ROOT / 'evidence'; W = ROOT / 'worktree'
MAIN = Path('/workspace/共享/Eliora/未来教室计划')
TEMPLATE = MAIN / '开发计划/验证记录/2026-11-01-round67-ci/independent-runtime-first.py'
TEMPLATE_SHA = '0317ffb1719737bd9dfa45ca7952c242df6ffff0baa6b1611d7b912277f9e0e5'
SUCCESS = 'PASS_R73_FINITE_GNU_WIN11_AND_PURE_PS_ONLY'
PINS = {
 'n-release-third.json':'858d3c62e4d3c4e6739a5763a77e26ccf368038a22d4ef150f5d284a080b5b07',
 'n-producer-terminal-third.json':'e5a5041f66e359aef2e1e865760ebd2dce71d0c5a3e45f1fb6314f5801988ae9',
 'root-windows-gnu-full-third.json':'0f306c60555b316e48b6046194c172926a46b4e7df82549c2e48ab457c27696f',
 'root-windows-gnu-full-third.command.json':'fec8be2067db62892afc3babf8cc178e122130ccaa96e939fcdb85da122a1cab',
 'root-parse-native-children-first.py':'8dfd6c4731f25a4e981a985b7c85566889e6100e83f49180e70555a29ee69a62',
 'root-native-child-attribution-first.json':'2091d3213b7883a5b66a3bb39f2606fb8492da22b7fc5bde168e226ead1e0993',
 'powershell-contract-preparation-first/windows-powershell-contract-second.json':'3ef8b20183e35217c840436000ee8658fbac68473dd103874f5dc35022b1bdb9',
 'powershell-contract-preparation-first/run-powershell-contract-second.py':'b03734a8be5207aeccb4f952e09a7ce8fa5bb447136edc916af6ca35e45144d1',
 'powershell-contract-preparation-first/windows-powershell-contract-first.json':'27c43f4fd5624a982fa2b72c041694e3a3a647ec4a57055ec0b2052d836507bd',
 'root-windows-gnu-full-first.json':'37a64d413fc4a3560eac92fdeaa2a00644b28935036b85fb393956b120875b82',
 'windows-readonly-final-first.json':'eb279c7fd15815a4a5f60b34472114abab983d62ea5ec5678cef01fd7e6d8387',
 'windows-readonly-operation-first.json':'99e62deb733dd518f8c11494dd30a53b3f5f9bffa8490bf24ef00aea3e827f16'}
class Waiting(Exception): pass
def need(ok, code):
    if not ok: raise AssertionError(code)
def sha(data): return hashlib.sha256(data).hexdigest()
def read(path):
    p = Path(path)
    if not p.is_file(): raise Waiting('MISSING_' + p.name)
    need(not p.is_symlink(), 'REGULAR_ORIGINAL_REQUIRED')
    try: value = json.loads(p.read_text(encoding='utf-8-sig'))
    except (ValueError, UnicodeError): raise Waiting('INCOMPLETE_' + p.name) from None
    if isinstance(value, dict) and value.get('status') in ('RUNNING','STARTED','in_progress','queued','OBSERVATION_INTERRUPTED_NOT_COMPLETION'): raise Waiting('NONTERMINAL_' + p.name)
    return value
def fact(path):
    p = Path(path)
    if not p.is_file(): raise Waiting('MISSING_' + p.name)
    need(not p.is_symlink() and (p.resolve().is_relative_to(ROOT) or p.resolve().is_relative_to(MAIN/'开发计划/验证记录')), 'SCOPED_REGULAR_ORIGINAL_ONLY')
    h = hashlib.sha256(); size = 0
    with p.open('rb') as stream:
        while data := stream.read(1024*1024): h.update(data); size += len(data)
    return dict(path=str(p), bytes=size, sha256=h.hexdigest())
def exact(row):
    p = Path(row['path']); p = p if p.is_absolute() else EV / p
    current = fact(p); need(current['sha256'] == row['sha256'] and ('bytes' not in row or current['bytes'] == row['bytes']), 'EXACT_ORIGINAL_BYTES'); return current
def encoded(row):
    data = base64.b64decode(row['base64'], validate=True)
    need(len(data) == row['bytes'] and sha(data) == row['sha256'], 'EXACT_ENCODED_RAW')
    if 'path' in row: need(Path(row['path']).read_bytes() == data, 'ENCODED_AND_REOPENED_RAW_EQUAL')
    return data
def pe(row, subsystem):
    identity = exact(row); path = Path(identity['path'])
    with path.open('rb') as stream:
        dos = stream.read(64); need(len(dos)==64 and dos[:2]==b'MZ', 'ACTUAL_PE_DOS')
        offset = struct.unpack_from('<I', dos, 60)[0]; need(offset <= identity['bytes']-96, 'ACTUAL_PE_HEADER_BOUNDS'); stream.seek(offset); header = stream.read(96)
    need(header[:4]==b'PE\0\0' and struct.unpack_from('<H',header,4)[0]==0x14c and struct.unpack_from('<H',header,24)[0]==0x10b and struct.unpack_from('<H',header,92)[0]==subsystem==row['peSubsystem'], 'ACTUAL_I386_PE32_GUI_CUI')
    return identity
def main():
    parser=argparse.ArgumentParser(description=__doc__); parser.add_argument('--report',type=Path,default=EV/'independent-finite-acceptance-first.json'); args=parser.parse_args()
    need(args.report.parent.resolve()==EV and not args.report.exists(), 'EXCLUSIVE_NEW_OUTPUT')
    report=dict(schemaVersion=1,status='FAIL_R73_FINITE_EVIDENCE_CHECK',recordedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat())
    try:
        need((ROOT/'.owner').read_bytes()==b'future-classroom-owned-r73-installer-broker/v1\n','EXACT_R73_OWNER')
        # Preflight all required terminal JSONs before inspecting any payload.
        values={name:read(EV/name) for name in PINS if name.endswith('.json')}
        identities={name:fact(EV/name) for name in PINS}; need(all(identities[n]['sha256']==h for n,h in PINS.items()),'EXACT_ROOT_CONFIRMED_ORIGINALS')
        need(fact(TEMPLATE)['sha256']==TEMPLATE_SHA,'UNCHANGED_R67_PARSER'); old=runpy.run_path(str(TEMPLATE))
        product=values['n-release-third.json']; terminal=values['n-producer-terminal-third.json']
        need(product['status']=='PASS_CODEGEN_NOT_RUNTIME' and product['originalPerCommandBudgetSeconds']==900 and product['sourceUnchanged'] is True and product['sourceBefore']==product['sourceAfter'] and len(product['sourceBefore'])==16,'N3_FROZEN16_SOURCE_BEFORE_AFTER')
        need(terminal['status']=='PASS_CONTROLLER_ACTUAL_EXIT0_REAPED' and terminal['actualToolTerminal']['exitCode']==0 and terminal['actualToolTerminal']['runningSessionReturned'] is False and terminal['report']['sha256']==PINS['n-release-third.json'] and terminal['sourceFreeze']['sha256']==product['freezeSha256'],'N3_ACTUAL_PRODUCER_PARENT_REAPED')
        exact(terminal['sourceFreeze']); exact(terminal['cargoMetadata'])
        source_rows={row['path']:row for row in product['sourceBefore']}; need(len(source_rows)==16,'UNIQUE_SOURCE16')
        for row in source_rows.values(): exact(dict(row,path=str(W/row['path'])))
        target=str(ROOT/'target'); common=['--locked','--offline','--target','i686-pc-windows-gnu','--target-dir',target]
        expected={
            'nightly-workspace-strict-clippy':['rustup','run','nightly-2026-10-29','cargo','clippy',*common,'--workspace','--all-targets','--','-D','warnings'],
            'stable-release-tests':['rustup','run','1.97.1','cargo','test',*common,'--release','--workspace','--all-targets','--no-run','--message-format=json'],
            'stable-release-bins':['rustup','run','1.97.1','cargo','build',*common,'--release','-p','fc-bootstrap','--bins','--message-format=json']}
        steps={row['name']:row for row in product['steps']}; need(set(steps)==set(expected) and len(steps)==3,'EXACT_THREE_ORIGINAL_GNU_COMMANDS')
        for name,row in steps.items():
            need(row['command']==expected[name] and row['status']=='PASS' and row['exitCode']==0 and row['actualExit'] is True and row['timedOut'] is False and 0<=row['elapsedSeconds']<=900,'ACTUAL_ORIGINAL_GNU_COMMAND_FLAGS_AND_BUDGET')
            exact(row['stdout']); exact(row['stderr'])
        tests=product['testExecutables']; normals=product['normalExecutables']; need(len(tests)==product['testHarnessCount']==22 and len(normals)==product['normalExecutableCount']==2 and {r['target'] for r in normals}=={'fc-bootstrap','fc-installer-lifecycle'},'ACTUAL22_TESTS_TWO_CLOSED_NORMALS')
        artifact_rows=tests+normals; need(len({r['path'] for r in artifact_rows})==24 and len({Path(r['path']).name for r in artifact_rows})==24,'UNIQUE24_ARTIFACTS')
        artifacts={r['path']:r for r in artifact_rows}; current_artifacts=[]
        for row in artifact_rows:
            is_test=row in tests; need(row['profile']['test'] is is_test,'ACTUAL_TEST_FALSE_NORMAL_PROFILE')
            current_artifacts.append(pe(row,3 if is_test else 2))
            if not is_test:
                need(row['kind']==['bin'] and '/crates/fc-bootstrap#' in row['packageId'],'CLOSED_NORMAL_PACKAGE_BIN')
                inspector=row['independentObjdump']; need(inspector['exitCode']==0,'NORMAL_ORIGINAL_STATIC_INSPECTION_EXIT'); exact(inspector['stdout']); exact(inspector['stderr'])
        association=[]
        for step_name,selected in [('stable-release-tests',tests),('stable-release-bins',normals)]:
            expected_rows={r['path']:r for r in selected}; seen=set()
            for number,line in enumerate(Path(exact(steps[step_name]['stdout'])['path']).read_bytes().splitlines(keepends=True),1):
                try: value=json.loads(line)
                except (ValueError,UnicodeError): continue
                if value.get('reason')!='compiler-artifact' or not value.get('executable'): continue
                selected_profile=value.get('profile',{}).get('test') is (step_name=='stable-release-tests')
                if not selected_profile: continue
                path=value['executable']; need(path in expected_rows,'NO_UNLISTED_CARGO_EXECUTABLE'); row=expected_rows[path]
                need(path not in seen and row['target']==value['target']['name'] and row['kind']==value['target']['kind'] and row['profile']==value['profile'] and row['packageId']==value['package_id'] and path in value['filenames'],'EXACT_CARGO_TARGET_PACKAGE_PROFILE_PRODUCER'); seen.add(path)
                association.append(dict(path=path,compileCommand=step_name,cargoLine=number,lineSha256=sha(line),profileTest=value['profile']['test']))
            need(seen==set(expected_rows),'COMPLETE_ORIGINAL_CARGO_EXECUTABLE_SET')
        app=next(r for r in normals if r['target']=='fc-bootstrap'); helper=next(r for r in normals if r['target']=='fc-installer-lifecycle')
        full=values['root-windows-gnu-full-third.json']; operation=values['root-windows-gnu-full-third.command.json']
        need(operation['status']=='PASS_FINITE_GNU_WIN11_FULL22_ONLY' and operation['exitCode']==0 and operation['actualExit'] is True and operation['actualStatus']==operation['cleanup']=='PASS' and operation['originalPerTestBudgetSeconds']==900 and operation['inputsUnchanged'] is True,'ACTUAL_FULL_PARENT_TERMINAL')
        for row in operation['inputIdentities'].values(): exact(row)
        exact(operation['stdout']); exact(operation['stderr']); need(exact(operation['actualReport'])==fact(EV/'root-windows-gnu-full-third.json'),'ACTUAL_FULL_REPORT_IDENTITY')
        need(operation['normalHelper']==helper,'NORMAL_HELPER_PIN_ONLY')
        cmd=operation['command']; need(cmd[:2]==['python3','-B'] and cmd.count('--exe')==1 and cmd[cmd.index('--exe')+1]==app['path'] and helper['path'] not in cmd and cmd[cmd.index('--test-timeout-seconds')+1]=='900' and cmd[cmd.index('--native-seconds')+1]=='1' and '--system-host' in cmd and '--public-desktop' in cmd and cmd[cmd.index('--test-metadata')+1]==exact(steps['stable-release-tests']['stdout'])['path'],'ORIGINAL_FULL900_NATIVE1_SYSTEM_PUBLIC_METADATA')
        old['check_platform'](full); need(full['exeSha256']==app['sha256'] and full['system']['exitCode']==0 and full['native']['hostExitCode']==0,'ACTUAL_NORMAL_PLATFORM_SYSTEM_HOST')
        expected_uploads={Path(r['path']).name:r['sha256'] for r in tests+[app]}
        need(len(full['uploadedArtifacts'])==23 and {r['name']:r['sha256'] for r in full['uploadedArtifacts']}==expected_uploads and helper['sha256'] not in expected_uploads.values(),'EXACT23_UPLOADS_HELPER_NORMAL_NOT_EXECUTED')
        need(len(full['tests'])==22 and {r['name'] for r in full['tests']}=={Path(r['path']).name for r in tests},'ACTUAL_COMPLETE22_HARNESS_SET')
        attribution=runpy.run_path(str(EV/'root-parse-native-children-first.py')); harnesses=[]
        for item in full['tests']:
            need(item['binarySha256']==expected_uploads[item['name']],'ACTUAL_HARNESS_UPLOAD_IDENTITY')
            parsed=attribution['native_harness'](item) if item['name']=='fc_windows-ac67b5f1df5ec8b2.exe' else old['harness'](item)
            harnesses.append(parsed)
        totals={k:sum(r[k] for r in harnesses) for k in ('passed','failed','ignored','measured','filteredOut')}; need(totals==operation['totals'] and all(totals[k]==0 for k in ('failed','ignored','measured','filteredOut')),'ALL_ACTUAL_CASE_SUMMARIES_AND_NO_FAILURE_SKIP_FILTER')
        probe=W/'scripts/windows-probe.py'; expected_public=None
        for node in ast.parse(probe.read_bytes()).body:
            if isinstance(node,ast.Assign) and any(isinstance(t,ast.Name) and t.id=='PUBLIC_DESKTOP_STEPS' for t in node.targets): expected_public=ast.literal_eval(node.value)
        need(expected_public is not None and len(expected_public)==20,'SOURCE_PINNED_PUBLIC20'); public=old['flow'](full,'public',expected_public)
        ps=values['powershell-contract-preparation-first/windows-powershell-contract-second.json']
        need(ps['status']=='PASS_ACTUAL_WINDOWS_PURE_POWERSHELL_CONTRACT_ONLY' and ps['budgetSeconds']==120 and 0<=ps['elapsedSeconds']<=120 and ps['cleanup']=='PASS' and ps['sourceUnchanged'] is True and ps['checksCompleted'] is True and ps['discovery']['pwsh']==[],'ACTUAL_PURE_PS_DESKTOP_ONLY_AND_CLEANUP')
        exact(ps['helper']); need(len(ps['versions'])==1 and ps['versions'][0]['actualVersion'].startswith('5.1.') and ps['versions'][0]['edition']=='Desktop' and ps['versions'][0]['ciHostedPowerShell72Proof'] is False,'ACTUAL_DESKTOP51_NOT_HOSTED72')
        for row in ps['inputs'].values(): exact(row)
        need([r['label'] for r in ps['operations']]==['01-create','02-scp','03-scp','04-discovery','05-contract-desktop51','06-post-source-hashes','99-finally-cleanup'],'COMPLETE_PURE_PS_TRANSPORT_OPERATIONS')
        for op in ps['operations']:
            need(op['exitCode']==0 and op['actualLocalParentReturned'] is True and op['timedOut'] is False,'EVERY_SSH_SCP_ACTUAL_PARENT_RETURNED')
            encoded(op['stdout']); encoded(op['stderr'])
            need(op['command'][0] in ('ssh','scp') and op['command'][op['command'].index('-i')+1]=='/config/.local/share/future-classroom-test-access/win11-192.168.10.212','IDENTITY_TRANSPORT_PATH_ONLY')
        need(ps['cleanupResult']['status']=='REMOVED' and ps['cleanupResult']['marker']==ps['marker'] and ps['remoteRoot'].endswith(ps['leaf']),'EXACT_OWNED_PS_ROOT_REMOVED')
        contract_bytes=base64.b64decode(ps['versions'][0]['actualContract']['base64'],validate=True); contract= json.loads(contract_bytes)
        need(len(contract_bytes)==ps['versions'][0]['actualContract']['bytes'] and sha(contract_bytes)==ps['versions'][0]['actualContract']['sha256'],'ACTUAL_CONTRACT_PIPELINE_JSON_BYTES')
        check_source=(W/'scripts/check-msvc-gui-contract.ps1').read_text(); check_names=re.findall(r"^\s*Check '([^']+)'",check_source,re.M)
        need(contract['status']=='PASS_PURE_POWERSHELL_GUI_CONTRACT_ONLY' and contract['sourceSha256']==contract['sourceExpectedSha256']==source_rows['scripts/windows-msvc-ci.ps1']['sha256'] and contract['fullSourceParseErrors']==0 and contract['fatal'] is None and [r['name'] for r in contract['checks']]==check_names and all(r['status']=='PASS' and r['assertions']>0 for r in contract['checks']),'REAL_COMPLETE_FROZEN_SOURCE_PURE_PS_CONTRACT')
        need(all(contract[k] is False for k in ('ciEntrypointExecuted','filesystemOrEnvironmentWritten','processOrThreadStarted')) and all(contract[k]=='NOT_RUN' for k in ('windowsRuntime','actualProductPe','actualRedirectedDiagnostic')),'CHECKER_PURE_SCOPE_NO_PRODUCT_OR_CI_RUN')
        first=values['powershell-contract-preparation-first/windows-powershell-contract-first.json']; need(first['status']=='FAIL_WINDOWS_PURE_POWERSHELL_CONTRACT_OR_CLEANUP' and first['cleanup']=='PASS' and first['failure']['kind']=='JSONDecodeError' and first['versions']==[],'FIRST_POLICY_FAILURE_NOT_REWRITTEN')
        first_op=next(r for r in first['operations'] if r['label']=='05-contract-desktop51'); first_error=encoded(first_op['stderr']); encoded(first_op['stdout']); need(first_op['exitCode']==1 and b'PSSecurityException' in first_error and b'UnauthorizedAccess' in first_error,'ORIGINAL_POLICY_REFUSAL_BEFORE_CHECKER')
        n2=values['root-windows-gnu-full-first.json']; native_first=next(r for r in n2['tests'] if r['name'].startswith('fc_windows-')); summaries=[old['SUMMARY'].fullmatch(s) for s in native_first['output'] if old['SUMMARY'].fullmatch(s)]
        need(n2['status']=='FAIL' and n2['cleanup']=='PASS' and native_first['exitCode']==101 and summaries[-1][1]=='FAILED' and list(map(int,summaries[-1].groups()[1:6]))==[243,18,0,0,0],'N2_NATIVE243_18_FAIL_RETAINED')
        qualification=read(EV/'root-native-harness-third-terminal-qualification.json'); need(qualification['actualRootParentExitCode']==1 and qualification['remoteProbeStatus']==qualification['remoteCleanup']=='PASS','ORIGINAL_N3_NATIVE_ROOT_PARSER_FAILURE_QUALIFIED')
        # Deliberately do not read this old recorder through terminal read():
        # RUNNING is the original stale state after the separately recorded exit1.
        historical_recorder=json.loads((EV/'root-windows-native-harness-third.command.json').read_bytes()); need(historical_recorder['status']=='RUNNING','STALE_RECORDER_STATE_PRESERVED')
        final=values['windows-readonly-final-first.json']; audit_op=values['windows-readonly-operation-first.json']
        need(audit_op['status']=='PASS_COMMAND' and audit_op['actualExit'] is True and audit_op['exitCode']==0,'FINAL_READONLY_PARENT_ACTUAL_RETURN'); exact(audit_op['stdout']); exact(audit_op['stderr'])
        audit_cmd=audit_op['command']; need(audit_cmd[audit_cmd.index('--report')+1]==str(EV/'windows-readonly-final-first.json'),'EXACT_FINAL_READONLY_REPORT_BINDING')
        need(final['status']=='PASS' and final.get('completedAtUtc') and final['ssh']['exitCode']==0 and final['authentication']==final['integrityStatus']=='PASS' and final['scratchDirectories']==final['ownedProcesses']==final['ownedTasks']==[] and final['publicAuthorizationUnchanged'] is True and final['projectRunUnchanged'] is True and final['sshd']=='Running' and final['persistentKeyMode']=='0o600' and final['persistentKeyMetadata']['contentsRead'] is False,'FINAL_ZERO_FIXTURES_RUN_SSH_KEY_METADATA_ONLY')
        exact(dict(final['baseline'],path=str(MAIN/final['baseline']['path'])))
        for row in final['ssh'].values():
            if isinstance(row,dict) and 'base64' in row: encoded(row)
        observed=datetime.datetime.fromisoformat(final['observedAtUtc']).timestamp(); full_mtime=(EV/'root-windows-gnu-full-third.command.json').stat().st_mtime
        need(observed>=full_mtime and observed>=datetime.datetime.fromisoformat(ps['completedUtc']).timestamp(),'READONLY_AFTER_COMPLETED_FULL_AND_PS')
        for row in source_rows.values(): exact(dict(row,path=str(W/row['path'])))
        need(identities=={name:fact(EV/name) for name in PINS},'ORIGINAL_INPUTS_UNCHANGED_DURING_LOCAL_AUDIT')
        report.update(status=SUCCESS,producer=identities['n-release-third.json'],producerTerminal=identities['n-producer-terminal-third.json'],sourceBeforeAfter=product['sourceBefore'],actualArtifacts=current_artifacts,cargoAssociations=association,
            fullRegression=dict(report=identities['root-windows-gnu-full-third.json'],controller=identities['root-windows-gnu-full-third.command.json'],uploadedArtifacts=full['uploadedArtifacts'],harnesses=harnesses,totals=totals,publicDesktop=public,normalHelperExecution='NOT_RUN_PINNED_ONLY; its separate unit-test harness is part of22 tests'),
            purePowershell=dict(report=identities['powershell-contract-preparation-first/windows-powershell-contract-second.json'],actualVersion=ps['versions'][0]['actualVersion'],edition='Desktop',checks=len(contract['checks']),assertions=contract['assertions'],cleanup='PASS',hostedPowerShell72='NOT_VALIDATED',capture='Checker pipeline JSON string UTF8 bytes; actual SSH stdout/stderr separately byte/base64 checked.'),
            historical=dict(firstPolicyFailure=identities['powershell-contract-preparation-first/windows-powershell-contract-first.json'],n2NativeFailure=identities['root-windows-gnu-full-first.json'],nativeParserTerminalQualification=fact(EV/'root-native-harness-third-terminal-qualification.json'),originalStaleNativeRecorder=fact(EV/'root-windows-native-harness-third.command.json'),policyQualification=fact(EV/'powershell-contract-preparation-first/first-failure-qualification-first.json')),
            readonly=dict(audit=identities['windows-readonly-final-first.json'],controller=identities['windows-readonly-operation-first.json'],fullCompletedRecordMtimeUtc=datetime.datetime.fromtimestamp(full_mtime,datetime.timezone.utc).isoformat(),orderingQualification='Read-only observation follows the completed full-controller file mtime and explicit PS completedUtc. Full completed record contains actualExit0 after wait; mtime is local file metadata, not an invented raw process timestamp.'),
            childAttributionParser=identities['root-parse-native-children-first.py'],priorIndependentChildReview=fact(EV/'child-attribution-review-first/review-first.json'),scope='Frozen N3 local GNU22 harnesses/app self/system/hidden lifecycle/public20 on authorized Win11 build26300/WOW64, dual normal GUI headers/bytes pinning, and actual Desktop5.1 pure PS contract only. No Hosted MSVC/new CI, MSI fresh install/rollback, Win7 or Win10 OS, campus or final product acceptance. Only the current Win11 environment is provided; other OS runtime states remain NOT_RUN.',
            privateKey='No content read or hash. SSH/SCP path was transport-only; final audit preserved only0600 metadata.',helper=fact(Path(__file__)))
    except Waiting as error:
        print(json.dumps(dict(status='WAIT',reasonCode=str(error),reportCreated=False))); return 3
    except Exception as error:
        report['error']=dict(kind=type(error).__name__,code=str(error) if isinstance(error,AssertionError) else 'ORIGINAL_INPUT_OR_PARSER_FAILURE')
    data=(json.dumps(report,ensure_ascii=False,indent=2)+'\n').encode()
    with args.report.open('xb') as stream: stream.write(data)
    print(json.dumps(dict(status=report['status'],report=str(args.report),sha256=sha(data))))
    return 0 if report['status']==SUCCESS else 1
if __name__=='__main__': raise SystemExit(main())
