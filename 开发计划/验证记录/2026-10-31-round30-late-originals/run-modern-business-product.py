import subprocess,pathlib,os,json,time,hashlib,sys
root=pathlib.Path.cwd(); evidence=pathlib.Path('/tmp/fc-round30-evidence'); prefix=sys.argv[1]
command=['cargo','build','-p','fc-bootstrap','--bin','fc-bootstrap','--release','--locked','--target','i686-pc-windows-gnu','--target-dir','/tmp/fc-round30-win-target','--message-format=json']
def snapshot():
 paths=sorted([p for p in root.glob('crates/**/*.rs') if p.is_file()]+[root/x for x in ['Cargo.lock','Cargo.toml','rust-toolchain.toml','.cargo/config.toml','scripts/windows-tls-business-probe.py','scripts/windows-tls-probe.py','scripts/windows-probe.py']])
 return {str(p.relative_to(root)):hashlib.sha256(p.read_bytes()).hexdigest() for p in paths}
before=snapshot();start=time.time()
print('candidate business product compilation started',flush=True)
with (evidence/(prefix+'.jsonl')).open('wb') as output,(evidence/(prefix+'.stderr.log')).open('wb') as errors:
 result=subprocess.run(command,env={**os.environ,'CARGO_BUILD_JOBS':'2'},stdout=output,stderr=errors)
after=snapshot();diagnostics=[];artifacts=[]
for line in (evidence/(prefix+'.jsonl')).read_text().splitlines():
 try: row=json.loads(line)
 except ValueError: continue
 if row.get('reason')=='compiler-message': diagnostics.append(row['message'].get('rendered',row['message']['message']))
 if row.get('reason')=='compiler-artifact' and row.get('executable'):
  artifact=pathlib.Path(row['executable']);artifacts.append({'target':row['target'],'executable':str(artifact),'sha256':hashlib.sha256(artifact.read_bytes()).hexdigest(),'bytes':artifact.stat().st_size})
(evidence/(prefix+'.diagnostics.log')).write_text('\n'.join(diagnostics))
report={'command':command,'exitCode':result.returncode,'elapsedSeconds':time.time()-start,'sourceBefore':before,'sourceAfter':after,'changedInputs':[p for p in set(before)|set(after) if before.get(p)!=after.get(p)],'artifacts':artifacts,'scope':'finite explicit business candidate product compile only; separate test executables and actual TLS run required'}
(evidence/(prefix+'.json')).write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n')
print(json.dumps({'exitCode':result.returncode,'elapsedSeconds':report['elapsedSeconds'],'artifacts':artifacts},ensure_ascii=False),flush=True)
if result.returncode: print('\n'.join(diagnostics)[-18000:],flush=True)
raise SystemExit(result.returncode)
