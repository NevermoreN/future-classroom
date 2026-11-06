#!/usr/bin/env python3
"""ABI2 candidate from four original Win7-target MSVC CI artifacts.

Default inert. This packages only; no compiled process or installer is run.
CI proves compilation. Static packaging never grants product release readiness,
Win7/Win10 runtime compatibility, or a Web source-build certification.
"""
from __future__ import annotations

import argparse
import base64
import hashlib
import json
from pathlib import Path
import re
import runpy
import struct
from types import SimpleNamespace

HERE = Path(__file__).resolve().parent
ABI2 = runpy.run_path(str(HERE / 'package-abi2-upgrade-dev.py'))
B = ABI2['B']
NEED = ABI2['NEED']
TARGET = 'i686-win7-windows-msvc'
PUBLICATION = runpy.run_path(str(HERE / 'installer-upgrade-publication.py'))
ARG_PATHS = {'source_root', 'input_zip', 'lifecycle_helper', 'guard_library',
             'build_evidence', 'candidate_instructions'}
ARG_HASHES = {'input_sha256', 'lifecycle_helper_sha256', 'guard_library_sha256',
              'build_result_sha256', 'candidate_instructions_sha256'}


def original_file(path: Path, limit: int = 64 * 1024 * 1024) -> bytes:
    NEED(path.is_absolute() and path == path.resolve(), 'Canonical absolute original evidence path')
    B['PROBE']['real_directory'](path.parent)
    return B['PROBE']['bounded_regular_file'](path, limit)


def bound_fact(fact: dict, limit: int = 64 * 1024 * 1024) -> bytes:
    NEED(isinstance(fact, dict) and set(fact) == {'path', 'bytes', 'sha256'}
         and isinstance(fact['path'], str) and type(fact['bytes']) is int
         and 0 <= fact['bytes'] <= limit and isinstance(fact['sha256'], str)
         and re.fullmatch('[0-9a-f]{64}', fact['sha256']), 'Closed original evidence fact')
    raw = original_file(Path(fact['path']), limit)
    NEED(len(raw) == fact['bytes'] and B['digest'](raw) == fact['sha256'], 'Exact original evidence bytes/SHA')
    return raw


def decode_predecessor(record: bytes) -> dict:
    NEED(type(record) is bytes, 'Immutable formal predecessor record required')
    ABI2['predecessor_bytes'](record)
    NEED(record[12] == 1, 'Formal predecessor must derive from original MSVC target, never GNU')
    meta = dict(target=record[12], source_commit=record[16:36].hex(),
                template_bytes=struct.unpack_from('<I', record, 228)[0])
    meta.update({key: record[36+32*n:68+32*n].hex()
                 for n, key in enumerate(PUBLICATION['DIGEST_KEYS'])})
    NEED(PUBLICATION['encode_format'](meta, record[248:], ABI2['C']['parse_payload_manifest']) == record,
         'Exact predecessor encode/decode roundtrip without tail or reserved bytes')
    return meta


def validate_predecessor_cache(cache: dict, manifest: dict, package: dict) -> None:
    NEED(cache.get('status') == 'KNOWN_PRESENT_EXACT_API_LOCALPACKAGE_READONLY'
         and type(cache.get('apiCode')) is int and cache['apiCode'] == 0
         and type(cache.get('context')) is int and cache['context'] == 2
         and cache.get('property') == 'LocalPackage' and cache.get('mutations') is False
         and cache.get('productCode') == manifest['productCode'] and cache.get('userSid') == manifest['expectedUserSid']
         and cache.get('sha256') == package['msi']['sha256']
         and type(cache.get('bytes')) is int and cache['bytes'] == package['msi']['bytes']
         and isinstance(cache.get('path'), str) and re.fullmatch(r'[A-Za-z]:\\Windows\\Installer\\[0-9a-fA-F]{1,16}\.msi', cache['path'])
         and all(type(cache.get(n)) is int and 0 <= cache[n] <= 0xffffffff for n in ('volume', 'indexHigh', 'indexLow')),
         'Direct API-selected stable whole-cache bytes equal the actual predecessor source MSI')


def validate_successor_identity(package: dict, new_version: str, streams: dict, artifacts: dict) -> None:
    NEED(B['version_parts'](package['version']) < B['version_parts'](new_version)
         and package['productCode'] != B['product_code'](new_version), 'Strictly newer actual MSI/product identity')
    NEED(package.get('guardProtocol') == ABI2['C']['GUARD_PROTOCOL'],
         'Installed release does not support this upgrade; retain its data and use a compatible installer')
    # predecessor_package already replays these old bytes against their OWN
    # original CI. Never substitute them for the successor's genuine artifacts.
    old_helper = ABI2['C']['parse_carrier_template'](streams[B['TEMPLATE_BINARY']])
    NEED(B['digest'](old_helper) == package['normalProvenance']['normal_helper_sha256']
         and B['digest'](streams[B['GUARD_BINARY']]) == package['normalGuardProvenance']['dll_sha256'],
         'Old auxiliary bytes belong to original old normal helper and guard admissions')
    helper, guard = artifacts['lifecycle-compile'][1], artifacts['guard-library-compile'][1]
    ABI2['C']['_normal'](helper)
    B['pe_dll_header'](guard)
    NEED(ABI2['C']['parse_carrier_template'](ABI2['C']['make_carrier_template'](helper)) == helper,
         'New auxiliary bytes are the actual separately admitted current normal artifacts')


