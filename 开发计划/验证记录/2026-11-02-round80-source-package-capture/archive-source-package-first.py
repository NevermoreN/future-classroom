#!/usr/bin/env python3
"""Finite evidence only; default inert. --verify-only replays without original R inputs."""
from pathlib import Path
import argparse, gzip, hashlib, io, json, tarfile

M = Path('/workspace/共享/Eliora/未来教室计划')
R = Path('/tmp/fc-round80-controlled-repair')
D = M / '开发计划/验证记录/2026-11-02-round80-source-package-capture'
BACKEND = M / '开发计划/验证记录/2026-11-02-round79-msi-maintenance/evidence-originals-first.tar.gz'
MEMBER = 'archive-preparation-second/archive-second.py'
BACKEND_SHA = 'da2906c450ea8c2ac7d18e358507023f519dbcc6faf1a9ac1abb1ae16af8c30f'
RUNTIME_SHA = '6aefdbe85d887a2202d579ab6a38936810e6a259f16b33eb196b3b8e370cbcc5'
PROMOTE = [
 'inner-guard-abi2-implementation-first/freeze-first.json',
 'inner-guard-abi2-implementation-first/implementation-second.json',
 'inner-guard-abi2-implementation-first/native-checks-first.json',
 'inner-guard-abi2-implementation-first/native-checks-fourth.json',
 'source-package-reader-implementation-first/freeze-first.json',
 'source-package-reader-implementation-first/implementation-first.json',
 'source-package-reader-implementation-first/native-checks-first.json',
 'source-package-probe-implementation-first/freeze-first.json',
 'source-package-probe-implementation-first/implementation-first.json',
 'source-package-probe-implementation-first/producer-wrapper-failure-first.json',
 'source-package-probe-implementation-first/producer-report-third.json',
 'source-package-probe-fixtures-first/freeze-first.json',
 'source-package-probe-fixtures-first/report.json',
 'source-package-probe-runtime-preparation-first/freeze-first.json',
 'source-package-probe-runtime-local-checks-first/report.json',
 'source-package-probe-runtime-first/report.json',
 'root-source-package-probe-ps-parse-first.json',
 'source-package-integrated-checks-first/freeze-first.json',
 'source-package-integrated-checks-first/report-first.json',
 'source-package-integrated-preservation-first/freeze-first.json',
 'source-package-integrated-preservation-first/report.json',
]

def need(ok, label):
 if not ok: raise AssertionError(label)

def sha(b): return hashlib.sha256(b).hexdigest()
def raw(j): return (json.dumps(j, ensure_ascii=False, indent=2) + '\n').encode()
def safe(name):
 p = Path(name)
 need(not p.is_absolute() and p.as_posix() == name and '..' not in p.parts and name != '.', 'CANONICAL_RELATIVE_PATH')
 return p

def original(row):
 p = R / safe(row['path'])
 need(p.is_file() and not p.is_symlink() and p.resolve() == p, 'EXACT_REGULAR_ORIGINAL')
 b = p.read_bytes()
 need(len(b) == row['bytes'] and sha(b) == row['sha256'], 'EXACT_ORIGINAL_BYTE_SHA_' + row['path'])
 return b

