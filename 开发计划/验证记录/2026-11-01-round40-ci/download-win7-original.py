from pathlib import Path
import subprocess,urllib.request,urllib.error,urllib.parse,json,hashlib,time,zipfile
root=Path('/tmp/fc-round40-ci-evidence');out=root/'win7-full-download-first.json';dest=Path('/tmp/fc-round40-win7-original.zip')
assert not out.exists() and not dest.exists()
expected=41254371;expected_sha='43587d8992441a5602c47d49b63dcc45f59ff34b7395a2882d9ffb8b2f8f201f'
r={'status':'RUNNING','scope':'Full exact original failed Win7 MSVC CI artifact download, with independent full ZIP SHA/CRC. Compiled targets are not target-OS runtime evidence. Plugin supplied artifact metadata/reference; native byte retrieval uses official API because no local file accessor is available for its opaque file reference. No signed URL, header, credential or URL-bearing exception text is retained.','runId':41240791635,'artifactId':13058086743,'sourceSha':'3b3d3abbfb80240989e32bc0858234d25bc277fc','expectedBytes':expected,'expectedSha256':expected_sha,'overallBudgetSeconds':1200,'socketTimeoutSeconds':25}
def save():out.write_text(json.dumps(r,indent=2)+'\n')
save();started=time.monotonic();count=0;h=hashlib.sha256()
class NoRedirect(urllib.request.HTTPRedirectHandler):
 def redirect_request(self,*args,**kwargs):return None
try:
 credentials=subprocess.run(['git','credential','fill'],input=b'protocol=https\nhost=github.com\n\n',capture_output=True,timeout=15,check=True)
 values=dict(line.split('=',1) for line in credentials.stdout.decode().splitlines() if '=' in line);token=values.get('password');assert token
 request=urllib.request.Request('https://api.github.com/repos/NevermoreN/future-classroom/actions/artifacts/13058086743/zip',headers={'Authorization':'Bearer '+token,'Accept':'application/vnd.github+json','X-GitHub-Api-Version':'2026-03-10','User-Agent':'FutureClassroom-validation'})
 try:response=urllib.request.build_opener(NoRedirect).open(request,timeout=25)
 except urllib.error.HTTPError as error:
  assert error.code==302
  location=error.headers.get('Location');parts=urllib.parse.urlsplit(location)
  assert parts.scheme=='https' and parts.hostname and (parts.hostname.endswith('.blob.core.windows.net') or parts.hostname.endswith('.actions.githubusercontent.com'))
  r['officialRedirectStatus']=302;save()
  response=urllib.request.urlopen(urllib.request.Request(location,headers={'Accept-Encoding':'identity'}),timeout=25)
 with response, dest.open('xb') as stream:
  assert response.status==200
  if response.headers.get('Content-Length'):assert int(response.headers['Content-Length'])==expected
  while True:
   assert time.monotonic()-started<1200
   data=response.read(min(256*1024,expected-count+1))
   if not data:break
   count+=len(data);assert count<=expected;stream.write(data);h.update(data)
  assert count==expected and h.hexdigest()==expected_sha
 with zipfile.ZipFile(dest) as z:assert z.testzip() is None;r['zipMemberCount']=len(z.infolist())
 r['status']='PASS'
except Exception as e:r.update(status='FAIL',errorKind=type(e).__name__)
finally:r.update(bytes=count,sha256=h.hexdigest(),elapsedSeconds=time.monotonic()-started);save()
print(json.dumps({k:r.get(k) for k in ['status','bytes','sha256','elapsedSeconds','errorKind','zipMemberCount']}),flush=True)
raise SystemExit(0 if r['status']=='PASS' else 1)
