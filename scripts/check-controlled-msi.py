#!/usr/bin/env python3
"""Pure controlled-authoring/provenance hostile vectors; no compiler/MSI runs."""
import copy
import json
from pathlib import Path
import runpy
import struct
import tempfile
from types import SimpleNamespace
import unittest
import xml.etree.ElementTree as ET

HERE = Path(__file__).absolute().parent
OLD = runpy.run_path(str(HERE / 'check-msi-candidate.py'))
M = OLD['M']; SHA = M['digest']; TABLE = OLD['export_table']
DEV = runpy.run_path(str(HERE / 'package-controlled-dev.py'))

def controlled_model():
    summary, tables, version, payloads = OLD['compiled_contract_model']()
    tables.pop('MsiEmbeddedChainer')
    tables['Property'] += b'MsiLogging\tvoicewarmup!\n'
    tables['Binary'] = TABLE('Binary', ['Name','Data'], [[n, 'Binary.'+n] for n in ('FcControlledGuardDll','FcGuardClientTemplate')])
    tables['CustomAction'] = TABLE('CustomAction', ['Action','Type','Source','Target'], [['FcControlledInnerGuard','1','FcControlledGuardDll','FcControlledInnerGuard']])
    tables['InstallExecuteSequence'] = tables['InstallExecuteSequence'].replace(b'FcBeginGate', b'FcControlledInnerGuard')
    return summary, tables, version, payloads

