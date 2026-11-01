#!/usr/bin/env python3
"""R73 prepared dual-GUI/metadata-derived test original-artifact auditor.
All source/run/job/artifact/ZIP identities and official metadata are required.
No defaults for any old run/source; no model/prepared path can emit artifact PASS.
Only static PE inspection is performed when root later invokes against actual
terminal original CI artifacts. No download, PE execution, Windows or dispatch.
"""
from __future__ import annotations
import argparse
import copy
import datetime
import hashlib
import io
import json
import math
import os
from pathlib import Path, PurePosixPath, PureWindowsPath
import re
import runpy
import stat
import struct
import subprocess
import sys
import tomllib
import zipfile
import zlib

EV = Path(__file__).resolve().parent
ROOT = Path('/workspace/共享/Eliora/未来教室计划')
PREFIX = '开发计划/验证记录/'
CI_SHA256 = 'edec879ecad9bad5b3ef21f48afa895bb3e8f0ac58e6aba561d5748e5e147802'
PRODUCT_COMMANDS = {'product-compile':'fc-bootstrap', 'lifecycle-compile':'fc-installer-lifecycle'}
MAX_SOURCE_PATHS = 20000 # Source inventory only; artifact/ZIP and byte caps stay unchanged.
MAX_FILE = 64 * 1024 * 1024
MAX_HASHED = 256 * 1024 * 1024
MAX_META = 16 * 1024 * 1024
MAX_PAYLOAD = 512 * 1024 * 1024
SCHEMA = 'future-classroom.ci-input-domains/v1'
TREE_SCHEMA = 'future-classroom.ci-git-tree-inputs/v1'
BASELINES = {
    'Modern': dict(toolchain='1.97.1-x86_64-pc-windows-msvc', target='i686-pc-windows-msvc',
        flags='-C target-feature=+crt-static', role='modern-msvc-compilation-only',
        prefix='future-classroom-msvc', workflow='.github/workflows/windows-msvc.yml'),
    'Win7': dict(toolchain='nightly-2026-10-29-x86_64-pc-windows-msvc', target='i686-win7-windows-msvc',
        flags='-C target-feature=+crt-static -C link-arg=/SUBSYSTEM:CONSOLE,6.01',
        role='win7-msvc-candidate-compilation-only', prefix='future-classroom-win7-msvc',
        workflow='.github/workflows/windows-win7-msvc.yml'),
}

class Invalid(Exception):
    """Fixed diagnostic codes only: never include response bodies, URLs or credentials."""

def need(ok, code):
    if not ok:
        raise Invalid(code)

def integer(value, maximum, code='INVALID_INTEGER', minimum=0):
    need(type(value) is int and minimum <= value <= maximum, code)
    return value

def hexstr(value, length, code='INVALID_DIGEST'):
    need(isinstance(value, str) and re.fullmatch('[0-9a-f]{'+str(length)+'}', value) is not None, code)
    return value

def digest(data):
    return hashlib.sha256(data).hexdigest()

def ci_source_matches(data):
    return digest(data) == CI_SHA256

def file_fact(path):
    h = hashlib.sha256(); count = 0
    with Path(path).open('rb') as source:
        for chunk in iter(lambda: source.read(256 * 1024), b''):
            h.update(chunk); count += len(chunk)
    return dict(path=str(path), bytes=count, sha256=h.hexdigest())

def json_pairs(pairs):
    result = {}
    for key, value in pairs:
        need(key not in result, 'DUPLICATE_JSON_KEY')
        result[key] = value
    return result

def decode_json(data):
    return json.loads(data.decode('utf-8-sig'), object_pairs_hook=json_pairs,
        parse_constant=lambda _: (_ for _ in ()).throw(Invalid('NONFINITE_JSON')))

def safe_path(value):
    need(isinstance(value, str) and bool(value) and not any(ord(c) < 32 or ord(c) == 127 for c in value), 'INVALID_PATH')
    need('\\' not in value and ':' not in value, 'INVALID_PATH')
    p = PurePosixPath(value)
    need(not p.is_absolute() and p.as_posix() == value and all(x not in ('', '.', '..') for x in value.split('/')), 'INVALID_PATH')
    need(all(not x.endswith((' ', '.')) for x in p.parts), 'WINDOWS_PATH_ALIAS')
    need(all(re.fullmatch(r'(?i)(CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])(?:\..*)?', x) is None for x in p.parts), 'WINDOWS_DEVICE_PATH')
    return value

def domain(path):
    safe_path(path)
    return 'historical_git_archive' if path.startswith(PREFIX) else 'sha256_input'

def unique_rows(rows, key='path', max_rows=10000):
    need(isinstance(rows, list) and len(rows) <= max_rows, 'INVALID_ROW_COLLECTION')
    out = {}; folded = set()
    for row in rows:
        need(isinstance(row, dict), 'INVALID_ROW')
        p = safe_path(row.get(key)); fold = p.casefold()
        need(p not in out and fold not in folded, 'DUPLICATE_OR_CASE_ALIAS')
        out[p] = row; folded.add(fold)
    return out

def parse_git_tree(raw):
    need(0 < len(raw) <= MAX_META and raw.endswith(b'\0'), 'INVALID_GIT_TREE_BYTES')
    rows = []
    for record in raw[:-1].split(b'\0'):
        head, sep, name = record.partition(b'\t')
        need(bool(sep), 'INVALID_GIT_TREE_RECORD')
        m = re.fullmatch(rb'(100644|100755) blob ([0-9a-f]{40}) +([0-9]+)', head)
        need(m is not None, 'NONREGULAR_GIT_INPUT')
        path = safe_path(name.decode('utf-8'))
        rows.append(dict(path=path, mode=m[1].decode(), type='blob', git_blob_oid_sha1=m[2].decode(),
            git_blob_bytes=integer(int(m[3]), MAX_FILE), domain=domain(path)))
    unique_rows(rows, max_rows=MAX_SOURCE_PATHS)
    return rows

def parse_paths(raw):
    need(0 < len(raw) <= MAX_META and raw.endswith(b'\0'), 'INVALID_TRACKED_PATH_BYTES')
    paths = [safe_path(x.decode('utf-8')) for x in raw[:-1].split(b'\0')]
    need(len(paths) <= MAX_SOURCE_PATHS and len(set(paths)) == len(paths) and len({x.casefold() for x in paths}) == len(paths), 'INVALID_TRACKED_PATH_SET')
    return set(paths)

def compare_checkout(blob, row):
    integer(row.get('bytes'), MAX_FILE); hexstr(row.get('sha256'), 64)
    if len(blob) == row['bytes'] and digest(blob) == row['sha256']:
        return 'EXACT_GIT_BLOB'
    need(b'\r' not in blob and b'\0' not in blob, 'UNSUPPORTED_CHECKOUT_TRANSFORM')
    crlf = blob.replace(b'\n', b'\r\n')
    need(len(crlf) == row['bytes'] and digest(crlf) == row['sha256'], 'CHECKOUT_SHA_MISMATCH')
    return 'EXACT_FULL_LF_TO_CRLF'

