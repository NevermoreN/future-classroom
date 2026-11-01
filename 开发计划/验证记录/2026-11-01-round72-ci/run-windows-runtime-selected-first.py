"""Prepared R72 serial full-runtime controller; --execute is explicit.
Local terminal-static inputs are gated before opening any output or invoking
Windows. Missing/in-progress inputs return WAIT without a runtime report.
"""
from pathlib import Path
import argparse, ast, datetime, hashlib, json, runpy, subprocess, sys, time
sys.dont_write_bytecode = True
EV = Path(__file__).resolve().parent
REPO = Path('/workspace/共享/Eliora/未来教室计划')
GATE = EV / 'prepare-ci-inputs-first.py'
GATE_SHA = '15ef935d61aa74a306abcc55598a66c91edf91fef2530955d470ca6b107c5ca0'
PROBE = REPO / 'scripts/windows-probe.py'
PARSER = REPO / '开发计划/验证记录/2026-11-01-round67-ci/independent-runtime-first.py'
PARSER_SHA = '0317ffb1719737bd9dfa45ca7952c242df6ffff0baa6b1611d7b912277f9e0e5'
SCOPE = ('R72 source-bound CI MSVC full21/public20 finite execution on authorized '
         'Win11 build26300/WOW64 only. Win7-target on Win11 is not Win7/10 OS '
         'evidence. GNU ordinary31/material-stop, recovery and reinitialization '
         'are separate R70 archived evidence, not rerun or promoted here. No '
         'independent visual/Edge/header/ordinary31, campus or MSI acceptance.')
def need(ok, code):
    if not ok: raise AssertionError(code)
def sha(data): return hashlib.sha256(data).hexdigest()
def fact(path):
    path = Path(path)
    need(path.is_file() and not path.is_symlink(), 'NONREGULAR_LOCAL_INPUT')
    h = hashlib.sha256(); size = 0
    with path.open('rb') as stream:
        while data := stream.read(1024 * 1024): h.update(data); size += len(data)
    return dict(path=str(path), bytes=size, sha256=h.hexdigest())
