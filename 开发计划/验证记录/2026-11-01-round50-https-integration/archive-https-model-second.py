"""Owned synthetic filesystem and memory models; no real evidence inventory/seal."""
from pathlib import Path
import base64,copy,hashlib,importlib.util,json,shutil,sys,tempfile
sys.dont_write_bytecode=True
E=Path('/tmp/fc-round50-https-integration-evidence');helper=E/'archive-https-rounds.py'
spec=importlib.util.spec_from_file_location('https_archive',helper);m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
checks=[];root=Path(tempfile.mkdtemp(prefix='.archive-https-model-first-',dir=E));marker='fc.https.archive.models-owned-only/v1\n';(root/'.model-owner').write_text(marker)
result={'schema':'fc.https.archive.local-models/v1','status':'FAIL','scope':'Pure scanner/malformed metadata and synthetic owned filesystem models only. Does not inventory real R47-R51 evidence, write main, seal, execute tests/browser/SSH/Cargo or clean real evidence.'}
def ok(name,condition):
 if not condition:raise AssertionError(name)
 checks.append({'name':name,'status':'PASS'})
def reject(name,fn):
 try:fn()
 except (ValueError,OSError):checks.append({'name':name,'status':'PASS','expectation':'rejected'});return
 raise AssertionError(name+' was accepted')
def scanner(name,text,kind):
 s=m.Scanner();s.text(text,'model');ok(name,kind in {x['kind'] for x in s.blocked});ok(name+' findings expose only path/type',all(set(x)=={'kind','pathOrDecodedLocation'} for x in s.blocked))
