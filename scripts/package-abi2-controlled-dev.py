#!/usr/bin/env python3
"""Normal GNU ABI2 development bundle. Default inert; no installation authority.

This separate DEV producer never changes formal MSVC admission. ABI2 binds a
normal immutable carrier to one complete MSI only in its external32-byte slot.
"""
from __future__ import annotations
import argparse
import hashlib
import json
from pathlib import Path
import re
import runpy
import struct
from types import SimpleNamespace
import uuid
import xml.etree.ElementTree as ET

HERE = Path(__file__).resolve().parent
DEV = runpy.run_path(str(HERE / 'package-controlled-dev.py'))
B = DEV['BUILD']
C = runpy.run_path(str(HERE / 'installer-carrier-format.py'))
NEED = DEV['need']
MANIFEST_BINARY = 'FcPayloadManifest'
BINARY_NAMES = (B['GUARD_BINARY'], B['TEMPLATE_BINARY'], MANIFEST_BINARY)


def extent(raw: bytes, dll: bool = False) -> dict:
    """Prove actual compiler bytes end at PE raw extent; never trim or patch."""
    if dll:
        B['pe_dll_header'](raw)
    else:
        C['_normal'](raw)
    pe = struct.unpack_from('<I', raw, 60)[0]
    symbol_pointer, symbols = struct.unpack_from('<II', raw, pe + 12)
    NEED(symbol_pointer == symbols == 0, 'Actual strip=symbols: no COFF symbol tail')
    count = struct.unpack_from('<H', raw, pe + 6)[0]
    optional = pe + 24
    size = struct.unpack_from('<H', raw, pe + 20)[0]
    sections = optional + size
    headers = struct.unpack_from('<I', raw, optional + 60)[0]
    NEED(1 <= count <= 96 and size >= 224 and sections + count * 40 <= headers <= len(raw), 'Complete bounded normal PE section table')
    end = headers; ranges = []
    for index in range(count):
        length, offset = struct.unpack_from('<II', raw, sections + index * 40 + 16)
        if not length: continue
        stop = offset + length
        NEED(headers <= offset < stop <= len(raw) and not any(offset < z and stop > a for a,z in ranges), 'Normal raw section bounds/nonoverlap')
        ranges.append((offset, stop)); end = max(end, stop)
    count_dirs = struct.unpack_from('<I', raw, optional + 92)[0]
    NEED(count_dirs <= 16, 'Normal PE directory bound')
    if count_dirs > 4:
        certificate, length = struct.unpack_from('<II', raw, optional + 128)
        if certificate or length:
            NEED(certificate >= end and certificate % 8 == 0 and length >= 8 and certificate + length == len(raw), 'Exact PE certificate extent')
            end = len(raw)
    NEED(end == len(raw), 'Normal actual raw extent equals file length; no trimming')
    return {'bytes': len(raw), 'sha256': B['digest'](raw), 'coffSymbolPointer': 0,
            'coffSymbolCount': 0, 'parsedExtentEqualsFileLength': True,
            'headerOrTailModified': False}


def maintenance_source_binding(source_root: Path, evidence: Path, data: dict) -> dict:
    mode = 'ABI2_SAME_PACKAGE_MAINTENANCE_NORMAL_GNU_DEV'
    NEED(data.get('lifecycle_mode') == mode, 'Exact actual maintenance producer mode, no Fresh-only artifact relabel')
    path = DEV['bound'](evidence.parent, data['maintenance_source'])
    binding = B['json_document'](path.read_bytes())
    NEED(binding['status'] == 'FROZEN_MAINTENANCE_SOURCE_OVERLAY_BOUND' and binding['sourceRoot'] == str(source_root)
         and binding['lifecycleMode'] == mode and 1 <= len(binding['sources']) <= 32, 'Original frozen maintenance source receipt')
    original = binding['originalFreeze']
    original_path = Path(original['path'])
    NEED(DEV['fact'](original_path) == {k:original[k] for k in ('bytes','sha256')}, 'Actual original maintenance freeze bytes/SHA')
    frozen = B['json_document'](original_path.read_bytes())
    owned = frozen['source']
    NEED(len(owned) == len(binding['sources']), 'Every original owned maintenance source must be mapped exactly once')
    NEED(len({r['path'] for r in binding['sources']}) == len(binding['sources']), 'Maintenance source mapping cannot omit an original by duplication')
    for row in binding['sources']:
        matches = [r for r in owned if r['relativePath'] == row['path'] and r['path'].endswith('/' + row['path'])]
        NEED(len(matches) == 1 and all(matches[0][k] == row[k] for k in ('bytes','sha256')), 'Exact source fact from original maintenance freeze')
        NEED(DEV['fact'](DEV['relative'](source_root,row['path'])) == {k:row[k] for k in ('bytes','sha256')}, 'Actual maintenance source bytes from frozen producer')
    return {'path': str(path), 'bytes': path.stat().st_size, 'sha256': B['file_digest'](path), 'lifecycleMode': mode,
            'originalFreeze': binding['originalFreeze'], 'sourceCount': len(binding['sources'])}


