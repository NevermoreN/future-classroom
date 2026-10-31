"""Exact allowlist archive of failed CI observation and CI-only byte recount fix."""
from pathlib import Path
import base64, hashlib, importlib.util, json, re, shutil
SRC=Path('/tmp/fc-round30-evidence')
DST=Path('/workspace/共享/Eliora/未来教室计划/开发计划/验证记录')
PREFIX='2026-10-31-round30-'
NAMES=['git-ci-linker-help-sync.json','ci-dispatch-sixth-once.py','ci-dispatch-sixth.json','ci-fifth-accepted-jobs-initial.json','ci-fifth-accepted-jobs-terminal.json','ci-fifth-run-tools.json','service-msvc-measure-readonly-probe.ps1','ci-measure-dictionary-stdin-probe.ps1','ci-measure-dictionary-windows-ps51.json','ci-payload-accounting-correction.json','ci-payload-accounting-readonly-probe.ps1','ci-payload-accounting-windows-readonly.json']
sha=lambda b:hashlib.sha256(b).hexdigest()
spec=importlib.util.spec_from_file_location('old_archive_checks',SRC/'archive-ci-tool-resolution-stage.py');checks=importlib.util.module_from_spec(spec);spec.loader.exec_module(checks)

def validate_embedded(value):
    if isinstance(value,dict):
        for key,child in value.items():
            if isinstance(child,str) and 'base64' in key.lower():
                data=base64.b64decode(child,validate=True)
                assert not data.startswith((b'MZ',b'PK\x03\x04')), 'No binary payload admitted'
                checks.secret_check(data)
            validate_embedded(child)
    elif isinstance(value,list):
        for child in value:validate_embedded(child)

files=[SRC/n for n in NAMES]; values={}
assert len(files)==len(set(files))==12
for p in files:
    assert p.is_file() and not p.is_symlink()
    data=p.read_bytes();checks.secret_check(data)
    if p.suffix=='.json':
        values[p.name]=json.loads(data);validate_embedded(values[p.name])
    if p.suffix=='.ps1':
        for encoded in re.findall(rb"FromBase64String\('([A-Za-z0-9+/=]+)'\)",data):
            decoded=base64.b64decode(encoded,validate=True);assert not decoded.startswith((b'MZ',b'PK\x03\x04'));checks.secret_check(decoded)
correction=values['ci-payload-accounting-correction.json']
for phase in ['before','after']:
    entry=correction[phase];data=base64.b64decode(entry['base64'],validate=True)
    assert len(data)==entry['bytes'] and sha(data)==entry['sha256']
assert correction['before']['sha256']=='e7142bf65e1fc19decfe19e0e810c39bd7f604b738aeec3a87da5cf2e9ff6cee'
source_sha='94e853dbb55f40d65ff914dbe767408ffc412cc75b3650e19a0f139f54d525cd'
assert correction['after']['sha256']==source_sha
assert 'validation_pending' in correction and 'required' in correction['validation_pending']
assert all(c['exit_code']==0 for c in correction['checks'])
actual_source=Path('/workspace/共享/Eliora/未来教室计划/scripts/windows-msvc-ci.ps1').read_bytes()
assert sha(actual_source)==source_sha
probe_reports={}
for report_name,script in [('ci-measure-dictionary-windows-ps51.json','ci-measure-dictionary-stdin-probe.ps1'),('ci-payload-accounting-windows-readonly.json','ci-payload-accounting-readonly-probe.ps1')]:
    value=values[report_name];assert value['exit_code']==0 and value['probe_sha256']==sha((SRC/script).read_bytes())
    for stream in ['stdout','stderr']:
        assert base64.b64decode(value[stream+'_base64'],validate=True).decode()==value[stream]
    probe_reports[report_name]=json.loads(value['stdout'])
repro=probe_reports['ci-measure-dictionary-windows-ps51.json'];assert repro['ps_version']=='5.1.26100.9711' and len(repro['observations'])==6
assert [r['outcome'] for r in repro['observations']]==['THREW','THREW','RETURNED','RETURNED','RETURNED','RETURNED']
actual=probe_reports['ci-payload-accounting-windows-readonly.json'];assert actual['status']=='PASS' and actual['parserErrors']==0 and actual['caseCount']==len(actual['cases'])==10 and all(c['passed'] for c in actual['cases'])
assert actual['sourceSha256']==source_sha and actual['powershell']=='5.1.26100.9711'
embedded=re.findall(rb"FromBase64String\('([A-Za-z0-9+/=]+)'\)",(SRC/'ci-payload-accounting-readonly-probe.ps1').read_bytes());assert len(embedded)==1 and base64.b64decode(embedded[0],validate=True)==actual_source
head='551299e17ba69f6944cf924917c632eaa005e07a';dispatch=values['ci-dispatch-sixth.json'];assert dispatch['sourceSha']==head and int(dispatch['runId'])==41207536551 and dispatch['headShaConfirmed'] is True
assert dispatch['status']=='RUN_IDENTIFIED'
jobs=values['ci-fifth-accepted-jobs-terminal.json']['structuredContent']['jobs'];assert len(jobs)==1;job=jobs[0]
assert job['status']=='completed' and job['conclusion']=='failure' and int(job['run_id'])==41207536551
assert [s['conclusion'] for s in job['steps'][:6]]==['success','success','success','failure','success','success']
log=values['ci-fifth-run-tools.json'][0]['value']['structuredContent']['content']
commands=[]
for name,duration in [('workspace-test-compile',841),('product-compile',1),('clippy',68)]:
    expected=f'{name}: exit=0 passed=True duration={duration}s';assert expected in log;commands.append({'name':name,'exitCode':0,'passed':True,'durationSeconds':duration,'evidence':'Exact hosted log line, not executable/runtime tests'})