def validate_predecessor_runtime(runtime: dict, manifest: dict, package: dict) -> None:
    """Check original physical receipts and direct whole-cache observations."""
    NEED(runtime.get('status') == 'PASS_ORIGINAL_WIN7_MSVC_ABI2_WIN11_CONTROLLED_PRODUCT_CLOSURE_AND_EXACT_SCRATCH_CLEANUP'
         and runtime.get('downloadVerified') is True and type(runtime.get('runtimeActualExit')) is int
         and runtime['runtimeActualExit'] == 0 and runtime.get('originalWholeBudgetExceeded') is False,
         'Actual original MSVC runtime and complete download, not a GNU or shell-only PASS')
    p = runtime['phase']
    NEED(p.get('status') == 'PASS_ORIGINAL_WIN7_MSVC_ABI2_WIN11_CONTROLLED_PRODUCT_INSTALL_LICENSE_REPAIR_UNINSTALL'
         and all(p.get(key) is False for key in ('ownerUnknown', 'instrumentUnknown', 'originalBudgetExceeded'))
         and p.get('allSetupBusinessSucceeded') is True
         and p.get('wholeMilliseconds') == 600000 and p.get('eachClientMaximumMilliseconds') == 150000
         and p.get('setupInternalSeconds') == 120 and p.get('cleanupReserveMilliseconds') == 180000,
         'Original bounded three-phase MSVC physical qualification')
    NEED(manifest.get('formalCI') is True and manifest.get('target') == TARGET
         and manifest.get('normalProfileTest') is False and manifest.get('guardABI') == 2
         and manifest.get('sourceGit') == package['sourceBase']
         and manifest.get('sourceEvidenceSha256') == package['sourceEvidenceSha256']
         and manifest.get('productCode') == '{' + package['productCode'] + '}'
         and manifest.get('version') == package['version'] and manifest.get('artifactVersion') == package['artifactVersion']
         and manifest.get('packageCode') == package['packageCode'] and manifest.get('payloads') == package['payloads'],
         'Same actual original MSVC source, product, version and complete14 runtime ledger')
    for key in ('msi', 'setup'):
        NEED(all(manifest[key][n] == package[key][n] for n in ('bytes', 'sha256')),
             'Actual runtime bundle equals original static package')
    rows = p['phases']
    NEED(len(rows) == 3 and [x['verb'] for x in rows] == ['install', 'repair', 'uninstall'],
         'Exactly three serial original maintenance phases')
    for row in rows:
        client, observer, caps = row['client'], row['observerFinal'], row['directOriginalFinal']
        NEED(client.get('started') is True and client.get('actualExit') is True
             and type(client.get('exitCode')) is int and client['exitCode'] == 0
             and client.get('pipesComplete') is True and client.get('timedOut') is False
             and row.get('knownControlsReturned') is True and row.get('nativeBusinessSucceeded') is True
             and row['postPhaseProcesses'].get('knownZero') is True
             and observer.get('Closed') is True and observer.get('Errors') == []
             and isinstance(caps, list) and len(caps) == 1, 'Original process return and observer completeness')
        cap = caps[0]
        NEED(cap.get('Status') == 'HELD_ORIGINAL_IDENTITY' and type(cap.get('Wait')) is int and cap['Wait'] == 0
             and cap.get('ExitKnown') is True and type(cap.get('ExitCode')) is int and cap['ExitCode'] == 0
             and cap.get('CloseAttempted') is True and cap.get('CheckedClose') is True
             and cap.get('Pid') == client.get('pid') and cap.get('Creation') == client.get('creationFiletime'),
             'Original B object actual wait/exit and checked close')
        processes = observer['Processes']
        NEED(len(processes) == 2 and sorted(x['Role'] for x in processes) == sorted(['setup-' + row['verb'], 'owned-guardian']),
             'Exactly the original B and one owned guardian')
        setup_process = next(x for x in processes if x['Role'] == 'setup-' + row['verb'])
        guardian = next(x for x in processes if x['Role'] == 'owned-guardian')
        NEED(setup_process.get('Pid') == client['pid'] and setup_process.get('CreationFiletime') == client['creationFiletime']
             and guardian.get('ParentPid') == client['pid'] and guardian.get('ParentCreationFiletime') == client['creationFiletime'],
             'Observed original B instance and exact owned guardian parent instance')
        for process in [cap, *processes]:
            NEED(process.get('Sha256') == manifest['setup']['sha256']
                 and process.get('Sid') == manifest['expectedUserSid'], 'Original exact Setup bytes and actual user')
        for process in processes:
            NEED(process.get('IdentityStatus') == 'ORIGINAL_QUERY_SYNC_HANDLE_EXACT_IMAGE_BYTES_USER'
                 and type(process.get('ActualWait')) is int and process['ActualWait'] == 0
                 and type(process.get('ActualExitCode')) is int and process['ActualExitCode'] == 0
                 and process.get('CloseStatus') == 'CHECKED_CLOSE_SUCCESS', 'Original B/G physical return receipts')
    for key in ('initial', 'uninstalled', 'finallyProduct'):
        NEED(p[key].get('knownAbsent') is True and p[key].get('runAbsent') is True
             and p[key].get('status') == 'PASS_READONLY_EXPECTED_ABSENT', 'Original absent product boundaries')
    for key in ('installed', 'repaired'):
        state = p[key]
        if state.get('runAbsent') is not True:
            expected_run = ('"' + state['observations']['files']['root']
                            + '\\fc-bootstrap.exe" --autostart\0').encode('utf-16-le')
            run = dict(view='Registry32', value='FutureClassroom', queryCode=0,
                       type=1, bytes=len(expected_run),
                       base64=base64.b64encode(expected_run).decode('ascii'),
                       absent=False, contentsRead=True, matchesExpected=True)
            removed = p.get('uninstallRemovedEnabledRun', {})
            NEED(manifest.get('enableRunDuringMaintenance') is True
                 and state.get('runExact') is True
                 and state['observations']['run'] == run
                 and p.get('originalEnabledRunMaintenance') == run
                 and p.get('runRepairPreserved') is True
                 and p.get('enabledRunUpgrade') == 'PASS_REPAIR_EXACT_RUN_PRESERVED'
                 and removed.get('view') == 'Registry32'
                 and removed.get('value') == 'FutureClassroom'
                 and removed.get('queryCode') == 2 and removed.get('absent') is True,
                 'Actual exact owned Run preserved by repair and removed by uninstall')
        B['require_payload_members'](package['artifactVersion'], [x['path'] for x in package['payloads']])
        NEED(state.get('healthyPresent') is True
             and p[key].get('status') == 'PASS_READONLY_EXPECTED_PRESENT', 'Installed and repaired complete14 state')
        files = state['observations']['files']['files']
        NEED(state.get('sourceGit') == manifest['sourceGit'] and state.get('currentUserSid') == manifest['expectedUserSid']
             and state.get('phaseErrors') == [] and len(files) == len(package['payloads'])
             and sorted((x['path'], x['bytes'], x['sha256']) for x in files)
                 == sorted((x['path'], x['size'], x['sha256']) for x in package['payloads'])
             and all(x.get('status') == 'EXACT' and x.get('links') == 1 for x in files),
             'Actual independent installed/repaired14 file observations match the original package')
    NEED(p['licenseDamage'].get('status') == 'DELETED_ONLY_EXACT_NEW_INSTALL_LICENSE_BY_ORIGINAL_HANDLE'
         and p['licenseDamage'].get('checkedClose') is True and p['damaged'].get('healthyPresent') is False,
         'Actual original-handle LICENSE damage followed by successful repair')
    license_row = next(x for x in package['payloads'] if x['path'] == 'LICENSE')
    NEED(p['licenseDamage'].get('bytes') == license_row['size']
         and p['licenseDamage'].get('sha256') == license_row['sha256'], 'Damage receipt binds the actual installed original LICENSE')
    for key in ('installedCache', 'repairedCache'):
        validate_predecessor_cache(p[key]['actualApi'], manifest, package)
    NEED(p['uninstalledCache'].get('allObservedAbsent') is True
         and p['uninstalledCache']['actualApi'].get('apiCode') == 1605
         and p['finallyCoordination'].get('pendingKnownAbsent') is True
         and p['finallyCoordination'].get('namespaceKnownAbsent') is True
         and runtime['cleanup'].get('knownAbsent') is True,
         'Original cache, Pending, gate and exact runtime scratch returned')
    paths = p['uninstalledCache']['paths']
    NEED({x['path'] for x in paths} == {p[key]['actualApi']['path'] for key in ('installedCache', 'repairedCache')}
         and all(x.get('status') == 'KNOWN_ABSENT' for x in paths), 'Both actually observed old cache paths are retired')


