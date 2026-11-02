"""ABI2 byte formats only; no CLI, source admission or installation authority.

The invariant carrier holds no product/version/payload ledger. Each package puts
that ledger in Binary.FcPayloadManifest; only the final external Setup receives
the SHA256 of the complete MSI. Legacy FCLM73 image parsing stays unchanged.
"""
from __future__ import annotations

import hashlib
from pathlib import Path
import re
import runpy
import struct

_BUILDER = runpy.run_path(str(Path(__file__).with_name("package-msi-candidate.py")))
FAMILY = b"99378807-098D-5F04-8D4D-F181DFA218D5"
CARRIER_MAGIC = b"FCCAR2\0\0"
CARRIER_FOOTER = b"FCCR2-END-v2\0\0\0\0"
PAYLOAD_MAGIC = b"FCPM2\0\0\0"
SEAL_MAGIC = b"FC-SETUP-MSI\0v1\0"
MAX_IMAGE = 64 * 1024 * 1024
MAX_BODY = 32768
MAX_PAYLOAD = 30 * 1024 * 1024
DESCRIPTOR = (
    CARRIER_MAGIC + struct.pack("<HH", 2, 64) + FAMILY
    + struct.pack("<HHHHI", 120, 64, 8, 72, MAX_PAYLOAD) + bytes(4)
)
OVERLAY_BYTES = 132


def _bounded(value: bytes, maximum: int) -> None:
    if type(value) is not bytes or not 1 <= len(value) <= maximum:
        raise ValueError("Expected nonempty bounded immutable bytes")


def _normal(prefix: bytes) -> None:
    _bounded(prefix, MAX_IMAGE - OVERLAY_BYTES)
    _BUILDER["pe_header"](prefix)
    if (prefix.endswith(CARRIER_FOOTER)
        or prefix.endswith(_BUILDER["LIFECYCLE_FOOTER"])
        or (len(prefix) >= 48 and prefix[-48:-32] == SEAL_MAGIC)):
        raise ValueError("Normal PE already carries an installer overlay")
    try:
        pe = struct.unpack_from("<I", prefix, 60)[0]
        count = struct.unpack_from("<H", prefix, pe + 6)[0]
        optional = pe + 24
        optional_bytes = struct.unpack_from("<H", prefix, pe + 20)[0]
        sections = optional + optional_bytes
        headers = struct.unpack_from("<I", prefix, optional + 60)[0]
        if (pe < 64 or not 1 <= count <= 96 or optional_bytes < 224
            or headers < sections + count * 40 or headers > len(prefix)):
            raise ValueError("Invalid PE headers or section table")
        ranges, end = [], headers
        for index in range(count):
            size, offset = struct.unpack_from("<II", prefix, sections + index * 40 + 16)
            if not size:
                continue
            stop = offset + size
            if offset < headers or stop > len(prefix) or any(offset < z and stop > a for a, z in ranges):
                raise ValueError("PE raw sections overlap or escape the file")
            ranges.append((offset, stop)); end = max(end, stop)
        directories = struct.unpack_from("<I", prefix, optional + 92)[0]
        if directories > 16:
            raise ValueError("Too many PE data directories")
        if directories > 4:
            certificate, size = struct.unpack_from("<II", prefix, optional + 128)
            if certificate or size:
                if certificate < end or certificate % 8 or size < 8 or certificate + size != len(prefix):
                    raise ValueError("PE certificate range mismatch")
                end = len(prefix)
        if end != len(prefix):
            raise ValueError("Normal PE has an unparsed overlay")
    except struct.error as error:
        raise ValueError("Truncated PE structure") from error


def make_carrier_template(normal_pe: bytes) -> bytes:
    _normal(normal_pe)
    return normal_pe + SEAL_MAGIC + bytes(32) + DESCRIPTOR + struct.pack("<I", 64) + CARRIER_FOOTER


def _parts(image: bytes) -> tuple[bytes, int]:
    _bounded(image, MAX_IMAGE)
    if len(image) <= OVERLAY_BYTES:
        raise ValueError("Truncated carrier")
    prefix = image[:-OVERLAY_BYTES]
    _normal(prefix)
    tail = image[-OVERLAY_BYTES:]
    if (tail[:16] != SEAL_MAGIC or tail[48:112] != DESCRIPTOR
        or tail[112:116] != struct.pack("<I", 64) or tail[116:] != CARRIER_FOOTER):
        raise ValueError("Carrier descriptor, version, budgets or footer mismatch")
    return prefix, len(prefix) + 16


def parse_carrier_template(template: bytes) -> bytes:
    prefix, offset = _parts(template)
    if template[offset:offset + 32] != bytes(32):
        raise ValueError("Template requires its canonical zero32 slot")
    return prefix


def bind_carrier_template(template: bytes, complete_msi: bytes) -> bytes:
    parse_carrier_template(template)
    _bounded(complete_msi, MAX_IMAGE)
    digest = hashlib.sha256(complete_msi).digest()
    if digest == bytes(32):
        raise ValueError("Zero digest is reserved for templates")
    offset = len(template) - OVERLAY_BYTES + 16
    return template[:offset] + digest + template[offset + 32:]


