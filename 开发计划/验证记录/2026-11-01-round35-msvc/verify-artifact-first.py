#!/usr/bin/env python3
"""Verify one exact original compile-only CI artifact; never execute its EXEs.

Only the parent should invoke this after the selected original download report is PASS.
All derived destinations/report must be fresh. Failed evidence is retained.
"""
from __future__ import annotations
import argparse
import binascii
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import stat
import struct
import subprocess
import sys
import time
import zipfile

SOURCE = '83fe847f1a5b1787bdf2390c1294ae4e740e6e75'
RUN_ID = 41223081037
MIB = 1024 * 1024
CRT = re.compile(r'^(?:msvcr(?:t|\d+)[a-z0-9_]*|vcruntime[a-z0-9_]*|msvcp[a-z0-9_]*|ucrtbased?|api-ms-win-crt-[^\s]+|libgcc_s_[^\s]*|libstdc\+\+[^\s]*|libwinpthread[^\s]*)\.dll$', re.I)
RESERVED = re.compile(r'^(?:con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\..*)?$', re.I)

class Rejected(Exception):
    pass

def require(ok, message):
    if not ok:
        raise Rejected(message)

def sha(data):
    return hashlib.sha256(data).hexdigest()

def integer(value):
    return isinstance(value, int) and not isinstance(value, bool)

def safe_path(value):
    require(isinstance(value, str) and 0 < len(value) <= 1024, 'Invalid relative member path type/length')
    require(not any(ord(c) < 32 for c in value) and '\\' not in value and ':' not in value, 'Unsafe relative member path')
    path = PurePosixPath(value)
    require(not path.is_absolute() and path.as_posix() == value and all(p not in ('', '.', '..') for p in value.split('/')), 'Noncanonical relative member path')
    require(all(not p.endswith(('.', ' ')) and not RESERVED.fullmatch(p) for p in path.parts), 'Windows-ambiguous relative member path')
    return path

