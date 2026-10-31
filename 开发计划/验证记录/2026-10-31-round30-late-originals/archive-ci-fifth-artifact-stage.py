"""Archive only declared nonbinary originals; never execute verifier or Git/remote."""
from pathlib import Path
import base64,hashlib,importlib.util,json,shutil
SRC=Path('/tmp/fc-round30-evidence');DST=Path('/workspace/共享/Eliora/未来教室计划/开发计划/验证记录');PREFIX='2026-10-31-round30-'
NAMES=['ci-fifth-run-artifact.json','verify-ci-fifth-original-artifact.py','verify-ci-fifth-original-artifact-second.py','ci-fifth-original-artifact-verification-second.json','ci-fifth-local-binary-cleanup.json','ci-dispatch-seventh-once.py','ci-dispatch-seventh.json','git-ci-payload-sync.json','ci-sixth-accepted-jobs-initial.json']
sha=lambda b:hashlib.sha256(b).hexdigest()
spec=importlib.util.spec_from_file_location('old_checks',SRC/'archive-ci-tool-resolution-stage.py');checks=importlib.util.module_from_spec(spec);spec.loader.exec_module(checks)
def scan_embedded(value):
 if isinstance(value,dict):
  for key,child in value.items():
   if isinstance(child,str) and 'base64' in key.lower():
    data=base64.b64decode(child,validate=True);assert not data.startswith((b'MZ',b'PK\x03\x04'));checks.secret_check(data)
   scan_embedded(child)
 elif isinstance(value,list):
  for child in value:scan_embedded(child)
raw=SRC/'ci-fifth-run-artifact-raw';rawfiles=sorted(p for p in raw.rglob('*') if p.is_file());assert len(rawfiles)==73
files=[SRC/n for n in NAMES]+rawfiles;assert len(files)==len(set(files))==82
values={}
for p in files:
 assert p.is_file() and not p.is_symlink() and p.suffix.lower() not in {'.zip','.exe','.dll'}
 data=p.read_bytes();assert not data.startswith((b'MZ',b'PK\x03\x04'));checks.secret_check(data)
 if p.suffix=='.json':
  value=json.loads(data);scan_embedded(value);values[str(p.relative_to(SRC))]=value
artifact=values['ci-fifth-run-artifact.json'];verified=values['ci-fifth-original-artifact-verification-second.json'];cleanup=values['ci-fifth-local-binary-cleanup.json'];head='551299e17ba69f6944cf924917c632eaa005e07a';zipsha='52c64e8b7058a4423f0268e836c4a076eb1c97b26d02668988d9abd7fbf1c821'
assert artifact['run_id']==verified['runId']==41207536551 and artifact['source_sha']==verified['sourceSha']==head
assert artifact['zip_bytes']==verified['zipBytes']==37661711 and artifact['zip_sha256']==verified['zipSha256']==zipsha
members=artifact['members'];assert len(members)==verified['members']==95 and len({m['path'] for m in members})==95
nonbinary={m['path']:m for m in members if m['binary'] is False};binary={m['path']:m for m in members if m['binary'] is True};assert len(nonbinary)==verified['nonbinaryMembers']==73 and len(binary)==22
assert {str(p.relative_to(raw)) for p in rawfiles}==set(nonbinary)
for relative,row in nonbinary.items():
 p=raw/relative;data=p.read_bytes();assert str(p)==row['preserved_path'] and len(data)==row['bytes'] and sha(data)==row['sha256']
manifest=values['ci-fifth-run-artifact-raw/artifact-sha256.json'];assert len(manifest)==94 and len({r['path'] for r in manifest})==94
assert {r['path'] for r in manifest}==set(nonbinary)|set(binary)-{'artifact-sha256.json'} if False else True
assert {r['path'] for r in manifest}==set(m['path'] for m in members)-{'artifact-sha256.json'}
for row in manifest:
 member=nonbinary.get(row['path'],binary.get(row['path']));assert member is not None and row['bytes']==member['bytes'] and row['sha256']==member['sha256']
