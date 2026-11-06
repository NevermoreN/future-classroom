#!/usr/bin/env python3
"""Create/check D14 transport envelopes; never sign, trust, extract, or install.

.fcupdate contains byte-identical payload.zip and external TUF JSON files.
TRANSPORT_ONLY means structural/size/CRC checks, not TUF, Authenticode,
Setup/MSI source-seal or predecessor qualification. GNU developer bundles are
not formal update sources and cannot be promoted by this tool.
"""
from __future__ import annotations

import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import re
import stat
import struct
import sys
import zipfile

CHUNK = 64 * 1024
MAX_PAYLOAD = 128 * 1024 * 1024
MAX_ENVELOPE = 128 * 1024 * 1024
MAX_METADATA = 256 * 1024
MAX_METADATA_TOTAL = 2 * 1024 * 1024
MAX_SOURCE = 64 * 1024 * 1024
MAX_MANIFEST = 32 * 1024
# License files belong to the installed payload, not the TUF metadata budget.
# Keep their independent ceiling within the existing 30 MiB product payload cap.
MAX_LICENSE = 30 * 1024 * 1024
MAX_CENTRAL = 64 * 1024
ROLE_NAMES = {"timestamp.json", "snapshot.json", "targets.json"}
ROOT_NAME = re.compile(r"([1-9][0-9]{0,9})\.root\.json\Z")
INNER_LIMITS = {"Setup.exe": MAX_SOURCE, "FutureClassroom.msi": MAX_SOURCE,
                "LICENSE": MAX_LICENSE, "THIRD-PARTY-NOTICES.txt": MAX_LICENSE,
                "manifest.json": MAX_MANIFEST, "installer-variants.bin": 32 * 1024 * 1024}
REQUIRED_INNER = {"Setup.exe", "FutureClassroom.msi"}


def regular(path: Path):
    """Inputs must be actual regular files, not symlink/reparse aliases."""
    info = path.lstat()
    if not stat.S_ISREG(info.st_mode) or info.st_nlink != 1 or getattr(info, "st_file_attributes", 0) & 0x400:
        raise ValueError(f"not an independent regular file: {path}")
    return info


def metadata_names(names):
    if not ROLE_NAMES.issubset(names):
        raise ValueError("timestamp.json, snapshot.json and targets.json are required")
    roots = []
    for name in names:
        if name in ROLE_NAMES:
            continue
        match = ROOT_NAME.fullmatch(name)
        if not match:
            raise ValueError(f"unexpected metadata name: {name}")
        roots.append(int(match[1]))
    roots.sort()
    if len(roots) > 32 or any(b != a + 1 for a, b in zip(roots, roots[1:])):
        raise ValueError("root chain must contain at most 32 consecutive versions")
    return sorted(ROLE_NAMES) + [f"{version}.root.json" for version in roots]


