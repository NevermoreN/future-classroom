"""R74 real MSVC full22/public20 controller, explicitly --execute only.
Requires successful terminal download and independent 24PE/dual-GUI audit.
Root remains sole Windows owner; normal lifecycle helper is pinned, not run.
"""
from pathlib import Path
import argparse, ast, datetime, hashlib, json, runpy, subprocess, sys, time
sys.dont_write_bytecode=True
D=Path(__file__).resolve().parent;E=D.parent
REPO=Path('/workspace/共享/Eliora/未来教室计划')
GATE=D/'ci-inputs-first.py';CHILD_PARSER=D/'parse-native-children-first.py'
PARSER=REPO/'开发计划/验证记录/2026-11-01-round67-ci/independent-runtime-first.py'
PARSER_SHA='0317ffb1719737bd9dfa45ca7952c242df6ffff0baa6b1611d7b912277f9e0e5'
PROBE=REPO/'scripts/windows-probe.py'
PROBE_SHA='f176a022bf90565be7074f53a2d502d6519afd1748cdd2022f608a217d485c14'
SCOPE=('R74 successful source-bound modern MSVC 22 harnesses/app self-check/system host/hidden native/public20 on authorized Win11 build26300/WOW64 only. '
       'Two normal GUI products are pinned but lifecycle normal helper is never uploaded/executed by full runner. Separate helper unit-test harness is included in22. '
       'No Hosted CI execution is claimed by runtime alone; CI originals are separate input evidence. No Win7/Win10 OS, MSI fresh/rollback, ordinary31, recovery, visual, Edge, header, campus or final-product acceptance.')
def sha(data):return hashlib.sha256(data).hexdigest()
def local_fact(path):
    p=Path(path)
    if not p.is_file() or p.is_symlink():raise AssertionError('REGULAR_PINNED_TEMPLATE_OR_PROBE')
    return dict(path=str(p),bytes=p.stat().st_size,sha256=sha(p.read_bytes()))