programs=verified['programs'];assert len(programs)==22 and {r['path'] for r in programs}==set(binary)
for row in programs:
 member=binary[row['path']];assert row['bytes']==member['bytes'] and row['sha256']==member['sha256'] and row['machine']=='0x014c' and row['optionalMagic']=='0x010b' and row['executed'] is False
 assert not Path(member['preserved_path']).exists()
assert cleanup['status']=='PASS' and cleanup['programs']==programs and cleanup['artifactZipSha256']==zipsha and cleanup['removedFiles']==23 and cleanup['removedBytes']==111589903
assert cleanup['removedBytes']==37661711+sum(r['bytes'] for r in programs)
assert all(not Path(p).exists() for p in cleanup['roots']) and not Path(artifact['original_archive_path']).exists()
result=values['ci-fifth-run-artifact-raw/result.json'];original_cleanup=values['ci-fifth-run-artifact-raw/cleanup.json'];failure=values['ci-fifth-run-artifact-raw/failure.json']
assert result['status']==verified['originalOverallStatus']=='FAILED_OR_INCOMPLETE' and result['source_sha']==head and result['executable_count']==22
assert failure==verified['originalFailure'] and 'bytes' in failure['message'];assert original_cleanup==verified['originalCleanup'] and original_cleanup['status']=='OWNED_BUILD_ROOTS_REMOVED' and all(r['removed'] for r in original_cleanup['roots'])
assert all(result[k]=='NOT_RUN' for k in ['windows7','windows10','windows11','seewo_device'])
steps={r['name']:r for r in result['commands']};assert not any(name.startswith('pe-imports-') for name in steps) and 'source-clean-after' not in steps
for name in ['workspace-test-compile','product-compile','clippy']:
 step=steps[name];assert step['exit_code']==0 and step['passed'] is True and step['parent_exited'] is True and step['timed_out_or_pipe_stalled'] is False
 assert verified['actualCommandResults'][name]=={'exitCode':0,'elapsedSeconds':step['duration_seconds']}
inputs=values['ci-fifth-run-artifact-raw/input-sha256.json'];assert len(inputs)==verified['gitCheckoutInputsReconstructed']==3228
assert verified['exactGitBlobInputs']==962 and verified['exactLfToCrlfCheckoutInputs']==2266 and len(verified['lfToCrlfPaths'])==len(set(verified['lfToCrlfPaths']))==2266
assert set(verified['lfToCrlfPaths'])<={r['path'] for r in inputs} and '.cargo/config.toml' in verified['lfToCrlfPaths']
dispatch=values['ci-dispatch-seventh.json'];sync=values['git-ci-payload-sync.json'];initial=values['ci-sixth-accepted-jobs-initial.json']['value']['structuredContent']['jobs'];newhead='6bde62ccf123ce4ed397c354f56d23d2dc9ee3b9'
assert dispatch['sourceSha']==sync['head']==newhead and dispatch['runId']==41209904161 and dispatch['status']=='RUN_IDENTIFIED' and dispatch['conclusion'] is None and sync['remoteVerified'] is True
assert len(initial)==1 and initial[0]['id']==124562515788 and initial[0]['run_id']==41209904161 and initial[0]['status']=='in_progress' and initial[0]['conclusion'] is None
ip=DST/(PREFIX+'stage-progress-index.json');oldbytes=ip.read_bytes();index=json.loads(oldbytes);old=index['files'][:];assert len(old)==632 and sha(oldbytes)=='3792314618bd586bf088203ad8bbcac631b4ed9fba6f2a7886307daa090f726c'
for row in old:
 data=(DST/row['path']).read_bytes();assert len(data)==row['bytes'] and sha(data)==row['sha256']
mapping=[]
for p in files:
 data=p.read_bytes();relative=PREFIX+str(p.relative_to(SRC));target=DST/relative;assert not target.exists() or target.read_bytes()==data
 mapping.append({'original':str(p),'archived':relative,'bytes':len(data),'sha256':sha(data)})
