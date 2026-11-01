#!/usr/bin/env python3
"""Check application/test PE role separation with synthetic metadata, not OS execution."""
from pathlib import Path
import runpy
import struct
import unittest

ROOT = Path(__file__).resolve().parent.parent
AUDIT = runpy.run_path(str(ROOT / "scripts/audit-pe.py"))
MSI = runpy.run_path(str(ROOT / "scripts/package-msi-candidate.py"))


def dump(subsystem: str) -> str:
    return ("fixture: file format pei-i386\nMagic 010b (PE32)\n"
            + subsystem + "\nDLL Name: KERNEL32.dll\n 1234 1 GetLastError\n")


def image(subsystem=2, version=(6, 1)) -> bytearray:
    data = bytearray(512)
    data[:2] = b"MZ"
    struct.pack_into("<I", data, 60, 128)
    data[128:132] = b"PE\0\0"
    struct.pack_into("<H", data, 132, 0x14C)
    struct.pack_into("<H", data, 148, 224)
    struct.pack_into("<H", data, 152, 0x10B)
    struct.pack_into("<HH", data, 200, *version)
    struct.pack_into("<H", data, 220, subsystem)
    return data


class Roles(unittest.TestCase):
    def test_application_default_rejects_console_and_other_subsystems(self):
        report = AUDIT["parse_objdump"](dump("Subsystem 00000002 (Windows GUI)"))
        self.assertFalse(report["structural_errors"])
        self.assertEqual(report["subsystem"], "Windows GUI")
        self.assertEqual(report["imports"]["kernel32.dll"], ["GetLastError"])
        for line in ("Subsystem 00000003 (Windows CUI)", "Subsystem 00000001 (NT native)",
                     "Subsystem 00000002 (Windows CUI)", ""):
            with self.subTest(line=line):
                self.assertTrue(AUDIT["parse_objdump"](dump(line))["structural_errors"])

    def test_test_role_requires_explicit_console_selection(self):
        console = dump("Subsystem 00000003 (Windows CUI)")
        self.assertFalse(AUDIT["parse_objdump"](console, "console")["structural_errors"])
        self.assertTrue(AUDIT["parse_objdump"](dump("Subsystem 00000002 (Windows GUI)"), "console")["structural_errors"])
        with self.assertRaises(ValueError):
            AUDIT["parse_objdump"](console, "any")

    def test_wrong_architecture_and_magic_stay_rejected(self):
        valid = dump("Subsystem 00000002 (Windows GUI)")
        for malformed in (valid.replace("pei-i386", "pei-x86-64"), valid.replace("010b", "020b")):
            with self.subTest(malformed=malformed):
                self.assertTrue(AUDIT["parse_objdump"](malformed)["structural_errors"])

    def test_candidate_requires_gui_and_exact_win7_version(self):
        MSI["pe_header"](image())
        for data in (image(3), image(1), image(version=(6, 0)), image(version=(10, 0))):
            with self.subTest(subsystem=struct.unpack_from("<H", data, 220)[0]):
                with self.assertRaises(ValueError):
                    MSI["pe_header"](data)

    def test_candidate_rejects_truncated_declared_optional_header(self):
        for size in (0, 52, 68, 69, 361, 65535):
            with self.subTest(size=size):
                data = image()
                struct.pack_into("<H", data, 148, size)
                with self.assertRaises(ValueError):
                    MSI["pe_header"](data)
        for end in (0, 63, 128, 200, 220, 221, 222):
            with self.subTest(end=end):
                with self.assertRaises(ValueError):
                    MSI["pe_header"](image()[:end])


if __name__ == "__main__":
    unittest.main(verbosity=2)
