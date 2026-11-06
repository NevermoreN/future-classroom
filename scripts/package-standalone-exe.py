#!/usr/bin/env python3
"""Wrap an admitted sealed Setup/MSI pair in its exact normal GUI helper.

This adds a presentation/extraction envelope, never installation authority.
The inner original Setup retains source seals and every native guard.
"""
from __future__ import annotations
import argparse
import hashlib
import json
from pathlib import Path
import runpy
import struct
import io
import zipfile

HERE = Path(__file__).resolve().parent
C = runpy.run_path(str(HERE / 'installer-carrier-format.py'))
MAGIC = b'FC-SFX-PACK\0v1\0\0'
NAMES = ('Setup.exe', 'FutureClassroom.msi', 'LICENSE', 'THIRD-PARTY-NOTICES.txt')
LIMITS = (32 << 20, 128 << 20, 2 << 20, 4 << 20)
FOOTER = 88
MAGIC_V2 = b'FC-SFX-PACK\0v2\0\0'
PREVIOUS_NAMES = ('previous/Setup.exe', 'previous/FutureClassroom.msi')
PREVIOUS_LIMITS = (32 << 20, 128 << 20)
MAGIC_V3 = b'FC-SFX-PACK\0v3\0\0'
LEGACY_NAMES = ('legacy/Setup.exe', 'legacy/FutureClassroom.msi')
MAGIC_V4 = b'FC-SFX-PACK\0v4\0\0'


def distribution_archive(image: bytes) -> tuple[bytes, bytes]:
    if len(image) < 576 or image[-64:-48] != MAGIC_V4:
        raise ValueError('Unified distribution footer required')
    prefix, length = struct.unpack('<2Q', image[-48:-32])
    if not 512 <= prefix <= 32 << 20 or not 1 <= length <= 128 << 20 or prefix + length + 64 != len(image):
        raise ValueError('Unified distribution bounds differ')
    archive = image[prefix:-64]
    if hashlib.sha256(archive).digest() != image[-32:]:
        raise ValueError('Unified distribution checksum differs')
    return image[:prefix], archive


def distribution_members(archive: bytes) -> dict[str, bytes]:
    envelope = runpy.run_path(str(HERE / 'package-update-envelope.py'))
    # Reuse the closed canonical transport parser, including duplicate/path and
    # ZIP bounds checks. Signatures remain the existing publisher responsibility.
    source = io.BytesIO(archive)
    envelope['zip_directory'](source, len(archive), 36)
    with zipfile.ZipFile(source) as outer:
        files = envelope['members'](outer, envelope=True)
        names = set(files)
        if 'payload.zip' not in names or any(n != 'payload.zip' and not n.startswith('metadata/') for n in names):
            raise ValueError('Duplicate or missing distribution payload')
        envelope['metadata_names']({n.removeprefix('metadata/') for n in names if n != 'payload.zip'})
        if any(files[n].file_size > 256 << 10 for n in names if n != 'payload.zip') or sum(files[n].file_size for n in names if n != 'payload.zip') > 2 << 20:
            raise ValueError('Distribution metadata exceeds bounds')
        raw, _ = envelope['stream_member'](outer, files['payload.zip'], 128 << 20, collect=True)
        payload_source = io.BytesIO(raw)
        envelope['zip_directory'](payload_source, len(raw), 32)
        with zipfile.ZipFile(payload_source) as payload:
            files = envelope['members'](payload, envelope=False)
            if not set(NAMES + ('installer-variants.bin',)).issubset(files) or not set(files).issubset(NAMES + ('installer-variants.bin', 'manifest.json')):
                raise ValueError('Unified payload requires only current source and its variants')
            members = {}
            for name, limit in zip(NAMES, LIMITS):
                if not 1 <= payload.getinfo(name).file_size <= limit:
                    raise ValueError('Payload bound differs: ' + name)
                members[name] = payload.read(name)
            return members


def package_distribution(helper: bytes, archive: bytes) -> bytes:
    members = distribution_members(archive)
    C['verify_carrier_binding'](C['make_carrier_template'](helper), members['Setup.exe'], members['FutureClassroom.msi'])
    if not 512 <= len(helper) <= 32 << 20 or not 1 <= len(archive) <= 128 << 20:
        raise ValueError('Unified distribution bounds differ')
    return helper + archive + MAGIC_V4 + struct.pack('<2Q', len(helper), len(archive)) + hashlib.sha256(archive).digest()


def package(helper: bytes, members: dict[str, bytes]) -> bytes:
    legacy = set(members) == set(NAMES + PREVIOUS_NAMES + LEGACY_NAMES)
    previous = legacy or set(members) == set(NAMES + PREVIOUS_NAMES)
    names = NAMES + (PREVIOUS_NAMES if previous else ()) + (LEGACY_NAMES if legacy else ())
    limits = LIMITS + PREVIOUS_LIMITS * (2 if legacy else 1 if previous else 0)
    if set(members) != set(names) or not 512 <= len(helper) <= 32 << 20:
        raise ValueError('Exact normal helper and four fixed members required')
    for name, limit in zip(names, limits):
        if type(members[name]) is not bytes or not 1 <= len(members[name]) <= limit:
            raise ValueError('Member exceeds its bound: ' + name)
    template = C['make_carrier_template'](helper)
    C['verify_carrier_binding'](template, members['Setup.exe'], members['FutureClassroom.msi'])
    header = (MAGIC_V3 if legacy else MAGIC_V2 if previous else MAGIC) + struct.pack('<'+str(1+len(names))+'Q', len(helper), *(len(members[n]) for n in names))
    payload = b''.join(members[name] for name in names)
    return helper + payload + header + hashlib.sha256(header + payload).digest()


