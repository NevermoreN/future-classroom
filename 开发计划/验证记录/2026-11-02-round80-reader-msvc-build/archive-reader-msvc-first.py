#!/usr/bin/env python3
"""Default-inert finite ABF official MSVC evidence archive; verify-only needs no ZIP/R/Git."""
from pathlib import Path
import argparse, gzip, hashlib, io, json, re, stat, tarfile, zipfile

M = Path('/workspace/共享/Eliora/未来教室计划')
R = Path('/tmp/fc-round80-controlled-repair')
D = M / '开发计划/验证记录/2026-11-02-round80-reader-msvc-build'
SOURCE = 'abf71f4b387616f8b83c4c5af8622898e594f60d'
QUALIFICATION_SHA = '878cad53e84e695dfcfb7f5698e71b31e8692f02598a32ce43593fb33d66051e'
BACKEND = M / '开发计划/验证记录/2026-11-02-round79-msi-maintenance/evidence-originals-first.tar.gz'
MEMBER = 'archive-preparation-second/archive-second.py'
BACKEND_SHA = 'da2906c450ea8c2ac7d18e358507023f519dbcc6faf1a9ac1abb1ae16af8c30f'
PROMOTE = ['abf-ci-qualification-first/' + n for n in ['freeze-first.json', 'modern-report.json', 'win7-report.json', 'modern-all-commands.json', 'win7-all-commands.json', 'run-41368780507-terminal.json', 'run-41368783471-terminal.json']]
for label in ['modern', 'win7']:
 PROMOTE += ['abf-' + label + '-download-first/' + n for n in ['download-report.json', 'zip-inventory.json', 'artifact-text/result.json', 'artifact-text/host.json', 'artifact-text/cleanup.json', 'artifact-text/executables.json']]

def need(ok, label):
 if not ok: raise AssertionError(label)
def sha(b): return hashlib.sha256(b).hexdigest()
def raw(j): return (json.dumps(j, ensure_ascii=False, indent=2) + '\n').encode()
def safe(name):
 p = Path(name); need(not p.is_absolute() and p.as_posix() == name and '..' not in p.parts and name != '.', 'CANONICAL_RELATIVE_PATH'); return p

