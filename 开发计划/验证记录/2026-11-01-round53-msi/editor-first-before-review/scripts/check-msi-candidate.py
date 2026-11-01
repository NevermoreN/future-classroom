#!/usr/bin/env python3
"""Pure hostile-input/authoring checks; does not build or install an MSI."""
from __future__ import annotations

import io
import json
from pathlib import Path
import runpy
import stat
import tempfile
import unittest
import warnings
import xml.etree.ElementTree as ET
import zipfile

M = runpy.run_path(str(Path(__file__).absolute().with_name("package-msi-candidate.py")))
ROOT, SHA = M["ROOT"], M["digest"]


def fixture(web=True):
    payloads = {"fc-bootstrap.exe": b"synthetic fixture only; not a PE or executable",
                "LICENSE": (ROOT / "LICENSE").read_bytes(),
                "THIRD-PARTY-NOTICES.txt": (ROOT / "packaging/THIRD-PARTY-NOTICES.txt").read_bytes(),
                "PROBE-README.txt": b"NOT_VALIDATED development fixture"}
    frontend = {"included": False, "status": "NOT_INCLUDED", "scope": "synthetic no-Web fixture"}
    if web:
        html = b"<!doctype html><title>synthetic</title>"
        document = {"schemaVersion": 1, "apiVersion": 1, "uiCompatibilityVersion": 1,
                    "assets": [{"path": "index.html", "sizeBytes": len(html), "sha256": SHA(html)}]}
        raw = json.dumps(document).encode()
        payloads.update({"web/index.html": html, "web/fc-assets.json": raw,
                         "web/THIRD-PARTY-NOTICES.txt": (ROOT / "web/THIRD-PARTY-NOTICES.txt").read_bytes()})
        frontend = {"included": True, "status": "PARTIAL_NOT_VALIDATED", "manifest_path": "web/fc-assets.json",
                    "manifest_sha256": SHA(raw), "schema_version": 1, "api_version": 1, "ui_compatibility_version": 1,
                    "asset_count": 1, "asset_bytes": len(html), "third_party_notices": "web/THIRD-PARTY-NOTICES.txt", "scope": "synthetic"}
    manifest = {"schema_version": 1, "product": "FutureClassroom", "version": "0.1.0", "artifact_name": "synthetic.zip",
                "artifact_type": "development-probe", "status": "NOT_VALIDATED", "target": M["TARGET"],
                "target_role": "formal-candidate-unvalidated", "toolchain": {"packaging_environment": "synthetic",
                "declared_build_toolchain": None, "provenance": "NOT_COMPILER_EVIDENCE"}, "sha256": SHA(payloads["fc-bootstrap.exe"]),
                "frontend": frontend, "features": {"fixture": "not production"}, "gates": {"release_tag": "NOT_ALLOWED"},
                "pe_audit": {"structural_errors": []}}
    ledger(manifest, payloads)
    return manifest, payloads


def ledger(manifest, payloads):
    manifest["files"] = [{"path": name, "size": len(data), "sha256": SHA(data)} for name, data in payloads.items()]


def zipped(manifest, payloads, extra=()):
    output = io.BytesIO()
    with zipfile.ZipFile(output, "w", compression=zipfile.ZIP_DEFLATED) as archive:
        archive.writestr("manifest.json", json.dumps(manifest).encode())
        for name, data in payloads.items():
            archive.writestr(name, data)
        for name, data in extra:
            with warnings.catch_warnings():
                warnings.simplefilter("ignore", UserWarning)
                archive.writestr(name, data)
    return output.getvalue()


