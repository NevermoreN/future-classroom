"""Pure ABI2 DEV authoring/PE-boundary rejections; no compiler or MSI execution."""
from pathlib import Path
import runpy
import struct
import unittest
import tempfile
import json
import hashlib
import xml.etree.ElementTree as ET
HERE = Path(__file__).resolve().parent
D = runpy.run_path(str(HERE/'package-abi2-controlled-dev.py'))
F = runpy.run_path(str(HERE/'test-installer-carrier-format.py'))

class Abi2DevTests(unittest.TestCase):
    def test_authoring_adds_exact_third_binary_without_ignored_package_attribute(self):
        raw = D['authoring']('0.1.0', sorted(D['B']['LIFECYCLE_REQUIRED']))
        root = ET.fromstring(raw); ns = '{'+D['B']['WIX_NS']+'}'; p=root.find(ns+'Product')
        self.assertIsNone(p.find(ns+'Package').get('Id'))
        rows={x.get('Id'):x.get('SourceFile')for x in p.findall(ns+'Binary')}
        self.assertEqual(set(rows),set(D['BINARY_NAMES']))
        self.assertEqual(rows['FcPayloadManifest'],'lifecycle/payload-manifest.bin')
        self.assertFalse(p.findall(ns+'MsiEmbeddedChainer'))
    def test_payload_envelope_rejects_bad_or_zero_actual_package(self):
        body=D['B']['lifecycle_manifest']('0.1.0',{name:name.encode() for name in D['B']['LIFECYCLE_REQUIRED']})
        for value in ('', 'x', '00000000-0000-0000-0000-000000000000'):
            with self.assertRaises(ValueError):D['C']['make_payload_manifest'](value,body)
    def test_msi_versions_get_distinct_product_identity_without_version_in_carrier(self):
        pe=F['structural_pe'](); carrier=D['C']['make_carrier_template'](pe)
        ids=[]
        for version in ('0.2.0','0.3.0'):
            xml=ET.fromstring(D['authoring'](version,sorted(D['B']['LIFECYCLE_REQUIRED'])))
            product=xml.find('{'+D['B']['WIX_NS']+'}Product')
            self.assertEqual(product.get('Version'),version);ids.append(product.get('Id'))
            self.assertEqual(D['C']['make_carrier_template'](pe),carrier)
        self.assertNotEqual(ids[0],ids[1])
        for version in ('0.3','00.3.0','256.0.0','0.1.65536'):
            with self.assertRaises(ValueError):D['authoring'](version,[])
    def test_maintenance_source_requires_actual_original_freeze_and_current_bytes(self):
        with tempfile.TemporaryDirectory(prefix='fc-abi2-pure-') as directory:
            root=Path(directory); source=root/'source';source.mkdir();evidence=root/'evidence.json'
            rel='crates/fc-windows/source.rs';p=source/rel;p.parent.mkdir(parents=True);p.write_bytes(b'owned frozen source')
            def fact(p):return {'bytes':p.stat().st_size,'sha256':hashlib.sha256(p.read_bytes()).hexdigest()}
            f=root/'freeze.json';f.write_text(json.dumps({'source':[dict(path='/original/'+rel,relativePath=rel,**fact(p))]}))
            mode='ABI2_SAME_PACKAGE_MAINTENANCE_NORMAL_GNU_DEV'
            binding={'status':'FROZEN_MAINTENANCE_SOURCE_OVERLAY_BOUND','sourceRoot':str(source),'lifecycleMode':mode,'originalFreeze':dict(path=str(f),**fact(f)),'sources':[dict(path=rel,**fact(p))]}
            receipt=root/'maintenance-source.json';receipt.write_text(json.dumps(binding));data={'lifecycle_mode':mode,'maintenance_source':dict(path=receipt.name,**fact(receipt))}
            self.assertEqual(D['maintenance_source_binding'](source,evidence,data)['sourceCount'],1)
            bad=dict(data,lifecycle_mode='FRESH_ONLY')
            with self.assertRaises(ValueError):D['maintenance_source_binding'](source,evidence,bad)
            p.write_bytes(b'changed source')
            with self.assertRaises(ValueError):D['maintenance_source_binding'](source,evidence,data)
            p.write_bytes(b'owned frozen source');f.write_text(f.read_text()+' ')
            with self.assertRaises(ValueError):D['maintenance_source_binding'](source,evidence,data)
    def test_normal_extent_rejects_symbols_overlay_and_malformed_sections(self):
        pe=F['structural_pe']();self.assertTrue(D['extent'](pe)['parsedExtentEqualsFileLength'])
        for raw in (pe+b'COFFTAIL',pe[:-1]):
            with self.assertRaises(ValueError):D['extent'](raw)
        raw=bytearray(pe);struct.pack_into('<II',raw,140,1024,1)
        with self.assertRaises(ValueError):D['extent'](bytes(raw))
        raw=bytearray(pe);struct.pack_into('<II',raw,392,512,1024)
        with self.assertRaises(ValueError):D['extent'](bytes(raw))
    def test_final_contract_requires_exact_v2_binary_before_shared_structural_checks(self):
        for rows in ([],['FcControlledGuardDll','FcGuardClientTemplate'],list(D['BINARY_NAMES'])+['Extra']):
            raw=('Name\tData\r\ns72\tv0\r\nBinary\tName\r\n'+''.join(x+'\tBinary.'+x+'\r\n'for x in rows)).encode()
            with self.assertRaises(ValueError):D['validate_contract'](b'',{'Binary':raw},'0.1.0',{},'12345678-1234-1234-1234-123456789ABC')



# These are synthetic metadata contract tests, never original CI provenance.
import copy
FORMAL = runpy.run_path(str(HERE/'package-abi2-msvc-candidate.py'))
TARGET = FORMAL['TARGET']

