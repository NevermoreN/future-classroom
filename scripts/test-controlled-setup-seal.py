#!/usr/bin/env python3
"""Pure synthetic byte-format tests; no installer or runnable Setup is produced."""
import ast
import hashlib
from pathlib import Path
import runpy
import struct
import sys
import unittest

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
SEAL = runpy.run_path(str(HERE / "controlled-setup-seal.py"))
BUILDER = runpy.run_path(str(HERE / "package-msi-candidate.py"))


def structural_pe():
    # Only a byte-format fixture accepted by the existing pe_header contract.
    image = bytearray(512)
    image[:2] = b"MZ"
    struct.pack_into("<I", image, 60, 128)
    image[128:132] = b"PE\0\0"
    struct.pack_into("<H", image, 132, 0x14C)
    struct.pack_into("<H", image, 148, 224)
    struct.pack_into("<H", image, 152, 0x10B)
    struct.pack_into("<HH", image, 200, 6, 1)
    struct.pack_into("<H", image, 220, 2)
    return bytes(image)


class SetupSealTests(unittest.TestCase):
    def setUp(self):
        self.helper = structural_pe()
        payload = {name: name.encode("ascii") for name in BUILDER["LIFECYCLE_REQUIRED"]}
        self.legacy = BUILDER["lifecycle_image"](self.helper, "0.1.0", payload)
        self.msi = b"synthetic final inner MSI bytes\0with distinct tail"
        self.sealed = SEAL["seal_setup_image"](self.legacy, self.msi)

    def rejected(self, callback, *args):
        with self.assertRaises(ValueError):
            callback(*args)

    def test_exact_layout_keeps_manifest_length_footer_and_inner_bytes(self):
        # Independent byte comparison, not merely encoder/decoder agreement.
        split = len(self.helper)
        self.assertEqual(SEAL["SEAL_MAGIC"], b"FC-SETUP-MSI\0v1\0")
        self.assertEqual(len(SEAL["SEAL_MAGIC"]), 16)
        self.assertEqual(self.sealed[:split], self.helper)
        self.assertEqual(self.sealed[split:split + 16], b"FC-SETUP-MSI\0v1\0")
        self.assertEqual(self.sealed[split + 16:split + 48], hashlib.sha256(self.msi).digest())
        self.assertEqual(self.sealed[split + 48:], self.legacy[split:])
        self.assertEqual(len(self.sealed), len(self.legacy) + 48)
        self.assertEqual(SEAL["parse_sealed_setup_image"](self.sealed, self.msi),
                         BUILDER["parse_lifecycle_image"](self.legacy))

    def test_hash_compares_entire_msi_bytes_separately_from_ledger(self):
        read = SEAL["parse_sealed_setup_image"]
        for other in (self.msi[:-1], self.msi + b"extra", b"X" + self.msi[1:]):
            self.rejected(read, self.sealed, other)
        # A caller cannot substitute a supplied hex digest for the full MSI.
        self.rejected(read, self.sealed, hashlib.sha256(self.msi).hexdigest().encode())

    def test_each_seal_byte_tamper_and_zero_hash_rejected(self):
        start = len(self.helper)
        for offset in range(48):
            image = bytearray(self.sealed)
            image[start + offset] ^= 1
            self.rejected(SEAL["parse_sealed_setup_image"], bytes(image), self.msi)
        image = self.sealed[:start + 16] + bytes(32) + self.sealed[start + 48:]
        self.rejected(SEAL["parse_sealed_setup_image"], image, self.msi)

    def test_unsealed_and_duplicate_seals_rejected(self):
        self.rejected(SEAL["parse_sealed_setup_image"], self.legacy, self.msi)
        self.rejected(SEAL["seal_setup_image"], self.sealed, self.msi)
        start = len(self.helper)
        double = self.sealed[:start] + self.sealed[start:start + 48] + self.sealed[start:]
        self.rejected(SEAL["parse_sealed_setup_image"], double, self.msi)
        self.rejected(SEAL["seal_setup_image"], double, self.msi)

    def test_truncation_footer_and_manifest_bound_fail_closed(self):
        read = SEAL["parse_sealed_setup_image"]
        for size in (0, 1, 19, 20, 47, len(self.sealed) - 1):
            self.rejected(read, self.sealed[:size], self.msi)
        # Delete inside the seal while retaining the original footer/manifest.
        for offset in range(48):
            at = len(self.helper) + offset
            self.rejected(read, self.sealed[:at] + self.sealed[at + 1:], self.msi)
        for length in (0, 32769, len(self.sealed), 0xFFFFFFFF):
            image = self.sealed[:-20] + struct.pack("<I", length) + self.sealed[-16:]
            self.rejected(read, image, self.msi)
        self.rejected(read, self.sealed[:-1] + b"!", self.msi)

    def test_existing_pe_and_closed_manifest_constraints_still_apply(self):
        for offset, value in ((0, 0), (132, 0), (152, 0), (200, 5), (220, 3)):
            image = bytearray(self.legacy)
            image[offset] = value
            self.rejected(SEAL["seal_setup_image"], bytes(image), self.msi)
        # Alter product identity inside the otherwise complete manifest.
        image = bytearray(self.legacy)
        image[len(self.helper) + 12] ^= 1
        self.rejected(SEAL["seal_setup_image"], bytes(image), self.msi)
        nested = self.legacy + self.legacy[len(self.helper):]
        self.rejected(SEAL["seal_setup_image"], nested, self.msi)

    def test_real_native_msi_bound_and_immutable_input_types(self):
        seal = SEAL["seal_setup_image"]
        read = SEAL["parse_sealed_setup_image"]
        for bad in (b"", bytearray(self.msi), memoryview(self.msi), "path/to/file.msi", None):
            self.rejected(seal, self.legacy, bad)
            self.rejected(read, self.sealed, bad)
        limit = SEAL["MAX_INNER_MSI_BYTES"]
        at_limit = bytes(limit)
        encoded = seal(self.legacy, at_limit)
        self.assertEqual(read(encoded, at_limit)[0], self.helper)
        del at_limit
        self.rejected(seal, self.legacy, bytes(limit + 1))
        self.rejected(read, self.sealed, bytes(limit + 1))
        self.rejected(seal, bytearray(self.legacy), self.msi)
        self.rejected(read, bytearray(self.sealed), self.msi)

    def test_setup_image_bound_accounts_for_inserted_seal(self):
        # Keep a valid manifest/footer and extend only the synthetic PE prefix.
        limit = SEAL["MAX_SETUP_IMAGE_BYTES"]
        body = self.legacy[len(self.helper):]
        prefix_size = limit - 48 - len(body)
        legacy = self.helper + bytes(prefix_size - len(self.helper)) + body
        encoded = SEAL["seal_setup_image"](legacy, self.msi)
        self.assertEqual(len(encoded), limit)
        self.assertEqual(len(SEAL["parse_sealed_setup_image"](encoded, self.msi)[0]), prefix_size)
        del encoded
        self.rejected(SEAL["seal_setup_image"], legacy[:-len(body)] + b"x" + body, self.msi)
        self.rejected(SEAL["parse_sealed_setup_image"], legacy + bytes(49), self.msi)

    def test_sources_parse_without_bytecode_or_product_cli(self):
        for name in ("controlled-setup-seal.py", "test-controlled-setup-seal.py"):
            ast.parse((HERE / name).read_text(), filename=name)
        self.assertNotIn("main", SEAL)


if __name__ == "__main__":
    unittest.main()
