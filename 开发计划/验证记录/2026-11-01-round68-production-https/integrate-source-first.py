"""Copy the exact 47 validated files after validating every main preimage."""
from pathlib import Path
import datetime
import hashlib
import json
import subprocess

MAIN = Path('/workspace/共享/Eliora/未来教室计划')
EVIDENCE = Path('/tmp/fc-round68-production-https/evidence')
FREEZE = EVIDENCE / 'joint-implementation-freeze-seventh.json'
REPORT = EVIDENCE / 'main-integrated-source-equality-first.json'

def identity(path):
    if not path.exists():
        return None
    data = path.read_bytes()
    return {'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()}

def main():
    assert not REPORT.exists()
    frozen = json.loads(FREEZE.read_bytes())
    worktree = Path(frozen['worktree'])
    assert not subprocess.check_output(['git', 'status', '--porcelain'], cwd=MAIN)
    head = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=MAIN, text=True).strip()
    assert head == '452b41984443d5cacb8b34219c92b64f0287c2ae'
    assert len(frozen['files']) == 47
    for row in frozen['workingFiles']:
        assert identity(worktree / row['path']) == {'bytes': row['bytes'], 'sha256': row['sha256']}, row['path']
    for row in frozen['files']:
        assert identity(MAIN / row['path']) == row['before'], row['path']
    # Validate ALL preimages before the first mutation; never copy stale metadata.
    for row in frozen['files']:
        target = MAIN / row['path']
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes((worktree / row['path']).read_bytes())
    differences = []
    for row in frozen['workingFiles']:
        expected = {'bytes': row['bytes'], 'sha256': row['sha256']}
        actual = identity(MAIN / row['path'])
        if actual != expected:
            differences.append({'path': row['path'], 'main': actual, 'isolated': expected})
    assert {r['path'] for r in differences} == {'.gitattributes', '开发计划/10-实施进度与验证记录.md'}
    for row in frozen['files']:
        assert identity(MAIN / row['path']) == {'bytes': row['bytes'], 'sha256': row['sha256']}
    report = {
        'status': 'PASS_EXACT_VALIDATED_SOURCE_INTEGRATED',
        'createdUtc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
        'mainPreimageHead': head,
        'sourceFreeze': FREEZE.name,
        'sourceFreezeSha256': hashlib.sha256(FREEZE.read_bytes()).hexdigest(),
        'changedFiles': frozen['files'],
        'changedFileCount': 47,
        'guardedWorkingFileCount': len(frozen['workingFiles']),
        'scope': '595-file guard includes noncompiler metadata; actual build and runtime producers remain the isolated worktree. All Rust/Cargo/web/script bytes and all 47 changed files match. Main retains newer R67 metadata.',
        'nonCompilerMetadataDifferences': differences,
    }
    with REPORT.open('x') as output:
        json.dump(report, output, ensure_ascii=False, indent=2)
        output.write('\n')
    print(report['status'], report['changedFileCount'])

if __name__ == '__main__':
    main()