class Inputs(unittest.TestCase):
    def reject(self, manifest, payloads, extra=()):
        with self.assertRaises((ValueError, zipfile.BadZipFile)):
            M["validate_zip"](zipped(manifest, payloads, extra))

    def test_exact_closed_ledgers_require_complete_web(self):
        manifest, payloads = fixture()
        actual, admitted = M["validate_zip"](zipped(manifest, payloads))
        self.assertEqual(actual, manifest)
        self.assertEqual(admitted, {**payloads, "manifest.json": json.dumps(manifest).encode()})
        self.reject(*fixture(False))
        with self.assertRaises(ValueError):
            M["pe_header"](fixture()[1]["fc-bootstrap.exe"])

    def test_traversal_absolute_ads_reserved_and_case_aliases(self):
        for name in ("../outside", "/fc-bootstrap.exe", "C:/fc-bootstrap.exe", "web/assets/a.js:stream", "web/assets/CON.js", "web/assets/a.JS", "web\\index.html", "FC-BOOTSTRAP.EXE", "web/assets/a.js."):
            with self.subTest(path=name):
                self.reject(*fixture(), [(name, b"attacker")])

    def test_duplicate_central_directory_and_unlisted_known_member(self):
        self.reject(*fixture(), [("LICENSE", b"duplicate")])
        self.reject(*fixture(), [("web/assets/extra.js", b"extra")])

    def test_symlink_and_directory_members_are_not_materialized(self):
        manifest, payloads = fixture()
        info = zipfile.ZipInfo("web/assets/evil.js")
        info.create_system = 3
        info.external_attr = (stat.S_IFLNK | 0o777) << 16
        self.reject(manifest, payloads, [(info, b"../../outside")])

    def test_hash_size_bool_and_ledger_set_mismatch(self):
        for update in ({"size": True}, {"size": 1}, {"sha256": "0" * 64}, {"extra": "field"}):
            with self.subTest(update=update):
                manifest, payloads = fixture()
                manifest["files"][0].update(update)
                self.reject(manifest, payloads)
        manifest, payloads = fixture()
        manifest["files"].pop()
        self.reject(manifest, payloads)

    def test_target_status_or_manifest_schema_cannot_promote_readiness(self):
        for key, value in (("target", "i686-pc-windows-gnu"), ("target", "i686-pc-windows-msvc"), ("status", "VALIDATED"), ("schema_version", True), ("extra", "field")):
            with self.subTest(key=key, value=value):
                manifest, payloads = fixture()
                manifest[key] = value
                self.reject(manifest, payloads)

    def test_license_cannot_be_replaced_even_with_updated_manifest_hash(self):
        for name in ("LICENSE", "THIRD-PARTY-NOTICES.txt", "web/THIRD-PARTY-NOTICES.txt"):
            with self.subTest(name=name):
                manifest, payloads = fixture(True)
                payloads[name] = b"substituted license"
                ledger(manifest, payloads)
                self.reject(manifest, payloads)

    def test_web_versions_and_transitive_manifest_binding(self):
        for kind in ("bad_version", "extra_asset_field", "wrong_digest", "unlisted_asset", "false_count", "unmanifested_payload"):
            with self.subTest(kind=kind):
                manifest, payloads = fixture(True)
                web = json.loads(payloads["web/fc-assets.json"])
                if kind == "bad_version":
                    web["apiVersion"] = True
                elif kind == "extra_asset_field":
                    web["assets"][0]["extra"] = 1
                elif kind == "wrong_digest":
                    web["assets"][0]["sha256"] = "0" * 64
                elif kind == "unlisted_asset":
                    web["assets"][0]["path"] = "assets/absent.js"
                elif kind == "false_count":
                    manifest["frontend"]["asset_count"] = True
                else:
                    payloads["web/assets/unlisted.js"] = b"unlisted"
                payloads["web/fc-assets.json"] = json.dumps(web).encode()
                manifest["frontend"]["manifest_sha256"] = SHA(payloads["web/fc-assets.json"])
                ledger(manifest, payloads)
                self.reject(manifest, payloads)

    def test_duplicate_json_keys_and_nonfinite_numbers(self):
        for raw in (b'{"version":1,"version":2}', b'{"number":NaN}', b'{"number":Infinity}'):
            with self.subTest(raw=raw), self.assertRaises(ValueError):
                M["json_document"](raw)

    def test_msi_version_rejects_fourth_field_and_unrepresentable_versions(self):
        for version in ("1.2.3.4", "01.2.3", "256.0.0", "0.256.0", "0.0.65536", "1.2", "1.2.-1", "1.2.3 ", True):
            with self.subTest(version=version), self.assertRaises(ValueError):
                M["version_parts"](version)

    def test_fresh_output_does_not_replace_directories_or_follow_aliases(self):
        with tempfile.TemporaryDirectory(prefix="FutureClassroom-MSI-input-check-") as name:
            parent = Path(name)
            created = M["fresh_output"](parent / "new")
            original = (created / ".owner").read_bytes()
            with self.assertRaises(FileExistsError):
                M["fresh_output"](created)
            self.assertEqual((created / ".owner").read_bytes(), original)
            (parent / "alias").symlink_to(created, target_is_directory=True)
            with self.assertRaises(ValueError):
                M["fresh_output"](parent / "alias" / "child")
            self.assertFalse((created / "child").exists())


