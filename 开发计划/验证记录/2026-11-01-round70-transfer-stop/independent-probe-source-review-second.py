#!/usr/bin/env python3
"""R70 second frozen-source review; actual rendezvous producer, no remote I/O."""
from pathlib import Path
import ast,copy,datetime,hashlib,importlib.util,json,re,subprocess,sys,time
sys.dont_write_bytecode=True
R=Path('/tmp/fc-round70-ordinary-transfer-stop');E=R/'evidence';W=R/'worktree'
sha=lambda b:hashlib.sha256(b).hexdigest()
def main():
 freeze=E/'implementation-freeze-second.json';assert sha(freeze.read_bytes())=='eb16a0bec8803fd7c39854a381b4f2c96ad07f92a809b708e35d4f2c99ef7802'
 joint=E/'joint-source-freeze-second.json';assert sha(joint.read_bytes())=='2407a98d2fc1eb60d3dca90f42e59fd6aba7b635ea526dbe7b15ad43515cbc7e'
 frozen=json.loads(freeze.read_bytes());facts=[]
 for row in frozen['source']:
  p=W/row['relativePath'];data=p.read_bytes();assert sha(data)==row['after']['sha256'] and len(data)==row['after']['bytes']
  facts.append(dict(path=row['relativePath'],bytes=len(data),sha256=sha(data)))
 runner=W/'scripts/windows-ordinary-https-probe.py';old=E/'source-after-first/scripts/windows-ordinary-https-probe.py'
 before=ast.parse(old.read_text());after=ast.parse(runner.read_text())
 old_functions={n.name:n for n in before.body if isinstance(n,ast.FunctionDef)}
 new_functions={n.name:n for n in after.body if isinstance(n,ast.FunctionDef)}
 assert set(new_functions)-set(old_functions)=={'business_done_value'} and not set(old_functions)-set(new_functions)
 changed={name for name in old_functions if ast.dump(old_functions[name])!=ast.dump(new_functions[name])}
 assert changed=={'signal_business_done','self_test'}
 # Confirm signal_business_done is actually wired to the newly validated numeric
 # producer and that its I/O, original deadline and owned-file rules are unchanged.
 expected=copy.deepcopy(old_functions['signal_business_done'])
 values=[n for n in expected.body if isinstance(n,ast.Assign) and any(isinstance(t,ast.Name) and t.id=='value' for t in n.targets)]
 assert len(values)==1;values[0].value=ast.parse('business_done_value(marker,generation,cycle,ids)',mode='eval').body
 assert ast.dump(expected)==ast.dump(new_functions['signal_business_done'])
 old_other=[n for n in before.body if not isinstance(n,ast.FunctionDef)]
 new_other=[n for n in after.body if not isinstance(n,ast.FunctionDef)]
 assert ast.dump(ast.Module(body=old_other,type_ignores=[]))==ast.dump(ast.Module(body=new_other,type_ignores=[]))
 command=['python3','-B',str(runner),'--self-test'];began=time.monotonic()
 result=subprocess.run(command,cwd=W,capture_output=True,timeout=60);streams={}
 for name,data in [('stdout',result.stdout),('stderr',result.stderr)]:
  p=E/('independent-probe-pure-second.'+name+'.log')
  with p.open('xb') as output:output.write(data)
  streams[name]=dict(path=p.name,bytes=len(data),sha256=sha(data))
 assert result.returncode==0;pure=json.loads(result.stdout)
 assert pure['status']=='PASS' and pure['checks']==188 and pure['completionIdPositiveCases']==4 and pure['completionIdNegativeCases']==18
 spec=importlib.util.spec_from_file_location('r70_second_independent_probe',runner);m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
 class NoIoRemote:
  def __init__(self):self.scripts=[]
  def powershell(self,script,timeout):self.scripts.append(script);return 'OWNED_CLIENT_JOURNEY_DONE'
 marker='FutureClassroom-Test-TLS-'+'0'*32;positive=[]
 for ids in [['1','2'],['9','10'],[str(2**64-2),str(2**64-1)]]:
  for generation in (1,2**64-1):
   for cycle in (1,2):
    fake=NoIoRemote();m.signal_business_done(fake,'C:\\synthetic',marker,generation,time.monotonic()+5,cycle,ids)
    assert len(fake.scripts)==1
    match=re.search(r"\[Text.Encoding\]::ASCII.GetBytes\('([^']*)'\)",fake.scripts[0],re.S);assert match
    text=match.group(1);fields=text.split('\n')
    assert len(fields)==7 and fields[:4]==['FC_ORDINARY_DONE_V2',marker,f'{generation:020d}',str(cycle)] and fields[6]==''
    assert len(text.encode('ascii'))<=256 and [int(s) for s in fields[4:6]]==[int(s) for s in ids]
    assert all(len(s)==20 and re.fullmatch('[0-9]{20}',s) and 0<int(s)<=2**64-1 and s==f'{int(s):020d}' for s in fields[4:6])
    assert text==m.business_done_value(marker,generation,cycle,ids)
    positive.append(dict(ids=ids,generation=str(generation),cycle=cycle,canonicalRoundtrip=True,actualProducerCalled=True))
 malformed=[['0','2'],['01','2'],[True,'2'],['1',2],['1','1'],['1',str(2**64)],['-1','2'],[' 1','2'],['1\n','2'],['1'],['1','2','3'],'12',('1','2'),['','2'],['１','2']]
 args=[(marker,17,1,ids) for ids in malformed]
 args += [(marker,True,1,['1','2']),(marker,0,1,['1','2']),(marker,2**64,1,['1','2']),
  (marker,17,True,['1','2']),(marker,17,3,['1','2']),(marker+'\n',17,1,['1','2'])]
 negative=0
 for mark,generation,cycle,ids in args:
  fake=NoIoRemote()
  try:m.signal_business_done(fake,'C:\\synthetic',mark,generation,time.monotonic()+5,cycle,ids)
  except RuntimeError:pass
  else:raise AssertionError('malformed input reached producer I/O')
  assert fake.scripts==[];negative+=1
 fake=NoIoRemote()
 try:m.signal_business_done(fake,'C:\\synthetic',marker,17,time.monotonic()-1,1,['1','2'])
 except RuntimeError:pass
 else:raise AssertionError('expired original deadline reached I/O')
 assert fake.scripts==[];negative+=1
 native=(W/'crates/fc-bootstrap/src/application/ordinary_https_probe.rs').read_text()
 order=re.findall(r'"([a-z-]+)"',native.split('const ORDER: [&str; 31] = [',1)[1].split('];',1)[0]);assert tuple(order)==m.STEPS
 for row in facts:assert sha((W/row['path']).read_bytes())==row['sha256']
 first=E/'independent-probe-source-review-first.json';assert sha(first.read_bytes())=='09defffe805656773c8c338e0aa5a6c76f8cba243e936bb05c24911aac2272a0'
 report=dict(schemaVersion=1,status='PASS_SECOND_SOURCE_P1_CORRECTED_PURE_ONLY_WINDOWS_NOT_RUN',recordedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),
  implementationFreeze=dict(path=freeze.name,sha256=sha(freeze.read_bytes())),jointFreeze=dict(path=joint.name,sha256=sha(joint.read_bytes())),source=facts,sourceUnchanged=True,
  pureReplay=dict(command=command,exitCode=result.returncode,elapsedSeconds=time.monotonic()-began,actualResult=pure,streams=streams),
  actualProducerNoIoReview=dict(positiveCount=len(positive),positiveCases=positive,negativeCount=negative,
   negativeScope='Malformed/zero/noncanonical/overflow/duplicate/nonlist/nonstring/bool/Unicode IDs; invalid marker/generation/cycle and expired original deadline. All rejected before remote adapter call.',
   nativeConsumerContract='Seven newline-separated fields; exact magic/marker; canonical20digit generation and each nonzero bounded u64 upload ID; exact cycle; distinct IDs; final newline; <=256 ASCII bytes.'),
  sourceDiffReview=dict(onlyChangedFunctions=sorted(changed),onlyAddedFunction='business_done_value',otherFunctionAstIdentical=True,otherModuleAstIdentical=True,
   actualSignalUsesNewProducer=True,signalOwnedFileAndDeadlineAstUnchangedApartFromValue=True,nativeRustIdenticalToFirstFreeze=True,orderedNativePythonSteps=31),
  firstFinding=dict(path=first.name,sha256=sha(first.read_bytes()),status='BLOCKED_P1_FROZEN_PROBE_RENDEZVOUS_ID_ENCODING',disposition='Corrected in second Python source. First frozen defect and first166-check result remain original historical evidence; no Windows31 success inferred.'),
  scope='Independent source/pure-only admission review. Original31 step contracts, first MaterialReady+partial socket lifetime, same stop5s origin, exact-stage removal, original2 shared workers, old cookie401, real second native pairing, C/D bothReady before cleanup and typed69001/retry/readback/cancel remain unchanged by AST comparison. Secrets remain memory/owned transient channels; new helper emits only canonical public identity/IDs.',
  findings=[],windows='NOT_RUN',cargo='NOT_RUN',network='NOT_RUN',
  limitations=['No Rust compile/link or actual TLS/material lifecycle/partial termination/stage cleanup/permit reuse proof is claimed. Root must run actual terminal workflow and cleanup.',
   'Neither Win7/10 OS nor classroom campus acceptance follows from this review.'])
 data=(json.dumps(report,ensure_ascii=False,indent=2)+'\n').encode();path=E/'independent-probe-source-review-second.json'
 with path.open('xb') as output:output.write(data)
 print(json.dumps(dict(status=report['status'],reportSha256=sha(data),pureChecks=pure['checks'],actualProducerPositiveCount=len(positive),actualProducerNegativeCount=negative,sourceUnchanged=True)))
if __name__=='__main__':main()
