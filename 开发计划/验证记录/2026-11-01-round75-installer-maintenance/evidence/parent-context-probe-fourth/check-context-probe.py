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
                  'sourceGitBase': 'a' * 40, 'candidateSourceProducerSha256': 'b' * 64,
                  'candidateSourceFreezeSha256': 'c' * 64,
                  'sourceCui': {'path': 'MODEL_ONLY_NOT_CREATED.exe', 'bytes': len(original), 'sha256': M['sha'](original)},
                  'guiFixture': {'path': 'MODEL_ONLY_NOT_CREATED_GUI.exe', 'bytes': len(clone), 'sha256': M['sha'](clone)},
                  'cargoArtifact': {'package': 'fc-windows', 'target': 'fc_windows', 'role': 'tests',
                                    'cargo_profile_test': True, 'sha256': M['sha'](original)},
                  'changedFields': [{'name': 'Subsystem', 'offset': 220, 'beforeHex': '0300', 'afterHex': '0200'}]}
        self.assertFalse(M['validate_fixture'](original, clone, ledger)['normalProduct'])
        for change in ('code', 'entry', 'profile', 'normal', 'offset', 'extra-field', 'legacy-source-label', 'short-producer', 'short-freeze'):
            modified, metadata = bytearray(clone), copy.deepcopy(ledger)
            if change == 'code': modified[400] = 1
            if change == 'entry': modified[168] = 1
            if change == 'profile': metadata['cargoArtifact']['cargo_profile_test'] = False
            if change == 'normal': metadata['normalProduct'] = True
            if change == 'offset': metadata['changedFields'][0]['offset'] += 1
            if change == 'extra-field': metadata['changedFields'].append({'name': 'EntryPoint', 'offset': 168, 'beforeHex': '00', 'afterHex': '01'})
            if change == 'legacy-source-label': metadata['sourceGitSha'] = metadata.pop('sourceGitBase')
            if change == 'short-producer': metadata['candidateSourceProducerSha256'] = 'b' * 40
            if change == 'short-freeze': metadata['candidateSourceFreezeSha256'] = 'c' * 40
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

    def test_exact_topology_model_has_no_production_gate_or_payload(self):
        # Only in-memory IDT models here; no authoring function is invoked.
        command, tables = model()
        self.assertEqual(M['verify'](tables, command)['observerRuntime'], 'NOT_RUN')
        self.assertFalse(M['FORBIDDEN'] & tables.keys())

    def test_virtual_codepage_is_not_an_ordinary_or_effects_table(self):
        raw = b'\r\n\r\n0\t_ForceCodepage\r\n\0'
        self.assertEqual(M['force_codepage'](raw)['stringPoolCodepage'], 0)
        for altered in (raw[:-1], raw + b'payload', raw.replace(b'0\t', b'1252\t'),
                        raw.replace(b'\r\n', b'\n'), b'\r\n\r\n0\t_ForceCodepage\r\n\0\0',
                        b'Codepage\nI2\n_ForceCodepage\tCodepage\n0\n'):
            with self.subTest(raw=altered), self.assertRaises(ValueError):
                M['force_codepage'](altered)
        with self.assertRaises(ValueError):
            M['idt'](raw, '_ForceCodepage')
        command, tables = model()
        tables['_ForceCodepage'] = [{'Action': 'WriteRegistryValues'}]
        with self.assertRaises(ValueError):
            M['verify'](tables, command)

    def test_ascii_idt_keys_and_actual_export_membership_are_closed(self):
        raw = b'Property\tValue\ns72\tl0\nProperty\tProperty\nX\t1\n'
        codepage = b'\r\n\r\n0\t_ForceCodepage\r\n\0'
        originals = {'Property': raw, '_ForceCodepage': codepage}
        tables, metadata = M['exported_tables'](originals, ['Property', '_ForceCodepage'])
        self.assertEqual(tables, {'Property': [{'Property': 'X', 'Value': '1'}]})
        self.assertEqual(metadata['stringPoolCodepage'], 0)
        for bad in (raw + b'X\t2\n', raw + b'Y\t1\textra\n', raw.replace(b'X\t1', b'X\t\0'),
                    raw.replace(b'X\t1', 'X\t\u4e2d'.encode('utf-8'))):
            with self.subTest(raw=bad), self.assertRaises(ValueError):
                M['exported_tables']({'Property': bad, '_ForceCodepage': codepage}, ['Property', '_ForceCodepage'])
        for changed, listed in [
            ({'Property': raw}, ['Property', '_ForceCodepage']),
            (originals, ['Property']),
            (originals, ['Property', '_ForceCodepage', 'Property']),
            (originals, ['Property', '_ForceCodepage', 'File']),
            (dict(originals, Extra=raw.replace(b'Property', b'Extra')), ['Property', '_ForceCodepage']),
        ]:
            with self.subTest(listed=listed), self.assertRaises(ValueError):
                M['exported_tables'](changed, listed)
        with self.assertRaises(ValueError):
            M['arguments'](TEST, REPORT.replace('Owned Space', '\u4e2d'), NONCE)

    def test_auxiliary_admin_exception_is_exact_and_never_custom_execution(self):
        command, tables = model()
        admin = {'Action': 'InstallAdminPackage', 'Condition': '', 'Sequence': '3900'}
        tables['AdminExecuteSequence'] = [admin]
        M['verify'](tables, command)
        for change in ('condition', 'sequence', 'main', 'ui', 'admin-ui', 'advt',
                       'unknown-admin', 'duplicate', 'observer-admin', 'abort-admin', 'extra-column'):
            changed = copy.deepcopy(tables)
            if change == 'condition': changed['AdminExecuteSequence'][0]['Condition'] = 'NOT Installed'
            if change == 'sequence': changed['AdminExecuteSequence'][0]['Sequence'] = '3901'
            if change == 'main': changed['InstallExecuteSequence'].extend(changed.pop('AdminExecuteSequence'))
            if change == 'ui': changed['InstallUISequence'] = changed.pop('AdminExecuteSequence')
            if change == 'admin-ui': changed['AdminUISequence'] = changed.pop('AdminExecuteSequence')
            if change == 'advt': changed['AdvtExecuteSequence'] = changed.pop('AdminExecuteSequence')
            if change == 'unknown-admin': changed['AdminExecuteSequence'][0]['Action'] = 'ArbitraryAdminAction'
            if change == 'duplicate': changed['AdminExecuteSequence'].append(dict(admin))
            if change == 'observer-admin': changed['AdminExecuteSequence'][0]['Action'] = 'FcObserveParent'
            if change == 'abort-admin': changed['AdminExecuteSequence'][0]['Action'] = 'FcAbortBeforeInitialize'
            if change == 'extra-column': changed['AdminExecuteSequence'][0]['Extra'] = 'ignored'
            with self.subTest(change=change), self.assertRaises(ValueError):
                M['verify'](changed, command)
        for table in M['AUXILIARY_STANDARD']:
            for action, sequence in M['AUXILIARY_STANDARD'][table].items():
                accepted = copy.deepcopy(tables)
                accepted[table] = [{'Action': action, 'Condition': '', 'Sequence': str(sequence)}]
                M['verify'](accepted, command)
                for field, value in [('Condition', 'NOT Installed'), ('Sequence', str(sequence + 1))]:
                    rejected = copy.deepcopy(accepted)
                    rejected[table][0][field] = value
                    with self.subTest(table=table, action=action, field=field), self.assertRaises(ValueError):
                        M['verify'](rejected, command)
        for action, condition, sequence in [('CostFinalize', '', '999'), ('InstallAdminPackage', '', '3900'),
                                             ('ArbitraryAction', '', '1498')]:
            rejected = copy.deepcopy(tables)
            rejected['InstallExecuteSequence'] = [r for r in rejected['InstallExecuteSequence'] if r['Action'] != action]
            rejected['InstallExecuteSequence'].append({'Action': action, 'Condition': condition, 'Sequence': sequence})
            with self.subTest(action=action), self.assertRaises(ValueError):
                M['verify'](rejected, command)

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