def verify_carrier_binding(template: bytes, image: bytes, complete_msi: bytes) -> bytes:
    """Full image equality outside exactly 32 bytes, not provenance approval."""
    prefix = parse_carrier_template(template)
    actual_prefix, offset = _parts(image)
    _bounded(complete_msi, MAX_IMAGE)
    digest = image[offset:offset + 32]
    if (len(template) != len(image) or actual_prefix != prefix
        or template[:offset] != image[:offset]
        or template[offset + 32:] != image[offset + 32:]
        or digest == bytes(32) or digest != hashlib.sha256(complete_msi).digest()):
        raise ValueError("Carrier differs outside its seal or MSI bytes changed")
    return prefix


def _guid(value: bytes) -> str:
    if (re.fullmatch(rb"[0-9A-F]{8}(?:-[0-9A-F]{4}){3}-[0-9A-F]{12}", value) is None
        or value == b"00000000-0000-0000-0000-000000000000"):
        raise ValueError("Expected canonical nonzero GUID")
    return value.decode("ascii")


def _path(name: str) -> bool:
    if name in _BUILDER["LIFECYCLE_REQUIRED"]:
        return True
    if not name.startswith("web/assets/"):
        return False
    leaf = name[len("web/assets/"):]
    stem = leaf.split(".")[0].upper()
    return (1 <= len(leaf) <= 128 and not leaf.startswith(".")
            and re.fullmatch(r"[A-Za-z0-9_.-]+", leaf) is not None
            and stem not in {"CON", "PRN", "AUX", "NUL"}
            and re.fullmatch(r"(?:COM|LPT)[1-9]", stem) is None
            and "." in leaf and leaf.rsplit(".", 1)[1] in {"js", "css", "svg", "png", "jpg", "jpeg", "webp"})


def _body(raw: bytes) -> dict:
    _bounded(raw, MAX_BODY)
    offset = 0

    def take(count: int) -> bytes:
        nonlocal offset
        if offset + count > len(raw):
            raise ValueError("Truncated payload ledger")
        value = raw[offset:offset + count]
        offset += count
        return value

    if take(8) != b"FCLM73\0\0":
        raise ValueError("Invalid embedded payload grammar")
    version, count = struct.unpack("<HH", take(4))
    if version != 1 or not 1 <= count <= 72:
        raise ValueError("Payload count/grammar mismatch")
    product = _guid(take(36))
    length = take(1)[0]
    if not 1 <= length <= 24:
        raise ValueError("Payload version length exceeded")
    product_version = take(length).decode("ascii")
    _BUILDER["version_parts"](product_version)
    entries, folded, components, total, previous = {}, set(), set(), 0, ""
    for _ in range(count):
        length = struct.unpack("<H", take(2))[0]
        if not 1 <= length <= 240:
            raise ValueError("Payload path length exceeded")
        name = take(length).decode("ascii")
        size = struct.unpack("<I", take(4))[0]
        digest = take(32)
        component = _guid(take(36))
        total += size
        if (not _path(name) or name <= previous or name.lower() in folded
            or component in components or not size or total > MAX_PAYLOAD or digest == bytes(32)):
            raise ValueError("Invalid payload namespace/order/identity/budget")
        entries[name] = {"bytes": size, "sha256": digest.hex(), "component_guid": component}
        folded.add(name.lower()); components.add(component); previous = name
    if offset != len(raw) or not _BUILDER["LIFECYCLE_REQUIRED"] <= entries.keys():
        raise ValueError("Payload ledger has trailing bytes or lacks required files")
    return {"product": product, "version": product_version, "entries": entries}


def parse_payload_manifest(envelope: bytes) -> dict:
    _bounded(envelope, 88 + MAX_BODY)
    if (len(envelope) < 88 or envelope[:8] != PAYLOAD_MAGIC
        or envelope[8:12] != struct.pack("<HH", 2, 2) or envelope[12:48] != FAMILY):
        raise ValueError("Payload envelope magic/format/ABI/family mismatch")
    package = _guid(envelope[48:84])
    length = struct.unpack_from("<I", envelope, 84)[0]
    if not 1 <= length <= MAX_BODY or len(envelope) != 88 + length:
        raise ValueError("Payload envelope length mismatch")
    return {"guard_abi": 2, "package": package, **_body(envelope[88:])}


def make_payload_manifest(package: str, body: bytes) -> bytes:
    if type(package) is not str:
        raise ValueError("Expected package identity text")
    _bounded(body, MAX_BODY)
    try:
        encoded = package.encode("ascii")
    except UnicodeError as error:
        raise ValueError("Expected ASCII package identity") from error
    _guid(encoded)
    result = PAYLOAD_MAGIC + struct.pack("<HH", 2, 2) + FAMILY + encoded + struct.pack("<I", len(body)) + body
    parse_payload_manifest(result)
    return result
