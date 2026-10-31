"""Prepare explicit archive only; never copy or mutate progress/index/worktrees."""
from pathlib import Path
import base64,hashlib,importlib.util,json,zlib
SRC=Path('/tmp/fc-round30-evidence');DST=Path('/workspace/共享/Eliora/未来教室计划/开发计划/验证记录');PREFIX='2026-10-31-round30-'
NAMES=['ci-sixth-accepted-jobs-terminal.json','ci-sixth-run-tools.json','read-ci-sixth-nonbinary-ranges.py','ci-sixth-range-originals.json','read-ci-sixth-import-diagnosis.py','ci-sixth-import-diagnosis.json','windows-pwsh-availability-readonly.json','ci-sixth-crt-core-readonly-review.json']
RAW={'ci-sixth-range-raw':['bootstrap.json','cleanup.json','failure.json','logs/cargo-metadata.stdout.log','logs/pe-imports-0.result.json','logs/pe-imports-0.stderr.log','logs/pe-imports-0.stdout.log','result.json'],'ci-sixth-import-diagnosis-raw':['logs/product-compile.stdout.log','logs/rust-target-cfg.stdout.log','logs/workspace-test-compile.stdout.log']}
sha=lambda b:hashlib.sha256(b).hexdigest()
spec=importlib.util.spec_from_file_location('old_checks',SRC/'archive-ci-tool-resolution-stage.py');checks=importlib.util.module_from_spec(spec);spec.loader.exec_module(checks)
def embedded(value):
 if isinstance(value,dict):
  for k,v in value.items():
   if isinstance(v,str) and 'base64' in k.lower():
    data=base64.b64decode(v,validate=True);assert not data.startswith((b'MZ',b'PK\x03\x04'));checks.secret_check(data)
   embedded(v)
 elif isinstance(value,list):
  for v in value:embedded(v)
paths=[SRC/n for n in NAMES]
for directory,names in RAW.items():
 assert {str(p.relative_to(SRC/directory)) for p in (SRC/directory).rglob('*') if p.is_file()}==set(names)
 paths += [SRC/directory/n for n in names]
assert len(paths)==len(set(paths))==19
values={};mapping=[]
for p in paths:
 assert p.is_file() and not p.is_symlink() and p.suffix.lower() not in {'.zip','.exe','.dll'}
 data=p.read_bytes();assert not data.startswith((b'MZ',b'PK\x03\x04'));checks.secret_check(data)
 relative=str(p.relative_to(SRC));mapping.append({'original':str(p),'archiveProposed':PREFIX+relative,'bytes':len(data),'sha256':sha(data)})
 if p.suffix=='.json':values[relative]=json.loads(data);embedded(values[relative])
ranges=values['ci-sixth-range-originals.json'];diagnosis=values['ci-sixth-import-diagnosis.json']
assert ranges['status']==diagnosis['status']=='FAIL' and ranges['errorKind']=='URLError' and diagnosis['errorKind']=='HTTPError'
assert ranges['centralDirectoryMemberCount']==diagnosis['centralDirectoryMemberCount']==99
assert len(ranges['members'])==8 and len(diagnosis['members'])==6
for report,directory in [(ranges,'ci-sixth-range-raw'),(diagnosis,'ci-sixth-import-diagnosis-raw')]:
 for row in report['members']:
  if 'binaryHandling' in row:
   assert row['binaryHandling']=='MEMORY_ONLY_NOT_EXECUTED_NOT_SAVED';continue
  data=(SRC/directory/row['path']).read_bytes();assert len(data)==row['bytes'] and sha(data)==row['sha256'] and f'{zlib.crc32(data)&0xffffffff:08x}'==row['zipCrc32']
