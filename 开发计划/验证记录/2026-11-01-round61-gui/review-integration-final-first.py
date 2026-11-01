#!/usr/bin/env python3
"""Independent local read-only binding of R61 completed evidence. No native execution."""
from pathlib import Path
import base64,hashlib,json,re,runpy,stat,struct
E=Path(__file__).resolve().parent;W=E.parent/'worktree';M=Path('/workspace/共享/Eliora/未来教室计划')
OUT=E/'integration-independent-final-first.json';assert not OUT.exists()
checks=[];facts={}
def require(ok,name):
 if not ok:raise RuntimeError(name)
 checks.append({'name':name,'status':'PASS'})
def fact(path,expected=None):
 p=Path(path);require(p.is_file() and not p.is_symlink(),'regular actual file '+str(p));b=p.read_bytes();r={'path':str(p),'bytes':len(b),'sha256':hashlib.sha256(b).hexdigest()}
 if expected:require(r['bytes']==expected['bytes'] and r['sha256']==expected['sha256'],'exact original bytes/SHA '+str(p))
 facts[str(p)]=r;return r

def load(name):
 p=E/name;fact(p);return json.loads(p.read_bytes())
def command(r):
 require(r['exitCode']==0 and r['status']=='PASS','actual command completed exit0')
 for stream in ['stdout','stderr']:
  v=r[stream]
  if isinstance(v,dict):fact(v['path'],v)
  else:fact(v,{'bytes':r[stream+'Bytes'],'sha256':r[stream+'Sha256']})

def test_lines(lines,expected,label):
 starts=[x for x in lines if re.fullmatch(r'running [0-9]+ tests?',x)];summaries=[x for x in lines if x.startswith('test result:')]
 require(len(starts)==len(summaries)==1,'unique running and summary '+label)
 start=re.fullmatch(r'running ([0-9]+) tests?',starts[0]);summary=re.fullmatch(r'test result: ok\. ([0-9]+) passed; ([0-9]+) failed; ([0-9]+) ignored; ([0-9]+) measured; ([0-9]+) filtered out; finished in ([0-9.]+)s',summaries[0])
 require(summary is not None and int(start[1])==int(summary[1])==expected and all(int(summary[i])==0 for i in range(2,6)),'actual exact count and zero failure/ignored/measured/filtered '+label)
 names=[m[1] for x in lines if (m:=re.fullmatch(r'test (.+) \.\.\. ok',x))]
 require(len(names)==len(set(names))==expected,'unique successful test lines equal summary '+label)
 return {'label':label,'passed':expected,'failed':0,'ignored':0,'measured':0,'filtered':0,'testNames':names,'running':starts[0],'summary':summaries[0]}
def pe(path):
 b=Path(path).read_bytes();o=struct.unpack_from('<I',b,60)[0];require(b[:2]==b'MZ' and b[o:o+4]==b'PE\0\0','actual PE signature '+str(path));require(struct.unpack_from('<H',b,o+4)[0]==0x14c and struct.unpack_from('<H',b,o+24)[0]==0x10b,'actual x86 PE32 '+str(path))
 return {'subsystem':struct.unpack_from('<H',b,o+92)[0],'subsystemVersion':list(struct.unpack_from('<HH',b,o+72))}
