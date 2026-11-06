"""Delta codec regression with sector relocation, exact hashes and corruption."""
from pathlib import Path
import random
import io
import runpy
import struct
import unittest
import zipfile

V = runpy.run_path(str(Path(__file__).with_name('package-installer-variants.py')))


class VariantTests(unittest.TestCase):
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
