from pathlib import Path,PurePosixPath
import os,io,json,urllib.request,hashlib,zipfile,zlib
url=os.environ.pop('FC_CI_ARTIFACT_URL')
base=Path('/tmp/fc-round30-evidence');out=base/'ci-sixth-range-originals.json';root=base/'ci-sixth-range-raw'
assert not out.exists() and not root.exists()
size=37664420;requests=[];etag=None
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
names=['logs/pe-imports-0.stdout.log','logs/pe-imports-0.stderr.log','logs/pe-imports-0.result.json','failure.json','result.json','cleanup.json','bootstrap.json','logs/cargo-metadata.stdout.log','logs/workspace-test-compile.stdout.log','logs/product-compile.stdout.log','logs/rust-target-cfg.stdout.log']
r={'scope':'Selected original nonbinary ZIP members read via validated HTTP206 ranges and ZIP member CRC. This is NOT a whole-archive SHA validation or an EXE/runtime/import recheck. API/upload-reported whole artifact SHA is retained as metadata only. Signed URLs/headers are excluded.','artifactId':13045459617,'runId':41209904161,'sourceSha':'6bde62ccf123ce4ed397c354f56d23d2dc9ee3b9','apiReportedZipBytes':size,'apiReportedZipSha256':'a92b7cfea5b9aa548366679feae0645bd52160c045072ab8ab5a99d2eb01347d','members':[],'requests':requests,'status':'RUNNING'}
def save():out.write_text(json.dumps(r,indent=2)+'\n')
save();root.mkdir()
try:
    with zipfile.ZipFile(RangedFile()) as z:
        infos=z.infolist();r['centralDirectoryMemberCount']=len(infos)
        for name in names:
            item=z.getinfo(name);p=PurePosixPath(name)
            assert not p.is_absolute() and '..' not in p.parts and '\\' not in name and p.suffix.lower() not in ['.exe','.dll','.zip','.pdb','.obj','.lib']
            assert item.file_size<=4*1024*1024 and item.compress_size<=1024*1024
            data=z.read(item);assert len(data)==item.file_size and zlib.crc32(data)&0xffffffff==item.CRC
            dest=root/name;dest.parent.mkdir(parents=True,exist_ok=True);dest.write_bytes(data)
            r['members'].append({'path':name,'bytes':len(data),'sha256':sha(data),'zipCrc32':f'{item.CRC:08x}'})
            save()
    r['status']='PASS_SELECTED_ORIGINAL_MEMBERS';r['rangeTransferBytes']=sum(x['bytes'] for x in requests);save()
except Exception as error:
    r['status']='FAIL';r['errorKind']=type(error).__name__;save();raise RuntimeError('Range retrieval failed; inspect saved safe report') from None
print(json.dumps({'status':r['status'],'members':len(r['members']),'requests':len(requests),'rangeTransferBytes':r['rangeTransferBytes']}),flush=True)
print((root/'logs/pe-imports-0.stdout.log').read_text(errors='replace'),flush=True)
