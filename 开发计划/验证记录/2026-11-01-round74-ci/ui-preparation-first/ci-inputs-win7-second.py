"""R74 terminal original CI gate and exact dual-GUI static audit invocation.
Preparation only unless --execute-static. No network/download/product execution.
No ZIP access until the CI, download receipt and download parent are terminal.
"""
from pathlib import Path
import argparse, datetime, hashlib, json, re, subprocess, sys, time
sys.dont_write_bytecode=True
D=Path(__file__).resolve().parent;E=D.parent
REPO=Path('/workspace/共享/Eliora/未来教室计划')
AUDITOR=D/'verify-ci-artifact-dual-gui-third.py'
AUDITOR_SHA='4047bc6598059bd99a0aaf266c670b14983263a1a7909c4a0e19c9fc8c3b9622'
BROKER='crates/fc-windows/src/installer_lifecycle/native/broker.rs'
BROKER_SHA='977a4ab8fd8cf15ee400e30bb5a6badea67f773cf500ca4d791ec4286bd96a04'
CONFIG={'modern':dict(baseline='Modern',workflow='.github/workflows/windows-msvc.yml',job='compile-i686',prefix='future-classroom-msvc'),
        'win7':dict(baseline='Win7',workflow='.github/workflows/windows-win7-msvc.yml',job='compile-win7-i686',prefix='future-classroom-win7-msvc')}
class Waiting(Exception):pass
class Invalid(Exception):pass
def need(v,c):
    if not v:raise Invalid(c)
def sha(b):return hashlib.sha256(b).hexdigest()
def scoped(path):
    p=Path(path);need(p.resolve().is_relative_to(E.resolve()) and not any(x.is_symlink() for x in [p,*p.parents] if x==E or E in x.parents),'R74_INPUT_SCOPE_REGULAR')
    need(p.suffix.lower() not in {'.key','.pfx','.p12'},'PRIVATE_CONTENT_FORBIDDEN');return p
def fact(path):
    p=scoped(path)
    if not p.is_file():raise Waiting('MISSING_'+p.name)
    h=hashlib.sha256();size=0
    with p.open('rb') as f:
        while data:=f.read(1024*1024):h.update(data);size+=len(data)
    return dict(path=str(p),bytes=size,sha256=h.hexdigest())
def read(path):
    p=scoped(path)
    if not p.is_file():raise Waiting('MISSING_'+p.name)
    try:v=json.loads(p.read_text(encoding='utf-8-sig'))
    except (ValueError,UnicodeError):raise Waiting('INCOMPLETE_'+p.name) from None
    if isinstance(v,dict) and v.get('status') in ('RUNNING','STARTED','queued','in_progress','pending','waiting','requested','OBSERVATION_INTERRUPTED_NOT_COMPLETION'):raise Waiting('NONTERMINAL_'+p.name)
    return v
def api(path):
    v=read(path)
    if isinstance(v,dict):need(v.get('isError') is not True,'API_WRAPPER_ERROR');v=v.get('structuredContent',v)
    if isinstance(v,dict) and isinstance(v.get('content'),str):
        try:v=json.loads(v['content'])
        except ValueError:raise Waiting('INCOMPLETE_API_CONTENT') from None
    return v
def exact(row):
    v=fact(row['path']);need(v['sha256']==row['sha256'] and ('bytes' not in row or v['bytes']==row['bytes']),'EXACT_REOPENED_INPUT');return v
def command_value(cmd,flag,value):need(cmd.count(flag)==1 and cmd.index(flag)+1<len(cmd) and cmd[cmd.index(flag)+1]==str(value),'ORIGINAL_COMMAND_'+flag)
def add_inputs(p):
    p.add_argument('--source-sha',required=True);p.add_argument('--baseline',choices=CONFIG,required=True)
    for n in ('run-id','job-id','artifact-id','zip-bytes'):p.add_argument('--'+n,type=int,required=True)
    p.add_argument('--zip-sha256',required=True)
    for n in ('zip','run-metadata','job-metadata','artifact-metadata','download-receipt','download-operation'):p.add_argument('--'+n,type=Path,required=True)
    p.add_argument('--download-receipt-sha256',required=True);p.add_argument('--download-operation-sha256',required=True)
def boot():
    need((E/'.owner').read_bytes()==b'future-classroom-owned-r74-ci-evidence/v1\n','EXACT_R74_OWNER')
    need((D/'.owner').read_bytes()==b'future-classroom-owned-r74-ui-preparation/v1\n','EXACT_PREPARATION_OWNER')
    need(fact(AUDITOR)['sha256']==AUDITOR_SHA,'RETAINED_SECOND_AUDITOR_EXACT')