def scanning(forms):
 with tarfile.open(BACKEND, 'r:gz') as t:
  info = t.getmember(MEMBER)
  need(info.isfile() and info.size == 26272, 'FROZEN_SCANNER_MEMBER')
  b = t.extractfile(info).read()
 need(sha(b) == BACKEND_SHA, 'FROZEN_SCANNER_SHA')
 ns = {'__name__': 'finite_source_package_existing_scanner'}
 exec(compile(b, '<frozen-strict-scanner>', 'exec'), ns)
 table = {r['path']: r for r in forms['files']}
 need(len(table) == len(forms['files']) == 11, 'EXACT_11_STRUCTURAL_OR_DERIVED_VIEWS')
 ns['TEXT_FORMS'] = {k: v for k, v in table.items() if v['textForm'] == 'EXACT_UTF8_LICENSE_WITH_18_FORM_FEEDS'}
 ns['TEXT_DERIVED_VIEWS'] = {}
 previous = ns['reviewed_control_views']
 def reviewed(data, label):
  relative = None
  for prefix in ('source:', 'stored-tar-gzip-decoded:', 'promoted:'):
   if isinstance(label, str) and label.startswith(prefix): relative = label[len(prefix):]
  row = table.get(relative)
  if row is None or row['textForm'] == 'EXACT_UTF8_LICENSE_WITH_18_FORM_FEEDS': return previous(data, label)
  need(len(data) == row['bytes'] and sha(data) == row['sha256'], 'EXACT_STRUCTURAL_VIEW_BYTES_SHA')
  text = data.decode('utf-8'); kind = row['textForm']
  if kind == 'EXACT_MSI_STREAM_NAMES':
   control = '\x05'
   need(text in ('payload.cab\nBinary.FcControlledGuardDll\nBinary.FcGuardClientTemplate\n\x05SummaryInformation\n', 'payload.cab\nBinary.FcPayloadManifest\nBinary.FcControlledGuardDll\nBinary.FcGuardClientTemplate\n\x05SummaryInformation\n'), 'EXACT_MSI_STREAM_ENUMERATION_STRUCTURE')
  elif kind == 'EXACT_FORCE_CODEPAGE_TABLE':
   control = '\x00'; need(text == '\r\n\r\n0\t_ForceCodepage\r\n\x00', 'EXACT_MSI_CODEPAGE_TABLE_STRUCTURE')
  elif kind == 'EXACT_MSI_SUMMARY_STREAM_NAME_DERIVED_JSON':
   control = '\x05'; need(text == '\x05SummaryInformation', 'EXACT_DERIVED_MSI_SUMMARY_STREAM_NAME')
  else: raise AssertionError('UNKNOWN_STRUCTURAL_FORM')
  need(text.count(control) == 1 and not any(ord(c) < 32 and c not in ('\r', '\n', '\t', control) for c in text), 'NO_OTHER_STRUCTURAL_CONTROLS')
  return [('utf8-exact-structural', text), ('utf8-visible-boundary', text.replace(control, '[U+%04X]' % ord(control))), ('utf8-control-concatenated', text.replace(control, ''))]
 ns['reviewed_control_views'] = reviewed
 stats = {k: 0 for k in ('textViews', 'utf16Views', 'cp936Views', 'clixmlDecodedViews', 'decodedBase64Views', 'jsonContainers', 'decodedJsonStringViews')}
 findings = []
 def scan(data, label):
  ns['credential_scan'](data, label, stats, findings)
  if label.endswith(('.jsonl', '.log')):
   for i, line in enumerate(data.splitlines()):
    if line.lstrip().startswith((b'{', b'[')):
     need(i < 250000, 'FINITE_JSONL_BOUND')
     try: json.loads(line)
     except (ValueError, UnicodeError):
      need(not label.endswith('.jsonl'), 'DECLARED_JSONL_LINE_VALID'); continue
     ns['credential_scan'](line, label + '::jsonl/' + str(i), stats, findings)
  need(not findings, 'CREDENTIAL_FINDING_STOP_PATH_KIND_ONLY_' + json.dumps(findings))
 return scan, stats