def inspect(image: bytes) -> tuple[bytes, dict[str, bytes]]:
    if len(image) >= 64 and image[-64:-48] == MAGIC_V4:
        helper, archive = distribution_archive(image)
        members = distribution_members(archive)
        C['verify_carrier_binding'](C['make_carrier_template'](helper), members['Setup.exe'], members['FutureClassroom.msi'])
        return helper, members
    legacy = len(image) >= 120 and image[-120:-104] == MAGIC_V3
    previous = legacy or len(image) >= 104 and image[-104:-88] == MAGIC_V2
    size = 120 if legacy else 104 if previous else FOOTER
    names = NAMES + (PREVIOUS_NAMES if previous else ()) + (LEGACY_NAMES if legacy else ())
    limits = LIMITS + PREVIOUS_LIMITS * (2 if legacy else 1 if previous else 0)
    header_size = size - 32
    if not size + 512 <= len(image) <= (32 << 20) + sum(limits) + size:
        raise ValueError('Standalone image size out of bounds')
    footer = image[-size:]
    if footer[:16] != (MAGIC_V3 if legacy else MAGIC_V2 if previous else MAGIC):
        raise ValueError('Wrong standalone footer')
    prefix, *lengths = struct.unpack('<'+str(1+len(names))+'Q', footer[16:header_size])
    if (not 512 <= prefix <= 32 << 20 or prefix + sum(lengths) + size != len(image)
        or any(not 1 <= length <= limit for length, limit in zip(lengths, limits))):
        raise ValueError('Standalone lengths differ')
    if hashlib.sha256(footer[:header_size] + image[prefix:-size]).digest() != footer[header_size:]:
        raise ValueError('Standalone checksum differs')
    members, offset = {}, prefix
    for name, length in zip(names, lengths):
        members[name] = image[offset:offset + length]
        offset += length
    helper = image[:prefix]
    C['verify_carrier_binding'](C['make_carrier_template'](helper), members['Setup.exe'], members['FutureClassroom.msi'])
    return helper, members


def main():
    p = argparse.ArgumentParser(description=__doc__)
    for name in ('helper', 'bundle', 'output'):
        p.add_argument('--' + name, type=Path, required=True)
    p.add_argument('--report-sha256', required=True)
    p.add_argument('--update-envelope', type=Path, required=True, help='Exact signed official update archive')
    a = p.parse_args()
    report_raw = C['_BUILDER']['PROBE']['bounded_regular_file'](a.bundle / 'report.json', 16 << 20)
    if hashlib.sha256(report_raw).hexdigest() != a.report_sha256:
        raise ValueError('Explicit formal report digest differs')
    report = json.loads(report_raw)
    if (report.get('formalCI') is not True or report.get('normalProfileTest') is not False
        or report.get('status') != 'PASS_ORIGINAL_WIN7_MSVC_CI_ABI2_SUCCESSOR_STATIC_CANDIDATE_NATIVE_NOT_RUN'):
        raise ValueError('Original formal successor required')
    helper = C['_BUILDER']['PROBE']['bounded_regular_file'](a.helper, 32 << 20)
    if hashlib.sha256(helper).hexdigest() != report['normalProvenance']['normal_helper_sha256']:
        raise ValueError('Original admitted normal GUI helper required')
    members = {}
    for name, limit in zip(NAMES, LIMITS):
        path = a.bundle / name if name in NAMES[:2] else a.bundle / 'payload' / name
        members[name] = C['_BUILDER']['PROBE']['bounded_regular_file'](path, limit)
        fact = report['setup' if name == 'Setup.exe' else 'msi'] if name in NAMES[:2] else next(
            {'bytes': row['size'], 'sha256': row['sha256']} for row in report['payloads'] if row['path'] == name)
        if len(members[name]) != fact['bytes'] or hashlib.sha256(members[name]).hexdigest() != fact['sha256']:
            raise ValueError('Formal member bytes differ: ' + name)
    archive = C['_BUILDER']['PROBE']['bounded_regular_file'](a.update_envelope, 128 << 20)
    if distribution_members(archive) != members:
        raise ValueError('Signed archive differs from formal current source')
    image = package_distribution(helper, archive)
    if inspect(image) != (helper, members):
        raise ValueError('Standalone complete readback failed')
    with a.output.open('xb') as stream:
        stream.write(image)
    print(json.dumps({'output': str(a.output), 'bytes': len(image), 'sha256': hashlib.sha256(image).hexdigest(),
                      'sourceCommit': report['sourceBase'], 'internalVersion': report['version'],
                      'members': list(members), 'readback': 'COMPLETE_EXACT',
                      'installationAuthority': 'original sealed inner Setup/MSI; wrapper is presentation only'}))


if __name__ == '__main__':
    main()
