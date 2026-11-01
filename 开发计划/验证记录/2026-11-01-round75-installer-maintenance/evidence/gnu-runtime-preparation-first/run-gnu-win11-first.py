"""Prepared R75 full GNU runtime controller; root alone uses --execute.
Requires explicit successful producer AND parent terminal hashes, N/joint freeze
hashes, eight source guards and 24 original PE identities before any upload.
Git base is not an invented commit identity for this uncommitted candidate.
"""
from pathlib import Path, PurePosixPath
import argparse, ast, datetime, hashlib, json, re, runpy, stat, struct, subprocess, sys, time
sys.dont_write_bytecode=True
ROOT=Path('/tmp/fc-round75-installer-maintenance');W=ROOT/'worktree';E=ROOT/'evidence';D=E/'gnu-runtime-preparation-first'
PARSER=Path('/workspace/共享/Eliora/未来教室计划/开发计划/验证记录/2026-11-01-round67-ci/independent-runtime-first.py')
PARSER_SHA='0317ffb1719737bd9dfa45ca7952c242df6ffff0baa6b1611d7b912277f9e0e5'
PROBE=W/'scripts/windows-probe.py';PROBE_SHA='f176a022bf90565be7074f53a2d502d6519afd1748cdd2022f608a217d485c14'
BROKER='crates/fc-windows/src/installer_lifecycle/native/broker.rs';BROKER_SHA='2812383941139a17169559a902df34724c6cf2deadcd2aaf9961493c31c12691'
BASE='2dfb2e7e07ace64c61abf66c9a423aa815de7110'
SCOPE='R75 actual frozen GNU22 tests/app self/native1/system/public20 on authorized Win11 build26300/WOW64 only. Both normal GUI products pinned; lifecycle helper normal NOT_RUN. No observer armed/report, MSI execution, CI, other OS, ordinary31 or campus acceptance. Git base alone is not candidate source identity.'
class Waiting(Exception):pass
def need(v,c):
    if not v:raise AssertionError(c)
def sha(data):return hashlib.sha256(data).hexdigest()
def scoped(path):
    p=Path(path);need(p.is_absolute() and p.resolve().is_relative_to(ROOT),'EXACT_R75_INPUT_SCOPE')
    need(not any(q.is_symlink() for q in [p,*p.parents] if q==ROOT or ROOT in q.parents),'NO_SYMLINK_INPUT')
    need(p.suffix.lower() not in {'.key','.pfx','.p12'},'PRIVATE_CONTENT_NOT_READ');return p
def fact(path):
    p=scoped(path)
    if not p.is_file():raise Waiting('MISSING_'+p.name)
    need(stat.S_ISREG(p.lstat().st_mode),'REGULAR_INPUT');h=hashlib.sha256();size=0
    with p.open('rb') as f:
        while data:=f.read(1024*1024):h.update(data);size+=len(data)
    return dict(path=str(p),bytes=size,sha256=h.hexdigest())
def read(path):
    p=scoped(path)
    if not p.is_file():raise Waiting('MISSING_'+p.name)
    try:v=json.loads(p.read_text(encoding='utf-8-sig'))
    except (ValueError,UnicodeError):raise Waiting('INCOMPLETE_'+p.name) from None
    if v.get('status') in ('RUNNING','STARTED','queued','in_progress','OBSERVATION_INTERRUPTED_NOT_COMPLETION'):raise Waiting('NONTERMINAL_'+p.name)
    return v
def exact(row):
    p=Path(row['path']);p=p if p.is_absolute() else E/p;f=fact(p);need(f['sha256']==row['sha256'] and ('bytes' not in row or f['bytes']==row['bytes']),'EXACT_REOPENED_INPUT');return f
def source_path(name):
    rel=PurePosixPath(name);need(name==rel.as_posix() and not rel.is_absolute() and '..' not in rel.parts,'SOURCE_RELATIVE_GUARD');return W/name
