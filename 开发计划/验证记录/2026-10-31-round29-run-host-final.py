from pathlib import Path
import subprocess, os, json, hashlib, re
p=Path('/tmp/fc-round29-evidence');env={**os.environ,'CARGO_BUILD_JOBS':'2'}
base=['cargo','test','--workspace','--release','--locked','--target-dir','/tmp/fc-round29-host']
def run(args,out,err=None):
 with (p/out).open('w') as o:
  if err:
   with (p/err).open('w') as e: result=subprocess.run(args,stdout=o,stderr=e,env=env)
  else: result=subprocess.run(args,stdout=o,stderr=subprocess.STDOUT,env=env)
 if result.returncode: raise SystemExit(result.returncode)
def artifacts(name):
 rows=[json.loads(l) for l in (p/name).read_text().splitlines()];items=[r for r in rows if r.get('reason')=='compiler-artifact' and r.get('profile',{}).get('test') and r.get('executable')]
 return [{'path':r['executable'],'target':r['target']['name'],'fresh':r['fresh'],'sha256':hashlib.sha256(Path(r['executable']).read_bytes()).hexdigest()} for r in items]
run(base+['--no-run','--message-format=json'],'host-test-artifacts-final.jsonl','host-build-final.log')
before=artifacts('host-test-artifacts-final.jsonl');assert len(before)==20
(p/'host-artifacts-before-full.json').write_text(json.dumps(before,indent=2)+'\n')
run(base+['--','--test-threads=2'],'host-full-final.log')
run(base+['--no-run','--message-format=json'],'host-test-artifacts-confirmed.jsonl','host-build-confirmed.log')
after=artifacts('host-test-artifacts-confirmed.jsonl');assert all(r['fresh'] for r in after)
assert {r['path']:r['sha256'] for r in before}=={r['path']:r['sha256'] for r in after}
counts=[tuple(map(int,x)) for x in re.findall(r'test result: ok\. (\d+) passed; (\d+) failed;', (p/'host-full-final.log').read_text())]
assert all(f==0 for _,f in counts)
(p/'host-artifact-binding-final.json').write_text(json.dumps({'status':'PASS','scope':'all20 exact pre-execution binaries match post-execution all-fresh artifacts','testExecutables':len(after),'passed':sum(n for n,_ in counts),'resultGroups':len(counts),'files':after},indent=2)+'\n')
print(json.dumps({'status':'PASS','executables':len(after),'passed':sum(n for n,_ in counts),'groups':len(counts)}))
