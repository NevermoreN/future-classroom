"""Actual finite Win11 regression of final integrated R56/R58 sources only."""
from pathlib import Path
import hashlib,json,subprocess,time
ROOT=Path('/workspace/共享/Eliora/未来教室计划')
E=Path('/tmp/fc-round56-transactional-autorun-evidence')
T=Path('/tmp/fc-round56-transactional-autorun-target/stable/i686-pc-windows-gnu/release')
H=lambda b:hashlib.sha256(b).hexdigest()
F=lambda p:{'path':str(p),'bytes':p.stat().st_size,'sha256':H(p.read_bytes())}
binding=json.loads((E/'main-integration-first.json').read_text())
checks=json.loads((E/'integrated-checks-first.json').read_text())
assert checks['status'].startswith('PASS') and not checks['changedInputs']
assert checks['sourceBefore']==checks['sourceAfter']==binding['sourceInputs']
for rel,fact in binding['sourceInputs'].items():
 b=(ROOT/rel).read_bytes();assert {'bytes':len(b),'sha256':H(b)}==fact
for rel,fact in binding['r57ExtraInputs'].items():
 b=(ROOT/rel).read_bytes();assert {'bytes':len(b),'sha256':H(b)}==fact
raw=E/'integrated-windows-fc-windows-compile-first.stdout.log'
selected=[]
for line in raw.read_text().splitlines(keepends=True):
 item=json.loads(line)
 if item.get('reason')=='compiler-artifact' and item.get('profile',{}).get('test') and item.get('executable') and item['target']['name']=='fc_windows':selected.append((line,Path(item['executable'])))
assert len(selected)==1
metadata=E/'integrated-windows-selected-original-first.jsonl'
with metadata.open('x') as out:out.write(selected[0][0])
normal=T/'fc-bootstrap.exe'
product_raw=E/'integrated-windows-product-first.stdout.log'
products=[json.loads(line) for line in product_raw.read_text().splitlines()]
products=[row for row in products if row.get('reason')=='compiler-artifact' and row.get('executable') and row['target']['name']=='fc-bootstrap' and not row.get('profile',{}).get('test')]
assert len(products)==1 and Path(products[0]['executable'])==normal
script=ROOT/'scripts/windows-probe.py'
report=E/'integrated-windows-operation-first.json'
assert not report.exists()
record={'status':'RUNNING','scope':'Only affected fc_windows executable, public20/visual22/autostart25/default-output finite flows. Actual Win11/WOW64 only; no real Run, audible audio, visible picker, login/reboot, TLS/browser or Win7/Win10 certification.','sourceBinding':F(E/'main-integration-first.json'),'compileChecks':F(E/'integrated-checks-first.json'),'inputs':{'normal':F(normal),'test':F(selected[0][1]),'selectedOriginalMetadata':F(metadata),'fullOriginalMetadata':F(raw),'productOriginalMetadata':F(product_raw)},'script':F(script),'operations':[]}
def save():report.write_text(json.dumps(record,ensure_ascii=False,indent=2)+'\n')
save()
for label,flags in [('integrated-windows-affected-public-first',['--test-metadata',str(metadata),'--system-host','--public-desktop']),('integrated-windows-visual-first',['--subject-visuals']),('integrated-windows-autostart-first',['--autostart']),('integrated-windows-voice-output-first',['--voice-output'])]:
 destination=E/(label+'.json');assert not destination.exists()
 args=['python3','-B',str(script),'--host','elior@192.168.10.212','--identity','/config/.local/share/future-classroom-test-access/win11-192.168.10.212','--exe',str(normal),'--test-timeout-seconds','900','--native-seconds','1','--report',str(destination),*flags]
 row={'name':label,'argv':args,'status':'RUNNING'};record['operations'].append(row);save();print('WINDOWS_STARTED '+label,flush=True);started=time.monotonic()
 with (E/(label+'.stdout.log')).open('xb') as out,(E/(label+'.stderr.log')).open('xb') as err:
  child=subprocess.run(args,cwd=ROOT,stdout=out,stderr=err)
 result=json.loads(destination.read_text()) if destination.exists() else {}
 row.update(exitCode=child.returncode,actualExit=True,elapsedSeconds=time.monotonic()-started,result=F(destination) if destination.exists() else None,resultStatus=result.get('status'),cleanup=result.get('cleanup'),stdout=F(E/(label+'.stdout.log')),stderr=F(E/(label+'.stderr.log')))
 row['status']='PASS' if child.returncode==0 and result.get('status')=='PASS' and result.get('cleanup')=='PASS' else 'FAIL';save();print('WINDOWS_DONE '+label+' '+row['status'],flush=True)
 if row['status']!='PASS':record['status']='FAIL';save();raise SystemExit(1)
for name in ['normal','test']:
 p=Path(record['inputs'][name]['path']);assert F(p)==record['inputs'][name]
assert F(script)==record['script']
for rel,fact in binding['sourceInputs'].items():
 b=(ROOT/rel).read_bytes();assert {'bytes':len(b),'sha256':H(b)}==fact
record['status']='PASS_ACTUAL_WIN11_AFFECTED_AND4_NATIVE_FLOWS';save();print(record['status'],flush=True)