try:
 src=root/'source';src.mkdir();(src/'.owner').write_text('synthetic-only/v1\n');(src/'before.rs').write_text('const X:u8=1;\n');(src/'first-failure.json').write_text('{"status":"FAIL","scope":"synthetic model only"}\n');(src/'raw.stdout').write_text('ORIGINAL RAW\r\n')
 inv=m.inventory(src);ok('ordinary raw/source/originalFAIL included',len(inv['rows'])==4 and not inv['scanner'].blocked)
 dst=root/'copy';progress={'destinationCreated':False,'copiedFiles':0,'copiedBytes':0};m.copy_rows_to_destination(src,dst,inv['rows'],progress)
 ok('exact synthetic byte copy includes original failure',all((dst/x['path']).read_bytes()==(src/x['path']).read_bytes() for x in inv['rows']) and json.loads((dst/'first-failure.json').read_text())['status']=='FAIL')
 (root/'reader-link').symlink_to(src,target_is_directory=True)
 reject('safe reader forbids symlink ancestor',lambda:m.safe_bytes(root/'reader-link'/'before.rs'))
 reject('existing destination never overwritten',lambda:m.copy_rows_to_destination(src,dst,inv['rows'],progress))
 old=(src/'before.rs').read_bytes();(src/'before.rs').write_bytes(old.replace(b'1',b'2'));reject('same-size source mutation caught',lambda:m.stable(src,inv['rows'],inv['entries']));(src/'before.rs').write_bytes(old)
 (src/'added.txt').write_text('new');reject('new membership caught',lambda:m.stable(src,inv['rows'],inv['entries']));(src/'added.txt').unlink()
 (src/'link.txt').symlink_to(root/'.model-owner');reject('symlink never followed',lambda:m.enumerate_tree(src));(src/'link.txt').unlink()
 (src/'BEFORE.rs').write_text('case');reject('case collision',lambda:m.enumerate_tree(src));(src/'BEFORE.rs').unlink()
 (src/'oversize.txt').touch()
 with (src/'oversize.txt').open('r+b') as f:f.truncate(m.MAX_FILE+1)
 reject('single file bound before read',lambda:m.inventory(src));(src/'oversize.txt').unlink()
 reject('relative path escape',lambda:m.relative(root/'outside.txt',src))
 forbidden=[]
 for name in ['not-real.db','not-real.key','id_ed25519-fixture','live.control.json','undeclared.pem','not-real.exe','not-real.zip']:
  p=src/name;p.write_text('UNREAD SYNTHETIC PLACEHOLDER');forbidden.append(p)
 (src/'target').mkdir();(src/'target'/'unknown.zzz').write_text('not traversed')
 original=Path.read_bytes;safe_original=m.safe_bytes;seen=[]
 def guarded(p):
  if p in forbidden or 'target' in p.parts:raise AssertionError('private/binary/target was read')
  seen.append(str(p));return original(p)
 Path.read_bytes=guarded;m.safe_bytes=lambda p:guarded(p) if p in forbidden or 'target' in p.parts else safe_original(p)
 try:excluded=m.inventory(src)
 finally:Path.read_bytes=original;m.safe_bytes=safe_original
 ok('key/database/LIVEcontrol/binary/unknownPEM/target never opened or hashed',len(excluded['excluded'])==8 and len(excluded['rows'])==4 and not excluded['scanner'].blocked)
 for p in forbidden:p.unlink()
 shutil.rmtree(src/'target')
 (src/'unknown.zzz').write_text('opaque');unknown=m.inventory(src);ok('unknown eligible type blocks archive',any(x['kind']=='UNKNOWN_FILE_TYPE_REQUIRES_REVIEW' for x in unknown['scanner'].blocked));(src/'unknown.zzz').unlink()
 (src/'binary.txt').write_bytes(b'MZsynthetic');bad=m.inventory(src);ok('binary magic disguised as text blocks',any(x['kind']=='INLINE_BINARY_FILE_MAGIC' for x in bad['scanner'].blocked));(src/'binary.txt').unlink()
 (src/'bad.json').write_text('{');bad=m.inventory(src);ok('invalid JSON retained as blocking original',any(x['kind']=='INVALID_JSON' for x in bad['scanner'].blocked));(src/'bad.json').unlink()
 scanner('private key marker blocked','-----BEGIN '+ 'PRIVATE KEY-----','PRIVATE_KEY')
 scanner('provider token blocked','ghp_'+'X'*25,'GITHUB_TOKEN')
 scanner('signed URL blocked','https://example.test/file?sig='+'x'*20,'SIGNED_OR_CREDENTIALED_URL')
 s=m.Scanner();secret='0123456789abcdef'*4;s.walk({'payloadBase64':base64.b64encode(json.dumps({'cookie':'__Host-FutureClassroom='+secret}).encode()).decode()},'model')
 ok('nested JSON base64 session blocked',any(x['kind']=='JSON_SESSION_SECRET' for x in s.blocked))
 s=m.Scanner();encoded=base64.b64encode(('__Host-FutureClassroom='+secret).encode('utf-16-le')).decode();s.text('powershell -EncodedCommand '+encoded,'model');ok('UTF16 PowerShell wrapper secret decoded and blocked',any(x['kind']=='SESSION_COOKIE' for x in s.blocked))
 s=m.Scanner();s.walk({'base64':base64.b64encode(b'MZsynthetic').decode()},'model');ok('inline base64 executable blocked',any(x['kind']=='INLINE_EXECUTABLE_OR_ZIP' for x in s.blocked))
 s=m.Scanner();s.walk({'token': 'a'*64},'model');ok('explicit repetitive synthetic fixture annotated',not s.blocked and any(x['kind']=='REPETITIVE_SYNTHETIC_FIXTURE' for x in s.annotations))
 s=m.Scanner();s.text('__Host-FutureClassroom='+secret,'model',synthetic=True);ok('explicit expired synthetic fixture classified only',not s.blocked and any(x['kind']=='EXPIRED_EXPLICIT_SYNTHETIC_FIXTURE_SECRET' for x in s.annotations))
 s=m.Scanner();s.text("csrf='"+secret+"'",'model',retired=True);ok('exact retired owned stop scope classified only',not s.blocked and any(x['kind']=='EXPIRED_EXACT_OWNED_STOP_CAPABILITY' for x in s.annotations))
 sentinel=root/'do-not-follow.key';sentinel.write_text('unread synthetic')
 (src/'references.json').write_text(json.dumps({'path':str(sentinel)}));Path.read_bytes=guarded
 try:
  observed=[]
  def no_ref_read(p):
   if p==sentinel:raise AssertionError('external reference followed')
   observed.append(p);return original(p)
  Path.read_bytes=no_ref_read;m.safe_bytes=lambda p:no_ref_read(p) if p==sentinel else safe_original(p);m.inventory(src)
 finally:Path.read_bytes=original;m.safe_bytes=safe_original
 ok('JSON reference never followed',sentinel not in observed)
 inv=m.inventory(src);partial=root/'partial';last=inv['rows'][-1];p=src/last['path'];before=p.read_bytes();p.write_bytes(before+b'changed');progress={'destinationCreated':False,'copiedFiles':0,'copiedBytes':0}
 reject('late copy mutation retains truthful partial',lambda:m.copy_rows_to_destination(src,partial,inv['rows'],progress));ok('partial destination retained with actual copied count',progress['destinationCreated'] and 0<progress['copiedFiles']<len(inv['rows']) and partial.is_dir());p.write_bytes(before)
 binding={'root':str(src),'owner':'.owner','ownerBytes':len((src/'.owner').read_bytes()),'ownerSha256':m.sha((src/'.owner').read_bytes())};m.validate_owner(binding);wrong=dict(binding,ownerSha256='0'*64);reject('original owner mismatch denied',lambda:m.validate_owner(wrong))
 orig_bind=m.BINDINGS;m.BINDINGS={'model':{'root':str(src)}}
 proof_report=src/'pass-model.json';proof_report.write_text('{"status":"PASS_SYNTHETIC_MODEL_ONLY","cleanup":"PASS"}')
 roles=['source-integration','host-full-matrix','windows-runtime','browser-final','readonly-final'];rows=[{'role':role,'path':str(proof_report),'sha256':m.sha(proof_report.read_bytes()),'status':'PASS_SYNTHETIC_MODEL_ONLY','cleanupRequired':True} for role in roles]
 authority={'schema':'fc.https-rounds.final-archive-authority/v1','status':'ALL_CONSUMERS_FINISHED_SOURCE_INTEGRATED','activeConsumers':[],'sourceIntegration':'EXACT_VALIDATED_BYTES_INTEGRATED','gitHead':'a'*40,'proofs':rows};ap=src/'authority.json';ap.write_text(json.dumps(authority));m.final_proof(ap);ok('all-finished explicit model proof accepted',True)
 for name,mutate in [('active consumer',lambda a:a['activeConsumers'].append('running')),('integration missing',lambda a:a.__setitem__('sourceIntegration','PENDING')),('missing Host role',lambda a:a['proofs'].pop(1)),('proof bytes changed',lambda a:a['proofs'][0].__setitem__('sha256','0'*64)),('outside proof reference',lambda a:a['proofs'][0].__setitem__('path',str(sentinel))),('RUNNING cannot be final',lambda a:a['proofs'][0].__setitem__('status','RUNNING'))]:
  bad=copy.deepcopy(authority);mutate(bad);assert bad!=authority;ap.write_text(json.dumps(bad));reject(name,lambda:m.final_proof(ap))
 m.BINDINGS=orig_bind
 result.update(status='PASS_SYNTHETIC_MODELS_ONLY',checks=checks,count=len(checks),helper={'path':str(helper),'bytes':helper.stat().st_size,'sha256':hashlib.sha256(helper.read_bytes()).hexdigest()},fixtureRoot=str(root))
except BaseException as error:result.update(error={'type':type(error).__name__,'message':str(error)[:200]},checks=checks,count=len(checks));raise
finally:
 Path.read_bytes=original if 'original' in globals() else Path.read_bytes
 assert root.parent==E and root.name.startswith('.archive-https-model-first-') and (root/'.model-owner').read_text()==marker
 shutil.rmtree(root);result['ownedFixtureCleanup']='PASS' if not root.exists() else 'FAIL'
 out=E/'archive-https-model-second.json'
 with out.open('x') as f:json.dump(result,f,ensure_ascii=False,indent=2);f.write('\n')
 print(json.dumps({'status':result['status'],'checks':result.get('count'),'ownedFixtureCleanup':result['ownedFixtureCleanup'],'report':str(out),'sha256':hashlib.sha256(out.read_bytes()).hexdigest()}))