def validate_domains(source, tree_oid, expected_rows, before, after):
    """Pure semantic comparison; never reads historical payloads."""
    need(before == after, 'BEFORE_AFTER_INPUTS_CHANGED')
    hashed, tree, summary = before
    need(isinstance(tree, dict) and tree.get('schema') == TREE_SCHEMA and tree.get('source_sha') == source
         and tree.get('root_tree_oid_sha1') == tree_oid, 'GIT_MANIFEST_IDENTITY')
    actual = unique_rows(tree.get('files'), max_rows=MAX_SOURCE_PATHS); expected = unique_rows(expected_rows, max_rows=MAX_SOURCE_PATHS)
    need(actual == expected, 'FULL_GIT_METADATA_MISMATCH')
    need(isinstance(summary, dict) and summary.get('schema') == SCHEMA and summary.get('source_sha') == source
         and summary.get('root_tree_oid_sha1') == tree_oid, 'DOMAIN_MANIFEST_IDENTITY')
    need(summary.get('limits') == dict(paths=MAX_SOURCE_PATHS, per_file_bytes=MAX_FILE,
        sha256_input_total_bytes=MAX_HASHED, metadata_output_bytes=MAX_META), 'SOURCE_LIMITS_CHANGED')
    need(isinstance(summary.get('explicit_build_reference_policy'), str)
         and 'not hermetic' in summary['explicit_build_reference_policy'], 'HERMETICITY_CLAIM_CHANGED')
    s = {p for p in expected if domain(p) == 'sha256_input'}; h = set(expected)-s
    sha_rows = unique_rows(hashed, max_rows=MAX_SOURCE_PATHS)
    need(set(sha_rows) == s, 'SHA_SUBSET_NOT_COMPLETE_PARTITION')
    total = 0
    for row in sha_rows.values():
        need(set(row) == {'path', 'bytes', 'sha256'}, 'UNEXPECTED_SHA_FIELDS')
        total += integer(row.get('bytes'), MAX_FILE); hexstr(row.get('sha256'), 64)
    need(total <= MAX_HASHED, 'SHA_TOTAL_OVER_256MIB')
    hist = summary.get('historical_git_archive'); sh = summary.get('sha256_input')
    need(isinstance(hist, dict) and isinstance(sh, dict), 'MISSING_DOMAIN_DETAIL')
    hist_rows = unique_rows(hist.get('checkout_metadata'), max_rows=MAX_SOURCE_PATHS)
    need(set(hist_rows) == h, 'HISTORY_NOT_COMPLETE_PARTITION')
    hist_size = 0
    for row in hist_rows.values():
        need(set(row) == {'path', 'checkout_bytes'}, 'HISTORY_SHA_OR_UNEXPECTED_FIELDS')
        hist_size += integer(row.get('checkout_bytes'), MAX_FILE)
    git_size = sum(expected[p]['git_blob_bytes'] for p in h)
    need(summary.get('tracked_file_count') == len(expected) and type(summary['tracked_file_count']) is int, 'TRACKED_COUNT')
    need(sh.get('manifest') == 'input-sha256.json' and sh.get('file_count') == len(s)
         and type(sh['file_count']) is int and sh.get('checkout_bytes_hashed') == total
         and type(sh['checkout_bytes_hashed']) is int, 'SHA_SUMMARY_COUNTERS')
    need(hist.get('exact_prefix') == PREFIX and hist.get('git_manifest') == 'input-git-tree.json'
         and hist.get('content_sha256') == 'NOT_READ_BY_INPUT_HASHER' and hist.get('file_count') == len(h)
         and type(hist['file_count']) is int and hist.get('git_blob_bytes') == git_size
         and type(hist['git_blob_bytes']) is int and hist.get('checkout_metadata_bytes') == hist_size
         and type(hist['checkout_metadata_bytes']) is int, 'HISTORICAL_METADATA_HONESTY')
    return dict(fullGitMetadata=len(expected), actualShaSubset=len(s), historicalMetadataOnly=len(h),
        actualCheckoutBytesHashed=total, historicalRawGitBlobBytes=git_size,
        historicalCheckoutMetadataBytes=hist_size), sha_rows

def cargo_arguments(command, args, baseline):
    if baseline != 'Win7' or command in ('metadata', 'fmt'):
        return [command] + args
    need('--target' in args and args[args.index('--target')+1] == BASELINES['Win7']['target'], 'WIN7_TARGET')
    result = ([command, '-Z', 'build-std'] if command == 'clippy' else ['-Z', 'build-std', command]) + args
    return result

def product_rustflags(baseline):
    return ('-C target-feature=+crt-static -C link-arg=/SUBSYSTEM:WINDOWS,6.01'
        if baseline == 'Win7' else BASELINES[baseline]['flags'])

def compiler_flag_phases(baseline):
    default = BASELINES[baseline]['flags']
    return {'before-vs':default, 'after-vs':default, 'before-cargo':default,
        'before-product':product_rustflags(baseline), 'after-product':default}

def required_commands(baseline):
    target = BASELINES[baseline]['target']
    result = {
      'source-sha': ['rev-parse', 'HEAD'], 'source-sha-after': ['rev-parse', 'HEAD'],
      'source-clean-before': ['status', '--porcelain', '--untracked-files=all'],
      'source-clean-after': ['status', '--porcelain', '--untracked-files=all'],
      'source-input-paths': ['ls-files', '-z'], 'source-input-paths-after': ['ls-files', '-z'],
      'source-git-tree-before': ['ls-tree','-r','-z','-l','--full-tree','HEAD'],
      'source-git-tree-after': ['ls-tree','-r','-z','-l','--full-tree','HEAD'],
      'source-git-tree-id-before': ['rev-parse','HEAD^{tree}'], 'source-git-tree-id-after': ['rev-parse','HEAD^{tree}'],
      'cargo-metadata': cargo_arguments('metadata', ['--locked','--format-version','1','--no-deps'], baseline),
      'fmt': cargo_arguments('fmt', ['--all','--','--check'], baseline),
      'architecture': ['scripts/check-architecture.py'],
      'workspace-test-compile': cargo_arguments('test', ['-vv','--workspace','--all-targets','--release','--locked','--target',target,'--no-run','--message-format=json'], baseline),
      'product-compile': cargo_arguments('rustc', ['-vv','--release','--locked','--target',target,'-p','fc-bootstrap','--bin','fc-bootstrap','--message-format=json'], baseline),
      'lifecycle-compile': cargo_arguments('rustc', ['-vv','--release','--locked','--target',target,'-p','fc-bootstrap','--bin','fc-installer-lifecycle','--message-format=json'], baseline),
      'clippy': cargo_arguments('clippy', ['-vv','--workspace','--all-targets','--release','--locked','--target',target,'--','-D','warnings'], baseline),
    }
    install = ['toolchain','install',BASELINES[baseline]['toolchain'],'--profile','minimal','--component','clippy','--component','rustfmt']
    install += ['--component','rust-src'] if baseline == 'Win7' else ['--target',target]
    result['install-fixed-rust'] = install
    result['rustup-version'] = ['--version']; result['cargo-version'] = ['--version','--verbose']
    result['rust-version'] = ['run',BASELINES[baseline]['toolchain'],'rustc','-vV']
    result['rust-target-cfg'] = ['run',BASELINES[baseline]['toolchain'],'rustc','--target',target,'-C','target-feature=+crt-static','--print','cfg']
    result['visual-studio'] = ['-latest','-version','[17.0,18.0)','-products','*','-requires','Microsoft.VisualStudio.Component.VC.Tools.x86.x64','-format','json']
    if baseline == 'Win7':
        result['rust-src-components'] = ['component','list','--installed','--toolchain',BASELINES[baseline]['toolchain']]
        result['rust-sysroot'] = ['run',BASELINES[baseline]['toolchain'],'rustc','--print','sysroot']
    return result