report={'schema':'future-classroom.r61-integration-independent/v1','status':'REVIEWING','scope':'Read-only completed source/artifact/raw/report reconciliation. No Cargo, Windows/SSH/network, UI activation, source edits or CI dispatch by this reviewer.'}
compile=load('integrated-checks-first.json');programs=load('programs-static-first.json');boot=load('bootstrap-test-static-first.json')
require(compile['sourceUnchanged'] is True and compile['sourceBefore']==compile['sourceAfter'],'original441 input before/after exact')
require(compile['status']=='PASS_HOST_MODELS_AND_COMPILE_LINT_ONLY_WINDOWS_NOT_RUN' and len(compile['commands'])==10,'original ten compilation/model/host/lint stages passed')
for c in compile['commands']:require(c['actualExit'] is True,'original stage actual process exit '+c['name']);command(c)
require(compile['cargoJobs']==compile['testThreads']==2 and compile['commandTimeoutSeconds']==900 and compile['streamLimitBytes']==16*1024*1024,'original compile budget/parallelism unchanged')
subset={k:v for k,v in compile['sourceBefore'].items() if k.endswith('.rs') or k.endswith('Cargo.toml') or k=='Cargo.lock' or k.startswith('.cargo/')}
require(len(subset)==431 and subset==programs['sourceBinding']['sourceBefore'],'exact original431 Rust/Cargo/config domain')
source_facts=[]
for k,v in subset.items():source_facts.append(fact(W/k,v))
# Independent count from actual crate files, not just a report subset.
current={p.relative_to(W).as_posix() for p in (W/'crates').rglob('*') if p.is_file() and (p.suffix=='.rs' or p.name=='Cargo.toml')}
current.update(['Cargo.toml','Cargo.lock','.cargo/config.toml']);require(current==set(subset),'actual complete431 domain has no added/missing source')
require((W/'rust-toolchain.toml').read_bytes() and fact(W/'rust-toolchain.toml')['sha256']==compile['sourceBefore']['rust-toolchain.toml']['sha256'],'fixed Rust toolchain also unchanged')
require(programs['status']=='PASS_STATIC_ONLY_RUNTIME_NOT_RUN' and programs['sourceBinding']['currentMatchesBefore'] is True,'original GNU artifact inspection passed')
normal=None;window_test=None
for row in programs['programs']:
 fact(row['identity']['path'],row['identity']);fact(row['producerLog']['path'],row['producerLog']);command(row['producerCommand']);command({'status':'PASS',**row['objdump']});command({'status':'PASS',**row['audit']})
 raw=Path(row['producerLog']['path']).read_text();artifacts=[json.loads(x) for x in raw.splitlines() if x.startswith('{')];require(row['producer'] in artifacts,'exact original producer JSON '+row['role'])
 actual=pe(row['identity']['path']);expected=2 if row['role']=='normal' else 3;require(actual['subsystem']==expected==row['directPeHeader']['subsystem'],'actual product GUI2/test CUI3 '+row['role'])
 if row['role']=='normal':normal=row
 else:window_test=row
require(normal is not None and window_test is not None,'both actual GNU roles present')
fact(programs['testMetadata']['path'],programs['testMetadata']);fact(boot['program'],{'bytes':boot['bytes'],'sha256':boot['sha256']});fact(boot['metadata']['path'],boot['metadata']);require(pe(boot['program'])['subsystem']==3,'bootstrap actual test harness CUI3')
command(load('windows-bootstrap-tests-compile-first.command.json'))
boot_artifacts=[json.loads(x) for x in Path(boot['metadata']['path']).read_text().splitlines() if x.startswith('{')]
require(len([a for a in boot_artifacts if a.get('reason')=='compiler-artifact' and a.get('executable')==boot['program'] and a.get('profile',{}).get('test') is True])==1,'actual bootstrap test producer exact')

