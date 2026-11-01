#!/usr/bin/env python3
"""Read-only R67 original CI/runtime audit. Never runs product, SSH or Cargo.

Missing or still-running originals produce WAIT on stdout and no report. Invoke
only after the original Modern and Win7-target controllers finish. Both runtime
sets are Windows 11 WOW64 evidence, never Windows 7/10 or R68 evidence.
"""
from pathlib import Path, PurePosixPath
import argparse
import ast
import collections
import datetime
import hashlib
import json
import re
import struct
import subprocess
import sys
import zipfile

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parent
REPO = Path('/workspace/共享/Eliora/未来教室计划')
SOURCE = '54397fdd358a42bf8896e86b048c6b43a4e12028'
BASELINES = {
    'modern': ('first', 'Modern', 41283223610, 124779223219, 13074073386,
               '.github/workflows/windows-msvc.yml'),
    'win7': ('second', 'Win7', 41283223767, 124779223557, 13074588673,
             '.github/workflows/windows-win7-msvc.yml'),
}
RUNNING = re.compile(r'running ([0-9]+) tests?')
CASE = re.compile(r'test (.+) \.\.\. (ok|FAILED|ignored)(?:, .*)?')
SUMMARY = re.compile(r'test result: (ok|FAILED)\. ([0-9]+) passed; ([0-9]+) failed; ([0-9]+) ignored; ([0-9]+) measured; ([0-9]+) filtered out; finished in ([0-9]+(?:\.[0-9]+)?)s')


class Waiting(Exception):
    pass


def need(condition, message):
    if not condition:
        raise AssertionError(message)


def digest(data):
    return hashlib.sha256(data).hexdigest()


def identity(path):
    path = Path(path)
    if not path.is_file():
        raise Waiting('missing original: ' + str(path))
    need(not path.is_symlink(), 'symlink original: ' + str(path))
    h = hashlib.sha256()
    count = 0
    with path.open('rb') as source:
        while chunk := source.read(1024 * 1024):
            count += len(chunk)
            h.update(chunk)
    return {'path': str(path), 'bytes': count, 'sha256': h.hexdigest()}


def exact_file(expected):
    got = identity(expected['path'])
    need(got['bytes'] == expected['bytes'] and got['sha256'] == expected['sha256'],
         'original identity mismatch: ' + got['path'])
    return got


def read(path):
    if not Path(path).is_file():
        raise Waiting('missing original: ' + str(path))
    return json.loads(Path(path).read_text(encoding='utf-8-sig'))


def api(path):
    wrapper = read(path)
    need(wrapper.get('isError') is not True, 'API wrapper isError')
    result = wrapper.get('structuredContent', wrapper)
    if isinstance(result, dict) and isinstance(result.get('content'), str):
        result = json.loads(result['content'])
    return result


def basename(value):
    return value.replace('\\', '/').rsplit('/', 1)[-1]


def pe(data):
    need(len(data) >= 64 and data[:2] == b'MZ', 'bounded DOS header')
    offset = struct.unpack_from('<I', data, 60)[0]
    need(offset + 24 + 70 <= len(data) and data[offset:offset+4] == b'PE\0\0', 'bounded PE header')
    machine = struct.unpack_from('<H', data, offset+4)[0]
    optional = offset + 24
    magic = struct.unpack_from('<H', data, optional)[0]
    version = list(struct.unpack_from('<HH', data, optional+48))
    subsystem = struct.unpack_from('<H', data, optional+68)[0]
    return {'machine': f'0x{machine:04x}', 'optionalMagic': f'0x{magic:04x}',
            'subsystem': subsystem, 'subsystemVersion': version}


