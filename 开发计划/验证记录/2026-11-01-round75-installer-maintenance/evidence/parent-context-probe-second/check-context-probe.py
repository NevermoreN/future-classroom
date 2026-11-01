#!/usr/bin/env python3
"""Pure encoder/export negatives only; no EXE, MSI, process or Windows fixture."""
from pathlib import Path
import copy
import runpy
import struct
import unittest
import xml.etree.ElementTree as ET

M = runpy.run_path(str(Path(__file__).with_name('context-probe.py')))
TEST = 'installer_lifecycle::native::broker::tests::future_readonly_parent_observer'
NONCE = 'a' * 32
REPORT = 'C:\\Owned Space\\FutureClassroom-r75-parent-observer-' + NONCE


def model():
    command = M['subprocess'].list2cmdline(M['arguments'](TEST, REPORT, NONCE))
    return command, {
        'Binary': [{'Name': 'FcReadonlyObserver', 'Data': 'Binary.FcReadonlyObserver'}],
        'CustomAction': [
            {'Action': 'FcObserveParent', 'Type': '2', 'Source': 'FcReadonlyObserver', 'Target': command},
            {'Action': 'FcAbortBeforeInitialize', 'Type': '19', 'Source': '', 'Target': M['ABORT']}],
        'InstallExecuteSequence': [{'Action': name, 'Condition': condition, 'Sequence': str(sequence)}
                                  for name, condition, sequence in [('CostFinalize', '', 1000), ('InstallValidate', '', 1400),
                                      ('FcObserveParent', '1', 1499), ('FcAbortBeforeInitialize', '1', 1500), ('InstallInitialize', '', 1501)]]}


class Contracts(unittest.TestCase):
    def test_exact_gui_fixture_ledger_preserves_all_other_bytes(self):
        # In-memory structural vectors only: no executable clone is produced.
        original = bytearray(512)
        original[:2] = b'MZ'
        struct.pack_into('<I', original, 60, 128)
        original[128:132] = b'PE\0\0'
        struct.pack_into('<H', original, 132, 0x14c)
        struct.pack_into('<H', original, 148, 224)
        struct.pack_into('<H', original, 152, 0x10b)
        struct.pack_into('<H', original, 220, 3)
        clone = bytearray(original)
        struct.pack_into('<H', clone, 220, 2)
        ledger = {'status': 'PASS_EXACT_CUI3_TO_GUI2_TEST_FIXTURE_CLONE', 'profileTest': True, 'normalProduct': False,
                  'sourceGitSha': 'a' * 40,
                  'sourceCui': {'path': 'MODEL_ONLY_NOT_CREATED.exe', 'bytes': len(original), 'sha256': M['sha'](original)},
                  'guiFixture': {'path': 'MODEL_ONLY_NOT_CREATED_GUI.exe', 'bytes': len(clone), 'sha256': M['sha'](clone)},
                  'cargoArtifact': {'package': 'fc-windows', 'target': 'fc_windows', 'role': 'tests',
                                    'cargo_profile_test': True, 'sha256': M['sha'](original)},
                  'changedFields': [{'name': 'Subsystem', 'offset': 220, 'beforeHex': '0300', 'afterHex': '0200'}]}
        self.assertFalse(M['validate_fixture'](original, clone, ledger)['normalProduct'])
        for change in ('code', 'entry', 'profile', 'normal', 'offset', 'extra-field'):
            modified, metadata = bytearray(clone), copy.deepcopy(ledger)
            if change == 'code': modified[400] = 1
            if change == 'entry': modified[168] = 1
            if change == 'profile': metadata['cargoArtifact']['cargo_profile_test'] = False
            if change == 'normal': metadata['normalProduct'] = True
            if change == 'offset': metadata['changedFields'][0]['offset'] += 1
            if change == 'extra-field': metadata['changedFields'].append({'name': 'EntryPoint', 'offset': 168, 'beforeHex': '00', 'afterHex': '01'})
            metadata['guiFixture']['sha256'] = M['sha'](modified)
            with self.subTest(change=change), self.assertRaises(ValueError):
                M['validate_fixture'](original, modified, metadata)
        modified = bytearray(clone)
        struct.pack_into('<I', modified, 216, 0x1234)
        metadata = copy.deepcopy(ledger)
        metadata['guiFixture']['sha256'] = M['sha'](modified)
        with self.assertRaises(ValueError):
            M['validate_fixture'](original, modified, metadata)
        metadata['changedFields'].append({'name': 'CheckSum', 'offset': 216, 'beforeHex': '00000000', 'afterHex': '34120000'})
        M['validate_fixture'](original, modified, metadata)

    def test_encoder_has_no_production_gate_or_payload(self):
        root = ET.fromstring(M['authoring'](TEST, REPORT, NONCE))
        ns = {'w': M['NS']}
        nodes = list(root.iter())
        self.assertFalse({'File', 'Component', 'RegistryValue', 'CreateFolder', 'MsiEmbeddedChainer'} & {p.tag.split('}')[-1] for p in nodes})
        actions = root.findall('w:Product/w:CustomAction', ns)
        self.assertEqual([p.get('Id') for p in actions], ['FcObserveParent', 'FcAbortBeforeInitialize'])
        self.assertEqual(actions[1].get('Error'), M['ABORT'])
        self.assertNotIn('Pending', ET.tostring(root).decode())
        command, tables = model()
        self.assertEqual(M['verify'](tables, command)['observerRuntime'], 'NOT_RUN')

    def test_closed_argv_rejects_aliases_and_formatted_paths(self):
        for root in ('\\\\server\\share\\' + REPORT.split('\\')[-1], REPORT + '\\', REPORT.replace('Owned Space', '[TARGETDIR]'),
                     REPORT.replace('Owned Space', '..'), REPORT.replace('Owned Space', 'bad.'), REPORT.replace('C:', 'C:alias')):
            with self.subTest(root=root), self.assertRaises(ValueError):
                M['arguments'](TEST, root, NONCE)
        for test in ('--setup', 'native::tests::anything', TEST + ' --list'):
            with self.assertRaises(ValueError):
                M['arguments'](test, REPORT, NONCE)

    def test_exports_reject_nonconstant_or_late_abort_and_side_effects(self):
        command, tables = model()
        for change in ('conditional', 'late', 'deferred', 'extra-ca', 'file', 'registry', 'chainer', 'unknown-effects'):
            modified = copy.deepcopy(tables)
            if change == 'conditional': modified['InstallExecuteSequence'][3]['Condition'] = 'NOT Installed'
            if change == 'late': modified['InstallExecuteSequence'][3]['Sequence'] = '1502'
            if change == 'deferred': modified['CustomAction'][0]['Type'] = '1026'
            if change == 'extra-ca': modified['CustomAction'].append(dict(modified['CustomAction'][0], Action='FcBeginGate'))
            if change == 'file': modified['File'] = [{'File': 'product.exe'}]
            if change == 'registry': modified['Registry'] = [{'Key': 'Run'}]
            if change == 'chainer': modified['MsiEmbeddedChainer'] = [{'Source': 'FcLifecycleHelper'}]
            if change == 'unknown-effects': modified['MoveFile'] = [{'SourceName': '*'}]
            with self.subTest(change=change), self.assertRaises(ValueError):
                M['verify'](modified, command)


if __name__ == '__main__':
    unittest.main(verbosity=2)
