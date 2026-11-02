"""Offline READONLY publication codec. Captured source data is not installed trust.
No Windows process/API or permission construction; fixed schema SQL only.
"""
from pathlib import Path
import ctypes as C,ctypes.util,struct,hashlib,json,re
ROOT=Path('/tmp/fc-round80-controlled-repair/inner-guard-probe-tools-first/root/usr/lib/x86_64-linux-gnu') # exact controlled local builder tool domain
LIMIT=64*1024*1024;DOMAIN=b'FC-SAME-SOURCE-DB-v1\0'
def u32(n):
 assert type(n)is int and 0<=n<=0xffffffff
 return struct.pack('<I',n)
def text(s):b=s.encode('utf8');return u32(len(b))+b
def schema_from_rust(source):
 result=[]
 for block in source.split('    Table {')[1:]:
  name=re.search(r'name: "([A-Za-z0-9_]+)"',block)[1];query=re.search(r'query: "([^"]+)"',block)[1]
  cols=re.findall(r'\("([A-Za-z0-9_]+)", "([sSlLiIvV][0-9]+)"\)',block.split('    },')[0])
  assert query=='SELECT * FROM `'+name+'`' and 1<=len(cols)<=32
  result.append((name,cols))
 assert len(result)==31 and [n for n,c in result]==sorted(n for n,c in result)
 return result
class API:
 def __init__(self):
  C.CDLL(str(ROOT/'libgsf-1.so.114'),mode=C.RTLD_GLOBAL);self.m=C.CDLL(str(ROOT/'libmsi-1.0.so.0'));self.g=C.CDLL(ctypes.util.find_library('glib-2.0'));self.o=C.CDLL(ctypes.util.find_library('gobject-2.0'));self.io=C.CDLL(ctypes.util.find_library('gio-2.0'))
  def fn(lib,name,ret,*args):f=getattr(lib,name);f.restype=ret;f.argtypes=list(args);return f
  p=C.c_void_p;u=C.c_uint;e=C.POINTER(p)
  self.db=fn(self.m,'libmsi_database_new',p,C.c_char_p,u,C.c_char_p,e)
  self.readonly=fn(self.m,'libmsi_database_is_readonly',C.c_int,p)
  self.new=fn(self.m,'libmsi_query_new',p,p,C.c_char_p,e)
  self.execute=fn(self.m,'libmsi_query_execute',C.c_int,p,p,e)
  self.fetch=fn(self.m,'libmsi_query_fetch',p,p,e)
  self.close=fn(self.m,'libmsi_query_close',C.c_int,p,e)
  self.col=fn(self.m,'libmsi_query_get_column_info',p,p,C.c_int,e)
  self.count=fn(self.m,'libmsi_record_get_field_count',u,p)
  self.null=fn(self.m,'libmsi_record_is_null',C.c_int,p,u)
  self.string=fn(self.m,'libmsi_record_get_string',p,p,u)
  self.integer=fn(self.m,'libmsi_record_get_int',C.c_int,p,u)
  self.stream=fn(self.m,'libmsi_record_get_stream',p,p,u)
  self.read=fn(self.io,'g_input_stream_read',C.c_ssize_t,p,p,C.c_size_t,p,e)
  self.close_stream=fn(self.io,'g_input_stream_close',C.c_int,p,p,e)
  self.free=fn(self.g,'g_free',None,p);self.unref=fn(self.o,'g_object_unref',None,p)
 def call(self,fn,*args):
  e=C.c_void_p();v=fn(*args,C.byref(e));assert not e.value,'LIBMSI_KNOWN_ERROR';return v
 def getstr(self,row,i):
  p=self.string(row,i);assert p
  try:return C.string_at(p).decode('utf8','strict')
  finally:self.free(p)
 def query(self,db,query,columns):
  v=self.call(self.new,db,query.encode());assert v
  try:
   assert self.call(self.execute,v,None)
   for which,index in [(0,0),(1,1)]:
    r=self.call(self.col,v,which);assert r
    try:assert self.count(r)==len(columns) and [self.getstr(r,i+1)for i in range(len(columns))]==[c[index]for c in columns]
    finally:self.unref(r)
   rows=[]
   while True:
    r=self.call(self.fetch,v)
    if not r:break
    try:
     assert len(rows)<72;fields=[]
     for i,(name,kind)in enumerate(columns,1):
      if self.null(r,i):fields.append(None);continue
      k=kind[0].lower()
      if k=='i':value=self.integer(r,i);assert value!=-2147483648;fields.append(value)
      elif k in ('s','l'):
       value=self.getstr(r,i);assert len(value.encode('utf-16-le'))//2<=4096;fields.append(value)
      elif k=='v':
       stream=self.stream(r,i);assert stream;data=bytearray()
       try:
        buf=C.create_string_buffer(16384)
        while True:
         n=self.call(self.read,stream,buf,16384,None);assert n>=0
         if not n:break
         data+=buf.raw[:n];assert len(data)<=LIMIT
        assert self.call(self.close_stream,stream,None)
       finally:self.unref(stream)
       fields.append(bytes(data))
      else:raise AssertionError('UNKNOWN_FIELD_TYPE')
     rows.append(fields)
    finally:self.unref(r)
   return rows
  finally:
   assert self.call(self.close,v);self.unref(v)