def harness(item):
    need(type(item.get('exitCode')) is int and item['exitCode'] == 0, 'actual test binary exit0: '+item['name'])
    need(item.get('timeoutSeconds') == 900, 'original per-harness900s budget')
    lines = item.get('output')
    need(isinstance(lines, list) and all(isinstance(s, str) for s in lines), 'normalized per-harness output')
    runs = [(i, RUNNING.fullmatch(line)) for i, line in enumerate(lines) if RUNNING.fullmatch(line)]
    summaries = [(i, SUMMARY.fullmatch(line)) for i, line in enumerate(lines) if SUMMARY.fullmatch(line)]
    need(len(runs) == len(summaries) == 1, 'unique running/summary: '+item['name'])
    begin, declared = runs[0][0], int(runs[0][1][1])
    end, summary = summaries[0]
    need(begin < end and summary[1] == 'ok', 'successful harness bounded by original markers')
    names = set()
    counts = collections.Counter()
    for i, line in enumerate(lines):
        if line.startswith('test ') and not line.startswith('test result:'):
            case = CASE.fullmatch(line)
            need(case is not None and begin < i < end, 'malformed/outside test case: '+item['name'])
            need(case[1] not in names, 'duplicate case name within harness: '+case[1])
            names.add(case[1])
            counts[case[2]] += 1
    values = list(map(int, summary.groups()[1:6]))
    need(values == [counts['ok'], counts['FAILED'], counts['ignored'], 0, 0],
         'per-case ok/FAILED/ignored differs from unique summary: '+item['name'])
    need(declared == len(names) == sum(values[:3]), 'running count differs from actual case lines')
    need(values[1:] == [0, 0, 0, 0], 'failed/ignored/measured/filtered case cannot pass full run')
    return {'name': item['name'], 'binarySha256': item['binarySha256'], 'declared': declared,
            'actualCaseLines': len(names), 'passed': counts['ok'], 'failed': counts['FAILED'],
            'ignored': counts['ignored'], 'measured': 0, 'filteredOut': 0,
            'summarySeconds': float(summary[7]), 'capture': 'normalized merged per-harness output lines; not raw streams'}


def check_platform(report):
    p = report.get('platform', {})
    need('Windows 11' in p.get('caption', '') and str(p.get('build')) == '26300'
         and p.get('version') == '10.0.26300' and p.get('osArchitecture') == '64-bit',
         'actual Windows11 build26300 x64 host')
    need(report.get('win7') == 'NOT_VALIDATED', 'no Win7-OS promotion')
    need(report.get('status') == report.get('cleanup') == 'PASS', 'actual runtime and exact fixture cleanup')
    need(report.get('selfCheck', {}).get('exitCode') == 0, 'actual normal program self-check')
    need(report.get('native', {}).get('exitCode') == 0, 'actual hidden lifecycle exit')
    need(any(re.fullmatch(r'native_lifecycle=PASS elapsed_ms=[0-9]+ rendered_frames=[0-9]+ visible_samples=0 dib_bytes=[0-9]+ program_data_resolved=true offscreen_render_verified=true', s)
             for s in report['native'].get('output', [])), 'actual hidden/offscreen lifecycle marker')


def flow(report, kind, expected_steps):
    check_platform(report)
    public = kind == 'public'
    stem = 'PUBLIC_DESKTOP' if public else 'SUBJECT_VISUAL'
    key = 'publicDesktop' if public else 'subjectVisual'
    native = report['native']
    need(native.get(key+'ExitCode') == 0 and report.get(key+'NativeStatus') == 'PASS', 'actual native '+kind+' exit')
    lines = native.get(key+'Output', [])
    got = [s for s in lines if s.startswith(stem+'_NATIVE_STEP')]
    wanted = [f'{stem}_NATIVE_STEP index={i} name={name}' for i, name in enumerate(expected_steps)]
    need(got == wanted and report.get(key+'NativeStepNames') == list(expected_steps), 'exact ordered native '+kind+' steps')
    need(report.get(key+'NativeSteps') == len(wanted), 'native step count binding')
    footers = [(i, s) for i, s in enumerate(lines) if s.startswith(stem+'_NATIVE_PASS')]
    suffix = (r' steps=20 shared_workers=2 public_session=NONE database_reopen=NOT_EXERCISED change_restore=PASS old_scene=REJECTED foreground=UNTOUCHED isolated_cleanup=PASS elapsed_ms=([0-9]+)'
              if public else r' steps=22 shared_workers=2 real_sqlite_reopen=PASS joint_save=PASS stale_rejected=PASS synthetic_d2d=PASS colour_dialog=NOT_OPENED foreground=UNTOUCHED isolated_cleanup=PASS elapsed_ms=([0-9]+)')
    need(len(footers) == 1, 'unique '+kind+' footer')
    index, line = footers[0]
    match = re.fullmatch(stem+'_NATIVE_PASS'+suffix, line)
    need(match is not None and index > lines.index(wanted[-1]), 'native footer after complete ordered steps')
    elapsed = int(match[1])
    need(0 < elapsed <= 120000 and report.get(key+'ElapsedMs') == elapsed, 'native original120s budget')
    return {'steps': len(wanted), 'elapsedMs': elapsed, 'foreground': 'UNTOUCHED',
            'capture': 'normalized merged native output lines; controller stdout/stderr separately byte-verified'}


