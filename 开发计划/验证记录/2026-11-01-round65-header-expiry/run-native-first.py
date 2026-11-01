from pathlib import Path
import json,hashlib,subprocess,time,re,sys
e=Path('/tmp/fc-round65-header-expiry/evidence');w=e.parent/'worktree';main=Path('/workspace/共享/Eliora/未来教室计划');script=main/'scripts/windows-probe.py';sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
f=json.loads((e/'implementation-freeze-first.json').read_text())
for x in f['source']:assert sha(w/x['relativePath'])==x['after']['sha256']
c=json.loads((e/'compile-test-first.json').read_text());assert c['exitCode']==0 and not c.get('timedOut') and not c.get('changedInputs')
meta=e/'compile-test-first.stdout.log';tests=[]
for line in meta.read_text().splitlines():
 try:x=json.loads(line)
 except ValueError:continue
 if x.get('reason')=='compiler-artifact' and x.get('profile',{}).get('test') and x.get('executable'):tests.append(x)
assert len(tests)==1 and tests[0]['target']['name']=='fc_windows'
a=json.loads((main/'开发计划/验证记录/2026-11-01-round64-ci/modern-original-independent-first.json').read_text());assert a['sourceSha']=='0f2bec38c9bd0442ae269225b13c3a6ce3aec0db';exe=Path(a['normalExecutable']['localPath']);assert sha(exe)==a['normalExecutable']['producer']['sha256']
out=e/'windows-native-first.json';record=e/'windows-native-operation-first.json';assert not out.exists() and not record.exists()
cmd=['python3','-B',str(script),'--host','elior@192.168.10.212','--identity','/config/.local/share/future-classroom-test-access/win11-192.168.10.212','--exe',str(exe),'--test-metadata',str(meta),'--test-timeout-seconds','900','--native-seconds','1','--report',str(out)]
r={'status':'RUNNING','command':cmd,'sourceFreezeSha256':sha(e/'implementation-freeze-first.json'),'scriptSha256':sha(script),'metadataSha256':sha(meta),'testExecutable':{'path':tests[0]['executable'],'sha256':sha(Path(tests[0]['executable']))},'baselineScaffoldNormal':{'sourceSha':a['sourceSha'],'path':str(exe),'sha256':sha(exe)},'scope':'Only the newly compiled R65 fc_windows test harness is candidate validation. The R64 modern normal EXE is an explicitly separate baseline diagnostic scaffold, NOT an R65 product. Actual Win11/WOW64 only, no physical TCP 5s or Win7/Win10 certification.'};record.open('x').write(json.dumps(r,indent=2)+'\n');print('R65_NEW_NATIVE_TESTS_STARTED',flush=True);t=time.monotonic()
with (e/'windows-native-first.stdout.log').open('xb') as x,(e/'windows-native-first.stderr.log').open('xb') as y:ret=subprocess.run(cmd,cwd=main,stdout=x,stderr=y).returncode
v=json.loads(out.read_text()) if out.exists() else {};r.update(status='PASS_COMMAND' if ret==0 else 'FAIL_COMMAND',exitCode=ret,elapsedSeconds=time.monotonic()-t,resultStatus=v.get('status'),cleanup=v.get('cleanup'),sourceUnchanged=all(sha(w/x['relativePath'])==x['after']['sha256'] for x in f['source']))
for k in ['stdout','stderr']:
 p=e/('windows-native-first.'+k+'.log');r[k]={'bytes':p.stat().st_size,'sha256':sha(p)}
if ret==0:
 assert len(v['tests'])==1;rows=[x for x in v['tests'][0]['output'] if x.startswith('test result:')];assert len(rows)==1;m=re.search(r'(\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out',rows[0]);assert m;r['counts']=list(map(int,m.groups()));assert r['counts']==[224,0,0,0,0]
record.write_text(json.dumps(r,indent=2)+'\n');print(json.dumps({k:r.get(k) for k in ['status','exitCode','elapsedSeconds','resultStatus','cleanup','counts','sourceUnchanged']}),flush=True);sys.exit(0 if ret==0 and r.get('cleanup')=='PASS' and r.get('resultStatus')=='PASS' and r['sourceUnchanged'] else 1)