def pe(row,subsystem):
    f=exact(row)
    with Path(f['path']).open('rb') as s:
        dos=s.read(64);need(len(dos)==64 and dos[:2]==b'MZ','ACTUAL_DOS_PE');offset=struct.unpack_from('<I',dos,60)[0];need(64<=offset<=f['bytes']-96,'PE_OFFSET_BOUNDS');s.seek(offset);head=s.read(96)
    need(head[:4]==b'PE\0\0' and struct.unpack_from('<H',head,4)[0]==row['peMachine']==0x14c and struct.unpack_from('<H',head,24)[0]==row['peOptionalMagic']==0x10b and struct.unpack_from('<H',head,92)[0]==row['peSubsystem']==subsystem,'ACTUAL_I386_PE32_ROLE_SUBSYSTEM')
    return f
def main():
    p=argparse.ArgumentParser(description=__doc__)
    for name in ('producer-report','producer-terminal-report','freeze','joint-freeze'):p.add_argument('--'+name,type=Path,required=True);p.add_argument('--'+name+'-sha256',required=True)
    p.add_argument('--source-git-base',required=True);p.add_argument('--report',type=Path,required=True);p.add_argument('--operation',type=Path,required=True);p.add_argument('--execute',action='store_true');a=p.parse_args()
    try:
        need((ROOT/'.owner').read_bytes()==b'future-classroom-owned-r75-installer-maintenance/v1\n' and (D/'.owner').read_bytes()==b'future-classroom-owned-r75-gnu-runtime-preparation/v1\n','EXACT_OWNERS')
        need(a.source_git_base==BASE,'EXACT_BASE_ONLY_NOT_CANDIDATE_SOURCE_COMMIT')
        for name in ('producer_report','producer_terminal_report','freeze','joint_freeze'):need(re.fullmatch('[0-9a-f]{64}',getattr(a,name+'_sha256')),'EXPLICIT_ACTUAL_ORIGINAL_SHA256')
        # All four required originals terminal and pinned before opening any
        # codegen metadata, artifact or source payload beyond these JSONs.
        records={name:read(getattr(a,name)) for name in ('producer_report','producer_terminal_report','freeze','joint_freeze')}
        inputs={str(getattr(a,name)):fact(getattr(a,name)) for name in records}
        need(all(inputs[str(getattr(a,name))]['sha256']==getattr(a,name+'_sha256') for name in records),'ALL_FOUR_ACTUAL_ORIGINAL_HASHES')
        producer=records['producer_report'];terminal=records['producer_terminal_report'];freeze=records['freeze'];joint=records['joint_freeze']
        need(terminal['status']=='PASS_CONTROLLER_ACTUAL_EXIT0_REAPED' and terminal['actualToolTerminal']['exitCode']==0 and terminal['actualToolTerminal']['runningSessionReturned'] is False,'ACTUAL_PRODUCER_PARENT_TERMINAL_EXIT0')
        need(exact(terminal['report'])==fact(a.producer_report) and terminal['sourceFreeze']['sha256']==a.freeze_sha256 and exact(terminal['sourceFreeze'])==fact(a.freeze),'TERMINAL_BOUND_TO_EXACT_PRODUCER_AND_FREEZE')
        need(producer['status']=='PASS_CODEGEN_NOT_RUNTIME' and producer['originalPerCommandBudgetSeconds']==900 and producer['sourceUnchanged'] is True and producer['sourceBefore']==producer['sourceAfter'] and producer['freezeSha256']==a.freeze_sha256,'ORIGINAL_CODEGEN_SUCCESS_SAME_BUDGET_AND_FREEZE')
        source_rows={r['path']:r for r in producer['sourceBefore']};need(len(source_rows)==len(producer['sourceBefore'])==8 and 'Cargo.lock' in source_rows,'EXACT_EIGHT_PRODUCER_GUARDS')
        need(joint['status']=='PASS_FROZEN_SOURCE_NOT_RUNTIME' and joint['gitBaseOnly']==BASE and joint['sourceGuardCount']==8 and joint['changedSourceCount']==7,'FINITE_JOINT_EIGHT_GUARDS_BASE_ONLY')
        joint_rows={r['path']:r for r in joint['sources']};need(source_rows==joint_rows,'EXACT_PRODUCER_JOINT_GUARD_SET')
        need(any(r['path']==Path(a.freeze).name and r['sha256']==a.freeze_sha256 for r in joint['freezeReports']),'JOINT_BINDS_EXACT_N_FREEZE')
        for row in joint['freezeReports']:
            f=exact(row);inputs[f['path']]=f
        for row in freeze['source']:
            need(row['path'] in source_rows and all(row[k]==source_rows[row['path']][k] for k in ('bytes','sha256')),'N_FREEZE_SOURCE_SUBSET_EXACT')
        need(source_rows[BROKER]['sha256']==BROKER_SHA,'PINNED_UNARMED_OBSERVER_AND_CHILD_SOURCE')
        for row in source_rows.values():
            f=exact(dict(row,path=str(source_path(row['path']))));inputs[f['path']]=f
        common=['--locked','--offline','--target','i686-pc-windows-gnu','--target-dir',str(ROOT/'target')]
        expected={'nightly-workspace-strict-clippy':['rustup','run','nightly-2026-10-29','cargo','clippy',*common,'--workspace','--all-targets','--','-D','warnings'],
                  'stable-release-tests':['rustup','run','1.97.1','cargo','test',*common,'--release','--workspace','--all-targets','--no-run','--message-format=json'],
                  'stable-release-bins':['rustup','run','1.97.1','cargo','build',*common,'--release','-p','fc-bootstrap','--bins','--message-format=json']}
        steps={r['name']:r for r in producer['steps']};need(set(steps)==set(expected) and len(producer['steps'])==3,'EXACT_ORIGINAL_THREE_GNU_COMMANDS')
        for name,row in steps.items():
            need(row['status']=='PASS' and row['exitCode']==0 and row['actualExit'] is True and row['timedOut'] is False and row['command']==expected[name] and 0<=row['elapsedSeconds']<=900,'ORIGINAL_ACTUAL_GNU_FLAGS_900')
            for stream in ('stdout','stderr'):f=exact(row[stream]);inputs[f['path']]=f
        tests=producer['testExecutables'];normal_rows=producer['normalExecutables'];normals={r['target']:r for r in normal_rows}
        need(len(tests)==producer['testHarnessCount']==22 and len(normal_rows)==producer['normalExecutableCount']==2 and set(normals)=={'fc-bootstrap','fc-installer-lifecycle'},'EXACT22_TESTS_TWO_CLOSED_NORMAL_GUI')
        all_rows=tests+normal_rows;need(len({r['path'] for r in all_rows})==len({Path(r['path']).name for r in all_rows})==24,'UNIQUE24_ARTIFACTS_AND_NAMES')
        for row in all_rows:
            need(Path(row['path']).resolve().is_relative_to((ROOT/'target').resolve()) and row['profile']['test'] is (row in tests),'OWNED_ORIGINAL_PROFILE_TYPED')
            f=pe(row,3 if row in tests else 2);inputs[f['path']]=f
            if row in normal_rows:
                need(row['kind']==['bin'] and '/crates/fc-bootstrap#' in row['packageId'],'CLOSED_NORMAL_PACKAGE')
                inspector=row['independentObjdump'];need(inspector['exitCode']==0,'ORIGINAL_NORMAL_STATIC_GUI_INSPECTION')
                for stream in ('stdout','stderr'):f=exact(inspector[stream]);inputs[f['path']]=f
        metadata=Path(exact(steps['stable-release-tests']['stdout'])['path']);need(exact(terminal['cargoMetadata'])==fact(metadata),'TERMINAL_BINDS_ACTUAL_CARGO_METADATA')
        associations=[]
        for name,rows in [('stable-release-tests',tests),('stable-release-bins',normal_rows)]:
            listed={r['path']:r for r in rows};seen=set()
            for index,line in enumerate(Path(exact(steps[name]['stdout'])['path']).read_bytes().splitlines(keepends=True),1):
                v=json.loads(line)
                if v.get('reason')!='compiler-artifact' or not v.get('executable') or v.get('profile',{}).get('test') is not (name=='stable-release-tests'):continue
                path=v['executable'];need(path in listed and path not in seen,'EXACT_UNIQUE_ORIGINAL_CARGO_EXECUTABLE');r=listed[path]
                need(r['target']==v['target']['name'] and r['kind']==v['target']['kind'] and r['packageId']==v['package_id'] and r['profile']==v['profile'] and path in v['filenames'],'ACTUAL_CARGO_PACKAGE_TARGET_PROFILE_IDENTITY');seen.add(path)
                associations.append(dict(command=name,line=index,lineSha256=sha(line),path=path))
            need(seen==set(listed),'COMPLETE_CARGO22_AND_TWO_NORMAL_SET')
        native=[r for r in tests if r['target']=='fc_windows' and r['kind']==['lib'] and '/crates/fc-windows#' in r['packageId']];need(len(native)==1,'ONE_ACTUAL_NATIVE_LIBTEST_IDENTITY');native=native[0]
        child_parser=D/'parse-native-children-first.py';manifest=read(D/'prepared-implementation-first.json')
        for row in manifest['implementations']:f=exact(row);inputs[f['path']]=f
        need({r['path'] for r in manifest['implementations']}=={str(Path(__file__).resolve()),str(child_parser)},'EXACT_PREPARED_RUNNER_AND_CHILD_PARSER')
        inputs[str(D/'prepared-implementation-first.json')]=fact(D/'prepared-implementation-first.json')
        need(PARSER.is_file() and not PARSER.is_symlink() and sha(PARSER.read_bytes())==PARSER_SHA,'ORIGINAL_R67_STRICT_PARSER')
        f=fact(PROBE);need(f['sha256']==PROBE_SHA,'UNCHANGED_WINDOWS_PROBE_SOURCE');inputs[str(PROBE)]=f
        stdout=a.operation.with_suffix('.stdout.log');stderr=a.operation.with_suffix('.stderr.log')
        for output in (a.report,a.operation,stdout,stderr):scoped(output);need(not output.exists(),'EXCLUSIVE_NEW_RUNTIME_OUTPUTS')
        command=['python3','-B',str(PROBE),'--host','elior@192.168.10.212','--identity','/config/.local/share/future-classroom-test-access/win11-192.168.10.212','--exe',normals['fc-bootstrap']['path'],'--test-metadata',str(metadata),'--test-timeout-seconds','900','--native-seconds','1','--system-host','--public-desktop','--report',str(a.report)]
        if not a.execute:print(json.dumps(dict(status='PREPARED_GNU_RUNTIME_ONLY_NOT_EXECUTED',sourceGitBase=BASE,candidateSourceProducerSha256=a.producer_report_sha256,candidateSourceFreezeSha256=a.freeze_sha256,jointSourceFreezeSha256=a.joint_freeze_sha256,command=command,actualArtifactCount=24,expectedUploads=23,expectedHarnesses=22,reportCreated=False,scope=SCOPE)));return 0
        need(inputs=={name:fact(name) for name in inputs},'ALL_SOURCE_AND_ARTIFACT_IDENTITIES_STABLE_BEFORE_UPLOAD')
        result=dict(status='RUNNING',startedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),command=command,sourceGitBase=BASE,candidateSourceProducerSha256=a.producer_report_sha256,candidateSourceFreezeSha256=a.freeze_sha256,jointSourceFreezeSha256=a.joint_freeze_sha256,producerParentTerminal=fact(a.producer_terminal_report),originalPerTestBudgetSeconds=900,nativeSeconds=1,systemHostRequiredExitCode=0,sourceGuards=list(source_rows.values()),inputIdentities=inputs,normalHelper=normals['fc-installer-lifecycle'],normalHelperExecution='NOT_RUN_PINNED_ONLY',actualArtifactLedger=all_rows,cargoAssociations=associations,scope=SCOPE)
        with a.operation.open('x') as f:json.dump(result,f,indent=2);f.write('\n')
        began=time.monotonic();code=None
        try:
            with stdout.open('xb') as out,stderr.open('xb') as err:
                child=subprocess.Popen(command,cwd=W,stdout=out,stderr=err);result['pid']=child.pid;code=child.wait()
            result.update(exitCode=code,actualExit=True,elapsedSeconds=time.monotonic()-began,completedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),inputsUnchanged=inputs=={name:fact(name) for name in inputs})
            need(code==0 and result['inputsUnchanged'] and sha(PARSER.read_bytes())==PARSER_SHA,'ACTUAL_PARENT_RETURNED_AND_FROZEN_INPUTS_UNCHANGED')
            actual=read(a.report);result.update(actualReport=fact(a.report),actualStatus=actual.get('status'),cleanup=actual.get('cleanup'))
            old=runpy.run_path(str(PARSER));parse_native=runpy.run_path(str(child_parser))['native_harness'];old['check_platform'](actual)
            need(actual['exeSha256']==normals['fc-bootstrap']['sha256'] and actual['testTimeoutSeconds']==900 and actual['system']['exitCode']==actual['native']['hostExitCode']==0,'ACTUAL_APP_SELF_SYSTEM_HOST_NATIVE')
            uploaded={Path(r['path']).name:r['sha256'] for r in tests+[normals['fc-bootstrap']]}
            need(len(actual['uploadedArtifacts'])==23 and {r['name']:r['sha256'] for r in actual['uploadedArtifacts']}==uploaded and normals['fc-installer-lifecycle']['sha256'] not in uploaded.values(),'EXACT23_UPLOADS_NORMAL_HELPER_NOT_EXECUTED')
            need(len(actual['tests'])==22 and {r['name'] for r in actual['tests']}=={Path(r['path']).name for r in tests},'COMPLETE_ACTUAL22_HARNESS_SET')
            cases=[]
            for row in actual['tests']:
                need(row['binarySha256']==uploaded[row['name']],'EVERY_ACTUAL_TEST_BINARY_SHA');cases.append(parse_native(row,native,source_rows[BROKER]) if row['name']==Path(native['path']).name else old['harness'](row))
            steps=None
            for node in ast.parse(PROBE.read_bytes()).body:
                if isinstance(node,ast.Assign) and any(isinstance(n,ast.Name) and n.id=='PUBLIC_DESKTOP_STEPS' for n in node.targets):steps=ast.literal_eval(node.value)
            need(steps is not None and len(steps)==20,'SOURCE_PUBLIC20_CONTRACT')
            result.update(status='PASS_R75_FINITE_GNU22_WIN11_ONLY',publicDesktop=old['flow'](actual,'public',steps),harnesses=cases,totals={k:sum(r[k] for r in cases) for k in ('passed','failed','ignored','measured','filteredOut')})
        except Exception as error:
            result.update(status='FAIL_RUNTIME_OR_EVIDENCE',exitCode=code,elapsedSeconds=time.monotonic()-began,error=dict(kind=type(error).__name__,code=str(error) if isinstance(error,AssertionError) else 'LOCAL_INPUT_OR_RUNTIME_ERROR'))
            if a.report.is_file():
                try:v=read(a.report);result.update(actualReport=fact(a.report),actualStatus=v.get('status'),cleanup=v.get('cleanup'))
                except (ValueError,OSError,Waiting):pass
        for name,path in [('stdout',stdout),('stderr',stderr)]:
            if path.is_file():result[name]=fact(path)
        a.operation.write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(dict(status=result['status'],exitCode=code,cleanup=result.get('cleanup'),totals=result.get('totals'),operation=fact(a.operation))));return 0 if result['status']=='PASS_R75_FINITE_GNU22_WIN11_ONLY' else 1
    except Waiting as error:print(json.dumps(dict(status='WAIT',reasonCode=str(error),reportCreated=False)));return 3
    except AssertionError as error:print(json.dumps(dict(status='BLOCKED_INVALID_INPUT',reasonCode=str(error),reportCreated=False)));return 1
if __name__=='__main__':raise SystemExit(main())