def validate_old_publication(package: dict, record: bytes | None, streams: dict) -> None:
    """Validate one old fourth-stream record as data, never re-run its history."""
    if record is None:
        NEED(package.get('status') == 'PASS_ORIGINAL_WIN7_MSVC_CI_ABI2_STATIC_CANDIDATE_NATIVE_NOT_RUN'
             and package.get('binaryReadback') == 'ALL_THREE_EXACT'
             and 'publishedPredecessor' not in package and 'predecessorPublication' not in package,
             'Initial old3 package has no fourth-stream publication claim')
        return
    meta = decode_predecessor(record)
    receipt = dict(bytes=len(record), sha256=B['digest'](record))
    publication = package['predecessorPublication']
    NEED(package.get('status') == 'PASS_ORIGINAL_WIN7_MSVC_CI_ABI2_SUCCESSOR_STATIC_CANDIDATE_NATIVE_NOT_RUN'
         and package.get('binaryReadback') == 'ALL_FOUR_EXACT'
         and package.get('publishedPredecessor') == receipt and publication.get('record') == receipt
         and publication.get('status') == 'BOUND_ORIGINAL_MSVC_PREDECESSOR_STATIC_NOT_NATIVE_UPGRADE_PASS'
         and publication.get('meta') == meta, 'Old4 original record/readback and publication metadata agree')
    prior = ABI2['C']['parse_payload_manifest'](record[248:])
    NEED(prior['package'] != package['packageCode'] and prior['product'] != package['productCode']
         and B['version_parts'](prior['version']) < B['version_parts'](package['version']),
         'Old4 retains its sealed older publication as bounded data, independently of its own current auxiliary bytes')
    normal, guard = publication['previousNormalProvenance'], publication['previousNormalGuardProvenance']
    NEED(normal.get('source_sha') == guard.get('source_sha') == meta['source_commit']
         and normal.get('result_sha256') == guard.get('result_sha256') == meta['producer_sha']
         and guard.get('dll_sha256') == meta['guard_sha']
         and normal.get('profile_test') is False and guard.get('profile_test') is False,
         'Old4 serialized original MSVC provenance agrees with its sealed record')


