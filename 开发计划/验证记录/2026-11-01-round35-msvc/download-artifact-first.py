from pathlib import Path
import sys,json,hashlib,time,urllib.request,datetime

base=Path('/tmp/fc-round35-msvc-evidence');out=base/'artifact-download-first.json';dest=Path('/tmp/fc-round35-msvc-original.zip')
assert not out.exists() and not dest.exists()
expected_meta=json.loads((base/'artifact-expected.json').read_bytes());expected=expected_meta['bytes'];expected_sha=expected_meta['sha256']
assert 1<=expected<=128*1024*1024 and len(expected_sha)==64
url=sys.stdin.readline().strip();assert url.startswith('https://')
r={'scope':'Full modern MSVC source83fe CI artifact download. Runtime remains NOT_RUN until exact binaries are validated and actually executed on authorized Windows. Signed URL is intentionally excluded.','runId':41223081037,'sourceSha':'83fe847f1a5b1787bdf2390c1294ae4e740e6e75','artifactId':expected_meta['artifactId'],'expectedBytes':expected,'expectedSha256':expected_sha,'path':str(dest),'status':'RUNNING','originalOverallBudgetSeconds':1200,'socketTimeoutSeconds':25,'startedUtc':datetime.datetime.now(datetime.timezone.utc).isoformat()}
def save():out.write_text(json.dumps(r,indent=2)+'\n')
save();started=time.monotonic();count=0;h=hashlib.sha256();next_progress=1024*1024
try:
    request=urllib.request.Request(url,headers={'Accept-Encoding':'identity'})
    with urllib.request.urlopen(request,timeout=25) as response,dest.open('xb') as f:
        assert response.status==200 and int(response.headers['Content-Length'])==expected
        while True:
            assert time.monotonic()-started<1200,'Original overall download budget exhausted'
            chunk=response.read(min(256*1024,expected-count+1))
            if not chunk:break
            count+=len(chunk);assert count<=expected;h.update(chunk);f.write(chunk)
            if count>=next_progress:
                r.update(bytesSoFar=count,elapsedSeconds=time.monotonic()-started);save();next_progress=count+1024*1024
                print(json.dumps({'downloadedBytes':count,'expectedBytes':expected,'elapsedSeconds':r['elapsedSeconds']}),flush=True)
    assert count==expected and h.hexdigest()==expected_sha
    r['status']='PASS'
except Exception as e:
    r.update(status='FAIL',errorKind=type(e).__name__)
    # URL-bearing exception text is deliberately excluded.
finally:
    r.update(bytes=count,sha256=h.hexdigest(),elapsedSeconds=time.monotonic()-started);save()
print(json.dumps({k:r.get(k) for k in ['status','bytes','sha256','elapsedSeconds','errorKind']}),flush=True)
raise SystemExit(0 if r['status']=='PASS' else 1)
