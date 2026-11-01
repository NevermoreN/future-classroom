from pathlib import Path
import ast
import datetime
import hashlib
import json
import subprocess

root = Path('/workspace/共享/Eliora/未来教室计划')
owned = Path('/tmp/fc-round52-artifact-review-owned')
source = 'f6963919f100618fc482b1bff1a42dcbc571e610'
rel = '开发计划/验证记录/2026-11-01-round46-msvc/verify-ci-artifact-first.py'
original = (root / rel).read_bytes()
p = owned / 'verify-ci-artifact-first.py'
assert p.read_bytes() == original
original_git = subprocess.run(['git', 'show', source + ':' + rel], cwd=root, capture_output=True, check=True).stdout
assert original_git == original
ci = subprocess.run(['git', 'show', source + ':scripts/windows-msvc-ci.ps1'], cwd=root, capture_output=True, check=True).stdout
ci_sha = hashlib.sha256(ci).hexdigest()
assert ci_sha == 'ed26c9932325d377bbbf4eaf63aad29d75fc0cc8ad77561b1121f3389024c418'
tree = ast.parse(original, filename=str(p))
compile(tree, str(p), 'exec')  # compile-only, never execute helper globals/main

def arithmetic(node):
    if isinstance(node, ast.Constant) and type(node.value) is int:
        return node.value
    if isinstance(node, ast.BinOp) and isinstance(node.op, ast.Mult):
        return arithmetic(node.left) * arithmetic(node.right)
    raise ValueError('NOT_LITERAL_MULTIPLICATION')

assignments = {n.targets[0].id: n.value for n in tree.body if isinstance(n, ast.Assign)
               and len(n.targets) == 1 and isinstance(n.targets[0], ast.Name)}
assert ast.literal_eval(assignments['CI_SHA256']) == ci_sha
for name, expected in [('MAX_FILE', 64 * 1024 * 1024), ('MAX_HASHED', 256 * 1024 * 1024), ('MAX_META', 16 * 1024 * 1024)]:
    assert arithmetic(assignments[name]) == expected
text = original.decode()
assert all(str(n) not in text for n in [7508, 550, 6958])
main = next(n for n in tree.body if isinstance(n, ast.FunctionDef) and n.name == 'main')
parser_args = []
for n in ast.walk(main):
    if isinstance(n, ast.Call) and isinstance(n.func, ast.Attribute) and n.func.attr == 'add_argument':
        parser_args.extend(a.value for a in n.args if isinstance(a, ast.Constant) and isinstance(a.value, str))
assert all(name in parser_args for name in ['--source', '--baseline', '--zip', '--zip-bytes', '--zip-sha256',
                                          '--run-id', '--job-id', '--artifact-id', '--repo', '--objdump'])