def admit_normal_inputs(source_root: Path, evidence: Path, evidence_sha256: str) -> dict:
    """Exact frozen mixed-source ledger, three real normal Cargo artifacts/PEs.

    Caller separately pins this validator and the package builder from the final
    report. source_base is only the Git base, never a clean-tree/source claim.
    This admits static DEV provenance only, not a runtime installer permission.
    """
    source_root=Path(source_root).absolute();evidence=Path(evidence).absolute()
    module={'B':B,'DEV':DEV,'NEED':NEED,'extent':extent}
    module['B']['configure_source_root'](source_root)
    data, artifacts, provenance=module['DEV']['admit'](evidence,evidence_sha256,source_root)
    maintenance=maintenance_source_binding(source_root,evidence,data)
    need=module['NEED']
    need(data['environment']['CARGO_PROFILE_RELEASE_STRIP']=='symbols'
         and 'FC_OWNED_REMOVAL_SCOPE_HEX'in data['removed_environment_keys'], 'Normal stripped default source without compiled test-removal scope')
    need(set(artifacts)=={'product-compile','lifecycle-compile','guard-library-compile'},'Exactly three normal artifacts')
    extents={role:module['extent'](raw,role=='guard-library-compile')for role,(_,raw)in artifacts.items()}
    rows={role:{'path':str(path),'bytes':len(raw),'sha256':hashlib.sha256(raw).hexdigest(),'profileTest':False,'extent':extents[role]}for role,(path,raw)in artifacts.items()}
    return dict(status='PASS_NORMAL_GNU_ABI2_DEV_SOURCE_AND_THREE_ARTIFACTS_ONLY',source_root=str(source_root),baseGit=data['source_head'],sourceEvidenceSha256=evidence_sha256,sourceInventory=provenance['source_inventory_sha256'],sourceInputCount=provenance['current_inputs_verified'],target='i686-pc-windows-gnu',formalCI=False,normalProfileTest=False,normalArtifacts=rows,maintenanceSourceBinding=maintenance,builder={'path':str(source_root/'scripts/package-abi2-controlled-dev.py'),'sha256':hashlib.sha256((source_root/'scripts/package-abi2-controlled-dev.py').read_bytes()).hexdigest()},Windows='NOT_RUN',runtimeAuthority='NOT_GRANTED')


def authoring(version: str, names: list[str]) -> bytes:
    xml = ET.fromstring(B['authoring'](version, names, True))
    ns = '{' + B['WIX_NS'] + '}'
    product = xml.find(ns + 'Product')
    ET.SubElement(product, ns + 'Binary', Id=MANIFEST_BINARY, SourceFile='lifecycle/payload-manifest.bin')
    ET.indent(xml)
    return ET.tostring(xml, encoding='utf-8', xml_declaration=True) + b'\n'


