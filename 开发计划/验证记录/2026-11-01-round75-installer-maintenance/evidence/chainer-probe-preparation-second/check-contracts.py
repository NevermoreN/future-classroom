#!/usr/bin/env python3
"""Pure closed-contract checks only. Does not compile/author an operational fixture."""
import copy
from pathlib import Path
import runpy
import struct
import unittest
import xml.etree.ElementTree as ET
M=runpy.run_path(str(Path(__file__).with_name('chainer-probe.py')))
NONCE='a'*32
ROOT='C:\\Temp\\FutureClassroom-r75-chainer-probe-'+NONCE
OWNER='S-1-5-21-1-2-3-1001'
def spec():return M['parameters'](NONCE,ROOT,OWNER)
def tables():
    v=spec()
    return {'Property':[{'Property':k,'Value':value} for k,value in {'UpgradeCode':v['upgradeCode'],'MSIRESTARTMANAGERCONTROL':'DisableShutdown','Manufacturer':'FutureClassroom test-only','ProductLanguage':'1033','ProductCode':v['productCode'],'ProductName':M['NAME'],'ProductVersion':'0.0.1'}.items()],
    'Directory':[{'Directory':'TARGETDIR','Directory_Parent':'','DefaultDir':'SourceDir'},{'Directory':'LocalAppDataFolder','Directory_Parent':'TARGETDIR','DefaultDir':'.'}],
    'Component':[{'Component':'ChainerRegistryProbe','ComponentId':v['componentCode'],'Directory_':'LocalAppDataFolder','Attributes':'4','Condition':'','KeyPath':'ChainerRegistryMarker'}],
    'Registry':[{'Registry':'ChainerRegistryMarker','Root':'1','Key':v['registryKey'],'Name':v['registryValue'],'Value':'#1','Component_':'ChainerRegistryProbe'}],
    'FeatureComponents':[{'Feature_':'ProbeFeature','Component_':'ChainerRegistryProbe'}],
    'Feature':[{'Feature':'ProbeFeature','Feature_Parent':'','Title':'Rollback-only chainer; owned HKCU marker','Description':'','Display':'2','Level':'1','Directory_':'','Attributes':'0'}],
    'Binary':[{'Name':'FcIndependentChainer','Data':'Binary.FcIndependentChainer'}],
    'MsiEmbeddedChainer':[{'MsiEmbeddedChainer':'FcIndependentChainer','Condition':M['CONDITION'],'CommandLine':v['commandLine'],'Source':'FcIndependentChainer','Type':'2'}],
    'LaunchCondition':[{'Condition':'NOT ALLUSERS','Description':'Independent chainer fixture is per-user only.'},{'Condition':'MSIRESTARTMANAGERCONTROL = "DisableShutdown"','Description':'Independent chainer fixture cannot close applications.'}],
    'InstallExecuteSequence':[{'Action':k,'Condition':'NOT Installed' if k=='AllocateRegistrySpace' else '', 'Sequence':str(n)} for k,n in M['EXECUTE'].items()]}
def pe():
    b=bytearray(1024);b[:2]=b'MZ';struct.pack_into('<I',b,60,64);p=64;b[p:p+4]=b'PE\0\0'
    struct.pack_into('<HH',b,p+4,0x14c,1);struct.pack_into('<H',b,p+20,224);o=p+24;struct.pack_into('<H',b,o,0x10b);struct.pack_into('<H',b,o+68,2)
    struct.pack_into('<II',b,o+96+14*8,0x2000,72);s=o+224;struct.pack_into('<IIII',b,s+8,512,0x2000,512,512);struct.pack_into('<I',b,512+16,3);return b