def review(get, manifest):
 by = {r['path']: r for r in manifest['originals'] + manifest['excludedBinaries']}
 def j(name): return json.loads(get(name))
 def binding(row):
  p = Path(row['path']); name = p.relative_to(R).as_posix() if p.is_absolute() else p.as_posix()
  need(name in by and by[name]['bytes'] == row['bytes'] and by[name]['sha256'] == row['sha256'], 'SELECTED_SOURCE_BINDING_' + name)
 for tree in manifest['trees']:
  name = tree + '/freeze-first.json'
  if name not in manifest['freezePins']: continue
  f = j(name); need(sha(get(name)) == manifest['freezePins'][name], 'FROZEN_DOMAIN_SHA')
  seen = set()
  for row in f['files']:
   p = Path(row['path']); name2 = p.relative_to(R).as_posix() if p.is_absolute() else tree + '/' + p.as_posix()
   safe(name2); need(name2 in by and by[name2]['bytes'] == row['bytes'] and by[name2]['sha256'] == row['sha256'], 'FROZEN_MEMBER_BOUND'); seen.add(name2)
  need(seen | {name} == {k for k in by if k.startswith(tree + '/')}, 'EXACT_FROZEN_DOMAIN_MEMBERSHIP')
  for row in f.get('ownedSources', []):
   suffix = row['path'].split('/src/installer_lifecycle/', 1)[1]
   copy = tree + '/source-final/' + suffix
   need(copy in by and by[copy]['bytes'] == row['bytes'] and by[copy]['sha256'] == row['sha256'], 'OWNED_SOURCE_FROZEN_COPY')
 n = 'source-package-probe-runtime-first/report.json'; need(sha(get(n)) == RUNTIME_SHA, 'ROOT_PINNED_ACTUAL_RUNTIME_SHA'); runtime = j(n)
 need(runtime['status'] == 'PASS_THREE_FIXED_SOURCE_PACKAGE_CFGTESTS_BEFORE_BEGIN_WIN11_ONLY', 'ACTUAL_FINITE_RUNTIME_STATUS')
 for k in ['producer', 'producerFreeze', 'fixtureReport', 'fixtureFreeze', 'controllerInput', 'preparationFreeze']: binding(runtime[k])
 need(runtime['downloadVerified'] and runtime['downloadCounts'] == {'files': 9, 'directories': 6} and runtime['ownedProcessReadback'] == {'count': 0} and runtime['delete']['knownAbsent'] and runtime['sourceArtifactsAndControllerUnchanged'], 'ACTUAL_DOWNLOAD_AND_CLEANUP')
 need(len(runtime['operations']) == 9 and all(o['actualReturnCode'] == 0 and o['originalLocalReaped'] and not o['budgetExceeded'] and not o['remoteOriginalReturnUnknown'] for o in runtime['operations']), 'ALL_ORIGINAL_TRANSPORT_RETURNS')
 for op in runtime['operations']:
  for k in ['stdout', 'stderr']: binding(op[k])
 root = Path(runtime['windowsRoot']).name
 # Windows paths contain backslashes on this host; obtain only the fixed basename.
 root = runtime['windowsRoot'].rsplit('\\', 1)[-1]
 download = 'source-package-probe-runtime-first/download/' + root + '/'
 for row in runtime['inventory']['files']:
  name = download + row['path']; need(name in by and by[name]['bytes'] == row['bytes'] and by[name]['sha256'] == row['sha256'], 'FULL_NINE_DOWNLOADED_IDENTITIES')
 need(len(runtime['inventory']['files']) == 9 and len(runtime['inventory']['directories']) == 6, 'FINITE_NINE_SIX_INVENTORY')
 native = runtime['native']; need(j(download + 'native-result.json') == native, 'ORIGINAL_NATIVE_JSON_EQUALS_RUNTIME_EMBED')
 need(native['originalParentReturned'] and native['originalWholeSeconds'] == 600 and len(native['cases']) == len(native['originalObjects']) == 3, 'ORIGINAL_CASE_SCOPE')
 fixture = j('source-package-probe-fixtures-first/report.json')
 for case in native['cases']:
  name = case['name']; original_cap = case['original']
  need(case['actualExit'] is True and case['exitCode'] == 0 and original_cap in native['originalObjects'] and original_cap['Wait'] == original_cap['ExitCode'] == 0 and original_cap['ExitKnown'] and original_cap['CheckedClose'] and original_cap['Error'] is None and original_cap['Sha256'] == fixture['cases'][name]['exe']['sha256'], 'CASE_ORIGINAL_CAP_RETURNED_CHECKED_CLOSE')
  need('1 passed; 0 failed; 0 ignored' in case['stdout'] and 'claims=0' in case['stdout'] and case['originalSeconds'] == 150, 'EXACT_SINGLE_CFGTEST_CASE_OUTCOME')
 for q in runtime['qualifiedCases']: need(q['passed'] == 1 and q['failed'] == q['ignored'] == 0 and q['beforeBeginOnly'], 'QUALIFIED_THREE_BEFORE_BEGIN')
 need(len(runtime['qualifiedCases']) == 3 and fixture['all14PayloadsPreserved'] and fixture['canonicalUnchanged'], 'FIXTURE_14_UNCHANGED')
 for case in fixture['cases'].values():
  for k in ['exe', 'msi']: binding(case[k])
 probe = j('source-package-probe-implementation-first/producer-report-third.json')
 need(probe['sourceBefore'] == probe['sourceAfter'] and probe['sourceUnchanged'] and probe['sourceInputCount'] == len(probe['sourceBefore']) == 477 and all(o['actualExit'] == 0 and o['parentReaped'] for o in probe['checks']) and probe['compilerArtifact']['profile']['test'] and probe['features'] == ['default'] and not probe['normalProductProvenance'], 'FINAL_TEST_PRODUCER_BOUND')
 binding(probe['testExecutable'])
 for row in j('source-package-probe-implementation-first/freeze-first.json')['ownedSources']:
  relative = 'crates/fc-windows/src/installer_lifecycle/' + row['path'].split('/src/installer_lifecycle/', 1)[1]
  ledger = {v['path']: v for v in probe['sourceBefore']}
  need(relative in ledger and ledger[relative]['bytes'] == row['bytes'] and ledger[relative]['sha256'] == row['sha256'], 'ACTUAL_PRODUCER_CONTAINS_FINAL_SOURCE')
 for name in ['inner-guard-abi2-implementation-first/native-checks-fourth.json', 'source-package-reader-implementation-first/native-checks-first.json']:
  q = j(name); need(q['sourceUnchanged'] and q['compilerInputsUnchanged'] and q['sourceBefore'] == q['sourceAfter'] and all(o['actualExit'] == 0 for o in q['checks']), 'FINAL_COMPONENT_CHECKS')
 first = j('source-package-probe-implementation-first/producer-wrapper-failure-first.json'); need(first['actualWrapperExit'] == 1 and not first['originalNumericalCompilerExitSerialized'], 'ORIGINAL_WRAPPER_FAILURE_NOT_RECLASSIFIED')
 need(j('inner-guard-abi2-implementation-first/native-checks-first.json')['checks'][0]['actualExit'] == 101, 'ORIGINAL_COMPILER101_RETAINED')
 ast = j('root-source-package-probe-ps-parse-first.json'); need(ast['actualExit'] == 0 and ast['originalReaped'] and all(not row['errors'] for row in ast['native']['rows']), 'ACTUAL_PS_AST')
 integrated = j('source-package-integrated-checks-first/report-first.json'); preserved = j('source-package-integrated-preservation-first/report.json'); identities = j('source-package-integrated-checks-first/source-identities-first.json')
 need(integrated['status'] == 'PASS_INTEGRATED_SOURCE_CAPTURE_CHECKS' and integrated['sourceBefore'] == integrated['sourceAfter'] and integrated['sourceUnchanged'] and len(integrated['sourceBefore']) == 482 and len(integrated['operations']) == 7 and all(op['actualExit'] == 0 for op in integrated['operations']), 'INDEPENDENT_INTEGRATION_CHECKS')
 need(integrated['sourceBase'] == preserved['base'] == identities['sourceBase'] == 'd9dacefb481678a2f1c12cf8371fa6567820547d' and preserved['head'] == 'f95389d42a9868d5cb595c33393834c235dbdd37' and preserved['files'] == identities['files'] and len(preserved['files']) == 11, 'RECORDED_INTEGRATED_CANDIDATE11')
 combined_ledger = {r['path']: r for r in integrated['sourceBefore']}
 for row in preserved['files']:
  copy = 'source-package-integrated-checks-first/source-final/' + row['path']
  need(copy in by and by[copy]['bytes'] == row['bytes'] and by[copy]['sha256'] == row['sha256'] and combined_ledger[row['path']] == row, 'ALL11_TESTED_COMMITTED_SOURCE_COPY_BYTES')
 return {'status': 'PASS_FINITE_FROZEN_SOURCE_AND_THREE_ACTUAL_WIN11_READONLY_CAPTURE_CASES', 'runtimeOriginalSha256': RUNTIME_SHA, 'runtimeOriginalStatus': runtime['status'], 'runtimeSourceBase': probe['sourceBase'], 'runtimeProducerInputs': probe['sourceInputCount'], 'integratedChecks': {'sourceBase': integrated['sourceBase'], 'candidateCommit': preserved['head'], 'sourceInputs': 482, 'sourceFiles': 11, 'allSevenOperationsActual0': True, 'candidateNotPushedOrMainAtPreservation': True, 'runtimeQualification': 'Original three Win11 probes used separately frozen a470+W5+carrier+CA+Source+Probe inputs, excluding W6/hostcfg; they were not run against this later d9 integrated candidate.'}, 'cases': [{'name': c['name'], 'originalPid': c['pid'], 'exitCode': c['exitCode'], 'checkedClose': c['original']['CheckedClose']} for c in native['cases']], 'positiveCapture': 'ABI2 actual readonly MSI database/summary/streams; exact14 ledger;10 actual MSI close receipts before Begin, with3 view close returns; source/template bindings asserted by frozen ignored test.', 'negativeCases': ['wholeMSI seal mismatch rejected before Begin', 'template byte outside32-byte digest hole rejected before Begin'], 'sourceReview': 'Frozen native_probe.rs inspected: positive verifies actual close receipts and checked Resources release; both negative probes require Invalid plus pristine claims/fault/retained state. No MSI Begin/Install/End or install mutation called by these exact tests.', 'downloadVerified': {'files': 9, 'directories': 6}, 'ownedProcessCount': 0, 'exactNonceKnownAbsent': runtime['delete']['nonce'], 'historicalFailures': 'Original GNU compiler101, pure compile/test failures and producer wrapper1 are retained unchanged. First wrapper did not serialize numeric compiler exit; do not invent it. Earlier static NOT_RUN reports remain historical.', 'scope': 'GNU DEV cfg(test) Win11 readonly source capture before Begin only. Header-patched testPE fixtures and original canonical payload evidence do not authorize normal/official product, PublishedHost, Fresh/maintenance/upgrade, Win7/Win10 runtime or new-source MSVC. Prior carrier/CI and active later implementations excluded.', 'actions': {'Windows': False, 'Cargo': False, 'Git': False, 'sourceDeletion': False}}

