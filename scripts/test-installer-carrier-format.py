"""Synthetic ABI2 format tests; no MSI, publication or runnable Setup."""
import hashlib
from pathlib import Path
import runpy
import struct
import sys
import unittest

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
C = runpy.run_path(str(HERE / "installer-carrier-format.py"))
B = runpy.run_path(str(HERE / "package-msi-candidate.py"))
PACKAGE = "12345678-1234-1234-1234-123456789ABC"


def structural_pe():
    image = bytearray(1024)
    image[:2] = b"MZ"
    struct.pack_into("<I", image, 60, 128)
    image[128:132] = b"PE\0\0"
    struct.pack_into("<HH", image, 132, 0x14C, 1)
    struct.pack_into("<H", image, 148, 224)
    struct.pack_into("<H", image, 152, 0x10B)
    struct.pack_into("<HH", image, 200, 6, 1)
    struct.pack_into("<I", image, 212, 512)
    struct.pack_into("<H", image, 220, 2)
    struct.pack_into("<I", image, 244, 16)
    image[376:381] = b".text"
    struct.pack_into("<II", image, 392, 512, 512)
    return bytes(image)


class CarrierFormatTests(unittest.TestCase):
    def setUp(self):
        self.pe = structural_pe()
        self.template = C["make_carrier_template"](self.pe)
        self.msi = b"synthetic complete MSI bytes: no installer authority"
        self.image = C["bind_carrier_template"](self.template, self.msi)
        self.payloads = {name: name.encode() for name in B["LIFECYCLE_REQUIRED"]}
        self.body = B["lifecycle_manifest"]("0.2.0", self.payloads)
        self.envelope = C["make_payload_manifest"](PACKAGE, self.body)

    def reject(self, function, *args):
        with self.assertRaises(ValueError):
            C[function](*args)

    def test_template_exact_descriptor_fields_and_only_one_digest_hole(self):
        self.assertEqual(len(self.template), len(self.pe) + 132)
        self.assertEqual(self.template[:len(self.pe)], self.pe)
        descriptor = self.template[-84:-20]
        self.assertEqual(len(descriptor), 64)
        self.assertEqual(descriptor[:12], b"FCCAR2\0\0\x02\0\x40\0")
        self.assertEqual(descriptor[12:48], b"99378807-098D-5F04-8D4D-F181DFA218D5")
        self.assertEqual(struct.unpack("<HHHHI", descriptor[48:60]), (120, 64, 8, 72, 31457280))
        self.assertEqual(descriptor[60:], bytes(4))
        offset = len(self.pe) + 16
        self.assertEqual(self.image[:offset], self.template[:offset])
        self.assertEqual(self.image[offset:offset + 32], hashlib.sha256(self.msi).digest())
        self.assertEqual(self.image[offset + 32:], self.template[offset + 32:])
        self.assertEqual(C["verify_carrier_binding"](self.template, self.image, self.msi), self.pe)

    def test_whole_msi_and_full_image_are_compared(self):
        for other in (self.msi[:-1], self.msi + b"tail", b"x" + self.msi[1:]):
            self.reject("verify_carrier_binding", self.template, self.image, other)
        for position in (len(self.pe) - 1, len(self.pe) + 16, len(self.image) - 1):
            altered = bytearray(self.image); altered[position] ^= 1
            self.reject("verify_carrier_binding", self.template, bytes(altered), self.msi)
        self.reject("parse_carrier_template", self.image)
        self.reject("verify_carrier_binding", self.template, self.template, self.msi)

    def test_all_descriptor_seal_and_footer_mutations_reject(self):
        for position in range(len(self.pe), len(self.template)):
            changed = bytearray(self.template); changed[position] ^= 1
            self.reject("parse_carrier_template", bytes(changed))

    def test_nested_legacy_or_carrier_or_adjacent_seal_is_rejected(self):
        legacy = B["lifecycle_image"](self.pe, "0.2.0", self.payloads)
        for prefix in (legacy, self.template, self.image, self.pe + C["SEAL_MAGIC"] + bytes(32)):
            self.reject("make_carrier_template", prefix)

    def test_truncation_tail_and_mutable_inputs_reject(self):
        for image in (b"", self.template[:131], self.template[:-1], self.template + b"x", bytearray(self.template)):
            self.reject("parse_carrier_template", image)
        self.reject("bind_carrier_template", self.template, b"")
        self.reject("make_payload_manifest", PACKAGE, bytearray(self.body))

    def test_pe_extent_sections_and_optional_certificate_are_closed(self):
        self.reject("make_carrier_template", self.pe + b"unparsed overlay")
        for position, value in ((134, 0), (392, 513), (396, 511), (244, 17)):
            changed = bytearray(self.pe)
            struct.pack_into("<I" if position != 134 else "<H", changed, position, value)
            self.reject("make_carrier_template", bytes(changed))
        signed_shape = bytearray(self.pe + bytes(8))
        struct.pack_into("<II", signed_shape, 280, 1024, 8)
        template = C["make_carrier_template"](bytes(signed_shape))
        self.assertEqual(C["parse_carrier_template"](template), bytes(signed_shape))
        struct.pack_into("<I", signed_shape, 280, 1023)
        self.reject("make_carrier_template", bytes(signed_shape))

    def test_payload_package_and_product_are_independent_and_versioned(self):
        parsed = C["parse_payload_manifest"](self.envelope)
        self.assertEqual(self.envelope[:12], b"FCPM2\0\0\0\x02\0\x02\0")
        self.assertEqual(self.envelope[48:84], PACKAGE.encode())
        self.assertEqual(self.envelope[88:], self.body)
        self.assertEqual(parsed["package"], PACKAGE)
        self.assertEqual(parsed["product"], B["product_code"]("0.2.0"))
        self.assertEqual(parsed["version"], "0.2.0")
        self.assertEqual(set(parsed["entries"]), set(self.payloads))

    def test_family_abi_length_package_and_legacy_envelope_reject(self):
        for position in (0, 8, 10, 12, 48, 84):
            changed = bytearray(self.envelope); changed[position] = 0xFF
            self.reject("parse_payload_manifest", bytes(changed))
        for data in (self.body, self.envelope[:-1], self.envelope + b"tail"):
            self.reject("parse_payload_manifest", data)
        for package in ("00000000-0000-0000-0000-000000000000", PACKAGE.lower(), "{" + PACKAGE + "}", "x"):
            self.reject("make_payload_manifest", package, self.body)

    def test_zero_identity_digest_and_noncanonical_version_reject(self):
        changed = self.body[:12] + b"00000000-0000-0000-0000-000000000000" + self.body[48:]
        self.reject("make_payload_manifest", PACKAGE, changed)
        # First row starts after header + version. Corrupt only its digest.
        row = 49 + self.body[48]
        path_bytes = struct.unpack_from("<H", self.body, row)[0]
        digest = row + 2 + path_bytes + 4
        changed = self.body[:digest] + bytes(32) + self.body[digest + 32:]
        self.reject("make_payload_manifest", PACKAGE, changed)
        changed = self.body[:48] + b"\x06" + b"00.2.0" + self.body[54:]
        self.reject("make_payload_manifest", PACKAGE, changed)

    def test_no_cli_or_publication_admission_is_created(self):
        self.assertNotIn("main", C)
        self.assertNotIn("PublishedInstallerHost", C)
        self.assertNotIn("subprocess", C)


if __name__ == "__main__":
    unittest.main()
