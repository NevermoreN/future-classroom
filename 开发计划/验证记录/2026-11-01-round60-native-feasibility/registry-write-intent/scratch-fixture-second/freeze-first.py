from pathlib import Path
import json,hashlib,shutil,difflib,ast
P=Path(__file__).parent

def info(p):
 b=p.read_bytes();return {'path':str(p),'bytes':len(b),'sha256':hashlib.sha256(b).hexdigest()}
def put(p,o):
 with p.open('x') as f:json.dump(o,f,ensure_ascii=False,indent=2);f.write('\n')
compile=json.loads((P/'compile-second.json').read_bytes());assert info(P/'registry-write-intent.c')==compile['source'];assert info(P/'registry-write-intent.exe')==compile['executable']
prior=json.loads((P/'compile-first.json').read_bytes());assert info(P/'source-first-compiled.c')['sha256']==prior['source']['sha256'];assert info(P/'registry-write-intent-first.exe')['sha256']==prior['executable']['sha256']
# The static checker ran before one runner-only final review correction. Retain
# its exact reviewed source bytes rather than silently rebinding the old report.
runner=(P/'run-windows-probe.ps1').read_text();start=runner.index('        if(Test-Path -LiteralPath $pair[1]){\n');end=runner.index('\n    }\n    $stream=',start)
old='''        if(Test-Path -LiteralPath $pair[1]){$report[$pair[0]+'Bytes']=(Get-Item -LiteralPath $pair[1]).Length;$report[$pair[0]+'Sha256']=(Get-FileHash -LiteralPath $pair[1] -Algorithm SHA256).Hash.ToLowerInvariant()}'''
oldRunner=runner[:start]+old+runner[end:]
oldBytes=oldRunner.encode();static=json.loads((P/'static-first.json').read_bytes());assert hashlib.sha256(oldBytes).hexdigest()==static['runner']['sha256']
with (P/'runner-static-first-reviewed.ps1').open('xb') as f:f.write(oldBytes)
with (P/'runner-hash-live-stream-final.patch').open('x') as f:f.write(''.join(difflib.unified_diff(oldRunner.splitlines(True),runner.splitlines(True),fromfile='static-first-reviewed/runner.ps1',tofile='final/runner.ps1')))
snap=P/'source-final';snap.mkdir()
for n in ['registry-write-intent.c','registry-write-intent.exe','run-windows-probe.ps1']:
 with (snap/n).open('xb') as f:f.write((P/n).read_bytes())
