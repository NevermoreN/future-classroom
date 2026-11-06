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
    def test_builder_rejects_old_compiled_protocol_instead_of_relabeling_it(self):
        with tempfile.TemporaryDirectory(prefix='fc-abi2-protocol-') as directory:
            root=Path(directory)
            p=root/'crates/fc-windows/src/installer_lifecycle/broker/carrier.rs'
            p.parent.mkdir(parents=True)
            for old in (b'pub(crate) const ABI: u16 = 2;', b'pub(crate) const GUARD_PROTOCOL: u16 = 0;'):
                p.write_bytes(old)
                with self.assertRaises(ValueError):D['require_source_protocol'](root)
            p.write_bytes(b'pub(crate) const GUARD_PROTOCOL: u16 = 3;')
            D['require_source_protocol'](root)

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
    def test_d14_complete_pwa_assembly_contract_and_legacy_replay(self):
        # Synthetic bytes test the actual shared assembly preconditions and codec.
        # They are never marked as CI evidence or executed as an installer.
        shared=FORMAL['ABI2']; builder=shared['B']; pe=F['structural_pe']()
        names=builder['LIFECYCLE_REQUIRED'] | {f'web/assets/f{i}.js' for i in range(5)}
        old={name:name.encode() for name in names};old['fc-bootstrap.exe']=pe
        artifacts={role:(Path('/synthetic-not-a-build')/role,pe) for role in
                   ('product-compile','lifecycle-compile','guard-library-compile')}
        self.assertEqual(shared['require_admitted_artifacts']({'version':'0.1.4'},old,artifacts,True),14)
        current=dict(old, **{'fc-update-coordinator.exe':pe})
        current_artifacts=dict(artifacts, **{'update-coordinator-compile':(Path('/synthetic-not-a-build/coordinator'),pe)})
        self.assertEqual(shared['require_admitted_artifacts']({'version':'0.1.5'},current,current_artifacts,True),15)
        complete=dict(current, **{name:name.encode() for name in builder['PWA_PAYLOADS']})
        self.assertEqual(shared['require_admitted_artifacts']({'version':'0.1.5'},complete,current_artifacts,True),20)
        self.assertEqual(builder['payload_readback_label'](20),'ALL_TWENTY_EXACT')
        screenshots={f'web/assets/{name}-ABCDEFGH.png' for name in builder['TUTORIAL_SCREENSHOTS']}
        tutorial=set(complete) | screenshots
        self.assertEqual(builder['require_payload_members']('0.1.8',tutorial),31)
        self.assertEqual(builder['payload_readback_label'](31),'ALL_THIRTY_ONE_EXACT')
        social_codes={f'web/assets/{name}-ABCDEFGH.png' for name in builder['SOCIAL_QR_IMAGES']}
        self.assertEqual(builder['require_payload_members']('1.0.2',tutorial | social_codes),33)
        self.assertEqual(builder['payload_readback_label'](33),'ALL_THIRTY_THREE_EXACT')
        with self.assertRaises(ValueError):
            builder['require_payload_members']('1.0.2',tutorial | {next(iter(social_codes))})
        with self.assertRaises(ValueError):
            builder['require_payload_members']('1.0.1',tutorial | social_codes)
        with self.assertRaises(ValueError):
            builder['require_payload_members']('0.1.8',tutorial - {next(iter(screenshots))})
        with self.assertRaises(ValueError):
            builder['require_payload_members']('0.1.5',tutorial)
        for name in builder['PWA_PAYLOADS']:
            incomplete=dict(complete);del incomplete[name]
            with self.assertRaises(ValueError):
                shared['require_admitted_artifacts']({'version':'0.1.5'},incomplete,current_artifacts,True)
        invalid=dict(complete);invalid['web/sw.js?x']=invalid.pop('web/sw.js')
        with self.assertRaises(ValueError):
            shared['require_admitted_artifacts']({'version':'0.1.5'},invalid,current_artifacts,True)
        current=complete
        body=builder['lifecycle_manifest']('0.1.5',current)
        envelope=shared['C']['make_payload_manifest'](F['PACKAGE'],body)
        self.assertEqual(set(shared['C']['parse_payload_manifest'](envelope)['entries']),set(current))
        for changed,roles in [(old,current_artifacts),(current,artifacts),
                              (dict(current,**{'fc-update-coordinator.exe':b'different'}),current_artifacts)]:
            with self.assertRaises(ValueError):
                shared['require_admitted_artifacts']({'version':'0.1.5'},changed,roles,True)
        with self.assertRaises(ValueError):
            builder['require_payload_members']('0.1.4',current)

    def test_d14_requires_coordinator_without_rejecting_old_source_grammar(self):
        legacy = formal_result_fixture()
        FORMAL['check_compile_result'](legacy)
        with self.assertRaises(ValueError):
            FORMAL['check_compile_result'](legacy, require_coordinator=True)
        phase = copy.deepcopy(legacy['commands'][1])
        phase['name'] = 'update-coordinator-compile'
        phase['arguments'][phase['arguments'].index('--bin') + 1] = 'fc-update-coordinator'
        current = copy.deepcopy(legacy)
        current['commands'].append(phase)
        FORMAL['check_compile_result'](current, require_coordinator=True)
        phase['arguments'][phase['arguments'].index('--bin') + 1] = 'fc-installer-lifecycle'
        with self.assertRaises(ValueError):
            FORMAL['check_compile_result'](current, require_coordinator=True)
        self.assertEqual(len(FORMAL['B']['normal_product_targets']('0.1.4')), 2)
        self.assertEqual({n for n, _, _ in FORMAL['B']['normal_product_targets']('0.1.5')},
                         {'fc-bootstrap', 'fc-installer-lifecycle', 'fc-update-coordinator'})

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