def main():
 p = argparse.ArgumentParser(description=__doc__); g = p.add_mutually_exclusive_group(); g.add_argument('--execute', action='store_true'); g.add_argument('--verify-only', action='store_true'); p.add_argument('--expected-freeze-sha256'); a = p.parse_args()
 if not a.execute and not a.verify_only: print(json.dumps({'status': 'PREPARED_NOT_EXECUTED', 'Windows': False, 'Cargo': False})); return
 need(Path(__file__).resolve() == D / 'archive-source-package-first.py', 'EXACT_MAIN_CONTROLLER')
 manifest = json.loads((D / 'source-manifest-first.json').read_bytes()); rows = manifest['originals']; by = {r['path']: r for r in rows}; scan, stats = scanning(json.loads((D / 'text-forms-first.json').read_bytes()))
 need(len(by) == len(rows) == manifest['originalCount'] and sum(r['bytes'] for r in rows) == manifest['originalBytes'], 'FINITE_COUNTS')
 if a.verify_only:
  fb = (D / 'freeze-first.json').read_bytes(); need(a.expected_freeze_sha256 and sha(fb) == a.expected_freeze_sha256, 'CALLER_PINNED_FINAL_FREEZE'); freeze = json.loads(fb)
  expected = {r['path']: r for r in freeze['files']}
  actual = {f.relative_to(D).as_posix() for f in D.rglob('*') if f.is_file() and f != D / 'freeze-first.json'}
  need(actual == set(expected), 'EXACT_FINAL_ARCHIVE_FILE_SET')
  for name, row in expected.items():
   f = D / safe(name); need(f.is_file() and not f.is_symlink() and f.resolve() == f, 'REGULAR_ARCHIVE_FILE'); b = f.read_bytes(); need(len(b) == row['bytes'] and sha(b) == row['sha256'], 'FINAL_FROZEN_BYTE_SHA')
  index = json.loads((D / 'archive-index-first.json').read_bytes()); ab = (D / 'evidence-originals-first.tar.gz').read_bytes(); need(len(ab) == index['archive']['bytes'] and sha(ab) == index['archive']['sha256'], 'ARCHIVE_IDENTITY')
 else:
  need({f.name for f in D.iterdir()} == {'archive-source-package-first.py', 'source-manifest-first.json', 'text-forms-first.json'}, 'NEW_FINITE_DESTINATION')
  need((R / '.owner').read_bytes() == b'future-classroom-owned-r80-controlled-repair/v1\n', 'R80_OWNER')
  current = set()
  for tree in manifest['trees']:
   root = R / safe(tree)
   for f in root.rglob('*'):
    need(not f.is_symlink(), 'NO_SOURCE_LINKS')
    if f.is_file(): current.add(f.relative_to(R).as_posix())
  current.update(manifest['rootSingles']); need(current == {r['path'] for r in rows + manifest['excludedBinaries']}, 'EXACT_FINITE_MEMBERSHIP')
  for row in rows: scan(original(row), 'source:' + row['path'])
  for row in manifest['excludedBinaries']: original(row)
  review(lambda n: original(by[n]), manifest)
  with (D / 'evidence-originals-first.tar.gz').open('xb') as outer:
   with gzip.GzipFile(filename='', mode='wb', fileobj=outer, mtime=0, compresslevel=9) as zipped:
    with tarfile.open(fileobj=zipped, mode='w', format=tarfile.PAX_FORMAT) as t:
     for row in rows:
      b = original(row); info = tarfile.TarInfo(row['path']); info.size = len(b); info.mode = 0o600; info.uid = info.gid = info.mtime = 0; info.uname = info.gname = ''; t.addfile(info, io.BytesIO(b))
 stored = {}
 with tarfile.open(D / 'evidence-originals-first.tar.gz', 'r:gz') as t:
  members = t.getmembers(); need([m.name for m in members] == [r['path'] for r in rows], 'EXACT_ORDER_NO_UNKNOWN_MEMBER')
  for member, row in zip(members, rows):
   need(member.isfile() and member.mode == 0o600 and member.uid == member.gid == member.mtime == 0 and not member.uname and not member.gname, 'CANONICAL_REGULAR_MEMBER')
   b = t.extractfile(member).read(); need(len(b) == row['bytes'] and sha(b) == row['sha256'], 'FULL_STORED_BYTE_REPLAY')
   if a.execute: need(b == original(row), 'FULL_STORED_SOURCE_EQUALITY')
   scan(b, 'stored-tar-gzip-decoded:' + row['path']); stored[row['path']] = b
 facts = review(lambda n: stored[n], manifest)
 for name in PROMOTE:
  dest = D / safe(name)
  if a.execute:
   dest.parent.mkdir(parents=True, exist_ok=True)
   with dest.open('xb') as f: f.write(stored[name])
  need(dest.read_bytes() == stored[name], 'PROMOTED_ORIGINAL_BYTES'); scan(stored[name], 'promoted:' + name)
 if a.execute:
  ab = (D / 'evidence-originals-first.tar.gz').read_bytes()
  documents = {'finite-review-first.json': facts, 'archive-index-first.json': {'status': 'PASS_FINITE_SOURCE_PACKAGE_CAPTURE_ARCHIVE', 'archive': {'path': 'evidence-originals-first.tar.gz', 'bytes': len(ab), 'sha256': sha(ab), 'members': len(rows)}, 'originalTextBytes': manifest['originalBytes'], 'excludedBinaryCount': len(manifest['excludedBinaries']), 'excludedBinaryPolicy': 'Exact identity only; no binary original bytes copied into Git', 'promotedOriginals': [by[n] for n in PROMOTE], 'sourceDeletion': False}, 'archive-verification-first.json': {'status': 'PASS_FULL_MEMBER_SOURCE_REPLAY_AND_STRICT_SCAN', 'members': len(rows), 'statistics': stats.copy(), 'credentialFindings': [], 'scanner': {'sourceArchive': str(BACKEND.relative_to(M)), 'member': MEMBER, 'bytes': 26272, 'sha256': BACKEND_SHA}, 'controlPolicy': 'Only11 exact path/bytes/SHA+structural views from text-forms-first.json; all raw/visible/concatenated views scanned. Unknown controls rejected.', 'coverage': 'UTF8/UTF16/CP936/CLIXML/JSON and nested base64; frozen depth12/decoded64MiB/250000 bounds unchanged.', 'Windows': False, 'Cargo': False}}
  for name, obj in documents.items():
   b = raw(obj); scan(b, 'archive-metadata:' + name)
   with (D / name).open('xb') as f: f.write(b)
  for row in rows + manifest['excludedBinaries']: original(row)
 for f in D.rglob('*'):
  need(not f.is_symlink(), 'NO_DESTINATION_LINK')
  if f.is_file() and f.name != 'evidence-originals-first.tar.gz':
   name = f.relative_to(D).as_posix()
   if name not in PROMOTE: scan(f.read_bytes(), 'archive-metadata:' + name)
 print(json.dumps({'status': 'PASS_VERIFY_ONLY_NO_WRITES' if a.verify_only else 'PASS_ACTUAL_FINITE_ARCHIVE', 'members': len(rows), 'originalTextBytes': manifest['originalBytes'], 'excludedBinaryCount': len(manifest['excludedBinaries']), 'promotedOriginals': len(PROMOTE), 'archiveBytes': (D / 'evidence-originals-first.tar.gz').stat().st_size, 'archiveSha256': sha((D / 'evidence-originals-first.tar.gz').read_bytes()), 'credentialFindings': 0, 'statistics': stats, 'Windows': False, 'Cargo': False, 'Git': False, 'sourceDeletion': False}))

if __name__ == '__main__': main()