def scanner(forms):
 with tarfile.open(BACKEND, 'r:gz') as t:
  i = t.getmember(MEMBER); need(i.isfile() and i.size == 26272, 'FROZEN_SCANNER_MEMBER'); b = t.extractfile(i).read()
 need(sha(b) == BACKEND_SHA, 'FROZEN_SCANNER_SHA'); ns = {'__name__': 'abf_existing_strict_scanner'}; exec(compile(b, '<frozen-strict-scanner>', 'exec'), ns)
 table = {r['path']: r for r in forms['files']}; need(len(table) == len(forms['files']) == 6 and all(r['textForm'] in ['EXACT_UTF8_LICENSE_WITH_18_FORM_FEEDS', 'EXACT_GIT_LS_TREE_Z_19638_RECORDS'] for r in table.values()), 'EXACT_SIX_TEXT_FORMS')
 ns['TEXT_FORMS'] = {k: r for k, r in table.items() if r['textForm'] == 'EXACT_UTF8_LICENSE_WITH_18_FORM_FEEDS'}; ns['TEXT_DERIVED_VIEWS'] = {}; previous = ns['reviewed_control_views']
 def reviewed(data, label):
  relative = None
  for prefix in ('source:', 'stored-tar-gzip-decoded:', 'promoted:'):
   if isinstance(label, str) and label.startswith(prefix): relative = label[len(prefix):]
  row = table.get(relative)
  if row is None or row['textForm'] == 'EXACT_UTF8_LICENSE_WITH_18_FORM_FEEDS': return previous(data, label)
  need(len(data) == row['bytes'] and sha(data) == row['sha256'], 'EXACT_GIT_Z_ORIGINAL_BYTES_SHA'); text = data.decode('utf-8'); records = text.split('\0'); need(records[-1] == '' and len(records) == 19639, 'EXACT_19638_Z_RECORDS'); paths = []
  for record in records[:-1]:
   match = re.fullmatch(r'(100644|100755|120000|040000|160000) (blob|tree|commit) ([0-9a-f]{40}) +([0-9]+|-)\t([^\x00]+)', record)
   need(match is not None, 'STRICT_MODE_TYPE_OID_SIZE_TAB_PATH')
   need((match[1] in ('100644', '100755', '120000') and match[2] == 'blob') or (match[1] == '040000' and match[2] == 'tree') or (match[1] == '160000' and match[2] == 'commit'), 'CONSISTENT_GIT_OBJECT_MODE_TYPE')
   need((match[2] == 'blob' and match[4].isdigit() and str(int(match[4])) == match[4] and int(match[4]) <= 64*1024*1024) or (match[2] in ('tree', 'commit') and match[4] == '-'), 'GIT_SIZE_MATCHES_OBJECT_TYPE')
   safe(match[5]); need(not any(ord(c) < 32 for c in match[5]), 'NO_CONTROL_IN_GIT_FILENAME'); paths.append(match[5])
  need(len(set(paths)) == len(paths) and not any(ord(c) < 32 and c not in '\r\n\t\0' for c in text), 'UNIQUE_Z_PATHS_NO_OTHER_CONTROLS')
  return [('utf8-exact-git-z', text), ('utf8-visible-z', text.replace('\0', '[U+0000]')), ('utf8-concatenated-z', text.replace('\0', ''))]
 ns['reviewed_control_views'] = reviewed
 stats = {k: 0 for k in ('textViews', 'utf16Views', 'cp936Views', 'clixmlDecodedViews', 'decodedBase64Views', 'jsonContainers', 'decodedJsonStringViews')}; findings = []
 def scan(b, label):
  ns['credential_scan'](b, label, stats, findings)
  if label.endswith(('.jsonl', '.log')):
   for i, line in enumerate(b.splitlines()):
    if line.lstrip().startswith((b'{', b'[')):
     need(i < 250000, 'JSONL_COUNT_BOUND')
     try: json.loads(line)
     except (ValueError, UnicodeError):
      need(not label.endswith('.jsonl'), 'VALID_DECLARED_JSONL'); continue
     ns['credential_scan'](line, label + '::jsonl/' + str(i), stats, findings)
  need(not findings, 'CREDENTIAL_FINDING_STOP_PATH_KIND_ONLY_' + json.dumps(findings))
 return scan, stats

class Originals:
 def __init__(self, manifest):
  self.zips = {}
  for row in manifest['zips']:
   p = R / safe(row['path']); need(p.is_file() and not p.is_symlink() and p.resolve() == p, 'EXACT_ORIGINAL_ZIP_PATH'); b = p.read_bytes(); need(len(b) == row['bytes'] and sha(b) == row['sha256'], 'EXACT_ORIGINAL_ZIP_IDENTITY'); z = zipfile.ZipFile(p); self.zips[row['path']] = z
   infos = [i for i in z.infolist() if not i.is_dir()]; names = [i.filename for i in infos]; need(len(names) < 1000 and len(names) == len(set(names)), 'BOUNDED_UNIQUE_ZIP_MEMBERS')
   expected = {r['source']['member'] for r in manifest['originals'] + manifest['binaryIdentities'] if r['source'].get('zip') == row['path']}; need(set(names) == expected, 'ALL_ZIP_MEMBERS_IDENTIFIED')
   for info in infos: safe(info.filename); need(not stat.S_ISLNK(info.external_attr >> 16) and not info.flag_bits & 1 and info.file_size < 16*1024*1024, 'REGULAR_BOUNDED_UNENCRYPTED_MEMBER')
 def get(self, row):
  source = row['source']
  if 'file' in source:
   p = R / safe(source['file']); need(p.is_file() and not p.is_symlink() and p.resolve() == p, 'EXACT_REGULAR_SOURCE'); b = p.read_bytes()
  else: b = self.zips[source['zip']].read(source['member'])
  need(len(b) == row['bytes'] and sha(b) == row['sha256'], 'EXACT_SOURCE_BYTES_SHA_' + row['path']); return b