def official_and_artifact(tag, audit):
    suffix, baseline, run_id, job_id, artifact_id, workflow = BASELINES[tag]
    need(audit.get('status') == 'PASS_STATIC_ONLY_RUNTIME_NOT_RUN' and audit.get('sourceSha') == SOURCE
         and audit.get('baseline') == baseline, 'exact original static audit source/baseline')
    need(all(p['status'] == 'PASS' for p in audit['phases']), 'all original independent static phases')
    for item in audit['inputs']:
        exact_file(item)
    run = api(ROOT/(tag+'-run-final-first.json'))
    jobs = api(Path(audit['inputs'][1]['path']))['jobs']
    artifacts = api(ROOT/(tag+'-artifacts-first.json'))['artifacts']
    need(run['id'] == run_id and run['head_sha'] == SOURCE and run['status'] == 'completed'
         and run['conclusion'] == 'success' and run['path'] == workflow, 'official original run/source/workflow/completion')
    selected = [j for j in jobs if j['id'] == job_id]
    need(len(selected) == 1, 'exact original job identity')
    job = selected[0]
    need(job['run_id'] == run_id and job['status'] == 'completed' and job['conclusion'] == 'success'
         and all(s['status'] == 'completed' and s['conclusion'] == 'success' for s in job['steps']), 'official all original job steps terminal success')
    selected = [a for a in artifacts if a['id'] == artifact_id]
    need(len(selected) == 1, 'exact official artifact identity')
    artifact = selected[0]
    need(not artifact['expired'] and artifact['workflow_run']['id'] == run_id
         and artifact['workflow_run']['head_sha'] == SOURCE, 'official exact artifact source/run')
    zip_identity = exact_file(audit['zip'])
    need(artifact['size_in_bytes'] == zip_identity['bytes'] and artifact['digest'] == 'sha256:'+zip_identity['sha256'], 'official exact whole ZIP bytes/SHA')
    download = read(ROOT/(tag+'-download-original-first.json'))
    need(download['status'] == 'PASS' and download['runId'] == run_id and download['artifactId'] == artifact_id
         and download['bytes'] == zip_identity['bytes'] and download['sha256'] == zip_identity['sha256'], 'actual original download terminal exact digest')
    artifact_root = Path(audit['testMetadata']['path']).parent
    with zipfile.ZipFile(zip_identity['path']) as archive:
        infos = [i for i in archive.infolist() if not i.is_dir()]
        names = [i.filename for i in infos]
        need(len(names) == len(set(names)), 'ZIP duplicate membership')
        members = {m['path']: m for m in audit['members']}
        need(set(names) == set(members), 'full ZIP membership exact prior static evidence')
        for info in infos:
            rel = PurePosixPath(info.filename)
            need(not rel.is_absolute() and '..' not in rel.parts, 'bounded ZIP member path')
            h = hashlib.sha256()
            length = 0
            with archive.open(info) as stream:
                while chunk := stream.read(1024*1024):
                    length += len(chunk)
                    h.update(chunk)
            original = members[info.filename]
            need(length == original['bytes'] and h.hexdigest() == original['sha256']
                 and f'{info.CRC:08x}' == original['crc32'], 'full ZIP original CRC/bytes/SHA member: '+info.filename)
            local = artifact_root/('programs' if info.filename.endswith('.exe') else 'raw')/info.filename
            exact_file({'path':str(local),'bytes':length,'sha256':h.hexdigest()})
    raw = artifact_root/'raw'
    producers = read(raw/'executables.json')
    ledger = read(raw/'artifact-sha256.json')
    need({x['path'] for x in ledger} == set(members)-{'artifact-sha256.json'}, 'exact complete original producer ledger membership')
    for row in ledger:
        member = members[row['path']]
        need((row['bytes'],row['sha256']) == (member['bytes'],member['sha256']), 'producer ledger exact ZIP member')
    result = read(raw/'result.json')
    need(result['source_sha'] == result['dispatch_source_sha'] == SOURCE and result['baseline'] == baseline
         and result['status'] == 'COMPILE_CHECKS_PASSED_RUNTIME_NOT_RUN', 'original CI source/result scope')
    need(result['later_test_arguments'] == ['--test-threads=2'], 'original later-test threads2')
    commands = {row['name']:row for row in result['commands']}
    for name, budget in [('workspace-test-compile',2400),('product-compile',1200),('clippy',1500)]:
        row = commands[name]
        need(row['passed'] and row['parent_exited'] and row['exit_code'] == 0
             and not row['timed_out_or_pipe_stalled'] and not row.get('log_truncated',False)
             and row['duration_seconds'] <= budget, 'actual original CI command '+name)
    original_tests = []
    for line in (raw/'logs/workspace-test-compile.stdout.log').read_text(encoding='utf-8-sig').splitlines():
        try:
            row = json.loads(line)
        except json.JSONDecodeError:
            continue
        if row.get('reason') == 'compiler-artifact' and row.get('profile',{}).get('test') is True and row.get('executable'):
            original_tests.append(row)
    metadata_identity = exact_file(audit['testMetadata'])
    metadata = [json.loads(s) for s in Path(metadata_identity['path']).read_text().splitlines() if s.strip()]
    tests = [p for p in producers if p['role'] == 'tests']
    product = [p for p in producers if p['role'] == 'product']
    need(len(producers) == len(audit['executables']) == 22 and len(tests) == len(original_tests) == len(metadata) == 21
         and len(product) == 1, 'actual complete original22 producer/21 Cargo test association')
    by_name = {basename(p['relative_path']):p for p in producers}
    need(len(by_name) == 22, 'unique producer executable basenames')
    original_by_name = {basename(p['executable']):p for p in original_tests}
    metadata_by_name = {basename(p['executable']):p for p in metadata}
    need(set(original_by_name) == set(metadata_by_name) == {basename(p['relative_path']) for p in tests}, 'Cargo/metadata/test producer exact set')
    actual_pes = []
    for item in audit['executables']:
        producer = item['producer']
        name = basename(producer['relative_path'])
        need(producer == by_name[name], 'prior static exact producer row')
        actual = exact_file({'path':item['localPath'],'bytes':producer['bytes'],'sha256':producer['sha256']})
        header = pe(Path(actual['path']).read_bytes())
        need(header == item['peHeader'] and header['machine'] == '0x014c' and header['optionalMagic'] == '0x010b'
             and header['subsystem'] == (2 if producer['role']=='product' else 3), 'actual original22 PE32 x86 GUI/test-CUI headers')
        if tag == 'win7':
            need(header['subsystemVersion'] == [6,1], 'Win7-target actual PE subsystem version6.1')
        exact_file(item['rawInspection'])
        if producer['role'] == 'tests':
            original, mapped = original_by_name[name], metadata_by_name[name]
            need(original['target'] == mapped['target'] and original['profile'] == mapped['profile']
                 and mapped['reason'] == 'compiler-artifact' and mapped['profile']['test'] is True
                 and original['package_id'] == mapped['package_id'] and original['features'] == mapped['features']
                 and mapped['target']['name'] == producer['target'] and mapped['target']['kind'] == producer['kinds']
                 and mapped['executable'] == item['localPath'], 'original Cargo profile/target/package/feature remapped metadata association')
        actual_pes.append({'name':name,'bytes':actual['bytes'],'sha256':actual['sha256'],'role':producer['role'],'pe':header})
    normal = audit['normalExecutable']
    need(normal['producer'] == product[0] and normal['localPath'] == str(artifact_root/'programs'/product[0]['relative_path']), 'exact normal product association')
    cleanup = read(raw/'cleanup.json')
    need(cleanup == audit['originalCleanup'] and cleanup['status'] == 'OWNED_BUILD_ROOTS_REMOVED'
         and all(row['removed'] is True for row in cleanup['roots']), 'actual original owned CI roots cleanup')
    return {'normalSha256':product[0]['sha256'],'metadataSha256':metadata_identity['sha256'],
            'producers':by_name,'executables':actual_pes,'runId':run_id,'jobId':job_id,
            'artifactId':artifact_id,'zip':zip_identity,'artifactRoot':str(artifact_root)}


