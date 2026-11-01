from pathlib import Path
import json,hashlib,subprocess,datetime,sys
root=Path('/tmp/fc-round51-browser-diagnostics-worktree');ev=Path('/tmp/fc-round50-https-integration-evidence');ci=Path('/tmp/fc-round46-ci-evidence');diag=Path('/tmp/fc-round51-browser-diagnostics-evidence')
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
f=diag/'implementation-freeze-second.json';assert sha(f)=='38555fe4cdcab3bdb34dcafe41f8249aab94d36d7dbc199ae3a81bff3ae74530';freeze=json.loads(f.read_text())
for item in freeze['ownedFiles']:assert sha(root/item['path'])==item['sha256']
review=diag/'editor-review-second.json';assert sha(review)=='b73349a738af590f82e47924c7864327851f90e4c88679cac27b1e61389c2f5b'
reg=json.loads((ev/'windows-regressions-first.json').read_text());assert reg['status']=='PASS'
old=ci/'modern-artifact-original/programs/product/fc-bootstrap/fc-bootstrap.exe';new=Path('/tmp/fc-round50-https-programs-preliminary/fc-bootstrap.exe');assert sha(old)=='bc8f224a97797311e76182486bfa6502f8c11110e3a7151893f5212ff9d911aa';assert sha(new)=='b6368400a58b87d4725cbd664b5bde88a4f51927a74656e5c566010e8bbf6930'
out=ev/'final-browsers-first.json';assert not out.exists();r={'status':'RUNNING','startedUtc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'sourceFreeze':{'path':str(f),'sha256':sha(f)},'review':{'path':str(review),'sha256':sha(review)},'scope':'One final finite run per program after fixed diagnostics. R46 originalfirstFAIL retained causeUNKNOWN; a later PASS is not its fix. No strict5s sample while NAS Cargo active. Root only Windows slot, serial cleanup before next.','operations':[]};out.write_text(json.dumps(r,indent=2)+'\n')
commands=[]
for label,exe,base,readonly in [('modern-msvc-default-browser-continuous-final-third',old,ci,'windows-readonly-final-second'),('windows-default-browser-continuous-first',new,ev,'windows-readonly-final-first')]:
 cmd=['python3','-B','scripts/windows-tls-business-probe.py','--ssh-target','elior@192.168.10.212','--identity','/config/.local/share/future-classroom-test-access/win11-192.168.10.212','--exe',str(exe),'--web-bundle','/mnt/data/FutureClassroom-0.1.0-voice-capability-dev.zip','--continuous-accept','--browser','--voice-capabilities','--report',str(base/(label+'.json'))]
 commands.append((label,base,cmd));commands.append((readonly,base,['python3','-B',str(base/'audit-windows-readonly.py'),'--report',str(base/(readonly+'.json')),'--baseline','/workspace/共享/Eliora/未来教室计划/开发计划/验证记录/2026-11-01-round42-ci/windows-after-first.json']))
for name,base,args in commands:
 print('FINAL_BROWSER_STARTED '+name,flush=True);c=subprocess.run(['python3','-B',str(base/'run-recorded.py'),name,*args],cwd=root);r['operations'].append({'name':name,'exitCode':c.returncode});out.write_text(json.dumps(r,indent=2)+'\n')
 if c.returncode:r.update(status='FAIL',failedOperation=name);out.write_text(json.dumps(r,indent=2)+'\n');sys.exit(c.returncode)
r.update(status='PASS',completedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat());out.write_text(json.dumps(r,indent=2)+'\n');print('FINAL_BROWSERS_SERIAL_TERMINAL_PASS',flush=True)