def validate_contract(summary: bytes, tables: dict, version: str, payloads: dict, package: str) -> dict:
    rows = B['idt_rows'](tables['Binary'], 'Binary')
    expected = [{'Name': name, 'Data': 'Binary.' + name} for name in BINARY_NAMES]
    NEED(sorted(rows, key=lambda x: x['Name']) == sorted(expected, key=lambda x: x['Name']), 'Exactly three fixed ABI2 Binary streams')
    # The complete legacy-controlled structural checker remains strict for the
    # unchanged two Binary rows and every other table. The third row is closed
    # above and its actual envelope bytes/package/payload identities below.
    structural = dict(tables)
    lines = tables['Binary'].decode('ascii').splitlines()
    structural['Binary'] = ('\r\n'.join(lines[:3] + ['\t'.join((r['Name'], r['Data'])) for r in rows if r['Name'] != MANIFEST_BINARY]) + '\r\n').encode('ascii')
    result = B['verify_msi_contract'](summary, structural, version, payloads, True)
    matches = re.findall(rb'^Revision number \(UUID\): \{([0-9A-F-]{36})\}\r?$', summary, re.M)
    NEED(matches == [package.encode('ascii')], 'Actual summary PackageCode equals envelope identity')
    result['guardABI'] = 2
    result['binaryStreams'] = list(BINARY_NAMES)
    result['packageCode'] = package
    return result


