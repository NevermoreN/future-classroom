"""Run complete frozen GNU candidate harnesses on Win11 after actual producer exit."""
from pathlib import Path
import argparse, ast, hashlib, json, runpy, subprocess, time
root = Path('/tmp/fc-round73-installer-broker'); work = root/'worktree'; ev = root/'evidence'
p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--producer-parent-terminal', action='store_true')
p.add_argument('--execute', action='store_true')
a = p.parse_args()
assert (root/'.owner').read_bytes() == b'future-classroom-owned-r73-installer-broker/v1\n'
producer = ev/'n-release-first.json'
v = json.loads(producer.read_bytes())
if v['status'] == 'RUNNING' or not a.producer_parent_terminal:
    print(json.dumps(dict(status='WAIT', reason='Original producer and parent must be terminal; no binary/metadata opened.')))
    raise SystemExit(3)
assert v['status'] == 'PASS_CODEGEN_NOT_RUNTIME' and v['sourceUnchanged'] and v['sourceBefore'] == v['sourceAfter']
assert len(v['steps']) == 3 and all(s['status']=='PASS' and s['exitCode']==0 and s['actualExit'] and not s['timedOut'] for s in v['steps'])
def fact(path):
    path = Path(path); assert path.is_file() and not path.is_symlink()
    b = path.read_bytes(); return dict(path=str(path), bytes=len(b), sha256=hashlib.sha256(b).hexdigest())
inputs = {}
for row in v['sourceBefore']:
    path = work/row['path']; assert path.resolve().is_relative_to(work)
    f = fact(path); assert f['bytes']==row['bytes'] and f['sha256']==row['sha256']; inputs[str(path)] = f
for row in v['testExecutables'] + v['normalExecutables']:
    f = fact(row['path']); assert f['bytes']==row['bytes'] and f['sha256']==row['sha256']; inputs[row['path']] = f
tests = v['testExecutables']; normals = {r['target']:r for r in v['normalExecutables']}
assert len(tests)==22 and len({r['path'] for r in tests})==22 and all(r['profile']['test'] is True and r['peSubsystem']==3 for r in tests)
assert set(normals)=={'fc-bootstrap','fc-installer-lifecycle'} and all(r['profile']['test'] is False and r['peSubsystem']==2 for r in normals.values())
metadata = ev/'n-stable-release-tests-first.stdout.log'; probe = work/'scripts/windows-probe.py'
assert fact(probe)['sha256']=='f176a022bf90565be7074f53a2d502d6519afd1748cdd2022f608a217d485c14'
for path in (producer,metadata,probe): inputs[str(path)] = fact(path)
expected_tests = {str(Path(x['executable'])) for x in map(json.loads,metadata.read_text().splitlines()) if x.get('reason')=='compiler-artifact' and x.get('profile',{}).get('test') is True and x.get('executable')}
assert expected_tests == {r['path'] for r in tests}
report = ev/'root-windows-gnu-full-first.json'; operation = ev/'root-windows-gnu-full-first.command.json'
out,err = ev/'root-windows-gnu-full-first.stdout.log',ev/'root-windows-gnu-full-first.stderr.log'
assert not any(p.exists() for p in (report,operation,out,err))
command = ['python3','-B',str(probe),'--host','elior@192.168.10.212',
    '--identity','/config/.local/share/future-classroom-test-access/win11-192.168.10.212',
    '--exe',normals['fc-bootstrap']['path'],'--test-metadata',str(metadata),'--test-timeout-seconds','900',
    '--native-seconds','1','--system-host','--public-desktop','--report',str(report)]
if not a.execute:
    print(json.dumps(dict(status='PREPARED_ONLY', command=command, testHarnessCount=22, expectedUploadedArtifacts=23, helperNormal='Pinned GUI product, not uploaded or executed by this full harness controller.')))
    raise SystemExit(0)
result = dict(status='RUNNING', command=command, inputIdentities=inputs, normalHelper=normals['fc-installer-lifecycle'], originalPerTestBudgetSeconds=900,
    scope='Finite GNU full22/native1/system/public20 on authorized Win11 only. No actual MSI/helper normal execution, Win7/10 OS, campus or full installer acceptance.')
with operation.open('x') as s: json.dump(result,s,indent=2)
t = time.monotonic()
with out.open('xb') as stdout,err.open('xb') as stderr:
    child = subprocess.Popen(command,cwd=work,stdout=stdout,stderr=stderr); code = child.wait()
result.update(exitCode=code,actualExit=True,elapsedSeconds=time.monotonic()-t)
try:
    assert code == 0 and all(fact(path)==row for path,row in inputs.items())
    actual = json.loads(report.read_bytes())
    parser = runpy.run_path('/workspace/共享/Eliora/未来教室计划/开发计划/验证记录/2026-11-01-round67-ci/independent-runtime-first.py')
    parser['check_platform'](actual)
    assert actual['exeSha256']==normals['fc-bootstrap']['sha256'] and actual['testTimeoutSeconds']==900
    uploaded = {Path(r['path']).name:r['sha256'] for r in tests+[normals['fc-bootstrap']]}
    assert len(actual['uploadedArtifacts'])==23 and {r['name']:r['sha256'] for r in actual['uploadedArtifacts']}==uploaded
    assert len(actual['tests'])==22 and {r['name'] for r in actual['tests']}=={Path(r['path']).name for r in tests}
    cases=[]
    for row in actual['tests']:
        assert row['binarySha256']==uploaded[row['name']]; cases.append(parser['harness'](row))
    steps=None
    for node in ast.parse(probe.read_bytes()).body:
        if isinstance(node,ast.Assign) and any(isinstance(n,ast.Name) and n.id=='PUBLIC_DESKTOP_STEPS' for n in node.targets): steps=ast.literal_eval(node.value)
    assert steps is not None and len(steps)==20
    result.update(publicDesktop=parser['flow'](actual,'public',steps),harnesses=cases,totals={k:sum(c[k] for c in cases) for k in ('passed','failed','ignored','measured','filteredOut')})
    assert actual['native']['hostExitCode']==actual['system']['exitCode']==0 and actual['cleanup']=='PASS'
    result.update(status='PASS_FINITE_GNU_WIN11_FULL22_ONLY',cleanup=actual['cleanup'],inputsUnchanged=True)
except Exception as error:
    result.update(status='FAIL_RUNTIME_OR_VERIFICATION',failureType=type(error).__name__)
for key,path in [('stdout',out),('stderr',err)]: result[key]=fact(path)
if report.exists(): result['actualReport']=fact(report)
operation.write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps({k:result.get(k) for k in ('status','exitCode','elapsedSeconds','cleanup','totals')}))
raise SystemExit(0 if result['status']=='PASS_FINITE_GNU_WIN11_FULL22_ONLY' else 1)