def predecessor_package(q: dict, old_args, original: tuple) -> tuple[dict, bytes, dict]:
    """Replay a qualified old package from real original logs and normal bytes."""
    _, artifact, payloads, artifacts, normal, guard = original
    raw = bound_fact(q['packageReport'], B['MAX_LOG'])
    package = B['json_document'](raw)
    root = Path(q['packageReport']['path']).parent
    instructions, instruction_fact = read_candidate_instructions(old_args)
    NEED(package.get('status') in ('PASS_ORIGINAL_WIN7_MSVC_CI_ABI2_STATIC_CANDIDATE_NATIVE_NOT_RUN',
                                  'PASS_ORIGINAL_WIN7_MSVC_CI_ABI2_SUCCESSOR_STATIC_CANDIDATE_NATIVE_NOT_RUN')
         and package.get('formalCI') is True and package.get('normalProfileTest') is False
         and package.get('guardProtocol') == ABI2['C']['GUARD_PROTOCOL']
         and package.get('target') == TARGET and package.get('normalProvenance') == normal
         and package.get('normalGuardProvenance') == guard and package.get('sourceBase') == normal['source_sha']
         and package.get('sourceEvidenceSha256') == normal['result_sha256']
         and package.get('sourceRoot') == str(old_args.source_root)
         and package.get('candidateInstructions') == instruction_fact
         and package.get('artifactVersion') == artifact['version'], 'Original same-CI MSVC3 admission and old builder inputs')
    NEED(bound_fact(package['sourceZip'], B['MAX_ZIP']) == original[0], 'Exact old original ZIP')
    payloads = dict(payloads, **{'MSI-CANDIDATE.txt': instructions})
    ledger = [dict(path=n, size=len(data), sha256=B['digest'](data), component_guid=B['component_code'](n))
              for n, data in sorted(payloads.items())]
    B['require_payload_members'](artifact['version'], payloads)
    NEED(package['payloads'] == ledger, 'Original complete version-specific static ledger')
    for name, data in payloads.items():
        NEED(original_file(root / 'payload' / name, B['MAX_INSTALLED']) == data, 'Exact old payload bytes')
    helper, dll = artifacts['lifecycle-compile'][1], artifacts['guard-library-compile'][1]
    template = ABI2['C']['make_carrier_template'](helper)
    msi, setup = (original_file(root / name, B['MAX_INSTALLED']) for name in ('FutureClassroom.msi', 'Setup.exe'))
    for key, data, leaf in (('msi', msi, 'FutureClassroom.msi'), ('setup', setup, 'Setup.exe')):
        NEED(package[key] == dict(path=leaf, bytes=len(data), sha256=B['digest'](data)), 'Actual old sealed bundle fact')
    NEED(ABI2['C']['verify_carrier_binding'](template, setup, msi) == helper, 'Full old MSI seal and immutable normal helper')
    body = B['lifecycle_manifest'](package['version'], payloads)
    envelope = ABI2['C']['make_payload_manifest'](package['packageCode'], body)
    NEED(package['productCode'] == B['product_code'](package['version'])
         and package['payloadReadback'] == B['payload_readback_label'](len(payloads))
         and package['onlyExternal32HoleFilled'] is True, 'Actual old ABI2 sealed complete14 package')
    prior_record = None
    if package['status'] == 'PASS_ORIGINAL_WIN7_MSVC_CI_ABI2_SUCCESSOR_STATIC_CANDIDATE_NATIVE_NOT_RUN':
        prior_record = original_file(root / 'lifecycle/published-predecessor.bin', PUBLICATION['HEADER'] + PUBLICATION['MAX_ENVELOPE'])
    def log(label):
        rows = [row for row in package['commands'] if row['name'] == label]
        NEED(len(rows) == 1 and type(rows[0]['exit_code']) is int and rows[0]['exit_code'] == 0, 'One original successful package command')
        row = rows[0]
        for stream in ('stdout', 'stderr'):
            fact = row[stream]
            NEED(set(fact) == {'path', 'size', 'sha256'} and fact['path'] == label + '.' + stream + '.log', 'Fixed original log path')
            data = original_file(root / fact['path'], B['MAX_LOG'])
            NEED(len(data) == fact['size'] and B['digest'](data) == fact['sha256'], 'Original full package log bytes')
            if stream == 'stdout': result = data
        return result
    NEED(isinstance(package['commands'], list) and 1 <= len(package['commands']) <= 160, 'Bounded original package command ledger')
    for row in package['commands']: log(row['name'])
    streams = ABI2['published_streams'](dict(zip(ABI2['BINARY_NAMES'], (dll, template, envelope))), prior_record)
    validate_old_publication(package, prior_record, streams)
    for name, data in streams.items(): NEED(log('binary-' + name) == data, 'Original exact MSI Binary extraction')
    names = log('tables').decode('ascii').splitlines()
    tables = {n: log('table-' + n) for n in set(names) & (B['CONTROLLED_REQUIRED_TABLES'] | B['FORBIDDEN_TABLES'])}
    summary = log('summary')
    NEED(summary == log('intermediate-summary'), 'Actual complete old Summary unchanged')
    _, legacy = ABI2['upgrade_sequence'](log('intermediate-sequence'))
    NEED(ABI2['validate_contract'](summary, tables, package['version'], payloads, package['packageCode'], legacy, prior_record) == package['msiContract'],
         'Replay exact original closed3 or closed4 ABI2 table contract')
    return package, msi, streams


