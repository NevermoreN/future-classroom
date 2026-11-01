from pathlib import Path
import datetime,difflib,hashlib,json
r=Path('/tmp/fc-round70-ordinary-transfer-stop');w=r/'worktree';e=r/'evidence';prior=e/'implementation-freeze-first.json';d=json.loads(prior.read_text())
sha=lambda b:hashlib.sha256(b).hexdigest()
rows=[];delta=[];diff=[]
for old in d['source']:
 rel=old['relativePath'];before=(e/old['after']['path']).read_bytes();after=(w/rel).read_bytes();out=e/'source-after-second'/rel;out.parent.mkdir(parents=True,exist_ok=True)
 with out.open('xb') as f:f.write(after)
 rows.append({'relativePath':rel,'before':old['before'],'after':{'path':str(out.relative_to(e)),'bytes':len(after),'sha256':sha(after)}})
 if before!=after:
  delta.append(rel);diff.extend(difflib.unified_diff(before.decode().splitlines(True),after.decode().splitlines(True),fromfile='first/'+rel,tofile='second/'+rel))
assert delta==['scripts/windows-ordinary-https-probe.py']
assert rows[0]['after']['sha256']=='73100e7f848cbca3c8a9416c9b0f27c78d65696637597016f6256c6dd9623736'
with (e/'first-to-second-minimal.diff').open('x') as f:f.writelines(diff)
check=e/'pure-contract-second.json';c=json.loads(check.read_text());assert c['status']=='PASS_PURE_CONTRACT_AND_FORMAT_ONLY' and c['actualPureResult']['checks']==188
for x in rows:assert c['source'][x['relativePath']]==x['after']['sha256']
p1=e/'business-done-encoding-review-second.json'
v={**{k:d[k] for k in ['schema','baseCommit','worktree','sourceFileCount','scope','originalBudgets','secrets','noChanges','nextOwner']},'status':'P1_COMPLETION_ID_FIX_FROZEN_PURE_CONTRACT_PASS_WINDOWS_NOT_RUN','atUtc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'parentFreeze':{'path':prior.name,'sha256':sha(prior.read_bytes())},'source':rows,'onlyModifiedSinceFirst':delta,'minimalDiff':{'path':'first-to-second-minimal.diff','bytes':(e/'first-to-second-minimal.diff').stat().st_size,'sha256':sha((e/'first-to-second-minimal.diff').read_bytes())},'pureContract':{'path':check.name,'sha256':sha(check.read_bytes()),'actualResult':c['actualPureResult']},'originalP1':{'report':p1.name,'sha256':sha(p1.read_bytes()),'disposition':'Source-fixed only; first actual normal build PASS remains valid for unchanged Rust but is not treated as a successful material workflow. Original str ID padding could generate wrong/overflow IDs. Now canonical nonzero bounded u64 strings are validated, converted to int, then formatted020d; four native-format roundtrip and eighteen malformed/overflow/bool/cycle rejection cases added.'},'nativePythonOrderedSteps':31,'validationNotPerformed':['Rust cached product identity for new whole source guard','Strict release Clippy','Windows/SSH real31-step TLS/material stop/reuse','Actual owned cleanup/secret scan']}
with (e/'implementation-freeze-second.json').open('x') as f:json.dump(v,f,ensure_ascii=False,indent=2);f.write('\n')
print(json.dumps({'freezeSha256':sha((e/'implementation-freeze-second.json').read_bytes()),'delta':delta,'diffBytes':v['minimalDiff']['bytes'],'after':{x['relativePath']:x['after']['sha256'] for x in rows}}))