def terminal_pass(result, conclusion):
    return result.get('status') == 'COMPILE_CHECKS_PASSED_RUNTIME_NOT_RUN' and conclusion == 'success'

def expected_subsystem(role):
    need(role in ('product', 'tests'), 'INVALID_PE_ROLE')
    return 2 if role == 'product' else 3

def producer_subsystem_matches(row, baseline):
    return (row.get('role') in ('product', 'tests')
        and type(row.get('pe_subsystem')) is int
        and row['pe_subsystem'] == expected_subsystem(row['role'])
        and row.get('pe_subsystem_version') == ('6.1' if baseline == 'Win7' else None))

def pe_header(data, baseline, role):
    need(len(data) >= 256 and data[:2] == b'MZ', 'PE_TRUNCATED_OR_BAD_DOS')
    offset = struct.unpack_from('<I', data, 0x3c)[0]
    need(offset <= len(data)-24 and data[offset:offset+4] == b'PE\0\0', 'PE_SIGNATURE')
    machine, sections = struct.unpack_from('<HH', data, offset+4)
    optlen = struct.unpack_from('<H', data, offset+20)[0]; opt = offset+24
    need(machine == 0x14c and sections > 0 and optlen >= 96 and optlen <= len(data)-opt, 'PE_X86_OPTIONAL_BOUNDS')
    subsystem = expected_subsystem(role)
    need(struct.unpack_from('<H', data, opt)[0] == 0x10b, 'PE32_MAGIC')
    need(struct.unpack_from('<H', data, opt+68)[0] == subsystem, 'PE_ROLE_SUBSYSTEM')
    version = list(struct.unpack_from('<HH', data, opt+48))
    need(baseline != 'Win7' or version == [6,1], 'WIN7_SUBSYSTEM_NOT_6_1')
    need(opt+optlen+40*sections <= len(data), 'PE_SECTION_TABLE_BOUNDS')
    return dict(machine='0x014c', optionalMagic='0x010b', subsystem=subsystem, subsystemVersion=version)

def unwrap_metadata(value):
    if isinstance(value, dict) and 'structuredContent' in value:
        need(value.get('isError') is not True, 'METADATA_CONNECTOR_ERROR')
        value = value['structuredContent']
    if isinstance(value, dict) and isinstance(value.get('content'), str):
        value = decode_json(value['content'].encode('utf-8'))
    return value

def output_path(path):
    path = Path(path).absolute()
    need(path.is_relative_to(EV) and path.parent.resolve().is_relative_to(EV), 'OUTPUT_OUTSIDE_OWNED_EVIDENCE')
    need(not path.exists() and not path.is_symlink(), 'REFUSE_EXISTING_OUTPUT')
    return path

def zip_inventory(infos):
    need(0 < len(infos) <= 10000, 'ZIP_MEMBER_COUNT')
    names=[]; folded=set(); total=0
    for item in infos:
        name=safe_path(item.filename);need(not item.is_dir(),'UNEXPECTED_ZIP_DIRECTORY')
        need(name.casefold() not in folded,'ZIP_DUPLICATE_OR_CASE_ALIAS');folded.add(name.casefold());names.append(name)
        mode=item.external_attr>>16
        need(not stat.S_ISLNK(mode) and stat.S_IFMT(mode) in (0,stat.S_IFREG),'ZIP_NONREGULAR_MEMBER')
        need(not item.flag_bits&1,'ZIP_ENCRYPTED_MEMBER')
        integer(item.file_size,128*1024*1024 if name.lower().endswith('.exe') else MAX_META)
        total+=item.file_size
    need(total<=1024*1024*1024,'ZIP_TOTAL_BOUND')
    return names