def terminal_download(a):
    boot();cfg=CONFIG[a.baseline]
    need(re.fullmatch('[0-9a-f]{40}',a.source_sha) and re.fullmatch('[0-9a-f]{64}',a.zip_sha256),'EXPLICIT_SOURCE_ZIP_SHA')
    need(all(type(v) is int and v>0 for v in (a.run_id,a.job_id,a.artifact_id,a.zip_bytes)) and a.zip_bytes<=512*1024*1024,'EXPLICIT_FINITE_IDS_ZIP_SIZE')
    run=api(a.run_metadata);jobs=api(a.job_metadata);arts=api(a.artifact_metadata)
    if run.get('status')!='completed':raise Waiting('OFFICIAL_RUN_NOT_TERMINAL')
    need(run.get('conclusion')=='success' and run.get('id')==a.run_id and run.get('head_sha')==a.source_sha and run.get('path')==cfg['workflow'] and run.get('event')=='workflow_dispatch','SUCCESSFUL_EXACT_OFFICIAL_RUN')
    jobs=jobs.get('jobs',jobs) if isinstance(jobs,dict) else jobs;need(isinstance(jobs,list),'OFFICIAL_JOB_LIST')
    selected=[v for v in jobs if v.get('id')==a.job_id];need(len(selected)==1,'UNIQUE_EXACT_JOB');job=selected[0]
    if job.get('status')!='completed':raise Waiting('OFFICIAL_JOB_NOT_TERMINAL')
    need(job.get('run_id')==a.run_id and job.get('name')==cfg['job'] and job.get('conclusion')=='success','SUCCESSFUL_EXACT_JOB')
    arts=arts.get('artifacts',arts) if isinstance(arts,dict) else arts;arts=[arts] if isinstance(arts,dict) else arts;need(isinstance(arts,list),'OFFICIAL_ARTIFACT_LIST')
    selected=[v for v in arts if v.get('id')==a.artifact_id];need(len(selected)==1,'UNIQUE_EXACT_ARTIFACT');art=selected[0];attempt=run.get('run_attempt')
    need(type(attempt) is int and attempt>0 and art.get('expired') is False and art.get('size_in_bytes')==a.zip_bytes and art.get('digest')=='sha256:'+a.zip_sha256 and art.get('workflow_run',{}).get('id')==a.run_id and art['workflow_run'].get('head_sha')==a.source_sha and art.get('name')==f"{cfg['prefix']}-i686-{a.source_sha}-{a.run_id}-{attempt}",'EXACT_ARTIFACT_SOURCE_ZIP_DIGEST')
    receipt=read(a.download_receipt);op=read(a.download_operation)
    need(fact(a.download_receipt)['sha256']==a.download_receipt_sha256 and fact(a.download_operation)['sha256']==a.download_operation_sha256,'EXPLICIT_TERMINAL_DOWNLOAD_ORIGINALS')
    need(receipt.get('status')=='PASS' and receipt.get('sourceSha')==a.source_sha and receipt.get('runId')==a.run_id and receipt.get('artifactId')==a.artifact_id and receipt.get('expectedBytes')==receipt.get('bytes')==a.zip_bytes and receipt.get('expectedSha256')==receipt.get('sha256')==a.zip_sha256,'ACTUAL_TERMINAL_DOWNLOAD_RECEIPT')
    need(op.get('status') in ('PASS','PASS_COMMAND') and op.get('exitCode')==0 and op.get('actualExit') is True,'ACTUAL_DOWNLOAD_PARENT_EXIT0')
    cmd=op.get('command',op.get('args'));need(isinstance(cmd,list),'ORIGINAL_DOWNLOAD_COMMAND')
    for flag,value in (('--source-sha',a.source_sha),('--baseline',a.baseline),('--run-id',a.run_id),('--artifact-id',a.artifact_id),('--expected-bytes',a.zip_bytes),('--expected-sha256',a.zip_sha256)):command_value(cmd,flag,value)
    for key in ('stdout','stderr'):
        if isinstance(op.get(key),dict) and 'path' in op[key]:exact(op[key])
    # Whole ZIP is opened only here: both download originals and official CI
    # have already physically completed successfully, not a running candidate.
    z=fact(a.zip);need(z['bytes']==a.zip_bytes and z['sha256']==a.zip_sha256,'WHOLE_TERMINAL_ZIP_EXACT')
    return dict(sourceSha=a.source_sha,baseline=a.baseline,runId=a.run_id,jobId=a.job_id,artifactId=a.artifact_id,zip=z,downloadReceipt=fact(a.download_receipt),downloadOperation=fact(a.download_operation),officialMetadata=[fact(v) for v in (a.run_metadata,a.job_metadata,a.artifact_metadata)])
