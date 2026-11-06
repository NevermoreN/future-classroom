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
MAGIC_V2 = b'FC-SFX-PACK\0v2\0\0'
PREVIOUS_NAMES = ('previous/Setup.exe', 'previous/FutureClassroom.msi')
PREVIOUS_LIMITS = (32 << 20, 128 << 20)
MAGIC_V3 = b'FC-SFX-PACK\0v3\0\0'
LEGACY_NAMES = ('legacy/Setup.exe', 'legacy/FutureClassroom.msi')


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
    p.add_argument('--previous-setup', type=Path, help='Exact previously published EXE, bound to reviewed baseline')
    p.add_argument('--legacy-setup', type=Path, help='Exact official bridge installer; requires previous source')
    a = p.parse_args()
    if a.legacy_setup and not a.previous_setup:
        raise ValueError('Legacy bridge requires the exact predecessor source')
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
    if a.previous_setup:
        import re
        source = (Path(report['sourceRoot']) / 'crates/fc-windows/src/installer_lifecycle/native/broker/controlled_owner/setup_entry/previous_source.rs').read_text()
        for argument, prefix, names in [(a.previous_setup, '', PREVIOUS_NAMES), (a.legacy_setup, 'LEGACY_', LEGACY_NAMES)]:
            if argument is None:
                continue
            version = re.search(r'const '+prefix+r'VERSION:\s*&str\s*=\s*"([0-9.]+)";', source)[1]
            from_version = '1.0.' + '.'.join(version.split('.')[1:])
            if not prefix and from_version != report['predecessorPublication']['baseline']['publicVersion']:
                raise ValueError('Maintenance source must match the formal exact predecessor')
            baseline = json.loads((Path(report['sourceRoot']) / 'packaging/release-baselines' / (from_version+'.json')).read_bytes())
            old = C['_BUILDER']['PROBE']['bounded_regular_file'](argument, 384 << 20)
            if {'bytes': len(old), 'sha256': hashlib.sha256(old).hexdigest()} != baseline['setupExe']:
                raise ValueError('Previous public installer differs from reviewed baseline')
            _, previous = inspect(old)
            if {'bytes':len(previous['FutureClassroom.msi']), 'sha256':hashlib.sha256(previous['FutureClassroom.msi']).hexdigest()} != baseline['msi']:
                raise ValueError('Previous MSI differs from reviewed baseline')
            for symbol, member in [('SETUP_SHA', 'Setup.exe'), ('MSI_SHA', 'FutureClassroom.msi')]:
                match = re.search(r'const '+prefix+symbol+r':\s*\[u8;\s*32\]\s*=\s*\[(.*?)\];', source, re.S)
                expected = bytes(int(x,16) for x in re.findall(r'0x([0-9a-fA-F]{2})', match[1])) if match else b''
                if expected != hashlib.sha256(previous[member]).digest():
                    raise ValueError('Previous member differs from same-CI native maintenance binding: '+member)
            members.update({name:previous[name.split('/',1)[1]] for name in names})
    image = package(helper, members)
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