summary={'schemaVersion':1,'scope':'Supplement completed original failed run artifact metadata, nonbinary originals and binary cleanup; new corrected CI initial observation only. No Round31 source or live reports included.',
 'originalRun':{'id':41207536551,'headSha':head,'status':result['status'],'executableCount':22,'failure':failure,'actualCompileCommands':verified['actualCommandResults'],'cleanup':original_cleanup,'windowsRuntime':'NOT_RUN','PEImportsAndStaticCrt':verified['peImportsAndStaticCrt'],'sourceAfter':verified['sourcePostBuildImmutability']},
 'originalArtifact':{'zipBytes':37661711,'zipSha256':zipsha,'members':95,'nonbinaryOriginalsArchived':73,'binaryExesExcluded':22,'checksumRows':94,'verification':'All73 available raw exact bytes/SHA independently rechecked; all94 manifest rows cross-checked against95 original member metadata and22 PE-header records. Original ZIP CRC/member/source/PE checks were performed by root before exact binary cleanup; not rerun after deletion.','rootSourceReconstruction':{'inputs':3228,'exactGitBlobBytes':962,'exactLfToCrlfCheckoutBytes':2266,'scope':'Counts/conversion-path metadata cross-checked against original input manifest; no Git command executed by this archive task. Prebuild inputs only; no successful source-after proof inferred.'},'firstVerifierFailurePreserved':verified['firstVerifierFailure'],'binaryPolicy':'Only original scripts/JSON/logs archived. No EXE/ZIP or their binary base64 committed.'},
 'localCleanup':{'status':'PASS','removedFiles':23,'removedBytes':111589903,'allKnownBinaryAndZipPathsAbsent':True,'programHashRecordsMatchOriginalManifest':True,'scope':cleanup['scope']},
 'correctedRunInitialObservation':{'id':41209904161,'jobId':124562515788,'headSha':newhead,'asObservedStatus':'in_progress','asObservedConclusion':None,'dispatchOriginalStatus':dispatch['status'],'scope':'Only initial observation archived; no terminal result or corrected gate success inferred.'},
 'preservation':{'oldFiles':632,'oldIndexSha256':sha(oldbytes),'originals':mapping,'scan':'No concrete credentials/private keys/signedURLs, EXE/ZIP files or embedded binary base64 in exact allowlist; original bytes/states retained.'},
 'notClaimed':['Original overall FAILED_OR_INCOMPLETE is not upgraded to PASS','PE32 headers do not prove PE imports/static CRT, source-after, runtime or OS compatibility','No target test EXE executed; no new Win7/10/11/Seewo evidence','Corrected CI terminal result is not yet archived','No Round31 build/live evidence touched'], 'fullGoal':'ACTIVE_NOT_COMPLETE'}
sp=SRC/'ci-fifth-original-artifact-stage-summary.json'
with sp.open('x') as output:output.write(json.dumps(summary,ensure_ascii=False,indent=2)+'\n')
files.append(sp);existing={r['path'] for r in old}
for p in files:
 data=p.read_bytes();relative=PREFIX+str(p.relative_to(SRC));target=DST/relative;assert relative not in existing
 if target.exists():assert target.read_bytes()==data
 else:target.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(p,target)
 assert target.read_bytes()==data;index['files'].append({'path':relative,'bytes':len(data),'sha256':sha(data)});existing.add(relative)
assert index['files'][:632]==old and ip.read_bytes()==oldbytes
ip.write_text(json.dumps(index,ensure_ascii=False,indent=2)+'\n')
for row in index['files']:
 data=(DST/row['path']).read_bytes();assert len(data)==row['bytes'] and sha(data)==row['sha256']
print(json.dumps({'status':'ARCHIVE_ORIGINAL_BYTES_SHA_PASS','addedFiles':len(files),'indexedFiles':len(index['files']),'indexedBytes':sum(r['bytes'] for r in index['files']),'indexSha256':sha(ip.read_bytes()),'manifestSha256':sha(json.dumps(index['files'],ensure_ascii=False,sort_keys=True,separators=(',',':')).encode()),'summary':str(DST/(PREFIX+sp.name)),'summarySha256':sha(sp.read_bytes())},ensure_ascii=False,indent=2))
