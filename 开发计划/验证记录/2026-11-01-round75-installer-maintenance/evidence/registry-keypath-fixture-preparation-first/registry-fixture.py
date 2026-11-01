#!/usr/bin/env python3
"""Author/check an isolated registry-only MSI fixture; never build/install it."""
from pathlib import Path
import argparse
import hashlib
import json
import re
import stat
import uuid
import xml.etree.ElementTree as ET

NS = 'http://schemas.microsoft.com/wix/2006/wi'
SPACE = uuid.UUID('04E22830-40E1-43D2-A610-F36EAB21A955')
SYSTEM = {'_Tables', '_Columns', '_Validation', '_SummaryInformation'}
ALLOWED = {'Property', 'Directory', 'Component', 'Registry', 'Feature', 'FeatureComponents', 'LaunchCondition',
           'InstallExecuteSequence', 'InstallUISequence', 'AdminExecuteSequence', 'AdminUISequence', 'AdvtExecuteSequence'}
STANDARD = {'LaunchConditions': 100, 'ValidateProductID': 700, 'CostInitialize': 800, 'FileCost': 900,
            'CostFinalize': 1000, 'InstallValidate': 1400, 'InstallInitialize': 1500,
            'AllocateRegistrySpace': 1550, 'ProcessComponents': 1600, 'UnpublishComponents': 1700,
            'UnpublishFeatures': 1800, 'RemoveRegistryValues': 2600, 'RemoveFiles': 3500, 'RemoveFolders': 3600,
            'CreateFolders': 3700, 'InstallFiles': 4000, 'WriteRegistryValues': 5000,
            'RegisterUser': 6000, 'RegisterProduct': 6100, 'PublishComponents': 6200,
            'PublishFeatures': 6300, 'PublishProduct': 6400, 'InstallExecute': 6500, 'InstallFinalize': 6600}


def need(value, message):
    if not value:
        raise ValueError(message)


def sha(data):
    return hashlib.sha256(data).hexdigest()


def spec(nonce):
    need(re.fullmatch('[0-9a-f]{32}', nonce), 'explicit 32 lower-hex nonce required')
    def guid(role):
        return '{' + str(uuid.uuid5(SPACE, nonce + ':' + role)).upper() + '}'
    return {'schemaVersion': 1, 'scope': 'TEST_ONLY_REGISTRY_KEYPATH_FIXTURE', 'nonce': nonce,
            'productCode': guid('product'), 'upgradeCode': guid('family'), 'componentCode': guid('component'),
            'registryKey': 'Software\\FutureClassroom-Test-RegistryKeyPath-' + nonce,
            'registryValue': 'OwnedMarker', 'registryData': 1, 'registryView': 'Registry32', 'version': '0.0.1'}


def authoring(nonce):
    value = spec(nonce)
    ET.register_namespace('', NS)
    def add(parent, name, **attributes):
        return ET.SubElement(parent, '{' + NS + '}' + name, attributes)
    wix = ET.Element('{' + NS + '}Wix')
    product = add(wix, 'Product', Id=value['productCode'], UpgradeCode=value['upgradeCode'],
                  Name='FutureClassroom registry keypath fixture', Manufacturer='FutureClassroom test-only',
                  Version=value['version'], Language='1033', Codepage='1252')
    add(product, 'Package', InstallerVersion='500', InstallScope='perUser', Compressed='yes')
    add(product, 'Property', Id='MSIRESTARTMANAGERCONTROL', Value='DisableShutdown')
    add(product, 'Condition', Message='Registry fixture is per-user only.').text = 'NOT ALLUSERS'
    add(product, 'Condition', Message='Registry fixture cannot automatically close applications.').text = 'MSIRESTARTMANAGERCONTROL = "DisableShutdown"'
    target = add(product, 'Directory', Id='TARGETDIR', Name='SourceDir')
    local = add(target, 'Directory', Id='LocalAppDataFolder')
    component = add(local, 'Component', Id='RegistryProbe', Guid=value['componentCode'], Win64='no')
    add(component, 'RegistryValue', Id='RegistryMarker', Root='HKCU', Key=value['registryKey'],
        Name=value['registryValue'], Type='integer', Value='1', KeyPath='yes')
    feature = add(product, 'Feature', Id='RegistryFeature', Title='Registry-only fixture', Level='1')
    add(feature, 'ComponentRef', Id='RegistryProbe')
    ET.indent(wix)
    return ET.tostring(wix, encoding='utf-8', xml_declaration=True) + b'\n'


