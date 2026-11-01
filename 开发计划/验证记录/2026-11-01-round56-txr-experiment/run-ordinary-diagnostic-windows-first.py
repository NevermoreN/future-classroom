from pathlib import Path
import json,hashlib,subprocess,time
R=Path('/workspace/共享/Eliora/未来教室计划');E=Path('/tmp/fc-round56-transactional-autorun-evidence');H=lambda b:hashlib.sha256(b).hexdigest();F=lambda p:{'path':str(p),'bytes':p.stat().st_size,'sha256':H(p.read_bytes())}
p=E/'ordinary-diagnostic-compile-first.json';c=json.loads(p.read_text());assert c['status'].startswith('PASS') and c['sourceBefore']==c['sourceAfter']
for rel,f in c['sourceAfter'].items():
 b=(R/rel).read_bytes();assert {'bytes':len(b),'sha256':H(b)}==f
normal=Path(c['normalBefore']['path']);assert F(normal)==c['normalBefore']
raw=Path(c['command']['stdout']['path']);rows=[]
for line in raw.read_text().splitlines(keepends=True):
 q=json.loads(line)
 if q.get('reason')=='compiler-artifact' and q.get('profile',{}).get('test') and q.get('executable') and q['target']['name']=='fc_windows':rows.append((line,Path(q['executable'])))
assert len(rows)==1
meta=E/'ordinary-diagnostic-selected-original-first.jsonl'
with meta.open('x') as out:out.write(rows[0][0])
result=E/'ordinary-diagnostic-windows-first.json';report=E/'ordinary-diagnostic-windows-operation-first.json';assert not result.exists() and not report.exists()
a=['python3','-B',str(E/'windows-probe-ordinary-filter-first.py'),'--host','elior@192.168.10.212','--identity','/config/.local/share/future-classroom-test-access/win11-192.168.10.212','--exe',str(normal),'--test-metadata',str(meta),'--test-timeout-seconds','900','--native-seconds','1','--report',str(result)]
r={'status':'RUNNING','scope':'Same production candidate, one existing test gets raw diagnostic output. Execute exactly one fresh-process ordinary competing-writer test without system-probe preflight; original assertion unchanged, other tests intentionally filtered. Native UI may remain unexecuted if tests fail.','compile':F(p),'test':F(rows[0][1]),'normal':F(normal),'selectedOriginalMetadata':F(meta),'fullOriginalMetadata':F(raw),'argv':a}
def save():report.write_text(json.dumps(r,ensure_ascii=False,indent=2)+'\n')
save();print('WINDOWS_DIAGNOSTIC_STARTED',flush=True);start=time.monotonic()
with (E/'ordinary-diagnostic-windows-first.stdout.log').open('xb') as out,(E/'ordinary-diagnostic-windows-first.stderr.log').open('xb') as err:child=subprocess.run(a,cwd=R,stdout=out,stderr=err)
d=json.loads(result.read_text()) if result.exists() else {}
r.update(status=d.get('status','MISSING_RESULT'),exitCode=child.returncode,actualExit=True,elapsedSeconds=time.monotonic()-start,result=F(result) if result.exists() else None,cleanup=d.get('cleanup'),stdout=F(E/'ordinary-diagnostic-windows-first.stdout.log'),stderr=F(E/'ordinary-diagnostic-windows-first.stderr.log'));save();print(json.dumps({'status':r['status'],'exitCode':child.returncode,'cleanup':r['cleanup']}),flush=True)
raise SystemExit(child.returncode)