def prepare_published_predecessor(a, current: tuple, spec: dict, spec_fact: dict) -> tuple[bytes, dict]:
    """Publish an exact released 1.0.0.x source, without inventing runtime receipts.

    The reviewed baseline pins the public EXE and original MSI. All original CI,
    source, table, payload and carrier admissions are replayed. Native Setup
    still verifies the actual installed predecessor before Begin.
    """
    NEED(set(spec) == {'publishedRelease', 'formalArguments', 'packageReport', 'publishedSetup'},
         'Closed published-release predecessor spec')
    version = spec['publishedRelease']
    NEED(isinstance(version, str) and re.fullmatch(r'1\.0\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)', version)
         and version != '1.0.0.0',
         'Only the current 1.0.x.y official release series may be a new baseline')
    baseline_path = a.source_root / 'packaging/release-baselines' / (version + '.json')
    baseline_raw = original_file(baseline_path)
    baseline = B['json_document'](baseline_raw)
    NEED(set(baseline) == {'publicVersion', 'internalVersion', 'sourceCommit', 'ciRun', 'releaseUrl', 'setupExe', 'msi'}
         and baseline['publicVersion'] == version
         and baseline['internalVersion'] == '1.' + '.'.join(version.split('.')[2:])
         and baseline['releaseUrl'] == 'https://github.com/NevermoreN/future-classroom/releases/tag/v' + version,
         'Reviewed official release baseline and exact public/internal version mapping')
    args = spec['formalArguments']
    NEED(set(args) == ARG_PATHS | ARG_HASHES, 'Original closed normal MSVC argument set')
    old_args = SimpleNamespace(**{name: Path(value) if name in ARG_PATHS else value for name, value in args.items()})
    old_admit = runpy.run_path(str(HERE / 'package-abi2-msvc-candidate.py'))['admit']
    original = old_admit(old_args)
    package, msi, streams = predecessor_package({'packageReport': spec['packageReport']}, old_args, original)
    bootstrap = B['json_document'](original_file(old_args.build_evidence / 'bootstrap.json', B['MAX_LOG']))
    NEED(package['sourceBase'] == baseline['sourceCommit'] == original[4]['source_sha']
         and package['version'] == baseline['internalVersion']
         and bootstrap['run_id'] == str(baseline['ciRun'])
         and dict(bytes=len(msi), sha256=B['digest'](msi)) == baseline['msi'],
         'Exact previously released MSI, source and original CI run')
    validate_successor_identity(package, a.product_version or current[1]['version'], streams, current[3])
    outer = bound_fact(spec['publishedSetup'], 198 << 20)
    NEED(dict(bytes=len(outer), sha256=B['digest'](outer)) == baseline['setupExe'],
         'Exact public release EXE pinned in the reviewed source baseline')
    sfx = runpy.run_path(str(HERE / 'package-standalone-exe.py'))
    helper, members = sfx['inspect'](outer)
    root = Path(spec['packageReport']['path']).parent
    NEED(helper == original[3]['lifecycle-compile'][1]
         and {n: members[n] for n in sfx['NAMES']} == {'Setup.exe': original_file(root / 'Setup.exe'), 'FutureClassroom.msi': msi,
                         'LICENSE': original_file(root / 'payload/LICENSE'),
                         'THIRD-PARTY-NOTICES.txt': original_file(root / 'payload/THIRD-PARTY-NOTICES.txt')},
         'Published EXE contains the exact original admitted helper and complete formal members')
    schema = PUBLICATION['schema_from_rust'](original_file(old_args.source_root /
        'crates/fc-windows/src/installer_lifecycle/native/broker/maintenance/schema.rs').decode('utf-8'))
    tables = PUBLICATION['capture'](root / 'FutureClassroom.msi', B['digest'](msi), schema)
    NEED(dict(next(rows for name, _, rows in tables if name == 'Binary')) == streams,
         'Actual readonly MSI Binary streams match the complete original replay')
    inventory = original_file(old_args.build_evidence / 'input-sha256.json', B['MAX_LOG'])
    meta = dict(target=1, source_commit=baseline['sourceCommit'], source_manifest_sha=B['digest'](inventory),
        producer_sha=old_args.build_result_sha256, source_msi_sha=B['digest'](msi),
        template_sha=B['digest'](streams[B['TEMPLATE_BINARY']]), template_bytes=len(streams[B['TEMPLATE_BINARY']]),
        guard_sha=B['digest'](streams[B['GUARD_BINARY']]), logical_sha=PUBLICATION['signature'](tables)[0])
    record = PUBLICATION['encode_format'](meta, streams[ABI2['MANIFEST_BINARY']], ABI2['C']['parse_payload_manifest'])
    NEED(decode_predecessor(record) == meta and old_admit(old_args) == original,
         'Final exact released source replay and predecessor encode/decode')
    return record, dict(status='BOUND_ORIGINAL_MSVC_PREDECESSOR_STATIC_NOT_NATIVE_UPGRADE_PASS',
        qualificationBasis='REVIEWED_PUBLISHED_1_0_0_X_SOURCE_REPLAY', spec=spec_fact,
        baseline=dict(publicVersion=version, bytes=len(baseline_raw), sha256=B['digest'](baseline_raw)),
        package=spec['packageReport'], publishedSetup=spec['publishedSetup'], meta=meta,
        previousNormalProvenance=original[4], previousNormalGuardProvenance=original[5],
        record=dict(bytes=len(record), sha256=B['digest'](record)),
        runtime='NOT_RUN; no new install/repair/uninstall receipt asserted',
        upgrade='NOT_RUN; native recaptures actual current registration/cache and all installed payloads before Begin')


