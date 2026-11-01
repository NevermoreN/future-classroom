#!/usr/bin/env python3
"""Pure validator contracts. No Windows, transports or product actions."""
from pathlib import Path
import copy,json,runpy,unittest
M=runpy.run_path(str(Path(__file__).with_name('run-quiet-chainer-first.py')))
def model():
    c={'nonce':'a'*32,'root':'C:\\Temp\\FutureClassroom-r75-chainer-probe-'+'a'*32}
    own={'status':'OK','pid':1234,'creationFiletime':123456789,'image':'C:\\Windows\\Installer\\MSI.fixture.tmp','tokenUserSidHex':'010100000000000512000000'}
    parent={'status':'OK','pid':333,'creationFiletime':123,'image':'C:\\Windows\\System32\\msiexec.exe','tokenUserSidHex':'010100000000000512000000'}
    pre={'scope':'PRE_JOIN_IDENTITY_ONLY_NOT_TRANSACTION_SUCCESS','nonce':c['nonce'],'argv':['123','probe',c['nonce'],c['root']],'pointerBytes':4,'own':own,'parent':parent,'consoleWindowPresent':False}
    result=dict(pre,scope='INDEPENDENT_CHAINER_API_FIXTURE_NOT_RUST_PRODUCT_ACCEPTANCE',status='JOIN_END_ROLLBACK_RETURNED',transaction=123,joinReturn=0,endRollbackReturn=0,changeOwnerEventReturned=True,failure=None,checkedClose=[{'role':'returned-change-owner-event','ok':True,'win32':0}],releaseProof='UNTRUSTED_JSON_IS_IGNORED')
    actual={'helperExpected':{'pid':1234,'creationFiletime':str(own['creationFiletime']),'image':own['image']},'helperExitEvidence':'ACTUAL_HELD_PROCESS_GETEXITCODE','helperExactInstanceAndBinary':True,'helperExitCode':0,'helperHandleClose':{'ok':True},'negativeOwnerDeathAttempted':False,'clientActualExit':True,'clientExitCode':1603}
    return pre,result,actual,c
class Contracts(unittest.TestCase):
    def test_held_exit_evidence_does_not_predict_msi_exit_or_rely_json(self):
        pre,result,actual,c=model();M['qualify'](pre,result,actual,c)
        actual['clientExitCode']=0;M['qualify'](pre,result,actual,c)
    def test_missing_exit_changed_instance_negative_kill_native_failure_reject(self):
        for obj,key,value in [('actual','helperExitEvidence','UNKNOWN_NOT_CAPTURED'),('actual','helperExitCode',1),('actual','negativeOwnerDeathAttempted',True),('actual','helperHandleClose',{'ok':False}),('result','joinReturn',5),('result','transaction',124),('result','checkedClose',[{'role':'returned-change-owner-event','ok':False,'win32':6}])]:
            pre,result,actual,c=model();(actual if obj=='actual' else result)[key]=value
            with self.subTest(obj=obj,key=key),self.assertRaises(AssertionError):M['qualify'](pre,result,actual,c)
        pre,result,actual,c=model();actual['helperExpected']['creationFiletime']='999'
        with self.assertRaises(AssertionError):M['qualify'](pre,result,actual,c)
    def test_short_utf8_stdin_and_exact_cleanup_interfaces(self):
        req=M['remote'](M['SETTLE'],{'nonce':'a'*32,'root':'C:\\临时'})
        self.assertLessEqual(len(req['command'][-1]),1200)
        raw=req['input'].decode().split("'|ConvertFrom-Json;",1)[0].removeprefix("$c='")
        self.assertEqual(json.loads(raw)['root'],'C:\\临时')
        self.assertIn("InvokeMember('Count'",M['COMMON']);self.assertIn("InvokeMember('RelatedProducts'",M['COMMON'])
        self.assertIn('SetOwner($sid)',M['CREATE']);self.assertNotIn('S-1-5-32-544',M['CREATE'])
        self.assertIn('original_gone',Path(__file__).with_name('run-quiet-chainer-first.py').read_text())
        self.assertNotIn('TerminateProcess',M['SETTLE']+M['DELETE']);self.assertIn('downloadsAcknowledged',M['DELETE'])
        self.assertFalse(M['original_gone']('UNKNOWN'));self.assertFalse(M['original_gone']('ORIGINAL_STILL_LIVE'))
if __name__=='__main__':unittest.main(verbosity=2)