class Controlled(unittest.TestCase):
    def test_controlled_is_a_separate_closed_contract(self):
        values = controlled_model()
        self.assertEqual(M['verify_msi_contract'](*values, controlled=True)['status'], 'PASS')
        with self.assertRaises(ValueError):M['verify_msi_contract'](*values)
        with self.assertRaises(ValueError):M['verify_msi_contract'](*OLD['compiled_contract_model'](), controlled=True)
        tree = ET.fromstring(M['authoring']('0.1.0', ['LICENSE'], True));ns={'w':M['WIX_NS']}
        self.assertEqual({b.attrib['Id'] for b in tree.findall('w:Product/w:Binary',ns)}, {'FcControlledGuardDll','FcGuardClientTemplate'})
        self.assertEqual([p.attrib['Value'] for p in tree.findall('w:Product/w:Property',ns) if p.attrib['Id']=='MsiLogging'], ['voicewarmup!'])
        legacy=ET.fromstring(M['authoring']('0.1.0', ['LICENSE']))
        self.assertFalse([p for p in legacy.findall('w:Product/w:Property',ns) if p.attrib['Id']=='MsiLogging'])
    def test_hostile_mixed_bypass_async_wrong_target_and_successor_reject(self):
        original = controlled_model()
        mutations = [('CustomAction',b'\t1\t',b'\t65\t'),('CustomAction',b'FcControlledGuardDll',b'FcGuardClientTemplate'),
            ('CustomAction',b'\tFcControlledInnerGuard\n',b'\tOtherTarget\n'),
            ('InstallExecuteSequence',b'FcControlledInnerGuard\t1\t1499',b'FcControlledInnerGuard\t0\t1499'),
            ('InstallExecuteSequence',b'FcControlledInnerGuard\t1\t1499',b'FcControlledInnerGuard\tHOST=1\t1499'),
            ('InstallExecuteSequence',b'InstallInitialize\t\t1500',b'InstallInitialize\t\t1501'),
            ('Property',b'MsiLogging\tvoicewarmup!\n',b''),
            ('Property',b'MsiLogging\tvoicewarmup!\n',b'MsiLogging\tvoicewarmup+\n')]
        for table, old, new in mutations:
            with self.subTest(table=table, new=new):
                summary,tables,version,payloads=copy.deepcopy(original)
                self.assertIn(old,tables[table]);tables[table]=tables[table].replace(old,new)
                with self.assertRaises(ValueError):M['verify_msi_contract'](summary,tables,version,payloads,True)
        summary,tables,version,payloads=copy.deepcopy(original)
        tables['MsiEmbeddedChainer']=OLD['compiled_contract_model']()[1]['MsiEmbeddedChainer']
        with self.assertRaises(ValueError):M['verify_msi_contract'](summary,tables,version,payloads,True)
        tables.pop('MsiEmbeddedChainer');tables['Binary']+=b'FcLifecycleHelper\tBinary.FcLifecycleHelper\n'
        with self.assertRaises(ValueError):M['verify_msi_contract'](summary,tables,version,payloads,True)
    def test_guard_real_dll_role_and_undecorated_target(self):
        normal=OLD['normal_pe_fixture']()
        with self.assertRaises(ValueError):M['pe_dll_header'](normal)
        dll=bytearray(normal);struct.pack_into('<H',dll,150,0x2000)
        M['pe_dll_header'](bytes(dll))
        struct.pack_into('<H',dll,220,3);M['pe_dll_header'](bytes(dll))
        struct.pack_into('<H',dll,220,4)
        with self.assertRaises(ValueError):M['pe_dll_header'](bytes(dll))
        M['verify_guard_exports'](b' [ 0] FcControlledInnerGuard\n')
        for raw in [b'', b' [ 0] FcControlledInnerGuard@4\n',b' [ 0] Other\n']:
            with self.assertRaises(ValueError):M['verify_guard_exports'](raw)
    def test_guard_admission_binds_actual_library_cargo_metadata_and_copy(self):
        # Structural vector only; never represented as real compiler evidence.
        with tempfile.TemporaryDirectory(prefix='fc-controlled-admit-') as td:
            root=Path(td);(root/'logs').mkdir();p=root/'custom_action_library/fc-windows/fc_windows.dll';p.parent.mkdir(parents=True)
            dll=bytearray(OLD['normal_pe_fixture']());struct.pack_into('<H',dll,150,0x2000);p.write_bytes(dll)
            target={'name':'fc_windows','crate_types':['rlib','cdylib'],'kind':['rlib','cdylib'],'src_path':'C:/owned/crates/fc-windows/src/lib.rs'}
            package={'name':'fc-windows','version':'0.1.0','id':'opaque:fc-windows','source':None,'manifest_path':'C:/owned/crates/fc-windows/Cargo.toml','targets':[target]}
            metadata={'version':1,'workspace_root':'C:/owned','workspace_members':[package['id']],'packages':[package]}
            log_message={'reason':'compiler-artifact','package_id':package['id'],'manifest_path':package['manifest_path'],'target':target,'profile':{'test':False},'filenames':['C:/owned/target/release/fc_windows.dll']}
            step={'name':'guard-library-compile','passed':True,'exit_code':0,'parent_exited':True,'timed_out_or_pipe_stalled':False,'log_truncated':False,'stdout':'logs/guard-library-compile.stdout.log','arguments':['-Z','build-std','build','--release','--locked','--target',M['TARGET'],'-p','fc-windows','--lib','--message-format=json']}
            row={'role':'custom_action_library','package':'fc-windows','target':'fc_windows','cargo_profile_test':False,'pe_subsystem':2,'relative_path':'custom_action_library/fc-windows/fc_windows.dll','bytes':len(dll),'sha256':SHA(bytes(dll))}
            result={'commands':[step]}
            args=SimpleNamespace(build_evidence=root,guard_library=p,guard_library_sha256=SHA(bytes(dll)))
            def save():
                meta=json.dumps(metadata).encode();(root/'logs/cargo-metadata.stdout.log').write_bytes(meta)
                log=(json.dumps(log_message)+'\n').encode();(root/step['stdout']).write_bytes(log);step['stdout_sha256']=SHA(log)
                ledger=json.dumps([row]).encode();(root/'executables.json').write_bytes(ledger);result['executables_manifest_sha256']=SHA(ledger)
                raw=json.dumps(result).encode();(root/'result.json').write_bytes(raw)
                return {'result_sha256':SHA(raw),'cargo_metadata_sha256':SHA(meta),'source_sha':'7'*40}
            proof=save();actual,_=M['admit_normal_guard'](args,proof,'0.1.0');self.assertEqual(actual,bytes(dll))
            log_message['profile']['test']=True
            with self.assertRaises(ValueError):M['admit_normal_guard'](args,save(),'0.1.0')
            log_message['profile']['test']=False;target['src_path']='C:/other/src/lib.rs'
            with self.assertRaises(ValueError):M['admit_normal_guard'](args,save(),'0.1.0')
            target['src_path']='C:/owned/crates/fc-windows/src/lib.rs';proof=save();p.write_bytes(bytes(dll)+b'changed')
            with self.assertRaises(ValueError):M['admit_normal_guard'](args,proof,'0.1.0')
    def test_dev_scope_never_admits_a_formal_result_or_unsafe_fact(self):
        with tempfile.TemporaryDirectory(prefix='fc-dev-admit-') as td:
            root=Path(td);p=root/'evidence.json';p.write_text(json.dumps({'status':'COMPILE_CHECKS_PASSED_RUNTIME_NOT_RUN'}))
            with self.assertRaises(ValueError):DEV['admit'](p,DEV['fact'](p)['sha256'],root)
            for name in ['../secret','/absolute','a\\b','x:y','a//b']:
                with self.assertRaises(ValueError):DEV['relative'](root,name)
            for raw in [{'path':'evidence.json','bytes':True,'sha256':'0'*64}]:
                with self.assertRaises(ValueError):DEV['bound'](root,raw)

if __name__=='__main__':unittest.main(verbosity=2)
