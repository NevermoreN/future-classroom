from pathlib import Path
import subprocess,json,hashlib,os,re
p=Path('/tmp/fc-round29-evidence');env={**os.environ,'CARGO_BUILD_JOBS':'2'}
with (p/'host-test-artifacts-accepted.jsonl').open('w') as o,(p/'host-build-accepted.log').open('w') as e:
 r=subprocess.run(['cargo','test','--workspace','--release','--locked','--target-dir','/tmp/fc-round29-host','--no-run','--message-format=json'],stdout=o,stderr=e,env=env)
assert r.returncode==0
rows=[json.loads(x) for x in (p/'host-test-artifacts-accepted.jsonl').read_text().splitlines()];a=[x for x in rows if x.get('reason')=='compiler-artifact' and x.get('profile',{}).get('test') and x.get('executable')];assert len(a)==20
sha=lambda s:hashlib.sha256(Path(s).read_bytes()).hexdigest()
old=json.loads((p/'host-artifacts-before-full.json').read_text());oldmap={x['path']:x['sha256'] for x in old};final=[{'path':x['executable'],'target':x['target']['name'],'fresh':x['fresh'],'sha256':sha(x['executable'])} for x in a];assert set(oldmap)=={x['path'] for x in final}
changed=[x for x in final if x['sha256']!=oldmap[x['path']]];assert all(x['target']=='fc_protocol' for x in changed)
protocol=next(x for x in final if x['target']=='fc_protocol')
with (p/'host-protocol-exact-final.log').open('w') as o:r=subprocess.run([protocol['path'],'--test-threads=2'],stdout=o,stderr=subprocess.STDOUT)
assert r.returncode==0 and sha(protocol['path'])==protocol['sha256']
assert all(sha(x['path'])==x['sha256'] for x in final)
log=(p/'host-protocol-exact-final.log').read_text();assert re.search(r'test result: ok\. 69 passed; 0 failed;',log)
full=json.loads((p/'host-artifact-binding-stage1.json').read_text());report={'status':'PASS','scope':'all20 exact final binaries executed:19 unchanged programs from full791 run, final protocol69 re-executed after test-only lexical-scope Clippy fix; counts are unique coverage, not summed duplicate runs','testExecutables':20,'passed':full['passed'],'resultGroups':full['resultGroups'],'finalProtocolRetest':69,'replacedTestPrograms':[x['target'] for x in changed],'unchanged19ComparedToPreFullSha':True,'inputSha256':json.loads((p/'source-inputs-final.json').read_text())['inputSha256'],'files':final};(p/'host-artifact-binding-final.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({k:v for k,v in report.items() if k!='files'}))
