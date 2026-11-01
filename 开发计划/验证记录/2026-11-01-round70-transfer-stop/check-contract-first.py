from pathlib import Path
import ast,datetime,hashlib,importlib.util,json,os,re,subprocess,sys,time
sys.dont_write_bytecode=True
root=Path('/tmp/fc-round70-ordinary-transfer-stop');w=root/'worktree';e=root/'evidence'
paths=['crates/fc-bootstrap/src/application/ordinary_https_probe.rs','scripts/windows-ordinary-https-probe.py']
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
before={p:sha(w/p) for p in paths}
cmd=['python3','-B','scripts/windows-ordinary-https-probe.py','--self-test'];started=time.monotonic()
r=subprocess.run(cmd,cwd=w,capture_output=True,timeout=60)
for name,b in [('stdout.log',r.stdout),('stderr.log',r.stderr)]:
 with (e/f'pure-contract-first.{name}').open('xb') as f:f.write(b)
require=lambda value,msg:None if value else (_ for _ in ()).throw(AssertionError(msg))
require(r.returncode==0,'pure contract command failed')
result=json.loads(r.stdout);require(result['status']=='PASS' and result['transferJourneyPositiveCases']==4 and result['transferNegativeCases']==72,'actual new positive/negative contracts missing')
spec=importlib.util.spec_from_file_location('r70_contract_verifier',w/paths[1]);m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
s=(w/paths[0]).read_text();order=re.findall(r'"([a-z-]+)"',s.split('const ORDER: [&str; 31] = [',1)[1].split('];',1)[0]);require(tuple(order)==m.STEPS and len(order)==31,'native/python31-step contract mismatch')
ast.parse((w/paths[1]).read_text());require('set_enabled(' not in s,'probe logical Web enable bypass')
require('Duration::from_secs(120)' in s and 'Duration::from_secs(5)' in s,'original overall/stop budget missing')
require('client_deadline=client_deadline or time.monotonic()+30' in (w/paths[1]).read_text(),'original client deadline changed')
fmt=subprocess.run(['rustfmt','--check','--edition','2021','--config','skip_children=true',paths[0]],cwd=w,capture_output=True,timeout=60)
for name,b in [('stdout.log',fmt.stdout),('stderr.log',fmt.stderr)]:
 with (e/f'rustfmt-first.{name}').open('xb') as f:f.write(b)
require(fmt.returncode==0,'native rustfmt check failed')
require(all(sha(w/p)==before[p] for p in paths),'sources changed during checks')
d={'status':'PASS_PURE_CONTRACT_AND_FORMAT_ONLY','atUtc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'command':cmd,'exitCode':r.returncode,'elapsedSeconds':time.monotonic()-started,'source':before,'sourceUnchanged':True,'actualPureResult':result,'nativePythonOrderedStepsMatch':31,'ast':'PASS','rustfmtExitCode':fmt.returncode,'originalBudgets':{'workflowSeconds':120,'clientSeconds':30,'stopSeconds':5,'stopOrigin':'one captured Instant immediately before native Confirm, no per-phase renewal'},'windowsInvoked':False,'networkInvoked':False,'cargoInvoked':False,'notProven':['Rust typecheck/codegen','actual MaterialReady/partial consumption and stop','actual second native pairing and two physical permit reuse','actual Win11 cleanup/Win7/Win10/campus']}
with (e/'pure-contract-first.json').open('x') as f:json.dump(d,f,ensure_ascii=False,indent=2);f.write('\n')
print(json.dumps({'status':d['status'],'sourceUnchanged':True,'checks':result['checks'],'transferNegativeCases':result['transferNegativeCases'],'reportSha256':sha(e/'pure-contract-first.json')}))
