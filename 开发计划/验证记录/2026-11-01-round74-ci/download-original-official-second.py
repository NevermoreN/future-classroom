"""Download exact official artifact bytes without persisting redirects or credentials."""
from pathlib import Path
import argparse, hashlib, json, subprocess, time, urllib.error, urllib.parse, urllib.request, zipfile

parser = argparse.ArgumentParser()
parser.add_argument('--baseline', choices=['modern','win7'], required=True)
parser.add_argument('--run-id', type=int, required=True)
parser.add_argument('--source-sha', required=True)
parser.add_argument('--artifact-id', type=int, required=True)
parser.add_argument('--expected-bytes', type=int, required=True)
parser.add_argument('--expected-sha256', required=True)
args = parser.parse_args()
root = Path(__file__).resolve().parent
assert (root/'.owner').read_bytes()==b'future-classroom-owned-r74-ci-evidence/v1\n'
assert len(args.source_sha)==40 and all(c in '0123456789abcdef' for c in args.source_sha)
workflow='windows-msvc' if args.baseline=='modern' else 'windows-win7-msvc'
dispatch=json.loads((root/('FutureClassroom-round74-'+workflow+'-dispatch.json')).read_text())
assert dispatch.get('status')=='RUN_IDENTIFIED' and dispatch.get('sourceSha')==args.source_sha and dispatch.get('runId')==args.run_id and dispatch.get('workflowPath')=='.github/workflows/'+workflow+'.yml'
dest = root / (args.baseline+'-official-original-second.zip')
out = root / (args.baseline+'-download-original-official-second.json')
assert not out.exists() and not dest.exists()
assert 0 < args.expected_bytes <= 512*1024*1024
assert len(args.expected_sha256)==64 and all(c in '0123456789abcdef' for c in args.expected_sha256)
r = {'status':'RUNNING','scope':'Exact bytes from official artifact API; plugin metadata separately retained. Connected file-URI transfer exceeded or cannot fit its original bounded budget; fresh official API fallback retains original attempt unchanged. Redirect URL and credentials stay only in memory. No product/test executable is run.', 'runId':args.run_id,'sourceSha':args.source_sha,'artifactId':args.artifact_id,'expectedBytes':args.expected_bytes,'expectedSha256':args.expected_sha256,'overallBudgetSeconds':1200,'socketTimeoutSeconds':25}
def save(): out.write_text(json.dumps(r,indent=2)+'\n')
class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self,*a,**kw): return None
save(); started=time.monotonic(); count=0; h=hashlib.sha256()
try:
    cred=subprocess.run(['git','credential','fill'],input=b'protocol=https\nhost=github.com\n\n',capture_output=True,timeout=15,check=True)
    values=dict(line.split('=',1) for line in cred.stdout.decode().splitlines() if '=' in line)
    token=values.get('password');assert token
    req=urllib.request.Request('https://api.github.com/repos/NevermoreN/future-classroom/actions/artifacts/'+str(args.artifact_id)+'/zip',headers={'Authorization':'Bearer '+token,'Accept':'application/vnd.github+json','X-GitHub-Api-Version':'2026-03-10','User-Agent':'FutureClassroom-validation'})
    try:
        response=urllib.request.build_opener(NoRedirect).open(req,timeout=25)
    except urllib.error.HTTPError as error:
        assert error.code==302
        location=error.headers.get('Location'); parts=urllib.parse.urlsplit(location)
        assert parts.scheme=='https' and parts.hostname and (parts.hostname.endswith('.blob.core.windows.net') or parts.hostname.endswith('.actions.githubusercontent.com'))
        r['officialRedirectStatus']=302;save()
        # Never forward the GitHub credential to a redirect host.
        response=urllib.request.urlopen(urllib.request.Request(location,headers={'Accept-Encoding':'identity'}),timeout=25)
    with response,dest.open('xb') as stream:
        assert response.status==200
        if response.headers.get('Content-Length'): assert int(response.headers['Content-Length'])==args.expected_bytes
        while True:
            assert time.monotonic()-started<1200
            data=response.read(min(256*1024,args.expected_bytes-count+1))
            if not data: break
            count+=len(data);assert count<=args.expected_bytes
            stream.write(data);h.update(data)
        assert count==args.expected_bytes and h.hexdigest()==args.expected_sha256
    with zipfile.ZipFile(dest) as z:
        assert z.testzip() is None
        r['zipMemberCount']=len(z.infolist())
    r['status']='PASS'
except Exception as error:
    r['status']='FAIL';r['errorKind']=type(error).__name__
finally:
    r.update(bytes=count,sha256=h.hexdigest(),elapsedSeconds=time.monotonic()-started)
    save()
print(json.dumps({k:r.get(k) for k in ['status','bytes','sha256','elapsedSeconds','errorKind','zipMemberCount']}),flush=True)
raise SystemExit(0 if r['status']=='PASS' else 1)
