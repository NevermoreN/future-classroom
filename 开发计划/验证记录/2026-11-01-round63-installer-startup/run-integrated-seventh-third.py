from pathlib import Path
import json,hashlib,subprocess,time,sys
r=Path('/tmp/fc-round63-installer-owner');e=r/'evidence';w=r/'worktree';main=Path('/workspace/共享/Eliora/未来教室计划');script=main/'scripts/windows-probe.py'
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
f=json.loads((e/'implementation-freeze-seventh.json').read_text())
for row in f['files']:assert sha(w/row['path'])==row['sha256']
for name in ['strict-clippy-seventh','windows-workspace-all-targets-test-compile-seventh','windows-product-build-seventh']:
 d=json.loads((e/(name+'.json')).read_text());assert d['status']=='PASS' and d['sourceUnchanged'] and d['sourceFreezeSha256']==sha(e/'implementation-freeze-seventh.json')
def artifacts(p):
 rows=[]
 for line in p.read_text().splitlines():
  try:d=json.loads(line)
  except ValueError:continue
  if d.get('reason')=='compiler-artifact' and d.get('executable'):rows.append(d)
 return rows
rows=artifacts(e/'windows-product-build-seventh.stdout.log');products=[x for x in rows if x['target']['name']=='fc-bootstrap' and not x['profile']['test']];assert len(products)==1;exe=Path(products[0]['executable'])
meta=e/'windows-workspace-all-targets-test-compile-seventh.stdout.log';tests=[x for x in artifacts(meta) if x['profile']['test']];assert len(tests)==21
record=e/'windows-integrated-seventh-operation-second.json';dest=e/'windows-integrated-seventh-second.json';assert not record.exists() and not dest.exists()
cmd=['python3','-B',str(script),'--host','elior@192.168.10.212','--identity','/config/.local/share/future-classroom-test-access/win11-192.168.10.212','--exe',str(exe),'--test-metadata',str(meta),'--test-timeout-seconds','900','--native-seconds','1','--system-host','--public-desktop','--report',str(dest)]
d={'status':'RUNNING','command':cmd,'sourceFreezeSha256':sha(e/'implementation-freeze-seventh.json'),'scriptSha256':sha(script),'metadataSha256':sha(meta),'normal':{'path':str(exe),'bytes':exe.stat().st_size,'sha256':sha(exe)},'tests':[{'path':x['executable'],'bytes':Path(x['executable']).stat().st_size,'sha256':sha(Path(x['executable']))} for x in tests],'scope':'Actual Win11/WOW64 GNU 21 harnesses and dedicated hidden public probe; visual and repair are separate operations; no actual production namespace, Win7/10 or MSI/broker claim'}
record.open('x').write(json.dumps(d,indent=2)+'\n');print('R63_WINDOWS_INTEGRATED_STARTED',flush=True);t=time.monotonic()
with (e/'windows-integrated-seventh-second.stdout.log').open('xb') as a,(e/'windows-integrated-seventh-second.stderr.log').open('xb') as b:ret=subprocess.run(cmd,stdout=a,stderr=b,cwd=main).returncode
d.update(status='PASS_COMMAND' if ret==0 else 'FAIL_COMMAND',exitCode=ret,elapsedSeconds=time.monotonic()-t,scriptUnchanged=sha(script)==d['scriptSha256'],sourceUnchanged=all(sha(w/row['path'])==row['sha256'] for row in f['files']))
if dest.exists():v=json.loads(dest.read_text());d.update(resultStatus=v['status'],cleanup=v['cleanup'])
for k in ['stdout','stderr']:
 p=e/('windows-integrated-seventh-second.'+k+'.log');d[k]={'bytes':p.stat().st_size,'sha256':sha(p)}
record.write_text(json.dumps(d,indent=2)+'\n');print(json.dumps({k:d.get(k) for k in ['status','exitCode','elapsedSeconds','resultStatus','cleanup','sourceUnchanged']}),flush=True);sys.exit(0 if ret==0 and d.get('cleanup')=='PASS' and d.get('resultStatus')=='PASS' and d['sourceUnchanged'] else 1)