def formal_result_fixture():
    r=dict(status='COMPILE_CHECKS_PASSED_RUNTIME_NOT_RUN',baseline='Win7',target=TARGET,
        toolchain='nightly-2026-10-29-x86_64-pc-windows-msvc',
        rustflags='-C target-feature=+crt-static -C link-arg=/SUBSYSTEM:CONSOLE,6.01',
        product_rustflags='-C target-feature=+crt-static -C link-arg=/SUBSYSTEM:WINDOWS,6.01',commands=[])
    for name, package, target in [('product-compile','fc-bootstrap','fc-bootstrap'),('lifecycle-compile','fc-bootstrap','fc-installer-lifecycle'),('guard-library-compile','fc-windows',None)]:
        args=['-Z','build-std','build' if target is None else 'rustc','-vv','--release','--locked','--target',TARGET,'-p',package]
        args+=['--lib']if target is None else['--bin',target]
        args+=['--message-format=json']
        r['commands'].append(dict(name=name,arguments=args,passed=True,exit_code=0,parent_exited=True,timed_out_or_pipe_stalled=False,log_truncated=False))
    return r

class FormalMsvcAbi2Contract(unittest.TestCase):
    def test_closed_normal_command_matrix(self):
        FORMAL['check_compile_result'](formal_result_fixture())
        for index in range(3):
            for token in ('--tests','--features','--all-targets','--all-features','--no-default-features','--bin','test'):
                bad=formal_result_fixture();bad['commands'][index]['arguments'].append(token)
                with self.subTest(phase=index,token=token),self.assertRaises(ValueError):FORMAL['check_compile_result'](bad)
    def test_each_original_phase_required_once(self):
        for index in range(3):
            bad=formal_result_fixture();del bad['commands'][index]
            with self.assertRaises(ValueError):FORMAL['check_compile_result'](bad)
            bad=formal_result_fixture();bad['commands'].append(copy.deepcopy(bad['commands'][index]))
            with self.assertRaises(ValueError):FORMAL['check_compile_result'](bad)
    def test_original_parent_success_and_log_completeness(self):
        for index in range(3):
            for key,value in [('exit_code',False),('exit_code',1),('parent_exited',False),('timed_out_or_pipe_stalled',True),('log_truncated',True),('passed',False)]:
                bad=formal_result_fixture();bad['commands'][index][key]=value
                with self.subTest(phase=index,key=key),self.assertRaises(ValueError):FORMAL['check_compile_result'](bad)
    def test_other_target_toolchain_or_crt_scope_cannot_be_relabelled(self):
        for key,value in [('target','i686-pc-windows-gnu'),('target','i686-pc-windows-msvc'),('baseline','Modern'),('toolchain','stable'),('rustflags','-C target-feature=-crt-static'),('product_rustflags','-C target-feature=+crt-static'),('status','DEV_FROZEN_NORMAL_BUILD_ONLY')]:
            bad=formal_result_fixture();bad[key]=value
            with self.subTest(key=key,value=value),self.assertRaises(ValueError):FORMAL['check_compile_result'](bad)
    def test_other_ci_command_failure_not_hidden_by_normal_phases(self):
        bad=formal_result_fixture();bad['commands'].append(dict(name='clippy',passed=False))
        with self.assertRaises(ValueError):FORMAL['check_compile_result'](bad)

    def test_instructions_default_source_and_explicit_builder_bytes_are_separate(self):
        with tempfile.TemporaryDirectory(prefix='fc-instructions-pure-') as directory:
            source=Path(directory)/'original'; path=source/'packaging/CONTROLLED-MSI-CANDIDATE.md'
            path.parent.mkdir(parents=True); path.write_bytes(b'original compiler-source document\n')
            shared=FORMAL['ABI2']['instructions_bytes']
            self.assertEqual(shared(source),path.read_bytes())
            reviewed='ABI2 three Binary streams; Win7/10 untested.\n'.encode()
            self.assertEqual(shared(source,reviewed),reviewed)
            self.assertEqual(path.read_bytes(),b'original compiler-source document\n')
            for invalid in (b'',bytearray(reviewed),b'\xff',bytes(65537)):
                with self.assertRaises((ValueError,UnicodeError)): shared(source,invalid)
    def test_reviewed_instructions_pin_exact_raw_bytes_and_canonical_name(self):
        from types import SimpleNamespace
        with tempfile.TemporaryDirectory(prefix='fc-instructions-pin-') as directory:
            root=Path(directory); path=root/'CONTROLLED-MSI-CANDIDATE.md'
            path.write_bytes(b'reviewed ABI2 instructions\n')
            args=SimpleNamespace(source_root=root/'untouched-ci-source',candidate_instructions=path,
                candidate_instructions_sha256=hashlib.sha256(path.read_bytes()).hexdigest())
            raw,fact=FORMAL['read_candidate_instructions'](args)
            self.assertEqual(raw,path.read_bytes());self.assertEqual(fact['sha256'],args.candidate_instructions_sha256)
            self.assertEqual(fact['scope'],'REVIEWED_BUILDER_DOCUMENT_NOT_ORIGINAL_CI_SOURCE')
            path.write_bytes(raw.replace(b'\n',b'\r\n'))
            with self.assertRaises(ValueError):FORMAL['read_candidate_instructions'](args)
            path.write_bytes(raw);args.candidate_instructions_sha256='0'*64
            with self.assertRaises(ValueError):FORMAL['read_candidate_instructions'](args)
            other=root/'other.md';other.write_bytes(raw);args.candidate_instructions=other
            args.candidate_instructions_sha256=hashlib.sha256(raw).hexdigest()
            with self.assertRaises(ValueError):FORMAL['read_candidate_instructions'](args)

if __name__=='__main__':unittest.main()
