#!/usr/bin/env python3
"""Prepare test-only observer authoring/check exports; never build or execute MSI."""
from pathlib import Path, PureWindowsPath
import argparse
import hashlib
import json
import re
import stat
import struct
import subprocess
import uuid
import xml.etree.ElementTree as ET

NS = 'http://schemas.microsoft.com/wix/2006/wi'
NAMESPACE = uuid.UUID('BE01A185-9988-441B-A60E-FB5DB5FBEAD3')
ABORT = 'Owned readonly context probe intentionally stops before InstallInitialize.'
TOKEN = 'fc-owned-parent-observer-v1:'
FORBIDDEN = {'File', 'Component', 'FeatureComponents', 'Registry', 'RemoveRegistry', 'RemoveFile',
             'CreateFolder', 'ServiceInstall', 'ServiceControl', 'Shortcut', 'Environment', 'IniFile',
             'RemoveIniFile', 'LockPermissions', 'MsiLockPermissionsEx', 'MsiEmbeddedChainer', 'Upgrade'}
ALLOWED = {'Binary', 'CustomAction', 'InstallExecuteSequence', 'InstallUISequence', 'AdminExecuteSequence',
           'AdminUISequence', 'AdvtExecuteSequence', 'Property', 'Directory', 'Feature', 'Media', 'LaunchCondition', 'Error'}
SYSTEM_TABLES = {'_Tables', '_Columns', '_Validation', '_SummaryInformation'}


def need(value, message):
    if not value:
        raise ValueError(message)


def sha(data):
    return hashlib.sha256(data).hexdigest()


def read(path, limit=32 * 1024 * 1024):
    need(stat.S_ISREG(path.lstat().st_mode) and 0 <= path.stat().st_size <= limit, 'bounded regular input required')
    return path.read_bytes()


def pe_offset(data, subsystem):
    need(len(data) >= 64 and data[:2] == b'MZ', 'actual PE bytes required')
    offset = struct.unpack_from('<I', data, 60)[0]
    need(offset + 120 <= len(data) and data[offset:offset + 4] == b'PE\0\0'
         and struct.unpack_from('<H', data, offset + 4)[0] == 0x14c
         and struct.unpack_from('<H', data, offset + 20)[0] >= 96
         and struct.unpack_from('<H', data, offset + 24)[0] == 0x10b
         and struct.unpack_from('<H', data, offset + 92)[0] == subsystem, 'x86 PE32 test fixture subsystem differs')
    return offset


def validate_fixture(source, fixture, ledger):
    """Verify an independently made test-only clone; never create/patch one."""
    need(ledger['status'] == 'PASS_EXACT_CUI3_TO_GUI2_TEST_FIXTURE_CLONE'
         and ledger['profileTest'] is True and ledger['normalProduct'] is False, 'test-fixture ledger required')
    need(len(source) == ledger['sourceCui']['bytes'] == ledger['guiFixture']['bytes'] == len(fixture)
         and sha(source) == ledger['sourceCui']['sha256']
         and sha(fixture) == ledger['guiFixture']['sha256'], 'source/clone identity differs')
    cargo = ledger['cargoArtifact']
    need(cargo['package'] == 'fc-windows' and cargo['target'] == 'fc_windows' and cargo['role'] == 'tests'
         and cargo['cargo_profile_test'] is True and cargo['sha256'] == sha(source), 'original fc_windows test provenance required')
    need(re.fullmatch(r'[0-9a-f]{40}', ledger['sourceGitSha']), 'actual source Git SHA required')
    offset = pe_offset(source, 3)
    need(pe_offset(fixture, 2) == offset, 'clone PE layout changed')
    fields = ledger['changedFields']
    need([field['name'] for field in fields] in [['Subsystem'], ['Subsystem', 'CheckSum']], 'only explicit Subsystem and optional CheckSum fields allowed')
    patched = bytearray(source)
    for field in fields:
        expected_offset, width = (offset + 92, 2) if field['name'] == 'Subsystem' else (offset + 88, 4)
        need(set(field) == {'name', 'offset', 'beforeHex', 'afterHex'} and field['offset'] == expected_offset,
             'patch field schema/offset differs')
        before = source[expected_offset:expected_offset + width]
        after = fixture[expected_offset:expected_offset + width]
        need(field['beforeHex'] == before.hex() and field['afterHex'] == after.hex()
             and before != after, 'patch field byte identities differ')
        if field['name'] == 'Subsystem':
            need(before == b'\x03\0' and after == b'\x02\0', 'only CUI3 to GUI2 conversion allowed')
        patched[expected_offset:expected_offset + width] = after
    need(bytes(patched) == fixture, 'code/entry/imports/other PE bytes changed')
    return {'sourceCuiSha256': sha(source), 'guiFixtureSha256': sha(fixture),
            'profileTest': True, 'normalProduct': False, 'changedFields': fields}


