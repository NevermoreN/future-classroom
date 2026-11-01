#!/usr/bin/env python3
"""Pure fixture wire/authoring negatives, no actual MSI or Windows calls."""
from pathlib import Path
import copy
import runpy
import unittest
import xml.etree.ElementTree as ET

M = runpy.run_path(str(Path(__file__).with_name('registry-fixture.py')))
NONCE = 'a' * 32  # In-memory model; root has supplied no operational nonce.
SUMMARY = b'Template: Intel;1033\nVersion: 500 (1f4)\nSource: 10 (a)\n'


def model():
    v = M['spec'](NONCE)
    return {
        'Property': [{'Property': k, 'Value': value} for k, value in {
            'ProductCode': v['productCode'], 'UpgradeCode': v['upgradeCode'], 'ProductVersion': '0.0.1',
            'ProductName': 'FutureClassroom registry keypath fixture', 'Manufacturer': 'FutureClassroom test-only',
            'ProductLanguage': '1033', 'MSIRESTARTMANAGERCONTROL': 'DisableShutdown'}.items()],
        'Directory': [{'Directory': 'TARGETDIR', 'Directory_Parent': '', 'DefaultDir': 'SourceDir'},
                      {'Directory': 'LocalAppDataFolder', 'Directory_Parent': 'TARGETDIR', 'DefaultDir': '.'}],
        'Component': [{'Component': 'RegistryProbe', 'ComponentId': v['componentCode'], 'Directory_': 'LocalAppDataFolder',
                       'Attributes': '4', 'Condition': '', 'KeyPath': 'RegistryMarker'}],
        'Registry': [{'Registry': 'RegistryMarker', 'Root': '1', 'Key': v['registryKey'], 'Name': 'OwnedMarker',
                      'Value': '#1', 'Component_': 'RegistryProbe'}],
        'Feature': [{'Feature': 'RegistryFeature', 'Feature_Parent': '', 'Title': 'Registry-only fixture', 'Description': '',
                     'Display': '2', 'Level': '1', 'Directory_': '', 'Attributes': '0'}],
        'FeatureComponents': [{'Feature_': 'RegistryFeature', 'Component_': 'RegistryProbe'}],
        'LaunchCondition': [{'Condition': 'NOT ALLUSERS', 'Description': 'Registry fixture is per-user only.'},
                            {'Condition': 'MSIRESTARTMANAGERCONTROL = "DisableShutdown"',
                             'Description': 'Registry fixture cannot automatically close applications.'}],
        'InstallExecuteSequence': [{'Action': name, 'Condition': '', 'Sequence': str(sequence)} for name, sequence in [
            ('CostFinalize', 1000), ('InstallValidate', 1400), ('InstallInitialize', 1500), ('ProcessComponents', 1600),
            ('RemoveRegistryValues', 2600), ('WriteRegistryValues', 5000), ('RegisterProduct', 6100),
            ('PublishProduct', 6400), ('InstallFinalize', 6600)]]}


class Contracts(unittest.TestCase):
    def test_nonce_separates_every_fixture_identity_and_only_authors_one_value(self):
        one, two = M['spec'](NONCE), M['spec']('b' * 32)
        self.assertEqual(len({one[k] for k in ('productCode', 'upgradeCode', 'componentCode')}), 3)
        self.assertTrue(all(one[k] != two[k] for k in ('productCode', 'upgradeCode', 'componentCode', 'registryKey')))
        tree = ET.fromstring(M['authoring'](NONCE)); ns = {'w': M['NS']}
        tags = {node.tag.split('}')[-1] for node in tree.iter()}
        self.assertFalse(tags & {'File', 'Binary', 'CustomAction', 'MajorUpgrade', 'ServiceInstall', 'Environment', 'CreateFolder'})
        values = tree.findall('.//w:RegistryValue', ns)
        self.assertEqual(len(values), 1)
        self.assertEqual(values[0].get('Key'), one['registryKey'])
        self.assertEqual(values[0].get('KeyPath'), 'yes')
        self.assertNotIn('CurrentVersion', M['authoring'](NONCE).decode())
        self.assertEqual(M['verify'](model(), NONCE, SUMMARY)['registryKeypathRuntime'], 'NOT_RUN')

    def test_exact_tables_reject_side_effects_and_keypath_aliases(self):
        original = model()
        for table, field, value in [('Registry', 'Root', '2'), ('Registry', 'Key', 'Software\\Microsoft\\Windows\\CurrentVersion\\Run'),
                                    ('Registry', 'Name', '*'), ('Registry', 'Value', '#2'), ('Registry', 'Component_', 'Foreign'),
                                    ('Component', 'Attributes', '260'), ('Component', 'KeyPath', 'Foreign'),
                                    ('Component', 'Condition', 'NOT Installed'), ('Feature', 'Level', '0')]:
            changed = copy.deepcopy(original); changed[table][0][field] = value
            with self.subTest(table=table, field=field), self.assertRaises(ValueError): M['verify'](changed, NONCE, SUMMARY)
        for name in ('File', 'Binary', 'CustomAction', 'MsiEmbeddedChainer', 'Upgrade', 'RemoveRegistry', 'RemoveFile', 'ServiceInstall', 'Environment', 'UnknownEffects'):
            changed = copy.deepcopy(original); changed[name] = [{'Unexpected': '1'}]
            with self.subTest(table=name), self.assertRaises(ValueError): M['verify'](changed, NONCE, SUMMARY)

    def test_formal_uninstall_has_no_bypass_or_missing_removal_action(self):
        for change in ('skip-removal', 'missing-removal', 'early-write', 'external-upgrade'):
            tables = model(); rows = tables['InstallExecuteSequence']
            if change == 'skip-removal': rows[4]['Condition'] = 'NOT Installed'
            if change == 'missing-removal': rows.pop(4)
            if change == 'early-write': rows[5]['Sequence'] = '1499'
            if change == 'external-upgrade': rows.append({'Action': 'RemoveExistingProducts', 'Condition': '', 'Sequence': '6501'})
            with self.subTest(change=change), self.assertRaises(ValueError): M['verify'](tables, NONCE, SUMMARY)
        with self.assertRaises(ValueError): M['verify'](model(), NONCE, SUMMARY + b'Template: x64;1033\n')

    def test_idt_parser_rejects_duplicates_and_unknown_nonce(self):
        raw = b'Property\tValue\ns72\tl0\nProperty\tProperty\nX\t1\n'
        self.assertEqual(M['idt'](raw, 'Property'), [{'Property': 'X', 'Value': '1'}])
        for data in (raw + b'X\t2\n', raw + b'Y\t1\textra\n'):
            with self.assertRaises(ValueError): M['idt'](data, 'Property')
        for nonce in ('', 'a' * 31, 'A' * 32, '../' + 'a' * 32):
            with self.assertRaises(ValueError): M['spec'](nonce)


if __name__ == '__main__':
    unittest.main(verbosity=2)