def prepare_predecessor(a, current: tuple) -> tuple[bytes, dict] | None:
    spec_path, pin = getattr(a, 'predecessor_spec', None), getattr(a, 'predecessor_spec_sha256', None)
    NEED(bool(spec_path) == bool(pin), 'Predecessor spec path and SHA must be supplied together')
    if spec_path is None:
        return None
    raw = original_file(Path(spec_path).absolute(), B['MAX_LOG'])
    NEED(isinstance(pin, str) and re.fullmatch('[0-9a-f]{64}', pin) and B['digest'](raw) == pin, 'Pinned explicit predecessor spec')
    spec = B['json_document'](raw)
    if 'publishedRelease' in spec:
        return prepare_published_predecessor(a, current, spec,
            dict(path=str(spec_path), bytes=len(raw), sha256=pin))
    NEED(set(spec) == {'qualification', 'runtime', 'downloadRoot'}, 'Closed predecessor original-evidence spec; no caller permit fields')
    q = B['json_document'](bound_fact(spec['qualification'], B['MAX_LOG']))
    NEED(set(q) == {'status', 'sourceGit', 'expectedUserSid', 'formalArguments', 'packageReport'}
         and q['status'] == 'ROOT_SELECTED_ORIGINAL_MSVC_ABI2_FOR_WIN11_THREE_PHASE', 'Original MSVC qualification config')
    args = q['formalArguments']
    NEED(set(args) == ARG_PATHS | ARG_HASHES, 'Original closed3 normal MSVC argument set')
    for name in ARG_PATHS:
        NEED(isinstance(args[name], str) and Path(args[name]).is_absolute(), 'Absolute original source and CI argument')
    old_args = SimpleNamespace(**{name: Path(value) if name in ARG_PATHS else value for name, value in args.items()})
    # A fresh module owns its own B/configure_source_root; old admission must not
    # mutate the current package's source domain or rely on old executable code.
    old_admit = runpy.run_path(str(HERE / 'package-abi2-msvc-candidate.py'))['admit']
    original = old_admit(old_args)
    NEED(original[4]['source_sha'] == q['sourceGit'], 'Original predecessor source commit')
    package, msi, streams = predecessor_package(q, old_args, original)
    new_version = a.product_version or current[1]['version']
    validate_successor_identity(package, new_version, streams, current[3])
    runtime = B['json_document'](bound_fact(spec['runtime'], B['MAX_LOG']))
    manifest = B['json_document'](bound_fact(runtime['artifactManifest'], B['MAX_LOG']))
    NEED(manifest['qualificationInput'] == spec['qualification'] and manifest['packageReport'] == q['packageReport']
         and manifest['expectedUserSid'] == q['expectedUserSid'], 'Actual runtime bound to selected old qualification')
    validate_predecessor_runtime(runtime, manifest, package)
    source = B['json_document'](bound_fact(runtime['sourceManifest'], B['MAX_LOG']))
    inventory_raw = original_file(old_args.build_evidence / 'input-sha256.json', B['MAX_LOG'])
    result = B['json_document'](original_file(old_args.build_evidence / 'result.json', B['MAX_LOG']))
    NEED(manifest['sourceManifest'] == runtime['sourceManifest']
         and source == dict(sourceBase=q['sourceGit'], sources=B['json_document'](inventory_raw))
         and B['digest'](inventory_raw) == result['input_manifest_sha256'], 'Actual complete old raw CI source manifest')
    download = Path(spec['downloadRoot'])
    NEED(download.is_absolute() and download == download.resolve(), 'Exact original download root')
    B['PROBE']['real_directory'](download)
    files = runtime['inventory']['files']
    NEED(isinstance(files, list) and 1 <= len(files) <= 160, 'Finite original complete download inventory')
    downloaded = {}
    for fact in files:
        name = fact['path']
        NEED(isinstance(name, str) and '\\' not in name and all(x not in ('', '.', '..') for x in name.split('/'))
             and not Path(name).is_absolute() and name not in downloaded, 'Unique closed downloaded relative path')
        downloaded[name] = bound_fact(dict(fact, path=str(download / name)))
    NEED(B['json_document'](downloaded['product-result.json']) == runtime['phase']
         and downloaded['FutureClassroom.msi'] == msi
         and downloaded['Setup.exe'] == original_file(Path(q['packageReport']['path']).parent / 'Setup.exe')
         and downloaded['artifact-manifest.json'] == bound_fact(runtime['artifactManifest'])
         and downloaded['source-manifest.json'] == bound_fact(runtime['sourceManifest']), 'Actual downloaded phase/source/bundle bytes')
    schema = PUBLICATION['schema_from_rust'](original_file(old_args.source_root / 'crates/fc-windows/src/installer_lifecycle/native/broker/maintenance/schema.rs').decode('utf-8'))
    source_msi = Path(q['packageReport']['path']).parent / 'FutureClassroom.msi'
    tables = PUBLICATION['capture'](source_msi, B['digest'](msi), schema)
    captured_streams = dict(next(rows for name, _, rows in tables if name == 'Binary'))
    NEED(captured_streams == streams, 'Actual readonly old MSI closed3/closed4 Binary bytes including old record, not report assertions')
    meta = dict(target=1, source_commit=q['sourceGit'], source_manifest_sha=B['digest'](inventory_raw),
                producer_sha=old_args.build_result_sha256, source_msi_sha=B['digest'](msi),
                template_sha=B['digest'](streams[B['TEMPLATE_BINARY']]), template_bytes=len(streams[B['TEMPLATE_BINARY']]),
                guard_sha=B['digest'](streams[B['GUARD_BINARY']]), logical_sha=PUBLICATION['signature'](tables)[0])
    record = PUBLICATION['encode_format'](meta, streams[ABI2['MANIFEST_BINARY']], ABI2['C']['parse_payload_manifest'])
    NEED(decode_predecessor(record) == meta and old_admit(old_args) == original, 'Final original old source/admission and record replay')
    return record, dict(status='BOUND_ORIGINAL_MSVC_PREDECESSOR_STATIC_NOT_NATIVE_UPGRADE_PASS',
        spec=dict(path=str(spec_path), bytes=len(raw), sha256=pin), qualification=spec['qualification'],
        package=q['packageReport'], runtime=spec['runtime'], meta=meta,
        previousNormalProvenance=original[4], previousNormalGuardProvenance=original[5],
        cacheEvidence=dict(method='DIRECT_STABLE_API_LOCALPACKAGE_WHOLE_FILE_EQUAL_TO_SOURCE',
            nativeCacheLogicalDigestSerialized=False, installed=runtime['phase']['installedCache']['actualApi'],
            repaired=runtime['phase']['repairedCache']['actualApi']),
        record=dict(bytes=len(record), sha256=B['digest'](record)),
        upgrade='NOT_RUN; native recaptures current actual old registration/cache before Begin')