def field(kind,value):
 if value is None:return b'\1'
 k=kind[0].lower();prefix=b'\0'
 if k=='i':return prefix+struct.pack('<i',value)
 if k in ('s','l'):return prefix+text(value)
 if k=='v':return prefix+u32(len(value))+value
 raise AssertionError('UNKNOWN_FIELD_TYPE')
def capture(path,expected_sha,schema):
 path=Path(path);before=path.read_bytes();assert 0<len(before)<=LIMIT and hashlib.sha256(before).hexdigest()==expected_sha
 api=API();db=api.call(api.db,str(path).encode(),1,None);assert db and api.readonly(db)
 try:
  storage=api.call(api.new,db,b'SELECT `Name` FROM `_Storages`');assert storage
  try:
   assert api.call(api.execute,storage,None);row=api.call(api.fetch,storage)
   if row:api.unref(row);raise AssertionError('UNSUPPORTED_EMBEDDED_STORAGE')
  finally:assert api.call(api.close,storage);api.unref(storage)
  names=api.query(db,'SELECT `Name` FROM `_Tables`',[('Name','s64')]);assert sorted(row[0]for row in names)==[n for n,c in schema],'UNKNOWN_OR_MISSING_TABLE'
  data=[]
  for name,cols in schema:data.append((name,cols,api.query(db,'SELECT * FROM `'+name+'`',cols)))
 finally:api.unref(db)
 assert path.read_bytes()==before,'SOURCE_CHANGED'
 return data
def signature(tables):
 all=bytearray(DOMAIN);facts=[]
 for name,cols,rows in tables:
  assert len(rows)<=72;digests=sorted(hashlib.sha256(b''.join(field(k,v)for (_,k),v in zip(cols,row))).digest()for row in rows)
  all+=text(name)
  for n,k in cols:all+=text(n)+text(k)
  all+=u32(len(rows))+b''.join(digests);facts.append({'table':name,'rows':len(rows),'schema':cols,'sortedRowHashes':[v.hex()for v in digests]})
 return hashlib.sha256(all).hexdigest(),facts,bytes(all)
def capture_wire(tables):
 b=bytearray(b'FCDB-CAPTURE-v1\0'+u32(len(tables)))
 for name,cols,rows in tables:
  b+=text(name)+u32(len(cols))
  for n,k in cols:b+=text(n)+text(k)
  b+=u32(len(rows))
  for row in rows:
   for (_,k),v in zip(cols,row):b+=field(k,v)
 assert len(b)<=2*LIMIT
 return bytes(b)


"""Pure bounded publication-record FORMAT, not publication/admission authority.
Builder must independently bind normal producer, exact immutable template/guard,
source MSI and matching actually observed cache signature before sealing a release.
"""
import struct,re
MAGIC=b'FCPRED2\0';HEADER=248;MAX_ENVELOPE=88+32768
DIGEST_KEYS=['source_manifest_sha','producer_sha','source_msi_sha','template_sha','guard_sha','logical_sha']
def encode_format(meta,envelope,validate_envelope):
 assert set(meta)=={'target','source_commit','template_bytes',*DIGEST_KEYS}
 assert type(meta['target'])is int and meta['target'] in (1,2)
 assert type(meta['template_bytes'])is int and 196<=meta['template_bytes']<=64*1024*1024
 assert type(envelope)is bytes and 1<=len(envelope)<=MAX_ENVELOPE
 validate_envelope(envelope)
 h=bytearray(HEADER);h[:8]=MAGIC;h[8:10]=struct.pack('<H',2);h[10:12]=struct.pack('<H',2);h[12]=meta['target']
 commit=meta['source_commit'];assert re.fullmatch('[0-9a-f]{40}',commit) and bytes.fromhex(commit)!=bytes(20);h[16:36]=bytes.fromhex(commit)
 for n,key in enumerate(DIGEST_KEYS):
  value=meta[key];assert re.fullmatch('[0-9a-f]{64}',value) and bytes.fromhex(value)!=bytes(32);h[36+32*n:68+32*n]=bytes.fromhex(value)
 h[228:232]=struct.pack('<I',meta['template_bytes']);h[232:236]=struct.pack('<I',len(envelope));h[236:240]=struct.pack('<I',3)
 return bytes(h)+envelope