host51=test_lines((E/'host-windows-adapter-tests-first.stdout.log').read_text().splitlines(),51,'Host fc-windows')
hostcommand=load('host-startup-feedback-first.command.json');command(hostcommand);host1=test_lines((E/'host-startup-feedback-first.stdout.log').read_text().splitlines(),1,'Host bootstrap classifier')
ci=fact(W/'scripts/windows-msvc-ci.ps1');require(ci['sha256']=='eac2e9e42cfd073aca58584e8dbc6d9cefa864a5b84711b27e0ab83d43689a7b','final CI is eac second source, not43 first')
require(ci['sha256']==fact(E/'bootstrap-ci-after-second/scripts/windows-msvc-ci.ps1')['sha256'],'CI immutable second exact')
ps_rows=[]
for name,count,assertions,checker in [('gui-pure-powershell-second.json',9,181,'scripts/check-msvc-gui-contract.ps1'),('inventory-pure-powershell-second.json',10,20081,'scripts/check-ci-source-inventory.ps1')]:
 r=load(name);p=r['originalPowershellReport'];require(r['exitCode']==0 and r['actualSshExit'] is True and r['sourceUnchanged'] is True,'actual PS5.1 memory execution exit '+name)
 require(r['sourceSha256']==p['sourceSha256']==p['sourceExpectedSha256']==ci['sha256'] and r['checkerSha256']==fact(W/checker)['sha256'],'PS exact second source/checker binding '+name)
 for stream in ['stdout','stderr','remote-input.ps1']:fact(r[stream]['path'],r[stream])
 require(json.loads(Path(r['stdout']['path']).read_bytes())==p and len(p['checks'])==count and p['assertions']==assertions and p['fullSourceParseErrors']==0 and p['fatal'] is None and all(x['status']=='PASS' for x in p['checks']),'actual PS groups/assertions/source report '+name)
 ps_rows.append({'report':name,'groups':count,'assertions':assertions,'powershell':p['powershell'],'sourceSha256':r['sourceSha256'],'scope':p['scope']})
ci_text=(W/'scripts/windows-msvc-ci.ps1').read_text()
require("$env:RUSTFLAGS = $productRustFlags" in ci_text and "} finally {\n        $env:RUSTFLAGS = $originalProductRustFlags" in ci_text and "Write-CrtFlagState 'after-product'" in ci_text,'second source sets product env and restores finally')
require("Get-ProductRustFlags $Baseline" in ci_text and 'product_rustflags =' in ci_text and 'rustflags_scope =' in ci_text,'second source metadata discloses product exception')
require("$Baseline) 1200" in ci_text and 'GUI2/product' not in '', 'original product1200sec retained')

counter=load('cargo-final-flag-order-counterexample-first.json');fact(counter['compilerLog']['path'],counter['compilerLog']);fact(counter['program']['path'],counter['program']);require(pe(counter['program']['path'])['subsystem']==3,'actual first toy remained CUI3 counterexample')
require(counter['ordering']['guiArgumentPosition']<counter['ordering']['environmentConsoleArgumentPosition'],'actual first compiler log positions place envCONSOLE after GUI')
firstlog=Path(counter['compilerLog']['path']).read_text();require(firstlog.find('subsystem,windows')<firstlog.find('subsystem,console'),'raw first toy linker argument order')
command(load('cargo-final-flag-order-first.command.json'))
fixed=load('cargo-product-environment-static-second.json');fixed_program=E.parent/'link-order-target-second/i686-pc-windows-gnu/release/fc-gui-link-order-scratch.exe';fact(fixed_program,{'bytes':fixed['bytes'],'sha256':fixed['programSha256']});require(pe(fixed_program)=={'subsystem':2,'subsystemVersion':[6,1]} and fixed['subsystem']==2 and fixed['version']==[6,1],'actual second toy GUI2/version6.1')
command(load('cargo-product-environment-second.command.json'))

vpath=W/'scripts/windows-probe.py';fact(vpath);validators=runpy.run_path(str(vpath));runtime_rows=[]
for name,expected,test_identity in [('gui-windows-affected-public-first.json',219,window_test['identity']),('gui-windows-bootstrap-tests-first.json',218,{'path':boot['program'],'bytes':boot['bytes'],'sha256':boot['sha256']})]:
 r=load(name);command(load(name.replace('.json','.command.json')))
 require(r['status']==r['cleanup']=='PASS' and r['exeSha256']==normal['identity']['sha256'],'actual runtime GUI normal/cleanup '+name)
 require(len(r['tests'])==1 and len(r['uploadedArtifacts'])==2,'actual one affected test upload '+name)
 t=r['tests'][0];require(t['exitCode']==0 and t['binarySha256']==test_identity['sha256'] and t['timeoutSeconds']==900,'actual affected test SHA/exit/budget '+name)
 require({x['sha256'] for x in r['uploadedArtifacts']}=={normal['identity']['sha256'],test_identity['sha256']},'actual uploaded normal and harness SHA '+name)
 require(r['selfCheck']['exitCode']==0 and r['native']['exitCode']==0,'actual self/native exit0 '+name)
 runtime_rows.append({'report':name,'test':test_lines(t['output'],expected,name),'platform':r['platform'],'harness':test_identity,'normal':normal['identity'],'cleanup':r['cleanup'],'nativeScope':r['native']['output']})
 if expected==219:
  require(r['system']['exitCode']==0 and any('console_window_present=false' in x for x in r['system']['output']),'actual GUI system console HWND absent observation')
  public=validators['validate_public_desktop_output'](r['native']['publicDesktopExitCode'],r['native']['publicDesktopOutput']);require(public['steps']==20 and public['elapsedMs']==r['publicDesktopElapsedMs'],'actual public20 ordered transcript')
