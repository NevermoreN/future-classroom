"""Focused native harness using one exact original Cargo JSON row; no test filter."""
from pathlib import Path
import hashlib, json, runpy, subprocess, time
r=Path('/tmp/fc-round73-installer-broker');e=r/'evidence';w=r/'worktree'
v=json.loads((e/'n-release-third.json').read_bytes())
assert v['status']=='PASS_CODEGEN_NOT_RUNTIME' and v['sourceUnchanged']
terminal=json.loads((e/'n-producer-terminal-third.json').read_bytes())
assert terminal['status']=='PASS_CONTROLLER_ACTUAL_EXIT0_REAPED' and terminal['actualToolTerminal']['exitCode']==0 and terminal['actualToolTerminal']['runningSessionReturned'] is False
assert terminal['report']['path']=='n-release-third.json' and terminal['report']['sha256']==hashlib.sha256((e/'n-release-third.json').read_bytes()).hexdigest()
rows=[]
for line in (e/'n-stable-release-tests-third.stdout.log').read_bytes().splitlines(keepends=True):
    item=json.loads(line)
    if item.get('reason')=='compiler-artifact' and item.get('profile',{}).get('test') is True and item.get('executable') and item['target']['name']=='fc_windows': rows.append((line,item))
assert len(rows)==1
def fact(p):
    p=Path(p);b=p.read_bytes();return dict(path=str(p),bytes=len(b),sha256=hashlib.sha256(b).hexdigest())
native=next(x for x in v['testExecutables'] if x['target']=='fc_windows')
assert fact(native['path'])['sha256']==native['sha256'] and rows[0][1]['executable']==native['path']
app=next(x for x in v['normalExecutables'] if x['target']=='fc-bootstrap')
assert fact(app['path'])['sha256']==app['sha256'] and app['peSubsystem']==2
subset=e/'root-native-harness-metadata-third.jsonl'
with subset.open('xb') as stream: stream.write(rows[0][0])
report=e/'root-windows-native-harness-third.json';op=e/'root-windows-native-harness-third.command.json'
out=e/'root-windows-native-harness-third.stdout.log';err=e/'root-windows-native-harness-third.stderr.log'
assert not any(p.exists() for p in [report,op,out,err])
command=['python3','-B',str(w/'scripts/windows-probe.py'),'--host','elior@192.168.10.212','--identity','/config/.local/share/future-classroom-test-access/win11-192.168.10.212','--exe',app['path'],'--test-metadata',str(subset),'--test-timeout-seconds','900','--native-seconds','1','--system-host','--report',str(report)]
before=[fact(native['path']),fact(app['path']),fact(subset),fact(e/'n-release-third.json')]
v=dict(status='RUNNING',command=command,inputIdentities=before,originalPerTestBudgetSeconds=900,scope='One complete native harness, original unfiltered cases, native1/system. Not full22/public20/MSI or Win7/10 OS acceptance.')
with op.open('x') as s: json.dump(v,s,indent=2)
t=time.monotonic()
with out.open('xb') as stdout,err.open('xb') as stderr:
    child=subprocess.Popen(command,cwd=w,stdout=stdout,stderr=stderr);code=child.wait()
v.update(exitCode=code,actualExit=True,elapsedSeconds=time.monotonic()-t,status='FAIL_ACTUAL_NATIVE_OR_VERIFICATION')
if report.exists():
    actual=json.loads(report.read_bytes());v.update(actualStatus=actual['status'],cleanup=actual['cleanup'],actualReport=fact(report))
    if code==0 and actual['status']==actual['cleanup']=='PASS':
        parser=runpy.run_path('/workspace/共享/Eliora/未来教室计划/开发计划/验证记录/2026-11-01-round67-ci/independent-runtime-first.py')
        parser['check_platform'](actual)
        assert len(actual['tests'])==1 and actual['tests'][0]['binarySha256']==native['sha256']
        assert [fact(x['path']) for x in before]==before
        v.update(status='PASS_ACTUAL_NATIVE_HARNESS_ONLY',totals=parser['harness'](actual['tests'][0]),inputsUnchanged=True)
v.update(stdout=fact(out),stderr=fact(err));op.write_text(json.dumps(v,indent=2)+'\n')
print(json.dumps({k:v.get(k) for k in ['status','exitCode','cleanup','elapsedSeconds','totals']}))
raise SystemExit(0 if v['status']=='PASS_ACTUAL_NATIVE_HARNESS_ONLY' else 1)
