#!/usr/bin/env python3
"""Build independently sealed CURRENT-release sources for older official inputs.

The outer package carries one complete current source plus bounded COPY/LITERAL
differences. It never embeds an old Setup/MSI, rewrites an admitted MSI, or
changes the one-predecessor capability-3 contract of existing native guards.
"""
from __future__ import annotations

import argparse
import difflib
import hashlib
import io
import json
from pathlib import Path
import runpy
import stat
import struct
import zipfile
from types import SimpleNamespace

HERE = Path(__file__).resolve().parent
M = runpy.run_path(str(HERE / 'package-abi2-msvc-candidate.py'))
MAGIC = b'FC-MSI-VARIANTS1'
MAX_BYTES = 32 << 20


def canonical_payload(members: dict[str, bytes]) -> bytes:
    """Byte-reproducible wire also reconstructed by installed LAN seeds."""
    names = ['Setup.exe', 'FutureClassroom.msi', 'LICENSE', 'THIRD-PARTY-NOTICES.txt']
    if 'installer-variants.bin' in members:
        names.append('installer-variants.bin')
    if set(members) != set(names):
        raise ValueError('Exact current source payload members')
    output = io.BytesIO()
    with zipfile.ZipFile(output, 'w', compression=zipfile.ZIP_STORED, allowZip64=False) as archive:
        for name in names:
            info = zipfile.ZipInfo(name, (1980, 1, 1, 0, 0, 0))
            info.create_system = 3
            info.create_version = info.extract_version = 20
            info.external_attr = (stat.S_IFREG | 0o600) << 16
            archive.writestr(info, members[name])
    return output.getvalue()


def fact(raw: bytes) -> dict:
    return dict(bytes=len(raw), sha256=hashlib.sha256(raw).hexdigest())


def delta(base: bytes, target: bytes) -> bytes:
    """OLE streams use 512-byte sectors; preserve matching runs verbatim."""
    if not 0 < len(base) <= 128 << 20 or not 0 < len(target) <= 128 << 20:
        raise ValueError('Source bound')
    chunk = 512
    left = [base[n:n + chunk] for n in range(0, len(base), chunk)]
    right = [target[n:n + chunk] for n in range(0, len(target), chunk)]
    operations = []
    for kind, a, b, c, d in difflib.SequenceMatcher(None, left, right, autojunk=True).get_opcodes():
        if kind == 'equal':
            length = min(b * chunk, len(base)) - a * chunk
            operations.append(b'\0' + struct.pack('<II', a * chunk, length))
        elif d > c:
            raw = target[c * chunk:min(d * chunk, len(target))]
            operations.append(b'\1' + struct.pack('<I', len(raw)) + raw)
    if not 0 < len(operations) <= 65536:
        raise ValueError('Patch operation bound')
    return struct.pack('<I', len(operations)) + b''.join(operations)


def apply(base: bytes, patch: bytes, target: dict) -> bytes:
    if len(patch) < 4:
        raise ValueError('Short delta')
    count, = struct.unpack_from('<I', patch)
    if not 0 < count <= 65536:
        raise ValueError('Operation count')
    offset, output = 4, bytearray()
    for _ in range(count):
        if offset >= len(patch):
            raise ValueError('Short operation')
        tag = patch[offset]
        offset += 1
        if tag == 0:
            start, length = struct.unpack_from('<II', patch, offset)
            offset += 8
            raw = base[start:start + length]
        elif tag == 1:
            length, = struct.unpack_from('<I', patch, offset)
            offset += 4
            raw = patch[offset:offset + length]
            offset += length
        else:
            raise ValueError('Invalid operation')
        if not length or len(raw) != length or len(output) + length > target['bytes']:
            raise ValueError('Operation range')
        output.extend(raw)
    result = bytes(output)
    if offset != len(patch) or fact(result) != target:
        raise ValueError('Exact reconstructed source differs')
    return result


def encode(base_report: dict, base_setup: bytes, base_msi: bytes, variants: list[tuple], base_record: bytes) -> bytes:
    if not 0 < len(variants) <= 16:
        raise ValueError('Variant count')
    base_old = M['ABI2']['C']['parse_payload_manifest'](base_record[248:])
    catalog = dict(base_package_code=base_report['packageCode'],
                   base_predecessor_version=base_old['version'],
                   base_predecessor_product=base_old['product'],
                   base_predecessor_msi_sha256=M['decode_predecessor'](base_record)['source_msi_sha'],
                   base_setup=fact(base_setup),
                   base_msi=fact(base_msi), variants=[])
    blobs = []
    for report, record, setup, msi in variants:
        old = M['ABI2']['C']['parse_payload_manifest'](record[248:])
        first, second = delta(base_setup, setup), delta(base_msi, msi)
        if apply(base_setup, first, fact(setup)) != setup or apply(base_msi, second, fact(msi)) != msi:
            raise ValueError('Delta roundtrip')
        catalog['variants'].append(dict(package_code=report['packageCode'],
            predecessor_version=old['version'], predecessor_product=old['product'],
            predecessor_msi_sha256=M['decode_predecessor'](record)['source_msi_sha'],
            setup=fact(setup), msi=fact(msi), setup_delta_bytes=len(first), msi_delta_bytes=len(second)))
        blobs.extend((first, second))
    codes = [base_report['packageCode']] + [v['package_code'] for v in catalog['variants']]
    if len(set(codes)) != len(codes):
        raise ValueError('Each admitted MSI needs an independent actual PackageCode')
    metadata = json.dumps(catalog, separators=(',', ':'), sort_keys=True).encode('ascii')
    raw = MAGIC + struct.pack('<I', len(metadata)) + metadata + b''.join(blobs)
    if len(metadata) > 65536 or len(raw) > MAX_BYTES:
        raise ValueError('Variant container bound')
    return raw


