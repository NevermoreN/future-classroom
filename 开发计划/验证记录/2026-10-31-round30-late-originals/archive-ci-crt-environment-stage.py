"""Final explicit nonbinary archive only; no Git, remote or CI command."""
from pathlib import Path
import base64,hashlib,importlib.util,json,re,shutil
SRC=Path('/tmp/fc-round30-evidence');DST=Path('/workspace/共享/Eliora/未来教室计划/开发计划/验证记录');PREFIX='2026-10-31-round30-';sha=lambda b:hashlib.sha256(b).hexdigest()
spec=importlib.util.spec_from_file_location('prepare_only',SRC/'prepare-ci-crt-environment-stage.py');prep=importlib.util.module_from_spec(spec);spec.loader.exec_module(prep)
ADDED=['ci-crt-env-fix-frozen-static.json','ci-crt-env-fix-ast-restore-probe.ps1','ci-crt-env-flag-guard-extra.ps1','run-ci-crt-env-windows-probe.py','ci-crt-env-windows-stdin-probe.ps1','ci-crt-env-windows-probe.json','git-ci-original-artifact-sync.json','ci-crt-env-local-static-final.json']
files=list(prep.paths)+[SRC/n for n in ADDED];assert len(files)==len(set(files))==27
values=dict(prep.values)
for p in files:
 data=p.read_bytes();assert p.is_file() and not p.is_symlink() and not data.startswith((b'MZ',b'PK\x03\x04')) and p.suffix.lower() not in {'.exe','.zip','.dll'}
 prep.checks.secret_check(data)
 if p.suffix=='.json':values[str(p.relative_to(SRC))]=json.loads(data);prep.embedded(values[str(p.relative_to(SRC))])
 for encoded in re.findall(rb"FromBase64String\('([A-Za-z0-9+/=]+)'\)",data):
  decoded=base64.b64decode(encoded,validate=True);assert not decoded.startswith((b'MZ',b'PK\x03\x04'));prep.checks.secret_check(decoded)
frozen=values['ci-crt-env-fix-frozen-static.json'];final=values['ci-crt-env-local-static-final.json'];windows=values['ci-crt-env-windows-probe.json'];expected='f7e6a6f28c5e000f0f57a979a2e49c5f093764f28e652dac35f0ba2f7ab7c9b7'
for phase in ['before','after']:
 row=frozen[phase];data=base64.b64decode(row['base64'],validate=True);assert len(data)==row['bytes'] and sha(data)==row['sha256']
assert frozen['after']['sha256']==final['sourceSha256']==expected
assert sha(Path('/workspace/共享/Eliora/未来教室计划/scripts/windows-msvc-ci.ps1').read_bytes())==expected
assert frozen['status']=='SOURCE_FIXED_FROZEN_STATIC_ONLY_PENDING_POWERSHELL_AND_CI'
for evidence in [frozen['probe'],windows['stdinSource']]:
 data=Path(evidence['path']).read_bytes();assert len(data)==evidence['bytes'] and sha(data)==evidence['sha256']
assert windows['exitCode']==0 and len(windows['observations'])==2
for name in ['stdout','stderr']:
 stream=windows[name];data=base64.b64decode(stream['base64'],validate=True);assert len(data)==stream['bytes'] and sha(data)==stream['sha256']
observations=[json.loads(line) for line in base64.b64decode(windows['stdout']['base64'],validate=True).decode().splitlines() if line.strip()];assert observations==windows['observations']
restore,guard=observations;assert restore['status']==guard['status']=='PASS' and restore['parserErrors']==0 and restore['powershell']=='5.1.26100.9711'
assert restore['caseCount']==len(restore['cases'])==3 and all(r['passed'] for r in restore['cases']) and restore['presentEmptySupported'] is False
assert guard['caseCount']==len(guard['cases'])==8 and all(r['passed'] for r in guard['cases']) and guard['processFlagsRestored'] is True and guard['filesWritten']==guard['externalExecutables']==0
assert final['status']=='PASS_LOCAL_AND_PS5_ONLY' and all(r['exitCode']==0 for r in final['commands'])
assert final['actualWindowsPS5']['restoreCases']==3 and final['actualWindowsPS5']['guardCases']==8 and final['actualWindowsPS5']['processFlagsRestored'] is True
jobs=values['ci-sixth-accepted-jobs-terminal.json']['value']['structuredContent']['jobs'];assert len(jobs)==1
job=jobs[0];assert job['id']==124562515788 and job['run_id']==41209904161 and job['status']=='completed' and job['conclusion']=='failure'
assert [s['conclusion'] for s in job['steps'][:6]]==['success','success','success','failure','success','success']
indexpath=DST/(PREFIX+'stage-progress-index.json');original=indexpath.read_bytes();index=json.loads(original);prior=index['files'][:];assert len(prior)==715 and sha(original)=='3ffce6ef6855491502378ba1c6117f3d701f6dd1f1d74701dc1e546aa38651b6'
for row in prior:
 data=(DST/row['path']).read_bytes();assert len(data)==row['bytes'] and sha(data)==row['sha256']