def audit_command(a):
    return ['python3','-B',str(AUDITOR),'--source',a.source_sha,'--baseline',CONFIG[a.baseline]['baseline'],'--zip',str(a.zip),'--zip-bytes',str(a.zip_bytes),'--zip-sha256',a.zip_sha256,'--run-id',str(a.run_id),'--job-id',str(a.job_id),'--artifact-id',str(a.artifact_id),'--run-metadata',str(a.run_metadata),'--job-metadata',str(a.job_metadata),'--artifact-metadata',str(a.artifact_metadata),'--output-dir',str(a.output_dir),'--report',str(a.static_audit),'--repo',str(REPO)]
def runtime_inputs(a):
    bound=terminal_download(a);audit=read(a.static_audit);op=read(a.static_operation)
    need(fact(a.static_audit)['sha256']==a.static_audit_sha256 and fact(a.static_operation)['sha256']==a.static_operation_sha256,'EXPLICIT_STATIC_ORIGINAL_IDENTITIES')
    need(audit.get('status')=='PASS_STATIC_ONLY_RUNTIME_NOT_RUN' and audit.get('sourceSha')==a.source_sha and audit.get('baseline')==CONFIG[a.baseline]['baseline'] and (audit.get('runId'),audit.get('jobId'),audit.get('artifactId'))==(a.run_id,a.job_id,a.artifact_id) and audit.get('helper',{}).get('sha256')==AUDITOR_SHA,'ACTUAL_SECOND_AUDITOR_EXACT_PASS')
    need(len(audit.get('phases',[]))==6 and all(v.get('status')=='PASS' for v in audit['phases']) and audit['zip']==bound['zip'],'ALL_STATIC_PHASES_WHOLE_ZIP_IDENTITY')
    need(op.get('status')=='PASS_STATIC_AUDITOR_ACTUAL_EXIT0' and op.get('exitCode')==0 and op.get('actualExit') is True and op.get('command')==audit_command(a) and op.get('actualReport')==fact(a.static_audit),'STATIC_PARENT_ACTUAL_RETURNED')
    exact(op['stdout']);exact(op['stderr'])
    rows=audit['executables'];tests=[r for r in rows if r['producer']['role']=='tests'];normals={r['producer']['target']:r for r in rows if r['producer']['role']=='product'}
    need(len(rows)==24 and len(tests)==22 and len(normals)==2 and set(normals)=={'fc-bootstrap','fc-installer-lifecycle'} and audit['counts']['executables']==24 and audit['counts']['tests']==22 and audit['counts']['product']==2 and audit['counts']['metadataExpectedTests']==22,'COMPLETE_ACTUAL24_LEDGER_22TESTS_TWO_GUI')
    need(len({Path(r['localPath']).name for r in rows})==24 and len({r['localPath'] for r in rows})==24,'UNIQUE24_PE_NAMES_AND_PATHS')
    for row in rows:
        producer=row['producer'];f=fact(row['localPath']);need(f['bytes']==producer['bytes'] and f['sha256']==producer['sha256'],'EVERY_ACTUAL24_PE_REOPENED')
        need(row['peHeader']['subsystem']==(3 if producer['role']=='tests' else 2) and producer['cargo_profile_test'] is (producer['role']=='tests'),'ROLE_BOUND_ACTUAL_GUI_CUI_PROFILE')
        exact(row['rawInspection'])
    need(audit['normalExecutables']==normals and audit['normalExecutable']==normals['fc-bootstrap'] and audit['lifecycleExecutable']==normals['fc-installer-lifecycle'],'CLOSED_GUI_NORMAL_ALIASES')
    metadata=exact(audit['testMetadata']);cargo=[]
    for line in Path(metadata['path']).read_bytes().splitlines():
        v=json.loads(line)
        if v.get('reason')=='compiler-artifact' and v.get('profile',{}).get('test') is True and v.get('executable'):cargo.append(Path(v['executable']).resolve())
    need(len(cargo)==len(set(cargo))==22 and set(cargo)=={Path(r['localPath']).resolve() for r in tests},'EXACT22_CARGO_JSON_TEST_METADATA')
    native=[r for r in tests if r['producer']['package']=='fc-windows' and r['producer']['target']=='fc_windows' and r['producer']['kinds']==['lib']];need(len(native)==1,'EXACT_SOURCE_BOUND_MSVC_NATIVE_TEST_TARGET')
    blob=subprocess.check_output(['git','show',a.source_sha+':'+BROKER],cwd=REPO);need(sha(blob)==BROKER_SHA and (REPO/BROKER).read_bytes()==blob,'SAME_PINNED_CHILD_FIXTURE_SOURCE_AT_ACTUAL_CI_SOURCE')
    sources=[r for r in audit['sourceMatches'] if r['path']==BROKER];need(len(sources)==1,'STATIC_SOURCE_MATCH_FOR_CHILD_FIXTURE');s=sources[0]
    checkout=blob if s['checkoutMatch']=='EXACT_GIT_BLOB' else blob.replace(b'\n',b'\r\n') if s['checkoutMatch']=='EXACT_FULL_LF_TO_CRLF' else None
    need(checkout is not None and sha(checkout)==s['sha256'] and len(checkout)==s['bytes'],'EXACT_CI_CHILD_SOURCE_CHECKOUT_BYTES')
    return dict(bound=bound,audit=audit,artifacts=rows,tests=tests,app=normals['fc-bootstrap'],helper=normals['fc-installer-lifecycle'],metadata=metadata,native=native[0],childSource=dict(path=BROKER,gitSourceSha256=sha(blob),ciCheckout=s))
