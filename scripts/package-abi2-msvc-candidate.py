#!/usr/bin/env python3
"""ABI2 candidate from three original Win7-target MSVC CI artifacts.

Default inert. This packages only; no compiled process or installer is run.
CI proves compilation. Static packaging never grants product release readiness,
Win7/Win10 runtime compatibility, or a Web source-build certification.
"""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import re
import runpy
import struct

HERE = Path(__file__).resolve().parent
ABI2 = runpy.run_path(str(HERE / 'package-abi2-upgrade-dev.py'))
B = ABI2['B']
NEED = ABI2['NEED']
TARGET = 'i686-win7-windows-msvc'


def check_compile_result(result: dict) -> None:
    """A closed compilation contract, additional to the shared CI admissions."""
    NEED(result.get('status') == 'COMPILE_CHECKS_PASSED_RUNTIME_NOT_RUN'
         and result.get('baseline') == 'Win7' and result.get('target') == TARGET
         and result.get('toolchain') == 'nightly-2026-10-29-x86_64-pc-windows-msvc'
         and result.get('rustflags') == '-C target-feature=+crt-static -C link-arg=/SUBSYSTEM:CONSOLE,6.01'
         and result.get('product_rustflags') == '-C target-feature=+crt-static -C link-arg=/SUBSYSTEM:WINDOWS,6.01',
         'Original successful fixed Win7 MSVC compile and static CRT flags required')
    commands = result.get('commands')
    NEED(isinstance(commands, list) and 1 <= len(commands) <= 160
         and all(isinstance(row, dict) and row.get('passed') is True for row in commands),
         'Complete original CI command set must pass')
    for phase, package, target in (
            ('product-compile', 'fc-bootstrap', 'fc-bootstrap'),
            ('lifecycle-compile', 'fc-bootstrap', 'fc-installer-lifecycle'),
            ('guard-library-compile', 'fc-windows', None)):
        expected = ['-Z', 'build-std', 'build' if target is None else 'rustc',
            '-vv', '--release', '--locked', '--target', TARGET, '-p', package]
        expected += ['--lib'] if target is None else ['--bin', target]
        expected += ['--message-format=json']
        rows = [row for row in commands if row.get('name') == phase]
        NEED(len(rows) == 1 and rows[0].get('arguments') == expected
             and type(rows[0].get('exit_code')) is int and rows[0]['exit_code'] == 0
             and rows[0].get('parent_exited') is True
             and rows[0].get('timed_out_or_pipe_stalled') is False
             and rows[0].get('log_truncated') is False,
             'Exact original closed normal compiler command: ' + phase)


def admit(a):
    """Reuse the original formal helper/guard CI, metadata and raw-source gates."""
    B['configure_source_root'](a.source_root)
    B['PROBE']['real_directory'](a.input_zip.parent)
    raw = B['PROBE']['bounded_regular_file'](a.input_zip, B['MAX_ZIP'])
    NEED(re.fullmatch(r'[0-9a-f]{64}', a.input_sha256) is not None
         and B['digest'](raw) == a.input_sha256, 'Explicit original input ZIP SHA')
    manifest, payloads = B['validate_zip'](raw)
    B['pe_header'](payloads['fc-bootstrap.exe'])
    helper, provenance = B['admit_normal_helper'](a, payloads, manifest['version'])
    guard, guard_provenance = B['admit_normal_guard'](a, provenance, manifest['version'])
    result_raw = B['PROBE']['bounded_regular_file'](a.build_evidence / 'result.json', B['MAX_LOG'])
    NEED(B['digest'](result_raw) == a.build_result_sha256 == provenance['result_sha256'],
         'Same original result after all three artifact admissions')
    result = B['json_document'](result_raw)
    check_compile_result(result)
    product = payloads['fc-bootstrap.exe']
    for image in (product, helper, guard):
        pe = struct.unpack_from('<I', image, 60)[0]
        NEED(struct.unpack_from('<HH', image, pe + 72) == (6, 1),
             'Each actual original MSVC PE declares subsystem version6.1')
    artifacts = {
        'product-compile': (a.build_evidence / 'product/fc-bootstrap/fc-bootstrap.exe', product),
        'lifecycle-compile': (a.lifecycle_helper, helper),
        'guard-library-compile': (a.guard_library, guard),
    }
    return raw, manifest, payloads, artifacts, provenance, guard_provenance


def builder_sources() -> list[dict]:
    names = ('package-abi2-msvc-candidate.py', 'package-abi2-upgrade-dev.py',
        'package-controlled-dev.py', 'package-msi-candidate.py',
        'installer-carrier-format.py', 'package-probe.py', 'audit-pe.py')
    return [{'path': 'scripts/' + name,
        'bytes': (HERE / name).stat().st_size,
        'sha256': B['file_digest'](HERE / name)} for name in names]