class Audit:
    def __init__(self, args):
        self.args=args; self.report_path=output_path(args.report); self.out=output_path(args.output_dir)
        self.out.mkdir(); self.raw=self.out/'raw';self.raw.mkdir();self.programs=self.out/'programs';self.programs.mkdir()
        self.report=dict(schema='future-classroom.ci-independent-artifact/v2', status='RUNNING',
            recordedUtc=datetime.datetime.now(datetime.timezone.utc).isoformat(), sourceSha=args.source,
            baseline=args.baseline,runId=args.run_id,jobId=args.job_id,artifactId=args.artifact_id,
            runtime='NOT_RUN',scope='Read-only original CI artifact audit. Git reads and static PE inspection only; no network, executable, Windows, Cargo, dispatch or Git mutation.',
            checks=[],inputs=[],members=[],executables=[],sourceMatches=[],limitations=[
              'Full Git metadata M is not full historical checkout SHA256. Actual checkout SHA256 covers only nonhistorical subset S; H historical payloads are NOT_READ_BY_INPUT_HASHER.',
              'Git clean/reparse guards and compiler/source observations are source-bound original CI evidence, not a hermetic sandbox or independently reobserved Windows filesystem.',
              'Toolchain/rust-src/SDK hashes are retained provenance and static evidence, not target-OS runtime certification or complete installed-toolchain certification.',
              'All Windows7/10/11, Seewo, production listener and strict firstTCP deadline acceptance remain separate. No executable is run by this helper.'])
        self.report['helper']=file_fact(__file__);self.members={};self.ledger={}
    def check(self, name, ok):
        self.report['checks'].append(dict(name=name,status='PASS' if ok else 'FAIL'));need(ok,name)
    def record(self, path):
        fact=file_fact(path);self.report['inputs'].append(fact);return Path(path).read_bytes()
    def original(self, name):
        safe_path(name);need(name in self.members,'MISSING_ORIGINAL_'+name.replace('/','_'))
        return (self.raw/name).read_bytes()
    def json(self, name):
        return decode_json(self.original(name))
    def log(self, name, kind='stdout'):
        return self.original(f'logs/{name}.{kind}.log')
    def textlog(self, name, kind='stdout'):
        return self.log(name,kind).decode('utf-8-sig')
    def git(self, *arguments):
        return subprocess.run(['git',*arguments],cwd=self.args.repo,check=True,capture_output=True,timeout=120).stdout
    def official(self):
        a=self.args;hexstr(a.source,40);hexstr(a.zip_sha256,64)
        for n in [a.run_id,a.job_id,a.artifact_id,a.zip_bytes]:integer(n,2**63-1,minimum=1)
        run=unwrap_metadata(decode_json(self.record(a.run_metadata)))
        jobs=unwrap_metadata(decode_json(self.record(a.job_metadata)))
        artifacts=unwrap_metadata(decode_json(self.record(a.artifact_metadata)))
        if isinstance(jobs,dict) and 'jobs' in jobs:jobs=jobs['jobs']
        if isinstance(jobs,dict):jobs=[jobs]
        need(isinstance(jobs,list),'JOB_METADATA_SHAPE')
        selected=[j for j in jobs if j.get('id')==a.job_id];need(len(selected)==1,'EXACT_JOB_NOT_FOUND');job=selected[0]
        if isinstance(artifacts,dict) and 'artifacts' in artifacts:artifacts=artifacts['artifacts']
        if isinstance(artifacts,dict):artifacts=[artifacts]
        selected=[v for v in artifacts if v.get('id')==a.artifact_id];need(len(selected)==1,'EXACT_ARTIFACT_NOT_FOUND');art=selected[0]
        config=BASELINES[a.baseline]
        self.check('official exact run/source/completed/workflow',run.get('id')==a.run_id and run.get('head_sha')==a.source
            and run.get('status')=='completed' and run.get('event')=='workflow_dispatch' and run.get('path')==config['workflow'])
        self.check('official exact completed job belongs original run',job.get('run_id')==a.run_id and ('head_sha' not in job or job['head_sha']==a.source) and job.get('status')=='completed' and job.get('name')==('compile-i686' if a.baseline=='Modern' else 'compile-win7-i686'))
        self.check('official artifact exact identity/ZIP digest/bytes/source',art.get('size_in_bytes')==a.zip_bytes and art.get('digest')=='sha256:'+a.zip_sha256
            and art.get('expired') is False and art.get('workflow_run',{}).get('id')==a.run_id and art['workflow_run'].get('head_sha')==a.source)
        attempt=integer(run.get('run_attempt'),100000,minimum=1)
        self.check('official artifact original baseline/run/attempt name',art.get('name')==f"{config['prefix']}-i686-{a.source}-{a.run_id}-{attempt}")
        self.report['officialIdentity']=dict(sourceSha=a.source,runId=a.run_id,jobId=a.job_id,artifactId=a.artifact_id,
            runAttempt=attempt,runStatus=run['status'],runConclusion=run.get('conclusion'),jobConclusion=job.get('conclusion'),
            workflow=config['workflow'],artifactName=art['name'],jobSourceBinding='EXACT_JOB_ID_AND_RUN_ID_TO_SOURCE_BOUND_OFFICIAL_RUN',jobHeadShaProvided='head_sha' in job)
        self.run=run;self.job=job;self.attempt=attempt
    def zip_members(self):
        a=self.args;zfact=file_fact(a.zip)
        self.check('original whole ZIP exact external bytes/SHA',zfact['bytes']==a.zip_bytes and zfact['sha256']==a.zip_sha256)
        self.report['zip']=zfact;need(zfact['bytes']<=MAX_PAYLOAD,'ZIP_OVER_LOCAL_BOUND')
        with zipfile.ZipFile(a.zip) as z:
            infos=z.infolist();names=zip_inventory(infos)
            producer=decode_json(z.read('artifact-sha256.json'));self.ledger=unique_rows(producer)
            self.check('dynamic ZIP exact complete producer ledger membership',set(names)==set(self.ledger)|{'artifact-sha256.json'})
            for row in self.ledger.values():integer(row.get('bytes'),128*1024*1024);hexstr(row.get('sha256'),64)
            for item in infos:
                data=z.read(item);name=item.filename
                need(len(data)==item.file_size and zlib.crc32(data)&0xffffffff==item.CRC,'ZIP_CRC_OR_SIZE')
                fact=dict(path=name,bytes=len(data),sha256=digest(data),crc32=f'{item.CRC:08x}')
                if name in self.ledger:
                    row=self.ledger[name];need(row['bytes']==len(data) and row['sha256']==fact['sha256'],'PRODUCER_LEDGER_MISMATCH')
                destination=(self.programs if name.lower().endswith('.exe') else self.raw)/name
                destination.parent.mkdir(parents=True,exist_ok=True)
                with destination.open('xb') as output:output.write(data)
                if not name.lower().endswith('.exe'):need(not data.startswith(b'MZ'),'UNLISTED_NATIVE_PAYLOAD')
                self.members[name]=fact;self.report['members'].append(fact)
        self.report['artifactIntegrity']='PASS_WHOLE_ZIP_CRC_AND_COMPLETE_PRODUCER_LEDGER'
        for name in ['result.json','run-start.json','cleanup.json']:
            self.check('mandatory original '+name,name in self.members)
        self.result=self.json('result.json')
        self.report['originalCiStatus']=self.result.get('status')
        self.report['originalCommands']=[dict(name=c.get('name'),exit_code=c.get('exit_code'),passed=c.get('passed'),
            parent_exited=c.get('parent_exited'),timed_out_or_pipe_stalled=c.get('timed_out_or_pipe_stalled')) for c in self.result.get('commands',[])]
        self.report['originalFailure']={k:self.json('failure.json').get(k) for k in ['utc']} if 'failure.json' in self.members else None
        # Do not copy an arbitrary failure message or command output into diagnostics.
    def sources(self):
        a=self.args;source=a.source
        tree_raw=self.git('ls-tree','-r','-z','-l','--full-tree',source);expected=parse_git_tree(tree_raw)
        tree_oid=self.git('rev-parse',source+'^{tree}').decode().strip();hexstr(tree_oid,40)
        self.report['pinnedGit']=dict(sourceSha=source,rootTreeOidSha1=tree_oid,metadataBytes=len(tree_raw),metadataSha256=digest(tree_raw))
        six=[]
        for suffix in ['', '-after']:
            triple=[]
            for stem in ['input-sha256','input-git-tree','input-domains']:
                data=self.original(stem+suffix+'.json');need(len(data)<=MAX_META,'METADATA_OVER_16MIB');triple.append(decode_json(data))
            six.append(triple)
        counts,hashed=validate_domains(source,tree_oid,expected,*six)
        self.report['counts']=counts
        self.check('full Git exact metadata and independent complete S/H partition before/after',True)
        for suffix in ['before','after']:
            command='source-git-tree-'+suffix
            observed=self.log(command);need(len(observed)<=MAX_META,'TREE_LOG_BOUND')
            self.check('original full Git tree stdout '+suffix,parse_git_tree(observed)==expected)
            self.check('original root tree stdout '+suffix,self.textlog('source-git-tree-id-'+suffix).strip()==tree_oid)
        for suffix in ['', '-after']:
            self.check('original tracked complete path set '+(suffix or 'before'),parse_paths(self.log('source-input-paths'+suffix))=={x['path'] for x in expected})
            self.check('original HEAD exact source '+(suffix or 'before'),self.textlog('source-sha'+suffix).strip()==source)
        for suffix in ['before','after']:
            self.check('original exact empty source-clean '+suffix,self.textlog('source-clean-'+suffix)=='')
        host=self.json('host.json');result=self.result
        for field,name in [('input_manifest_sha256','input-sha256.json'),('git_tree_manifest_sha256','input-git-tree.json'),('input_domains_manifest_sha256','input-domains.json')]:
            self.check('result/host exact before manifest raw digest '+name,result.get(field)==host.get(field)==self.members[name]['sha256'])
        self.check('new result schema and exact domain count summary',result.get('input_schema')==SCHEMA and result.get('input_domain_summary')==dict(
            manifest='input-domains.json',tracked_file_count=counts['fullGitMetadata'],sha256_file_count=counts['actualShaSubset'],
            sha256_checkout_bytes_hashed=counts['actualCheckoutBytesHashed'],historical_file_count=counts['historicalMetadataOnly'],
            historical_git_blob_bytes=counts['historicalRawGitBlobBytes'],historical_checkout_metadata_bytes=counts['historicalCheckoutMetadataBytes'],
            historical_content_sha256='NOT_READ_BY_INPUT_HASHER'))
        metadata=unique_rows(expected, max_rows=MAX_SOURCE_PATHS);direct=crlf=0
        # Only S blobs are read. Historical H content is never cat-file/read/hashed here.
        process=subprocess.Popen(['git','cat-file','--batch'],cwd=a.repo,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.DEVNULL)
        try:
            for path,row in hashed.items():
                oid=metadata[path]['git_blob_oid_sha1'];process.stdin.write((oid+'\n').encode());process.stdin.flush()
                header=process.stdout.readline().split();need(len(header)==3 and header[0].decode()==oid and header[1]==b'blob','GIT_BATCH_HEADER')
                size=int(header[2]);need(size==metadata[path]['git_blob_bytes'] and size<=MAX_FILE,'GIT_BATCH_SIZE')
                data=process.stdout.read(size);need(len(data)==size and process.stdout.read(1)==b'\n','GIT_BATCH_TRUNCATED')
                need(hashlib.sha1(b'blob '+str(size).encode()+b'\0'+data).hexdigest()==oid,'GIT_BLOB_OID')
                mode=compare_checkout(data,row);direct+=mode=='EXACT_GIT_BLOB';crlf+=mode=='EXACT_FULL_LF_TO_CRLF'
                basename=PurePosixPath(path).name.lower()
                if path.lower().endswith('.rs') or basename in ('cargo.toml','rust-toolchain.toml') or path.startswith('.cargo/'):
                    normalized=re.sub('/+','/',data.decode('utf-8-sig').replace('\\','/'))
                    need(PREFIX not in normalized,'EXPLICIT_HISTORICAL_BUILD_REFERENCE')
                self.report['sourceMatches'].append(dict(**row,gitBlobOidSha1=oid,gitBlobBytes=size,checkoutMatch=mode))
                if path=='scripts/windows-msvc-ci.ps1':self.ci_source=data
                if path=='scripts/audit-pe.py':self.pe_source=data
        finally:
            process.stdin.close();code=process.wait(timeout=30);need(code==0,'GIT_BATCH_EXIT')
        self.check('all SHA subset exact blob or exact whole LF->CRLF only',direct+crlf==counts['actualShaSubset'])
        counts.update(directGitBlobMatches=direct,exactFullLfToCrlfMatches=crlf)
        self.check('exact reviewed R73 dual-GUI CI helper pinned source',ci_source_matches(self.ci_source))
        source_text=self.ci_source.decode()
        for literal in ["$env:CARGO_BUILD_JOBS = '2'",'[int]$TimeoutSeconds = 120',"$Baseline) 2400", "$Baseline) 1200", "$Baseline) 1500",'Get-FileHash -LiteralPath $path -Algorithm SHA256','if ($total -gt 256MB)']:
            self.check('unchanged source contract '+literal,literal in source_text)
        self.check('original per-file/reparse/parent/metadata guards source-bound',all(x in source_text for x in ['$file.Length -gt 64MB','Source parent is a reparse point.','metadata_output_bytes = $LogLimit',"$LogLimit = 16MB"]))
        self.report['ciHelperSourceSha256']=digest(self.ci_source)
    def commands_and_baseline(self):
        a=self.args;c=BASELINES[a.baseline];r=self.result;start=self.json('run-start.json');bootstrap=self.json('bootstrap.json');host=self.json('host.json')
        for obj in [r,start,bootstrap]:
            self.check('source/dispatch/workflow exact original identity '+('result' if obj is r else 'start' if obj is start else 'bootstrap'),
                obj.get('source_sha',obj.get('expected_source_sha'))==a.source and obj.get('dispatch_source_sha')==a.source
                and (obj is r or obj.get('workflow_sha')==a.source))
        self.check('bootstrap actual run/attempt/repository',str(bootstrap.get('run_id'))==str(a.run_id) and str(bootstrap.get('run_attempt'))==str(self.attempt)
            and bootstrap.get('repository')=='NevermoreN/future-classroom' and bootstrap.get('windows_runtime_tests')=='NOT_RUN')
        for obj in [r,start]:
            self.check('fixed baseline target/toolchain '+('result' if obj is r else 'start'),all(obj.get(k)==c[k] for k in ['toolchain','target']) and obj.get('baseline')==a.baseline
                and all(obj.get(k)=='NOT_RUN' for k in ['windows7','windows10','windows11','seewo_device']))
        self.check('start fixed flags/role/no runtime',start.get('rustflags')==c['flags'] and start.get('role')==c['role'] and start.get('tests')=='COMPILE_ONLY_NO_RUN')
        for obj in [start,r]:
            self.check('actual default versus product-only environment metadata '+('start' if obj is start else 'result'),
                obj.get('rustflags')==c['flags'] and obj.get('product_rustflags')==product_rustflags(a.baseline)
                and obj.get('rustflags_scope')=='Workspace test/clippy default; product-compile uses product_rustflags only and restores the original flags.')
        self.check('actual host metadata PS>=7.2, compiler-host, hosted',host.get('source_sha')==a.source and host.get('compiler_host')=='x86_64-pc-windows-msvc'
            and host.get('runner_environment')=='github-hosted' and tuple(int(x) for x in host.get('powershell','0.0').split('.')[:2]) >= (7,2))
        commands=r.get('commands');need(isinstance(commands,list) and 0<len(commands)<=512,'COMMAND_LIST_BOUND')
        names=[v.get('name') for v in commands];need(all(isinstance(x,str) and re.fullmatch('[a-z0-9-]+',x) for x in names) and len(set(names))==len(names),'COMMAND_NAMES')
        self.command_map={v['name']:v for v in commands}
        unhealthy=[]
        for step in commands:
            name=step['name']
            ok=step.get('passed') is True and step.get('exit_code')==0 and type(step['exit_code']) is int and step.get('parent_exited') is True and step.get('timed_out_or_pipe_stalled') is False and step.get('log_truncated') is False and step.get('log_limit_bytes_per_stream')==MAX_META and type(step['log_limit_bytes_per_stream']) is int
            self.report['checks'].append(dict(name='original command physically exited/passed '+name,status='PASS' if ok else 'FAIL'))
            if not ok:unhealthy.append(name)
            duration=step.get('duration_seconds');need(type(duration) in (int,float) and math.isfinite(duration) and duration>=0,'INVALID_COMMAND_DURATION')
            need(isinstance(step.get('executable'),str) and PureWindowsPath(step['executable']).is_absolute(),'COMMAND_IMAGE_NOT_ABSOLUTE')
            self.check('original per-command result bound '+name,self.json(f'logs/{name}.result.json')==step)
            for kind in ['stdout','stderr']:
                self.check('canonical original log and stream cap '+name+' '+kind,step.get(kind)==f'logs/{name}.{kind}.log' and self.members[step[kind]]['bytes']<=MAX_META and step.get(kind+'_sha256')==self.members[step[kind]]['sha256'])
        for name,expected in required_commands(a.baseline).items():
            self.check('required exact original command arguments '+name,name in self.command_map and self.command_map[name].get('arguments')==expected)
        for name,budget in [('workspace-test-compile',2400),('product-compile',1200),('lifecycle-compile',1200),('clippy',1500)]:
            self.check('actual unchanged original compile duration bound '+name,self.command_map[name]['duration_seconds']<=budget)
        self.report['ciCommandFailures']=unhealthy
        # Negative original statuses are recorded above, then all available provenance
        # and the independent cleanup/PE phases are still inspected. Never upgraded.

        self.check('original no-runtime result flags/later tests2',r.get('later_test_arguments')==['--test-threads=2'] and r.get('no_toolchain_payloads') is True and r.get('no_environment_dump') is True)
        self.check('original fixed rust compiler host',re.search(r'(?m)^host: x86_64-pc-windows-msvc\r?$',self.textlog('rust-version')) is not None)
        if a.baseline=='Modern':
            self.check('modern fixed rust release',re.search(r'(?m)^release: 1\.97\.1\r?$',self.textlog('rust-version')) is not None)
        else:
            version=self.textlog('rust-version')
            self.check('Win7 fixed rust commit/release',re.search(r'(?m)^commit-hash: 92ffebeceaa0a071066f233873c2438e73d70fe7\r?$',version) is not None
                and re.search(r'(?m)^release: 1\.101\.0-nightly\r?$',version) is not None)
            self.check('Win7 rust-src installed',re.search(r'(?m)^rust-src\b',self.textlog('rust-src-components')) is not None)
            provenance=self.json('rust-src-provenance.json');self.report['originalRustSrcProvenance']=provenance
            self.check('Win7 selected six rust-src actual provenance',provenance.get('toolchain')==c['toolchain'] and provenance.get('rustc_commit')=='92ffebeceaa0a071066f233873c2438e73d70fe7'
                and isinstance(provenance.get('files'),list) and len(provenance['files'])==6 and provenance.get('sysroot')==self.textlog('rust-sysroot').strip())
            suffixes=['Cargo.toml','Cargo.lock','core/src/lib.rs','alloc/src/lib.rs','std/src/lib.rs','test/src/lib.rs']
            for item,suffix in zip(provenance['files'],suffixes):
                self.check('Win7 selected source bounded exact sysroot '+suffix,PureWindowsPath(item['path'])==PureWindowsPath(provenance['sysroot'])/'lib/rustlib/src/rust/library'/suffix)
                integer(item.get('bytes'),MAX_META,minimum=1);hexstr(item.get('sha256'),64)
        self.check('crt-static target cfg exact line','target_feature="crt-static"' in self.textlog('rust-target-cfg').splitlines())
        for phase,expected_flags in compiler_flag_phases(a.baseline).items():
            obj=self.json(f'logs/{phase}-crt-flags.json');flags=obj.get('flags',{});rf=flags.get('RUSTFLAGS',{});ef=flags.get('CARGO_ENCODED_RUSTFLAGS',{})
            self.check('actual compiler flags exact/no encoded override '+phase,obj.get('phase')==phase and rf.get('present') is True and rf.get('value')==expected_flags and rf.get('length')==len(expected_flags) and ef.get('present') is False and ef.get('value') in (None,'') and ef.get('length') in (None,0))
        sdk=self.json('msvc-sdk.json');self.report['originalMsvcSdk']=sdk
        for key in ['cl','linker','static_vc_runtime','static_ucrt','sdk_kernel32']:
            fact=sdk.get(key,{});integer(fact.get('bytes'),128*1024*1024,minimum=1);hexstr(fact.get('sha256'),64)
            need(PureWindowsPath(fact.get('path','')).is_absolute(),'SDK_PATH_NOT_ABSOLUTE')
        self.check('MSVC/SDK static runtime named libraries',PureWindowsPath(sdk['static_vc_runtime']['path']).name.lower()=='libcmt.lib' and PureWindowsPath(sdk['static_ucrt']['path']).name.lower()=='libucrt.lib' and PureWindowsPath(sdk['sdk_kernel32']['path']).name.lower()=='kernel32.lib')
        cl=self.command_map.get('cl-version-probe',{});clargs=cl.get('arguments',[])
        self.check('real CL version probe /MT/x86 path',cl.get('executable')==sdk['cl']['path'] and all(v in clargs for v in ['/nologo','/Bv','/MT','/c']) and '\\x86\\cl.exe' in cl['executable'].lower())
        self.check('Win7 C macro probe versus Modern',all(v in clargs for v in ['/D_WIN32_WINNT=0x0601','/DWINVER=0x0601']) if a.baseline=='Win7' else not any(v.startswith('/D_WIN') for v in clargs))
        self.report.setdefault('counts',{})['commands']=len(commands)
        self.check('recorded original CI and all commands actual terminal success remain required',not unhealthy and terminal_pass(r,self.run.get('conclusion')) and self.job.get('conclusion')=='success')
    def pe_and_associations(self):
        rows=self.json('executables.json');by_path=unique_rows(rows,'relative_path',128)
        need(sum(r.get('role')=='product' for r in rows)==len(PRODUCT_COMMANDS) and {r.get('target') for r in rows if r.get('role')=='product'}==set(PRODUCT_COMMANDS.values()),'EXACT_CLOSED_DUAL_PRODUCT_TARGETS')
        for row in rows:
            need(type(row.get('cargo_profile_test')) is bool and row['cargo_profile_test'] is (row.get('role')=='tests'),'EXACT_PRODUCER_PROFILE_TEST_BOOL')
            if row.get('role')=='product':
                need(row.get('package')=='fc-bootstrap' and row.get('target') in PRODUCT_COMMANDS.values() and row.get('kinds')==['bin'],'NO_ARBITRARY_PRODUCT_TARGET')
        need(set(by_path)=={p for p in self.members if p.lower().endswith('.exe')} and self.result.get('executable_count')==len(rows),'EXECUTABLE_MEMBER_SET')
        payload=0
        for name,row in by_path.items():
            payload+=integer(row.get('bytes'),128*1024*1024,minimum=256);hexstr(row.get('sha256'),64)
            self.check('actual exact listed PE bytes '+name,self.members[name]['bytes']==row['bytes'] and self.members[name]['sha256']==row['sha256'] and row.get('executed') is False)
            self.check('producer PE32 header and namespace '+name,row.get('pe_machine')=='0x014c' and row.get('pe_optional_magic')=='0x010b'
                and row.get('role') in ['tests','product'] and re.fullmatch('[a-z0-9-]+',row.get('package','')) is not None
                and name==row['role']+'/'+row['package']+'/'+PurePosixPath(name).name)
            safe_path(row.get('package_working_directory'))
            self.check('producer exact role and baseline subsystem claim '+name,producer_subsystem_matches(row,self.args.baseline))
        need(payload<=MAX_PAYLOAD,'EXECUTABLE_TOTAL_OVER_512MIB')
        # Execute only a static inspection tool on PE bytes; never a compiled artifact.
        pinned=self.pe_source;local=Path(self.args.repo)/'scripts/audit-pe.py'
        self.check('trusted PE parser same pinned Git source',local.read_bytes()==pinned)
        parser_namespace={'__name__':'fc_pinned_static_pe_parser','__file__':str(local)}
        exec(compile(pinned,str(local),'exec'),parser_namespace)
        parse=parser_namespace['parse_objdump']
        version=subprocess.run([self.args.objdump,'--version'],check=True,capture_output=True,timeout=10).stdout
        self.report['peInspector']=dict(command=self.args.objdump,versionSha256=digest(version),version=version.decode().splitlines()[0],parserSourceSha256=digest(pinned))
        self.report['peInspectorRawVersion']=dict(bytes=len(version),sha256=digest(version))
        for number,row in enumerate(rows):
            name=row['relative_path'];path=self.programs/name;data=path.read_bytes();header=pe_header(data,self.args.baseline,row['role'])
            inspect=subprocess.run([self.args.objdump,'-p',str(path)],check=True,capture_output=True,timeout=60)
            parsed=parse(inspect.stdout.decode('utf-8'), 'gui' if row['role']=='product' else 'console')
            need(not parsed['structural_errors'] and bool(parsed['imports']),'PE_IMPORT_PARSE_FAILED')
            dlls=[v.lower() for v in parsed['imports']]
            forbidden=[v for v in dlls if v.startswith(('vcruntime','msvcp','api-ms-win-crt','libgcc_s_','libstdc++','libwinpthread'))
                or re.fullmatch(r'(?:msvcr(?:t|\d+)[a-z0-9_]*|ucrtbased?)\.dll',v)]
            self.check('actual independent PE32/static CRT '+name,not forbidden)
            command=self.command_map.get('pe-imports-'+str(number))
            self.check('original real dumpbin association '+name,command is not None and command.get('arguments',[None])[0]=='/DEPENDENTS'
                and len(command['arguments'])==2 and PureWindowsPath(command['arguments'][1]).parts[-len(PurePosixPath(name).parts):]==tuple(PurePosixPath(name).parts))
            rawname=self.out/f'pe-{number}.objdump.stdout.log'
            with rawname.open('xb') as output:output.write(inspect.stdout)
            self.report['executables'].append(dict(producer=row,localPath=str(path),peHeader=header,independentPe=parsed,rawInspection=file_fact(rawname)))
        self.check('dynamic exact original PE command set',set(k for k in self.command_map if k.startswith('pe-imports-'))=={'pe-imports-'+str(n) for n in range(len(rows))})
        self.check('dynamic exact original command set, not fixed historical count',set(self.command_map)==set(required_commands(self.args.baseline))|{'cl-version-probe'}|{'pe-imports-'+str(n) for n in range(len(rows))})
        metadata=decode_json(self.log('cargo-metadata'))
        package_rows=[v for v in metadata.get('packages',[]) if v.get('id') in metadata.get('workspace_members',[])]
        packages={v['id']:v for v in package_rows}
        need(len(packages)==len(package_rows)==12,'WORKSPACE_TWELVE_CRATES')
        root=PureWindowsPath(metadata['workspace_root']);target_directory=PureWindowsPath(metadata['target_directory'])
        need(root.is_absolute() and target_directory.is_absolute(),'ABSOLUTE_SOURCE_AND_TARGET_METADATA')
        root_manifest=tomllib.loads(self.git('show',self.args.source+':Cargo.toml').decode('utf-8-sig'))
        source_version=root_manifest['workspace']['package']['version']
        bootstrap_manifest=tomllib.loads(self.git('show',self.args.source+':crates/fc-bootstrap/Cargo.toml').decode('utf-8-sig'))
        need(bootstrap_manifest['package']['name']=='fc-bootstrap' and bootstrap_manifest['package']['autobins'] is False
            and bootstrap_manifest['package']['default-run']=='fc-bootstrap' and bootstrap_manifest['package']['version']=={'workspace':True}
            and {v['name'] for v in bootstrap_manifest['bin']}==set(PRODUCT_COMMANDS.values()) and len(bootstrap_manifest['bin'])==len(PRODUCT_COMMANDS),'SOURCE_CLOSED_BOOTSTRAP_BIN_MANIFEST')
        bootstrap=[v for v in package_rows if v['name']=='fc-bootstrap']
        need(len(bootstrap)==1 and bootstrap[0]['version']==source_version,'SOURCE_NORMAL_PACKAGE_VERSION')
        bootstrap=bootstrap[0]
        need({v['name'] for v in bootstrap['targets'] if v['kind']==['bin']}==set(PRODUCT_COMMANDS.values()),'METADATA_CLOSED_BOOTSTRAP_BINS')
        expected_tests={};targets={}
        for package in package_rows:
            for target in package['targets']:
                key=(package['id'],target['name'],tuple(target['kind']))
                need(key not in targets,'DUPLICATE_METADATA_TARGET');targets[key]=target
                need(type(target.get('test')) is bool,'TYPED_METADATA_TEST_TARGET')
                # The pinned command explicitly selects --all-targets; metadata.test
                # describes the default selection, not this override (examples too).
                need(tuple(target['kind']) in {('lib',),('bin',),('example',),('test',),('bench',),('custom-build',)},'CLOSED_ALL_TARGETS_METADATA_KINDS')
                if target['kind']!=['custom-build']:
                    need(not target.get('required-features'),'ALL_TARGETS_FEATURE_GATED_TARGET_NOT_SUPPORTED')
                    expected_tests[key]=target
        need(expected_tests,'NO_METADATA_TEST_TARGETS')
        observed={};test_seen=set();normal_seen={};normal_profiles={};tests=[];associations=[]
        for command in ['workspace-test-compile',*PRODUCT_COMMANDS]:
            for number,line in enumerate(self.textlog(command).splitlines(),1):
                if not line.startswith('{'):continue
                msg=decode_json(line.encode())
                if msg.get('reason')!='compiler-artifact' or not msg.get('executable') or msg.get('package_id') not in packages:continue
                test_profile=msg.get('profile',{}).get('test')
                role='tests' if test_profile is True else 'product' if command in PRODUCT_COMMANDS and msg.get('target',{}).get('kind')==['bin'] else None
                if role is None:continue
                package=packages[msg['package_id']];target=msg['target'];key=(package['id'],target['name'],tuple(target['kind']))
                need(key in targets and target['src_path']==targets[key]['src_path'] and target['crate_types']==targets[key]['crate_types'],'CARGO_TARGET_MATCHES_ORIGINAL_METADATA')
                if role=='tests':
                    need(command=='workspace-test-compile' and test_profile is True and key in expected_tests,'TEST_ONLY_FROM_NO_RUN_WORKSPACE_COMPILE')
                else:
                    need(test_profile is False and package['id']==bootstrap['id'] and package['name']=='fc-bootstrap'
                        and package['version']==source_version and target['name']==PRODUCT_COMMANDS[command] and target['kind']==['bin'],'CLOSED_NORMAL_COMMAND_PACKAGE_TARGET_VERSION_FALSE_PROFILE')
                original=PureWindowsPath(msg['executable']);path_key=str(original).casefold()
                need(original.is_absolute() and original.is_relative_to(target_directory) and str(original) in msg.get('filenames',[]),'CARGO_EXECUTABLE_AND_FILENAMES_OWNED_TARGET_ASSOCIATION')
                target_relative=original.relative_to(target_directory).parts
                need(target_relative[:2]==(BASELINES[self.args.baseline]['target'],'release'),'EXACT_TARGET_RELEASE_EXECUTABLE')
                if role=='product':
                    need(target_relative==(BASELINES[self.args.baseline]['target'],'release',target['name']+'.exe'),'EXACT_CLOSED_NORMAL_BASENAME')
                name=role+'/'+package['name']+'/'+original.name
                need(name in by_path,'COMPILER_ARTIFACT_NOT_LISTED');row=by_path[name]
                pkgpath=PureWindowsPath(package['manifest_path']).parent.relative_to(root).as_posix()
                self.check('actual Cargo executable kind/package/target/profile association '+name,row['role']==role and row['target']==target['name'] and row['kinds']==target['kind'] and row['package_working_directory']==pkgpath and row['cargo_profile_test'] is test_profile)
                signature=(name,command,key,json.dumps(msg['profile'],sort_keys=True))
                if path_key in observed:
                    need(observed[path_key]==signature,'CONFLICTING_DUPLICATE_CARGO_EXECUTABLE');continue
                need(name not in {value[0] for value in observed.values()},'ALIASED_CARGO_PRODUCER_DESTINATION')
                observed[path_key]=signature
                if role=='tests':
                    need(key not in test_seen,'DUPLICATE_TEST_CARGO_TARGET');test_seen.add(key)
                    value=copy.deepcopy(msg);value['executable']=str(self.programs/name);tests.append(value)
                else:
                    need(target['name'] not in normal_seen,'DUPLICATE_NORMAL_TARGET');normal_seen[target['name']]=name
                    normal_profiles[target['name']]=json.dumps(msg['profile'],sort_keys=True)
                associations.append(dict(command=command,cargoLine=number,producerPath=name,packageId=package['id'],packageVersion=package['version'],target=target['name'],profileTest=test_profile))
        need(len(set(normal_profiles.values()))==1,'SAME_CLOSED_NORMAL_RELEASE_PROFILE')
        expected_count=len(expected_tests)+len(PRODUCT_COMMANDS)
        self.check('metadata-derived complete test target and two exact normal inventory',test_seen==set(expected_tests) and set(normal_seen)==set(PRODUCT_COMMANDS.values()) and len(rows)==expected_count and len(tests)==len(expected_tests))
        self.check('complete compiler artifact-to-all-PE set',{v[0] for v in observed.values()}==set(by_path) and len(observed)==len(rows))
        with (self.out/'test-metadata.jsonl').open('x') as output:
            for record in tests:output.write(json.dumps(record,ensure_ascii=False)+'\n')
        self.report['testMetadata']=file_fact(self.out/'test-metadata.jsonl')
        self.report['counts'].update(executables=len(rows),tests=len(tests),product=len(PRODUCT_COMMANDS),metadataExpectedTests=len(expected_tests),payloadBytes=payload,zipMembers=len(self.members),producerLedgerRows=len(self.ledger),nonbinaryRawFiles=len(self.members)-len(rows))
        self.report['normalExecutables']={target:next(v for v in self.report['executables'] if v['producer']['relative_path']==path) for target,path in normal_seen.items()}
        self.report['normalExecutable']=self.report['normalExecutables']['fc-bootstrap']
        self.report['lifecycleExecutable']=self.report['normalExecutables']['fc-installer-lifecycle']
        self.report['normalPackageVersion']=source_version
        self.report['cargoExecutableAssociations']=associations
        self.report['testTargetInventory']=[dict(packageId=k[0],target=k[1],kinds=list(k[2])) for k in sorted(expected_tests)]
    def cleanup(self):
        cleanup=self.json('cleanup.json');roots=cleanup.get('roots');need(isinstance(roots,list),'CLEANUP_ROOTS')
        self.check('four original owned roots actually removed',cleanup.get('status')=='OWNED_BUILD_ROOTS_REMOVED' and len(roots)==4 and {v.get('root') for v in roots}=={'target','scratch','cargo','rustup'} and all(v.get('removed') is True for v in roots))
        steps=self.job.get('steps');need(isinstance(steps,list),'OFFICIAL_JOB_STEPS_MISSING')
        for required in ["Clean only this ephemeral run's build and tool roots",'Upload original logs, metadata and compiled test executables']:
            observed=[v for v in steps if v.get('name')==required]
            self.check('official exact cleanup/upload step '+required,len(observed)==1 and observed[0].get('status')=='completed' and observed[0].get('conclusion')=='success')
        self.report['originalCleanup']=cleanup
    def run(self):
        failures=[];self.report['phases']=[]
        for name,action in [('officialIdentity',self.official),('originalZip',self.zip_members),('sourceDomains',self.sources),('commandsAndBaseline',self.commands_and_baseline),('peAndAssociations',self.pe_and_associations),('originalCleanup',self.cleanup)]:
            # Cleanup is an independent observation, not skipped after CI/compiler failure.
            # Later phases may fail closed on a missing prerequisite; preserve each result.
            try:
                action();self.report['phases'].append(dict(name=name,status='PASS'))
            except Exception as error:
                fact=dict(name=name,status='FAIL',failureCode=str(error) if isinstance(error,Invalid) else 'UNEXPECTED_INPUT_OR_TOOL_FAILURE',errorKind=type(error).__name__)
                self.report['phases'].append(fact);failures.append(fact)
        self.report.update(status='FAIL' if failures else 'PASS_STATIC_ONLY_RUNTIME_NOT_RUN',phaseFailures=failures)
        if failures:self.report['failureCode']=failures[0]['failureCode']
        self.report['helperUnchanged']=file_fact(__file__)==self.report['helper']
        need(self.report['helperUnchanged'],'HELPER_CHANGED_DURING_AUDIT')
        with self.report_path.open('x') as output:json.dump(self.report,output,ensure_ascii=False,indent=2);output.write('\n')
        print(json.dumps(dict(status=self.report['status'],report=file_fact(self.report_path),counts=self.report.get('counts'),failureCode=self.report.get('failureCode')),ensure_ascii=False))
        return 0 if self.report['status']=='PASS_STATIC_ONLY_RUNTIME_NOT_RUN' else 1

