#!/usr/bin/env python3
"""Pure byte binding for a future controlled Setup wrapper, with no CLI.

The final inner MSI is built first. Only the separate Setup image receives its
SHA256 seal; the helper embedded in that MSI remains unchanged. This binds bytes,
not provenance, permission, MSI validity or production installation authority.
Existing builder/CI source admission and payload limits remain independent.
"""
from __future__ import annotations

import hashlib
from pathlib import Path
import runpy

_BUILDER = runpy.run_path(str(Path(__file__).resolve().with_name("package-msi-candidate.py")))
SEAL_MAGIC = b"FC-SETUP-MSI\0v1\0"
SEAL_BYTES = 48
# Match the native bounded image/MSI digest reader. This does not increase the
# existing producer's 30 MiB payload/MSI or 32 MiB normal-helper input limits.
MAX_SETUP_IMAGE_BYTES = 64 * 1024 * 1024
MAX_INNER_MSI_BYTES = 64 * 1024 * 1024


def _bounded_bytes(value: bytes, limit: int, label: str) -> None:
    if type(value) is not bytes or not 1 <= len(value) <= limit:
        raise ValueError(label + " must be nonempty bounded immutable bytes")


def _legacy_parts(image: bytes) -> tuple[bytes, bytes, bytes, dict]:
    _bounded_bytes(image, MAX_SETUP_IMAGE_BYTES, "Lifecycle image")
    prefix, records = _BUILDER["parse_lifecycle_image"](image)
    # The existing parser owns PE/footer/length and complete closed-ledger checks.
    manifest_raw = image[len(prefix):-20]
    trailer = image[-20:]
    return prefix, manifest_raw, trailer, records


def _has_adjacent_seal(prefix: bytes) -> bool:
    return len(prefix) >= SEAL_BYTES and prefix[-SEAL_BYTES:-32] == SEAL_MAGIC


def seal_setup_image(lifecycle_image: bytes, final_inner_msi: bytes) -> bytes:
    """Seal one previously admitted legacy lifecycle image to final MSI bytes.

    No file path, caller-selected digest, installation flag or authority is
    accepted. The caller still owes the existing same-source CI admission.
    """
    _bounded_bytes(final_inner_msi, MAX_INNER_MSI_BYTES, "Final inner MSI")
    prefix, raw, trailer, _ = _legacy_parts(lifecycle_image)
    if _has_adjacent_seal(prefix):
        raise ValueError("Lifecycle image already has a Setup seal")
    if len(lifecycle_image) > MAX_SETUP_IMAGE_BYTES - SEAL_BYTES:
        raise ValueError("Sealed Setup image exceeds native read bound")
    return prefix + SEAL_MAGIC + hashlib.sha256(final_inner_msi).digest() + raw + trailer


def parse_sealed_setup_image(setup_image: bytes, final_inner_msi: bytes) -> tuple[bytes, dict]:
    """Verify full final MSI bytes and return original normal helper plus ledger.

    The returned ledger describes effects only; it never authorizes the MSI.
    This function neither opens an installer nor grants an installation permit.
    """
    _bounded_bytes(final_inner_msi, MAX_INNER_MSI_BYTES, "Final inner MSI")
    sealed_prefix, raw, trailer, records = _legacy_parts(setup_image)
    if not _has_adjacent_seal(sealed_prefix):
        raise ValueError("Missing exact Setup seal before lifecycle manifest")
    prefix = sealed_prefix[:-SEAL_BYTES]
    if _has_adjacent_seal(prefix):
        raise ValueError("Repeated Setup seals are forbidden")
    seal_hash = sealed_prefix[-32:]
    if seal_hash == bytes(32) or seal_hash != hashlib.sha256(final_inner_msi).digest():
        raise ValueError("Setup seal does not match complete final inner MSI bytes")
    # Removing exactly one seal must recover a valid original legacy image,
    # including the existing nested-lifecycle-footer and original PE constraints.
    normal, original_records = _BUILDER["parse_lifecycle_image"](prefix + raw + trailer)
    if normal != prefix or original_records != records:
        raise ValueError("Setup seal removal changed the lifecycle image")
    return prefix, records