def read(path, maximum=1024 * 1024):
    need(stat.S_ISREG(path.lstat().st_mode) and 0 <= path.stat().st_size <= maximum, 'bounded regular input required')
    return path.read_bytes()


def idt(data, name):
    lines = data.decode('utf-8').splitlines()
    need(len(lines) >= 3 and lines[2].split('\t')[0] == name, 'actual IDT header required')
    columns = lines[0].split('\t')
    keys = lines[2].split('\t')[1:]
    need(keys and len(set(columns)) == len(columns) and len(lines[1].split('\t')) == len(columns)
         and set(keys) <= set(columns), 'IDT schema')
    need(all(len(line.split('\t')) == len(columns) for line in lines[3:]), 'IDT row width')
    rows = [dict(zip(columns, line.split('\t'))) for line in lines[3:]]
    need(len({tuple(row[key] for key in keys) for row in rows}) == len(rows), 'duplicate actual IDT key')
    return rows


def verify(tables, nonce, summary):
    value = spec(nonce)
    required = {'Property', 'Directory', 'Component', 'Registry', 'Feature', 'FeatureComponents', 'LaunchCondition', 'InstallExecuteSequence'}
    need(required <= tables.keys(), 'fixture tables absent')
    need(all(not rows for name, rows in tables.items() if name not in ALLOWED | SYSTEM), 'unknown nonempty effects table')
    fields = {}
    for line in summary.decode('utf-8').splitlines():
        key, separator, text = line.partition(': ')
        need(separator and key not in fields, 'actual unique summary fields required')
        fields[key] = text
    need(fields.get('Template') == 'Intel;1033' and fields.get('Version') == '500 (1f4)'
         and fields.get('Source') == '10 (a)', 'fixture summary must be x86/per-user/compressed MSI500')
    expected = {
        'Property': [{'Property': k, 'Value': v} for k, v in {
            'ProductCode': value['productCode'], 'UpgradeCode': value['upgradeCode'], 'ProductVersion': '0.0.1',
            'ProductName': 'FutureClassroom registry keypath fixture', 'Manufacturer': 'FutureClassroom test-only',
            'ProductLanguage': '1033', 'MSIRESTARTMANAGERCONTROL': 'DisableShutdown'}.items()],
        'Directory': [{'Directory': 'TARGETDIR', 'Directory_Parent': '', 'DefaultDir': 'SourceDir'},
                      {'Directory': 'LocalAppDataFolder', 'Directory_Parent': 'TARGETDIR', 'DefaultDir': '.'}],
        'Component': [{'Component': 'RegistryProbe', 'ComponentId': value['componentCode'], 'Directory_': 'LocalAppDataFolder',
                       'Attributes': '4', 'Condition': '', 'KeyPath': 'RegistryMarker'}],
        'Registry': [{'Registry': 'RegistryMarker', 'Root': '1', 'Key': value['registryKey'], 'Name': 'OwnedMarker',
                      'Value': '#1', 'Component_': 'RegistryProbe'}],
        'Feature': [{'Feature': 'RegistryFeature', 'Feature_Parent': '', 'Title': 'Registry-only fixture', 'Description': '',
                     'Display': '2', 'Level': '1', 'Directory_': '', 'Attributes': '0'}],
        'FeatureComponents': [{'Feature_': 'RegistryFeature', 'Component_': 'RegistryProbe'}],
        'LaunchCondition': [{'Condition': 'NOT ALLUSERS', 'Description': 'Registry fixture is per-user only.'},
                            {'Condition': 'MSIRESTARTMANAGERCONTROL = "DisableShutdown"',
                             'Description': 'Registry fixture cannot automatically close applications.'}],
    }
    for name, rows in expected.items():
        order = lambda row: json.dumps(row, sort_keys=True)
        need(sorted(tables[name], key=order) == sorted(rows, key=order), 'exact fixture table differs: ' + name)
    sequence = tables['InstallExecuteSequence']
    actions = {row['Action']: row for row in sequence}
    need(len(actions) == len(sequence), 'duplicate execute action')
    required_actions = {'CostFinalize', 'InstallValidate', 'InstallInitialize', 'ProcessComponents',
                        'RemoveRegistryValues', 'WriteRegistryValues', 'RegisterProduct', 'PublishProduct', 'InstallFinalize'}
    need(required_actions <= actions.keys(), 'formal install/removal registration actions required')
    for name, row in actions.items():
        need(set(row) == {'Action', 'Condition', 'Sequence'} and name in STANDARD
             and row['Sequence'] == str(STANDARD[name]), 'unknown or reordered execute action')
        need(row['Condition'] == ("NOT Installed" if name == 'AllocateRegistrySpace' else ''), 'conditional fixture action bypass')
    for name in ('InstallUISequence', 'AdminUISequence', 'AdminExecuteSequence', 'AdvtExecuteSequence'):
        for row in tables.get(name, []):
            allowed = STANDARD | {'ExecuteAction': 1300, 'AdminInitialize': 1500, 'AdminFinalize': 6600}
            need(set(row) == {'Action', 'Condition', 'Sequence'} and row['Action'] in allowed
                 and row['Sequence'] == str(allowed[row['Action']])
                 and row['Condition'] in ('', 'NOT Installed'), 'unknown or changed auxiliary action')
    return {'status': 'PASS_STATIC_REGISTRY_ONLY_FIXTURE_TABLES', 'registryKeypathRuntime': 'NOT_RUN',
            'productionGuardChanged': False, 'spec': value}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--nonce', required=True)
    sub = parser.add_subparsers(dest='command', required=True)
    sub.add_parser('describe')
    author = sub.add_parser('author')
    author.add_argument('--output-dir', type=Path, required=True)
    check = sub.add_parser('check-exports')
    check.add_argument('--exports', type=Path, required=True)
    check.add_argument('--tables-list', type=Path, required=True)
    check.add_argument('--summary', type=Path, required=True)
    args = parser.parse_args()
    value = spec(args.nonce)
    if args.command == 'describe':
        print(json.dumps({'status': 'DESCRIBED_ONLY_MSI_NOT_BUILT', 'spec': value}))
    elif args.command == 'author':
        need(not args.output_dir.exists() and not args.output_dir.is_symlink(), 'fresh authoring directory required')
        args.output_dir.mkdir()
        (args.output_dir / '.owner').write_text('future-classroom-owned-r75-registry-keypath-authoring/v1\n')
        raw = authoring(args.nonce)
        (args.output_dir / 'fixture.wxs').write_bytes(raw)
        value['authoringSha256'] = sha(raw)
        (args.output_dir / 'fixture-spec.json').write_text(json.dumps(value, indent=2) + '\n')
        print(json.dumps({'status': 'AUTHORING_ONLY_MSI_NOT_BUILT', 'spec': value}))
    else:
        listed = read(args.tables_list, 65536).decode('utf-8').splitlines()
        need(len(set(listed)) == len(listed) and all(re.fullmatch('[A-Za-z_][A-Za-z_0-9]*', name) for name in listed), 'complete actual table listing required')
        tables = {path.stem: idt(read(path), path.stem) for path in args.exports.glob('*.idt')}
        need(set(tables) == set(listed) - SYSTEM, 'every non-system actual table must be exported')
        print(json.dumps(verify(tables, args.nonce, read(args.summary, 65536))))


if __name__ == '__main__':
    main()