cases=[
{'id':'A','target':'same','intent':'none','ordering':'tx read ORIGINAL -> ordinary writer sets identical ORIGINAL and returns -> tx delete -> commit'},
{'id':'B','target':'same','intent':'after-read','ordering':'tx read ORIGINAL -> actual NEW child -> close child handle -> ordinary identical write completes -> tx delete -> delete child in same tx -> close parent -> commit'},
{'id':'C','target':'same','intent':'after-read, child removed early','ordering':'tx read -> actual NEW child -> close child -> transacted child delete -> ordinary identical write completes -> target delete -> close parent -> commit'},
{'id':'D','target':'same','intent':'before-read','ordering':'actual NEW child -> close child -> tx read -> ordinary identical write completes -> target delete -> child delete -> close parent -> commit'},
{'id':'E','target':'same','intent':'before-read, child removed early','ordering':'actual NEW child -> close child -> tx read -> child delete -> ordinary identical write completes -> target delete -> close parent -> commit'},
{'id':'F','target':'distinct','intent':'before-read','ordering':'actual NEW child -> tx read ORIGINAL -> ordinary writes distinct REPLACEMENT and returns -> target delete -> child delete -> close parent -> commit'},
{'id':'G','target':'distinct','intent':'none','ordering':'tx read ORIGINAL -> ordinary writes distinct REPLACEMENT and returns -> target delete -> commit; old lost-update red control'},
{'id':'H','target':'none','intent':'before-read, no competitor','ordering':'actual NEW child -> tx read ORIGINAL -> target delete -> child delete -> close parent -> commit; real progress control'}]
contract={'schemaVersion':1,'status':'PREPARED_ACTUAL_CROSS_COMPILED_NATIVE_WINDOWS_NOT_RUN','scope':'Scratch-only real process/native API falsification fixture. Not Run adapter, product installer, general CAS theorem or real Windows evidence.',
'parentResearch':info(P.parent/'research-first.json'),'parentDecision':info(P.parent/'decision-first.md'),'preservedOldSource':info(P.parent/'scratch-fixture/registry-write-intent.c'),
'source':info(snap/'registry-write-intent.c'),'executable':info(snap/'registry-write-intent.exe'),'runner':info(snap/'run-windows-probe.ps1'),'cases':cases,
'windowsSteps':[
'Run serially after the root-owned MSI operation has physically returned. No parallel Windows process/registry mutation from this author.',
'Create a fresh owned direct TEMP directory named FutureClassroomRegistryIntent-<new32lowerhex>. Copy only final EXE and final runner there using exclusive writes; verify exact frozen hashes. This directory nonce is separate from native HKCU nonce.',
'Use Windows PowerShell5.1 Parser.ParseFile to inspect the entire runner before execution; errors are an actual failed preparation, not a reason to bypass validation.',
'Execute powershell.exe -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File <owned>\\run-windows-probe.ps1 -FixtureDirectory <owned> -ExpectedSha256 '+compile['executable']['sha256'],
'Retain native-first.json and byte-original native-first.stdout.log/native-first.stderr.log, runner command/stdout/stderr plus process actual exit. No logs or reports may overwrite a prior run.',
'If native outcome or physical exit is unknown, do not run a second fixture or cleanup based only on nonce. Retain exact original PID/creation identity and all partial evidence. No shared process or registry cleanup by this runner.',
'After exact native process/child jobs physically returned, independently inspect raw statuses/ordering and cleanup. The native helper only deletes the closed synthetic leaf after all owned job ActiveProcesses==0; runner never accesses registry.'],
'budgets':{'runnerAbsoluteMillis':90000,'eachNativeCaseMillis':6000,'prepareMillis':3000,'cleanupMillis':3000,'eachTransactionTimeoutMillis':3000,'writerFirstObservationMillis':1000,'writerAfterRollbackMillis':1500,'ownedJobDrainMillis':1500,'maxStdoutBytes':1048576,'maxStderrBytes':1048576,'maxParsedLines':4096,'maxLineChars':2048,'maxSuccessfulValueBytes':256},
'ownership':[
'Native supervisor generates one16-byte RtlGenRandom nonce for this execution, fixed for all eight cases; only HKCU32 Software\\FutureClassroom-RegistryWriteIntent-Test-<nonce>. No shared intermediate parent creation.',
'Native prepare opens existing Software, creates only leaf and requires real REG_CREATED_NEW_KEY. It writes original token SID as exact REG_BINARY plus nonce through same returned root HKEY. Parent proceeds only after prepare0 and actual physical child/job return. Nonzero/timeout preparation never adopts or cleans a marker-observed root.',
'Each case/prepare/cleanup is CreateProcess suspended -> original HANDLE/GetProcessTimes identity -> assign own KILL_ON_JOB_CLOSE Job -> Resume. Case writer is another actual process; all shared operations are fixed synthetic strings under one of A..H.',
'Every original native handle/RegClose return is logged. A completed ordinary writer process is the causal barrier before target delete; no sleep positions that race. If not returned within1s, rollback original transaction and observe its actual return up to1.5s; blocked/rejected/timeout never becomes a positive protection claim.',
'Supervisor observes original process exit and owned Job ActiveProcesses0 before cleanup. Owned timeout termination applies only to that new suspended child/job; no PID lookup, foreign process termination, msiserver or real application owner mutation.',
'Cleanup requires exact nonce/SID and closed names/value schema: root markers only, A..H children only, exact sentinel and target ORIGINAL/REPLACEMENT/Absent, and only empty own intent child. Delete only this leaf. This scratch check/delete is not an adversarial same-SID atomic cleanup CAS guarantee.'],
'rawWire':{'nonce':'RWI_OWNED_NONCE <32lowerhex>; exactly once from original native supervisor. Public synthetic ownership ID only.','events':'RWI_EVENT pid=<u32> seq=<u32> tick_ms=<GetTickCount64> name=<fixed scalar> code=<raw Win32/LSTATUS> detail=<numeric>. Created process HANDLE identity uses original_child_process_identity plus original_creation_low/high; physical exit/wait and job counts are separate events.','values':'Only successful exact bounded REG_SZ reads emit RWI_VALUE type=<native> bytes=<actual> hex=<exact successful bytes>. Failed query lengths are not returned value evidence. Actual SID bytes are compared internally and are not printed.','ordering':'Per-process sequence and event names plus original writer-process completion establish the barrier. No wall-clock alignment or process timestamp inference supplies a missing completed native call. Output interleaving or unexpected lines fails parser and retains original raw logs.'},
'interpretation':[
'F with ordinary set/open/close all0 and exact writer actual exit0 BEFORE target delete, followed by own commit0 and finalAbsent, is a distinct-value stale-delete counterexample.',
'G is a no-intent distinct-value red control; successful deletion after the completed replacement reproduces the lost update without claiming the candidate passed.',
'A/B/C/D/E identical write can establish whether this actual execution conflicted; identical bytes followed by committed delete is explicitly NOT itself a distinct-value value-CAS counterexample.',
'H commit0 plus finalAbsent, unchanged sentinel and missing intent is required to establish no-competitor progress; an always-failing scheme is not progress.',
'Writer timeout/rejection, native transaction failure/unknown, killed process, imperfect cleanup or missing child observations remain FAIL/INCONCLUSIVE. The EXE and runner preserve their actual nonzero statuses.',
'Native exit0 and OBSERVATIONS_COMPLETE... mean only these finite observations and synthetic cleanup completed. They never mean candidate CAS is correct, supported universally, production-qualified or that absent documented conflict granularity has been supplied.'],
'localActual':{'compile':info(P/'compile-second.json'),'staticPe':info(P/'static-first.json'),'windowsExecution':'NOT_RUN','PowerShellParse':'NOT_RUN_NO_LOCAL_PWSH','mainRustCargoCiWindowsNetwork':'NOT_TOUCHED'},
'iterationMapping':{'firstCompileOriginal':info(P/'compile-first.json'),'firstCompiledSourceRetained':info(P/'source-first-compiled.c'),'firstCompiledExeMovedWithoutByteChange':info(P/'registry-write-intent-first.exe'),'secondCompileReason':'Conservative prepare failure boundary: physical return plus marker alone never authorizes cleanup after unsuccessful NEW-ownership establishment.','staticFirstRunnerOriginalRetained':info(P/'runner-static-first-reviewed.ps1'),'finalRunnerOnlyDelta':'Live asynchronous output streams cannot be final-hashed while still open; retain incomplete hash status instead of losing unknown-state report in finally.','runnerDelta':info(P/'runner-hash-live-stream-final.patch')},
'notExercised':['Any Windows execution','Real Run/autostart values','Production CAS/maintenance guard','Other transaction competitors/preopened handles/existing-intent takeover/cancellation matrix beyond fixed eight cases','MSI install/repair/uninstall','Win7/10 OS or MSVC ABI runtime']}
put(P/'execution-contract-first.json',contract)
finalStatic={'status':'PASS_LOCAL_BUILD_PE_SOURCE_BINDING_ONLY_WINDOWS_NOT_RUN','originalStaticReport':info(P/'static-first.json'),'finalSource':info(snap/'registry-write-intent.c'),'finalExe':info(snap/'registry-write-intent.exe'),'finalRunner':info(snap/'run-windows-probe.ps1'),'changesAfterOriginalStatic':'Runner-only final-hash handling when output-copy task has not physically returned; native source and executable unchanged from actual second compile/objdump. Original static runner exact bytes retained.','windowsParser':'NOT_RUN','contract':info(P/'execution-contract-first.json')}
put(P/'static-final-first.json',finalStatic)
manifest={'status':'FROZEN_NATIVE_FIXTURE_PREPARED_WINDOWS_NOT_RUN','files':[info(p) for p in sorted(P.rglob('*')) if p.is_file() and p.name!='freeze-first.json'],'immutableDelivery':str(snap),'contract':info(P/'execution-contract-first.json'),'limitations':'Only actual gcc/PE/source binding. No model/native execution success claim; root owns actual Windows execution. Old scratch-fixture and research remain unchanged.'}
put(P/'freeze-first.json',manifest)
print(json.dumps({'freeze':info(P/'freeze-first.json'),'contract':info(P/'execution-contract-first.json'),'source':info(snap/'registry-write-intent.c'),'exe':info(snap/'registry-write-intent.exe'),'runner':info(snap/'run-windows-probe.ps1')},ensure_ascii=False))