class Contracts(unittest.TestCase):
    def test_nominal_model_static_only(self):
        self.assertEqual(M['verify'](tables(),spec())['runtime'],'NOT_RUN')
        self.assertEqual(M['pe'](pe())['subsystem'],2)
    def test_authoring_has_only_binary_and_no_payload_or_begin_gate(self):
        tree=ET.fromstring(M['authoring'](spec()));tags={n.tag.split('}')[-1] for n in tree.iter()}
        self.assertFalse(tags & (M['FORBIDDEN']|{'MsiEmbeddedChainer'}))
        self.assertIn('Binary',tags);self.assertIn('ComponentRef',tags)
        reg=tree.find('.//{'+M['NS']+'}RegistryValue');self.assertEqual(reg.get('Key'),spec()['registryKey']);self.assertEqual(reg.get('KeyPath'),'yes')
        row=M['idt'](M['chainer_idt'](spec()),'MsiEmbeddedChainer')[0]
        self.assertEqual(row,tables()['MsiEmbeddedChainer'][0]);self.assertIn('REMOVE = "ALL"',row['Condition'])
    def test_fixed_compile_parameters_do_not_equal_helper_own_sid(self):
        text=Path(__file__).with_name('ChainerProbe.cs.in').read_text()
        concrete=M['source'](spec(),text).decode();self.assertNotIn('@@',concrete)
        self.assertIn('GetOwner(typeof(SecurityIdentifier)).Value==ExpectedOwner',concrete)
        self.assertNotIn('sid==ExpectedOwner',concrete.split('static string ProcessFact')[1].split('static string ThreadFact')[0])
        self.assertIn('join=MsiJoinTransaction(transaction,0,out changed)',concrete)
        self.assertIn('if(join.Value==0)',concrete);self.assertIn('end=MsiEndTransaction(0)',concrete)
        self.assertNotIn('MsiEndTransaction(1)',concrete);self.assertNotIn('MsiBeginTransaction(',concrete)
        self.assertIn('Pack=4',concrete)
    def test_parameters_reject_path_format_nonce_sid_escape(self):
        for nonce,root,owner in [('A'*32,ROOT,OWNER),(NONCE,ROOT+'\\..',OWNER),(NONCE,ROOT.replace('C:',r'\\host\share'),OWNER),(NONCE,ROOT.replace('Temp','[Temp]'),OWNER),(NONCE,ROOT,'S-1-5-18'),(NONCE,ROOT,'S-1-5-21-1-2-3-4294967296')]:
            with self.subTest(root=root,owner=owner),self.assertRaises(ValueError):M['parameters'](nonce,root,owner)
    def test_no_side_effects_or_chainer_role_alias(self):
        for name in M['FORBIDDEN']|{'ForeignEffects'}:
            t=tables();t[name]=[{'Untrusted':'1'}]
            with self.subTest(table=name),self.assertRaises(ValueError):M['verify'](t,spec())
        for table,field,value in [('Component','Attributes','260'),('Registry','Root','2'),('Registry','Key','Software\\Microsoft\\Windows\\CurrentVersion\\Run'),('Registry','Value','#2'),('FeatureComponents','Component_','Foreign')]:
            t=tables();t[table][0][field]=value
            with self.subTest(table=table,field=field),self.assertRaises(ValueError):M['verify'](t,spec())
        for field,value in [('Type','18'),('Condition','1'),('CommandLine','commit'),('Source','Foreign')]:
            t=tables();t['MsiEmbeddedChainer'][0][field]=value
            with self.subTest(field=field),self.assertRaises(ValueError):M['verify'](t,spec())
    def test_only_exact_standard_admin_3900_and_execute_topology(self):
        t=tables();t['AdminExecuteSequence']=[{'Action':'InstallAdminPackage','Condition':'','Sequence':'3900'}];M['verify'](t,spec())
        for name,field,value in [('AdminExecuteSequence','Sequence','3901'),('AdminExecuteSequence','Condition','1'),('InstallExecuteSequence','Action','InstallAdminPackage')]:
            x=copy.deepcopy(t);x[name][0][field]=value
            with self.subTest(name=name,field=field),self.assertRaises(ValueError):M['verify'](x,spec())
        x=tables();x['InstallExecuteSequence']=[r for r in x['InstallExecuteSequence'] if r['Action']!='InstallFinalize']
        with self.assertRaises(ValueError):M['verify'](x,spec())
    def test_pe_rejects_cui_or_anycpu(self):
        for offset,value in [(64+24+68,3),(512+16,1),(512+16,0x20003)]:
            b=pe();struct.pack_into('<H' if offset==64+24+68 else '<I',b,offset,value)
            with self.subTest(offset=offset),self.assertRaises(ValueError):M['pe'](b)
    def test_idt_key_duplicates_and_nulls_rejected(self):
        b=M['chainer_idt'](spec())
        for raw in [b+b.splitlines(keepends=True)[3],b+b'\0',b.replace(b'\tType\r\n',b'\tType\tType\r\n')]:
            with self.subTest(rawlen=len(raw)),self.assertRaises(ValueError):M['idt'](raw,'MsiEmbeddedChainer')
if __name__=='__main__':unittest.main(verbosity=2)