class FormalSuccessorContract(unittest.TestCase):
    """Synthetic boundary checks only; never publication or native evidence."""
    def record(self, target=1):
        payloads={n:n.encode() for n in FORMAL['B']['LIFECYCLE_REQUIRED']}
        body=FORMAL['B']['lifecycle_manifest']('0.1.0',payloads)
        envelope=FORMAL['ABI2']['C']['make_payload_manifest'](F['PACKAGE'],body)
        meta=dict(target=target,source_commit='1'*40,template_bytes=1156)
        meta.update({n:'2'*64 for n in FORMAL['PUBLICATION']['DIGEST_KEYS']})
        return FORMAL['PUBLICATION']['encode_format'](meta,envelope,FORMAL['ABI2']['C']['parse_payload_manifest']),meta

    def test_default_has_no_predecessor_and_spec_arguments_are_paired(self):
        from types import SimpleNamespace
        self.assertIsNone(FORMAL['prepare_predecessor'](SimpleNamespace(),()))
        for a in (SimpleNamespace(predecessor_spec=Path('/not-read')),SimpleNamespace(predecessor_spec_sha256='a'*64)):
            with self.assertRaises(ValueError):FORMAL['prepare_predecessor'](a,())

    def test_record_roundtrip_rejects_gnu_reserved_tail_and_zero_digest(self):
        record,meta=self.record()
        self.assertEqual(FORMAL['decode_predecessor'](record),meta)
        mutations=[self.record(2)[0],record+b'x',bytearray(record)]
        for start,end in ((13,14),(236,237),(36,68)):
            raw=bytearray(record);raw[start:end]=b'\1'*(end-start) if end-start==1 else bytes(end-start);mutations.append(bytes(raw))
        for raw in mutations:
            with self.subTest(raw_type=type(raw)),self.assertRaises(ValueError):FORMAL['decode_predecessor'](raw)

    def test_fourth_stream_is_explicit_and_default_three_stays_closed(self):
        u=FORMAL['ABI2'];record,_=self.record();ns='{'+u['B']['WIX_NS']+'}'
        for blob,expected in ((None,set(u['BINARY_NAMES'])),(record,set(u['BINARY_NAMES'])|{u['PREDECESSOR_BINARY']})):
            xml=ET.fromstring(u['authoring']('0.2.0',[],blob));p=xml.find(ns+'Product')
            self.assertEqual({r.get('Id')for r in p.findall(ns+'Binary')},expected)
        def binary(names):return ('Name\tData\r\ns72\tv0\r\nBinary\tName\r\n'+''.join(n+'\tBinary.'+n+'\r\n'for n in names)).encode()
        for names,blob in ((list(u['BINARY_NAMES']),record),(list(u['BINARY_NAMES'])+[u['PREDECESSOR_BINARY']],None),(list(u['BINARY_NAMES'])+[u['PREDECESSOR_BINARY'],'Extra'],record)):
            with self.assertRaises(ValueError):u['validate_contract'](b'',{'Binary':binary(names)},'0.2.0',{},F['PACKAGE'],b'',blob)

    def test_successor_binds_each_aux_to_its_release_and_requires_newer_identity(self):
        b=FORMAL['B'];helper=F['structural_pe']();template=FORMAL['ABI2']['C']['make_carrier_template'](helper)
        dll=bytearray(helper);struct.pack_into('<H',dll,150,0x2000);dll=bytes(dll)
        new_helper=bytearray(helper);new_helper[-1]=7
        new_dll=bytearray(dll);new_dll[-1]=8
        package=dict(version='0.1.0',productCode=b['product_code']('0.1.0'),guardProtocol=3,
                     normalProvenance=dict(normal_helper_sha256=hashlib.sha256(helper).hexdigest()),
                     normalGuardProvenance=dict(dll_sha256=hashlib.sha256(dll).hexdigest()))
        artifacts={'lifecycle-compile':(None,bytes(new_helper)),'guard-library-compile':(None,bytes(new_dll))}
        streams={b['TEMPLATE_BINARY']:template,b['GUARD_BINARY']:dll}
        FORMAL['validate_successor_identity'](package,'0.2.0',streams,artifacts)
        for version,old in (('0.1.0',streams),('0.0.9',streams),('0.2.0',dict(streams,**{b['TEMPLATE_BINARY']:template+b'x'})),('0.2.0',dict(streams,**{b['GUARD_BINARY']:b'other DLL'}))):
            with self.assertRaises(ValueError):FORMAL['validate_successor_identity'](package,version,old,artifacts)
        with self.assertRaises(ValueError):FORMAL['validate_successor_identity'](dict(package,guardProtocol=0),'0.2.0',streams,artifacts)

    def test_cache_binding_requires_actual_api_product_user_full_hash_and_object(self):
        manifest=dict(productCode='{'+F['PACKAGE']+'}',expectedUserSid='S-1-5-21-1-2-3-1001')
        package=dict(msi=dict(sha256='a'*64,bytes=4096))
        cache=dict(status='KNOWN_PRESENT_EXACT_API_LOCALPACKAGE_READONLY',apiCode=0,context=2,property='LocalPackage',mutations=False,
                   productCode=manifest['productCode'],userSid=manifest['expectedUserSid'],sha256='a'*64,bytes=4096,
                   path='C:\\Windows\\Installer\\abc123.msi',volume=123,indexHigh=0,indexLow=456)
        FORMAL['validate_predecessor_cache'](cache,manifest,package)
        for key,value in [('apiCode',False),('context',True),('sha256','b'*64),('bytes',4095),('userSid','another'),('productCode','another'),('indexLow',None),('path','C:\\Temp\\abc123.msi')]:
            with self.subTest(key=key),self.assertRaises(ValueError):FORMAL['validate_predecessor_cache'](dict(cache,**{key:value}),manifest,package)

    def test_runtime_rejects_gnu_or_exit_only_without_touching_originals(self):
        for r in ({'status':'PASS_GNU_ABI2_SAME_PACKAGE_MAINTENANCE_WIN11_DEV_CONTROLLED_PRODUCT_CLOSURE_AND_EXACT_SCRATCH_CLEANUP'}, {'runtimeActualExit':0}, {}):
            with self.assertRaises(ValueError):FORMAL['validate_predecessor_runtime'](r,{}, {})

    def test_old_four_record_remains_bound_to_prior_publication_metadata(self):
        u=FORMAL['ABI2'];b=FORMAL['B'];record,meta=self.record()
        template=u['C']['make_carrier_template'](F['structural_pe']());dll=b'unchanged guard fixture'
        meta.update(template_sha=hashlib.sha256(template).hexdigest(),template_bytes=len(template),guard_sha=hashlib.sha256(dll).hexdigest())
        record=FORMAL['PUBLICATION']['encode_format'](meta,record[248:],u['C']['parse_payload_manifest'])
        fact=dict(bytes=len(record),sha256=hashlib.sha256(record).hexdigest())
        normal=dict(source_sha=meta['source_commit'],result_sha256=meta['producer_sha'],profile_test=False)
        guard=dict(normal,dll_sha256=meta['guard_sha'])
        package=dict(status='PASS_ORIGINAL_WIN7_MSVC_CI_ABI2_SUCCESSOR_STATIC_CANDIDATE_NATIVE_NOT_RUN',
                     binaryReadback='ALL_FOUR_EXACT',publishedPredecessor=fact,version='0.2.0',
                     productCode=b['product_code']('0.2.0'),packageCode='12345678-1234-1234-1234-123456789ABD',
                     predecessorPublication=dict(status='BOUND_ORIGINAL_MSVC_PREDECESSOR_STATIC_NOT_NATIVE_UPGRADE_PASS',
                                                meta=meta,record=fact,previousNormalProvenance=normal,previousNormalGuardProvenance=guard))
        streams={b['TEMPLATE_BINARY']:template,b['GUARD_BINARY']:dll}
        FORMAL['validate_old_publication'](package,record,streams)
        FORMAL['validate_old_publication'](package,record,dict(streams,**{b['GUARD_BINARY']:b'current old release differs from its predecessor'}))
        for mutation in ('missing-record','changed-record-fact','same-version','changed-provenance'):
            bad=copy.deepcopy(package);old=dict(streams);blob=record
            if mutation=='missing-record':blob=None
            elif mutation=='changed-record-fact':bad['publishedPredecessor']['sha256']='f'*64
            elif mutation=='same-version':bad['version']='0.1.0'
            else:bad['predecessorPublication']['previousNormalGuardProvenance']['dll_sha256']='f'*64
            with self.subTest(mutation=mutation),self.assertRaises(ValueError):FORMAL['validate_old_publication'](bad,blob,old)

    def test_shared_closed_streams_and_fixed_table_digest_include_old_fourth_bytes(self):
        u=FORMAL['ABI2'];record,_=self.record();base=dict(zip(u['BINARY_NAMES'],(b'DLL',b'template',b'envelope')))
        old=u['published_streams'](base,record)
        self.assertEqual(set(old),set(base)|{u['PREDECESSOR_BINARY']})
        self.assertEqual(u['published_streams'](base),base)
        for bad in (dict(base,Other=b'x'),old):
            with self.assertRaises(ValueError):u['published_streams'](bad,record)
        cols=[('Name','s72'),('Data','v0')]
        tables=[('Binary',cols,[[n,value]for n,value in old.items()])]
        first=FORMAL['PUBLICATION']['signature'](tables)[0]
        changed=[('Binary',cols,[[n,value+b'x'if n==u['PREDECESSOR_BINARY']else value]for n,value in old.items()])]
        self.assertNotEqual(FORMAL['PUBLICATION']['signature'](changed)[0],first)

if __name__=='__main__':unittest.main()