vis=load('gui-windows-visual-first.json');command(load('gui-windows-visual-first.command.json'));require(vis['status']==vis['cleanup']=='PASS' and vis['exeSha256']==normal['identity']['sha256'],'actual visual same GUI normal cleanup')
visual=validators['validate_subject_visual_output'](vis['native']['subjectVisualExitCode'],vis['native']['subjectVisualOutput']);require(visual['steps']==22 and visual['elapsedMs']==vis['subjectVisualElapsedMs'],'actual visual22 ordered transcript')

diag=load('gui-diagnostics-first.json');command(load('gui-diagnostics-first.command.json'));fact(diag['remoteScript']['path'],diag['remoteScript']);fact(E/'gui-diagnostics-first.remote.stdout')
require(json.loads((E/'gui-diagnostics-first.remote.stdout').read_bytes())==diag['observed'],'diagnostic raw observation equals complete report')
require(diag['status']=='PASS_ACTUAL_GUI_INHERITED_PIPES_AND_EXIT_CODES' and diag['cleanup']=='PASS' and diag['inputUnchanged'] is True and diag['exeSha256']==normal['identity']['sha256'] and diag['perChildDeadlineMillis']==5000,'actual diagnostic GUI SHA/cleanup/originaldeadline')
ops=diag['observed']['operations'];require(len(ops)==2,'exact two GUI diagnostic modes')
for op,code in zip(ops,[0,1]):
 require(op['exitCode']==code and op['actualExit'] is True and op['timedOut'] is False and op['redirectedPipes'] is True and op['useShellExecute'] is False and op['createNoWindow'] is False,'actual inherited stdout/err and physical exit '+str(code))
require(ops[0]['argument']=='--version' and ops[0]['stdout'].startswith('FutureClassroom ') and not ops[0]['stderr'],'actual GUI version stdout/exit0')
require(ops[1]['argument']=='--future-classroom-invalid-launch-probe' and not ops[1]['stdout'] and ops[1]['stderr'].startswith('FutureClassroom: unknown arguments'),'actual GUI invalid stderr/exit1')

readonly=load('windows-readonly-final-first.json');baseline_path=M/readonly['baseline']['path'];bf=fact(baseline_path);require(bf['sha256']==readonly['baseline']['sha256'],'fresh readonly exact prior baseline')
baseline=json.loads(baseline_path.read_bytes());require(readonly['status']==readonly['integrityStatus']=='PASS' and readonly['ssh']['exitCode']==0 and readonly['authentication']=='PASS','actual fresh readonly terminal authentication/health')
for stream in ['stdout','stderr']:
 raw=base64.b64decode(readonly['ssh'][stream]['base64'],validate=True);require(len(raw)==readonly['ssh'][stream]['bytes'] and hashlib.sha256(raw).hexdigest()==readonly['ssh'][stream]['sha256'],'complete fresh readonly original SSH '+stream+' bytes')
