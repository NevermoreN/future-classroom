#!/usr/bin/env python3
"""R70 source4 finite publication/R71 reorder review; no tests or remote I/O."""
from pathlib import Path
import ast,copy,datetime,hashlib,json,re
R=Path('/tmp/fc-round70-ordinary-transfer-stop');E=R/'evidence';W=R/'worktree'
sha=lambda d:hashlib.sha256(d).hexdigest()
def fact(p):
 d=p.read_bytes();return dict(path=str(p.relative_to(E)) if p.is_relative_to(E) else str(p),bytes=len(d),sha256=sha(d))
def main():
 freeze=E/'implementation-freeze-fourth.json';assert sha(freeze.read_bytes())=='e4ed7f5a8dbe971246fa91b037d3223b4dddcbc77fa3bff56ad721f596724e31'
 f=json.loads(freeze.read_bytes());sources=[]
 for row in f['source']:
  p=W/row['relativePath'];data=p.read_bytes();assert len(data)==row['after']['bytes'] and sha(data)==row['after']['sha256']
  assert data==(E/row['after']['path']).read_bytes();sources.append(dict(path=row['relativePath'],bytes=len(data),sha256=sha(data)))
 rel='scripts/windows-ordinary-https-probe.py';old=(E/'source-after-third'/rel).read_text();new=(W/rel).read_text()
 bt=ast.parse(old);at=ast.parse(new);bf={n.name:n for n in bt.body if isinstance(n,ast.FunctionDef)};af={n.name:n for n in at.body if isinstance(n,ast.FunctionDef)}
 assert set(bf)==set(af)
 changed={name for name in bf if ast.dump(bf[name])!=ast.dump(af[name])};assert changed=={'signal_business_done','self_test'}
 def without_script(node):
  node=copy.deepcopy(node);node.body=[n for n in node.body if not(isinstance(n,ast.Assign) and any(isinstance(t,ast.Name) and t.id=='script' for t in n.targets))];return node
 assert ast.dump(without_script(bf['signal_business_done']))==ast.dump(without_script(af['signal_business_done']))
 assert ast.dump(ast.Module(body=[n for n in bt.body if not isinstance(n,ast.FunctionDef)],type_ignores=[]))==ast.dump(ast.Module(body=[n for n in at.body if not isinstance(n,ast.FunctionDef)],type_ignores=[]))
 script_function=ast.get_source_segment(new,af['signal_business_done']);assert script_function
 # Compare the exact statement builder against the expected narrow atomic publish
 # edit, retaining all original ownership checks, value and deadline calls.
 old_script='''        + "$p=Join-Path $r "+b.ps_literal(f"ordinary-business-{cycle}.done")+";$s=[IO.File]::Open($p,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::None);"
        + "try{$v=[Text.Encoding]::ASCII.GetBytes("+b.ps_literal(value)+");$s.Write($v,0,$v.Length)}finally{$s.Dispose()};'OWNED_CLIENT_JOURNEY_DONE'")'''
 new_script='''        + "$p=Join-Path $r "+b.ps_literal(f"ordinary-business-{cycle}.done")+";$t=Join-Path $r "+b.ps_literal(f"ordinary-business-{cycle}.pending")+";"
        + "$s=[IO.File]::Open($t,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::None);"
        + "try{$v=[Text.Encoding]::ASCII.GetBytes("+b.ps_literal(value)+");$s.Write($v,0,$v.Length);$s.Flush($true)}finally{$s.Dispose()};"
        + "[IO.File]::Move($t,$p);'OWNED_CLIENT_JOURNEY_DONE'")'''
 old_function=ast.get_source_segment(old,bf['signal_business_done']);assert old_function.count(old_script)==1
 assert old_function.replace(old_script,new_script,1)==script_function
 native_rel='crates/fc-bootstrap/src/application/ordinary_https_probe.rs';native=(W/native_rel).read_bytes()
 assert sha(native)=='0a4f309afb7b76bbd94f557ae74768bcb9e4f5080c083e01e9d686505f02f36a'
 assert native==(E/'source-after-third'/native_rel).read_bytes()
 assert 'let bytes = fs::read(&path).map_err(|_| "owned completion read failed")?;' in native.decode()
 assert 'Err(_) => return Err("owned completion metadata failed".into()),' in native.decode()
 order=re.findall(r'"([a-z-]+)"',native.decode().split('const ORDER: [&str; 31] = [',1)[1].split('];',1)[0])
 steps=next(ast.literal_eval(n.value) for n in at.body if isinstance(n,ast.Assign) and any(isinstance(t,ast.Name) and t.id=='STEPS' for t in n.targets));assert len(order)==31 and tuple(order)==steps
 # Independently reconstruct the complete R71 block reorder from original bytes.
 r71=E/'round71-source-order';ff=json.loads((r71/'implementation-freeze-first.json').read_bytes());row=ff['source'][0]
 before=(r71/row['before']['path']).read_bytes();after=(r71/row['after']['path']).read_bytes()
 assert len(before)==len(after)==10617 and sha(before)==row['before']['sha256'] and sha(after)==row['after']['sha256']
 prefix,tail=before.split(b'#[cfg(test)]\nmod tests {',1);tests_and_sep,enum=tail.split(b'/// Sanitized startup result for an explicitly configured production listener.',1)
 tests=b'#[cfg(test)]\nmod tests {'+tests_and_sep
 assert tests.endswith(b'\n\n');tests=tests[:-1]
 enum=b'/// Sanitized startup result for an explicitly configured production listener.'+enum
 assert prefix+tests+b'\n'+enum==before and prefix+enum+b'\n'+tests==after
 assert after==(W/row['relativePath']).read_bytes()
 proof=json.loads((r71/'item-order-equivalence-first.json').read_bytes())
 assert proof['prefixSha256']==sha(prefix) and proof['testModuleOriginalBytesSha256']==sha(tests) and proof['enumDocumentationAttributesAndDefinitionBytesSha256']==sha(enum)
 joint=E/'joint-source-freeze-fourth.json';j=json.loads(joint.read_bytes());assert j['workingFileCount']==595 and len(j['files'])==3
 assert {x['path'] for x in j['files']}=={native_rel,rel,row['relativePath']}
 for item in j['files']:
  data=(W/item['path']).read_bytes();assert len(data)==item['bytes'] and sha(data)==item['sha256']
 qualification=E/'publication-first-failure-qualification-fourth.json';q=json.loads(qualification.read_bytes())
 assert sha(qualification.read_bytes())=='f3cbfd039d52e0016945b909e2654c271ad6865bd3b3ee7abc8d5050d766ac52'
 original=E/q['observedActualFirst']['report'];actual=json.loads(original.read_bytes())
 assert sha(original.read_bytes())==q['observedActualFirst']['sha256'] and actual['status']=='FAIL' and actual['cleanup']=='PASS'
 pure=E/'pure-contract-fourth.json';assert sha(pure.read_bytes())=='c5491cf5f26af219cc2a3280598d621b349dc3452b79e9114303ab5ed5071ce0'
 prior=E/'independent-probe-source-review-third.json';assert sha(prior.read_bytes())=='db7bb6d337eb5489c5a62228b2ed086ac32a202c01e4faa83e73e0b020db7691'
 report=dict(schemaVersion=1,status='PASS_FOURTH_FINITE_SOURCE_PUBLICATION_AND_ITEM_REORDER_ONLY',recordedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),
  freeze=fact(freeze),jointFreeze=fact(joint),jointFileCount=3,jointWorkingFileCount=595,source=sources,
  publicationReview=dict(exactBuilderTransformation=True,otherNonTestAstIdentical=True,
   sourceSequence=['owned-root marker and basename verification','same-root per-cycle pending/final sibling names','pending CreateNew/Write/FileShare.None','write full originalASCII value','Flush(true)','Dispose writer in finally','two-argument File.Move(pending,final) with no overwrite','exact completion reply required'],
   finalNotOpenedForWrite=True,noOverwrite=True,permissions='Both fixed filenames remain inside same existing protected owned root and inherit its ACL; no permission/trust/firewall change.',
   content='business_done_value and all its canonical ID/marker/generation/cycle bounds are byte/AST unchanged; no credential fields or credential digest added.',
   deadline='Same preexisting absolute deadline check and min(8,deadline-now) command timeout. No retry, exception suppression or refreshed budget introduced.',
   strictNativeRead='Native Rust is exact0a4f third bytes: non-NotFound metadata errors and every read error remain fatal; only not-yet-present final name is polled.'),
  r71Review=dict(status='PASS_INDEPENDENT_EXACT_BLOCK_RECONSTRUCTION_SOURCE_ONLY',path=row['relativePath'],beforeSha256=sha(before),afterSha256=sha(after),bytes=10617,
   prefixSha256=sha(prefix),originalTestModuleSha256=sha(tests),enumDocumentationAttributesDefinitionSha256=sha(enum),
   exactBefore='prefix+testModule+separator+enum',exactAfter='prefix+enum+separator+testModule',allItemBytesEqual=True,
   scope='Only enum documentation/derive/variants move before unchanged cfg(test) module. No test deletion, lint suppression, dependency, runtime logic or timing change. Static equivalence does not establish nightly Clippy or Win7 runtime success.'),
  inheritedReview=fact(prior),orderedSteps=31,pure192Original=fact(pure),pureTestsRerun=False,
  originalWindowsFailure=dict(identity=fact(original),status='FAIL',cleanup='PASS',qualification=fact(qualification),
   causeLimit='Original owned completion read failed has no captured OS errno. Non-atomic final-name exposure is a source defect fixed here, but neither errno32 nor sole first-run cause is proven. No remaining31-chain success inferred.'),
  findings=[],windows='NOT_RUN',cargo='NOT_RUN',network='NOT_RUN',sourceUnchanged=True,
  scope='Finite source4 admission review only; actual rebuilt product/strict lint/Windows31 flow, material stop/reuse, cleanup and credential scan remain root-owned validations. No Win7/10/campus acceptance claim.')
 for item in j['files']:assert sha((W/item['path']).read_bytes())==item['sha256']
 data=(json.dumps(report,ensure_ascii=False,indent=2)+'\n').encode();out=E/'independent-probe-source-review-fourth.json'
 with out.open('xb') as output:output.write(data)
 print(json.dumps(dict(status=report['status'],reportSha256=sha(data),orderedSteps=31,exactPublicationEdit=True,r71ExactBlockReconstruction=True,pureTestsRerun=False)))
if __name__=='__main__':main()