def review(get, manifest):
 by = {r['path']: r for r in manifest['originals'] + manifest['binaryIdentities']}
 def j(n): return json.loads(get(n))
 qprefix = 'abf-ci-qualification-first/'; need(sha(get(qprefix + 'freeze-first.json')) == QUALIFICATION_SHA, 'PINNED_QUALIFICATION_FREEZE')
 qfreeze = j(qprefix + 'freeze-first.json'); expected = {qprefix + r['path'] for r in qfreeze['files']} | {qprefix + 'freeze-first.json'}; need(expected == {n for n in by if n.startswith(qprefix)}, 'EXACT_FROZEN_QUALIFICATION_DOMAIN')
 for row in qfreeze['files']:
  r = by[qprefix + row['path']]; need(r['bytes'] == row['bytes'] and r['sha256'] == row['sha256'], 'FROZEN_QUALIFICATION_FILE')
 summaries = []
 for label, count, run_id, target in [('modern', 51, 41368780507, 'i686-pc-windows-msvc'), ('win7', 53, 41368783471, 'i686-win7-windows-msvc')]:
  root = 'abf-' + label + '-download-first/'; prefix = root + 'artifact-text/'; qualified = j(qprefix + label + '-report.json'); commands = j(qprefix + label + '-all-commands.json'); result = j(prefix + 'result.json'); receipt = j(root + 'download-report.json'); terminal = j(qprefix + 'run-' + str(run_id) + '-terminal.json'); zip_identity = next(r for r in manifest['zips'] if r['path'] == root + 'artifact.zip')
  need(qualified['status'] == 'PASS_STATIC_OFFICIAL_ABF_CI_INPUT_ADMISSION_ONLY' and qualified['sourceSha'] == SOURCE and qualified['runId'] == run_id and qualified['target'] == target and qualified['allNativeRuntime'] == 'NOT_RUN' and qualified['laterMainD9AndSourceCapture'] == 'NOT_COVERED_BY_ABF', 'QUALIFIED_BUILD_ONLY_SOURCE')
  need(terminal['status'] == 'completed' and terminal['conclusion'] == 'success' and terminal['head_sha'] == SOURCE and terminal['runId'] == run_id, 'ORIGINAL_CI_TERMINAL_SUCCESS')
  need(receipt['head'] == SOURCE and receipt['runId'] == run_id and receipt['artifactId'] == qualified['artifactId'] and receipt['bytes'] == zip_identity['bytes'] == qualified['zip']['bytes'] and receipt['sha256'] == zip_identity['sha256'] == qualified['zip']['sha256'], 'ORIGINAL_ACTUAL_ZIP_IDENTITY_RECEIPT')
  zip_rows = [r for r in manifest['originals'] + manifest['binaryIdentities'] if r['source'].get('zip') == zip_identity['path']]
  zip_member_rows = {r['source']['member']: r for r in zip_rows}; need({r['path']: r['bytes'] for r in receipt['members']} == {n: r['bytes'] for n, r in zip_member_rows.items()}, 'COMPLETE_DOWNLOAD_RECEIPT_MEMBERSHIP')
  for r in j(prefix + 'artifact-sha256.json'):
   actual = zip_member_rows[r['path']]; need(actual['bytes'] == r['bytes'] and actual['sha256'] == r['sha256'], 'ORIGINAL_ARTIFACT_HASH_MANIFEST')
  for r in qualified['ciMemberInputs']:
   actual = zip_member_rows[r['member']]; need(actual['bytes'] == r['bytes'] and actual['sha256'] == r['sha256'], 'ORIGINAL_QUALIFICATION_USED_SAME_ZIP_MEMBER')
  need(result['status'] == 'COMPILE_CHECKS_PASSED_RUNTIME_NOT_RUN' and result['source_sha'] == result['dispatch_source_sha'] == SOURCE and result['target'] == target and result['executable_count'] == 25 and len(result['commands']) == count and all(result[k] == 'NOT_RUN' for k in ['windows7', 'windows10', 'windows11', 'seewo_device']), 'ORIGINAL_ALL_COMMANDS_COMPILE_ONLY')
  need(commands['status'] == 'PASS_EVERY_ORIGINAL_CI_COMMAND_AND_LOG_HASH' and commands['qualificationReportSha256'] == sha(get(qprefix + label + '-report.json')) and len(commands['commands']) == count and all(c['exit'] == 0 and c['parentExited'] for c in commands['commands']), 'ORIGINAL_COMMAND_AUDIT')
  for command in result['commands']:
   original = j(prefix + 'logs/' + command['name'] + '.result.json'); need(original == command and original['exit_code'] == 0 and original['parent_exited'] and not original['timed_out_or_pipe_stalled'] and not original['log_truncated'] and original['passed'], 'ACTUAL_ORIGINAL_COMMAND_RESULT')
   for stream in ['stdout', 'stderr']: need(sha(get(prefix + original[stream])) == original[stream + '_sha256'], 'COMPLETE_ORIGINAL_COMMAND_LOG_HASH')
  inputs = j(prefix + 'input-sha256.json'); need(inputs == j(prefix + 'input-sha256-after.json') and len(inputs) == qualified['committedInputRowsVerified'] == len(qualified['committedInputMaterializationProof']) == 623 and sha(get(prefix + 'input-sha256.json')) == result['input_manifest_sha256'], 'EXACT_623_CI_BEFORE_AFTER_INPUTS')
  input_rows = {r['path']: r for r in inputs}
  for proof in qualified['committedInputMaterializationProof']:
   r = input_rows[proof['path']]; need(r['bytes'] == proof['ciCheckoutBytes'] and r['sha256'] == proof['ciCheckoutSha256'] and re.fullmatch('[0-9a-f]{40}', proof['gitBlobOid']) and re.fullmatch('[0-9a-f]{64}', proof['gitBlobSha256']), 'RECORDED_GIT_BLOB_CRLF_PROOF_MATCHES_CI_CHECKOUT')
  need(get(prefix + 'input-git-tree.json') == get(prefix + 'input-git-tree-after.json') and get(prefix + 'input-domains.json') == get(prefix + 'input-domains-after.json') and sha(get(prefix + 'input-git-tree.json')) == result['git_tree_manifest_sha256'] and sha(get(prefix + 'input-domains.json')) == result['input_domains_manifest_sha256'], 'ORIGINAL_DOMAIN_AND_GIT_METADATA_BEFORE_AFTER')
  binaries = [r for r in manifest['binaryIdentities'] if r['source']['zip'] == zip_identity['path']]; exes = j(prefix + 'executables.json'); need(len(binaries) == len(exes) == 25 and {r['source']['member'] for r in binaries} == {r['relative_path'] for r in exes}, 'ALL25_COMPILED_PE_IDENTITIES')
  for executable in exes:
   r = zip_member_rows[executable['relative_path']]; need(r['bytes'] == executable['bytes'] and r['sha256'] == executable['sha256'] and executable['pe_machine'] == '0x014c' and executable['pe_optional_magic'] == '0x010b' and not executable['executed'], 'PE32_IDENTITY_NO_RUNTIME')
  host = j(prefix + 'host.json'); need(host['source_sha'] == SOURCE and host['runner_environment'] == 'github-hosted' and j(prefix + 'cleanup.json')['status'] == 'OWNED_BUILD_ROOTS_REMOVED', 'HOSTED_COMPILER_HOST_AND_OWNED_CLEANUP')
  summaries.append({'label': label, 'runId': run_id, 'artifactId': qualified['artifactId'], 'target': target, 'toolchain': result['toolchain'], 'commandsActual0': count, 'originalInputRows': 623, 'originalPEIdentities': 25, 'runtime': 'NOT_RUN', 'zipIdentity': zip_identity})
 return {'status': 'PASS_FINITE_ABF_OFFICIAL_DUAL_MSVC_BUILD_EVIDENCE', 'sourceSha': SOURCE, 'builds': summaries, 'sourceProof': 'Frozen original qualification records each623 committed Gitblob and LF/CRLF materialization against actual CI checkout SHA. Archive readback crosschecks all623 proof rows with original before/after CI ledgers. No new Git query or source materialization is run by this archive.', 'historicalMetadata': '19638 tracked paths include19015 historical archive entries. Their tree/domain metadata is preserved; historical checkout content is not claimed as SHA256 productsource-read.', 'downloadProof': 'Actual ZIP identities and original receipts preserved. Original download script was not persisted; no download source or temporary signed URL is invented.', 'scope': 'Official MSVC compile evidence for immutable abf only. Neither local Windows nor Win7/Win10/Win11/Seewo runtime was performed by these jobs or this archive. Later d9/f953/Fresh/upgrade/Published/product release not covered.', 'actions': {'Windows': False, 'Git': False, 'Cargo': False, 'sourceDeletion': False}}