def read_candidate_instructions(a) -> tuple[bytes, dict]:
    """A separately reviewed builder document, never substituted CI source."""
    path = a.candidate_instructions.absolute()
    NEED(path.name == 'CONTROLLED-MSI-CANDIDATE.md', 'Explicit canonical candidate instructions name')
    B['PROBE']['real_directory'](path.parent)
    raw = B['PROBE']['bounded_regular_file'](path, 65536)
    NEED(re.fullmatch(r'[0-9a-f]{64}', a.candidate_instructions_sha256) is not None
         and B['digest'](raw) == a.candidate_instructions_sha256,
         'Actual separately reviewed candidate instructions bytes/SHA')
    ABI2['instructions_bytes'](a.source_root, raw)
    return raw, {'path': str(path), 'bytes': len(raw), 'sha256': B['digest'](raw),
        'scope': 'REVIEWED_BUILDER_DOCUMENT_NOT_ORIGINAL_CI_SOURCE'}


def build(a) -> dict:
    a.source_root = a.source_root.absolute()
    a.input_zip = a.input_zip.absolute()
    a.build_evidence = a.build_evidence.absolute()
    a.lifecycle_helper = a.lifecycle_helper.absolute()
    a.guard_library = a.guard_library.absolute()
    raw, manifest, payloads, artifacts, provenance, guard_provenance = admit(a)
    original_builders = builder_sources()
    original_instructions, instructions_fact = read_candidate_instructions(a)
    identity = {
        'scope': 'ORIGINAL_WIN7_MSVC_CI_ABI2_UPGRADE_STATIC_CANDIDATE_RUNTIME_NOT_RUN',
        'formalCI': True, 'target': TARGET, 'sourceBase': provenance['source_sha'],
        'sourceEvidenceSha256': a.build_result_sha256,
        'normalProvenance': provenance, 'normalGuardProvenance': guard_provenance,
        'lifecycleMode': 'ABI2_UPGRADE_CAPABLE_ORIGINAL_WIN7_MSVC_CI_CANDIDATE',
        'builderSources': original_builders,
        'sourceZip': {'path': str(a.input_zip), 'bytes': len(raw), 'sha256': B['digest'](raw)},
        'candidateInstructions': instructions_fact,
    }
    def verify_source():
        current = admit(a)
        NEED(current == (raw, manifest, payloads_before, artifacts, provenance, guard_provenance),
             'Original input ZIP, full source, CI result, metadata and three artifacts unchanged')
        NEED(builder_sources() == original_builders, 'Exact reviewed assembly dependencies unchanged')
        NEED(read_candidate_instructions(a) == (original_instructions, instructions_fact),
             'Exact separately reviewed candidate instructions unchanged')
    # Assembly adds its fixed instructions payload; preserve the admitted ZIP set
    # separately so final re-admission compares the original namespace, not it.
    payloads_before = dict(payloads)
    return ABI2['assemble_admitted'](a, manifest, payloads, artifacts, identity,
        'PASS_ORIGINAL_WIN7_MSVC_CI_ABI2_STATIC_CANDIDATE_NATIVE_NOT_RUN', verify_source,
        instructions=original_instructions)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--execute', action='store_true')
    parser.add_argument('--source-root', type=Path, default=HERE.parent,
        help='Exact original CI checkout bytes; builder identity is recorded separately')
    parser.add_argument('--product-version', help='Separate MSI identity; never rewrites original compiler bytes or artifact version')
    for name in ('input-zip', 'lifecycle-helper', 'guard-library', 'build-evidence', 'candidate-instructions',
            'output-dir', 'wixl', 'msibuild', 'msiinfo', 'msiextract', 'objdump'):
        parser.add_argument('--' + name, type=Path)
    for name in ('input-sha256', 'lifecycle-helper-sha256', 'guard-library-sha256',
            'build-result-sha256', 'msibuild-sha256', 'candidate-instructions-sha256'):
        parser.add_argument('--' + name)
    a = parser.parse_args()
    if not a.execute:
        print(json.dumps({'status': 'NOT_RUN', 'candidateOnly': True,
            'Windows': 'NOT_CONTACTED', 'productionReadiness': 'NOT_GRANTED'}))
        return 0
    NEED(all(getattr(a, name) for name in ('input_zip', 'input_sha256', 'lifecycle_helper',
        'lifecycle_helper_sha256', 'guard_library', 'guard_library_sha256', 'build_evidence',
        'build_result_sha256', 'output_dir', 'wixl', 'msibuild', 'msibuild_sha256',
        'msiinfo', 'msiextract', 'objdump', 'candidate_instructions',
        'candidate_instructions_sha256')), 'Every original CI/input/tool/output and reviewed instructions argument required')
    report = build(a)
    print(json.dumps({'status': report['status'], 'output': str(a.output_dir),
        'Windows': 'NOT_RUN', 'productionReadiness': 'NOT_GRANTED'}))
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