def new_bytes(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open('xb') as stream:
        stream.write(data)

def read_json(data):
    def unique(pairs):
        result = {}
        for key, value in pairs:
            require(key not in result, 'Duplicate JSON key')
            result[key] = value
        return result
    return json.loads(data.decode('utf-8-sig'), object_pairs_hook=unique)

def fact(path):
    data = path.read_bytes()
    return {'path': str(path), 'bytes': len(data), 'sha256': sha(data)}

def read_exact(stream, amount):
    chunks = bytearray()
    while len(chunks) < amount:
        part = stream.read(amount - len(chunks))
        require(bool(part), 'Short Git batch body')
        chunks.extend(part)
    return bytes(chunks)

def verify_git_inputs(repo, rows, source, report):
    require(isinstance(rows, list) and 0 < len(rows) <= 10000, 'Invalid source input inventory size')
    inventory = {}
    for row in rows:
        safe_path(row['path'])
        require(row['path'] not in inventory, 'Duplicate source input path')
        require(integer(row['bytes']) and 0 <= row['bytes'] <= 64 * MIB, 'Source input file exceeds bound')
        require(isinstance(row['sha256'], str) and re.fullmatch('[0-9a-f]{64}', row['sha256']), 'Invalid source input hash')
        inventory[row['path']] = row
    require(sum(r['bytes'] for r in rows) <= 256 * MIB, 'Source input aggregate bound')
    tree = subprocess.run(['git', 'ls-tree', '-r', '-z', source], cwd=repo, check=True, capture_output=True, timeout=60).stdout
    objects = {}
    for entry in tree.split(b'\0'):
        if not entry:
            continue
        header, path_bytes = entry.split(b'\t', 1)
        mode, kind, oid = header.split(b' ')
        require(kind == b'blob' and mode in (b'100644', b'100755'), 'Nonregular object in source commit')
        path = path_bytes.decode('utf-8')
        safe_path(path)
        objects[path] = oid.decode('ascii')
    require(set(inventory) == set(objects), 'Input inventory differs from specified source commit tree')
    require(len({p.casefold() for p in objects}) == len(objects), 'Case-colliding Git inputs')
    report['gitInputVerification'] = {'specifiedCommit': source, 'inventoryCount': len(rows), 'scope': 'All inputs compared to this specified commit, not current entire tree.', 'entries': [], 'lfToCrlfPaths': [], 'currentRustCargoInputs': []}
    out = report['gitInputVerification']
    batch = subprocess.Popen(['git', 'cat-file', '--batch'], cwd=repo, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
    try:
        for path in sorted(objects):
            row = inventory[path]
            batch.stdin.write((objects[path] + '\n').encode('ascii'))
            batch.stdin.flush()
            header = batch.stdout.readline(256).split()
            require(len(header) == 3 and header[0].decode('ascii') == objects[path] and header[1] == b'blob', 'Invalid Git batch header')
            length = int(header[2])
            require(0 <= length <= 64 * MIB, 'Git blob exceeds bound')
            data = read_exact(batch.stdout, length)
            require(read_exact(batch.stdout, 1) == b'\n', 'Invalid Git batch delimiter')
            method = 'EXACT_GIT_BLOB'
            if len(data) != row['bytes'] or sha(data) != row['sha256']:
                require(b'\0' not in data and b'\r\n' not in data, 'Source input is neither original blob nor exact LF-to-CRLF checkout')
                converted = data.replace(b'\n', b'\r\n')
                require(len(converted) == row['bytes'] and sha(converted) == row['sha256'], 'Source input differs from explicit checkout reconstruction')
                method = 'EXACT_LF_TO_CRLF_RECONSTRUCTION'
                out['lfToCrlfPaths'].append(path)
            out['entries'].append({'path': path, 'gitBlob': objects[path], 'gitBytes': len(data), 'gitSha256': sha(data), 'checkoutBytes': row['bytes'], 'checkoutSha256': row['sha256'], 'comparison': method})
            if path.endswith('.rs') or PurePosixPath(path).name in ('Cargo.toml', 'Cargo.lock') or path in ('.cargo/config.toml', 'rust-toolchain.toml'):
                current = repo.joinpath(*PurePosixPath(path).parts)
                require(current.is_file() and not current.is_symlink(), 'Current Rust/Cargo source missing or nonregular')
                current_data = current.read_bytes()
                if current_data == data:
                    comparison = 'CURRENT_EXACT_SOURCE_GIT_BLOB'
                elif b'\r\n' not in data and b'\0' not in data and current_data == data.replace(b'\n', b'\r\n'):
                    comparison = 'CURRENT_EXACT_LF_TO_CRLF_SOURCE_BLOB'
                else:
                    raise Rejected('Current Rust/Cargo input differs from frozen source')
                out['currentRustCargoInputs'].append({'path': path, 'bytes': len(current_data), 'sha256': sha(current_data), 'comparison': comparison})
        batch.stdin.close()
        require(batch.wait(timeout=15) == 0, 'Git batch failed')
    finally:
        if batch.poll() is None:
            batch.kill()
            batch.wait(timeout=10)
        if not batch.stdin.closed:
            batch.stdin.close()
        batch.stdout.close()
    out['exactGitBlobCount'] = sum(r['comparison'] == 'EXACT_GIT_BLOB' for r in out['entries'])
    out['exactLfToCrlfCount'] = len(out['lfToCrlfPaths'])
    out['currentRustCargoCount'] = len(out['currentRustCargoInputs'])
    production = [r for r in out['currentRustCargoInputs'] if r['path'].startswith('crates/') or r['path'] in ('Cargo.toml', 'Cargo.lock', '.cargo/config.toml', 'rust-toolchain.toml')]
    historical = [r for r in out['currentRustCargoInputs'] if r not in production]
    out['productionRustCargoCount'] = len(production)
    out['historicalEvidenceSourceCount'] = len(historical)
    out['historicalEvidenceSourcePaths'] = [r['path'] for r in historical]
    out['countClarification'] = 'Counts derived from exact source83fe Git tree at helper preparation; production and archived evidence paths reported separately.'
    require(len(production) == 383 and len(historical) == 14, 'Production/historical source input classification mismatch')
    require(out['currentRustCargoCount'] == 397, 'Specified-commit Rust/Cargo/toolchain input count mismatch')
    out['currentComparisonLimit'] = 'Compared exact source83fe main tree, not separate uncommitted Round35 feature worktree.'



class PreservedFiles(dict):
    def __getitem__(self, key):
        return super().__getitem__(key).read_bytes()


def verify(args, report):
    base = args.evidence
    expected = read_json(args.expected.read_bytes())
    require(expected['runId'] == RUN_ID and expected['sourceSha'] == SOURCE, 'Expected metadata run/source mismatch')
    download_path = args.download_report or (base / 'artifact-download-first.json')
    download = read_json(download_path.read_bytes())
    require(integer(expected['bytes']) and 0 < expected['bytes'] <= 128 * MIB, 'Invalid expected ZIP byte bound')
    require(isinstance(expected['sha256'], str) and re.fullmatch('[0-9a-f]{64}', expected['sha256']), 'Invalid expected ZIP SHA256')
    require(download['status'] == 'PASS' and download['sourceSha'] == SOURCE and download['runId'] == RUN_ID, 'Original full download not PASS for exact run/source')
    require(download['bytes'] == expected['bytes'] and download['sha256'] == expected['sha256'] and Path(download['path']) == args.archive, 'Download metadata mismatch')
    require(download['artifactId'] == expected['artifactId'], 'Download artifact identity mismatch')
    require(args.archive.is_file() and not args.archive.is_symlink() and args.archive.stat().st_size == expected['bytes'], 'Original ZIP missing/nonregular/wrong size')
    archive_data = args.archive.read_bytes()
    require(sha(archive_data) == expected['sha256'], 'Original full ZIP SHA256 mismatch')
    report['zip'] = {'path': str(args.archive), 'bytes': len(archive_data), 'sha256': sha(archive_data), 'artifactId': expected['artifactId'], 'downloadReport': fact(download_path), 'expectedMetadata': fact(args.expected), 'verification': 'FULL_ARCHIVE_BYTES_SHA256_AND_EVERY_MEMBER_CRC'}
    require(not args.raw.exists() and not args.programs.exists() and not (base / 'inspection-raw').exists(), 'Fresh extraction/inspection destinations required')
    args.raw.mkdir(parents=True, exist_ok=False)
    args.programs.mkdir(parents=True, exist_ok=False)
    all_members, nonbinary, binary_data = {}, PreservedFiles(), {}
    binary_names = set()
    report['rawRoot'] = str(args.raw)
    report['phase'] = 'zip-members-crc-and-preservation'
    with zipfile.ZipFile(io.BytesIO(archive_data)) as z:
        infos = z.infolist()
        require(0 < len(infos) <= 4096, 'ZIP member count bound')
        names, folded, total = set(), set(), 0
        canonical_components = {}
        for info in infos:
            require(info.orig_filename == info.filename, 'ZIP filename contains truncated NUL')
            name = info.filename[:-1] if info.is_dir() else info.filename
            safe_path(name)
            parts = PurePosixPath(name).parts
            for count in range(1, len(parts) + 1):
                prefix = '/'.join(parts[:count])
                require(canonical_components.get(prefix.casefold(), prefix) == prefix, 'Case-colliding ZIP directory components')
                canonical_components[prefix.casefold()] = prefix
            require(name not in names and name.casefold() not in folded, 'Duplicate/case-colliding ZIP member')
            names.add(name)
            folded.add(name.casefold())
            mode = (info.external_attr >> 16) & 0xffff
            require(stat.S_IFMT(mode) in (0, stat.S_IFREG, stat.S_IFDIR) and not stat.S_ISLNK(mode), 'ZIP symlink/special member rejected')
            require(not info.flag_bits & 1, 'Encrypted ZIP member rejected')
            require(0 <= info.file_size <= 128 * MIB, 'ZIP member byte bound')
            total += info.file_size
            require(total <= 512 * MIB, 'ZIP logical aggregate byte bound')
            if info.is_dir():
                require(info.file_size == 0, 'Nonempty ZIP directory')
                continue
            require(not stat.S_ISDIR(mode), 'ZIP file has directory mode')
            with z.open(info) as stream:
                data = stream.read(info.file_size + 1)
                require(stream.read(1) == b'', 'Extra ZIP member bytes')
            require(len(data) == info.file_size and binascii.crc32(data) & 0xffffffff == info.CRC, 'ZIP member size/CRC mismatch')
            row = {'path': name, 'bytes': len(data), 'sha256': sha(data), 'crc32': f'{info.CRC:08x}', 'binary': name.lower().endswith('.exe')}
            all_members[name] = row
            if row['binary']:
                basename = PurePosixPath(name).name
                require(re.fullmatch(r'[A-Za-z0-9_.-]+\.exe', basename) is not None, 'Invalid flat executable basename')
                require(basename.casefold() not in binary_names, 'Duplicate flattened executable basename')
                binary_names.add(basename.casefold())
                destination = args.programs / basename
                binary_data[name] = destination
            else:
                data.decode('utf-8-sig')
                destination = args.raw.joinpath(*PurePosixPath(name).parts)
                nonbinary[name] = destination
            new_bytes(destination, data)
            row['preservedPath'] = str(destination)
            del data
        # No regular file may be the parent of another member.
        for name in all_members:
            require(not any(str(parent) in all_members for parent in PurePosixPath(name).parents if str(parent) != '.'), 'ZIP file/directory prefix collision')
        report['zip']['memberCount'] = len(infos)
        report['zip']['logicalBytes'] = total
    # Single original ZIP read above; do not decompress/read any member twice.
    del archive_data
    report['members'] = list(all_members.values())
    ledger = read_json(nonbinary['artifact-sha256.json'])
    require(isinstance(ledger, list), 'All-member ledger is not an array')
    checked = set()
    for row in ledger:
        path = row['path']
        safe_path(path)
        require(path != 'artifact-sha256.json' and path in all_members and path not in checked, 'Invalid/duplicate all-member ledger path')
        checked.add(path)
        require(row['bytes'] == all_members[path]['bytes'] and row['sha256'] == all_members[path]['sha256'], 'All-member ledger bytes/SHA mismatch')
    require(checked == set(all_members) - {'artifact-sha256.json'}, 'All-member ledger incomplete or extra')
    report['allMemberLedger'] = {'status': 'PASS', 'entryCount': len(checked), 'selfEntry': 'Excluded by original producer; its exact bytes are covered by original ZIP and report mapping.'}
    # Every member is already preserved once; interpret only after CRC/ledger gates.
    report['phase'] = 'original-build-gates'
    result = read_json(nonbinary['result.json'])
    require('failure.json' not in nonbinary, 'Original artifact contains failure.json')
    require(result['status'] == 'COMPILE_CHECKS_PASSED_RUNTIME_NOT_RUN' and result['source_sha'] == SOURCE and result['dispatch_source_sha'] == SOURCE, 'Original compile result/source/dispatch gate failed')
    require(result['toolchain'] == '1.97.1-x86_64-pc-windows-msvc' and result['target'] == 'i686-pc-windows-msvc', 'Wrong compiler/target')
    require(result['executable_count'] == 22 and result['no_toolchain_payloads'] is True and result['no_environment_dump'] is True, 'Wrong executable count/payload policy')
    require(all(result[name] == 'NOT_RUN' for name in ('windows7', 'windows10', 'windows11', 'seewo_device')), 'Original result incorrectly claims runtime execution')
    bootstrap = read_json(nonbinary['bootstrap.json'])
    start = read_json(nonbinary['run-start.json'])
    require(bootstrap['expected_source_sha'] == SOURCE and bootstrap['dispatch_source_sha'] == SOURCE and str(bootstrap['run_id']) == str(RUN_ID), 'Original bootstrap source/run mismatch')
    require(bootstrap['repository'] == 'NevermoreN/future-classroom' and bootstrap['windows_runtime_tests'] == 'NOT_RUN', 'Original bootstrap scope mismatch')
    require(start['expected_source_sha'] == SOURCE and start['dispatch_source_sha'] == SOURCE and start['tests'] == 'COMPILE_ONLY_NO_RUN', 'Original start gate mismatch')
    report['originalResult'] = result
    steps = {}
    require(isinstance(result['commands'], list) and 0 < len(result['commands']) <= 256, 'Invalid command result inventory')
    for step in result['commands']:
        name = step['name']
        require(isinstance(name, str) and re.fullmatch('[a-z0-9-]+', name) and name not in steps, 'Invalid/duplicate command name')
        require(step['exit_code'] == 0 and step['passed'] is True and step['parent_exited'] is True and step['timed_out_or_pipe_stalled'] is False and step['log_truncated'] is False, 'Original command failed/timeout/truncated/not physically exited')
        require(step['log_limit_bytes_per_stream'] == 16 * MIB and isinstance(step['duration_seconds'], (int, float)) and step['duration_seconds'] >= 0, 'Original command log/time bound metadata mismatch')
        require(read_json(nonbinary[f'logs/{name}.result.json']) == step, 'Original per-step/result inventory mismatch')
        for key in ('stdout', 'stderr'):
            require(step[key] == f'logs/{name}.{key}.log' and len(nonbinary[step[key]]) <= 16 * MIB, 'Original command stream missing/bound exceeded')
        steps[name] = step
    required = {'source-sha', 'source-clean-before', 'source-input-paths', 'rustup-version', 'install-fixed-rust', 'rust-version', 'cargo-version', 'rust-target-cfg', 'visual-studio', 'cl-version-probe', 'cargo-metadata', 'fmt', 'architecture', 'workspace-test-compile', 'product-compile', 'clippy', 'source-clean-after'} | {f'pe-imports-{n}' for n in range(22)}
    require(set(steps) == required, 'Original command inventory is incomplete or unexpected')
    require(nonbinary[steps['source-sha']['stdout']].decode('utf-8-sig').strip() == SOURCE, 'Actual source-sha stdout mismatch')
    require(nonbinary[steps['source-clean-before']['stdout']] == b'' and nonbinary[steps['source-clean-after']['stdout']] == b'', 'Original source checkout not clean before/after')
    require(b'target_feature="crt-static"' in nonbinary[steps['rust-target-cfg']['stdout']], 'Original target cfg static CRT absent')
    for phase in ('before-vs', 'after-vs', 'before-cargo'):
        flags = read_json(nonbinary[f'logs/{phase}-crt-flags.json'])
        require(flags['phase'] == phase and flags['flags']['RUSTFLAGS']['present'] is True and flags['flags']['RUSTFLAGS']['value'] == '-C target-feature=+crt-static', 'Actual RUSTFLAGS gate failed')
        encoded = flags['flags']['CARGO_ENCODED_RUSTFLAGS']
        require(encoded['present'] is False and encoded['value'] is None and encoded['length'] is None, 'Encoded compiler flags not genuinely absent')
    cleanup = read_json(nonbinary['cleanup.json'])
    require(cleanup['status'] == 'OWNED_BUILD_ROOTS_REMOVED' and {r.get('root') for r in cleanup['roots']} == {'target', 'scratch', 'cargo', 'rustup'} and all(r.get('removed') is True for r in cleanup['roots']), 'Original owned build cleanup gate failed')
    report['originalCleanup'] = cleanup
    inputs_bytes = nonbinary['input-sha256.json']
    require(sha(inputs_bytes) == result['input_manifest_sha256'], 'Original input manifest SHA mismatch')
    host = read_json(nonbinary['host.json'])
    require(host['source_sha'] == SOURCE and host['input_manifest_sha256'] == sha(inputs_bytes), 'Original host/input identity mismatch')
    report['phase'] = 'specified-commit-git-inputs'
    verify_git_inputs(args.repo, read_json(inputs_bytes), SOURCE, report)
    executables = read_json(nonbinary['executables.json'])
    require(isinstance(executables, list) and len(executables) == 22, 'Original executable manifest length mismatch')
    require(all(e['role'] in ('tests', 'product') for e in executables), 'Invalid executable role')
    require(sum(e['role'] == 'tests' for e in executables) == 21 and sum(e['role'] == 'product' for e in executables) == 1, 'Expected 21 tests and one product')
    require({e['relative_path'] for e in executables} == set(binary_data) and len(binary_data) == 22, 'Executable manifest/ZIP binary membership mismatch')
    original_cargo = read_json(nonbinary[steps['cargo-metadata']['stdout']])
    package_names = {p['id']: p['name'] for p in original_cargo['packages'] if p['id'] in original_cargo['workspace_members']}
    compiled = {}
    for command in ('workspace-test-compile', 'product-compile'):
        for line in nonbinary[steps[command]['stdout']].decode('utf-8-sig').splitlines():
            if not line.startswith('{'):
                continue
            message = read_json(line.encode('utf-8'))
            if message.get('reason') != 'compiler-artifact' or not message.get('executable') or message.get('package_id') not in package_names:
                continue
            role = 'tests' if message['profile']['test'] else ('product' if command == 'product-compile' and 'bin' in message['target']['kind'] else None)
            if role is not None:
                basename = message['executable'].replace('\\', '/').rsplit('/', 1)[-1]
                identity = {'role': role, 'package': package_names[message['package_id']], 'target': message['target']['name'], 'kinds': message['target']['kind']}
                require(basename not in compiled or compiled[basename] == identity, 'Original Cargo executable basename collision')
                compiled[basename] = identity
    require(set(compiled) == {PurePosixPath(e['relative_path']).name for e in executables}, 'Original Cargo JSON/executable manifest membership mismatch')
    basenames = set()
    metadata = []
    report['binaryFacts'] = []
    spec = importlib.util.spec_from_file_location('fc_readonly_pe_audit', args.repo / 'scripts/audit-pe.py')
    audit = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(audit)
    tool = shutil.which(args.objdump)
    require(tool is not None, 'Local read-only objdump tool unavailable')
    inspection = base / 'inspection-raw'
    inspection.mkdir(exist_ok=False)
    version = subprocess.run([tool, '--version'], capture_output=True, timeout=10, check=True)
    new_bytes(inspection / 'logs/objdump-version.stdout.log', version.stdout)
    new_bytes(inspection / 'logs/objdump-version.stderr.log', version.stderr)
    began = time.monotonic()
    report['phase'] = 'actual-pe-imports'
    for number, exe in enumerate(executables):
        path = exe['relative_path']
        basename = PurePosixPath(path).name
        require(path.startswith(exe['role'] + '/' + exe['package'] + '/') and path.count('/') == 2 and re.fullmatch(r'[A-Za-z0-9_.-]+\.exe', basename), 'Invalid executable namespace')
        require(basename.casefold() not in basenames, 'Duplicate flattened executable basename')
        basenames.add(basename.casefold())
        require(compiled[basename] == {key: exe[key] for key in ('role', 'package', 'target', 'kinds')}, 'Original Cargo JSON/executable manifest profile/identity mismatch')
        safe_path(exe['package_working_directory'])
        preserved_binary = binary_data.pop(path)
        data = preserved_binary.read_bytes()
        require(exe['bytes'] == len(data) and exe['sha256'] == sha(data) and exe['executed'] is False, 'Original executable bytes/SHA/execution flag mismatch')
        require(exe['pe_machine'] == '0x014c' and exe['pe_optional_magic'] == '0x010b', 'Original executable PE declaration mismatch')
        require(len(data) >= 256 and data[:2] == b'MZ', 'Invalid DOS executable header')
        pe = struct.unpack_from('<I', data, 0x3c)[0]
        require(pe <= len(data) - 26 and data[pe:pe+4] == b'PE\0\0' and struct.unpack_from('<H', data, pe + 4)[0] == 0x14c and struct.unpack_from('<H', data, pe + 24)[0] == 0x10b, 'Actual executable is not PE32/i386')
        original_import = nonbinary[steps[f'pe-imports-{number}']['stdout']].decode('utf-8-sig')
        original_dlls = [line.strip().lower() for line in original_import.splitlines() if re.fullmatch(r'\s*[A-Za-z0-9_.+-]+\.dll\s*', line, re.I)]
        require(bool(original_dlls) and not any(CRT.fullmatch(dll) for dll in original_dlls), 'Original PE import log missing imports or contains dynamic CRT')
        destination = args.programs / basename
        require(preserved_binary == destination, 'Preserved executable destination mismatch')
        require(time.monotonic() - began < 900, 'Local PE inspection overall finite budget')
        local = subprocess.run([tool, '-p', str(destination)], capture_output=True, timeout=30, check=False)
        prefix = inspection / 'logs' / f'objdump-{number}'
        new_bytes(Path(str(prefix) + '.stdout.log'), local.stdout)
        new_bytes(Path(str(prefix) + '.stderr.log'), local.stderr)
        require(local.returncode == 0, 'Independent objdump process failed; original output retained')
        parsed = audit.parse_objdump(local.stdout.decode('utf-8', errors='strict'))
        require(not parsed['structural_errors'] and parsed['format'] == 'PE32-x86', 'Independent PE structural audit rejected executable')
        require(set(parsed['imports']) == set(original_dlls), 'Independent PE imports differ from original dumpbin log')
        require(not any(CRT.fullmatch(dll) for dll in parsed['imports']), 'Independent actual PE has direct dynamic CRT')
        binary_fact = {'originalPath': path, 'path': str(destination), 'role': exe['role'], 'package': exe['package'], 'packageWorkingDirectory': exe['package_working_directory'], 'target': exe['target'], 'bytes': len(data), 'sha256': sha(data), 'crc32': all_members[path]['crc32'], 'machine': '0x014c', 'optionalMagic': '0x010b', 'executed': False, 'originalImportLog': steps[f'pe-imports-{number}']['stdout'], 'localImportLog': fact(Path(str(prefix) + '.stdout.log')), 'peAudit': parsed}
        report['binaryFacts'].append(binary_fact)
        if exe['role'] == 'tests':
            metadata.append({'reason': 'compiler-artifact', 'profile': {'test': True}, 'target': {'name': exe['target'], 'kind': exe['kinds']}, 'executable': str(destination), 'package_id': exe['package'], 'package_working_directory': exe['package_working_directory']})
        del data
    require(not binary_data and len(metadata) == 21, 'Derived executable inventory mismatch')
    metadata_path = base / 'metadata.jsonl'
    new_bytes(metadata_path, ''.join(json.dumps(row, separators=(',', ':')) + '\n' for row in metadata).encode('utf-8'))
    report['derivedRuntimeMetadata'] = fact(metadata_path)
    report['derivedRuntimeMetadata']['testExecutables'] = len(metadata)
    report['sourcePostBuildImmutability'] = 'Original collector rehashed all initial inputs without difference and completed source-clean-after with exit0 and empty stdout. The after hash array is not separately emitted by producer.'
    report['platformScope'] = {'compile': 'Exact specified-source modern i686 MSVC compile checks verified; all original commands exit0 and complete/untruncated.', 'staticCrt': '22 original dumpbin import logs and 22 independent actual-PE inspections reject direct dynamic CRT.', 'modernApiRisks': 'Reported per binary; a static import list does not establish OS support or dynamic-load behavior.', 'windows7': 'NOT_RUN', 'windows10': 'NOT_RUN', 'windows11': 'NOT_RUN', 'seewoDevice': 'NOT_RUN', 'binariesExecutedByVerifier': False, 'cleanup': 'Only original hosted CI owned-build cleanup is verified; local extracted programs remain available for separately authorized runtime testing and later cleanup.'}
    report['phase'] = 'complete'
    report['status'] = 'PASS_ORIGINAL_COMPILE_ARTIFACT_VERIFIED_RUNTIME_NOT_RUN'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--evidence', type=Path, default=Path('/tmp/fc-round35-msvc-evidence'))
    parser.add_argument('--archive', type=Path, default=Path('/tmp/fc-round35-msvc-original.zip'))
    parser.add_argument('--repo', type=Path, default=Path('/workspace/共享/Eliora/未来教室计划'))
    parser.add_argument('--expected', type=Path)
    parser.add_argument('--download-report', type=Path, help='Explicit completed download report; default preserves the original first download path.')
    parser.add_argument('--objdump', default='i686-w64-mingw32-objdump')
    args = parser.parse_args()
    args.expected = args.expected or args.evidence / 'artifact-expected.json'
    args.raw = args.evidence / 'artifact-raw'
    args.programs = Path('/tmp/fc-round35-msvc-programs')
    path = args.evidence / 'artifact-verification-first.json'
    # Reserve report name before any extraction or subprocess; never overwrite.
    with path.open('xb') as output:
        report = {'status': 'FAIL', 'runId': RUN_ID, 'sourceSha': SOURCE, 'firstError': None, 'verifierSource': fact(Path(__file__).resolve()), 'binariesExecutedByVerifier': False, 'scope': 'Local-only original artifact validation and read-only PE inspection. No downloads, Windows calls, CI requests, compiled program execution or Git writes.'}
        try:
            verify(args, report)
        except Exception as error:
            # Fixed phase/type only for untrusted/system exceptions: never dump URLs/tokens/process environments.
            report['firstError'] = {'kind': type(error).__name__, 'detail': str(error) if isinstance(error, Rejected) else 'Validation exception; original local evidence retained. No external exception text exported.'}
        finally:
            output.write((json.dumps(report, ensure_ascii=False, indent=2) + '\n').encode('utf-8'))
    print(json.dumps({'status': report['status'], 'report': str(path), 'binaryCount': len(report.get('binaryFacts', [])), 'errorKind': (report['firstError'] or {}).get('kind')}), flush=True)
    return 0 if report['status'].startswith('PASS_') else 1

if __name__ == '__main__':
    raise SystemExit(main())
