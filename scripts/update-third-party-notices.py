#!/usr/bin/env python3
"""Audit or append Cargo.lock notices using checksum-verified cached .crate archives.

Run `python3 scripts/update-third-party-notices.py --update` after cargo fetch.
Default mode checks without writes. Existing component entries and all original
registry/native/toolchain license texts are retained byte for byte. This tool does
not automatically choose new SPDX alternatives or replace toolchain legal reviews.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import tarfile
import tomllib

ROOT = Path(__file__).resolve().parent.parent
NATIVE_MARKER = b"ADDITIONAL BUNDLED NATIVE SOURCE: SQLITE\n"
TEXT_MARKER = b"FULL ORIGINAL TEXTS (DEDUPLICATED BY SHA-256)\n"
RULE = b"=" * 76
TEXT_HEADER = re.compile(
    rb"(?m)^" + RULE + rb"\n(TEXT-\d+)\nOriginal local source: ([^\n]+)\n"
    rb"Text SHA-256: ([a-f0-9]{64})\n" + RULE + rb"\n"
)
COMPONENT = re.compile(rb"(?m)^Component: ([^\s]+) ([^\s]+)\n")
MAPPING = re.compile(rb"(?m)^  ([^\n]+?) -> (TEXT-\d+)(?: [^\n]*)?$")


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def matches_original(sha: str, included: bytes, upstream: bytes) -> bool:
    # Historical inventory generation normalized CRLF to LF for some files.
    # Retain those exact existing bytes, but require both the declared hash and
    # reproduced complete text to match the archive (with only that normalization).
    return any(sha == digest(candidate) and included.startswith(candidate)
               for candidate in (upstream, upstream.replace(b"\r\n", b"\n")))


def original_blocks(notice: bytes) -> dict[str, tuple[str, bytes]]:
    headers = list(TEXT_HEADER.finditer(notice))
    blocks = {}
    for index, header in enumerate(headers):
        identifier = header[1].decode()
        end = headers[index + 1].start() if index + 1 < len(headers) else len(notice)
        if identifier in blocks:
            raise ValueError(f"Duplicate original text identifier: {identifier}")
        blocks[identifier] = (header[3].decode(), notice[header.end():end])
    if not blocks:
        raise ValueError("No existing full original license texts found; refusing replacement")
    return blocks


def component_blocks(notice: bytes) -> dict[tuple[str, str], bytes]:
    registry = notice.split(NATIVE_MARKER, 1)[0]
    starts = list(COMPONENT.finditer(registry))
    result = {}
    for index, start in enumerate(starts):
        end = starts[index + 1].start() if index + 1 < len(starts) else len(registry)
        key = (start[1].decode(), start[2].decode())
        if key in result:
            raise ValueError(f"Duplicate component notice: {key}")
        result[key] = registry[start.start():end]
    return result


def source_files(package: dict, cargo_home: Path) -> tuple[dict, dict[str, bytes]]:
    name, version = package["name"], package["version"]
    if package["source"] != "registry+https://github.com/rust-lang/crates.io-index":
        raise ValueError(f"Review unsupported registry source for {name} {version}")
    archives = sorted((cargo_home / "registry" / "cache").glob(f"*/{name}-{version}.crate"))
    expected = package["checksum"]
    verified = [path for path in archives if digest(path.read_bytes()) == expected]
    if not verified:
        raise ValueError(f"No checksum-matching cached archive: {name} {version}; run cargo fetch --locked")
    prefix = f"{name}-{version}/"
    licenses = {}
    manifest = None
    # Nothing is extracted to disk. License bytes come directly from the verified
    # upstream archive, avoiding modified local unpacked sources and temp residue.
    with tarfile.open(verified[0], "r:gz") as archive:
        for entry in archive:
            if not entry.isfile() or not entry.name.startswith(prefix):
                continue
            relative = entry.name[len(prefix):]
            parts = PurePosixPath(relative)
            if parts.is_absolute() or ".." in parts.parts:
                raise ValueError(f"Unexpected archive path: {name} {version}")
            base = parts.name.upper()
            wanted = any(base.startswith(token) for token in ("LICENSE", "COPYING", "COPYRIGHT", "NOTICE", "UNLICENSE"))
            if relative != "Cargo.toml" and not wanted:
                continue
            if entry.size > 4 * 1024 * 1024:
                raise ValueError(f"Oversized license/manifest: {name} {version}/{relative}")
            source = archive.extractfile(entry)
            if source is None:
                raise ValueError(f"Unreadable archive entry: {name} {version}/{relative}")
            data = source.read()
            data.decode("utf-8")  # Refuse lossy replacement of original legal text.
            if relative == "Cargo.toml":
                manifest = tomllib.loads(data.decode())["package"]
            elif wanted:
                if relative in licenses:
                    raise ValueError(f"Duplicate license archive entry: {name} {version}/{relative}")
                licenses[relative] = data
    if manifest is None or manifest.get("name") != name or manifest.get("version") != version:
        raise ValueError(f"Upstream package identity mismatch: {name} {version}")
    if not licenses:
        raise ValueError(f"No license files found: {name} {version}; requires manual upstream review")
    return manifest, licenses


def reconcile(lock_bytes: bytes, notice: bytes, cargo_home: Path) -> tuple[bytes, dict]:
    if notice.count(NATIVE_MARKER) != 1 or notice.count(TEXT_MARKER) != 1:
        raise ValueError("Existing native/toolchain notice boundary missing or ambiguous")
    packages = [p for p in tomllib.loads(lock_bytes.decode())["package"] if p.get("source", "").startswith("registry+")]
    existing = component_blocks(notice)
    texts = original_blocks(notice)
    by_hash = {sha: identifier for identifier, (sha, _) in texts.items()}
    next_text = max(int(identifier.split("-")[1]) for identifier in texts) + 1
    mappings = []
    appended_texts = []
    added = []
    reviewed_files = 0
    for package in sorted(packages, key=lambda p: (p["name"], p["version"])):
        name, version = package["name"], package["version"]
        key = (name, version)
        manifest, licenses = source_files(package, cargo_home)
        expression = manifest.get("license")
        if not expression:
            raise ValueError(f"No SPDX manifest declaration: {name} {version}; requires review")
        if key in existing:
            block = existing[key]
            if f"Cargo.lock SHA-256: {package['checksum']}\n".encode() not in block:
                raise ValueError(f"Existing component checksum differs: {name} {version}")
            if f"Manifest license: {expression}\n".encode() not in block:
                raise ValueError(f"Existing SPDX expression differs: {name} {version}")
            files = {match[1].decode(): match[2].decode() for match in MAPPING.finditer(block)}
            for file, data in licenses.items():
                identifier = files.get(file)
                if identifier not in texts:
                    raise ValueError(f"Existing license file missing mapping: {name} {version}/{file}")
                sha, included = texts[identifier]
                if not matches_original(sha, included, data):
                    raise ValueError(f"Original license differs or was truncated: {name} {version}/{file}")
                reviewed_files += 1
            continue
        authors = "; ".join(manifest.get("authors", [])) or "See copyright notices below."
        lines = [f"Component: {name} {version}", f"Manifest license: {expression}", f"Authors: {authors}",
                 f"Source: https://crates.io/crates/{name}/{version}", f"Cargo.lock SHA-256: {package['checksum']}"]
        for file, data in sorted(licenses.items()):
            sha = digest(data)
            identifier = by_hash.get(sha)
            if identifier is None:
                identifier = f"TEXT-{next_text:03d}"
                next_text += 1
                by_hash[sha] = identifier
                heading = b"\n\n" + RULE + b"\n" + identifier.encode() + b"\nOriginal local source: " + f"{name}-{version}/{file}".encode() + b"\nText SHA-256: " + sha.encode() + b"\n" + RULE + b"\n"
                appended_texts.append(heading + data + (b"" if data.endswith(b"\n") else b"\n"))
            else:
                # A matching metadata hash alone is insufficient: ensure existing
                # full text really includes the bytes before reusing its mapping.
                if identifier in texts and not texts[identifier][1].startswith(data):
                    raise ValueError(f"Existing original text was truncated: {identifier}")
            lines.append(f"  {file} -> {identifier}")
            reviewed_files += 1
        mappings.append("\n".join(lines).encode() + b"\n\n")
        added.append({"name": name, "version": version, "license": expression, "files": sorted(licenses)})
    result = notice.replace(NATIVE_MARKER, b"".join(mappings) + NATIVE_MARKER, 1)
    result = re.sub(rb"(?m)^Cargo.lock file SHA-256: [a-f0-9]{64}$", b"Cargo.lock file SHA-256: " + digest(lock_bytes).encode(), result, count=1)
    result = re.sub(rb"(?m)^Registry component count: \d+$", f"Registry component count: {len(packages)}".encode(), result, count=1)
    result += b"".join(appended_texts)
    # Strong preservation invariant: all pre-existing original registry/native/
    # toolchain legal bytes form an unchanged prefix of the resulting legal tail.
    old_tail = notice[notice.index(TEXT_MARKER):]
    new_tail = result[result.index(TEXT_MARKER):]
    if not new_tail.startswith(old_tail):
        raise ValueError("Original legal text preservation invariant failed")
    report = {"registryComponents": len(packages), "verifiedArchiveChecksums": len(packages), "verifiedLicenseFiles": reviewed_files,
              "addedComponents": added, "appendedOriginalTexts": len(appended_texts), "preservedExistingOriginalTailSha256": digest(old_tail),
              "cargoLockSha256": digest(lock_bytes), "retainedHistoricalComponents": [list(key) for key in sorted(set(existing) - {(p['name'], p['version']) for p in packages})],
              "noticeCurrent": result == notice, "missingLicenseFiles": []}
    return result, report


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--update", action="store_true", help="append missing notices and update lock metadata")
    parser.add_argument("--cargo-home", type=Path, default=Path(os.environ.get("CARGO_HOME", Path.home() / ".cargo")))
    args = parser.parse_args()
    notice_path = ROOT / "packaging" / "THIRD-PARTY-NOTICES.txt"
    lock_path = ROOT / "Cargo.lock"
    lock_bytes = lock_path.read_bytes()
    previous = notice_path.read_bytes()
    result, report = reconcile(lock_bytes, previous, args.cargo_home)
    if lock_path.read_bytes() != lock_bytes or notice_path.read_bytes() != previous:
        raise ValueError("Lockfile/notices changed during audit; retry after concurrent edits finish")
    if args.update and result != previous:
        notice_path.write_bytes(result)
        report["updated"] = True
    else:
        report["updated"] = False
    print(json.dumps(report, ensure_ascii=False, indent=2))
    if not args.update and result != previous:
        raise SystemExit("Notices are stale; review and run with --update")


if __name__ == "__main__":
    main()
