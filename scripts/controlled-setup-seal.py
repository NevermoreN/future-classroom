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


def make_setup_template(lifecycle_image: bytes) -> bytes:
    """Insert the canonical zero digest slot for Binary.FcGuardClientTemplate.

    This template is embedded in the inner MSI before its final bytes exist.
    It contains no digest of that MSI and grants no installation authority.
    """
    prefix, raw, trailer, _ = _legacy_parts(lifecycle_image)
    if _has_adjacent_seal(prefix):
        raise ValueError("Lifecycle image already has a Setup seal or template")
    if len(lifecycle_image) > MAX_SETUP_IMAGE_BYTES - SEAL_BYTES:
        raise ValueError("Setup template exceeds native read bound")
    return prefix + SEAL_MAGIC + bytes(32) + raw + trailer


def parse_setup_template(template: bytes) -> tuple[bytes, dict]:
    """Require exactly one canonical magic/zero32 slot and intact legacy bytes."""
    sealed_prefix, raw, trailer, records = _legacy_parts(template)
    if not _has_adjacent_seal(sealed_prefix) or sealed_prefix[-32:] != bytes(32):
        raise ValueError("Guard template requires its exact zero digest slot")
    prefix = sealed_prefix[:-SEAL_BYTES]
    if _has_adjacent_seal(prefix):
        raise ValueError("Repeated Setup template or seal is forbidden")
    normal, original_records = _BUILDER["parse_lifecycle_image"](prefix + raw + trailer)
    if normal != prefix or original_records != records:
        raise ValueError("Guard template changed the legacy image")
    return prefix, records


def bind_setup_template(template: bytes, final_inner_msi: bytes) -> bytes:
    """Fill only the template's fixed zero32 hole after the inner MSI is final."""
    _bounded_bytes(final_inner_msi, MAX_INNER_MSI_BYTES, "Final inner MSI")
    parse_setup_template(template)
    sealed_prefix, _, _, _ = _legacy_parts(template)
    offset = len(sealed_prefix) - 32
    hashed = hashlib.sha256(final_inner_msi).digest()
    if hashed == bytes(32):
        raise ValueError("Final MSI digest cannot be the zero template sentinel")
    return template[:offset] + hashed + template[offset + 32:]


def seal_setup_image(lifecycle_image: bytes, final_inner_msi: bytes) -> bytes:
    """Seal either a legacy image or canonical zero-slot template to final MSI.

    No file path, caller-selected digest, installation flag or authority is
    accepted. The caller still owes the existing same-source CI admission.
    """
    _bounded_bytes(final_inner_msi, MAX_INNER_MSI_BYTES, "Final inner MSI")
    prefix, _, _, _ = _legacy_parts(lifecycle_image)
    template = (
        lifecycle_image if _has_adjacent_seal(prefix) else make_setup_template(lifecycle_image)
    )
    return bind_setup_template(template, final_inner_msi)


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


def verify_setup_template_binding(
    template: bytes, setup_image: bytes, final_inner_msi: bytes
) -> tuple[bytes, dict]:
    """Prove exact equality outside the one zero32-to-final-digest hole.

    No PE, manifest, footer, length, whitespace or other field is normalized.
    Caller still must establish same-source provenance of the original template.
    """
    parse_setup_template(template)
    result = parse_sealed_setup_image(setup_image, final_inner_msi)
    sealed_prefix, _, _, _ = _legacy_parts(template)
    offset = len(sealed_prefix) - 32
    if (
        len(template) != len(setup_image)
        or template[:offset] != setup_image[:offset]
        or template[offset + 32:] != setup_image[offset + 32:]
    ):
        raise ValueError("Setup differs from guard template outside the fixed digest hole")
    return result
