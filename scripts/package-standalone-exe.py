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

HERE = Path(__file__).resolve().parent
C = runpy.run_path(str(HERE / 'installer-carrier-format.py'))
MAGIC = b'FC-SFX-PACK\0v1\0\0'
NAMES = ('Setup.exe', 'FutureClassroom.msi', 'LICENSE', 'THIRD-PARTY-NOTICES.txt')
LIMITS = (32 << 20, 128 << 20, 2 << 20, 4 << 20)
FOOTER = 88


def package(helper: bytes, members: dict[str, bytes]) -> bytes:
    if set(members) != set(NAMES) or not 512 <= len(helper) <= 32 << 20:
        raise ValueError('Exact normal helper and four fixed members required')
    for name, limit in zip(NAMES, LIMITS):
        if type(members[name]) is not bytes or not 1 <= len(members[name]) <= limit:
            raise ValueError('Member exceeds its bound: ' + name)
    template = C['make_carrier_template'](helper)
    C['verify_carrier_binding'](template, members['Setup.exe'], members['FutureClassroom.msi'])
    header = MAGIC + struct.pack('<5Q', len(helper), *(len(members[n]) for n in NAMES))
    payload = b''.join(members[name] for name in NAMES)
    return helper + payload + header + hashlib.sha256(header + payload).digest()


def inspect(image: bytes) -> tuple[bytes, dict[str, bytes]]:
    if not FOOTER + 512 <= len(image) <= (32 << 20) + sum(LIMITS) + FOOTER:
        raise ValueError('Standalone image size out of bounds')
    footer = image[-FOOTER:]
    if footer[:16] != MAGIC:
        raise ValueError('Wrong standalone footer')
    prefix, *lengths = struct.unpack('<5Q', footer[16:56])
    if (not 512 <= prefix <= 32 << 20 or prefix + sum(lengths) + FOOTER != len(image)
        or any(not 1 <= size <= limit for size, limit in zip(lengths, LIMITS))):
        raise ValueError('Standalone lengths differ')
    if hashlib.sha256(footer[:56] + image[prefix:-FOOTER]).digest() != footer[56:]:
        raise ValueError('Standalone checksum differs')
    members, offset = {}, prefix
    for name, size in zip(NAMES, lengths):
        members[name] = image[offset:offset + size]
        offset += size
    helper = image[:prefix]
    C['verify_carrier_binding'](C['make_carrier_template'](helper), members['Setup.exe'], members['FutureClassroom.msi'])
    return helper, members


def main():
    p = argparse.ArgumentParser(description=__doc__)
    for name in ('helper', 'bundle', 'output'):
        p.add_argument('--' + name, type=Path, required=True)
    p.add_argument('--report-sha256', required=True)
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
    image = package(helper, members)
    if inspect(image) != (helper, members):
        raise ValueError('Standalone complete readback failed')
    with a.output.open('xb') as stream:
        stream.write(image)
    print(json.dumps({'output': str(a.output), 'bytes': len(image), 'sha256': hashlib.sha256(image).hexdigest(),
                      'sourceCommit': report['sourceBase'], 'internalVersion': report['version'],
                      'members': list(NAMES), 'readback': 'COMPLETE_EXACT',
                      'installationAuthority': 'original sealed inner Setup/MSI; wrapper is presentation only'}))


if __name__ == '__main__':
    main()
