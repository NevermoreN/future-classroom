from pathlib import Path
import hashlib,json,zipfile,subprocess,tempfile,shutil
root=Path.cwd();ev=Path('/tmp/fc-round36-public-desktop-evidence');package=Path('/mnt/data/FutureClassroom-0.1.0-public-desktop-dev.zip')
sha=lambda b:hashlib.sha256(b).hexdigest()
assert not (ev/'package-independent-final.json').exists()
r={'scope':'Independent ZIP CRC, exact manifest members and payload SHA256; exact tested product SHA; reproducible repack. Development gates remain unchanged.'}
with zipfile.ZipFile(package) as z:
 assert z.testzip() is None
 names=z.namelist();assert len(names)==len(set(names))==13
 m=json.loads(z.read('manifest.json'));assert len(m['files'])==12
 assert set(names)=={'manifest.json'}|{x['path'] for x in m['files']}
 for x in m['files']:
  data=z.read(x['path']);assert len(data)==x['size'] and sha(data)==x['sha256']
 assert m['sha256']=='97ac55e84291d592ad06736e02e1f25de79a59e7a82758fea8a656fb1b99eeee'
 assert m['gates']['release_tag']=='NOT_ALLOWED' and m['status']=='NOT_VALIDATED'
 r.update(manifest=m,members=[{'path':n,'bytes':len(z.read(n)),'sha256':sha(z.read(n))} for n in names])
r['archive']={'path':str(package),'bytes':package.stat().st_size,'sha256':sha(package.read_bytes())}
owned=Path(tempfile.mkdtemp(prefix='fc-round36-public-desktop-repack-'));copy=owned/package.name
try:
 command=['python3','scripts/package-probe.py','--exe','/tmp/fc-round36-public-desktop-programs-ninth/fc-bootstrap.exe','--web-dist','web/dist','--target','i686-pc-windows-gnu','--toolchain','1.97.1','-o',str(copy)]
 p=subprocess.run(command,capture_output=True,timeout=120);r['repackCommand']=command;r['exitCode']=p.returncode
 (ev/'package-repack.stdout.log').write_bytes(p.stdout);(ev/'package-repack.stderr.log').write_bytes(p.stderr)
 assert p.returncode==0 and package.read_bytes()==copy.read_bytes()
 r['deterministicRepack']='PASS'
finally:
 shutil.rmtree(owned);r['ownedRepackRemoved']=not owned.exists()
r['status']='PASS';(ev/'package-independent-final.json').write_text(json.dumps(r,ensure_ascii=False,indent=2)+'\n');print(json.dumps({'status':'PASS',**r['archive']}))