def main():
    p=argparse.ArgumentParser(description=__doc__);add_inputs(p);p.add_argument('--output-dir',type=Path,required=True);p.add_argument('--static-audit',type=Path,required=True);p.add_argument('--static-operation',type=Path,required=True);p.add_argument('--execute-static',action='store_true');a=p.parse_args()
    try:
        bound=terminal_download(a);cmd=audit_command(a)
        for path in (a.output_dir,a.static_audit,a.static_operation):scoped(path);need(not path.exists(),'EXCLUSIVE_FRESH_STATIC_OUTPUTS')
        need(a.output_dir.resolve().is_relative_to(D) and a.static_audit.parent.resolve().is_relative_to(D),'RETAINED_AUDITOR_OUTPUT_SCOPE_IS_PREPARATION_SUBDIRECTORY')
        if not a.execute_static:print(json.dumps(dict(status='PREPARED_STATIC_COMMAND_ONLY_NOT_EXECUTED',inputs=bound,command=cmd,staticRuntime='NOT_RUN')));return 0
        stdout=a.static_operation.with_suffix('.stdout.log');stderr=a.static_operation.with_suffix('.stderr.log');need(not stdout.exists() and not stderr.exists(),'EXCLUSIVE_STATIC_CAPTURE')
        result=dict(status='RUNNING',command=cmd,inputs=bound,startedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat())
        with a.static_operation.open('x') as f:json.dump(result,f,indent=2);f.write('\n')
        began=time.monotonic();code=None
        try:
            with stdout.open('xb') as out,stderr.open('xb') as err:
                child=subprocess.Popen(cmd,cwd=REPO,stdout=out,stderr=err);result['pid']=child.pid;code=child.wait()
            result.update(exitCode=code,actualExit=True,elapsedSeconds=time.monotonic()-began,stdout=fact(stdout),stderr=fact(stderr))
            audit=read(a.static_audit);result['actualReport']=fact(a.static_audit);need(code==0 and audit['status']=='PASS_STATIC_ONLY_RUNTIME_NOT_RUN','ACTUAL_STATIC_AUDITOR_EXIT_AND_PASS');result['status']='PASS_STATIC_AUDITOR_ACTUAL_EXIT0'
        except Exception as err:result.update(status='FAIL_STATIC_COMMAND_OR_EVIDENCE',exitCode=code,elapsedSeconds=time.monotonic()-began,error=dict(kind=type(err).__name__,code=str(err) if isinstance(err,Invalid) else 'LOCAL_STATIC_INPUT_OR_TOOL_FAILURE'))
        a.static_operation.write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(dict(status=result['status'],operation=fact(a.static_operation))));return 0 if result['status']=='PASS_STATIC_AUDITOR_ACTUAL_EXIT0' else 1
    except Waiting as err:print(json.dumps(dict(status='WAIT',reasonCode=str(err),reportCreated=False,zipAccessed=False)));return 3
    except Invalid as err:print(json.dumps(dict(status='BLOCKED_INVALID_INPUT',reasonCode=str(err),reportCreated=False)));return 1
if __name__=='__main__':raise SystemExit(main())
