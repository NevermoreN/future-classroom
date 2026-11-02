#!/usr/bin/env python3
"""Explicit GNU development fixture producer, never the formal MSVC admission.

Default inert. Root supplies one original frozen-source/compiler/Web evidence
manifest; selected real non-test artifacts and their original Cargo JSON must
match. This cannot turn a local DLL SHA or a test executable into CI provenance.
"""
from __future__ import annotations
import argparse
import hashlib
import json
import re
from pathlib import Path
import runpy
import subprocess
from types import SimpleNamespace

HERE = Path(__file__).absolute().parent
BUILD = runpy.run_path(str(HERE / "package-msi-candidate.py"))
TARGET = "i686-pc-windows-gnu"
ROLES = {"product-compile": ("fc-bootstrap", "fc-bootstrap", "fc-bootstrap.exe"),
         "lifecycle-compile": ("fc-bootstrap", "fc-installer-lifecycle", "fc-installer-lifecycle.exe"),
         "guard-library-compile": ("fc-windows", "fc_windows", "fc_windows.dll")}

def need(value, label):
    if not value:
        raise ValueError(label)

def fact(path: Path):
    raw = BUILD["PROBE"]["bounded_regular_file"](path, 64 * 1024 * 1024)
    return {"bytes": len(raw), "sha256": hashlib.sha256(raw).hexdigest()}

def relative(root: Path, name: str) -> Path:
    need(isinstance(name, str) and name and ":" not in name and "\\" not in name
         and not name.startswith("/") and all(p not in ("", ".", "..") for p in name.split("/")), "Safe evidence member")
    return root / name

def bound(root: Path, row: dict) -> Path:
    need(set(row) == {"path", "bytes", "sha256"} and type(row["bytes"]) is int and row["bytes"] >= 0
         and isinstance(row["sha256"], str) and re.fullmatch("[0-9a-f]{64}", row["sha256"]), "Exact evidence fact")
    path = relative(root, row["path"])
    need(fact(path) == {k: row[k] for k in ("bytes", "sha256")}, "Actual original evidence bytes: " + row["path"])
    return path

