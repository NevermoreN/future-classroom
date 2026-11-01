#!/usr/bin/env python3
"""Finite third-source tuple diff audit; no test replay, compiler or remote I/O."""
from pathlib import Path
import ast,datetime,hashlib,json,re
R=Path('/tmp/fc-round70-ordinary-transfer-stop');E=R/'evidence';W=R/'worktree'
sha=lambda d:hashlib.sha256(d).hexdigest()
def main():
 fp=E/'implementation-freeze-third.json';assert sha(fp.read_bytes())=='9a13b3ea4666013e6aeac69f15b78611c4170b663248ed9301ededdc55955221'
 frozen=json.loads(fp.read_bytes());facts=[]
 for row in frozen['source']:
  data=(W/row['relativePath']).read_bytes();assert len(data)==row['after']['bytes'] and sha(data)==row['after']['sha256']
  assert data==(E/row['after']['path']).read_bytes()
  facts.append(dict(path=row['relativePath'],bytes=len(data),sha256=sha(data)))
 rel='crates/fc-bootstrap/src/application/ordinary_https_probe.rs'
 previous=(E/'source-after-second'/rel).read_text();current=(W/rel).read_text();expected=previous
 old='''    cycle: u8,
    generation: u64,
    port: u16,
    steps: &mut usize,
    deadline: Instant,
) -> Check<()> {
    submit(app, web_editor::REFRESH, &[], deadline)?;'''
 new='''    cycle_generation: (u8, u64),
    port: u16,
    steps: &mut usize,
    deadline: Instant,
) -> Check<()> {
    let (cycle, generation) = cycle_generation;
    submit(app, web_editor::REFRESH, &[], deadline)?;'''
 assert expected.count(old)==1;expected=expected.replace(old,new,1)
 old='    pair(app, package_root, marker, 1, first, port, steps, deadline)?;'
 new='    pair(app, package_root, marker, (1, first), port, steps, deadline)?;'
 assert expected.count(old)==1;expected=expected.replace(old,new,1)
 old='    pair(app, package_root, marker, 2, second, port, steps, deadline)?;'
 new='''    pair(
        app,
        package_root,
        marker,
        (2, second),
        port,
        steps,
        deadline,
    )?;'''
 assert expected.count(old)==1;expected=expected.replace(old,new,1)
 assert expected==current,'third Rust contains changes beyond exact reviewed signature/destructure/two calls'
 # Immediate tuple destructure restores exactly the old local names and types.
 signature=current.split('fn pair(',1)[1].split(') -> Check<()> {',1)[0]
 assert signature.count('cycle_generation: (u8, u64)')==1
 assert re.search(r'fn pair\([\s\S]*?\) -> Check<\(\)> \{\s*let \(cycle, generation\) = cycle_generation;',current)
 python_rel='scripts/windows-ordinary-https-probe.py';py=(W/python_rel).read_bytes()
 assert sha(py)=='64c3f4f7154949cd8a870c8984fc63954382e847237bf287a076bba79e30b8b1'
 assert py==(E/'source-after-second'/python_rel).read_bytes()
 tree=ast.parse(py.decode());steps=next(ast.literal_eval(n.value) for n in tree.body if isinstance(n,ast.Assign) and any(isinstance(t,ast.Name) and t.id=='STEPS' for t in n.targets))
 native_order=re.findall(r'"([a-z-]+)"',current.split('const ORDER: [&str; 31] = [',1)[1].split('];',1)[0])
 assert len(native_order)==31 and tuple(native_order)==steps
 second=E/'independent-probe-source-review-second.json';assert sha(second.read_bytes())=='59b3306afb72fe98acd205e2ea365c1c6d6d810b5e7502818a550c6024ca9577'
 previous_review=json.loads(second.read_bytes());assert previous_review['status']=='PASS_SECOND_SOURCE_P1_CORRECTED_PURE_ONLY_WINDOWS_NOT_RUN'
 diff=E/'second-to-third-minimal.diff';assert sha(diff.read_bytes())=='6b6779a187ec528348c439eae8b5a0df6322188e23488d1120d10b6b6bddb0cb'
 joint=E/'joint-source-freeze-third.json';jointvalue=json.loads(joint.read_bytes());assert jointvalue['workingFileCount']==595
 for fact in facts:assert sha((W/fact['path']).read_bytes())==fact['sha256']
 report=dict(schemaVersion=1,status='PASS_THIRD_FINITE_SOURCE_DIFF_ONLY_WINDOWS_NOT_RUN',recordedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(),
  implementationFreeze=dict(path=fp.name,sha256=sha(fp.read_bytes())),jointFreeze=dict(path=joint.name,sha256=sha(joint.read_bytes()),workingFileCount=595),source=facts,sourceUnchanged=True,
  diff=dict(path=diff.name,sha256=sha(diff.read_bytes()),exactReconstructionMatches=True,
   onlyChanges=['pair accepts cycle_generation:(u8,u64) instead of separate cycle:u8/generation:u64','pair immediately destructures tuple into original cycle and generation locals','first call passes exact(1,first)','second call passes exact(2,second), rustfmt-expanded only']),
  pairArgumentSemantics=dict(first=dict(cycle=1,generationExpression='first'),second=dict(cycle=2,generationExpression='second'),
   otherArgumentsUnchanged=['app','package_root','marker','port','steps','deadline'],bodyUnchangedAfterImmediateDestructure=True,noLintSuppressionAdded=True),
  contracts=dict(nativePythonOrderedSteps=31,pythonExactSecondBytes=True,originalBudgetAndSecretLogicUnchanged=True,originalStopOriginAndPoolReuseUnchanged=True),
  inheritedSecondReview=dict(path=second.name,sha256=sha(second.read_bytes()),status=previous_review['status'],
   pureChecks=previous_review['pureReplay']['actualResult']['checks'],actualProducerPositiveCount=previous_review['actualProducerNoIoReview']['positiveCount'],actualProducerNegativeCount=previous_review['actualProducerNoIoReview']['negativeCount'],
   qualification='Second188 pure checks and12 actual-producer boundary positives/22 rejection cases are inherited by exact unchanged Python bytes; none rerun or relabeled as third runtime proof.'),
  findings=[],validationNotPerformed=['Cargo','Clippy rerun','compiler/linker','Windows/SSH','actual31-step TLS/material stop/reuse','network'],
  scope='Finite exact-source third revision admission review only. Original source2 Clippy failure and source1 ID-encoding defect stay retained as original evidence. Root must verify rebuilt binary identity, strict Clippy and actual terminal Windows workflow/cleanup. No Win7/10 OS or campus acceptance claim.')
 data=(json.dumps(report,ensure_ascii=False,indent=2)+'\n').encode();path=E/'independent-probe-source-review-third.json'
 with path.open('xb') as output:output.write(data)
 print(json.dumps(dict(status=report['status'],reportSha256=sha(data),exactRustReconstruction=True,pythonUnchanged=True,orderedSteps=31,testsRerun=False)))
if __name__=='__main__':main()
