from pathlib import Path
import datetime,difflib,hashlib,json,subprocess
r=Path('/tmp/fc-round70-ordinary-transfer-stop');w=r/'worktree';e=r/'evidence'
paths=['crates/fc-bootstrap/src/application/ordinary_https_probe.rs','scripts/windows-ordinary-https-probe.py']
base='02332c3ec3e20a496f35213228f3ec141f63327a'
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=w,text=True).strip()==base
changed=subprocess.check_output(['git','diff','--name-only'],cwd=w,text=True).splitlines();assert set(changed)==set(paths)
sha=lambda b:hashlib.sha256(b).hexdigest()
rows=[];diff=[]
for rel in paths:
 before=(e/'source-before-first'/rel).read_bytes();assert before==subprocess.check_output(['git','show',base+':'+rel],cwd=w)
 after=(w/rel).read_bytes();out=e/'source-after-first'/rel;out.parent.mkdir(parents=True,exist_ok=True)
 with out.open('xb') as f:f.write(after)
 rows.append({'relativePath':rel,'before':{'path':str((e/'source-before-first'/rel).relative_to(e)),'bytes':len(before),'sha256':sha(before)},'after':{'path':str(out.relative_to(e)),'bytes':len(after),'sha256':sha(after)}})
 diff.extend(difflib.unified_diff(before.decode().splitlines(True),after.decode().splitlines(True),fromfile='base/'+rel,tofile='candidate/'+rel))
with (e/'source-minimal-first.diff').open('x') as f:f.writelines(diff)
checks=json.loads((e/'pure-contract-first.json').read_text());assert checks['status']=='PASS_PURE_CONTRACT_AND_FORMAT_ONLY' and checks['sourceUnchanged']
for x in rows:assert checks['source'][x['relativePath']]==x['after']['sha256']
v={'schema':1,'status':'TWO_PROBE_FILES_FROZEN_PURE_CONTRACT_PASS_RUST_AND_WINDOWS_NOT_RUN','atUtc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'baseCommit':base,'worktree':str(w),'sourceFileCount':2,'source':rows,'minimalDiff':{'path':'source-minimal-first.diff','bytes':(e/'source-minimal-first.diff').stat().st_size,'sha256':sha((e/'source-minimal-first.diff').read_bytes())},'pureContract':{'path':'pure-contract-first.json','sha256':sha((e/'pure-contract-first.json').read_bytes()),'actualResult':checks['actualPureResult']},'nativePythonOrderedSteps':31,'scope':['ordinary configured RuntimeOwner; actual independent TeacherAdmin native approval in both generations','First full material upload retained MaterialReady and second partial upload consumes real staging bytes before original native Disable','Both exact old stages removed at physical quiescence, original workers queryable, same original5s bound','Second generation first rejects old cookie401, then actual new native approval, both upload slots successfully reused before either retired; typed Append69001, exact retry/durable receipt and cancel','No download/reset/restore expansion; no production runtime changes'], 'originalBudgets':{'nativeWorkflowSeconds':120,'clientAbsoluteSeconds':30,'nativeStopSeconds':5,'workerCount':2},'secrets':'Actual passwords/code/pollSecret/cookie/CSRF remain memory/owned transient sidechannel only; no credential-bearing body or digest added to archived transcript/report. Runtime artifacts still require root secret scan.', 'noChanges':['main','R68 source/evidence','production runtime gate/capacity','trust/firewall/HTTP.sys/SSH configuration'],'validationNotPerformed':['Cargo compile/link','Windows/SSH/TLS operation','actual31 step material stop/permit reuse','actual independent staging and cleanup receipt'],'nextOwner':'root builds normal GUI and performs architecture/strictclippy/format, source+binary guard, webbundle, then serialized actual ordinary31 workflow.'}
with (e/'implementation-freeze-first.json').open('x') as f:json.dump(v,f,ensure_ascii=False,indent=2);f.write('\n')
print(json.dumps({'files':2,'freezeSha256':sha((e/'implementation-freeze-first.json').read_bytes()),'diffBytes':v['minimalDiff']['bytes'],'after':{x['relativePath']:x['after']['sha256'] for x in rows}}))