def source_inventory(source: Path) -> list[dict]:
    """Root may call before and after its compiler commands; this never builds."""
    paths = subprocess.check_output(["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"], cwd=source, timeout=10).decode().split("\0")
    paths = sorted({p for p in paths if p and (p.startswith(("crates/", "scripts/", "web/", ".cargo/", "packaging/"))
        or p in {"Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "LICENSE"})})
    return [{"path": p, **fact(relative(source, p))} for p in paths]

def admit(evidence: Path, pin: str, source: Path):
    need(fact(evidence)["sha256"] == pin, "Explicit original DEV evidence SHA")
    data = BUILD["json_document"](evidence.read_bytes());root = evidence.parent
    need(data["status"] == "DEV_FROZEN_NORMAL_BUILD_ONLY" and data["target"] == TARGET
         and data["source_root"] == str(source) and data["formal_ci"] is False, "Explicit DEV-only scope")
    head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=source, timeout=10).decode().strip()
    need(data["source_head"] == head, "Actual base Git HEAD (frozen bytes remain separately bound)")
    before = bound(root, data["inputs_before"]).read_bytes();after = bound(root, data["inputs_after"]).read_bytes()
    need(before == after and BUILD["json_document"](before) == source_inventory(source), "All current tracked/untracked build inputs equal original frozen before/after")
    metadata_raw = bound(root, data["metadata"]).read_bytes();metadata = BUILD["json_document"](metadata_raw)
    need(metadata["version"] == 1 and metadata["workspace_root"] == str(source), "Original same-workspace Cargo metadata")
    mc = data["metadata_command"]
    need(mc["actual_exit"] == 0 and type(mc["actual_exit"]) is int and mc["parent_reaped"] is True and mc["timed_out"] is False
         and mc["stdout"] == data["metadata"] and all(x in mc["argv"] for x in ("metadata", "--locked", "--format-version", "1", "--no-deps")), "Original completed metadata command")
    bound(root, mc["stderr"])
    packages = {p["name"]: p for p in metadata["packages"] if p["id"] in metadata["workspace_members"]}
    need(set(ROLES) == {r["name"] for r in data["commands"]} and len(data["commands"]) == 3, "Exactly three original normal compiler phases")
    artifacts = {}
    for command in data["commands"]:
        phase = command["name"];package_name, target_name, leaf = ROLES[phase]
        need(type(command["actual_exit"]) is int and command["actual_exit"] == 0 and command["parent_reaped"] is True and command["timed_out"] is False, "Original actual compiler completed")
        argv = command["argv"]
        need(isinstance(argv, list) and "--release" in argv and "--locked" in argv and "--message-format=json" in argv
             and any(v in argv for v in ("build", "rustc")) and argv.count("--target") == 1
             and argv.index("--target") + 1 < len(argv) and argv[argv.index("--target") + 1] == TARGET and argv.count("-p") == 1
             and argv.index("-p") + 1 < len(argv) and argv[argv.index("-p") + 1] == package_name
             and not any(v in argv for v in ("test", "--tests", "--all-targets", "--all-features", "--no-default-features", "--features")), "Closed default-feature normal release compiler argv")
        if phase == "guard-library-compile":
            need("--lib" in argv and "--bin" not in argv, "Actual normal cdylib selection")
        else:
            need(argv.count("--bin") == 1 and argv.index("--bin") + 1 < len(argv) and argv[argv.index("--bin") + 1] == target_name, "Actual normal GUI binary selection")
        log = bound(root, command["stdout"]).read_bytes();bound(root, command["stderr"])
        messages = [BUILD["json_document"](line) for line in log.splitlines() if line.startswith(b"{")]
        package = packages[package_name]
        need(package.get("source") is None and package["manifest_path"] == str(source / "crates" / package_name / "Cargo.toml"), "Exact workspace package")
        selected = [m for m in messages if m.get("reason") == "compiler-artifact" and m.get("package_id") == package["id"] and m.get("target", {}).get("name") == target_name]
        need(len(selected) == 1, "Exactly one original selected compiler artifact")
        message = selected[0];targets = [t for t in package["targets"] if t["name"] == target_name]
        need(len(targets) == 1 and message["target"] == targets[0] and message["manifest_path"] == package["manifest_path"]
             and message["profile"]["test"] is False and message["profile"]["opt_level"] != "0", "Normal same-metadata target, no test harness")
        expected_src = {"product-compile": "crates/fc-bootstrap/src/main.rs", "lifecycle-compile": "crates/fc-bootstrap/src/bin/fc-installer-lifecycle.rs", "guard-library-compile": "crates/fc-windows/src/lib.rs"}[phase]
        need(targets[0]["src_path"] == str(source / expected_src), "Exact normal target source")
        if phase == "guard-library-compile":
            need(set(targets[0]["crate_types"]) == {"rlib", "cdylib"}, "Real fc-windows library crate types")
            paths = [p for p in message["filenames"] if Path(p).name == leaf]
        else:
            need(targets[0]["kind"] == ["bin"] and targets[0]["crate_types"] == ["bin"], "Real normal executable target")
            paths = [message["executable"]]
        need(len(paths) == 1 and Path(paths[0]).name == leaf
             and Path(paths[0]).parent == Path(metadata["target_directory"]) / TARGET / "release", "Actual owned target release artifact location")
        copied = bound(root, command["artifact"])
        need(copied.read_bytes() == BUILD["PROBE"]["bounded_regular_file"](Path(paths[0]), 32 * 1024 * 1024), "Copy equals actual original compiler output")
        binary = copied.read_bytes()
        (BUILD["pe_dll_header"] if phase == "guard-library-compile" else BUILD["pe_header"])(binary)
        artifacts[phase] = (copied, binary)
    return data, artifacts, {"source_sha": head, "source_inventory_sha256": hashlib.sha256(before).hexdigest(),
        "result_sha256": pin, "current_inputs_verified": len(BUILD["json_document"](before)),
        "target": TARGET, "development_fixture_only": True, "formal_ci": False, "profile_test": False}

def main():
    p = argparse.ArgumentParser(description=__doc__);p.add_argument("--execute", action="store_true")
    p.add_argument("--source-root", type=Path, default=HERE.parent);p.add_argument("--evidence", type=Path);p.add_argument("--evidence-sha256")
    p.add_argument("--web-report", type=Path);p.add_argument("--web-report-sha256");p.add_argument("--web-dist", type=Path)
    p.add_argument("--input-dir", type=Path);p.add_argument("--output-dir", type=Path)
    for name in ("wixl", "msibuild", "msiinfo", "msiextract", "objdump"):p.add_argument("--" + name, type=Path)
    p.add_argument("--msibuild-sha256");a = p.parse_args()
    if not a.execute:
        print(json.dumps({"status": "PREPARED_NOT_RUN", "formal_ci": False, "Windows": "NOT_CONTACTED"}));return 0
    need(all(getattr(a, key) for key in ("evidence", "evidence_sha256", "web_report", "web_report_sha256", "web_dist", "input_dir", "output_dir", "wixl", "msibuild", "msiinfo", "msiextract", "objdump", "msibuild_sha256")), "All explicit DEV evidence/tools/output arguments")
    source = a.source_root.absolute();BUILD["configure_source_root"](source)
    data, artifacts, provenance = admit(a.evidence.absolute(), a.evidence_sha256, source)
    need(fact(a.web_report)["sha256"] == a.web_report_sha256, "Original Web producer report SHA")
    web = BUILD["json_document"](a.web_report.read_bytes())
    need(web["status"] == "PASS_OWNED_WEB_BUILD_ONLY_NOT_PRODUCT_MSI_RUNTIME" and web["productionAdmission"] is False
         and web["originalWorktreeInputsUnchanged"] is True and len(web["operations"]) == 2
         and all(r["actualExit"] == 0 and r["originalParentReaped"] is True for r in web["operations"]), "Actual finite Web producer only")
    for row in web["originalInputs"]:need(fact(relative(source, row["path"])) == {k:row[k] for k in ("bytes", "sha256")}, "Current Web source equals original Web producer")
    for row in web["outputs"]:bound(a.web_dist, row)
    notice = source / "web/THIRD-PARTY-NOTICES.txt"
    need(fact(notice) == {k:web["generatedLicense"][k] for k in ("bytes", "sha256")}, "Generated Web license matches frozen source notice")
    inputs = BUILD["fresh_output"](a.input_dir);(inputs / ".owner").write_text("future-classroom-owned-controlled-dev-input/v1\n")
    namespace = BUILD["assemble"].__globals__;probe = namespace["PROBE"]
    zip_path = inputs / "FutureClassroom-GNU-DEV.zip"
    probe["build_package"](artifacts["product-compile"][0], zip_path, TARGET, str(a.objdump), "Original DEV compiler evidence " + a.evidence_sha256, a.web_dist)
    raw = zip_path.read_bytes();manifest, payloads = BUILD["validate_zip"](raw, TARGET, "modern-GNU-development-only")
    args = SimpleNamespace(**vars(a), controlled=True)
    report = BUILD["assemble"](args, raw, zip_path, manifest, payloads, artifacts["lifecycle-compile"][1], provenance, artifacts["guard-library-compile"][1])
    need(source_inventory(source) == BUILD["json_document"](bound(a.evidence.parent, data["inputs_after"]).read_bytes()), "Frozen inputs unchanged after DEV packaging")
    print(json.dumps({"status": report["status"], "scope": "DEV_ONLY_NOT_FORMAL_MSVC", "production_integration": "NOT_EXERCISED", "output": str(a.output_dir)}));return 0

if __name__ == "__main__":raise SystemExit(main())
