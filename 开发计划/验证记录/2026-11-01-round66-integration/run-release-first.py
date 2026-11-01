from pathlib import Path
import json,hashlib,subprocess,time,sys
r=Path('/tmp/fc-round66-integration');e=r/'evidence';w=Path('/tmp/fc-round63-installer-owner/worktree');main=Path('/workspace/共享/Eliora/未来教室计划');script=main/'scripts/windows-probe.py'
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
f=json.loads((e/'implementation-freeze-first.json').read_text())
for row in f['allRustCargoInputs']:
 assert sha(w/row['path'])==row['sha256']
 assert sha(main/row['path'])==row['sha256']
for name in ['release-clippy-first','release-tests-first','release-product-first']:
 d=json.loads((e/(name+'.json')).read_text());assert d['status']=='PASS' and d['sourceUnchanged'] and d['sourceFreezeSha256']==sha(e/'implementation-freeze-first.json')
def artifacts(p):
 rows=[]
 for line in p.read_text().splitlines():
  try:d=json.loads(line)
  except ValueError:continue
  if d.get('reason')=='compiler-artifact' and d.get('executable'):rows.append(d)
 return rows
rows=artifacts(e/'release-product-first.stdout.log');products=[x for x in rows if x['target']['name']=='fc-bootstrap' and not x['profile']['test']];assert len(products)==1;exe=Path(products[0]['executable'])
meta=e/'release-tests-first.stdout.log';tests=[x for x in artifacts(meta) if x['profile']['test']];assert len(tests)==21
record=e/'windows-release-operation-first.json';dest=e/'windows-release-first.json';assert not record.exists() and not dest.exists()
cmd=['python3','-B',str(script),'--host','elior@192.168.10.212','--identity','/config/.local/share/future-classroom-test-access/win11-192.168.10.212','--exe',str(exe),'--test-metadata',str(meta),'--test-timeout-seconds','900','--native-seconds','1','--system-host','--public-desktop','--report',str(dest)]
d={'status':'RUNNING','command':cmd,'sourceFreezeSha256':sha(e/'implementation-freeze-first.json'),'scriptSha256':sha(script),'metadataSha256':sha(meta),'normal':{'path':str(exe),'bytes':exe.stat().st_size,'sha256':sha(exe)},'tests':[{'path':x['executable'],'bytes':Path(x['executable']).stat().st_size,'sha256':sha(Path(x['executable']))} for x in tests],'scope':'R66 combined 13 changed files/all 813 frozen inputs; actual Win11/WOW64 release-profile GNU 21 harnesses and dedicated hidden public probe; visual and repair are separate operations; no actual production namespace, Win7/10 or MSI/broker claim'}
record.open('x').write(json.dumps(d,indent=2)+'\n');print('R66_WINDOWS_INTEGRATED_STARTED',flush=True);t=time.monotonic()
with (e/'windows-release-first.stdout.log').open('xb') as a,(e/'windows-release-first.stderr.log').open('xb') as b:ret=subprocess.run(cmd,stdout=a,stderr=b,cwd=main).returncode
d.update(status='PASS_COMMAND' if ret==0 else 'FAIL_COMMAND',exitCode=ret,elapsedSeconds=time.monotonic()-t,scriptUnchanged=sha(script)==d['scriptSha256'],sourceUnchanged=all(sha(w/row['path'])==row['sha256'] for row in f['allRustCargoInputs']))
if dest.exists():v=json.loads(dest.read_text());d.update(resultStatus=v['status'],cleanup=v['cleanup'])
for k in ['stdout','stderr']:
 p=e/('windows-release-first.'+k+'.log');d[k]={'bytes':p.stat().st_size,'sha256':sha(p)}
record.write_text(json.dumps(d,indent=2)+'\n');print(json.dumps({k:d.get(k) for k in ['status','exitCode','elapsedSeconds','resultStatus','cleanup','sourceUnchanged']}),flush=True);sys.exit(0 if ret==0 and d.get('cleanup')=='PASS' and d.get('resultStatus')=='PASS' and d['sourceUnchanged'] else 1)
