#!/usr/bin/env python3
"""Focused transport tests; fixtures grant no signed release qualification."""
from __future__ import annotations

import importlib.util
import json
from pathlib import Path
import stat
import struct
import tempfile
import unittest
from unittest.mock import patch
import warnings
import zipfile

SPEC = importlib.util.spec_from_file_location("update_envelope", Path(__file__).with_name("package-update-envelope.py"))
M = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(M)


class EnvelopeTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="fc-envelope-test-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.metadata = self.root / "metadata"
        self.metadata.mkdir()
        for name in M.ROLE_NAMES:
            (self.metadata / name).write_text(json.dumps({"signed": {}, "signatures": []}), encoding="utf-8")
        self.payload = self.root / "payload.zip"
        self.output = self.root / "release.fcupdate"
        self.payload_archive()

    def payload_archive(self, extra=None, compression=zipfile.ZIP_STORED):
        with zipfile.ZipFile(self.payload, "w", compression=compression) as archive:
            archive.writestr("Setup.exe", b"not a qualified executable")
            archive.writestr("FutureClassroom.msi", b"not a qualified MSI")
            archive.writestr("LICENSE", b"license fixture")
            for name, data in extra or []:
                archive.writestr(name, data)

    def envelope_archive(self, extra=None, compression=zipfile.ZIP_STORED):
        with warnings.catch_warnings(), zipfile.ZipFile(self.output, "w", compression=compression) as archive:
            warnings.simplefilter("ignore", UserWarning)
            for entry in sorted(self.metadata.iterdir()):
                archive.writestr("metadata/" + entry.name, entry.read_bytes())
            archive.writestr("payload.zip", self.payload.read_bytes())
            for name, data in extra or []:
                archive.writestr(name, data)

    def test_complete_original_bytes_and_explicit_transport_only(self):
        for version in (7, 8):
            (self.metadata / f"{version}.root.json").write_text('{"signed":{},"signatures":[]}', encoding="utf-8")
        original = self.payload.read_bytes()
        result = M.create(self.payload, self.metadata, self.output)
        self.assertEqual(result["status"], "TRANSPORT_ONLY")
        self.assertEqual(result["tufSignatures"], "NOT_VERIFIED")
        self.assertEqual(result["upgradeQualification"], "NOT_GRANTED")
        self.assertEqual(result["payloadSha256"], M.hashlib.sha256(original).hexdigest())
        with zipfile.ZipFile(self.output) as archive:
            self.assertEqual(archive.read("payload.zip"), original)
            for entry in self.metadata.iterdir():
                self.assertEqual(archive.read("metadata/" + entry.name), entry.read_bytes())
        self.assertEqual(M.inspect(self.output), result)
        self.assertEqual(sorted(p.name for p in self.root.iterdir()), ["metadata", "payload.zip", "release.fcupdate"])

    def test_no_clobber_and_failure_removes_only_new_output(self):
        self.output.write_bytes(b"operator delivery")
        with self.assertRaises(FileExistsError):
            M.create(self.payload, self.metadata, self.output)
        self.assertEqual(self.output.read_bytes(), b"operator delivery")
        self.output.unlink()
        (self.metadata / "targets.json").write_bytes(b'{"duplicate":1,"duplicate":2}')
        with self.assertRaises(ValueError):
            M.create(self.payload, self.metadata, self.output)
        self.assertFalse(self.output.exists())
        self.assertTrue(self.payload.exists())

    def test_current_third_party_notices_exceed_metadata_budget_and_roundtrip(self):
        notices = Path(__file__).resolve().parent.parent / "packaging" / "THIRD-PARTY-NOTICES.txt"
        self.assertGreater(notices.stat().st_size, M.MAX_METADATA)
        with zipfile.ZipFile(self.payload, "a") as archive:
            archive.write(notices, "THIRD-PARTY-NOTICES.txt")
        result = M.create(self.payload, self.metadata, self.output)
        self.assertIn("THIRD-PARTY-NOTICES.txt", result["payloadFiles"])
        self.assertEqual(M.inspect(self.output), result)
        self.assertEqual(M.MAX_METADATA, 256 * 1024)

    def test_duplicate_paths_extra_entries_and_inner_scripts_are_refused(self):
        for name in ("metadata/targets.json", "../escape", "/absolute", "metadata/../targets.json", "extra.txt"):
            with self.subTest(name=name):
                self.envelope_archive([(name, b"{}")])
                with self.assertRaises(ValueError):
                    M.inspect(self.output)
        self.payload_archive([("extra.ps1", b"run")])
        with self.assertRaises(ValueError):
            M.create(self.payload, self.metadata, self.root / "other.fcupdate")
        self.payload_archive([("Setup.exe", b"duplicate")])
        with self.assertRaises(ValueError):
            M.create(self.payload, self.metadata, self.root / "other.fcupdate")

    def test_symlink_member_and_source_are_refused(self):
        self.envelope_archive()
        with zipfile.ZipFile(self.output, "a") as archive:
            info = zipfile.ZipInfo("metadata/1.root.json")
            info.create_system = 3
            info.external_attr = (stat.S_IFLNK | 0o777) << 16
            archive.writestr(info, b"elsewhere")
        with self.assertRaises(ValueError):
            M.inspect(self.output)
        link = self.root / "linked.zip"
        try:
            link.symlink_to(self.payload)
        except OSError as error:
            self.skipTest(f"symlink unavailable: {error}")
        with self.assertRaises(ValueError):
            M.create(link, self.metadata, self.root / "other.fcupdate")

    def test_crc_tamper_and_outer_compression_are_refused(self):
        M.create(self.payload, self.metadata, self.output)
        with zipfile.ZipFile(self.output) as archive:
            info = archive.getinfo("payload.zip")
        raw = bytearray(self.output.read_bytes())
        offset = info.header_offset + 30 + len(info.filename.encode())
        raw[offset + 10] ^= 1
        self.output.write_bytes(raw)
        with self.assertRaises(zipfile.BadZipFile):
            M.inspect(self.output)
        self.envelope_archive(compression=zipfile.ZIP_DEFLATED)
        with self.assertRaises(ValueError):
            M.inspect(self.output)

    def test_metadata_size_total_root_chain_and_expanded_budget(self):
        with self.assertRaises(ValueError):
            M.metadata_names(M.ROLE_NAMES | {"1.root.json", "3.root.json"})
        with self.assertRaises(ValueError):
            M.metadata_names(M.ROLE_NAMES | {f"{v}.root.json" for v in range(1, 34)})
        with patch.object(M, "MAX_METADATA", 8):
            with self.assertRaises(ValueError):
                M.create(self.payload, self.metadata, self.output)
            self.assertFalse(self.output.exists())
        with patch.object(M, "MAX_METADATA_TOTAL", 10):
            with self.assertRaises(ValueError):
                M.create(self.payload, self.metadata, self.output)
            self.assertFalse(self.output.exists())
        self.payload_archive(compression=zipfile.ZIP_DEFLATED)
        with patch.dict(M.INNER_LIMITS, {"Setup.exe": 8}):
            with self.assertRaises(ValueError):
                M.create(self.payload, self.metadata, self.output)
            self.assertFalse(self.output.exists())
        with patch.object(M, "MAX_ENVELOPE", self.payload.stat().st_size):
            with self.assertRaises(ValueError):
                M.create(self.payload, self.metadata, self.output)
            self.assertFalse(self.output.exists())

    def test_bounded_central_directory_and_trailing_data(self):
        self.envelope_archive()
        raw = self.output.read_bytes()
        self.output.write_bytes(raw + b"trailing")
        with self.assertRaises(ValueError):
            M.inspect(self.output)
        forged = bytearray(raw)
        # Declared central directory size is checked before ZipFile allocates it.
        struct.pack_into("<L", forged, len(forged) - 10, M.MAX_CENTRAL + 1)
        self.output.write_bytes(forged)
        with self.assertRaises(ValueError):
            M.inspect(self.output)


if __name__ == "__main__":
    unittest.main()
