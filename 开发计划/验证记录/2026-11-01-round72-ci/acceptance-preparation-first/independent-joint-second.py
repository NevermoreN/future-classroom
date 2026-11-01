"""Prepared final R72 audit. Invoke only after all original controllers return.
Missing/incomplete/nonterminal required inputs yield WAIT/no report. No network,
Windows/product execution, Cargo, source mutation, deletion or Git mutation.
"""
from pathlib import Path
import argparse, ast, base64, datetime, hashlib, json, re, runpy, subprocess, sys, zipfile
sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parent.parent
REPO = Path('/workspace/共享/Eliora/未来教室计划')
SOURCE = '00120a026eea5dc4ff330f8526bf31169b373565'
TEMPLATE = REPO / '开发计划/验证记录/2026-11-01-round67-ci/independent-runtime-first.py'
TEMPLATE_SHA = '0317ffb1719737bd9dfa45ca7952c242df6ffff0baa6b1611d7b912277f9e0e5'
AUDITOR_SHA = 'c136efb0ecb5dc1dbffadadae31b933c43ac3d298c308921dd7c8e409fc75527'
CONTROLLER_SHA = '638a410737a5288fd0fe934d591cc410088f1f06569003601a5892b651b3ab37'
SUCCESS = 'PASS_R72_MODERN_AND_WIN7_TARGET_ON_WIN11_ONLY'
QUALIFIED = 'PASS_R72_MODERN_WIN11_WITH_WIN7_CI_FAILURE_PRESERVED'
CONFIG = {
 'modern': ('Modern', 41296599777, 'compile-i686', '.github/workflows/windows-msvc.yml', 'future-classroom-msvc'),
 'win7': ('Win7', 41296702693, 'compile-win7-i686', '.github/workflows/windows-win7-msvc.yml', 'future-classroom-win7-msvc')}
class Waiting(Exception): pass
def need(ok, code):
    if not ok: raise AssertionError(code)
def sha(data): return hashlib.sha256(data).hexdigest()
def read(path):
    path = Path(path)
    if not path.is_file(): raise Waiting('MISSING_' + path.name)
    need(not path.is_symlink(), 'REGULAR_ORIGINAL_REQUIRED')
    try: value = json.loads(path.read_text(encoding='utf-8-sig'))
    except (ValueError, UnicodeError): raise Waiting('INCOMPLETE_' + path.name) from None
    if isinstance(value, dict) and value.get('status') in ('RUNNING', 'STARTED', 'queued', 'in_progress', 'pending', 'waiting', 'requested', 'OBSERVATION_INTERRUPTED_NOT_COMPLETION'):
        raise Waiting('NONTERMINAL_' + path.name)
    return value
def fact(path):
    path = Path(path)
    if not path.is_file(): raise Waiting('MISSING_' + path.name)
    need(not path.is_symlink(), 'REGULAR_ORIGINAL_REQUIRED')
    h = hashlib.sha256(); size = 0
    with path.open('rb') as stream:
        while data := stream.read(1024 * 1024): h.update(data); size += len(data)
    return dict(path=str(path), bytes=size, sha256=h.hexdigest())
def exact(row):
    path = Path(row['path']); path = path if path.is_absolute() else ROOT / path
    actual = fact(path); need((actual['bytes'], actual['sha256']) == (row['bytes'], row['sha256']), 'EXACT_ORIGINAL_IDENTITY'); return actual
def api(path):
    value = read(path); need(value.get('isError') is not True, 'API_ERROR_WRAPPER'); value = value.get('structuredContent', value)
    if isinstance(value, dict) and isinstance(value.get('content'), str):
        try: value = json.loads(value['content'])
        except ValueError: raise Waiting('INCOMPLETE_API_' + Path(path).name) from None
    if value.get('status') in ('queued', 'in_progress', 'pending', 'waiting', 'requested'): raise Waiting('NONTERMINAL_API_' + Path(path).name)
    return value
