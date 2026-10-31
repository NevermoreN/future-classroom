"""Preserve this exact CI failure/correction evidence; no Git or remote work."""
from pathlib import Path
import base64
import hashlib
import io
import json
import re
import shutil
import zipfile
from urllib.parse import parse_qs, urlsplit

SRC = Path('/tmp/fc-round30-evidence')
DST = Path.cwd() / '开发计划/验证记录'
PREFIX = '2026-10-31-round30-'
NAMES = ['ci-dispatch-second.json', 'ci-dispatch-second-once.py', 'git-ci-fix-sync.json',
         'ci-first-run-tools.json', 'ci-first-run-artifact.json',
         'ci-tool-resolution-before-source.json', 'ci-tool-resolution-after-source.json',
         'ci-tool-resolution-static.json', 'ci-tool-resolution-diffcheck.json',
         'ci-tool-resolution-readonly-probe.ps1', 'ci-tool-resolution-windows-readonly.json',
         'ci-tool-resolution-root-review.json']
SUMMARY = 'ci-tool-resolution-stage-summary.json'
sha = lambda data: hashlib.sha256(data).hexdigest()


def secret_check(data):
    assert not re.search(rb'(?:gh[pousr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{30,}|-----BEGIN (?:RSA |EC |OPENSSH |ENCRYPTED )?PRIVATE KEY-----)', data)
    for url in re.findall(r'https?://[^\s\"\x27<>\\]+', data.decode('utf-8', 'replace')):
        try:
            keys = {key.lower() for key in parse_qs(urlsplit(url).query)}
        except ValueError:
            continue
        assert not keys & {'sig', 'signature', 'token', 'access_token', 'jwt', 'x-amz-signature', 'x-goog-signature', 'expires', 'policy', 'key-pair-id'}


def embedded_check(value):
    if isinstance(value, dict):
        for key, child in value.items():
            if isinstance(child, str) and 'base64' in key.lower():
                data = base64.b64decode(child, validate=True)
                if data.startswith(b'PK'):
                    with zipfile.ZipFile(io.BytesIO(data)) as archive:
                        for name in archive.namelist():
                            secret_check(archive.read(name))
                else:
                    secret_check(data)
            embedded_check(child)
    elif isinstance(value, list):
        for child in value:
            embedded_check(child)