def main():
 p = argparse.ArgumentParser(description=__doc__); g = p.add_mutually_exclusive_group(); g.add_argument('--execute', action='store_true'); g.add_argument('--verify-only', action='store_true'); p.add_argument('--expected-freeze-sha256'); a = p.parse_args()
 if not a.execute and not a.verify_only: print(json.dumps({'status': 'PREPARED_NOT_EXECUTED', 'Windows': False, 'Git': False})); return
 need(Path(__file__).resolve() == D / 'archive-reader-msvc-first.py', 'EXACT_MAIN_CONTROLLER')
 manifest = json.loads((D / 'source-manifest-first.json').read_bytes()); rows = manifest['originals']; by = {r['path']: r for r in rows}; need(len(rows) == len(by) == manifest['originalCount'] and sum(r['bytes'] for r in rows) == manifest['originalTextBytes'], 'FINITE_TEXT_COUNTS')
 scan, stats = scanner(json.loads((D / 'text-forms-first.json').read_bytes()))
 if a.verify_only:
  b = (D / 'freeze-first.json').read_bytes(); need(a.expected_freeze_sha256 and sha(b) == a.expected_freeze_sha256, 'CALLER_PINNED_IMMUTABLE_FREEZE'); freeze = json.loads(b); actual = {f.relative_to(D).as_posix() for f in D.rglob('*') if f.is_file() and f != D / 'freeze-first.json'}; need(actual == {r['path'] for r in freeze['files']}, 'EXACT_FINAL_MEMBERSHIP')
  for row in freeze['files']:
   f = D / safe(row['path']); need(f.is_file() and not f.is_symlink() and f.resolve() == f, 'EXACT_REGULAR_ARCHIVE_FILE'); b = f.read_bytes(); need(len(b) == row['bytes'] and sha(b) == row['sha256'], 'FINAL_FREEZE_BYTE_SHA')
  index = json.loads((D / 'archive-index-first.json').read_bytes()); ab = (D / 'evidence-originals-first.tar.gz').read_bytes(); need(len(ab) == index['archive']['bytes'] and sha(ab) == index['archive']['sha256'], 'FINAL_TAR_IDENTITY')
 else:
  need({f.name for f in D.iterdir()} == {'archive-reader-msvc-first.py', 'source-manifest-first.json', 'text-forms-first.json'}, 'FRESH_FINITE_DESTINATION'); need((R / '.owner').read_bytes() == b'future-classroom-owned-r80-controlled-repair/v1\n', 'R80_OWNER'); source = Originals(manifest)
  for row in rows: scan(source.get(row), 'source:' + row['path'])
  for row in manifest['binaryIdentities']: source.get(row)
  review(lambda n: source.get(by[n]), manifest)
  with (D / 'evidence-originals-first.tar.gz').open('xb') as outer:
   with gzip.GzipFile(filename='', mode='wb', fileobj=outer, compresslevel=9, mtime=0) as zipped:
    with tarfile.open(fileobj=zipped, mode='w', format=tarfile.PAX_FORMAT) as t:
     for row in rows:
      b = source.get(row); info = tarfile.TarInfo(row['path']); info.size = len(b); info.mode = 0o600; info.uid = info.gid = info.mtime = 0; info.uname = info.gname = ''; t.addfile(info, io.BytesIO(b))
 stored = {}
 with tarfile.open(D / 'evidence-originals-first.tar.gz', 'r:gz') as t:
  members = t.getmembers(); need([i.name for i in members] == [r['path'] for r in rows], 'EXACT_UNIQUE_REGULAR_TAR_MEMBERS')
  for info, row in zip(members, rows):
   need(info.isfile() and info.mode == 0o600 and info.uid == info.gid == info.mtime == 0 and not info.uname and not info.gname, 'CANONICAL_MEMBER'); b = t.extractfile(info).read(); need(len(b) == row['bytes'] and sha(b) == row['sha256'], 'FULL_STORED_BYTE_REPLAY')
   if a.execute: need(b == source.get(row), 'FULL_ORIGINAL_SOURCE_EQUALITY')
   scan(b, 'stored-tar-gzip-decoded:' + row['path']); stored[row['path']] = b
 facts = review(lambda n: stored[n], manifest)
 for name in PROMOTE:
  dest = D / safe(name)
  if a.execute:
   dest.parent.mkdir(parents=True, exist_ok=True)
   with dest.open('xb') as f: f.write(stored[name])
  need(dest.read_bytes() == stored[name], 'PROMOTED_ORIGINAL_BYTES'); scan(stored[name], 'promoted:' + name)
 if a.execute:
  ab = (D / 'evidence-originals-first.tar.gz').read_bytes(); documents = {'finite-review-first.json': facts, 'archive-index-first.json': {'status': 'PASS_FINITE_ABF_READER_MSVC_BUILD_ARCHIVE', 'archive': {'path': 'evidence-originals-first.tar.gz', 'bytes': len(ab), 'sha256': sha(ab), 'members': len(rows)}, 'originalTextBytes': manifest['originalTextBytes'], 'originalZipIdentities': manifest['zips'], 'binaryIdentities': manifest['binaryIdentities'], 'promotedOriginals': [by[n] for n in PROMOTE], 'sourceDeletion': False}, 'archive-verification-first.json': {'status': 'PASS_FULL_MEMBER_SOURCE_REPLAY_AND_STRICT_SCAN', 'members': len(rows), 'credentialFindings': [], 'statistics': stats.copy(), 'scanner': {'sourceArchive': str(BACKEND.relative_to(M)), 'member': MEMBER, 'bytes': 26272, 'sha256': BACKEND_SHA}, 'coverage': 'Original strict UTF8/UTF16/CP936/CLIXML/JSON/JSONL/nestedbase64 depth12/decoded64MiB/250000 bounds unchanged. Only6 exact path/bytes/SHA structural text exceptions; unknown controls rejected. All raw/visible/concatenated views scanned.', 'Windows': False, 'Git': False}}
  for name, obj in documents.items():
   b = raw(obj); scan(b, 'archive-metadata:' + name)
   with (D / name).open('xb') as f: f.write(b)
  for row in manifest['zips']:
   p = R / row['path']; b = p.read_bytes(); need(len(b) == row['bytes'] and sha(b) == row['sha256'], 'ORIGINAL_ZIPS_UNCHANGED')
 for f in D.rglob('*'):
  need(not f.is_symlink(), 'NO_ARCHIVE_LINKS')
  if f.is_file() and f.name != 'evidence-originals-first.tar.gz' and f.relative_to(D).as_posix() not in PROMOTE: scan(f.read_bytes(), 'archive-metadata:' + f.relative_to(D).as_posix())
 print(json.dumps({'status': 'PASS_VERIFY_ONLY_NO_WRITES' if a.verify_only else 'PASS_ACTUAL_FINITE_ARCHIVE', 'members': len(rows), 'originalTextBytes': manifest['originalTextBytes'], 'binaryIdentities': len(manifest['binaryIdentities']), 'zipIdentities': len(manifest['zips']), 'promotedOriginals': len(PROMOTE), 'archiveBytes': (D / 'evidence-originals-first.tar.gz').stat().st_size, 'archiveSha256': sha((D / 'evidence-originals-first.tar.gz').read_bytes()), 'credentialFindings': 0, 'statistics': stats, 'Windows': False, 'Git': False, 'Cargo': False, 'sourceDeletion': False}))

if __name__ == '__main__': main()