def operation(path, budget=None, success=True):
    value = read(path)
    need(type(value.get('exitCode')) is int, 'ACTUAL_CONTROLLER_EXIT_REQUIRED')
    if success: need(value.get('status') in ('PASS', 'PASS_COMMAND') and value['exitCode'] == 0, 'ACTUAL_CONTROLLER_SUCCESS_REQUIRED')
    for key in ('actualControllerReturned', 'actualExit'):
        if key in value: need(value[key] is True, 'CONTROLLER_ACTUALLY_RETURNED')
    if budget is not None: need(value['originalBudgetSeconds'] == budget and value['elapsedSeconds'] <= budget, 'ORIGINAL_OPERATION_BUDGET')
    streams = {}
    for key in ('stdout', 'stderr'):
        if isinstance(value.get(key), dict): streams[key] = exact(value[key])
        else:
            stream = Path(path).with_suffix('.' + key + '.log')
            streams[key] = exact(dict(path=str(stream), bytes=value[key + 'Bytes'], sha256=value[key + 'Sha256']))
    return value, dict(record=fact(path), raw=streams)
def source_domains(bound, audit, auditor):
    raw = Path(bound['rawRoot']) if 'rawRoot' in bound else Path(bound['artifactRoot']) / 'raw'
    tree = subprocess.check_output(['git', 'ls-tree', '-r', '-z', '-l', '--full-tree', SOURCE], cwd=REPO)
    oid = subprocess.check_output(['git', 'rev-parse', SOURCE + '^{tree}'], cwd=REPO).decode().strip()
    expected = auditor['parse_git_tree'](tree)
    triples = [[read(raw / (stem + suffix + '.json')) for stem in ('input-sha256', 'input-git-tree', 'input-domains')] for suffix in ('', '-after')]
    counts, hashed = auditor['validate_domains'](SOURCE, oid, expected, *triples)
    declared = {r['path']: {key: r[key] for key in ('path', 'bytes', 'sha256')} for r in audit['sourceMatches']}
    need(hashed == declared and counts['actualShaSubset'] == len(hashed) == audit['counts']['actualShaSubset'], 'ACTUAL_SOURCE_PARTITION_BYTES')
    need(counts['historicalMetadataOnly'] == audit['counts']['historicalMetadataOnly'], 'HISTORICAL_METADATA_SCOPE_DISTINCT')
    return counts