def main():
    files = [SRC / name for name in NAMES]
    raw = SRC / 'ci-first-run-artifact-raw'
    files += sorted(path for path in raw.rglob('*') if path.is_file())
    assert len(files) == 20 and len(set(files)) == 20
    values = {}
    for path in files:
        assert not path.is_symlink() and not path.name.endswith('.control.json')
        data = path.read_bytes(); secret_check(data)
        if path.suffix == '.json':
            value = json.loads(data); embedded_check(value); values[path.name] = value
    manifest = values['artifact-sha256.json']
    assert len(manifest) == 7 and len({row['path'] for row in manifest}) == 7
    for row in manifest:
        data = (raw / row['path']).read_bytes()
        assert len(data) == row['bytes'] and sha(data) == row['sha256']
    artifact = values['ci-first-run-artifact.json']
    zipped = base64.b64decode(artifact['zipBase64'], validate=True)
    assert len(zipped) == artifact['zipBytes'] and sha(zipped) == artifact['zipSha256']
    with zipfile.ZipFile(io.BytesIO(zipped)) as archive:
        assert archive.testzip() is None and len(archive.infolist()) == 8
        for row in artifact['files']:
            data = (raw / row['name']).read_bytes()
            assert data == base64.b64decode(row['base64'], validate=True) == archive.read(row['name'])
            assert len(data) == row['bytes'] and sha(data) == row['sha256']
    dispatch = values['ci-dispatch-second.json']
    expected = 'a54a41e5c4eea67efde983f31e63a1565dbf34ab'
    assert int(dispatch['runId']) == int(artifact['runId']) == 41204888955
    assert dispatch['sourceSha'] == expected and dispatch['headShaConfirmed'] is True
    assert values['result.json']['source_sha'] == values['bootstrap.json']['expected_source_sha'] == expected
    assert values['result.json']['status'] == 'FAILED_OR_INCOMPLETE'
    assert values['result.json']['commands'] == [] and values['result.json']['executable_count'] == 0
    assert values['cleanup.json']['status'] == 'OWNED_BUILD_ROOTS_REMOVED'
    assert all(row['removed'] is True for row in values['cleanup.json']['roots'])
    jobs = values['ci-first-run-tools.json']['jobs']['structuredContent']['jobs']
    assert len(jobs) == 1 and jobs[0]['conclusion'] == 'failure'
    assert [step['conclusion'] for step in jobs[0]['steps'][:6]] == ['success', 'success', 'success', 'failure', 'success', 'success']
    probe = values['ci-tool-resolution-windows-readonly.json']
    assert probe['exit_code'] == 0
    assert base64.b64decode(probe['stdout_base64'], validate=True).decode('utf-8') == probe['stdout']
    assert base64.b64decode(probe['stderr_base64'], validate=True).decode('utf-8') == probe['stderr']
    parsed = json.loads(probe['stdout'])
    after = values['ci-tool-resolution-after-source.json']
    after_bytes = base64.b64decode(after['base64'], validate=True)
    assert len(after_bytes) == after['bytes'] and sha(after_bytes) == after['sha256'] == '241b3afe56cd31075dc400e6808615f138b5f2b6c844e9c23a87cdeb0976cd81'
    assert parsed['sourceSha256'] == probe['source_sha256'] == after['sha256']
    assert parsed['status'] == 'PASS' and parsed['parserErrors'] == 0 and parsed['caseCount'] == len(parsed['cases']) == 14
    assert all(row['passed'] is True for row in parsed['cases'])
    assert sha((SRC / 'ci-tool-resolution-readonly-probe.ps1').read_bytes()) == probe['probe_sha256']
    review = values['ci-tool-resolution-root-review.json']
    assert review['source_sha256'] == after['sha256']
    assert review['probe_embedded_source_exact'] is review['production_helper_exact'] is review['seven_tool_bindings'] is True
    assert all(check['exit_code'] == 0 for check in review['checks'])
    ip = DST / (PREFIX + 'stage-progress-index.json'); original = ip.read_bytes(); index = json.loads(original)
    assert len(index['files']) == 460 and sha(original) == '27caf002bd1e08546943db94e142f7e95c4784b81681ee0375509bd7b3bd24e3'
    for row in index['files']:
        data = (DST / row['path']).read_bytes(); assert len(data) == row['bytes'] and sha(data) == row['sha256']
    prior = list(index['files']); mapping = []
    for path in files:
        data = path.read_bytes(); relative = PREFIX + str(path.relative_to(SRC))
        target = DST / relative
        assert not target.exists() or target.read_bytes() == data
        mapping.append({'original': str(path), 'archived': relative, 'bytes': len(data), 'sha256': sha(data)})
    summary = {'schemaVersion': 1, 'scope': 'Original failed modern MSVC CI followed by CI-only single-tool-PATH resolver fix; no new CI execution or platform acceptance inferred.',
               'run': {'id': 41204888955, 'headSha': expected, 'conclusion': 'failure', 'steps': [{'name': step['name'], 'conclusion': step['conclusion']} for step in jobs[0]['steps']]},
               'failure': {'status': 'FAILED_OR_INCOMPLETE', 'phase': 'first source SHA command, before toolchain install/compilation',
                           'reason': 'Three git.exe application matches were joined into one ProcessStartInfo.FileName.', 'commands': [], 'executableCount': 0,
                           'cleanup': values['cleanup.json']['status']},
               'artifactVerification': {'zipSha256': artifact['zipSha256'], 'zipBytes': artifact['zipBytes'], 'rawFiles': 8,
                                        'checksumManifestRows': 7, 'allManifestRowsAndZipBase64MembersMatchOriginalBytes': True},
               'resolverCorrection': {'sourceSha256': after['sha256'], 'scope': 'Only seven CI tool bindings resolve one validated PATH; no Rust behavior changes.'},
               'actualWindowsReadonly': {'powershell': parsed['powershell'], 'parserErrors': 0, 'cases': 14, 'exitCode': 0,
                                         'scope': parsed['scope'], 'stderr': 'Original CLIXML preparation progress preserved verbatim, not suppressed or rewritten.'},
               'rootIndependentReview': review,
               'preservation': {'originalFiles': mapping, 'tokenAndSignedUrlCheck': 'No concrete credential/private-key or signed-download-URL patterns in exact selected files and decoded source/artifact payloads. No signed URL saved.',
                                'oldIndexFiles': 460, 'oldIndexSha256': sha(original)},
               'notRun': ['No new CI after this resolver correction', 'No repeated Rust tests for CI-only script changes', 'No formal Windows7/10/11 or Seewo runtime acceptance'],
               'fullGoal': 'ACTIVE_NOT_COMPLETE'}
    sp = SRC / SUMMARY
    with sp.open('x') as output: output.write(json.dumps(summary, ensure_ascii=False, indent=2) + '\n')
    files.append(sp)
    existing = {row['path']: row for row in prior}
    for path in files:
        data = path.read_bytes(); relative = PREFIX + str(path.relative_to(SRC)); target = DST / relative
        row = {'path': relative, 'bytes': len(data), 'sha256': sha(data)}
        if target.exists(): assert target.read_bytes() == data
        else: target.parent.mkdir(parents=True, exist_ok=True); shutil.copyfile(path, target)
        assert target.read_bytes() == data
        if relative in existing: assert existing[relative] == row
        else: index['files'].append(row); existing[relative] = row
    assert index['files'][:460] == prior and ip.read_bytes() == original
    ip.write_text(json.dumps(index, ensure_ascii=False, indent=2) + '\n')
    for row in index['files']:
        data = (DST / row['path']).read_bytes(); assert len(data) == row['bytes'] and sha(data) == row['sha256']
    print(json.dumps({'status': 'ARCHIVE_ORIGINAL_BYTES_SHA_PASS', 'addedFiles': len(index['files']) - 460, 'indexedFiles': len(index['files']),
                      'indexedBytes': sum(row['bytes'] for row in index['files']), 'indexSha256': sha(ip.read_bytes()),
                      'manifestSha256': sha(json.dumps(index['files'], ensure_ascii=False, sort_keys=True, separators=(',', ':')).encode()),
                      'summary': str(DST / (PREFIX + SUMMARY)), 'summarySha256': sha(sp.read_bytes()), 'newFiles': [PREFIX + str(path.relative_to(SRC)) for path in files]}, ensure_ascii=False, indent=2))


if __name__ == '__main__':
    main()