def check_compile_result(result: dict, require_coordinator: bool = False) -> None:
    """A closed compilation contract, additional to the shared CI admissions."""
    NEED(result.get('status') == 'COMPILE_CHECKS_PASSED_RUNTIME_NOT_RUN'
         and result.get('baseline') == 'Win7' and result.get('target') == TARGET
         and result.get('toolchain') == 'nightly-2026-10-29-x86_64-pc-windows-msvc'
         and result.get('rustflags') == '-C target-feature=+crt-static -C link-arg=/SUBSYSTEM:CONSOLE,6.01'
         and result.get('product_rustflags') == '-C target-feature=+crt-static -C link-arg=/SUBSYSTEM:WINDOWS,6.01',
         'Original successful fixed Win7 MSVC compile and static CRT flags required')
    commands = result.get('commands')
    NEED(isinstance(commands, list) and 1 <= len(commands) <= 160
         and all(isinstance(row, dict) and row.get('passed') is True for row in commands),
         'Complete original CI command set must pass')
    phases = [('product-compile', 'fc-bootstrap', 'fc-bootstrap'),
              ('lifecycle-compile', 'fc-bootstrap', 'fc-installer-lifecycle'),
              ('guard-library-compile', 'fc-windows', None)]
    if require_coordinator:
        phases.append(('update-coordinator-compile', 'fc-bootstrap', 'fc-update-coordinator'))
    for phase, package, target in phases:
        expected = ['-Z', 'build-std', 'build' if target is None else 'rustc',
            '-vv', '--release', '--locked', '--target', TARGET, '-p', package]
        expected += ['--lib'] if target is None else ['--bin', target]
        expected += ['--message-format=json']
        rows = [row for row in commands if row.get('name') == phase]
        NEED(len(rows) == 1 and rows[0].get('arguments') == expected
             and type(rows[0].get('exit_code')) is int and rows[0]['exit_code'] == 0
             and rows[0].get('parent_exited') is True
             and rows[0].get('timed_out_or_pipe_stalled') is False
             and rows[0].get('log_truncated') is False,
             'Exact original closed normal compiler command: ' + phase)


def admit(a):
    """Reuse the original formal helper/guard CI, metadata and raw-source gates."""
    B['configure_source_root'](a.source_root)
    B['PROBE']['real_directory'](a.input_zip.parent)
    raw = B['PROBE']['bounded_regular_file'](a.input_zip, B['MAX_ZIP'])
    NEED(re.fullmatch(r'[0-9a-f]{64}', a.input_sha256) is not None
         and B['digest'](raw) == a.input_sha256, 'Explicit original input ZIP SHA')
    manifest, payloads = B['validate_zip'](raw)
    B['pe_header'](payloads['fc-bootstrap.exe'])
    helper, provenance = B['admit_normal_helper'](a, payloads, manifest['version'])
    guard, guard_provenance = B['admit_normal_guard'](a, provenance, manifest['version'])
    result_raw = B['PROBE']['bounded_regular_file'](a.build_evidence / 'result.json', B['MAX_LOG'])
    NEED(B['digest'](result_raw) == a.build_result_sha256 == provenance['result_sha256'],
         'Same original result after all four artifact admissions')
    result = B['json_document'](result_raw)
    require_coordinator = B['version_parts'](manifest['version']) >= (0, 1, 5)
    check_compile_result(result, require_coordinator=require_coordinator)
    product = payloads['fc-bootstrap.exe']
    coordinator = payloads.get('fc-update-coordinator.exe')
    for image in (product, helper, guard) + ((coordinator,) if require_coordinator else ()):
        pe = struct.unpack_from('<I', image, 60)[0]
        NEED(struct.unpack_from('<HH', image, pe + 72) == (6, 1),
             'Each actual original MSVC PE declares subsystem version6.1')
    artifacts = {
        'product-compile': (a.build_evidence / 'product/fc-bootstrap/fc-bootstrap.exe', product),
        'lifecycle-compile': (a.lifecycle_helper, helper),
        **({'update-coordinator-compile': (a.build_evidence / 'product/fc-bootstrap/fc-update-coordinator.exe', coordinator)} if require_coordinator else {}),
        'guard-library-compile': (a.guard_library, guard),
    }
    return raw, manifest, payloads, artifacts, provenance, guard_provenance


def builder_sources() -> list[dict]:
    names = ('package-abi2-msvc-candidate.py', 'package-abi2-upgrade-dev.py',
        'package-controlled-dev.py', 'package-msi-candidate.py',
        'installer-carrier-format.py', 'package-probe.py', 'audit-pe.py',
        'installer-upgrade-publication.py')
    return [{'path': 'scripts/' + name,
        'bytes': (HERE / name).stat().st_size,
        'sha256': B['file_digest'](HERE / name)} for name in names]


