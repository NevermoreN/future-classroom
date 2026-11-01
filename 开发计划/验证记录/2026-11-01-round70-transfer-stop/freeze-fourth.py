from pathlib import Path
import datetime,difflib,hashlib,json
r=Path('/tmp/fc-round70-ordinary-transfer-stop');w=r/'worktree';e=r/'evidence';prior=e/'implementation-freeze-third.json';d=json.loads(prior.read_text());sha=lambda b:hashlib.sha256(b).hexdigest();rows=[];delta=[];diff=[]
for old in d['source']:
 rel=old['relativePath'];before=(e/old['after']['path']).read_bytes();after=(w/rel).read_bytes();out=e/'source-after-fourth'/rel;out.parent.mkdir(parents=True,exist_ok=True)
 with out.open('xb') as f:f.write(after)
 rows.append({'relativePath':rel,'before':old['before'],'after':{'path':str(out.relative_to(e)),'bytes':len(after),'sha256':sha(after)}})
 if before!=after:
  delta.append(rel);diff.extend(difflib.unified_diff(before.decode().splitlines(True),after.decode().splitlines(True),fromfile='third/'+rel,tofile='fourth/'+rel))
assert delta==['scripts/windows-ordinary-https-probe.py']
assert rows[0]['after']['sha256']=='0a4f309afb7b76bbd94f557ae74768bcb9e4f5080c083e01e9d686505f02f36a'
with (e/'third-to-fourth-minimal.diff').open('x') as f:f.writelines(diff)
check=e/'pure-contract-fourth.json';c=json.loads(check.read_text());assert c['status']=='PASS_PURE_CONTRACT_AND_FORMAT_ONLY' and c['actualPureResult']['checks']==192
for x in rows:assert c['source'][x['relativePath']]==x['after']['sha256']
qual=e/'publication-first-failure-qualification-fourth.json'
v={**{k:d[k] for k in ['schema','baseCommit','worktree','sourceFileCount','scope','originalBudgets','secrets','noChanges','nextOwner','originalP1']},'status':'ATOMIC_COMPLETION_PUBLICATION_FIX_FROZEN_WINDOWS_NOT_RUN','atUtc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'parentFreeze':{'path':prior.name,'sha256':sha(prior.read_bytes())},'source':rows,'onlyModifiedSinceThird':delta,'minimalDiff':{'path':'third-to-fourth-minimal.diff','bytes':(e/'third-to-fourth-minimal.diff').stat().st_size,'sha256':sha((e/'third-to-fourth-minimal.diff').read_bytes())},'pureContract':{'path':check.name,'sha256':sha(check.read_bytes()),'actualResult':c['actualPureResult']},'publicationChange':'Same owned-root per-cycle pending file uses CreateNew+exclusive writer; complete write and Flush(true), Dispose, then File.Move(pending,final) with no overwrite. Final name is not opened for writing. No Rust read-error suppression.', 'actualFirstFailureQualification':{'path':qual.name,'sha256':sha(qual.read_bytes()),'disposition':'Known source publication defect fixed; actual first read failure retained with OS error unavailable, not labeled sole proved sharing-violation cause.'},'nativePythonOrderedSteps':31,'validationNotPerformed':['Actual cached product binding for new whole-source guard','Actual Windows31 transfer workflow and cleanup after publication fix'],'oldEvidenceQualification':'Source3 actual first Windows run FAIL/native error and all raw originals remain. First completed materialA and consumed partialB evidence is historical, not proof of the remaining stop/reuse chain.'}
with (e/'implementation-freeze-fourth.json').open('x') as f:json.dump(v,f,ensure_ascii=False,indent=2);f.write('\n')
print(json.dumps({'freezeSha256':sha((e/'implementation-freeze-fourth.json').read_bytes()),'diffBytes':v['minimalDiff']['bytes'],'after':{x['relativePath']:x['after']['sha256'] for x in rows}}))