result=values['ci-sixth-range-raw/result.json'];cleanup=values['ci-sixth-range-raw/cleanup.json'];failure=values['ci-sixth-range-raw/failure.json'];steps={r['name']:r for r in result['commands']}
assert result['status']=='FAILED_OR_INCOMPLETE' and result['source_sha']=='6bde62ccf123ce4ed397c354f56d23d2dc9ee3b9' and result['executable_count']==22
for name in ['workspace-test-compile','product-compile','clippy','pe-imports-0']:assert steps[name]['exit_code']==0 and steps[name]['passed'] is True
assert 'dynamic CRT import' in failure['message'] and cleanup['status']=='OWNED_BUILD_ROOTS_REMOVED' and all(r['removed'] for r in cleanup['roots'])
binaries=[r for r in diagnosis['members'] if 'binaryHandling' in r];assert len(binaries)==3 and all('VCRUNTIME140.dll' in r['imports'] for r in binaries)
assert 'VCRUNTIME140.dll' in (SRC/'ci-sixth-range-raw/logs/pe-imports-0.stdout.log').read_text()
pwsh=values['windows-pwsh-availability-readonly.json'];assert pwsh['exitCode']==0 and pwsh['observed']['pwshPath'] is None and pwsh['observed']['windowsPowerShell']=='5.1.26100.9711'
ip=DST/(PREFIX+'stage-progress-index.json');before=ip.read_bytes();index=json.loads(before);assert len(index['files'])==715 and sha(before)=='3ffce6ef6855491502378ba1c6117f3d701f6dd1f1d74701dc1e546aa38651b6'
summary={'status':'DRAFT_ONLY_WAITING_FINAL_REPAIR_EVIDENCE','scope':'No archive copies/index/progress changes. Selected original range members only, no full-ZIP verification; no Round31 report included.',
 'failedRun':{'id':41209904161,'headSha':result['source_sha'],'originalStatus':result['status'],'actualCommands':[{k:steps[n][k] for k in ['name','exit_code','passed','duration_seconds']} for n in ['workspace-test-compile','product-compile','clippy','pe-imports-0']],'failure':failure,'cleanup':cleanup,'exeCountFromOriginalResult':22,'runtime':'NOT_RUN','sourceAfter':'NOT_RUN'},
 'rangeRetrieval':{'firstStatus':ranges['status'],'firstErrorKind':ranges['errorKind'],'secondStatus':diagnosis['status'],'secondErrorKind':diagnosis['errorKind'],'secondUnderlyingReasonKind':diagnosis['underlyingReasonKind'],'centralDirectoryMembersObserved':99,'rawMembersAvailable':11,'all11RawBytesShaCrcIndependentlyRechecked':True,'scope':'Do not change either original FAIL. 99 observed entries are not99 complete archived members. Whole-ZIP digest only API/upload metadata; not verified by selected ranges.'},
 'binaryMemoryOnly':{'count':3,'members':binaries,'scope':'Only original CRC/SHA/import metadata retained; no EXE execution, EXE/ZIP/B64 disk payload. Actual first dumpbin import log independently proves dynamic CRT despite command exit0.'},
 'environmentDiagnosis':{'observation':pwsh['observed'],'scope':'Read-only probe proves this Win11 has PS5.1 and no resolved pwsh; no PS7 installed or executed on this machine. Null-versus-empty Process environment behavior and Cargo encoded RUSTFLAGS precedence are mechanism analysis only until separate exact helper/PS7 evidence. Original hosted runner did not record actual environment values; do not assert unique historical cause.'},
 'coreReadonlyReview':values['ci-sixth-crt-core-readonly-review.json'],
 'pending':{'repairSourceSha':'AWAIT_ROOT_FROZEN_SOURCE','actualPowerShellValidation':'AWAIT_TERMINAL_ORIGINALS','explicitAdditionalAllowlist':'AWAIT_ROOT','newCI':'NOT_RUN_BY_THIS_TASK'},
 'preservation':{'oldIndexFiles':715,'oldIndexSha256':sha(before),'preparedOriginals':mapping,'archiveCopiesPerformed':False,'indexOrProgressChanged':False},'fullGoal':'ACTIVE_NOT_COMPLETE'}
(SRC/'ci-crt-environment-stage-draft-summary.json').write_text(json.dumps(summary,ensure_ascii=False,indent=2)+'\n')
(SRC/'ci-crt-environment-stage-prepared-allowlist.json').write_text(json.dumps({'status':'PREPARED_NOT_ARCHIVED','files':mapping,'currentOriginals':19,'rawFiles':11,'pendingRepairAllowlist':True},ensure_ascii=False,indent=2)+'\n')
assert ip.read_bytes()==before
print(json.dumps({'status':'PREPARED_ONLY_NO_ARCHIVE_OR_INDEX_MUTATION','originals':19,'rawFiles':11,'indexFiles':715,'indexSha256':sha(before),'draftSummarySha256':sha((SRC/'ci-crt-environment-stage-draft-summary.json').read_bytes())}))
