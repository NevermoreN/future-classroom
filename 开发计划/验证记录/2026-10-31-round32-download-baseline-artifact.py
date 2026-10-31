from pathlib import Path
import sys,json,hashlib,time,urllib.request,datetime

base=Path('/tmp/fc-round32-deadline-evidence')
out=base/'baseline-artifact-download.json';dest=Path('/tmp/fc-round32-baseline-msvc.zip')
assert not out.exists() and not dest.exists()
url=sys.stdin.readline().strip();assert url.startswith('https://')
expected=39499461;expected_sha='1388e4c20fc34ea1b2943c9aa03d53095ebe707399bf9ca4e45dec0935ce3886'
r={'scope':'Full baseline CI artifact download, not feature runtime evidence; signed URL intentionally excluded','artifactId':13047332030,'runId':41212282071,'sourceSha':'c622aea0f178e42dcc776afbea96c4b2dfe43ce9','expectedBytes':expected,'expectedSha256':expected_sha,'path':str(dest),'status':'RUNNING','startedUtc':datetime.datetime.now(datetime.timezone.utc).isoformat()}
def save():out.write_text(json.dumps(r,indent=2)+'\n')
save();started=time.monotonic();count=0;h=hashlib.sha256()
try:
    request=urllib.request.Request(url,headers={'Accept-Encoding':'identity'})
    with urllib.request.urlopen(request,timeout=25) as response,dest.open('xb') as f:
        assert response.status==200
        assert int(response.headers['Content-Length'])==expected
        while True:
            assert time.monotonic()-started<300,'Original overall download budget exhausted'
            chunk=response.read(min(256*1024,expected-count+1))
            if not chunk:break
            count+=len(chunk);assert count<=expected
            h.update(chunk);f.write(chunk)
    assert count==expected and h.hexdigest()==expected_sha
    r['status']='PASS'
except Exception as e:
    r.update(status='FAIL',errorKind=type(e).__name__)
    # Exceptions may contain the signed URL; never serialize their text.
finally:
    r.update(bytes=count,sha256=h.hexdigest(),elapsedSeconds=time.monotonic()-started)
    save()
print(json.dumps({k:r.get(k) for k in ['status','bytes','sha256','elapsedSeconds','errorKind']}),flush=True)
raise SystemExit(0 if r['status']=='PASS' else 1)
