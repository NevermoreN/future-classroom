from pathlib import Path,PurePosixPath
import os,io,json,urllib.request,hashlib,zipfile,zlib
import sys
url=sys.stdin.readline().strip()
base=Path('/tmp/fc-round34-accept-evidence');out=base/'win7-selected-originals-first.json';root=base/'win7-selected-raw'
assert not out.exists() and not root.exists()
size=40268444;requests=[];etag=None
def sha(b):return hashlib.sha256(b).hexdigest()
class RangedFile(io.RawIOBase):
    def __init__(self):self.position=0
    def seekable(self):return True
    def readable(self):return True
    def tell(self):return self.position
    def seek(self,offset,whence=0):
        self.position=offset if whence==0 else self.position+offset if whence==1 else size+offset
        if not 0<=self.position<=size:raise ValueError('Out-of-archive seek')
        return self.position
    def read(self,length=-1):
        global etag
        length=size-self.position if length<0 else min(length,size-self.position)
        if length==0:return b''
        assert 0<length<=1024*1024,'Refuse a full archive or binary-sized range'
        start=self.position;end=start+length-1
        request=urllib.request.Request(url,headers={'Range':f'bytes={start}-{end}','Accept-Encoding':'identity'})
        with urllib.request.urlopen(request,timeout=25) as response:
            assert response.status==206,'Server did not honor exact range; no full read attempted'
            assert response.headers['Content-Range']==f'bytes {start}-{end}/{size}'
            current=response.headers.get('ETag');assert current
            if etag is None:etag=current
            assert current==etag
            data=response.read(length+1);assert len(data)==length
        requests.append({'start':start,'end':end,'status':206,'bytes':length,'sha256':sha(data)})
        self.position+=length
        return data
names=['result.json','failure.json','cleanup.json','executables.json','artifact-sha256.json','rust-src-provenance.json','msvc-sdk.json','logs/rust-version.stdout.log','logs/cargo-version.stdout.log','logs/rust-src-components.stdout.log','logs/rust-sysroot.stdout.log','logs/before-vs-crt-flags.json','logs/after-vs-crt-flags.json','logs/before-cargo-crt-flags.json']
r={'scope':'Selected original nonbinary ZIP members read via validated HTTP206 ranges and ZIP member CRC. This is NOT a whole-archive SHA validation or an EXE/runtime/import recheck. API/upload-reported whole artifact SHA is retained as metadata only. Signed URLs/headers are excluded.','artifactId':13049752897,'runId':41219441621,'sourceSha':'93414d5f26c829db552f8434433f6ad0684971f6','apiReportedZipBytes':size,'apiReportedZipSha256':'235a369b99a71fd207170b5222d5f1f5ab315ed271e0e0800d4cdf42a52c1ea5','members':[],'requests':requests,'status':'RUNNING'}
def save():out.write_text(json.dumps(r,indent=2)+'\n')
save();root.mkdir()
try:
    with zipfile.ZipFile(RangedFile()) as z:
        infos=z.infolist();r['centralDirectoryMemberCount']=len(infos)
        for name in names:
            if name not in z.namelist():
                r.setdefault('absent',[]).append(name);save();continue
            item=z.getinfo(name);p=PurePosixPath(name)
            assert not p.is_absolute() and '..' not in p.parts and '\\' not in name and p.suffix.lower() not in ['.exe','.dll','.zip','.pdb','.obj','.lib']
            assert item.file_size<=16*1024*1024 and item.compress_size<=1024*1024
            data=z.read(item);assert len(data)==item.file_size and zlib.crc32(data)&0xffffffff==item.CRC
            dest=root/name;dest.parent.mkdir(parents=True,exist_ok=True);dest.open('xb').write(data)
            r['members'].append({'path':name,'bytes':len(data),'sha256':sha(data),'zipCrc32':f'{item.CRC:08x}'})
            if name=='result.json':
                result=json.loads(data.decode('utf-8-sig'));r['originalBuildStatus']=result['status'];r['failedCommands']=[step['name'] for step in result['commands'] if not step['passed']]
                for label in r['failedCommands']:
                    assert label and all(c.isalnum() or c=='-' for c in label)
                    names.extend([f'logs/{label}.stdout.log',f'logs/{label}.stderr.log',f'logs/{label}.result.json'])
            save()
    ledger=json.loads((root/'artifact-sha256.json').read_bytes().decode('utf-8-sig'));by_name={row['path']:row for row in ledger}
    for member in r['members']:
        if member['path']=='artifact-sha256.json':continue
        assert by_name[member['path']]['bytes']==member['bytes'] and by_name[member['path']]['sha256']==member['sha256']
    r['selectedMemberLedger']='PASS';r['status']='PASS_SELECTED_ORIGINAL_MEMBERS';r['rangeTransferBytes']=sum(x['bytes'] for x in requests);save()
except Exception as error:
    r['status']='FAIL';r['errorKind']=type(error).__name__;save();raise RuntimeError('Range retrieval failed; inspect saved safe report') from None
print(json.dumps({'status':r['status'],'members':len(r['members']),'requests':len(requests),'rangeTransferBytes':r['rangeTransferBytes']}),flush=True)