def assemble(a, evidence: dict, artifacts: dict, provenance: dict, web: dict) -> dict:
    source = a.source_root.absolute()
    inputs = B['fresh_output'](a.input_dir)
    (inputs / '.owner').write_text('future-classroom-owned-abi2-dev-input/v1\n')
    zip_path = inputs / 'FutureClassroom-GNU-ABI2-DEV.zip'
    B['PROBE']['build_package'](artifacts['product-compile'][0], zip_path, DEV['TARGET'], str(a.objdump), 'Frozen normal GNU ABI2 DEV compiler ' + a.evidence_sha256, a.web_dist)
    raw = zip_path.read_bytes()
    manifest, payloads = B['validate_zip'](raw, DEV['TARGET'], 'modern-GNU-development-only')
    product_version = a.product_version or manifest['version']
    B['version_parts'](product_version)
    payloads['MSI-CANDIDATE.txt'] = B['PROBE']['bounded_regular_file'](source / 'packaging/CONTROLLED-MSI-CANDIDATE.md', 65536)
    NEED(len(payloads) == 14 and sum(map(len, payloads.values())) <= B['MAX_INSTALLED'], 'Complete14 payloads within30MiB')
    helper = artifacts['lifecycle-compile'][1]
    dll = artifacts['guard-library-compile'][1]
    normal_extents = {name: extent(value[1], name == 'guard-library-compile') for name, value in artifacts.items()}
    template = C['make_carrier_template'](helper)
    NEED(C['parse_carrier_template'](template) == helper, 'Immutable normal helper is exact full ABI2 carrier prefix')
    package = str(uuid.uuid4()).upper()
    body = B['lifecycle_manifest'](product_version, payloads)
    envelope = C['make_payload_manifest'](package, body)
    decoded = C['parse_payload_manifest'](envelope)
    NEED(decoded['product'] == B['product_code'](product_version) and decoded['version'] == product_version and len(decoded['entries']) == 14, 'Exact generated Product/version/payload envelope')
    tools = {name: B['tool_file'](getattr(a, name)) for name in ('wixl', 'msibuild', 'msiinfo', 'msiextract', 'objdump')}
    NEED(B['file_digest'](tools['msibuild']) == a.msibuild_sha256, 'Pinned msibuild actual bytes')
    output = B['fresh_output'](a.output_dir)
    report = {'status': 'FAIL_RETAIN_ORIGINALS', 'scope': 'NORMAL_GNU_ABI2_DEV_STATIC_BUNDLE_ONLY_NOT_FORMAL_MSVC',
        'formalCI': False, 'normalProfileTest': False, 'installation': 'NOT_RUN', 'productionReadiness': 'NOT_GRANTED',
        'sourceRoot': str(source), 'sourceBase': evidence['source_head'], 'sourceEvidenceSha256': a.evidence_sha256,
        'normalProvenance': provenance, 'maintenanceSourceBinding': maintenance_source_binding(source, a.evidence.absolute(), evidence), 'normalPEExtents': normal_extents, 'webReportSha256': a.web_report_sha256,
        'productCode': decoded['product'], 'packageCode': package, 'version': product_version, 'artifactVersion': manifest['version'], 'payloadVersionPolicy': 'Actual artifact/manifest.json compiler version preserved; MSI release identity is an explicit separate build input',
        'productCodeGeneration': 'Existing fixed UUIDv5 per version; unchanged authoring policy', 'packageCodeGeneration': 'Actual unexecuted compiler Summary UUID, then manifest stream replacement and exact full Summary re-read before seal', 'intermediateEnvelopePackageCode': package, 'authoringPackageId': 'OMITTED_PINNED_WIXL_0_103_IGNORES_ATTRIBUTE',
        'commands': [], 'tools': {name: {'path': str(p), 'sha256': B['file_digest'](p)} for name, p in tools.items()},
        'payloads': [{'path': n, 'size': len(d), 'sha256': B['digest'](d), 'component_guid': B['component_code'](n)} for n, d in sorted(payloads.items())],
        'lifecycleMode': 'ABI2_SAME_PACKAGE_MAINTENANCE_NORMAL_GNU_DEV',
        'builderSha256': B['digest'](Path(__file__).read_bytes()), 'codecSha256': B['file_digest'](HERE / 'installer-carrier-format.py')}
    def tool(label, argv): return B['run_tool'](output, label, [str(x) for x in argv], report['commands'])
    try:
        for name in ('wixl', 'msiinfo', 'msiextract', 'objdump'):
            version = tool(name + '-version', [tools[name], '--version'])
            if name == 'wixl': NEED(version.strip() == b'0.103', 'Pinned wixl0.103')
        lifecycle = output / 'lifecycle'; lifecycle.mkdir()
        (lifecycle / 'guard-client-template.exe').write_bytes(template)
        (lifecycle / 'fc_windows.dll').write_bytes(dll)
        (lifecycle / 'payload-manifest.bin').write_bytes(envelope)
        for name, data in payloads.items():
            destination = output / 'payload' / name; destination.parent.mkdir(parents=True, exist_ok=True); destination.write_bytes(data)
        for role, leaf in [('product', 'payload/fc-bootstrap.exe'), ('helper', 'lifecycle/guard-client-template.exe'), ('guard', 'lifecycle/fc_windows.dll')]:
            inspected = tool(role + '-pe', [tools['objdump'], '-p', output / leaf])
            if role == 'guard': B['verify_guard_exports'](inspected)
            raw_pe = (output / leaf).read_bytes(); subsystem = struct.unpack_from('<H', raw_pe, struct.unpack_from('<I', raw_pe, 60)[0] + 92)[0]
            audit = B['PE']['parse_objdump'](inspected.decode('utf-8', 'strict'), 'gui' if subsystem == 2 else 'console')
            NEED(not audit['structural_errors'] and not any(r['kind'] == 'RUNTIME_DEPLOYMENT' for r in audit['risks']), 'Actual normal PE static runtime audit')
            report[role + 'PEAudit'] = audit
        (output / 'candidate.wxs').write_bytes(authoring(product_version, list(payloads)))
        msi = output / 'FutureClassroom.msi'
        tool('wixl-build', [tools['wixl'], '-a', 'x86', '-o', msi.name, 'candidate.wxs'])
        initial_summary = tool('intermediate-summary', [tools['msiinfo'], 'suminfo', msi.name])
        actual_packages = re.findall(rb'^Revision number \(UUID\): \{([0-9A-F-]{36})\}\r?$', initial_summary, re.M)
        NEED(len(actual_packages) == 1, 'One canonical actual compiler Summary PackageCode')
        package = actual_packages[0].decode('ascii')
        envelope = C['make_payload_manifest'](package, body)
        decoded = C['parse_payload_manifest'](envelope)
        NEED(decoded['package'] == package, 'Actual Summary identity encoded without caller package permission')
        (lifecycle / 'payload-manifest.bin').write_bytes(envelope)
        tool('manifest-actual-package-import', [tools['msibuild'], msi.name, '-a', 'Binary.' + MANIFEST_BINARY, 'lifecycle/payload-manifest.bin'])
        report['packageCode'] = package
        actions = tool('intermediate-CustomAction', [tools['msiinfo'], 'export', msi.name, 'CustomAction'])
        NEED(not B['idt_rows'](actions, 'CustomAction'), 'Unexecuted compiler intermediate has no custom actions')
        seq = tool('intermediate-sequence', [tools['msiinfo'], 'export', msi.name, 'InstallExecuteSequence'])
        (output / 'CustomAction.idt').write_bytes(B['lifecycle_custom_action_idt'](True))
        (output / 'InstallExecuteSequence.idt').write_bytes(B['lifecycle_execute_sequence_idt'](seq, True))
        tool('guard-import', [tools['msibuild'], msi.name, '-q', 'DROP TABLE `CustomAction`', '-q', 'DROP TABLE `InstallExecuteSequence`', '-i', 'CustomAction.idt', '-i', 'InstallExecuteSequence.idt'])
        summary = tool('summary', [tools['msiinfo'], 'suminfo', msi.name])
        NEED(summary == initial_summary, 'Whole Summary unchanged from actual compiler UUID through final stream/table imports')
        names = tool('tables', [tools['msiinfo'], 'tables', msi.name]).decode('ascii').splitlines()
        NEED('MsiEmbeddedChainer' not in names and B['CONTROLLED_REQUIRED_TABLES'] <= set(names), 'ABI2 required tables and no legacy chainer')
        tables = {n: tool('table-' + n, [tools['msiinfo'], 'export', msi.name, n]) for n in sorted(set(names) & (B['CONTROLLED_REQUIRED_TABLES'] | B['FORBIDDEN_TABLES']))}
        report['msiContract'] = validate_contract(summary, tables, product_version, payloads, package)
        actual_seq = B['idt_rows'](tables['InstallExecuteSequence'], 'InstallExecuteSequence')
        NEED(sorted(actual_seq, key=lambda r: r['Action']) == sorted(B['idt_rows']((output / 'InstallExecuteSequence.idt').read_bytes(), 'InstallExecuteSequence'), key=lambda r: r['Action']), 'Exact original standard sequence plus guard1499/condition1')
        for name, expected in zip(BINARY_NAMES, (dll, template, envelope)):
            actual = tool('binary-' + name, [tools['msiinfo'], 'extract', msi.name, 'Binary.' + name])
            NEED(actual == expected, 'Exact same-source Binary readback ' + name)
        complete = B['PROBE']['bounded_regular_file'](msi, B['MAX_INSTALLED'])
        setup = C['bind_carrier_template'](template, complete)
        NEED(C['verify_carrier_binding'](template, setup, complete) == helper, 'Final MSI-only outer32 binding and exact normal prefix')
        (output / 'Setup.exe').write_bytes(setup)
        hole = len(helper) + 16
        NEED(setup[:hole] == template[:hole] and setup[hole+32:] == template[hole+32:] and setup[hole:hole+32] == hashlib.sha256(complete).digest(), 'Only external32-byte hole filled after MSI completion')
        extracted = output / 'extracted'; extracted.mkdir()
        tool('payload-extract', [tools['msiextract'], '-C', extracted, msi.name])
        matches = list(extracted.rglob('fc-bootstrap.exe')); NEED(len(matches) == 1, 'Unique actual extracted payload root')
        actual = {}
        for p in extracted.rglob('*'):
            if p.is_dir(): B['PROBE']['real_directory'](p); continue
            NEED(p.is_relative_to(matches[0].parent), 'No extraction escape'); actual[p.relative_to(matches[0].parent).as_posix()] = B['PROBE']['bounded_regular_file'](p, B['MAX_INSTALLED'])
        NEED(actual == payloads, 'Actual extracted14 payload exact bytes')
        NEED(DEV['source_inventory'](source) == B['json_document'](DEV['bound'](a.evidence.parent, evidence['inputs_after']).read_bytes()), 'Full frozen source inventory unchanged')
        report.update(status='PASS_NORMAL_GNU_ABI2_DEV_STATIC_BUNDLE_NATIVE_NOT_RUN',
            msi={'path': msi.name, 'bytes': len(complete), 'sha256': B['digest'](complete)},
            setup={'path': 'Setup.exe', 'bytes': len(setup), 'sha256': B['digest'](setup)},
            template={'bytes': len(template), 'sha256': B['digest'](template), 'hole': hole, 'canonicalZero32': True, 'normalPrefixExact': True},
            payloadManifest={'bytes': len(envelope), 'sha256': B['digest'](envelope), 'rawBodySha256': B['digest'](body), 'payloads': 14},
            binaryReadback='ALL_THREE_EXACT', payloadReadback='ALL_FOURTEEN_EXACT', onlyExternal32HoleFilled=True)
    except BaseException as error:
        report['error'] = type(error).__name__ + ': ' + str(error); raise
    finally: (output / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    return report


def main():
    p = argparse.ArgumentParser(description=__doc__);p.add_argument('--execute', action='store_true')
    p.add_argument('--source-root', type=Path, default=HERE.parent)
    p.add_argument('--product-version', help='Explicit MSI release version only; never rewrites compiler artifact/payload version or carrier bytes')
    for name in ('evidence', 'web-report', 'web-dist', 'input-dir', 'output-dir', 'wixl', 'msibuild', 'msiinfo', 'msiextract', 'objdump'): p.add_argument('--' + name, type=Path)
    for name in ('evidence-sha256', 'web-report-sha256', 'msibuild-sha256'): p.add_argument('--' + name)
    a = p.parse_args()
    if not a.execute: print(json.dumps({'status': 'NOT_RUN', 'normalGNUDevOnly': True, 'formalCI': False}));return 0
    NEED(all(getattr(a, n.replace('-', '_')) for n in ('evidence', 'evidence-sha256', 'web-report', 'web-report-sha256', 'web-dist', 'input-dir', 'output-dir', 'wixl', 'msibuild', 'msiinfo', 'msiextract', 'objdump', 'msibuild-sha256')), 'All explicit source/evidence/tools/output inputs required')
    source = a.source_root.absolute(); B['configure_source_root'](source)
    data, artifacts, provenance = DEV['admit'](a.evidence.absolute(), a.evidence_sha256, source)
    maintenance = maintenance_source_binding(source, a.evidence.absolute(), data)
    NEED(data['environment']['CARGO_PROFILE_RELEASE_STRIP'] == 'symbols' and 'FC_OWNED_REMOVAL_SCOPE_HEX' in data['removed_environment_keys'], 'Actual producer strip and no compiled test-removal scope')
    web_raw = B['PROBE']['bounded_regular_file'](a.web_report, 32*1024*1024)
    NEED(B['digest'](web_raw) == a.web_report_sha256, 'Exact Web original report')
    web = B['json_document'](web_raw)
    NEED(web['status'] == 'PASS_OWNED_WEB_BUILD_ONLY_NOT_PRODUCT_MSI_RUNTIME' and web['productionAdmission'] is False and web['originalWorktreeInputsUnchanged'] is True and len(web['operations']) == 2 and all(x['actualExit'] == 0 and x['originalParentReaped'] is True for x in web['operations']), 'Original actual Web producer PASS only')
    for row in web['originalInputs']: NEED(DEV['fact'](DEV['relative'](source, row['path'])) == {k:row[k] for k in ('bytes','sha256')}, 'Exact unchanged Web source domain')
    for row in web['outputs']: DEV['bound'](a.web_dist, row)
    NEED(DEV['fact'](source/'web/THIRD-PARTY-NOTICES.txt') == {k:web['generatedLicense'][k] for k in ('bytes','sha256')}, 'Exact generated Web notice')
    report = assemble(a, data, artifacts, provenance, web)
    print(json.dumps({'status': report['status'], 'output': str(a.output_dir), 'Windows': 'NOT_RUN'})); return 0


if __name__ == '__main__': raise SystemExit(main())