def official(tag, args):
    baseline, run_id, job_name, workflow, prefix = CONFIG[tag]
    run_path = getattr(args, tag + '_run_metadata'); jobs_path = getattr(args, tag + '_job_metadata'); arts_path = getattr(args, tag + '_artifact_metadata')
    run = api(run_path); jobs = api(jobs_path)['jobs']; artifacts = api(arts_path)['artifacts']
    need(run['id'] == run_id and run['head_sha'] == SOURCE and run['status'] == 'completed' and run['event'] == 'workflow_dispatch' and run['path'] == workflow, 'EXACT_OFFICIAL_RUN_SOURCE')
    dispatch = read(ROOT / ('FutureClassroom-round72-' + ('windows-msvc' if tag == 'modern' else 'windows-win7-msvc') + '-dispatch.json'))
    need(dispatch['status'] == 'RUN_IDENTIFIED' and dispatch['sourceSha'] == SOURCE and dispatch['runId'] == run_id and dispatch['workflowPath'] == workflow, 'SINGLE_DISPATCH_BINDING')
    selected = [j for j in jobs if j['run_id'] == run_id and j['name'] == job_name]; need(len(selected) == 1, 'UNIQUE_ORIGINAL_JOB'); job = selected[0]
    if job['status'] != 'completed': raise Waiting('NONTERMINAL_' + tag + '_JOB')
    need(job['conclusion'] == run['conclusion'], 'ORIGINAL_JOB_RUN_RESULT_AGREEMENT')
    if tag == 'modern': need(job['id'] == 124818653405, 'EXACT_MODERN_JOB')
    evidence = dict(run=fact(run_path), jobs=fact(jobs_path), artifacts=fact(arts_path), dispatch=fact(ROOT / ('FutureClassroom-round72-' + ('windows-msvc' if tag == 'modern' else 'windows-win7-msvc') + '-dispatch.json')))
    failed = run['conclusion'] != 'success'
    if failed:
        need(tag == 'win7' and run['conclusion'] == job['conclusion'] == 'failure' and job['id'] == 124818963251, 'EXACT_WIN7_OFFICIAL_FAILURE_ONLY')
        need(not list(ROOT.glob(tag + '-msvc-runtime-*.json')), 'FAILED_CANDIDATE_MUST_NOT_BE_EXECUTED')
    expected = f'{prefix}-i686-{SOURCE}-{run_id}-{run["run_attempt"]}'
    selected = [a for a in artifacts if a['name'] == expected]; need(len(selected) == 1, 'UNIQUE_ORIGINAL_ARTIFACT'); artifact = selected[0]
    need(artifact['expired'] is False and artifact['workflow_run']['id'] == run_id and artifact['workflow_run']['head_sha'] == SOURCE, 'ARTIFACT_SOURCE_IDENTITY')
    if tag == 'modern': need(artifact['id'] == 13080086867 and artifact['size_in_bytes'] == 44077101 and artifact['digest'] == 'sha256:87bd9b85c638fe2dd01540476070ff6750222837b25df4a75a081ee688e0653f', 'ROOT_CONFIRMED_MODERN_ARTIFACT')
    if tag == 'win7': need(artifact['id'] == 13080595231 and artifact['size_in_bytes'] == 44806575 and artifact['digest'] == 'sha256:c5cc184bf36ef951be4f39fe5e91fa45f6f44033bda59fe714bd92495e5e07dd', 'ROOT_CONFIRMED_WIN7_ARTIFACT')
    download = read(ROOT / (tag + '-download-original-first.json'))
    need(download['status'] == 'PASS' and download['sourceSha'] == SOURCE and download['runId'] == run_id and download['artifactId'] == artifact['id'] and download['bytes'] == download['expectedBytes'] == artifact['size_in_bytes'] and download['sha256'] == download['expectedSha256'] == artifact['digest'][7:] and download['overallBudgetSeconds'] == 1200 and download['socketTimeoutSeconds'] == 25, 'TERMINAL_EXACT_DOWNLOAD')
    dl_op, dl_identity = operation(ROOT / (tag + '-download-operation-first.json'), 1200)
    need(dl_op['actualControllerReturned'] is True, 'DOWNLOAD_PARENT_RETURNED_BEFORE_ZIP_ACCESS')
    command = dl_op.get('command', dl_op.get('args'))
    for flag, value in [('--source-sha', SOURCE), ('--baseline', tag), ('--run-id', str(run_id)), ('--artifact-id', str(artifact['id'])), ('--expected-bytes', str(artifact['size_in_bytes'])), ('--expected-sha256', artifact['digest'][7:])]:
        need(command.count(flag) == 1 and command[command.index(flag) + 1] == value, 'EXACT_DOWNLOAD_COMMAND')
    evidence.update(download=fact(ROOT / (tag + '-download-original-first.json')), downloadController=dl_identity)
    # Only now may a parser reopen/hash/read the complete terminal ZIP.
    outcome = dict(status='ACTUAL_CI_FAILURE_RETAINED_NO_RUNTIME_APPROVAL' if failed else 'OFFICIAL_CI_AND_DOWNLOAD_SUCCESS_RUNTIME_NOT_YET_CHECKED', runId=run_id, jobId=job['id'], artifactId=artifact['id'], originals=evidence, officialConclusion=run['conclusion'])
    if failed:
        outcome.update(failedSteps=[dict(name=v['name'], conclusion=v['conclusion']) for v in job['steps'] if v['conclusion'] != 'success'], runtime='NOT_RUN_NOT_APPROVED')
        return outcome, None
    return outcome, (baseline, run_id, job['id'], artifact['id'], workflow)
