from pathlib import Path
import hashlib,json,subprocess,time
e=Path(__file__).resolve().parent
command=['python3','-B',str(e/'root-audit-windows-readonly-first.py'),'--report',str(e/'windows-readonly-final-first.json'),'--baseline','/workspace/共享/Eliora/未来教室计划/开发计划/验证记录/2026-11-01-round73-installer-broker/windows-readonly-final-first.json']
report=e/'windows-readonly-operation-first.json';out=e/'windows-readonly-operation-first.stdout.log';err=e/'windows-readonly-operation-first.stderr.log'
assert not any(p.exists() for p in [report,out,err])
v=dict(status='RUNNING',command=command,scope='Final read-only owned-residue, Run and persistent SSH check after R74 modern MSVC finite Windows runtime; before R75 independent MSI probes.')
with report.open('x') as f:json.dump(v,f,indent=2)
t=time.monotonic()
with out.open('xb') as stdout,err.open('xb') as stderr:
 c=subprocess.Popen(command,stdout=stdout,stderr=stderr);code=c.wait()
v.update(status='PASS_COMMAND' if code==0 else 'FAIL_COMMAND',actualExit=True,exitCode=code,elapsedSeconds=time.monotonic()-t)
for k,p in [('stdout',out),('stderr',err)]:
 b=p.read_bytes();v[k]=dict(path=str(p),bytes=len(b),sha256=hashlib.sha256(b).hexdigest())
report.write_text(json.dumps(v,indent=2)+'\n');print(json.dumps(v))
raise SystemExit(0 if code==0 else 1)