def fixture_from_args(args):
    raw = read(args.fixture_ledger, 1024 * 1024)
    need(sha(raw) == args.fixture_ledger_sha256, 'explicit fixture ledger SHA differs')
    ledger = json.loads(raw)
    source = read(Path(ledger['sourceCui']['path']))
    fixture = read(Path(ledger['guiFixture']['path']))
    proof = validate_fixture(source, fixture, ledger)
    need(args.observer_sha256 == proof['guiFixtureSha256'], 'observer SHA must identify GUI test fixture')
    return fixture, proof


def arguments(test, root, nonce):
    need(re.fullmatch(r'installer_lifecycle::(?:[a-z_][a-z_0-9]*::)+[a-z_][a-z_0-9]*', test), 'exact future cfg(test) name required')
    need(re.fullmatch(r'[0-9a-f]{32}', nonce), 'nonce must be 32 lowercase hexadecimal characters')
    path = PureWindowsPath(root)
    need(path.is_absolute() and re.fullmatch(r'[A-Za-z]:', path.drive) and '\\' in root
         and path.name == 'FutureClassroom-r75-parent-observer-' + nonce and len(root.encode('utf-16-le')) <= 480, 'fixed owned Windows report root required')
    need(root == str(path) and not any(c in root for c in '\x00\r\n\t"[]*?<>|/')
         and all(part not in {'.', '..'} and not part.endswith((' ', '.')) for part in path.parts[1:]), 'report path alias or MSI formatting rejected')
    return ['--exact', test, '--test-threads=1', '--nocapture', '--skip', TOKEN + nonce + ':' + root]


def authoring(test, root, nonce):
    argv = arguments(test, root, nonce)
    ET.register_namespace('', NS)
    def add(parent, tag, **attributes):
        return ET.SubElement(parent, '{' + NS + '}' + tag, attributes)
    wix = ET.Element('{' + NS + '}Wix')
    product = add(wix, 'Product', Id=str(uuid.uuid5(NAMESPACE, nonce + ':product')).upper(),
                  UpgradeCode=str(uuid.uuid5(NAMESPACE, nonce + ':family')).upper(), Name='FutureClassroom readonly context probe',
                  Language='1033', Version='0.0.1', Manufacturer='FutureClassroom test-only')
    add(product, 'Package', InstallerVersion='500', InstallScope='perUser', Compressed='yes')
    add(product, 'Property', Id='MSIRESTARTMANAGERCONTROL', Value='DisableShutdown')
    add(product, 'Condition', Message='Readonly context probe is per-user only.').text = 'NOT ALLUSERS'
    add(product, 'Binary', Id='FcReadonlyObserver', SourceFile='observer-harness.exe')
    add(product, 'CustomAction', Id='FcObserveParent', BinaryKey='FcReadonlyObserver',
        ExeCommand=subprocess.list2cmdline(argv), Return='check', Impersonate='yes')
    add(product, 'CustomAction', Id='FcAbortBeforeInitialize', Error=ABORT)
    sequence = add(product, 'InstallExecuteSequence')
    add(sequence, 'Custom', Action='FcObserveParent', Sequence='1499').text = '1'
    add(sequence, 'Custom', Action='FcAbortBeforeInitialize', Sequence='1500').text = '1'
    add(sequence, 'InstallInitialize', Sequence='1501')
    add(product, 'Directory', Id='TARGETDIR', Name='SourceDir')
    add(product, 'Feature', Id='ObserverFeature', Title='Readonly observer; no installed payload', Level='1')
    ET.indent(wix)
    return ET.tostring(wix, encoding='utf-8', xml_declaration=True) + b'\n'