def failed_win7(outcome, args, modern_audit, auditor):
    receipt = read(args.win7_failed_receipt)
    need(fact(args.win7_failed_receipt)['sha256'] == '93847a26c52799020e15359c611fdf987695c4a1cd489f3dd39565f11921c284' and receipt['status'] == 'PASS_FAILED_CI_RAW_PRESERVATION_ONLY' and receipt['sourceSha'] == SOURCE and receipt['runId'] == CONFIG['win7'][1] and receipt['jobId'] == 124818963251 and receipt['artifactId'] == 13080595231 and receipt['zip']['path'] == 'win7-original.zip' and receipt['zip']['sha256'] == 'c5cc184bf36ef951be4f39fe5e91fa45f6f44033bda59fe714bd92495e5e07dd', 'EXACT_FAILED_WIN7_NONBINARY_RECEIPT')
    raw = args.win7_failed_receipt.with_suffix('')
    zip_path = ROOT / 'win7-original.zip'; zipped = fact(zip_path)
    need(zipped['bytes'] == 44806575 and zipped['sha256'] == receipt['zip']['sha256'], 'WHOLE_TERMINAL_FAILED_ZIP')
    retained = {r['path']: r for r in receipt['preservedFiles']}; need(len(retained) == len(receipt['preservedFiles']), 'UNIQUE_FAILED_NONBINARY_FILES')
    with zipfile.ZipFile(zip_path) as archive:
        infos = archive.infolist(); names = auditor['zip_inventory'](infos)
        ledger = auditor['decode_json'](archive.read('artifact-sha256.json')); by_name = {r['path']: r for r in ledger}
        need(len(by_name) == len(ledger) and set(names) == set(by_name) | {'artifact-sha256.json'}, 'FAILED_ORIGINAL_COMPLETE_PRODUCER_LEDGER')
        executables = {n for n in names if n.lower().endswith('.exe')}
        need(len(retained) == 210 and len(executables) == 22 and set(retained) == set(names) - executables and {r['path'] for r in receipt['excludedCompiledPayloads']} == executables, 'ALL_FAILED_NONBINARY_ORIGINALS_RETAINED')
        for info in infos:
            data = archive.read(info); need(len(data) == info.file_size, 'FAILED_ZIP_MEMBER_LENGTH')
            if info.filename in by_name:
                row = by_name[info.filename]; need(len(data) == row['bytes'] and sha(data) == row['sha256'], 'FAILED_PRODUCER_MEMBER_SHA')
            if info.filename in retained:
                row = retained[info.filename]; need((raw / info.filename).read_bytes() == data and len(data) == row['bytes'] and sha(data) == row['sha256'], 'FAILED_RETAINED_MEMBER_BYTES')
        producers = auditor['decode_json'](archive.read('executables.json'))
        need(len(producers) == 22 and sum(r['role'] == 'tests' for r in producers) == 21 and sum(r['role'] == 'product' for r in producers) == 1 and {r['relative_path'] for r in producers} == executables and all(r['executed'] is False for r in producers), 'FAILED_EXES_COMPILED_ONLY_NEVER_RUN')
        for row in producers:
            declared = next(v for v in receipt['excludedCompiledPayloads'] if v['path'] == row['relative_path'])
            need(row['bytes'] == by_name[row['relative_path']]['bytes'] == declared['bytes'] and row['sha256'] == by_name[row['relative_path']]['sha256'] == declared['sha256'], 'FAILED_EXE_EXACT_PRODUCER_IDENTITY')
    need({p.relative_to(raw).as_posix() for p in raw.rglob('*') if p.is_file()} == set(retained), 'FAILED_RAW_EXACT_MEMBERSHIP')
    result = read(raw / 'result.json')
    need(result['source_sha'] == result['dispatch_source_sha'] == SOURCE and result['baseline'] == 'Win7' and result['target'] == 'i686-win7-windows-msvc' and result['status'] == 'FAILED_OR_INCOMPLETE' and all(result[k] == 'NOT_RUN' for k in ('windows7', 'windows10', 'windows11', 'seewo_device')), 'FAILED_RESULT_SOURCE_TARGET_AND_NOT_RUN')
    commands = {r['name']: r for r in result['commands']}; need(len(commands) == len(result['commands']), 'UNIQUE_FAILED_COMMAND_ROWS')
    failures = []; summary = []
    for name, budget in [('workspace-test-compile', 2400), ('product-compile', 1200), ('clippy', 1500)]:
        if name not in commands: continue
        row = commands[name]
        need(row['parent_exited'] is True and not row['timed_out_or_pipe_stalled'] and not row.get('log_truncated', False) and type(row['exit_code']) is int and row['duration_seconds'] <= budget, 'FAILED_COMMAND_ACTUAL_RETURN_AND_ORIGINAL_BUDGET')
        if row['exit_code'] != 0 or row['passed'] is not True: failures.append(name)
        summary.append(dict(name=name, exitCode=row['exit_code'], passed=row['passed'], elapsedSeconds=row['duration_seconds'], originalBudgetSeconds=budget))
    need(failures == ['clippy'] and commands['workspace-test-compile']['exit_code'] == commands['product-compile']['exit_code'] == 0 and commands['clippy']['exit_code'] == 101, 'ONLY_ORIGINAL_FATAL_CLIPPY_COMMAND')
    diagnostic = (raw / 'logs/clippy.stderr.log').read_text(encoding='utf-8-sig'); plain = re.sub(r'\x1b\[[0-9;]*m', '', diagnostic)
    errors = [line for line in plain.splitlines() if re.match(r'^error(?:\[|:)', line)]
    need(errors == ['error: this `if` guard returns a bool literal and is followed by another', 'error: could not compile `fc-bootstrap` (bin \"fc-bootstrap\" test) due to 1 previous error'] and 'needless_bool' in plain and 'web_editor.rs:59' in plain.replace('\\', '/') and 'items_after_test_module' not in plain, 'EXACT_ORIGINAL_FIRSTPARTY_NEEDLESS_BOOL_FATAL_OLD_ITEM_ORDER_ABSENT')
    cleanup = read(raw / 'cleanup.json')
    need(cleanup['status'] == 'OWNED_BUILD_ROOTS_REMOVED' and all(row['removed'] is True for row in cleanup['roots']), 'FAILED_CI_OWNED_ROOT_CLEANUP')
    counts = source_domains(dict(rawRoot=str(raw)), modern_audit, auditor)
    outcome.update(status='ACTUAL_CI_FAIL_ORIGINALS_VERIFIED_NOT_RUNTIME_APPROVED', zip=zipped, rawReceipt=fact(args.win7_failed_receipt), originalResult=fact(raw / 'result.json'), retainedNonbinaryCount=len(retained), compiledNotExecutedCount=len(producers), commands=summary, failedCommands=failures, firstpartyDiagnostic='clippy::needless_bool in fc-bootstrap application/web_editor.rs:59 cfg(test) synthetic boolean; prior item-order fatal absent', diagnosticOriginal=fact(raw / 'logs/clippy.stderr.log'), sourceDomains=counts, cleanup=fact(raw / 'cleanup.json'), qualification='Exact original CI failure retained: workspace-test/product compiled successfully, strict Clippy failed on the sole firstparty bool guard. No Win7 static PASS or candidate execution/approval, and no aggregate dual-CI PASS.')
    return outcome

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for tag in CONFIG:
        for key, suffix in [('run-metadata', 'run-final-first.json'), ('job-metadata', 'jobs-first.json'), ('artifact-metadata', 'artifacts-first.json'), ('static-operation', 'static-operation-first.json')]:
            parser.add_argument('--' + tag + '-' + key, type=Path, default=ROOT / ((tag + '-run-finalfirst.json') if key == 'run-metadata' else (tag + '-jobs-fourth.json' if tag == 'modern' else tag + '-jobs-seventh.json') if key == 'job-metadata' else tag + '-' + suffix))
    parser.add_argument('--win7-failed-receipt', type=Path, default=ROOT / 'win7-failed-original-raw-first.json')
    parser.add_argument('--readonly-audit', type=Path, default=ROOT / 'windows-readonly-final-second.json')
    parser.add_argument('--readonly-operation', type=Path, required=True)
    parser.add_argument('--readonly-sha256', required=True)
    parser.add_argument('--report', type=Path, default=ROOT / 'independent-joint-first.json')
    args = parser.parse_args(); need(args.report.parent.resolve() == ROOT and not args.report.exists(), 'NEW_EXCLUSIVE_REPORT_REQUIRED')
    report = dict(schemaVersion=1, status='FAIL_R72_INDEPENDENT_EVIDENCE_CHECK', sourceSha=SOURCE, recordedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat())
    try:
        need((ROOT / '.owner').read_bytes() == b'future-classroom-owned-r72-ci-evidence/v1\n', 'EXACT_R72_OWNER')
        # Read final audit/controller first: an unfinished whole round never
        # creates a final report, even if one candidate is already successful.
        final = read(args.readonly_audit); final_op, final_identity = operation(args.readonly_operation)
        final_command = final_op.get('command', final_op.get('args'))
        need(isinstance(final_command, list) and final_command.count('--report') == 1 and Path(final_command[final_command.index('--report') + 1]).resolve() == args.readonly_audit.resolve(), 'ORIGINAL_FINAL_READONLY_CONTROLLER_REPORT_BINDING')
        need(fact(args.readonly_audit)['sha256'] == args.readonly_sha256, 'ROOT_CONFIRMED_FINAL_READONLY')
        need(fact(TEMPLATE)['sha256'] == TEMPLATE_SHA, 'ORIGINAL_R67_PARSER')
        old = runpy.run_path(str(TEMPLATE)); g = old['official_and_artifact'].__globals__
        need(fact(ROOT / 'verify-ci-artifact-first.py')['sha256'] == AUDITOR_SHA and fact(ROOT / 'run-windows-runtime-selected-first.py')['sha256'] == CONTROLLER_SHA, 'PINNED_STATIC_AND_RUNTIME_CONTROLLERS')
        auditor = runpy.run_path(str(ROOT / 'verify-ci-artifact-first.py'))
        probe = subprocess.check_output(['git', 'show', SOURCE + ':scripts/windows-probe.py'], cwd=REPO); probe_sha = sha(probe)
        steps = None
        for node in ast.parse(probe).body:
            if isinstance(node, ast.Assign) and any(isinstance(t, ast.Name) and t.id == 'PUBLIC_DESKTOP_STEPS' for t in node.targets): steps = ast.literal_eval(node.value)
        need(steps is not None and len(steps) == 20, 'EXACT_SOURCE_PUBLIC20')
        outcomes = {}; runtime_ended = []
        mappings = {str(ROOT / (tag + '-run-final-first.json')): getattr(args, tag + '_run_metadata') for tag in CONFIG}
        mappings.update({str(ROOT / (tag + '-artifacts-first.json')): getattr(args, tag + '_artifact_metadata') for tag in CONFIG})
        def mapped_api(path): return api(mappings.get(str(path), path))
        g.update(ROOT=ROOT, SOURCE=SOURCE, api=mapped_api, read=read, exact_file=exact)
        for tag in CONFIG:
            outcome, binding = official(tag, args); outcomes[tag] = outcome
            if binding is None:
                need('modern_audit' in locals(), 'MODERN_ACTUAL_STATIC_REQUIRED_BEFORE_FAILURE_COMPARISON')
                outcomes[tag] = failed_win7(outcome, args, modern_audit, auditor)
                continue
            audit_path = ROOT / (tag + '-original-independent-first.json'); audit = read(audit_path)
            need(audit['helper']['sha256'] == AUDITOR_SHA and len(audit['phases']) == 6 and all(v['status'] == 'PASS' for v in audit['phases']), 'EXACT_COMPLETE_STATIC_AUDIT')
            g['BASELINES'] = {tag: ('first', *binding)}
            bound = old['official_and_artifact'](tag, audit)
            if tag == 'modern': modern_audit = audit
            counts = source_domains(bound, audit, auditor)
            static_value, static_identity = operation(getattr(args, tag + '_static_operation'))
            need(static_value.get('actualExit') is True and static_value['sourceSha'] == SOURCE and static_value['baseline'] == tag and static_value['elapsedSeconds'] >= 0, 'ACTUAL_STATIC_PARENT_EXIT_SOURCE')
            static_identity['outerBudgetQualification'] = 'No outer static budget was declared in the original controller; none is inferred or added. Original CI producer budgets are independently checked.'
            static_identity['elapsedSeconds'] = static_value['elapsedSeconds']
            path = ROOT / (tag + '-msvc-runtime-first.json'); runtime = read(path)
            controller_path = ROOT / (tag + '-msvc-runtime-first.command.json'); controller, controller_identity = operation(controller_path, success=False)
            need(controller.get('actualExit') is True and controller['sourceSha'] == SOURCE, 'ACTUAL_RUNTIME_TERMINAL_SOURCE')
            if runtime.get('status') != 'PASS' or controller.get('status') != 'PASS_COMMAND' or controller['exitCode'] != 0:
                outcome.update(status='ACTUAL_RUNTIME_FAILURE_RETAINED_NO_DUAL_PASS', staticAudit=fact(audit_path), runtime=fact(path), controller=controller_identity, actualRuntimeStatus=runtime.get('status'), actualCleanup=runtime.get('cleanup'), actualExitCode=controller['exitCode'])
                continue
            old['check_platform'](runtime)
            need(all(controller.get(k) is True for k in ('inputsUnchanged', 'scriptUnchanged', 'metadataUnchanged', 'artifactsUnchanged')) and controller['inputNormalSha256'] == runtime['exeSha256'] == bound['normalSha256'] and controller['metadataSha256'] == bound['metadataSha256'], 'ALL_ACTUAL_RUNTIME_IDENTITIES_UNCHANGED')
            for row in controller['inputIdentities'].values(): exact(row)
            old['command'](controller, audit['normalExecutable']['localPath'], probe_sha)
            command = controller['args']
            need(command.count('--public-desktop') == 1 and '--system-host' in command and command[command.index('--test-timeout-seconds') + 1] == '900' and command[command.index('--test-metadata') + 1] == audit['testMetadata']['path'], 'ORIGINAL_FULL900_NATIVE1_SYSTEM_PUBLIC_CONTRACT')
            uploaded = {r['name']: r['sha256'] for r in runtime['uploadedArtifacts']}
            need(len(uploaded) == len(runtime['uploadedArtifacts']) == 22 and uploaded == {n: p['sha256'] for n, p in bound['producers'].items()}, 'ALL22_ACTUAL_UPLOADS')
            need(len(runtime['tests']) == 21 and {r['name'] for r in runtime['tests']} == {n for n, p in bound['producers'].items() if p['role'] == 'tests'}, 'COMPLETE21_ACTUAL_HARNESSES')
            cases = []
            for item in runtime['tests']:
                need(item['binarySha256'] == uploaded[item['name']], 'ACTUAL_TEST_BINARY_IDENTITY')
                parsed = old['harness'](item); parsed['caseNames'] = [g['CASE'].fullmatch(s)[1] for s in item['output'] if g['CASE'].fullmatch(s)]; cases.append(parsed)
            need(runtime['system']['exitCode'] == 0 and runtime['native']['hostExitCode'] == 0, 'ACTUAL_SYSTEM_HOST_EXIT0')
            public = old['flow'](runtime, 'public', steps)
            outcome.update(status='PASS_ACTUAL_TARGET_CI_STATIC_AND_FINITE_WIN11_RUNTIME_ONLY', staticAudit=fact(audit_path), staticController=static_identity, executables=bound['executables'], normalSha256=bound['normalSha256'], metadataSha256=bound['metadataSha256'], sourceDomains=counts,
                runtime=fact(path), controller=controller_identity, actualHost=runtime['platform'], harnesses=cases, totals={k: sum(r[k] for r in cases) for k in ('passed', 'failed', 'ignored', 'measured', 'filteredOut')}, publicDesktop=public, cleanup='PASS')
            runtime_ended.append(datetime.datetime.fromisoformat(controller['startedUtc']) + datetime.timedelta(seconds=controller['elapsedSeconds']))
        need(final['status'] == 'PASS' and final.get('completedAtUtc') and final['ssh']['exitCode'] == 0 and final['authentication'] == final['integrityStatus'] == 'PASS' and final['publicAuthorizationUnchanged'] is True and final['projectRunUnchanged'] is True and final['ownedProcesses'] == final['ownedTasks'] == final['scratchDirectories'] == [] and final['sshd'] == 'Running' and final['persistentKeyMode'] == '0o600' and final['persistentKeyMetadata']['contentsRead'] is False, 'ACTUAL_FINAL_CLEANUP_AND_PERSISTENT_SSH')
        need(fact(REPO / final['baseline']['path'])['sha256'] == final['baseline']['sha256'], 'EXACT_PRECEDING_READONLY_BASELINE')
        for value in final['ssh'].values():
            if isinstance(value, dict) and 'base64' in value:
                raw = base64.b64decode(value['base64'], validate=True); need(len(raw) == value['bytes'] and sha(raw) == value['sha256'], 'ACTUAL_READONLY_RAW_STREAM')
        observed = datetime.datetime.fromisoformat(final['observedAtUtc']); need(all(observed >= end for end in runtime_ended), 'READONLY_AFTER_BOTH_ACTUAL_RUNTIME_CONTROLLERS')
        history = []
        for path in sorted(ROOT.rglob('*.json')):
            if path == args.report: continue
            # Historical RUNNING/FAIL observations are identities to retain,
            # never substituted for required terminal success inputs above.
            history.append(fact(path))
        passed = all(v['status'] == 'PASS_ACTUAL_TARGET_CI_STATIC_AND_FINITE_WIN11_RUNTIME_ONLY' for v in outcomes.values())
        qualified = outcomes['modern']['status'] == 'PASS_ACTUAL_TARGET_CI_STATIC_AND_FINITE_WIN11_RUNTIME_ONLY' and outcomes['win7']['status'] == 'ACTUAL_CI_FAIL_ORIGINALS_VERIFIED_NOT_RUNTIME_APPROVED'
        report.update(status=SUCCESS if passed else QUALIFIED if qualified else 'FAIL_R72_ACTUAL_TERMINAL_RESULT_RETAINED_NO_DUAL_PASS', modern=outcomes['modern'], win7=outcomes['win7'], windowsFinalAudit=fact(args.readonly_audit), readonlyController=final_identity,
            parserTemplate=fact(TEMPLATE), auditor=fact(ROOT / 'verify-ci-artifact-first.py'), helper=fact(Path(__file__)), historicalJsonIdentities=history,
            scope='Source00120: dual-success status requires both modern and Win7-target official CI/static plus each actual finite full21/public20 on Win11 build26300/WOW64. Qualified failure-preserved status instead requires modern actual success and exact Win7 official CI failure/nonbinary originals/NOT_RUN. Neither status proves Win7/Win10 OS, R70 GNU ordinary31 rerun, visual/Edge/header, MSI/installer/campus or final product certification.',
            captureQualification='Per-harness/native lines are original normalized merged output, separately bound to controller raw stdout/stderr. Counts derive from actual case lines and unique running/summary markers; no fixed1542 total. Static original RUNTIME_NOT_RUN and all first observations/FAILs remain unchanged.')
    except Waiting as error:
        print(json.dumps(dict(status='WAIT', reasonCode=str(error), reportCreated=False))); return 3
    except Exception as error:
        report['error'] = dict(kind=type(error).__name__, code=str(error) if isinstance(error, AssertionError) else 'INPUT_OR_ORIGINAL_PARSER_FAILURE')
    data = (json.dumps(report, ensure_ascii=False, indent=2) + '\n').encode()
    with args.report.open('xb') as stream: stream.write(data)
    print(json.dumps(dict(status=report['status'], report=str(args.report), sha256=sha(data))))
    return 0 if report['status'] in (SUCCESS, QUALIFIED) else 1
if __name__ == '__main__': raise SystemExit(main())