def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--baseline', choices=['modern', 'win7'], required=True)
    parser.add_argument('--source-sha', required=True)
    parser.add_argument('--execute', action='store_true')
    args = parser.parse_args()
    need((EV / '.owner').read_bytes() == b'future-classroom-owned-r72-ci-evidence/v1\n', 'EXACT_R72_OWNER')
    need(fact(GATE)['sha256'] == GATE_SHA, 'PREPARATION_GATE_CHANGED')
    gate_cmd = ['python3', '-B', str(GATE), '--mode', 'runtime-inputs', '--source-sha', args.source_sha, '--baseline', args.baseline]
    # This is a pure local input gate. It never executes a product or accesses
    # the network; in particular it does not open a partial/in-progress ZIP.
    admitted = subprocess.run(gate_cmd, cwd=REPO, capture_output=True)
    try: plan = json.loads(admitted.stdout)
    except (ValueError, UnicodeError):
        print(json.dumps(dict(status='BLOCKED_INVALID_INPUT', reasonCode='GATE_PROTOCOL_ERROR', reportCreated=False))); return 1
    if admitted.returncode:
        print(json.dumps(dict(status=plan.get('status', 'BLOCKED_INVALID_INPUT'), reasonCode=plan.get('reasonCode', 'GATE_REJECTED'), reportCreated=False)))
        return 3 if plan.get('status') == 'WAIT' else 1
    need(plan['status'] == 'PREPARED_RUNTIME_INPUTS_ONLY_NOT_EXECUTED' and plan['sourceSha'] == args.source_sha and plan['baseline'] == args.baseline, 'EXACT_GATE_ADMISSION')
    audit_path = EV / (args.baseline + '-original-independent-first.json')
    audit = json.loads(audit_path.read_bytes())
    exe = Path(plan['normal']['path']); metadata = Path(plan['testMetadata']['path'])
    need(audit['sourceSha'] == args.source_sha and audit['runId'] == plan['runId'], 'ACTUAL_STATIC_SOURCE_RUN')
    artifacts = {Path(row['localPath']).resolve(): row for row in audit['executables']}
    tests = {path for path, row in artifacts.items() if row['producer']['role'] == 'tests'}
    need(len(artifacts) == 22 and len(tests) == 21 and exe.resolve() in artifacts and artifacts[exe.resolve()]['producer']['role'] == 'product', 'EXACT21_TESTS_AND_NORMAL')
    need(len({p.name for p in artifacts}) == 22, 'UNIQUE_UPLOADED_BASENAMES')
    cargo_tests = []
    for line in metadata.read_text().splitlines():
        value = json.loads(line)
        if value.get('reason') == 'compiler-artifact' and value.get('profile', {}).get('test') is True and value.get('executable'):
            cargo_tests.append(Path(value['executable']).resolve())
    need(len(cargo_tests) == len(set(cargo_tests)) == 21 and set(cargo_tests) == tests, 'ACTUAL_METADATA_EXACT_TEST_SET')
    normal_fact = fact(exe); metadata_fact = fact(metadata)
    need(normal_fact == plan['normal'] and metadata_fact == plan['testMetadata'], 'EXACT_GATE_INPUT_BYTES')
    input_paths = [GATE, PROBE, PARSER, audit_path, metadata] + sorted(artifacts)
    before = {str(path): fact(path) for path in input_paths}
    need(before[str(PARSER)]['sha256'] == PARSER_SHA, 'ORIGINAL_CASE_PARSER')
    for path, row in artifacts.items():
        need(before[str(path)]['bytes'] == row['producer']['bytes'] and before[str(path)]['sha256'] == row['producer']['sha256'], 'EVERY_ACTUAL_ARTIFACT_IDENTITY')
    stem = args.baseline + '-msvc-runtime-first'
    out = EV / (stem + '.json'); record = EV / (stem + '.command.json')
    stdout = EV / (stem + '.stdout.log'); stderr = EV / (stem + '.stderr.log')
    need(not any(p.exists() for p in (out, record, stdout, stderr)), 'FRESH_RUNTIME_OUTPUTS_REQUIRED')
    command = ['python3', '-B', str(PROBE), '--host', 'elior@192.168.10.212',
        '--identity', '/config/.local/share/future-classroom-test-access/win11-192.168.10.212',
        '--exe', str(exe), '--test-metadata', str(metadata), '--test-timeout-seconds', '900',
        '--native-seconds', '1', '--system-host', '--public-desktop', '--report', str(out)]
    if not args.execute:
        print(json.dumps(dict(status='PREPARED_RUNTIME_CONTROLLER_ONLY_NOT_EXECUTED', sourceSha=args.source_sha,
            baseline=args.baseline, runId=plan['runId'], command=command, normal=normal_fact,
            testMetadata=metadata_fact, artifactCount=len(artifacts), reportCreated=False, scope=SCOPE)))
        return 0
    result = dict(status='RUNNING', startedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),
        args=command, gateCommand=gate_cmd, baseline=args.baseline, sourceSha=args.source_sha,
        runId=plan['runId'], staticAudit=fact(audit_path), normal=normal_fact, metadata=metadata_fact,
        scriptSha256=before[str(PROBE)]['sha256'], inputNormalSha256=normal_fact['sha256'], metadataSha256=metadata_fact['sha256'],
        inputIdentities=before, originalPerTestBudgetSeconds=900, nativeSeconds=1, harnessCount=21, scope=SCOPE)
    with record.open('x') as stream: json.dump(result, stream, indent=2); stream.write('\n')
    print('WINDOWS_RUNTIME_STARTED ' + args.baseline, flush=True)
    began = time.monotonic(); code = None
    try:
        with stdout.open('xb') as a, stderr.open('xb') as b:
            child = subprocess.Popen(command, cwd=REPO, stdout=a, stderr=b)
            result['pid'] = child.pid; code = child.wait()
        result.update(exitCode=code, actualExit=True, elapsedSeconds=time.monotonic() - began)
        after = {str(path): fact(path) for path in input_paths}
        result.update(inputsUnchanged=before == after, scriptUnchanged=before[str(PROBE)] == after[str(PROBE)],
                      metadataUnchanged=before[str(metadata)] == after[str(metadata)],
                      artifactsUnchanged=all(before[str(p)] == after[str(p)] for p in artifacts))
        need(code == 0 and before == after, 'ACTUAL_CONTROLLER_EXIT_AND_INPUTS_UNCHANGED')
        report = json.loads(out.read_bytes()); result.update(resultStatus=report.get('status'), cleanup=report.get('cleanup'), actualReport=fact(out))
        old = runpy.run_path(str(PARSER)); old['check_platform'](report)
        need(report['exeSha256'] == normal_fact['sha256'] and report['testTimeoutSeconds'] == 900 and report['system']['exitCode'] == 0 and report['native']['hostExitCode'] == 0, 'ACTUAL_FULL_PLATFORM_NORMAL_SYSTEM_HOST')
        uploaded = {p.name: before[str(p)]['sha256'] for p in artifacts}
        need(len(report['uploadedArtifacts']) == 22 and {r['name']: r['sha256'] for r in report['uploadedArtifacts']} == uploaded, 'ACTUAL_ALL22_UPLOADED_IDENTITIES')
        need(len(report['tests']) == 21 and {r['name'] for r in report['tests']} == {p.name for p in tests}, 'ACTUAL_COMPLETE21_HARNESSES')
        cases = []
        for item in report['tests']:
            need(item['binarySha256'] == uploaded[item['name']], 'ACTUAL_HARNESS_BINARY_IDENTITY')
            cases.append(old['harness'](item))
        expected = None
        for node in ast.parse(PROBE.read_bytes()).body:
            if isinstance(node, ast.Assign) and any(isinstance(t, ast.Name) and t.id == 'PUBLIC_DESKTOP_STEPS' for t in node.targets): expected = ast.literal_eval(node.value)
        need(expected is not None and len(expected) == 20, 'SOURCE_PUBLIC20_STEPS')
        result['publicDesktop'] = old['flow'](report, 'public', expected)
        result['harnesses'] = cases
        result['totals'] = {key: sum(row[key] for row in cases) for key in ('passed', 'failed', 'ignored', 'measured', 'filteredOut')}
        result['status'] = 'PASS_COMMAND'
    except Exception as error:
        result.update(status='FAIL_COMMAND_OR_EVIDENCE', exitCode=code, elapsedSeconds=time.monotonic() - began,
            failure=dict(kind=type(error).__name__, code=str(error) if isinstance(error, AssertionError) else 'LOCAL_INPUT_OR_CONTROLLER_ERROR'))
        if out.is_file():
            try:
                failed = json.loads(out.read_bytes()); result.update(resultStatus=failed.get('status'), cleanup=failed.get('cleanup'), actualReport=fact(out))
            except (OSError, ValueError): pass
    for name, path in [('stdout', stdout), ('stderr', stderr)]:
        if path.is_file(): result[name] = fact(path)
    record.write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(dict(status=result['status'], baseline=args.baseline, exitCode=code,
        cleanup=result.get('cleanup'), record=str(record), recordSha256=fact(record)['sha256'])))
    return 0 if result['status'] == 'PASS_COMMAND' else 1
if __name__ == '__main__': raise SystemExit(main())
