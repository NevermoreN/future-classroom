from pathlib import Path
import zipfile,json,hashlib,subprocess,base64,shutil
root=Path('/tmp/fc-round31-device-worktree');ev=Path('/tmp/fc-round31-device-evidence')
out=ev/'development-package-first.json';assert not out.exists()
old=Path('/mnt/data/FutureClassroom-0.1.0-streaming-tls-dev.zip')
web=Path('/tmp/fc-round31-web-dist');repack=Path('/tmp/fc-round31-repack')
package=Path('/mnt/data/FutureClassroom-0.1.0-voice-output-dev.zip')
assert not web.exists() and not repack.exists() and not package.exists()
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
assert sha(old)=='03f1f4ca2fa098688dedad32c5e3a274a21baba313ad2d1ff29c089b141caad6'
normal=Path('/tmp/fc-round31-device-programs/fc-bootstrap.exe')
assert sha(normal)=='81ae6c690b4e8ac2516f76a6b70cea55adc8ddef79ac621fc2813759abfeb3d3'
web.mkdir();repack.mkdir();webrows=[]
with zipfile.ZipFile(old) as archive:
    for name in archive.namelist():
        if not name.startswith('web/'):continue
        p=Path(name[4:]);assert not p.is_absolute() and '..' not in p.parts
        data=archive.read(name);dest=web/p;dest.parent.mkdir(parents=True,exist_ok=True);dest.write_bytes(data)
        webrows.append({'path':p.as_posix(),'bytes':len(data),'sha256':hashlib.sha256(data).hexdigest()})
r={'scope':'Updated development probe package with the exact Win11-tested GNU normal EXE and byte-identical previously validated Web payload. Not a release, installer, Win7/10 or formal MSVC certification.','reusedWebSource':{'packagePath':str(old),'sha256':sha(old),'files':webrows},'normalExeSha256':sha(normal),'packagerSourceSha256':sha(root/'scripts/package-probe.py'),'readmeSha256':sha(root/'packaging/PROBE-README.txt'),'commands':[]}
def save():out.write_text(json.dumps(r,indent=2)+'\n')
for output in [package,repack/package.name]:
    command=['python3','-B','scripts/package-probe.py','--exe',str(normal),'--output',str(output),'--target','i686-pc-windows-gnu','--toolchain','1.97.1-i686-pc-windows-gnu','--web-dist',str(web)]
    x=subprocess.run(command,cwd=root,capture_output=True);r['commands'].append({'command':command,'exitCode':x.returncode,'stdoutBase64':base64.b64encode(x.stdout).decode(),'stderrBase64':base64.b64encode(x.stderr).decode()});save();assert x.returncode==0
assert sha(package)==sha(repack/package.name)
with zipfile.ZipFile(package) as archive:
    assert archive.testzip() is None
    manifest=json.loads(archive.read('manifest.json'))
    assert manifest['status']=='NOT_VALIDATED' and manifest['sha256']==sha(normal)
    assert '--voice-output-probe' in manifest['features']['declared_probe_commands']
    assert manifest['features']['default_output_history']
    assert archive.read('fc-bootstrap.exe')==normal.read_bytes()
    for row in webrows:
        data=archive.read('web/'+row['path']);assert hashlib.sha256(data).hexdigest()==row['sha256']
    r['archiveMembers']=[{'path':i.filename,'bytes':i.file_size,'sha256':hashlib.sha256(archive.read(i)).hexdigest()} for i in archive.infolist()]
    r['manifest']=manifest
r['package']={'path':str(package),'bytes':package.stat().st_size,'sha256':sha(package)}
r['repackByteIdentical']=True;r['status']='PASS_DEVELOPMENT_PACKAGE_ONLY';save()
shutil.rmtree(web);shutil.rmtree(repack);r['temporaryPackagingRootsRemoved']=not web.exists() and not repack.exists();save()
print(json.dumps({'status':r['status'],'package':r['package'],'repackByteIdentical':True,'temporaryPackagingRootsRemoved':r['temporaryPackagingRootsRemoved']}),flush=True)