mapping=[]
for p in files:
 data=p.read_bytes();relative=PREFIX+str(p.relative_to(SRC));target=DST/relative
 assert not target.exists() or target.read_bytes()==data
 mapping.append({'original':str(p),'archived':relative,'bytes':len(data),'sha256':sha(data)})
summary=dict(prep.summary);summary['status']='ORIGINAL_FAILED_CI_PRESERVED_REPAIR_PS5_VALIDATED_NEW_PS7_CI_PENDING';summary['scope']='Exact27 original nonbinary files plus this independent summary. No Round31 files included, no binary artifact written, no complete ZIP/CRT or runtime success inferred.'
summary['failedRun']['jobTerminalStatus']='completed';summary['failedRun']['jobConclusion']='failure';summary['failedRun']['collectorAccounting']='Original result reached pe-imports-0 after byte-ledger/count checks; no collector byte-accounting exception in this run.';summary['failedRun']['uploadStep']='success'
summary['repair']={'sourceSha256':expected,'frozenStaticOriginalStatusPreserved':frozen['status'],'scope':'Remove encoded flags through Env provider, restore prior process-only environment exactly, sample/assert bounded whitelisted RUSTFLAGS and CARGO_ENCODED_RUSTFLAGS before VS/after VS/before actual Cargo; original compile/PE/CRT rejection unchanged.','actualPS5':{'powershell':restore['powershell'],'parserErrors':0,'restoreCases':restore['cases'],'guardCases':guard['cases'],'processFlagsRestored':True,'filesWritten':0,'externalExecutables':0,'exitCode':0,'presentEmptySupported':False,'emptyRestorationStatus':restore['emptyRestoration']},'finalLocalChecks':final['commands'],'officialMechanism':final['officialMechanism'],'limits':'PS5 cannot reproduce present-empty behavior. Official PS7.5/Cargo mechanism supports inference, not sampled original hosted environment or unique historical cause; corrected actual PS7 Cargo/PE gates remain pending.'}
summary['pending']={'correctedHostedPS7':'NOT_YET_TRIGGERED_AT_RECORD_TIME','staticCrtGate':'NOT_PASSED_BY_THIS_STAGE','actualOSRuntime':'NOT_RUN_BY_THIS_STAGE'}
summary['preservation']={'oldIndexFiles':715,'oldIndexSha256':sha(original),'originals':mapping,'checks':'All old/new bytes/SHA checked;11 selected raw members independently CRC checked. Both transfer FAIL/errorKind and static pending states preserved verbatim; no binary/signedURL/credential payload admitted.'}
sp=SRC/'ci-crt-environment-stage-summary.json'
with sp.open('x') as output:output.write(json.dumps(summary,ensure_ascii=False,indent=2)+'\n')
files.append(sp);known={r['path'] for r in prior}
for p in files:
 data=p.read_bytes();relative=PREFIX+str(p.relative_to(SRC));target=DST/relative;assert relative not in known
 if target.exists():assert target.read_bytes()==data
 else:target.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(p,target)
 assert target.read_bytes()==data;index['files'].append({'path':relative,'bytes':len(data),'sha256':sha(data)});known.add(relative)
assert indexpath.read_bytes()==original and index['files'][:715]==prior
indexpath.write_text(json.dumps(index,ensure_ascii=False,indent=2)+'\n')
for row in index['files']:
 data=(DST/row['path']).read_bytes();assert len(data)==row['bytes'] and sha(data)==row['sha256']
print(json.dumps({'status':'ARCHIVE_ORIGINAL_BYTES_SHA_PASS','addedFiles':len(files),'indexedFiles':len(index['files']),'indexedBytes':sum(r['bytes'] for r in index['files']),'indexSha256':sha(indexpath.read_bytes()),'manifestSha256':sha(json.dumps(index['files'],ensure_ascii=False,sort_keys=True,separators=(',',':')).encode()),'summary':str(DST/(PREFIX+sp.name)),'summarySha256':sha(sp.read_bytes())},ensure_ascii=False,indent=2))
