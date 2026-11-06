"""Ephemeral signing fixtures only: no production keys or sequence state used."""
import contextlib
import io
import json
from pathlib import Path
import runpy
import tempfile
from types import SimpleNamespace
import unittest
import zipfile

P = runpy.run_path(str(Path(__file__).with_name('publish-update-metadata.py')))


class PublishTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix='fc-publish-test-')
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name)
        self.keys, self.root = self.directory/'keys', self.directory/'root.json'
        self.payload = self.directory/'legacy-payload.zip'
        self.universal = self.directory/'universal-payload.zip'
        self.claims, self.more = self.directory/'legacy.json', self.directory/'universal.json'
        for path in (self.payload,self.universal):
            with zipfile.ZipFile(path,'w') as archive:
                archive.writestr('Setup.exe',b'fixture-not-a-real-setup')
                archive.writestr('FutureClassroom.msi',b'fixture-not-a-real-msi')
                if path == self.universal:
                    archive.writestr('installer-variants.bin',b'fixture-only')
        custom = dict(releaseSequence=12,product='FutureClassroom',installerAbi=2,
            guardCapability=3,allowedPredecessors=[{'fixture':True}],channel='stable',
            version='1.2.0',minimumCoordinator='1.1.0')
        self.claims.write_text(json.dumps(custom))
        self.more.write_text(json.dumps(dict(custom,minimumCoordinator='1.2.0')))
        self.args = SimpleNamespace(root=self.root,key_dir=self.keys,payload=self.payload,
            claims=self.claims,additional_target=[(self.universal,self.more)],
            output_dir=self.directory/'metadata')

    def test_two_targets_signed_once_and_existing_sequence_refused(self):
        with contextlib.redirect_stdout(io.StringIO()):
            P['init_root'](SimpleNamespace(key_dir=self.keys,output=self.root))
            P['publish'](self.args)
        root = P['read_public_root'](self.root)
        for name in ('targets','snapshot','timestamp'):
            meta = P['Metadata'].from_file(str(self.args.output_dir/(name+'.json')))
            root.signed.verify_delegate(name,meta.signed_bytes,meta.signatures)
            self.assertEqual(meta.signed.version,12)
        targets = P['Metadata'].from_file(str(self.args.output_dir/'targets.json'))
        self.assertEqual(set(targets.signed.targets),{'stable/12/legacy-payload.zip','stable/12/universal-payload.zip'})
        for name,path in [('legacy-payload.zip',self.payload),('universal-payload.zip',self.universal)]:
            target = targets.signed.targets['stable/12/'+name]
            with path.open('rb') as stream:
                target.verify_length_and_hashes(stream)
        self.assertEqual((self.keys/'last-release-sequence').read_text(),'12')
        with self.assertRaisesRegex(ValueError,'high-water'):
            P['publish'](self.args)

    def test_mismatched_release_rejected_before_opening_private_keys(self):
        custom = json.loads(self.more.read_text())
        custom['version'] = '1.3.0'
        self.more.write_text(json.dumps(custom))
        with self.assertRaisesRegex(ValueError,'same release'):
            P['publish'](self.args)
        self.assertFalse(self.keys.exists())
        self.assertFalse(self.args.output_dir.exists())


if __name__ == '__main__':
    unittest.main()
