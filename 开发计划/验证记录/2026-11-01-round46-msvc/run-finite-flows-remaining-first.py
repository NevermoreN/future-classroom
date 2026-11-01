from pathlib import Path
import json,subprocess,hashlib,sys,datetime
root=Path('/workspace/共享/Eliora/未来教室计划');ev=Path('/tmp/fc-round46-ci-evidence');out=ev/'finite-flows-remaining-first.json';assert not out.exists()
for tag in ('modern','win7'):
 d=json.loads((ev/(tag+'-msvc-runtime-second.json')).read_text());assert d['status']=='PASS' and d['cleanup']=='PASS'
zipfile=Path('/mnt/data/FutureClassroom-0.1.0-voice-capability-dev.zip');assert hashlib.sha256(zipfile.read_bytes()).hexdigest()=='5e280a84e726f0366db3e0ea1bd704158e26ac1b3fdb4e542f397b48fc1706c0'
r={'status':'RUNNING','startedUtc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'scope':'Actual strictly serial modern MSVC and Win7-target MSVC programs executed on Win11 only. Public20/visual22 each; modern finite raw HTTPS + continuous accept + defaultHOMEWORK Edge + voice capabilities. No new strict preconnect timing sample, no Win7/Win10 OS evidence.','operations':[]};out.write_text(json.dumps(r,indent=2)+'\n')
commands=[]
for tag in ('win7',):
 audit=json.loads((ev/(tag+'-original-independent-first.json')).read_text());exe=audit['normalExecutable']['localPath'];assert hashlib.sha256(Path(exe).read_bytes()).hexdigest()==audit['normalExecutable']['producer']['sha256']
 common=['python3','-B','scripts/windows-probe.py','--host','elior@192.168.10.212','--identity','/config/.local/share/future-classroom-test-access/win11-192.168.10.212','--exe',exe]
 for name,flag in [('public-native','--public-desktop'),('visual-native','--subject-visuals')]:
  title=tag+'-msvc-'+name+'-first';commands.append((title,common+[flag,'--report',str(ev/(title+'.json'))]))
 if tag=='modern':
  title='modern-msvc-default-browser-continuous-first'
  commands.append((title,['python3','-B','scripts/windows-tls-business-probe.py','--ssh-target','elior@192.168.10.212','--identity','/config/.local/share/future-classroom-test-access/win11-192.168.10.212','--exe',exe,'--web-bundle',str(zipfile),'--continuous-accept','--browser','--voice-capabilities','--report',str(ev/(title+'.json'))]))
commands.append(('windows-readonly-final-first',['python3','-B',str(ev/'audit-windows-readonly.py'),'--report',str(ev/'windows-readonly-final-first.json'),'--baseline',str(root/'开发计划/验证记录/2026-11-01-round42-ci/windows-after-first.json')]))
for name,cmd in commands:
 print('FINITE_FLOW_STARTED '+name,flush=True);c=subprocess.run(['python3','-B',str(ev/'run-recorded.py'),name,*cmd],cwd=root);r['operations'].append({'name':name,'exitCode':c.returncode});out.write_text(json.dumps(r,indent=2)+'\n')
 if c.returncode:r.update(status='FAIL',failedOperation=name);out.write_text(json.dumps(r,indent=2)+'\n');sys.exit(c.returncode)
r.update(status='PASS',completedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat());out.write_text(json.dumps(r,indent=2)+'\n');print('FINITE_FLOWS_ALL_TERMINAL_PASS',flush=True)
