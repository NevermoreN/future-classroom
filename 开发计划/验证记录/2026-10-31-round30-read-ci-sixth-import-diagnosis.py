from pathlib import Path,PurePosixPath
import os,io,json,urllib.request,hashlib,zipfile,zlib,struct
url=os.environ.pop('FC_CI_ARTIFACT_URL');base=Path('/tmp/fc-round30-evidence')
out=base/'ci-sixth-import-diagnosis.json';root=base/'ci-sixth-import-diagnosis-raw'
assert not out.exists() and not root.exists()
size=37664420;requests=[];etag=None;cache=[]
def sha(b):return hashlib.sha256(b).hexdigest()
r={'scope':'Resume missing nonbinary members after prior terminal URLError; inspect four original compiler EXEs solely in memory. HTTP206 ranges/unchanged ETag/member CRC prove selected bytes, not whole ZIP digest. No EXE execution, binary file, ZIP or binary base64 saved.','artifactId':13045459617,'runId':41209904161,'sourceSha':'6bde62ccf123ce4ed397c354f56d23d2dc9ee3b9','requests':requests,'members':[],'status':'RUNNING'}
def save():out.write_text(json.dumps(r,indent=2)+'\n')
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
        assert 0<length<=1024*1024
        start=self.position;end=start+length
        for offset,data in cache:
            if offset<=start and end<=offset+len(data):
                self.position=end;return data[start-offset:end-offset]
        begin=start//65536*65536;finish=min(size,max(end,begin+65536));count=finish-begin
        r['pendingRange']={'start':begin,'end':finish-1};save()
        headers={'Range':f'bytes={begin}-{finish-1}','Accept-Encoding':'identity'}
        if etag:headers['If-Match']=etag
        with urllib.request.urlopen(urllib.request.Request(url,headers=headers),timeout=25) as response:
            assert response.status==206 and response.headers['Content-Range']==f'bytes {begin}-{finish-1}/{size}'
            current=response.headers.get('ETag');assert current
            if etag is None:etag=current
            assert current==etag
            data=response.read(count+1);assert len(data)==count
        cache.append((begin,data));requests.append({'start':begin,'end':finish-1,'status':206,'bytes':len(data),'sha256':sha(data)})
        r.pop('pendingRange',None);save();self.position=end;return data[start-begin:end-begin]
def imports(data):
    assert data[:2]==b'MZ';pe=struct.unpack_from('<I',data,0x3c)[0]
    assert data[pe:pe+4]==b'PE\0\0' and struct.unpack_from('<H',data,pe+4)[0]==0x14c
    count=struct.unpack_from('<H',data,pe+6)[0];optional=pe+24
    assert struct.unpack_from('<H',data,optional)[0]==0x10b
    sections=optional+struct.unpack_from('<H',data,pe+20)[0]
    def offset(rva):
        for i in range(count):
            vsize,va,rawsize,pointer=struct.unpack_from('<4I',data,sections+i*40+8)
            if va<=rva<va+max(vsize,rawsize):
                result=pointer+rva-va;assert result<len(data);return result
        raise ValueError('Unmapped RVA')
    table,size_import=struct.unpack_from('<2I',data,optional+104)
    if table==0:return []
    start=offset(table);names=[]
    for i in range(512):
        descriptor=struct.unpack_from('<5I',data,start+i*20)
        if not any(descriptor):return names
        p=offset(descriptor[3]);end=data.index(b'\0',p,min(p+256,len(data)));names.append(data[p:end].decode('ascii'))
    raise ValueError('Unbounded import table')
names=['logs/workspace-test-compile.stdout.log','logs/product-compile.stdout.log','logs/rust-target-cfg.stdout.log',
 'tests/fc-bootstrap/verify_web_assets-79070da627d35f91.exe','tests/fc-domain/fc_domain-a8487b286decf594.exe','tests/fc-protocol/fc_protocol-a6a537c325d921e7.exe','tests/fc-ports/fc_ports-4960846507d5ec27.exe']
save();root.mkdir()
try:
    with zipfile.ZipFile(RangedFile()) as z:
        r['centralDirectoryMemberCount']=len(z.infolist())
        for name in names:
            item=z.getinfo(name);assert item.file_size<=4*1024*1024 and item.compress_size<=1024*1024
            data=z.read(item);assert len(data)==item.file_size and zlib.crc32(data)&0xffffffff==item.CRC
            row={'path':name,'bytes':len(data),'sha256':sha(data),'zipCrc32':f'{item.CRC:08x}'}
            if name.endswith('.exe'):
                row['binaryHandling']='MEMORY_ONLY_NOT_EXECUTED_NOT_SAVED';row['imports']=imports(data)
            else:
                dest=root/name;dest.parent.mkdir(parents=True,exist_ok=True);dest.write_bytes(data)
            r['members'].append(row);save()
    r['status']='PASS_SELECTED_BYTES_AND_IMPORT_DIAGNOSIS';r['rangeTransferBytes']=sum(x['bytes'] for x in requests);save()
except Exception as error:
    r['status']='FAIL';r['errorKind']=type(error).__name__;r['underlyingReasonKind']=type(getattr(error,'reason',None)).__name__;save();raise RuntimeError('Inspect safe report; previous range/member bytes retained') from None
print(json.dumps({'status':r['status'],'requests':len(requests),'rangeTransferBytes':r['rangeTransferBytes'],'programs':[x for x in r['members'] if x['path'].endswith('.exe')]}),flush=True)