def idt(raw, name):
    need(len(raw) <= 1024 * 1024, 'IDT too large')
    lines = raw.decode('utf-8').splitlines()
    need(len(lines) >= 3 and lines[2].split('\t')[0] == name, 'IDT table header')
    columns = lines[0].split('\t')
    need(len(set(columns)) == len(columns) and len(lines[1].split('\t')) == len(columns), 'IDT columns')
    rows = [dict(zip(columns, line.split('\t'))) for line in lines[3:]]
    need(all(len(line.split('\t')) == len(columns) for line in lines[3:]), 'IDT row width')
    return rows


def verify(tables, command):
    need({'Binary', 'CustomAction', 'InstallExecuteSequence'} <= tables.keys(), 'required exports absent')
    need(all(not tables.get(name) for name in FORBIDDEN), 'probe must have no payload/registry/chainer effects')
    need(all(not rows for name, rows in tables.items() if name not in ALLOWED | SYSTEM_TABLES), 'unknown nonempty effects table')
    need(tables['Binary'] == [{'Name': 'FcReadonlyObserver', 'Data': 'Binary.FcReadonlyObserver'}], 'only actual observer Binary admitted')
    expected = [{'Action': 'FcObserveParent', 'Type': '2', 'Source': 'FcReadonlyObserver', 'Target': command},
                {'Action': 'FcAbortBeforeInitialize', 'Type': '19', 'Source': '', 'Target': ABORT}]
    need(sorted(tables['CustomAction'], key=lambda r: r['Action']) == sorted(expected, key=lambda r: r['Action']), 'exact Type2/19 actions required')
    rows = tables['InstallExecuteSequence']
    need(len({r['Action'] for r in rows}) == len(rows), 'duplicate execute action')
    actions = {r['Action']: (r['Condition'], int(r['Sequence'])) for r in rows}
    need(actions.get('FcObserveParent') == ('1', 1499) and actions.get('FcAbortBeforeInitialize') == ('1', 1500)
         and actions.get('InstallInitialize') == ('', 1501), 'observer/abort/Initialize order differs')
    need(0 < actions['CostFinalize'][1] < actions['InstallValidate'][1] < 1499, 'directory costing/validation not before observer')
    need(all(r['Action'] not in {'FcObserveParent', 'FcAbortBeforeInitialize'} for r in tables.get('InstallUISequence', [])), 'observer must not run in UI sequence')
    return {'status': 'PASS_EXPORTED_PROBE_TOPOLOGY_ONLY', 'observerRuntime': 'NOT_RUN', 'installation': 'EXPECTED_ABORT_NOT_INSTALL_SUCCESS'}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--observer-test-name', required=True)
    parser.add_argument('--windows-report-root', required=True)
    parser.add_argument('--nonce', required=True)
    sub = parser.add_subparsers(dest='mode', required=True)
    sub.add_parser('describe')
    emit = sub.add_parser('author')
    emit.add_argument('--observer-exe', type=Path, required=True)
    emit.add_argument('--observer-sha256', required=True)
    emit.add_argument('--fixture-ledger', type=Path, required=True)
    emit.add_argument('--fixture-ledger-sha256', required=True)
    emit.add_argument('--actual-cli-proof', type=Path, required=True)
    emit.add_argument('--cli-proof-sha256', required=True)
    emit.add_argument('--output-dir', type=Path, required=True)
    check = sub.add_parser('check-exports')
    check.add_argument('--exports', type=Path, required=True)
    check.add_argument('--tables-list', type=Path, required=True, help='Original complete msiinfo tables stdout')
    check.add_argument('--extracted-observer', type=Path, required=True)
    check.add_argument('--observer-sha256', required=True)
    check.add_argument('--fixture-ledger', type=Path, required=True)
    check.add_argument('--fixture-ledger-sha256', required=True)
    args = parser.parse_args()
    argv = arguments(args.observer_test_name, args.windows_report_root, args.nonce)
    command = subprocess.list2cmdline(argv)
    if args.mode == 'describe':
        print(json.dumps({'status': 'PREPARED_ONLY_NO_OBSERVER_OR_MSI', 'argv': argv,
                          'commandLine': command, 'requiredCliParseArgs': argv + ['--list'],
                          'requiredMarker': 'future-classroom-owned-r75-parent-observer/v1\n',
                          'requiredFixture': 'ROOT_APPROVED_GUI2_TEST_CLONE; NOT_CREATED_OR_CHECKED_BY_DESCRIBE',
                          'observerRuntime': 'NOT_RUN', 'msiProduced': False}))
    elif args.mode == 'author':
        observer = read(args.observer_exe)
        fixture, fixture_proof = fixture_from_args(args)
        need(observer == fixture, 'Binary must be the exact GUI2 test fixture')
        raw = read(args.actual_cli_proof, 1024 * 1024)
        need(sha(raw) == args.cli_proof_sha256, 'CLI proof SHA mismatch')
        proof = json.loads(raw)
        need(proof['status'] == 'ACTUAL_HARNESS_CLI_PARSE_PASS_NOT_OBSERVER_RUNTIME'
             and proof['observerSha256'] == args.observer_sha256 and proof['actualExit'] is True
             and proof['exitCode'] == 0 and proof['commandArgs'] == argv + ['--list']
             and proof['selectedTests'] == [args.observer_test_name], 'actual CLI parser proof required')
        need(not args.output_dir.exists() and not args.output_dir.is_symlink(), 'fresh output required')
        args.output_dir.mkdir()
        (args.output_dir / '.owner').write_text('future-classroom-owned-r75-parent-observer-authoring/v1\n')
        (args.output_dir / 'observer-harness.exe').write_bytes(observer)
        (args.output_dir / 'context-probe.wxs').write_bytes(authoring(args.observer_test_name, args.windows_report_root, args.nonce))
        (args.output_dir / 'fixture-ledger.json').write_bytes(read(args.fixture_ledger, 1024 * 1024))
        print(json.dumps({'status': 'AUTHORING_ONLY_MSI_NOT_BUILT', 'observerSha256': sha(observer),
                          'fixture': fixture_proof, 'observerRuntime': 'NOT_RUN'}))
    else:
        tables = {p.stem: idt(read(p, 1024 * 1024), p.stem) for p in args.exports.glob('*.idt')}
        listed = read(args.tables_list, 64 * 1024).decode('utf-8').splitlines()
        need(len(listed) == len(set(listed)) and all(re.fullmatch(r'[A-Za-z_][A-Za-z_0-9]*', name) for name in listed), 'actual complete table listing required')
        need(set(tables) == set(listed) - SYSTEM_TABLES, 'all non-system actual tables must be exported')
        properties = {r['Property']: r['Value'] for r in tables['Property']}
        need(properties['ProductCode'].strip('{}').upper() == str(uuid.uuid5(NAMESPACE, args.nonce + ':product')).upper()
             and properties['UpgradeCode'].strip('{}').upper() == str(uuid.uuid5(NAMESPACE, args.nonce + ':family')).upper()
             and properties['MSIRESTARTMANAGERCONTROL'] == 'DisableShutdown', 'isolated probe product identity/RM policy differs')
        extracted = read(args.extracted_observer)
        fixture, _ = fixture_from_args(args)
        need(extracted == fixture, 'actual Binary stream differs from exact GUI2 test fixture')
        print(json.dumps(verify(tables, command)))


if __name__ == '__main__':
    main()