assert 'Compilation evidence failed: Cannot process argument because the value of argument "bytes" is not valid.' in log
assert '37661711' in log and '52c64e8b7058a4423f0268e836c4a076eb1c97b26d02668988d9abd7fbf1c821' in log
ip=DST/(PREFIX+'stage-progress-index.json');original=ip.read_bytes();index=json.loads(original);prior=index['files'][:]
assert len(prior)==619 and sha(original)=='554ed7d818c9a5d4757fac3f42d7183d935dcd2b68a499531874e31b5b3c58bd'
for row in prior:
    data=(DST/row['path']).read_bytes();assert len(data)==row['bytes'] and sha(data)==row['sha256']
mapping=[]
for p in files:
    data=p.read_bytes();relative=PREFIX+p.name;target=DST/relative
    assert not target.exists() or target.read_bytes()==data
    mapping.append({'original':str(p),'archived':relative,'bytes':len(data),'sha256':sha(data)})
summary={'schemaVersion':1,'scope':'Round30 failed hosted MSVC run and CI-only payload-accounting repair. Round31 source worktree excluded; no new CI, runtime or OS acceptance inferred.',
 'run':{'id':41207536551,'headSha':head,'conclusion':'failure','commandsObservedPassed':commands,'jobCleanupStep':'success','artifactUploadStep':'success','dispatchInitialStatusPreserved':dispatch['status']},
 'failure':{'phase':'Post-compile executable collection byte accounting','reason':'Measure-Object bytes -Sum cannot obtain dictionary key bytes as an object property. Overall failure is preserved.','executableCount':'PENDING_ORIGINAL_ARTIFACT','PEImportChecks':'PENDING_ORIGINAL_ARTIFACT','rawCleanupDetails':'PENDING_ORIGINAL_ARTIFACT'},
 'artifactPending':{'status':'NOT_ARCHIVED_IN_THIS_STAGE','reportedZipBytes':37661711,'reportedZipSha256':'52c64e8b7058a4423f0268e836c4a076eb1c97b26d02668988d9abd7fbf1c821','evidence':'Hosted upload log only; live download not touched or duplicated. Exact original payload metadata will be archived separately. No ZIP, EXE or binary base64 copied into Git.'},
 'dictionaryReproduction':{'powershell':repro['ps_version'],'observations':repro['observations'],'scope':'Actual read-only PS5.1 stdin-derived script. Original PS7 ReportPath probe is preserved as unexecuted source; not described as a PS7 execution.'},
 'correction':{'sourceSha256':source_sha,'scope':correction['scope'],'limits':correction['limits'],'validationPendingOriginalPreserved':correction['validation_pending'],'actualReadonlyValidation':{'powershell':actual['powershell'],'status':'PASS','parserErrors':0,'cases':actual['cases'],'caseCount':10,'exitCode':0,'embeddedProductionSourceExact':True},'staticChecks':correction['checks']},
 'preservation':{'oldFiles':619,'oldIndexSha256':sha(original),'originals':mapping,'checks':'All old and new files exact bytes/SHA; original status/validation_pending and CLIXML retained. Concrete credentials/private keys/signed URLs and binary archive payloads rejected.'},
 'notClaimed':['Corrected hosted CI has not yet run','Compiled test binaries have not been executed by this evidence task','No Rust behavior or developer ZIP changes; no repeated Rust tests for this CI-only fix','No new Windows7/10/11, Seewo or production-listener acceptance'], 'fullGoal':'ACTIVE_NOT_COMPLETE'}
sp=SRC/'ci-payload-accounting-stage-summary.json'
with sp.open('x') as output:output.write(json.dumps(summary,ensure_ascii=False,indent=2)+'\n')
files.append(sp)
existing={r['path']:r for r in prior}
for p in files:
    data=p.read_bytes();relative=PREFIX+p.name;target=DST/relative;row={'path':relative,'bytes':len(data),'sha256':sha(data)}
    assert relative not in existing
    if target.exists():assert target.read_bytes()==data
    else:shutil.copyfile(p,target)
    assert target.read_bytes()==data
    index['files'].append(row);existing[relative]=row
assert ip.read_bytes()==original and index['files'][:619]==prior
ip.write_text(json.dumps(index,ensure_ascii=False,indent=2)+'\n')
for row in index['files']:
    data=(DST/row['path']).read_bytes();assert len(data)==row['bytes'] and sha(data)==row['sha256']
print(json.dumps({'status':'ARCHIVE_ORIGINAL_BYTES_SHA_PASS','addedFiles':13,'indexedFiles':len(index['files']),'indexedBytes':sum(r['bytes'] for r in index['files']),'indexSha256':sha(ip.read_bytes()),'manifestSha256':sha(json.dumps(index['files'],ensure_ascii=False,sort_keys=True,separators=(',',':')).encode()),'summary':str(DST/(PREFIX+sp.name)),'summarySha256':sha(sp.read_bytes()),'newFiles':[PREFIX+p.name for p in files]},ensure_ascii=False,indent=2))