require(readonly['sshd']=='Running' and readonly['publicAuthorizationUnchanged'] is True and readonly['projectRunUnchanged'] is True and readonly['sshPublicAuthorizationSha256']==baseline['sshPublicAuthorizationSha256'] and readonly['projectRunReadOnlySnapshot']==baseline['projectRunReadOnlySnapshot'],'actual SSH authorization and true Run32/64 unchanged')
require(readonly['ownedProcesses']==readonly['ownedTasks']==readonly['scratchDirectories']==[] and readonly['activityObserved'] is False,'fresh project residual counts zero')
require(readonly['persistentKeyMetadata']['contentsRead'] is False and readonly['persistentKeyMode']=='0o600' and stat.S_IMODE(Path(readonly['persistentKeyMetadata']['path']).stat().st_mode)==0o600,'persistent SSH key mode only observed not read')
plan=fact(W/'开发计划/22-自启动与系统变更核对.md');text=(W/'开发计划/22-自启动与系统变更核对.md').read_text();require('219项' in text and '218项' in text and '尚' not in '' and '这不是新产品MSVC链接或Win7运行通过' in text and '未打开可见提示' in text,'plan22 finite actual evidence and explicit missing gates')
# Recheck source bytes at the end; docs/CI changed independently of first compile.
for k,v in subset.items():require(fact(W/k)['sha256']==v['sha256'],'final431 input unchanged '+k)
changed_nonrust=[k for k,v in compile['sourceBefore'].items() if k not in subset and (fact(W/k)['sha256']!=v['sha256'])]
report.update(status='PASS_FINITE_R61_INTEGRATION_STATIC_AND_RECORDED_WIN11_ONLY',checks=checks,checkCount=len(checks),originalFileFacts=list(facts.values()),sourceBinding={'count':431,'sourceBase':compile['sourceBase'],'rustCargoConfigCurrentMatchesOriginalBeforeAfter':True,'firstWholeSnapshotCount':len(compile['sourceBefore']),'laterChangedOutsideArtifactDomain':changed_nonrust,'facts':source_facts},hostTests=[host51,host1],actualWindowsTests=runtime_rows,public20=public,visual22=visual,ciSecond={'sha256':ci['sha256'],'purePowershell':ps_rows,'models':'No artifact/linker proof from pure models; first model pass did not detect Cargo ordering counterexample.'},linkOrder={'firstActualCounterexample':counter,'secondActualToy':fixed,'limitation':'Scratch GNU linker only, not new product MSVC or target OS execution.'},diagnostics=diag,readonlySummary={'report':'windows-readonly-final-first.json','status':'PASS','sshd':'Running','authAndRunUnchanged':True,'projectProcessTaskDirectoryCounts':[0,0,0],'keyContentsRead':False},plan22=plan,limitations=['Actual execution only authorized Windows11Pro build26300 x64/WOW64; Win7/Win10/Seewo/login remain separate unvalidated gates.','Product GUI2 and two actual CUI3 harnesses are GNU artifacts; new Modern/Win7 MSVC exact-source builds remain NOT_RUN.','No manual/repair visible startup error dialog was opened; only pure argument/text boundaries tested. No real autostart/logon invocation performed.','console_window_present=false is a point-in-time absence of GetConsoleWindow HWND, not a universal visual proof or pseudoconsole guarantee.','Windows harness output is original normalized merged PowerShell lines, not separately retained native stdout/stderr bytes; raw controller output/log SHA verified.','Original first freezes, model passes and real ordering counterexample remain unchanged; later source is eac second version.'],consumers={'reviewerNoFurtherR61ReadsOrWritesAfterThisReport':True,'canReleaseForRootArchiveAfterExistingRootConsumersReturn':True,'ongoingConsumersCreatedByReviewer':[]},helper=fact(__file__))
OUT.write_bytes((json.dumps(report,ensure_ascii=False,indent=2)+'\n').encode());print(json.dumps({'path':str(OUT),'bytes':OUT.stat().st_size,'sha256':hashlib.sha256(OUT.read_bytes()).hexdigest(),'checks':len(checks),'actualWindowsPassed':[219,218],'hostPassed':[51,1]}))
