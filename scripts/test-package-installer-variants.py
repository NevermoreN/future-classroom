"""Delta codec regression with sector relocation, exact hashes and corruption."""
from pathlib import Path
import random
import io
import json
import runpy
import struct
import unittest
import zipfile

V = runpy.run_path(str(Path(__file__).with_name('package-installer-variants.py')))


class VariantTests(unittest.TestCase):
    def published_sources(self):
        members = {'Setup.exe': b'base-setup' * 200, 'FutureClassroom.msi': b'base-msi' * 300}
        alternate = (members['Setup.exe'] + b'sealed-alternate',
                     members['FutureClassroom.msi'] + b'actual-other-package-code')
        first, second = (V['delta'](members[name], value) for name, value
                         in zip(('Setup.exe', 'FutureClassroom.msi'), alternate))
        catalog = dict(base_package_code='base', base_predecessor_version='1.1.0',
            base_predecessor_product='old', base_predecessor_msi_sha256='1' * 64,
            base_setup=V['fact'](members['Setup.exe']), base_msi=V['fact'](members['FutureClassroom.msi']),
            variants=[dict(package_code='alternate', predecessor_version='1.0.1',
                predecessor_product='older', predecessor_msi_sha256='2' * 64,
                setup=V['fact'](alternate[0]), msi=V['fact'](alternate[1]),
                setup_delta_bytes=len(first), msi_delta_bytes=len(second))])
        header = json.dumps(catalog).encode()
        raw = V['MAGIC'] + struct.pack('<I', len(header)) + header + first + second
        reviewed = [dict(packageCode=code, setup=V['fact'](setup), msi=V['fact'](msi),
                         formalReportSha256='3' * 64)
                    for code, setup, msi in [('base', *members.values()), ('alternate', *alternate)]]
        return raw, members, reviewed, alternate

    def test_published_predecessor_accepts_only_exact_embedded_source_membership(self):
        raw, members, reviewed, alternate = self.published_sources()
        check = V['M']['validate_published_variants']
        check(raw, members, reviewed, 'base', *members.values())
        check(raw, members, reviewed, 'alternate', *alternate)
        for code, setup, msi in [('not-published', *alternate),
                                 ('base', *alternate),
                                 ('alternate', alternate[0], alternate[1] + b'changed')]:
            with self.assertRaises(ValueError):
                check(raw, members, reviewed, code, setup, msi)
        for invalid in (raw[:-1], raw + b'tail', raw[:-1] + bytes([raw[-1] ^ 1])):
            with self.assertRaises(ValueError):
                check(invalid, members, reviewed, 'alternate', *alternate)

    def test_published_predecessor_rejects_unreviewed_or_missing_source_set(self):
        raw, members, reviewed, alternate = self.published_sources()
        for changed in (reviewed[:1], reviewed + [dict(reviewed[0], packageCode='unpublished')],
                        [reviewed[0], dict(reviewed[1], msi=V['fact'](b'other-msi'))]):
            with self.assertRaises(ValueError):
                V['M']['validate_published_variants'](raw, members, changed, 'alternate', *alternate)

    def test_canonical_legacy_wire_is_reproducible_with_fixed_attributes(self):
        members = {n:n.encode() for n in ('Setup.exe','FutureClassroom.msi','LICENSE','THIRD-PARTY-NOTICES.txt')}
        raw = V['canonical_payload'](members)
        self.assertEqual(raw,V['canonical_payload'](dict(reversed(list(members.items())))))
        with zipfile.ZipFile(io.BytesIO(raw)) as archive:
            self.assertEqual(archive.namelist(),list(members))
            for info in archive.infolist():
                self.assertEqual((info.date_time,info.create_system,info.create_version,info.extract_version,
                    info.flag_bits,info.external_attr,info.extra,info.comment,info.compress_type),
                    ((1980,1,1,0,0,0),3,20,20,0,0o100600<<16,b'',b'',zipfile.ZIP_STORED))
    def test_relocated_ole_sectors_do_not_repeat_complete_payload(self):
        rng = random.Random(47289)
        base = rng.randbytes(512 * 512)
        target = base[:512] + b'new-summary'.ljust(512,b'\0') + base[512:300*512] + base[301*512:] + b'tail'
        patch = V['delta'](base,target)
        self.assertLess(len(patch),len(target)//20)
        self.assertEqual(V['apply'](base,patch,V['fact'](target)),target)
        changed = bytearray(patch)
        changed[-1] ^= 1
        for invalid in (bytes(changed),patch[:-1],patch+b'\0',struct.pack('<I',65537)+patch[4:]):
            with self.assertRaises((ValueError,struct.error)):
                V['apply'](base,invalid,V['fact'](target))

    def test_independent_sealed_setup_only_changes_digest_hole(self):
        base = b'normal-helper' * 10000 + b'\0'*32 + b'footer'
        target = base[:-38]+b'1'*32+b'footer'
        patch = V['delta'](base,target)
        self.assertLess(len(patch),2048)
        self.assertEqual(V['apply'](base,patch,V['fact'](target)),target)


if __name__ == '__main__':
    unittest.main()