def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--report',type=Path,required=True)
    p.add_argument('--source');p.add_argument('--baseline',choices=BASELINES);p.add_argument('--zip',type=Path)
    p.add_argument('--zip-bytes',type=int);p.add_argument('--zip-sha256');p.add_argument('--run-id',type=int);p.add_argument('--job-id',type=int);p.add_argument('--artifact-id',type=int)
    for name in ['run-metadata','job-metadata','artifact-metadata','output-dir']:p.add_argument('--'+name,type=Path)
    p.add_argument('--repo',type=Path,default=ROOT);p.add_argument('--objdump',default='i686-w64-mingw32-objdump')
    a=p.parse_args()
    for name in ['source','baseline','zip','zip_bytes','zip_sha256','run_id','job_id','artifact_id','run_metadata','job_metadata','artifact_metadata','output_dir']:
        if getattr(a,name) is None:p.error('--'+name.replace('_','-')+' is required for artifact mode')
    return Audit(a).run()

if __name__=='__main__':
    try:sys.exit(main())
    except Exception as error:
        print(json.dumps(dict(status='FAIL',errorKind=type(error).__name__,failureCode=str(error) if isinstance(error,Invalid) else 'VALIDATOR_SETUP_OR_INPUT_FAILURE')))
        sys.exit(1)
