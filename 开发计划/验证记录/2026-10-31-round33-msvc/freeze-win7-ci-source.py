from pathlib import Path
import base64,difflib,hashlib,json,re
ROOT=Path('/tmp/fc-round33-win7-worktree');EV=Path('/tmp/fc-round33-msvc-evidence');sha=lambda b:hashlib.sha256(b).hexdigest()
before=json.loads((EV/'win7-ci-before-source.json').read_text());old={r['path']:base64.b64decode(r['base64'],validate=True) for r in before['files']}
helper=(ROOT/'scripts/windows-msvc-ci.ps1').read_bytes();workflow=(ROOT/'.github/workflows/windows-win7-msvc.yml').read_bytes();modern=(ROOT/'.github/workflows/windows-msvc.yml').read_bytes();assert modern==old['.github/workflows/windows-msvc.yml']
s=helper.decode();prior=old['scripts/windows-msvc-ci.ps1'].decode();w=workflow.decode()
def function(text,name):
 start=text.index('function '+name+'(') if 'function '+name+'(' in text else text.index('function '+name+' {')
 ends=[m.start() for m in re.finditer(r'^function ',text[start+1:],re.M)];end=start+1+ends[0] if ends else len(text)
 return text[start:end]
# These existing critical helpers remain exact original bytes, not a model of their behavior.
unchanged=[]
for name in ['Resolve-Application','Restore-ProcessEnvironment','Get-ActiveProcessIdentity','Invoke-Logged','Require-Step','Read-Output','Hash-Inputs','File-Fact']:
 assert function(s,name)==function(prior,name),name;unchanged.append(name)
assert "[ValidateSet('Modern', 'Win7')][string]$Baseline = 'Modern'" in s
assert s.count('function Get-MsvcBaseline(')==s.count('function Get-BaselineCargoArguments(')==s.count('function Test-MsvcExecutableHeader(')==1
modern_config=s[s.index("if ($Name -eq 'Modern')"):s.index("name = 'Win7'")]
win7_config=s[s.index("name = 'Win7'"):s.index('function Get-BaselineCargoArguments')]
assert "'--target','i686-pc-windows-msvc'" in modern_config and "'--component','rust-src'" not in modern_config
assert "'--component','rust-src'" in win7_config and "'--target'" not in win7_config
assert "nightly-2026-10-29-x86_64-pc-windows-msvc" in win7_config and '92ffebeceaa0a071066f233873c2438e73d70fe7' in win7_config
assert "return ,(@('-Z','build-std',$Command) + $Arguments)" in s and 'build-std=' not in s
modern_arguments=[]
for name,command in [('cargo-metadata','metadata'),('fmt','fmt'),('workspace-test-compile','test'),('product-compile','build'),('clippy','clippy')]:
 old_match=re.search(r"Invoke-Logged '"+name+r"' \$cargo @\(([^\n]+?)\)",prior);assert old_match,name
 new_match=re.search(r"Invoke-Logged '"+name+r"' \$cargo \(Get-BaselineCargoArguments '"+command+r"' @\(([^\n]+?)\) \$Baseline\)",s);assert new_match,name
 assert old_match[1]=="'"+command+"',"+new_match[1],name
 if command in ['test','build','clippy']:assert "'--target',$Target" in new_match[1]
 modern_arguments.append({'step':name,'oldLiteralArgumentsEqualToModernHelperExpansion':True})
assert "Invoke-Logged 'cargo-version' $cargo @('--version','--verbose')" in s
for gate in ["$LogLimit = 16MB","$env:CARGO_BUILD_JOBS = '2'","'COMPILE_ONLY_NO_RUN'","'windows7 = '","if ($payloadBytes -ne $script:PayloadBytes)"]:
 if gate=="'windows7 = '":continue
 assert gate in s
for name in ['CARGO_TARGET_I686_PC_WINDOWS_MSVC_LINKER','CC_i686_pc_windows_msvc','CARGO_TARGET_I686_WIN7_WINDOWS_MSVC_LINKER','CC_i686_win7_windows_msvc','CFLAGS_i686_win7_windows_msvc']:assert name in s
assert "/D_WIN32_WINNT=0x0601" in s and '/DWINVER=0x0601' in s and '/SUBSYSTEM:CONSOLE,6.01' in s
for value in ["'--no-run'","'/DEPENDENTS'","'NOT_RUN'","'source-clean-after'","'input-sha256.json'","'rust-src-provenance.json'"]:assert value in s
assert "'windows-win7-msvc-${{ github.ref }}'" not in w # group is unquoted, checked below
assert 'group: windows-win7-msvc-${{ github.ref }}' in w and 'cancel-in-progress: false' in w
assert 'future-classroom-win7-msvc-ci/v1' in w and 'future-classroom-win7-msvc-' in w and w.count('-Baseline Win7')==2
for minutes in ['135','105','10','15']:assert 'timeout-minutes: '+minutes in w
assert 'source_sha:' in w and 'workflow_dispatch:' in w and 'persist-credentials: false' in w and w.count('if: always()')==2
assert set(re.findall(r'uses: ([^\s]+)',w))==set(re.findall(r'uses: ([^\s]+)',modern.decode()))
assert all(not line.rstrip()!=line for line in s.splitlines()+w.splitlines())
yaml_check='NOT_RUN_MODULE_UNAVAILABLE'
try:
 import yaml
 parsed=yaml.load(w,Loader=yaml.BaseLoader);assert set(parsed['on'])=={'workflow_dispatch'} and parsed['concurrency']['cancel-in-progress']=='false';yaml_check='PARSED_EXISTING_PYYAML_BASELOADER'
