"""Preserve complete failed-CI nonbinary originals; never execute candidate PEs."""
from pathlib import Path, PurePosixPath
import hashlib, json, stat, zipfile

root = Path(__file__).resolve().parent
source = '00120a026eea5dc4ff330f8526bf31169b373565'
run, job, artifact = 41296702693, 124818963251, 13080595231
size, digest = 44806575, 'c5cc184bf36ef951be4f39fe5e91fa45f6f44033bda59fe714bd92495e5e07dd'
assert (root / '.owner').read_bytes() == b'future-classroom-owned-r72-ci-evidence/v1\n'
def read(name): return json.loads((root / name).read_bytes())
def api(name):
    v = read(name)
    assert not v.get('isError', False)
    return v.get('structuredContent', v)
def sha(data): return hashlib.sha256(data).hexdigest()
download, operation = read('win7-download-original-first.json'), read('win7-download-operation-first.json')
assert download['status'] == operation['status'] == 'PASS' and operation['exitCode'] == 0
assert download['sourceSha'] == source and download['runId'] == run and download['artifactId'] == artifact
assert download['bytes'] == size and download['sha256'] == digest
runmeta = api('win7-run-finalfirst.json')
assert runmeta['id'] == run and runmeta['head_sha'] == source and runmeta['status'] == 'completed' and runmeta['conclusion'] == 'failure'
assert runmeta['path'] == '.github/workflows/windows-win7-msvc.yml'
jobs = api('win7-jobs-seventh.json')['jobs']
assert len(jobs) == 1 and jobs[0]['id'] == job and jobs[0]['run_id'] == run and jobs[0]['conclusion'] == 'failure' and jobs[0]['status'] == 'completed'
artifacts = api('win7-artifacts-first.json')['artifacts']
assert len(artifacts) == 1
a = artifacts[0]
assert a['id'] == artifact and a['size_in_bytes'] == size and a['digest'] == 'sha256:' + digest
assert a['workflow_run']['id'] == run and a['workflow_run']['head_sha'] == source
assert a['name'] == f'future-classroom-win7-msvc-i686-{source}-{run}-1' and a['expired'] is False
archive = root / 'win7-original.zip'
assert archive.is_file() and not archive.is_symlink()
blob = archive.read_bytes()
assert len(blob) == size and sha(blob) == digest
dest = root / 'win7-failed-original-raw-first'
report = root / 'win7-failed-original-raw-first.json'
assert not dest.exists() and not report.exists()
retained, excluded, seen = [], [], set()
with zipfile.ZipFile(archive) as z:
    infos = z.infolist()
    assert 0 < len(infos) <= 500 and sum(i.file_size for i in infos) <= 512 * 1024 * 1024
    for i in infos:
        name = i.filename
        path = PurePosixPath(name)
        assert name and not path.is_absolute() and '\\' not in name and ':' not in name and '..' not in path.parts
        assert name.casefold() not in seen and not stat.S_ISLNK(i.external_attr >> 16)
        seen.add(name.casefold())
        assert 0 <= i.file_size <= 256 * 1024 * 1024 and not i.flag_bits & 1
    dest.mkdir()
    for i in infos:
        if i.is_dir(): continue
        data = z.read(i)
        row = dict(path=i.filename, bytes=len(data), sha256=sha(data))
        if PurePosixPath(i.filename).suffix.lower() in {'.exe', '.dll', '.pdb', '.lib', '.obj', '.rlib', '.rmeta', '.a', '.o'}:
            row['reason'] = 'Failed candidate compiled payload retained only in exact original ZIP; not extracted or executed.'
            excluded.append(row)
            continue
        assert not data.startswith(b'MZ'), 'UNCLASSIFIED_PE_PAYLOAD'
        out = dest / i.filename
        out.parent.mkdir(parents=True, exist_ok=True)
        with out.open('xb') as stream: stream.write(data)
        assert out.read_bytes() == data
        retained.append(row)
    assert z.testzip() is None
for row in retained:
    data = (dest / row['path']).read_bytes()
    assert len(data) == row['bytes'] and sha(data) == row['sha256']
assert len(excluded) == 22 and all(r['path'].endswith('.exe') for r in excluded)
result = dict(status='PASS_FAILED_CI_RAW_PRESERVATION_ONLY', sourceSha=source, runId=run,
    jobId=job, artifactId=artifact, zip=dict(path=archive.name, bytes=size, sha256=digest),
    preservedFiles=retained, excludedCompiledPayloads=excluded,
    scope='Official failed Win7-target CI nonbinary bytes only. No static PASS, candidate execution or Win7/10 OS claim.')
with report.open('x') as stream: json.dump(result, stream, indent=2); stream.write('\n')
print(json.dumps(dict(status=result['status'], retained=len(retained), excluded=len(excluded), reportSha256=sha(report.read_bytes()))))
