from pathlib import Path
import json,hashlib,struct,subprocess,zipfile

base=Path('/tmp/fc-round30-evidence')
report=base/'ci-fifth-original-artifact-verification.json'
assert not report.exists()
meta=json.loads((base/'ci-fifth-run-artifact.json').read_bytes())
archive=Path(meta['original_archive_path'])
def sha(data): return hashlib.sha256(data).hexdigest()
assert archive.stat().st_size==37661711
assert sha(archive.read_bytes())==meta['zip_sha256']=='52c64e8b7058a4423f0268e836c4a076eb1c97b26d02668988d9abd7fbf1c821'
with zipfile.ZipFile(archive) as z: assert z.testzip() is None
for row in meta['members']:
    data=Path(row['preserved_path']).read_bytes()
    assert len(data)==row['bytes'] and sha(data)==row['sha256']
raw=base/'ci-fifth-run-artifact-raw'
result=json.loads((raw/'result.json').read_bytes())
assert result['source_sha']==meta['source_sha']=='551299e17ba69f6944cf924917c632eaa005e07a'
assert result['executable_count']==22 and result['status']=='FAILED_OR_INCOMPLETE'
steps={s['name']:s for s in result['commands']}
for name in ['workspace-test-compile','product-compile','clippy']:
    assert steps[name]['exit_code']==0 and steps[name]['parent_exited'] and not steps[name]['timed_out_or_pipe_stalled'] and not steps[name]['log_truncated']
assert not any(name.startswith('pe-imports-') for name in steps)
assert not (raw/'executables.json').exists()
cleanup=json.loads((raw/'cleanup.json').read_bytes())
assert cleanup['status']=='OWNED_BUILD_ROOTS_REMOVED' and all(x['removed'] for x in cleanup['roots'])
inputs=json.loads((raw/'input-sha256.json').read_bytes())
assert sha((raw/'input-sha256.json').read_bytes())==result['input_manifest_sha256']
tree=subprocess.check_output(['git','ls-tree','-r','-z',result['source_sha']])
objects={}
for entry in tree.split(b'\0'):
    if not entry: continue
    header,path=entry.split(b'\t',1);mode,kind,oid=header.split(b' ')
    assert kind==b'blob'
    objects[path.decode()]=oid.decode()
assert {row['path'] for row in inputs}==set(objects)
batch=subprocess.Popen(['git','cat-file','--batch'],stdin=subprocess.PIPE,stdout=subprocess.PIPE)
try:
    for row in inputs:
        batch.stdin.write((objects[row['path']]+'\n').encode());batch.stdin.flush()
        header=batch.stdout.readline().split();assert header[1]==b'blob'
        data=batch.stdout.read(int(header[2]));assert batch.stdout.read(1)==b'\n'
        assert len(data)==row['bytes'] and sha(data)==row['sha256'],row['path']
finally:
    batch.stdin.close();batch.stdout.close();assert batch.wait()==0
programs=[]
for row in meta['members']:
    if not row['binary']: continue
    path=Path(row['preserved_path']);data=path.read_bytes()
    assert path.suffix.lower()=='.exe' and data[:2]==b'MZ'
    offset=struct.unpack_from('<I',data,0x3c)[0]
    assert data[offset:offset+4]==b'PE\0\0'
    assert struct.unpack_from('<H',data,offset+4)[0]==0x14c and struct.unpack_from('<H',data,offset+24)[0]==0x10b
    programs.append({'path':row['path'],'bytes':len(data),'sha256':sha(data),'machine':'0x014c','optionalMagic':'0x010b','executed':False})
assert len(programs)==22
r={'status':'VERIFIED_ORIGINAL_FAILED_CI_ARTIFACT','scope':'Independent local original archive, CRC, all member hashes, exact Git-blob inputs and PE32 headers. No runtime, PE imports or CRT validation claimed.','runId':meta['run_id'],'sourceSha':result['source_sha'],'zipBytes':meta['zip_bytes'],'zipSha256':meta['zip_sha256'],'members':len(meta['members']),'nonbinaryMembers':sum(not x['binary'] for x in meta['members']),'gitBlobInputsVerified':len(inputs),'actualCommandResults':{name:{'exitCode':steps[name]['exit_code'],'elapsedSeconds':steps[name]['duration_seconds']} for name in ['workspace-test-compile','product-compile','clippy']},'originalOverallStatus':result['status'],'originalFailure':json.loads((raw/'failure.json').read_bytes()),'originalCleanup':cleanup,'peImportsAndStaticCrt':'NOT_RUN; dictionary sum failed before first pe-imports command and executable manifest','windowsRuntime':'NOT_RUN','programs':programs,'binaryPolicy':'No EXE/ZIP/base64 binary committed; originals outside Git pending exact own cleanup.'}
report.write_text(json.dumps(r,indent=2)+'\n')
print(json.dumps({k:r[k] for k in ['status','members','nonbinaryMembers','gitBlobInputsVerified','actualCommandResults','peImportsAndStaticCrt']}),flush=True)