helper_sha = hashlib.sha256(original).hexdigest()
review = {
    'schema': 'future-classroom.r52-artifact-helper-preparation/v1',
    'status': 'READY_BYTE_IDENTICAL_REUSE_AST_ONLY',
    'recordedUtc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
    'expectedSourceSha': source,
    'modernRunId': 41259909480,
    'win7RunId': 'AWAIT_ROOT_METADATA',
    'baselineJobsAndArtifacts': 'NOT_PROVIDED_NO_GUESSED_IDENTITIES',
    'source': {'path': str(root / rel), 'bytes': len(original), 'sha256': helper_sha, 'matchesExactGitBlobAtSource': True},
    'preparedHelper': {'path': str(p), 'bytes': len(original), 'sha256': helper_sha, 'logicChanges': False},
    'ciHelperAtPinnedSource': {'path': 'scripts/windows-msvc-ci.ps1', 'sha256': ci_sha, 'matchesValidatorRequiredSha': True},
    'checks': [{'status': 'PASS_STATIC_READ_REVIEW', 'scope': item} for item in [
        'Source/baseline/run/job/artifact/ZIP digest+size and official metadata are parameters; no historical fixed input/member/executable totals.',
        'Full Git metadata M is read at the exact --source tree; exact regular Git OID/mode/size/path metadata checked before and after.',
        'Actual checkout SHA256 S excludes exact ordinal historical archive prefix H; M=S union H is complete, disjoint, dynamically counted.',
        'Three manifests input-sha256/input-git-tree/input-domains; all six before/after objects agree; original raw digests bound to result/host.',
        'S matches exact Git blob or exact whole LF-to-CRLF only; mixed CR/NUL/arbitrary normalization rejected. H is not claimed full raw SHA.',
        'Complete producer ledger and ZIP set/CRC/size/SHA; traversal/case alias/symlink/encrypted member and payload bounds retained.',
        'Toolchain/buildstd/CRT/PS>=7.2/command arguments and physical command terminal results bound; command/PE counts derive from artifact/compiler/ledger.',
        'PE32/static CRT/Win7 subsystem6.1 are static evidence only; manual API/runtime requirements are not upgraded.',
        'Actual CI failure remains FAIL; PE and original cleanup phases independently attempted; runtime NOT_RUN.',
        '64MiB per file/256MiB SHA subset/10000 paths/16MiB metadata/512MiB payload bounds unchanged.',
        'No 7508/550/6958 constants. Fixed twelve workspace crates remains the intentional architecture requirement.',
    ]],
    'astCheck': {'pythonAstParse': 'PASS', 'compileAstOnly': 'PASS', 'helperMainOrSelftestExecuted': False, 'bytecodeWritten': False,
                 'firstCheckerFailurePreserved': 'preparation-ast-first-failed.json'},
    'deployment': {
        'reason': 'EV is __file__.resolve().parent; original output_path requires fresh outputs under EV.',
        'rootActionBeforeActualAudit': 'Copy unchanged real helper file (not symlink) into /tmp/fc-round52-ci-evidence/verify-ci-artifact-first.py and verify same SHA.',
        'runActualAuditOnlyAfter': 'Root supplies original ZIP and exact official terminal run/job/artifact metadata plus expected digest/bytes.',
        'suggestedFreshOutputs': {'Modern': 'modern-artifact-independent-first.json and modern-artifact-independent-first/',
                                  'Win7': 'win7-artifact-independent-first.json and win7-artifact-independent-first/'},
        'reuseWithoutLogicAdaptation': True,
    },
    'limitations': ['Preparation is not an artifact audit, CI terminal result or executable runtime evidence.',
                    'No downloads/network/remote mutation/Cargo/Windows/cleanup performed.',
                    'No new product code, main or historical original edits.',
                    'Retain failed reports and use fresh names, never overwrite.'],
}
q = owned / 'artifact-helper-review-first.json'
assert not q.exists()
q.write_text(json.dumps(review, indent=2) + '\n')
command = ['python3', '-B', '/tmp/fc-round52-ci-evidence/verify-ci-artifact-first.py', '--source', source,
           '--baseline', '<Modern|Win7>', '--zip', '<original.zip>', '--zip-bytes', '<official-bytes>',
           '--zip-sha256', '<official-64hex>', '--run-id', '<exact-run-id>', '--job-id', '<exact-job-id>',
           '--artifact-id', '<exact-artifact-id>', '--run-metadata', '<preserved-official-run.json>',
           '--job-metadata', '<preserved-official-jobs.json>', '--artifact-metadata', '<preserved-official-artifacts.json>',
           '--output-dir', '/tmp/fc-round52-ci-evidence/<fresh-baseline-audit-directory>',
           '--report', '/tmp/fc-round52-ci-evidence/<fresh-baseline-audit-report.json>', '--repo', str(root)]
(owned / 'audit-command-template-first.json').write_text(json.dumps({'status': 'TEMPLATE_NOT_EXECUTED', 'argv': command,
    'objdump': 'Default i686-w64-mingw32-objdump; explicit --objdump optional. Static inspection only.'}, indent=2) + '\n')
for f in [p, q]:
    print(json.dumps({'path': str(f), 'bytes': f.stat().st_size, 'sha256': hashlib.sha256(f.read_bytes()).hexdigest()}))
