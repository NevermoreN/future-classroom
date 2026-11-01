"""Transfer a connected GitHub file reference without retaining its signed URL."""
from pathlib import Path
import argparse,hashlib,json,os,time,urllib.request,urllib.parse,urllib.error,zipfile
p=argparse.ArgumentParser()
p.add_argument('--baseline',choices=['modern','win7'],required=True)
p.add_argument('--source-sha',required=True);p.add_argument('--run-id',type=int,required=True);p.add_argument('--artifact-id',type=int,required=True)
p.add_argument('--expected-bytes',type=int,required=True);p.add_argument('--expected-sha256',required=True);p.add_argument('--connector-response-file',type=Path,required=True)
a=p.parse_args();E=Path(__file__).resolve().parent
assert (E/'.owner').read_bytes()==b'future-classroom-owned-r74-ci-evidence/v1\n'
assert len(a.source_sha)==40 and all(c in '0123456789abcdef' for c in a.source_sha)
assert len(a.expected_sha256)==64 and all(c in '0123456789abcdef' for c in a.expected_sha256)
assert 0<a.expected_bytes<=512*1024*1024
dispatch=json.loads((E/('FutureClassroom-round74-'+('windows-msvc' if a.baseline=='modern' else 'windows-win7-msvc')+'-dispatch.json')).read_bytes())
assert dispatch['sourceSha']==a.source_sha and dispatch['runId']==a.run_id and dispatch['status']=='RUN_IDENTIFIED'
private=a.connector_response_file;assert private.parent==E and private.name==a.baseline+'-plugin-download-first.json' and not private.is_symlink()
assert private.stat().st_mode&0o777==0o600
payload=json.loads(private.read_bytes());s=payload['structuredContent'];assert s['artifact_id']==a.artifact_id and s['file_uri']['mime_type']=='application/zip'
url=s['file_uri']['download_url'];parts=urllib.parse.urlsplit(url)
assert parts.scheme=='https' and parts.hostname and parts.hostname.endswith('.oaiusercontent.com') and not parts.username and not parts.password and parts.port in (None,443)
file_id=s['file_uri']['file_id'];private.unlink();del payload,s
dest=E/(a.baseline+'-original.zip');report=E/(a.baseline+'-download-original-first.json');assert not dest.exists() and not report.exists()
v=dict(status='RUNNING',scope='Exact official GitHub artifact digest/bytes fetched through connected GitHub plugin file reference. Signed URI remains memory-only and is never logged. Original response removed before transfer.',sourceSha=a.source_sha,runId=a.run_id,artifactId=a.artifact_id,expectedBytes=a.expected_bytes,expectedSha256=a.expected_sha256,overallBudgetSeconds=1200,socketTimeoutSeconds=25,connectorFileId=file_id,privateResponseRemoved=not private.exists())
def save():report.write_text(json.dumps(v,indent=2)+'\n')
class NoRedirect(urllib.request.HTTPRedirectHandler):
 def redirect_request(self,*args,**kwargs):return None
save();start=time.monotonic();count=0;h=hashlib.sha256()
try:
 request=urllib.request.Request(url,headers={'Accept-Encoding':'identity'})
 with urllib.request.build_opener(NoRedirect).open(request,timeout=25) as response,dest.open('xb') as out:
  assert response.status==200
  if response.headers.get('Content-Length'):assert int(response.headers['Content-Length'])==a.expected_bytes
  while True:
   assert time.monotonic()-start<1200
   data=response.read(min(256*1024,a.expected_bytes-count+1))
   if not data:break
   count+=len(data);assert count<=a.expected_bytes;out.write(data);h.update(data)
 assert count==a.expected_bytes and h.hexdigest()==a.expected_sha256
 with zipfile.ZipFile(dest) as z:assert z.testzip() is None;v['zipMemberCount']=len(z.infolist())
 v['status']='PASS'
except Exception as error:
 v['status']='FAIL';v['errorKind']=type(error).__name__
 if isinstance(error,urllib.error.HTTPError):v['httpStatus']=error.code
finally:
 v.update(bytes=count,sha256=h.hexdigest(),elapsedSeconds=time.monotonic()-start);save()
print(json.dumps({k:v.get(k) for k in ('status','bytes','sha256','elapsedSeconds','errorKind','zipMemberCount','privateResponseRemoved')}))
raise SystemExit(0 if v['status']=='PASS' else 1)