def command(record, expected_exe, source_hash):
    need(record.get('actualExit') is True and record.get('exitCode') == 0
         and record.get('resultStatus') == record.get('cleanup') == 'PASS', 'actual original controller successful exit/cleanup')
    need(record.get('scriptSha256',source_hash) == source_hash, 'source-bound original controller script')
    args = record['args']
    need(args[0:2] == ['python3','-B'] and args[args.index('--exe')+1] == expected_exe
         and args[args.index('--native-seconds')+1] == '1', 'actual normal executable/controller args')
    return {'stdout':exact_file(record['stdout']),'stderr':exact_file(record['stderr'])}


def preflight():
    required = []
    for tag,(suffix,*_) in BASELINES.items():
        required += [ROOT/(tag+'-original-independent-'+suffix+'.json'), ROOT/(tag+'-msvc-runtime-first.json'),
            ROOT/(tag+'-msvc-runtime-first.command.json'),ROOT/(tag+'-public-visual-operation-first.json'),
            ROOT/(tag+'-msvc-public-native-first.json'),ROOT/(tag+'-msvc-visual-native-first.json')]
    values = {}
    for path in required:
        try:
            value = read(path)
        except json.JSONDecodeError:
            raise Waiting('original not yet complete JSON: '+str(path)) from None
        if value.get('status') in ('RUNNING','STARTED'):
            raise Waiting('original controller still RUNNING: '+str(path))
        values[path.name] = value
    return values, required


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--report',type=Path,default=ROOT/'independent-runtime-first.json')
    args = parser.parse_args()
    need(args.report.parent.resolve() == ROOT and not args.report.exists(), 'fresh exclusive report in owned R67 root required')
    report = {'schema':'future-classroom.r67-independent-runtime/v1','status':'FAIL','recordedUtc':datetime.datetime.now(datetime.timezone.utc).isoformat(),
        'sourceSha':SOURCE,'scope':'Read-only audit of original R67 MSVC ZIPs,22 PE bytes per build,original controllers and normalized actual Windows11 test/native transcripts. No Windows,SSH,Cargo,network or source mutation.',
        'limitations':['Both executable sets ran on Windows11 build26300 x64 via WOW64; no Windows7/10 OS or Seewo runtime proof.',
            'Per-harness/native output is the original runner normalized merged lines, not separate raw stdout/stderr byte streams. Controller stdout/stderr original files are independently byte verified.',
            'Source54397 is R67. These results do not validate new R68 production HTTPS implementation.'], 'baselines':{}}
    try:
        values, required = preflight()
        # Read immutable source through Git only, never import or execute it.
        git = subprocess.run(['git','show',SOURCE+':scripts/windows-probe.py'],cwd=REPO,capture_output=True,check=True)
        probe_source = git.stdout
        source_hash = digest(probe_source)
        tree = ast.parse(probe_source)
        native_steps = {}
        for node in tree.body:
            if isinstance(node,ast.Assign):
                for target in node.targets:
                    if isinstance(target,ast.Name) and target.id in ('PUBLIC_DESKTOP_STEPS','SUBJECT_VISUAL_STEPS'):
                        native_steps[target.id] = ast.literal_eval(node.value)
        need(len(native_steps['PUBLIC_DESKTOP_STEPS']) == 20 and len(native_steps['SUBJECT_VISUAL_STEPS']) == 22, 'source-pinned exact native contracts')
        for tag,(suffix,*_) in BASELINES.items():
            audit = values[tag+'-original-independent-'+suffix+'.json']
            bound = official_and_artifact(tag,audit)
            runtime = values[tag+'-msvc-runtime-first.json']
            controller = values[tag+'-msvc-runtime-first.command.json']
            check_platform(runtime)
            normal = audit['normalExecutable']['localPath']
            need(controller['sourceSha'] == SOURCE and controller['scriptUnchanged'] is True
                 and controller['inputNormalSha256'] == runtime['exeSha256'] == bound['normalSha256']
                 and controller['metadataSha256'] == bound['metadataSha256'], 'runtime/controller/source/normal/metadata SHA binding')
            raw = command(controller,normal,source_hash)
            need(controller['args'][controller['args'].index('--test-timeout-seconds')+1] == '900'
                 and controller['args'][controller['args'].index('--test-metadata')+1] == audit['testMetadata']['path'], 'exact original runtime metadata and900s')
            uploaded = {x['name']:x['sha256'] for x in runtime['uploadedArtifacts']}
            need(len(uploaded) == len(runtime['uploadedArtifacts']) == 22
                 and uploaded == {n:p['sha256'] for n,p in bound['producers'].items()}, 'all22 exact actual uploaded SHA matches producer actual bytes')
            tests = runtime['tests']
            need(len(tests) == 21 and {t['name'] for t in tests} == {n for n,p in bound['producers'].items() if p['role']=='tests'}, 'actual21 harness complete set')
            counts = []
            for item in tests:
                need(item['binarySha256'] == uploaded[item['name']], 'actual harness uploaded binary exact SHA')
                counts.append(harness(item))
            need(runtime.get('system',{}).get('exitCode') == 0 and runtime['native'].get('hostExitCode') == 0, 'actual readonly system/hidden host exits')
            operations = values[tag+'-public-visual-operation-first.json']
            need(operations['status'] == 'PASS_SELECTED2_ORIGINAL_MSVC_PUBLIC_VISUAL_FLOWS'
                 and operations['scriptSha256'] == source_hash and len(operations['operations']) == 2, 'terminal actual separate public/visual controllers')
            flows = {}
            for kind, step_key in [('public','PUBLIC_DESKTOP_STEPS'),('visual','SUBJECT_VISUAL_STEPS')]:
                label = tag+'-msvc-'+kind+'-native-first'
                rows = [o for o in operations['operations'] if o['label'] == label]
                need(len(rows) == 1 and rows[0]['status'] == 'PASS' and rows[0]['exeSha256'] == bound['normalSha256'], 'exact original '+kind+' operation')
                operation = rows[0]
                data = values[label+'.json']
                need(data['exeSha256'] == bound['normalSha256'] and data['tests'] == []
                     and data['uploadedArtifacts'] == [{'name':basename(normal),'sha256':bound['normalSha256']}], 'native original sole normal upload')
                streams = command(operation,normal,source_hash)
                flows[kind] = dict(flow(data,kind,native_steps[step_key]),controllerRaw=streams)
            report['baselines'][tag] = {'status':'PASS','runId':bound['runId'],'jobId':bound['jobId'],'artifactId':bound['artifactId'],
                'staticAudit':identity(ROOT/(tag+'-original-independent-'+suffix+'.json')),
                'zip':bound['zip'],'executables':bound['executables'],'normalSha256':bound['normalSha256'],
                'metadataSha256':bound['metadataSha256'],'actualHost':runtime['platform'],'runtimeArchitecture':'x86 PE32 on x64 Windows11 (WOW64)',
                'harnesses':counts,'harnessCount':len(counts),'totals':{k:sum(row[k] for row in counts) for k in ('passed','failed','ignored','measured','filteredOut')},
                'controllerRaw':raw,'nativeFlows':flows,'cleanup':'PASS'}
        qualification = read(ROOT/'win7-premature-audit-qualification-first.json')
        exact_file(qualification['originalAudit'])
        need(qualification['originalReportPreserved'] is True and qualification['originalAudit']['status'] == 'FAIL'
             and qualification['observedOriginalDownloadStatus'] == 'RUNNING'
             and read(ROOT/'win7-original-independent-first.json')['status'] == 'FAIL', 'original premature FAIL and qualification preserved')
        report['historicalPrematureAudit']={'qualification':identity(ROOT/'win7-premature-audit-qualification-first.json'),
            'originalFail':qualification['originalAudit'],'scope':'Premature static audit of then-incomplete download; not product failure. Later second audit requires actual original terminal download PASS.'}
        report['runtimeInputs']=[identity(p) for p in required]
        report['sourceProbeSha256']=source_hash
        report['helper']=identity(Path(__file__))
        report['status']='PASS_R67_TWO_MSVC_BUILDS_ACTUAL_WIN11_ONLY'
    except Waiting as error:
        print(json.dumps({'status':'WAIT','reason':str(error),'reportCreated':False}))
        return 3
    except Exception as error:
        report['error']={'kind':type(error).__name__,'message':str(error)}
    with args.report.open('x',encoding='utf-8') as output:
        json.dump(report,output,ensure_ascii=False,indent=2)
        output.write('\n')
    print(json.dumps({'status':report['status'],'report':str(args.report),'sha256':digest(args.report.read_bytes())}))
    return 0 if report['status'].startswith('PASS_') else 1


if __name__ == '__main__':
    raise SystemExit(main())