def validate_json(raw):
    def unique(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise ValueError("duplicate JSON object key")
            result[key] = value
        return result
    def invalid_constant(_):
        raise ValueError("non-finite JSON number")
    value = json.loads(raw, object_pairs_hook=unique, parse_constant=invalid_constant)
    if not isinstance(value, dict):
        raise ValueError("metadata must be a JSON object; signatures remain UNVERIFIED")


def zip_directory(source, length, entry_limit):
    """Bound the central directory before ZipFile allocates its records.

    Canonical transport forbids comments, ZIP64, split archives, descriptors,
    prefix/trailing bytes and archive gaps. payload.zip may use deflate.
    """
    if length < 22:
        raise ValueError("truncated ZIP")
    source.seek(length - 22)
    eocd = source.read(22)
    signature, disk, central_disk, disk_entries, entries, size, offset, comment = struct.unpack("<4s4H2LH", eocd)
    if signature != b"PK\x05\x06" or disk or central_disk or comment or disk_entries != entries:
        raise ValueError("ZIP comments, split archives and trailing bytes are forbidden")
    if not 1 <= entries <= entry_limit or size > MAX_CENTRAL or offset + size != length - 22:
        raise ValueError("ZIP directory exceeds bounds or uses ZIP64")
    source.seek(0)


def members(archive, *, envelope):
    infos = archive.infolist()
    seen = set()
    previous_end = 0
    for info in infos:
        mode = info.external_attr >> 16
        if info.filename in seen or info.orig_filename != info.filename or "\x00" in info.filename:
            raise ValueError("duplicate or ambiguous ZIP member")
        seen.add(info.filename)
        if info.is_dir() or (mode and stat.S_IFMT(mode) not in (0, stat.S_IFREG)) or info.external_attr & 0x10:
            raise ValueError("only regular ZIP members are allowed")
        if info.flag_bits & ~0x800 or info.extra or info.comment:
            raise ValueError("encrypted, descriptor, extra-field and annotated ZIP members are forbidden")
        if info.compress_type not in ((zipfile.ZIP_STORED,) if envelope else (zipfile.ZIP_STORED, zipfile.ZIP_DEFLATED)):
            raise ValueError("unsupported ZIP compression")
        if info.compress_type == zipfile.ZIP_STORED and info.compress_size != info.file_size:
            raise ValueError("stored ZIP member size mismatch")
        if info.header_offset != previous_end:
            raise ValueError("ZIP member gaps, prefixes or overlapping records")
        archive.fp.seek(info.header_offset)
        raw = archive.fp.read(30)
        if len(raw) != 30:
            raise ValueError("truncated ZIP local header")
        local = struct.unpack("<4s5H3L2H", raw)
        if (local[0] != b"PK\x03\x04" or local[2] != info.flag_bits or local[3] != info.compress_type
                or local[6:9] != (info.CRC, info.compress_size, info.file_size) or local[10]):
            raise ValueError("ZIP local header differs from central directory")
        previous_end = info.header_offset + 30 + local[9] + local[10] + info.compress_size
    if previous_end != archive.start_dir:
        raise ValueError("ZIP payload and central directory are not adjacent")
    return {info.filename: info for info in infos}


def stream_member(archive, info, bound, *, collect=False):
    if not 0 < info.file_size <= bound or info.compress_size > MAX_PAYLOAD:
        raise ValueError(f"member size exceeds bound: {info.filename}")
    digest = hashlib.sha256()
    total = 0
    chunks = [] if collect else None
    with archive.open(info) as member:
        while chunk := member.read(CHUNK):
            total += len(chunk)
            if total > bound or total > info.file_size:
                raise ValueError("expanded member exceeds declared budget")
            digest.update(chunk)
            if collect:
                chunks.append(chunk)
    if total != info.file_size:
        raise ValueError("member length mismatch")
    return (b"".join(chunks) if collect else digest.hexdigest()), total


def inspect_payload(source, length):
    if not 0 < length <= MAX_PAYLOAD:
        raise ValueError("payload.zip exceeds 128 MiB")
    zip_directory(source, length, len(INNER_LIMITS))
    with zipfile.ZipFile(source) as archive:
        files = members(archive, envelope=False)
        if not REQUIRED_INNER.issubset(files) or not set(files).issubset(INNER_LIMITS):
            raise ValueError("payload requires original Setup.exe/MSI and fixed optional licenses/manifest only")
        if sum(info.file_size for info in files.values()) > MAX_PAYLOAD:
            raise ValueError("payload expanded total exceeds 128 MiB")
        for name, info in files.items():
            stream_member(archive, info, INNER_LIMITS[name])
    return sorted(files)


class PayloadSlice(io.RawIOBase):
    """Direct seek into the STORED payload, avoiding ZipExtFile.seek's large
    scratch reads when inspecting the nested ZIP. Never closes the outer file.
    """
    def __init__(self, source, start, length):
        self.source, self.start, self.length, self.position = source, start, length, 0

    def seekable(self):
        return True

    def tell(self):
        return self.position

    def seek(self, offset, whence=0):
        position = offset + (0 if whence == 0 else self.position if whence == 1 else self.length)
        if whence not in (0, 1, 2) or not 0 <= position <= self.length:
            raise ValueError("nested ZIP seek exceeds payload bounds")
        self.position = position
        return position

    def read(self, size=-1):
        count = min(self.length - self.position, CHUNK if size < 0 else min(size, CHUNK))
        self.source.seek(self.start + self.position)
        data = self.source.read(count)
        self.position += len(data)
        return data


def inspect(envelope: Path):
    length = regular(envelope).st_size
    if length > MAX_ENVELOPE:
        raise ValueError(".fcupdate exceeds 128 MiB")
    with envelope.open("rb") as source:
        zip_directory(source, length, 36)
        with zipfile.ZipFile(source) as archive:
            files = members(archive, envelope=True)
            if "payload.zip" not in files or any(name != "payload.zip" and not name.startswith("metadata/") for name in files):
                raise ValueError("only payload.zip and fixed metadata paths are allowed")
            ordered = metadata_names({name.removeprefix("metadata/") for name in files if name != "payload.zip"})
            metadata_total = 0
            for name in ordered:
                raw, count = stream_member(archive, files[f"metadata/{name}"], MAX_METADATA, collect=True)
                metadata_total += count
                if metadata_total > MAX_METADATA_TOTAL:
                    raise ValueError("metadata total exceeds 2 MiB")
                validate_json(raw)
            payload = files["payload.zip"]
            digest, count = stream_member(archive, payload, MAX_PAYLOAD)
            with PayloadSlice(source, payload.header_offset + 30 + len("payload.zip"), count) as inner:
                payload_files = inspect_payload(inner, count)
    return {"status": "TRANSPORT_ONLY", "payloadSha256": digest, "payloadBytes": count,
            "envelopeBytes": length, "metadataFiles": ordered, "payloadFiles": payload_files,
            "tufSignatures": "NOT_VERIFIED", "authenticodeAndSourceSeals": "NOT_VERIFIED",
            "upgradeQualification": "NOT_GRANTED"}


def copy_into(archive, name, source, bound):
    if not 0 < regular(source).st_size <= bound:
        raise ValueError(f"input exceeds bound: {source}")
    info = zipfile.ZipInfo(name, (1980, 1, 1, 0, 0, 0))
    info.create_system = 3
    info.external_attr = (stat.S_IFREG | 0o600) << 16
    total = 0
    # Known seekable output makes size/CRC final in the local header; no descriptor.
    with source.open("rb") as incoming, archive.open(info, "w") as outgoing:
        while chunk := incoming.read(CHUNK):
            total += len(chunk)
            if total > bound:
                raise ValueError("input changed or exceeded bound during copy")
            outgoing.write(chunk)
    return total


def create(payload: Path, metadata_dir: Path, output: Path):
    metadata_info = metadata_dir.lstat()
    if not stat.S_ISDIR(metadata_info.st_mode) or getattr(metadata_info, "st_file_attributes", 0) & 0x400:
        raise ValueError("metadata directory must be a real directory")
    names = set()
    for entry in metadata_dir.iterdir():
        if len(names) >= 35:
            raise ValueError("too many metadata files")
        names.add(entry.name)
    names = metadata_names(names)
    if sum(regular(metadata_dir / name).st_size for name in names) > MAX_METADATA_TOTAL:
        raise ValueError("metadata total exceeds 2 MiB")
    payload_size = regular(payload).st_size
    with payload.open("rb") as source:
        inspect_payload(source, payload_size)
    sizes = [(f"metadata/{name}", regular(metadata_dir / name).st_size) for name in names]
    sizes.append(("payload.zip", payload_size))
    total = 22 + sum(size + 30 + 46 + 2 * len(name.encode("utf-8")) for name, size in sizes)
    if total > MAX_ENVELOPE:
        raise ValueError(".fcupdate including ZIP headers would exceed 128 MiB")
    # Exclusive creation never overwrites an operator's existing delivery file.
    owned = False
    try:
        with output.open("xb") as destination:
            owned = True
            with zipfile.ZipFile(destination, "w", compression=zipfile.ZIP_STORED, allowZip64=False) as archive:
                for name in names:
                    copy_into(archive, f"metadata/{name}", metadata_dir / name, MAX_METADATA)
                copy_into(archive, "payload.zip", payload, MAX_PAYLOAD)
            destination.flush()
            os.fsync(destination.fileno())
        result = inspect(output)
        return result
    except BaseException:
        if owned:
            output.unlink()
        raise


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    build = commands.add_parser("create", help="wrap existing payload and public metadata without signing")
    build.add_argument("--payload", type=Path, required=True)
    build.add_argument("--metadata-dir", type=Path, required=True)
    build.add_argument("--output", type=Path, required=True)
    check = commands.add_parser("inspect", help="bounded read-only transport check; never extracts")
    check.add_argument("envelope", type=Path)
    args = parser.parse_args()
    try:
        result = create(args.payload, args.metadata_dir, args.output) if args.command == "create" else inspect(args.envelope)
    except (OSError, ValueError, RuntimeError, RecursionError, zipfile.BadZipFile, NotImplementedError) as error:
        print(f"update transport refused: {error}", file=sys.stderr)
        return 1
    print(json.dumps(result, ensure_ascii=False, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