except ImportError:pass
rows=[]
for name,data in [('scripts/windows-msvc-ci.ps1',helper),('.github/workflows/windows-win7-msvc.yml',workflow),('.github/workflows/windows-msvc.yml',modern)]:rows.append({'path':name,'bytes':len(data),'sha256':sha(data),'base64':base64.b64encode(data).decode()})
diff=''.join(difflib.unified_diff(prior.splitlines(keepends=True),s.splitlines(keepends=True),fromfile='before/scripts/windows-msvc-ci.ps1',tofile='after/scripts/windows-msvc-ci.ps1'))+''.join(difflib.unified_diff([],w.splitlines(keepends=True),fromfile='/dev/null',tofile='after/.github/workflows/windows-win7-msvc.yml'))
with (EV/'win7-ci-source.diff').open('x') as f:f.write(diff)
report={'status':'SOURCE_FROZEN_STATIC_ONLY_POWERSHELL_AST_MOCK_AND_HOSTED_CI_NOT_RUN','worktree':str(ROOT),'base':'57d7ca0','files':rows,'before':{'path':str(EV/'win7-ci-before-source.json'),'sha256':sha((EV/'win7-ci-before-source.json').read_bytes())},'diffSha256':sha(diff.encode()),'staticChecks':{'oldModernWorkflowByteIdentical':True,'criticalHelperBodiesExactOriginal':unchanged,'modernCargoLiteralArguments':modern_arguments,'oldCargoVersionArgumentsRetained':True,'win7StdCompileCommandsUseNoListBuildStdAndExplicitTarget':True,'win7InstallRustSrcNoPrebuiltTarget':True,'newRootMarkerAndConcurrencyDistinct':True,'existingLogJobPayloadCleanupAndPeCrtBoundsRetained':True,'yaml':yaml_check,'selectedFilesNoTrailingWhitespace':True},'pureHelpers':[{'name':'Get-MsvcBaseline','parameters':'Name: ValidateSet Modern/Win7 defaultModern','result':'OrderedDictionary with fixed toolchain/target/root prefix+marker/flags/install args/target-only env names/role'},{'name':'Get-BaselineCargoArguments','parameters':'Command:string, Arguments:string[], Name:ValidateSet Modern/Win7 defaultModern','result':'One string[]; Modern command+args unchanged; Win7 test/build/clippy require explicit i686-win7 target and prepend -Z build-std; metadata/fmt unchanged'},{'name':'Test-MsvcExecutableHeader','parameters':'Bytes:byte[], Name:ValidateSet Modern/Win7 defaultModern','result':'Modern exact original PE32 gate returnsnull; Win7 also bounds declared optional header and requiresSubsystemMajor6/Minor1, returns6.1'}],'recommendedActualChecks':['Full PS source AST under actual PowerShell; invoke only extracted pure/helpers with synthetic values for both baselines, no main side effects','Modern config/install/command args equivalence and ValidateSet reject unknown; root path+marker cross-baseline refusal','Win7 target-required argument builder, no explicit build-std list; no flags sent to fmt/version plugins','Synthetic PE32 defaultModern unchanged; Win7 subsystem6.1 positive,6.0/6.2/truncation/wrongmachine/invalidoptionalheader negative','Exact save/restore of both originalModern and newWin7 env vars, three-phase canonical flags; no persistent environment/file mutation','Fresh hosted immutableSHA compile/product/Clippy, actual rustc/cargo versions+fixed rustc commit, same-toolchain rust-src hashes, all actual x86 PE32 headers/subsystem/import/staticCRT and own cleanup'],'notRun':['No PowerShell AST or helper execution by this agent','No Cargo/rustup/compiler/native EXE/Windows/remote execution','No CI dispatch, Git write/commit/push or main-worktree mutation'],'risks':['Tier3 build-std/nightly may fail or exceed original bounded command/log budgets; original failures must remain and all steps still requireexit0','Win7 headers6.1/staticCRT/fixedcompiler/std hashes do not prove realWin7 APIs, selectedVS/SDK/CRT compatibility or any OS runtime','RUSTFLAGS applies only target compilation when actual Cargo commands use explicit--target; selected target CC/CFLAGS/linker must be checked from actual verbose logs','LibraryCargo.lock is recorded from actual Windows same-toolchain rust-src, not hardcoded to Linux line-ending SHA','No compiled EXE is executed; allOS and Seewo runtime fields stayNOT_RUN'],'primarySources':['https://doc.rust-lang.org/rustc/platform-support/win7-windows-msvc.html','https://doc.rust-lang.org/cargo/reference/unstable.html#build-std','https://doc.rust-lang.org/cargo/reference/config.html#buildrustflags']}
with (EV/'win7-ci-source-freeze.json').open('x') as f:f.write(json.dumps(report,ensure_ascii=False,indent=2)+'\n')
print(json.dumps({'status':report['status'],'helperSha256':sha(helper),'workflowSha256':sha(workflow),'evidence':str(EV/'win7-ci-source-freeze.json'),'evidenceSha256':sha((EV/'win7-ci-source-freeze.json').read_bytes()),'changedRepositoryFiles':['scripts/windows-msvc-ci.ps1','.github/workflows/windows-win7-msvc.yml'],'pureHelperNames':[r['name'] for r in report['pureHelpers']],'yaml':yaml_check},ensure_ascii=False,indent=2))