def main():
    g=runpy.run_path(str(GATE));need=g['need'];fact=g['fact'];exact=g['exact'];read=g['read']
    p=argparse.ArgumentParser(description=__doc__);g['add_inputs'](p)
    p.add_argument('--output-dir',type=Path,required=True);p.add_argument('--static-audit',type=Path,required=True);p.add_argument('--static-audit-sha256',required=True);p.add_argument('--static-operation',type=Path,required=True);p.add_argument('--static-operation-sha256',required=True)
    p.add_argument('--report',type=Path,required=True);p.add_argument('--operation',type=Path,required=True);p.add_argument('--execute',action='store_true');a=p.parse_args()
    try:
        need(a.baseline=='modern','MODERN_RUNTIME_ONLY_WIN7_REQUIRES_SEPARATE_AUTHORIZED_ENTRY')
        prepared=g['runtime_inputs'](a);audit=prepared['audit'];rows=prepared['artifacts'];tests=prepared['tests'];app=prepared['app'];helper=prepared['helper'];native=prepared['native'];metadata=Path(prepared['metadata']['path'])
        need(local_fact(PROBE)['sha256']==PROBE_SHA and local_fact(PARSER)['sha256']==PARSER_SHA,'PINNED_R67_PARSER_AND_WINDOWS_PROBE')
        manifest=read(D/'prepared-implementation-first.json')
        for r in manifest['implementations']:exact(r)
        need({r['path'] for r in manifest['implementations']}=={str(GATE),str(CHILD_PARSER),str(Path(__file__).resolve()),str(D/'verify-ci-artifact-dual-gui-second.py')},'COMPLETE_PREPARED_IMPLEMENTATION4')
        exe=Path(app['localPath']);stdout=a.operation.with_suffix('.stdout.log');stderr=a.operation.with_suffix('.stderr.log')
        for output in (a.report,a.operation,stdout,stderr):g['scoped'](output);need(not output.exists(),'EXCLUSIVE_NEW_RUNTIME_OUTPUTS')
        command=['python3','-B',str(PROBE),'--host','elior@192.168.10.212','--identity','/config/.local/share/future-classroom-test-access/win11-192.168.10.212','--exe',str(exe),'--test-metadata',str(metadata),'--test-timeout-seconds','900','--native-seconds','1','--system-host','--public-desktop','--report',str(a.report)]
        if not a.execute:
            print(json.dumps(dict(status='PREPARED_RUNTIME_CONTROLLER_ONLY_NOT_EXECUTED',inputs=prepared['bound'],command=command,actualArtifacts=24,uploadedArtifacts=23,harnesses=22,normalHelperExecution='NOT_RUN_PIN_ONLY',scope=SCOPE,reportCreated=False)));return 0
        input_paths=[GATE,CHILD_PARSER,Path(__file__).resolve(),D/'prepared-implementation-first.json',D/'verify-ci-artifact-dual-gui-second.py',a.static_audit,a.static_operation,a.download_receipt,a.download_operation,a.run_metadata,a.job_metadata,a.artifact_metadata,a.zip,metadata,*[Path(r['localPath']) for r in rows]]
        before={str(v):fact(v) for v in input_paths};outside_before={str(v):local_fact(v) for v in (PROBE,PARSER,REPO/g['BROKER'])}
        result=dict(status='RUNNING',startedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),command=command,sourceSha=a.source_sha,baseline='Modern',runId=a.run_id,jobId=a.job_id,artifactId=a.artifact_id,originalPerTestBudgetSeconds=900,nativeSeconds=1,systemHostRequiredExitCode=0,staticAudit=fact(a.static_audit),staticOperation=fact(a.static_operation),inputIdentities=before,pinnedSourceIdentities=outside_before,actualArtifactLedger=rows,normalHelper=helper,normalHelperExecution='NOT_RUN_PIN_ONLY',childSource=prepared['childSource'],harnessCount=22,scope=SCOPE)
        with a.operation.open('x') as f:json.dump(result,f,indent=2);f.write('\n')
        print('WINDOWS_RUNTIME_STARTED modern',flush=True);began=time.monotonic();code=None
        try:
            with stdout.open('xb') as out,stderr.open('xb') as err:
                child=subprocess.Popen(command,cwd=REPO,stdout=out,stderr=err);result['pid']=child.pid;code=child.wait()
            result.update(exitCode=code,actualExit=True,elapsedSeconds=time.monotonic()-began,completedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat())
            after={str(v):fact(v) for v in input_paths};outside_after={str(v):local_fact(v) for v in (PROBE,PARSER,REPO/g['BROKER'])}
            result['inputsUnchanged']=before==after and outside_before==outside_after
            need(code==0 and result['inputsUnchanged'],'ACTUAL_PARENT_EXIT0_AND_ALL_INPUTS_UNCHANGED')
            report=read(a.report);result.update(actualReport=fact(a.report),actualStatus=report.get('status'),cleanup=report.get('cleanup'))
            old=runpy.run_path(str(PARSER));attribution=runpy.run_path(str(CHILD_PARSER));old['check_platform'](report)
            need(report['exeSha256']==app['producer']['sha256'] and report['testTimeoutSeconds']==900 and report['system']['exitCode']==0 and report['native']['hostExitCode']==0,'ACTUAL_WIN11_NORMAL_SELF_SYSTEM_HOST_NATIVE')
            uploaded={Path(r['localPath']).name:r['producer']['sha256'] for r in tests+[app]}
            need(len(report['uploadedArtifacts'])==23 and {r['name']:r['sha256'] for r in report['uploadedArtifacts']}==uploaded and helper['producer']['sha256'] not in uploaded.values(),'EXACT23_UPLOADED_HELPER_NORMAL_NOT_EXECUTED')
            need(len(report['tests'])==22 and {r['name'] for r in report['tests']}=={Path(r['localPath']).name for r in tests},'EXACT_ACTUAL22_HARNESS_INVENTORY')
            cases=[]
            for item in report['tests']:
                need(item['binarySha256']==uploaded[item['name']],'EVERY_ACTUAL_HARNESS_UPLOAD_SHA')
                cases.append(attribution['native_harness'](item,native,prepared['childSource']) if item['name']==Path(native['localPath']).name else old['harness'](item))
            expected=None
            for node in ast.parse(PROBE.read_bytes()).body:
                if isinstance(node,ast.Assign) and any(isinstance(t,ast.Name) and t.id=='PUBLIC_DESKTOP_STEPS' for t in node.targets):expected=ast.literal_eval(node.value)
            need(expected is not None and len(expected)==20,'FROZEN_SOURCE_PUBLIC20_STEPS')
            result.update(publicDesktop=old['flow'](report,'public',expected),harnesses=cases,totals={k:sum(r[k] for r in cases) for k in ('passed','failed','ignored','measured','filteredOut')},status='PASS_R74_MODERN_MSVC_FULL22_WIN11_ONLY')
        except Exception as err:
            result.update(status='FAIL_RUNTIME_COMMAND_OR_EVIDENCE',exitCode=code,elapsedSeconds=time.monotonic()-began,error=dict(kind=type(err).__name__,code=str(err) if isinstance(err,(g['Invalid'],AssertionError)) else 'LOCAL_INPUT_OR_RUNTIME_CAPTURE_FAILURE'))
            if a.report.is_file():
                try:v=read(a.report);result.update(actualReport=fact(a.report),actualStatus=v.get('status'),cleanup=v.get('cleanup'))
                except (ValueError,OSError,g['Waiting'],g['Invalid']):pass
        for n,path in (('stdout',stdout),('stderr',stderr)):
            if path.is_file():result[n]=fact(path)
        a.operation.write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(dict(status=result['status'],exitCode=code,cleanup=result.get('cleanup'),operation=fact(a.operation))));return 0 if result['status']=='PASS_R74_MODERN_MSVC_FULL22_WIN11_ONLY' else 1
    except g['Waiting'] as err:print(json.dumps(dict(status='WAIT',reasonCode=str(err),reportCreated=False)));return 3
    except g['Invalid'] as err:print(json.dumps(dict(status='BLOCKED_INVALID_INPUT',reasonCode=str(err),reportCreated=False)));return 1
if __name__=='__main__':raise SystemExit(main())
