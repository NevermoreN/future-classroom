from pathlib import Path
import datetime,difflib,hashlib,json
r=Path('/tmp/fc-round70-ordinary-transfer-stop');w=r/'worktree';e=r/'evidence';prior=e/'implementation-freeze-second.json';d=json.loads(prior.read_text())
sha=lambda b:hashlib.sha256(b).hexdigest();rows=[];delta=[];diff=[]
for old in d['source']:
 rel=old['relativePath'];before=(e/old['after']['path']).read_bytes();after=(w/rel).read_bytes();out=e/'source-after-third'/rel;out.parent.mkdir(parents=True,exist_ok=True)
 with out.open('xb') as f:f.write(after)
 rows.append({'relativePath':rel,'before':old['before'],'after':{'path':str(out.relative_to(e)),'bytes':len(after),'sha256':sha(after)}})
 if before!=after:
  delta.append(rel);diff.extend(difflib.unified_diff(before.decode().splitlines(True),after.decode().splitlines(True),fromfile='second/'+rel,tofile='third/'+rel))
assert delta==['crates/fc-bootstrap/src/application/ordinary_https_probe.rs']
assert rows[1]['after']['sha256']=='64c3f4f7154949cd8a870c8984fc63954382e847237bf287a076bba79e30b8b1'
with (e/'second-to-third-minimal.diff').open('x') as f:f.writelines(diff)
check=e/'pure-contract-third.json';c=json.loads(check.read_text());assert c['status']=='PASS_PURE_CONTRACT_AND_FORMAT_ONLY' and c['actualPureResult']['checks']==188
for x in rows:assert c['source'][x['relativePath']]==x['after']['sha256']
v={**{k:d[k] for k in ['schema','baseCommit','worktree','sourceFileCount','scope','originalBudgets','secrets','noChanges','nextOwner','originalP1']},'status':'PAIR_PARAMETER_TUPLE_CLIPPY_FIX_FROZEN_NOT_BUILT_OR_WINDOWS_RUN','atUtc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'parentFreeze':{'path':prior.name,'sha256':sha(prior.read_bytes())},'source':rows,'onlyModifiedSinceSecond':delta,'minimalDiff':{'path':'second-to-third-minimal.diff','bytes':(e/'second-to-third-minimal.diff').stat().st_size,'sha256':sha((e/'second-to-third-minimal.diff').read_bytes())},'pureContract':{'path':check.name,'sha256':sha(check.read_bytes()),'actualResult':c['actualPureResult']},'clippyFix':'The pairing rendezvous cycle and its exact native generation are passed as one named cycle_generation:(u8,u64) argument then immediately destructured; seven total parameters. Both original values, lease, deadline and steps are unchanged. No lint suppression.', 'nativePythonOrderedSteps':31,'validationNotPerformed':['Actual rebuilt normal GUI binary identity','Actual strict release Clippy after source fix','Windows31 transfer workflow and owned cleanup'],'oldEvidenceQualification':'Original Clippy FAIL and successful source1/source2 product builds retained. Prior normal685261bb identity is not assigned to this source revision without an actual rebuilt artifact hash.'}
with (e/'implementation-freeze-third.json').open('x') as f:json.dump(v,f,ensure_ascii=False,indent=2);f.write('\n')
print(json.dumps({'freezeSha256':sha((e/'implementation-freeze-third.json').read_bytes()),'diffBytes':v['minimalDiff']['bytes'],'after':{x['relativePath']:x['after']['sha256'] for x in rows}}))
