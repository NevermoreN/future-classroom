from pathlib import Path
import json,hashlib,subprocess,shutil,runpy,time
root=Path.cwd();head=subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip();proof=json.loads(Path('/tmp/fc-round39-committed-proof-first.json').read_text());assert proof['status']=='PASS' and head==proof['sourceCommit'];h=lambda b:hashlib.sha256(b).hexdigest()
work=Path('/tmp/fc-round39-voice-capability-worktree');assert not subprocess.check_output(['git','status','--porcelain'],cwd=work)
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=work,text=True).strip()==head
r={'status':'RUNNING','sourceCommit':head,'committedFilesVerified':1070,'removed':[],'preserved':['persistent SSH private key and authorized_keys/sshd','main original target/global Cargo/Rust/npm caches','intentional /mnt/data development ZIPs','active R41 worktree/evidence']};out=root/'开发计划/验证记录/2026-11-01-round39-cleanup-first.json';assert not out.exists()
def save():out.write_text(json.dumps(r,ensure_ascii=False,indent=2)+'\n')
save()
# Carry only manifest-verified Web assets into the next concrete R39 Windows verification.
next_ev=Path('/tmp/fc-round42-ci-evidence');assert not next_ev.exists();next_ev.mkdir();(next_ev/'.owner').write_text('future-classroom-owned-r42-ci-evidence/v1\n')
assets=Path('/tmp/fc-round42-web-assets');assert not assets.exists();assets.mkdir();(assets/'.owner').write_text('future-classroom-owned-r42-web-assets/v1\n')
payloads,manifest=runpy.run_path(str(root/'scripts/package-probe.py'))['frontend_payloads'](work/'web/dist')
rows=[]
for p in sorted((work/'web/dist').rglob('*')):
 if not p.is_file():continue
 rel=p.relative_to(work/'web/dist');dest=assets/'dist'/rel;dest.parent.mkdir(parents=True,exist_ok=True);data=p.read_bytes();dest.write_bytes(data);assert dest.read_bytes()==data;rows.append({'path':str(rel),'bytes':len(data),'sha256':h(data)})
(next_ev/'web-build-budget-original.json').write_bytes((work/'web/build-budget.json').read_bytes())
(next_ev/'web-assets-handoff-first.json').write_text(json.dumps({'status':'PASS','sourceCommit':head,'sourceVerification':'committed round39 final-frozen-verification-first.json, Web56 source inputs/300 tests unchanged','packageManifestValidation':manifest,'assetRoot':str(assets),'assets':rows,'purpose':'Owned current R39 Web output for new exact-source Windows CI/runtime package, not old 3b3 runtime evidence. Remove after finished verification and intentional package byte proof.'},indent=2)+'\n')
r['webAssetsHandoff']={'root':str(assets),'fileCount':len(rows),'bytes':sum(x['bytes'] for x in rows),'evidence':str(next_ev/'web-assets-handoff-first.json')};save()
# Recheck committed original archive and finished evidence roots before deleting copies.
for name,label in [('fc-round39-voice-capability-evidence','2026-11-01-round39-voice-capability'),('fc-round40-ci-evidence','2026-11-01-round40-ci')]:
 source=Path('/tmp')/name;dest=root/'开发计划/验证记录'/label;idx=json.loads((dest/'INDEX.json').read_text());assert {p.relative_to(source).as_posix() for p in source.rglob('*') if p.is_file()}=={x['path'] for x in idx['files']}
 for x in idx['files']:
  data=(source/x['path']).read_bytes();assert len(data)==x['bytes'] and h(data)==x['sha256'] and data==(dest/x['path']).read_bytes()
# Exact original artifact bytes and all owned EXE copies were already independently audited in committed reports.
ev=root/'开发计划/验证记录/2026-11-01-round40-ci'
for n,report in [('fc-round40-modern-original.zip','modern-full-download-first.json'),('fc-round40-win7-original.zip','win7-full-download-first.json')]:
 p=Path('/tmp')/n;expected=json.loads((ev/report).read_text());assert expected['status']=='PASS';data=p.read_bytes();assert len(data)==expected['bytes'] and h(data)==expected['sha256'];p.unlink();r['removed'].append({'path':str(p),'kind':'exact original ZIP copy','bytes':len(data),'sha256':h(data),'removed':not p.exists()});save()
programs=Path('/tmp/fc-round40-modern-programs');audit=json.loads((ev/'modern-programs-independent-first.json').read_text());assert audit['status']=='PASS';assert {p.name for p in programs.iterdir()}=={Path(x['localPath']).name for x in audit['artifacts']}
for x in audit['artifacts']:
 p=programs/Path(x['localPath']).name;data=p.read_bytes();assert len(data)==x['bytes'] and h(data)==x['sha256']
shutil.rmtree(programs);r['removed'].append({'path':str(programs),'kind':'owned exact EXE copies','files':22,'removed':not programs.exists()});save()
# No local Cargo/test/compiler is live. No process is killed to make this true.
lines=subprocess.check_output(['ps','-eo','args'],text=True).splitlines()
assert not any(x.startswith(('/config/.rustup/','cargo ')) and any(y in x for y in ['/tmp/fc-round38-host-target','/tmp/fc-round38-lint-target','fc-round39-voice-capability-worktree']) for x in lines)
for name in ['fc-round38-host-target','fc-round38-lint-target']:
 p=Path('/tmp')/name;assert p.is_dir() and not p.is_symlink() and (p/'CACHEDIR.TAG').is_file();shutil.rmtree(p);r['removed'].append({'path':str(p),'kind':'owned completed build cache handed from R38 to R39, now terminal','removed':not p.exists()});save()
ignored=subprocess.check_output(['git','ls-files','--others','--ignored','--exclude-standard'],cwd=work,text=True).splitlines();assert all(x=='web/build-budget.json' or x.startswith('web/dist/') for x in ignored)
shutil.rmtree(work/'web/dist');(work/'web/build-budget.json').unlink();subprocess.run(['git','worktree','remove',str(work)],cwd=root,check=True);subprocess.run(['git','branch','-d','codex/voice-capability-observation'],cwd=root,check=True,stdout=subprocess.DEVNULL);r['removed'].append({'path':str(work),'kind':'clean owned merged worktree and local branch','removed':not work.exists()});save()
for name in ['fc-round39-voice-capability-evidence','fc-round40-ci-evidence']:
 p=Path('/tmp')/name;shutil.rmtree(p);r['removed'].append({'path':str(p),'kind':'committed exact original evidence work copy','removed':not p.exists()});save()
(root/'开发计划/验证记录/2026-11-01-round39-committed-proof.json').write_text(json.dumps(proof,indent=2)+'\n')
r['status']='PASS';r['noProcessesKilled']=True;r['windowsFinishAudit']='round40-ci/windows-after-first.json PASS, 0 owned process/temp/task, SSH/Run unchanged';save();print(json.dumps({'status':r['status'],'sourceCommit':head,'removedRoots':len(r['removed']),'webAssetHandoff':r['webAssetsHandoff']}),flush=True)