class AuthoredContract(unittest.TestCase):
    def test_actual_export_requires_removal_inside_transaction(self):
        def export(initialize=1500, remove=1501, finalize=6600):
            return ("Action\tCondition\tSequence\ns72\tS255\tI2\nInstallExecuteSequence\tAction\n"
                    f"InstallInitialize\t\t{initialize}\nRemoveExistingProducts\t\t{remove}\nInstallFinalize\t\t{finalize}\n").encode()
        self.assertEqual(M["verify_transaction_sequence"](export())["RemoveExistingProducts"], 1501)
        for data in (export(remove=1401), export(remove=1500), export(remove=6600), export(remove=6700),
                     export().replace(b"RemoveExistingProducts", b"Missing"),
                     export() + b"RemoveExistingProducts\t\t1502\n"):
            with self.subTest(data=data), self.assertRaises(ValueError):
                M["verify_transaction_sequence"](data)

    def test_component_identity_survives_payload_reordering_and_version_change(self):
        self.assertEqual(M["component_code"]("web/index.html"), M["component_code"]("WEB/INDEX.HTML"))
        self.assertNotEqual(M["component_code"]("web/index.html"), M["component_code"]("LICENSE"))
        self.assertEqual(M["product_code"]("0.1.0"), M["product_code"]("0.1.0"))
        self.assertNotEqual(M["product_code"]("0.1.0"), M["product_code"]("0.1.1"))
        names = ["fc-bootstrap.exe", "web/index.html", "LICENSE", "MSI-CANDIDATE.txt"]
        self.assertEqual(M["authoring"]("0.1.0", names), M["authoring"]("0.1.0", list(reversed(names))))

    def test_authored_scope_side_effects_and_manual_candidate_ack(self):
        raw = M["authoring"]("0.1.0", ["fc-bootstrap.exe", "web/index.html", "LICENSE"])
        root = ET.fromstring(raw)
        nodes = list(root.iter())
        tags = [node.tag.split("}")[-1] for node in nodes]
        self.assertTrue({"Product", "Package", "MajorUpgrade", "Condition", "File"} <= set(tags))
        self.assertFalse({"CustomAction", "Registry", "RegistryValue", "RemoveRegistryValue", "RemoveFile", "ServiceInstall", "ServiceControl", "Launch", "Shortcut"} & set(tags))
        ns = {"w": M["WIX_NS"]}
        self.assertEqual(root.find("w:Product/w:Package", ns).attrib, {"InstallerVersion": "500", "Compressed": "yes", "InstallScope": "perUser"})
        conditions = [node.text for node in root.findall("w:Product/w:Condition", ns)]
        self.assertIn("NOT ALLUSERS", conditions)
        self.assertIn("Installed OR VersionNT >= 601", conditions)
        self.assertIn('Installed OR FC_CANDIDATE_ACK = "I_UNDERSTAND_NOT_VALIDATED"', conditions)
        components = root.findall(".//w:Component", ns)
        self.assertEqual(len(components), 3)
        self.assertTrue(all(node.get("Win64") == "no" and len(node) == 1 and node[0].get("KeyPath") == "yes" for node in components))
        self.assertNotIn(b"ProgramData", raw)
        self.assertNotIn(b"Schedule=", raw)
        self.assertEqual(root.find("w:Product/w:InstallExecuteSequence/w:RemoveExistingProducts", ns).attrib, {"After": "InstallInitialize"})
        directories = {node.get("Id"): node for node in root.findall(".//w:Directory", ns)}
        self.assertEqual(directories["INSTALLDIR"].get("Name"), "FutureClassroom")
        self.assertIn(directories["INSTALLDIR"], list(directories["UserPrograms"]))


if __name__ == "__main__":
    unittest.main(verbosity=2)
