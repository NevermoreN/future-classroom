"""Bounded synthetic SFX tests, no executable or installation is run."""
from pathlib import Path
import hashlib
import runpy
import struct
import unittest
import io
import zipfile

HERE = Path(__file__).resolve().parent
S = runpy.run_path(str(HERE / 'package-standalone-exe.py'))
F = runpy.run_path(str(HERE / 'test-installer-carrier-format.py'))

class StandaloneTests(unittest.TestCase):
    def setUp(self):
        self.helper = F['structural_pe']()
        c = S['C']
        self.members = dict(zip(S['NAMES'], [c['bind_carrier_template'](c['make_carrier_template'](self.helper), b'msi'), b'msi', b'license', b'notices']))
        self.image = S['package'](self.helper, self.members)

    def test_original_pair_preserved_and_fixed_native_footer(self):
        self.assertEqual(self.image[:len(self.helper)], self.helper)
        footer = self.image[-88:]
        self.assertEqual(footer[:16], b'FC-SFX-PACK\0v1\0\0')
        self.assertEqual(struct.unpack('<5Q',footer[16:56]), (len(self.helper), *(len(self.members[n]) for n in S['NAMES'])))
        self.assertEqual(footer[56:],hashlib.sha256(footer[:56]+self.image[len(self.helper):-88]).digest())
        self.assertEqual(S['inspect'](self.image),(self.helper,self.members))

    def test_payload_table_truncation_and_wrong_inner_msi_are_rejected(self):
        for offset in (len(self.helper), -80, -40, -1):
            changed=bytearray(self.image);changed[offset]^=1
            with self.assertRaises(ValueError):S['inspect'](bytes(changed))
        with self.assertRaises(ValueError):S['inspect'](self.image[:-1])
        with self.assertRaises(ValueError):S['package'](self.helper,dict(self.members,**{'FutureClassroom.msi':b'other'}))
        with self.assertRaises(ValueError):S['package'](self.helper,dict(self.members,**{'../other':b'bad'}))

    def test_previous_maintenance_pair_is_bounded_and_covered_by_v2_digest(self):
        members = dict(self.members, **{'previous/Setup.exe': self.members['Setup.exe'],
            'previous/FutureClassroom.msi': self.members['FutureClassroom.msi']})
        image = S['package'](self.helper, members)
        self.assertEqual(image[-104:-88], b'FC-SFX-PACK\0v2\0\0')
        self.assertEqual(S['inspect'](image), (self.helper, members))
        for offset in (-105, -70, -1):
            changed=bytearray(image);changed[offset]^=1
            with self.assertRaises(ValueError):S['inspect'](bytes(changed))
        with self.assertRaises(ValueError):S['package'](self.helper,dict(self.members, **{'previous/Setup.exe':b'partial'}))

    def test_legacy_bridge_pair_requires_complete_v3_envelope(self):
        members = dict(self.members)
        for names in (S['PREVIOUS_NAMES'], S['LEGACY_NAMES']):
            members.update(dict(zip(names, [self.members['Setup.exe'], self.members['FutureClassroom.msi']])))
        image = S['package'](self.helper, members)
        self.assertEqual(image[-120:-104], S['MAGIC_V3'])
        self.assertEqual(S['inspect'](image), (self.helper, members))
        for offset in (-121, -110, -1):
            changed = bytearray(image); changed[offset] ^= 1
            with self.assertRaises(ValueError): S['inspect'](bytes(changed))
        del members['legacy/FutureClassroom.msi']
        with self.assertRaises(ValueError): S['package'](self.helper, members)

    def test_unified_distribution_preserves_signed_archive_and_binds_helper(self):
        payload = io.BytesIO()
        with zipfile.ZipFile(payload, 'w', compression=zipfile.ZIP_DEFLATED) as archive:
            for name, content in self.members.items(): archive.writestr(name, content)
            archive.writestr('installer-variants.bin', b'current-release-only')
        envelope = io.BytesIO()
        with zipfile.ZipFile(envelope, 'w', compression=zipfile.ZIP_STORED) as archive:
            for name in ('1.root.json', 'timestamp.json', 'snapshot.json', 'targets.json'):
                archive.writestr('metadata/' + name, b'{}')
            archive.writestr('payload.zip', payload.getvalue())
        original = envelope.getvalue()
        image = S['package_distribution'](self.helper, original)
        self.assertEqual(S['distribution_archive'](image), (self.helper, original))
        self.assertEqual(S['inspect'](image), (self.helper, self.members))
        self.assertEqual(len(image), len(self.helper) + len(original) + 64)
        for offset in (len(self.helper), -40, -1):
            changed = bytearray(image); changed[offset] ^= 1
            with self.assertRaises(ValueError): S['inspect'](bytes(changed))
        changed = bytearray(image); changed[512] ^= 1
        with self.assertRaises(ValueError): S['inspect'](bytes(changed))
        with self.assertRaises(ValueError): S['inspect'](image[:-1])

if __name__ == '__main__':unittest.main()
