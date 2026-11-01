from pathlib import Path
import json,hashlib,subprocess,time,sys
e=Path('/tmp/fc-round63-installer-owner/evidence');main=Path('/workspace/共享/Eliora/未来教室计划');sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest();script=main/'scripts/windows-probe.py'
a=json.loads((e/'windows-release-seventh-operation-first.json').read_text());v=json.loads((e/'windows-release-seventh-first.json').read_text());assert a['status']=='PASS_COMMAND' and v['status']=='PASS' and v['cleanup']=='PASS';exe=Path(a['normal']['path']);assert sha(exe)==a['normal']['sha256']
p=e/'windows-release-seventh-extra-flows-first.json';assert not p.exists();r={'status':'RUNNING','normal':a['normal'],'sourceFreezeSha256':a['sourceFreezeSha256'],'scriptSha256':sha(script),'operations':[]}
def save():p.write_text(json.dumps(r,indent=2)+'\n')
save()
for kind,flag in [('visual','--subject-visuals'),('repair','--reinitialization')]:
 name='windows-release-seventh-'+kind+'-first';out=e/(name+'.json');assert not out.exists();cmd=['python3','-B',str(script),'--host','elior@192.168.10.212','--identity','/config/.local/share/future-classroom-test-access/win11-192.168.10.212','--exe',str(exe),'--native-seconds','1',flag,'--report',str(out)];row={'kind':kind,'status':'RUNNING','command':cmd};r['operations'].append(row);save();print('FLOW_STARTED '+kind,flush=True);t=time.monotonic()
 with (e/(name+'.stdout.log')).open('xb') as x,(e/(name+'.stderr.log')).open('xb') as y:ret=subprocess.run(cmd,cwd=main,stdout=x,stderr=y).returncode
 d=json.loads(out.read_text()) if out.exists() else {};row.update(exitCode=ret,elapsedSeconds=time.monotonic()-t,resultStatus=d.get('status'),cleanup=d.get('cleanup'));row['status']='PASS' if ret==0 and d.get('status')=='PASS' and d.get('cleanup')=='PASS' else 'FAIL'
 for k in ['stdout','stderr']:
  f=e/(name+'.'+k+'.log');row[k]={'bytes':f.stat().st_size,'sha256':sha(f)}
 save()
 if row['status']!='PASS':r['status']='FAIL';save();sys.exit(1)
r['status']='PASS_TWO_DEDICATED_HIDDEN_FLOWS';save();print(r['status'],flush=True)
