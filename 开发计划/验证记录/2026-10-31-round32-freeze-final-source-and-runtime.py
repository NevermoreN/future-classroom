from pathlib import Path
import json,hashlib,re,subprocess

root=Path.cwd();ev=Path('/tmp/fc-round32-deadline-evidence');out=ev/'final-source-and-runtime-binding.json';assert not out.exists()
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
build=json.loads((ev/'windows-affected-first.json').read_bytes());assert build['status']=='PASS' and not build['changedInputs']
for p,row in build['sourceBefore'].items():
    q=root/p;assert q.stat().st_size==row['bytes'] and sha(q)==row['sha256'],p
assert build['sourceBefore']==build['sourceAfter']
current_files=list((root/'crates').rglob('*.rs'))+list((root/'crates').rglob('Cargo.toml'))+list((root/'.cargo').rglob('*'))+[root/n for n in ['Cargo.toml','Cargo.lock','rust-toolchain.toml']]
assert {p.relative_to(root).as_posix() for p in current_files if p.is_file()}==set(build['sourceBefore'])
runtime=json.loads((ev/'windows-platform-tests-first.json').read_bytes());tls=json.loads((ev/'windows-tls-business-first.json').read_bytes());after=json.loads((ev/'windows-after.json').read_bytes())
assert runtime['status']=='PASS' and runtime['cleanup']=='PASS' and tls['status']=='PASS' and tls['cleanup']=='PASS' and after['status']=='PASS'
assert len(runtime['tests'])==1
test=runtime['tests'][0];assert test['exitCode']==0
summaries=[line for line in test['output'] if line.startswith('test result:')]
assert len(summaries)==1 and re.fullmatch(r'test result: ok\. 168 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in [0-9.]+s',summaries[0])
test_source=root/'crates/fc-windows/src/tls_frontdoor/native/stream/tests.rs'
before=(ev/'stream.tests.rs.before').read_text() if (ev/'stream.tests.rs.before').exists() else None
if before is None:
    candidates=[p for p in ev.iterdir() if p.name.endswith('tests.rs.before')]
    assert len(candidates)==1;before=candidates[0].read_text()
pattern=r'#\[test\]\s*fn\s+(\w+)\s*\('
new_names=sorted(set(re.findall(pattern,test_source.read_text()))-set(re.findall(pattern,before)));assert len(new_names)==6
for name in new_names:
    assert sum(line==f'test tls_frontdoor::native::stream::tests::{name} ... ok' for line in test['output'])==1,name
for row in build['artifacts']:
    p=Path(row['path']);assert sha(p)==row['sha256'] and p.stat().st_size==row['bytes']
    if row['test']:assert test['binarySha256']==row['sha256']
    else:assert runtime['exeSha256']==tls['exeSha256']==row['sha256']
for p,digest in [('crates/fc-windows/src/tls_frontdoor/native/stream.rs','4ab35d0114a59aefa3353096db23a7d26c90e7898d02a73b1e9fb5622cd304fb'),('crates/fc-windows/src/tls_frontdoor/native/stream/tests.rs','2134353f8802dcaeb78334a17abd0ca79b13cb52b9579d267dd5892cd72159f4')]:assert sha(root/p)==digest
r={'status':'PASS_AFFECTED_SOURCE_AND_ACTUAL_WIN11_BINDING','scope':'One affected Windows crate 168 tests, normal GNU product and finite genuine Schannel API regression, all same frozen source and independently uploaded binary hashes. No formal modern MSVC, Win7/Win10/Seewo or strict TCP deadline certification.','source':build['sourceBefore'],'sourceFiles':len(build['sourceBefore']),'inventoryScope':'382 Rust/Cargo/build configuration files observed before and after every build stage and again here. Prior Round31 385-file all-crate inventory additionally included two README files and bootstrap minimal.docx fixture; those three are not claimed captured by this build snapshot.','gitHeadDuringBuild':build['gitHeadBefore'],'gitHeadAtBinding':subprocess.check_output(['git','rev-parse','HEAD']).decode().strip(),'commands':build['commands'],'newExactTestNames':new_names,'nativeTestSummary':summaries[0],'nativeTestSha256':test['binarySha256'],'normalExeSha256':runtime['exeSha256'],'artifacts':build['artifacts'],'runtimeReport':{'path':str(ev/'windows-platform-tests-first.json'),'sha256':sha(ev/'windows-platform-tests-first.json')},'tlsBusinessReport':{'path':str(ev/'windows-tls-business-first.json'),'sha256':sha(ev/'windows-tls-business-first.json')},'windowsAfter':{'path':str(ev/'windows-after.json'),'sha256':sha(ev/'windows-after.json')},'actualPlatform':runtime['platform'],'realWaitScope':'One exact-slot old AcceptEx-post deadline plus50ms sleep executed in native IOCP/fake records test; not an ESTABLISHED epoch or Schannel timing proof. Other absolute boundaries are injected, not real120s waiting.','strictFirstTcpDeadline':'Original Round13 FAIL unchanged; post+5 and continuous backlog unresolved; normal production listener disabled.','hostTests':'NOT_RERUN: only two cfg(windows) source files changed, no Host behavior change. Actual format/architecture checks ran.','priorMsvcScope':'CI c622 baseline and c7 voice-query feature source are distinct earlier revisions; neither validates this new Rust change.','cleanup':'Remote runner and audit PASS; local build cleanup follows packaging.'}
out.write_text(json.dumps(r,indent=2)+'\n');print(json.dumps({'status':r['status'],'sourceFiles':r['sourceFiles'],'tests':168,'newTests':new_names,'normalExeSha256':r['normalExeSha256']}),flush=True)