def read_candidate_instructions(a) -> tuple[bytes, dict]:
    """A separately reviewed builder document, never substituted CI source."""
    path = a.candidate_instructions.absolute()
    NEED(path.name == 'CONTROLLED-MSI-CANDIDATE.md', 'Explicit canonical candidate instructions name')
    B['PROBE']['real_directory'](path.parent)
    raw = B['PROBE']['bounded_regular_file'](path, 65536)
    NEED(re.fullmatch(r'[0-9a-f]{64}', a.candidate_instructions_sha256) is not None
         and B['digest'](raw) == a.candidate_instructions_sha256,
         'Actual separately reviewed candidate instructions bytes/SHA')
    ABI2['instructions_bytes'](a.source_root, raw)
    return raw, {'path': str(path), 'bytes': len(raw), 'sha256': B['digest'](raw),
        'scope': 'REVIEWED_BUILDER_DOCUMENT_NOT_ORIGINAL_CI_SOURCE'}


def build(a) -> dict:
    a.source_root = a.source_root.absolute()
    a.input_zip = a.input_zip.absolute()
    a.build_evidence = a.build_evidence.absolute()
    a.lifecycle_helper = a.lifecycle_helper.absolute()
    a.guard_library = a.guard_library.absolute()
    raw, manifest, payloads, artifacts, provenance, guard_provenance = admit(a)
    original_builders = builder_sources()
    original_instructions, instructions_fact = read_candidate_instructions(a)
    admitted = (raw, manifest, payloads, artifacts, provenance, guard_provenance)
    predecessor = prepare_predecessor(a, admitted)
    identity = {
        'scope': 'ORIGINAL_WIN7_MSVC_CI_ABI2_UPGRADE_STATIC_CANDIDATE_RUNTIME_NOT_RUN',
        'formalCI': True, 'target': TARGET, 'sourceBase': provenance['source_sha'],
        'sourceEvidenceSha256': a.build_result_sha256,
        'normalProvenance': provenance, 'normalGuardProvenance': guard_provenance,
        'lifecycleMode': 'ABI2_UPGRADE_CAPABLE_ORIGINAL_WIN7_MSVC_CI_CANDIDATE',
        'builderSources': original_builders,
        'sourceZip': {'path': str(a.input_zip), 'bytes': len(raw), 'sha256': B['digest'](raw)},
        'candidateInstructions': instructions_fact,
    }
    if predecessor is not None:
        identity['predecessorPublication'] = predecessor[1]
    def verify_source():
        current = admit(a)
        NEED(current == (raw, manifest, payloads_before, artifacts, provenance, guard_provenance),
             'Original input ZIP, full source, CI result, metadata and four artifacts unchanged')
        NEED(builder_sources() == original_builders, 'Exact reviewed assembly dependencies unchanged')
        NEED(read_candidate_instructions(a) == (original_instructions, instructions_fact),
             'Exact separately reviewed candidate instructions unchanged')
        NEED(prepare_predecessor(a, current) == predecessor, 'Original predecessor package/source/cache evidence unchanged')
    # Assembly adds its fixed instructions payload; preserve the admitted ZIP set
    # separately so final re-admission compares the original namespace, not it.
    payloads_before = dict(payloads)
    status = ('PASS_ORIGINAL_WIN7_MSVC_CI_ABI2_STATIC_CANDIDATE_NATIVE_NOT_RUN' if predecessor is None else
              'PASS_ORIGINAL_WIN7_MSVC_CI_ABI2_SUCCESSOR_STATIC_CANDIDATE_NATIVE_NOT_RUN')
    return ABI2['assemble_admitted'](a, manifest, payloads, artifacts, identity,
        status, verify_source, instructions=original_instructions,
        predecessor=None if predecessor is None else predecessor[0])


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--execute', action='store_true')
    parser.add_argument('--source-root', type=Path, default=HERE.parent,
        help='Exact original CI checkout bytes; builder identity is recorded separately')
    parser.add_argument('--product-version', help='Separate MSI identity; never rewrites original compiler bytes or artifact version')
    parser.add_argument('--predecessor-spec', type=Path,
        help='Pinned reviewed 1.0.0.x published-source baseline, or original historical qualification evidence')
    parser.add_argument('--predecessor-spec-sha256')
    for name in ('input-zip', 'lifecycle-helper', 'guard-library', 'build-evidence', 'candidate-instructions',
            'output-dir', 'wixl', 'msibuild', 'msiinfo', 'msiextract', 'objdump'):
        parser.add_argument('--' + name, type=Path)
    for name in ('input-sha256', 'lifecycle-helper-sha256', 'guard-library-sha256',
            'build-result-sha256', 'msibuild-sha256', 'candidate-instructions-sha256'):
        parser.add_argument('--' + name)
    a = parser.parse_args()
    if not a.execute:
        print(json.dumps({'status': 'NOT_RUN', 'candidateOnly': True,
            'Windows': 'NOT_CONTACTED', 'productionReadiness': 'NOT_GRANTED'}))
        return 0
    NEED(all(getattr(a, name) for name in ('input_zip', 'input_sha256', 'lifecycle_helper',
        'lifecycle_helper_sha256', 'guard_library', 'guard_library_sha256', 'build_evidence',
        'build_result_sha256', 'output_dir', 'wixl', 'msibuild', 'msibuild_sha256',
        'msiinfo', 'msiextract', 'objdump', 'candidate_instructions',
        'candidate_instructions_sha256')), 'Every original CI/input/tool/output and reviewed instructions argument required')
    report = build(a)
    print(json.dumps({'status': report['status'], 'output': str(a.output_dir),
        'Windows': 'NOT_RUN', 'productionReadiness': 'NOT_GRANTED'}))
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