def build_variants(base_bundle: Path, arguments: dict, specs: list[Path], output: Path) -> dict:
    """arguments are the exact reviewed formal-builder namespace, not CLI strings.

    Callers using locally pinned libmsi bindings must bind M['PUBLICATION'] in
    the same way as the base formal build before invoking this function.
    """
    base_report = json.loads((base_bundle / 'report.json').read_text())
    if base_report.get('formalCI') is not True or not base_report['status'].startswith('PASS_ORIGINAL_WIN7_MSVC_CI_ABI2_SUCCESSOR_'):
        raise ValueError('Original formal base successor required')
    base_setup, base_msi = (base_bundle / 'Setup.exe').read_bytes(), (base_bundle / 'FutureClassroom.msi').read_bytes()
    for raw, name in ((base_setup, 'setup'), (base_msi, 'msi')):
        if fact(raw) != {k: base_report[name][k] for k in ('bytes', 'sha256')}:
            raise ValueError('Base source digest')
    output.mkdir(exist_ok=False)
    variants, rows = [], []
    for index, spec in enumerate(specs):
        values = dict(arguments)
        for key in M['ARG_PATHS'] | {'wixl', 'msibuild', 'msiinfo', 'msiextract', 'objdump', 'output_dir'}:
            if values.get(key) is not None:
                values[key] = Path(values[key])
        spec = spec.resolve()
        values.update(predecessor_spec=spec, predecessor_spec_sha256=fact(spec.read_bytes())['sha256'],
                      output_dir=output / ('variant-' + str(index)))
        report = M['build'](SimpleNamespace(**values))
        if any(report[k] != base_report[k] for k in ('sourceBase', 'version', 'artifactVersion', 'payloads', 'normalProvenance', 'normalGuardProvenance')):
            raise ValueError('Variant must contain the exact same admitted current-release payloads')
        directory = values['output_dir']
        record = (directory / 'lifecycle/published-predecessor.bin').read_bytes()
        setup, msi = (directory / 'Setup.exe').read_bytes(), (directory / 'FutureClassroom.msi').read_bytes()
        variants.append((report, record, setup, msi))
        rows.append(dict(directory=str(directory), packageCode=report['packageCode'],
                         report=fact((directory / 'report.json').read_bytes())))
    base_record = (base_bundle / 'lifecycle/published-predecessor.bin').read_bytes()
    if fact(base_record) != base_report['publishedPredecessor']:
        raise ValueError('Base exact predecessor publication digest')
    raw = encode(base_report, base_setup, base_msi, variants, base_record)
    (output / 'installer-variants.bin').write_bytes(raw)
    members = {'Setup.exe':base_setup,'FutureClassroom.msi':base_msi,
        **{name:(base_bundle / 'payload' / name).read_bytes()
           for name in ('LICENSE','THIRD-PARTY-NOTICES.txt')}}
    for name in ('LICENSE','THIRD-PARTY-NOTICES.txt'):
        expected = next(row for row in base_report['payloads'] if row['path'] == name)
        if fact(members[name]) != dict(bytes=expected['size'],sha256=expected['sha256']):
            raise ValueError('Exact admitted license payload')
    legacy = canonical_payload(members)
    universal = canonical_payload(dict(members, **{'installer-variants.bin':raw}))
    (output / 'legacy-payload.zip').write_bytes(legacy)
    (output / 'universal-payload.zip').write_bytes(universal)
    report = dict(status='PASS_EXACT_CURRENT_SOURCE_VARIANTS_STATIC_NATIVE_NOT_RUN',
        builder=fact(Path(__file__).read_bytes()),
        baseReport=fact((base_bundle / 'report.json').read_bytes()), variants=rows,
        container=fact(raw), legacyPayload=fact(legacy), universalPayload=fact(universal),
        oldProgramSourcesIncluded=False, nativeUpgrade='NOT_RUN')
    (output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    return report


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--base-bundle', type=Path, required=True)
    p.add_argument('--arguments-json', type=Path, required=True)
    p.add_argument('--predecessor-spec', type=Path, action='append', required=True)
    p.add_argument('--output-dir', type=Path, required=True)
    a = p.parse_args()
    print(json.dumps(build_variants(a.base_bundle, json.loads(a.arguments_json.read_text()), a.predecessor_spec, a.output_dir)))
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
