from pathlib import Path
import datetime,difflib,hashlib,json
r=Path('/tmp/fc-round70-ordinary-transfer-stop');w=r/'worktree';e=r/'evidence';prior=e/'implementation-freeze-fifth.json';d=json.loads(prior.read_text());sha=lambda b:hashlib.sha256(b).hexdigest();rows=[];delta=[];diff=[]
for old in d['source']:
 rel=old['relativePath'];before=(e/old['after']['path']).read_bytes();after=(w/rel).read_bytes();out=e/'source-after-sixth'/rel;out.parent.mkdir(parents=True,exist_ok=True)
 with out.open('xb') as f:f.write(after)
 rows.append({'relativePath':rel,'before':old['before'],'after':{'path':str(out.relative_to(e)),'bytes':len(after),'sha256':sha(after)}})
 if before!=after:
  delta.append(rel);diff.extend(difflib.unified_diff(before.decode().splitlines(True),after.decode().splitlines(True),fromfile='fifth/'+rel,tofile='sixth/'+rel))
assert delta==['scripts/windows-ordinary-https-probe.py'];assert rows[0]['after']['sha256']=='0a4f309afb7b76bbd94f557ae74768bcb9e4f5080c083e01e9d686505f02f36a'
with (e/'fifth-to-sixth-minimal.diff').open('x') as f:f.writelines(diff)
check=e/'pure-contract-sixth.json';c=json.loads(check.read_text());assert c['status']=='PASS_PURE_CONTRACT_AND_FORMAT_ONLY' and c['actualPureResult']['checks']==235
for x in rows:assert c['source'][x['relativePath']]==x['after']['sha256']
qual=e/'append-third400-source-review-sixth.json'
v={**{k:d[k] for k in ['schema','baseCommit','worktree','sourceFileCount','scope','originalBudgets','secrets','noChanges','nextOwner','originalP1','actualFirstFailureQualification','actualSecondFailureQualification','thirdFileOwnership']},'status':'CLOSED_ERROR_DTO_DIAGNOSTIC_FROZEN_WINDOWS_NOT_RUN','atUtc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'parentFreeze':{'path':prior.name,'sha256':sha(prior.read_bytes())},'source':rows,'onlyModifiedSinceFifth':delta,'minimalDiff':{'path':'fifth-to-sixth-minimal.diff','bytes':(e/'fifth-to-sixth-minimal.diff').stat().st_size,'sha256':sha((e/'fifth-to-sixth-minimal.diff').read_bytes())},'pureContract':{'path':check.name,'sha256':sha(check.read_bytes()),'actualResult':c['actualPureResult']},'change':'Unexpected response may disclose only exact ErrorDTO schemaVersion int1/code from the22-code protocol enum; extra/duplicate/unknown/noncanonical JSON rejected. No payload, expected200, two Ready slots, receipt check or production API change.','actualThirdFailureReview':{'path':qual.name,'sha256':sha(qual.read_bytes()),'disposition':'Actual400 captured safely, typed code not yet captured. Source supports the ASCII TXT and matching Append request; validation/publish IO collapse means root cause still not established. No inference from45-byte length.'},'nativePythonOrderedSteps':31,'validationNotPerformed':['Actual cached whole-source artifact guard','Actual next Windows ErrorDTO code or complete31 workflow','Actual final Append/typed receipt/cancel/second stop']}
with (e/'implementation-freeze-sixth.json').open('x') as f:json.dump(v,f,ensure_ascii=False,indent=2);f.write('\n')
print(json.dumps({'freezeSha256':sha((e/'implementation-freeze-sixth.json').read_bytes()),'diffBytes':v['minimalDiff']['bytes'],'after':{x['relativePath']:x['after']['sha256'] for x in rows}}))
