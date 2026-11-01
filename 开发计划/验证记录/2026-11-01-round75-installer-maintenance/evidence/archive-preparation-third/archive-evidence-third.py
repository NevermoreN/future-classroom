#!/usr/bin/env python3
"""R75 exact evidence archive + independent source reread + decoded secret scan.
Only owned new archive files and this root's new verification receipt are written.
No removal, Git mutation, source change, Windows, SSH, Cargo or network operation.
"""
from pathlib import Path
import argparse
import copy
import base64
import datetime
import gzip
import hashlib
import json
import re
import stat
import sys
import ssl
import subprocess
import xml.etree.ElementTree as ET

SOURCE=Path('/tmp/fc-round75-installer-maintenance')
DEST=Path('/workspace/共享/Eliora/未来教室计划/开发计划/验证记录/2026-11-01-round75-installer-maintenance')
RECEIPT='evidence/archive-verification-first.json'
CONTROLLER=SOURCE/'evidence/archive-preparation-third'
PRUNED={'worktree','target','tools'}
CACHE_PARTS={'__pycache__','node_modules','.npm-cache','.cache'}
EXCLUDED_SUFFIXES={'.exe','.zip','.pdb','.dll','.lib','.rlib','.rmeta','.pyc','.db','.sqlite','.sqlite3','.cache','.obj','.o','.a','.so','.dylib','.wasm','.7z','.tar','.msi','.deb','.cab','.msm','.msp','.gz','.xz','.bz2','.bin','.class','.pyz','.nupkg'}
CERTIFICATES={}
BINARY_CONTAINERS={}
CERT_SUFFIXES={'.pem','.crt','.cer','.der'}
PRIVATE_SUFFIXES={'.key','.pfx','.p12'}
COMPRESS=re.compile(r'.*objdump[^/]*\.stdout\.log')
PATTERNS={
 'private-key-pem':re.compile(r'-----BEGIN (?:OPENSSH |RSA |EC |DSA |ENCRYPTED )?PRIVATE KEY-----\s+[A-Za-z0-9+/=\s]{32,}-----END (?:OPENSSH |RSA |EC |DSA |ENCRYPTED )?PRIVATE KEY-----'),
 'github-token':re.compile(r'(?<![A-Za-z0-9])(?:gh[pousr]_[A-Za-z0-9]{30,}|github_pat_[A-Za-z0-9_]{30,})(?![A-Za-z0-9])'),
 'openai-secret':re.compile(r'(?<![A-Za-z0-9])sk-(?:proj-|svcacct-)?[A-Za-z0-9_-]{24,}'),
 'aws-access-key':re.compile(r'(?<![A-Z0-9])(?:AKIA|ASIA)[A-Z0-9]{16}(?![A-Z0-9])'),
 'bearer-secret':re.compile(r'(?i)authorization\s*[=:]\s*[\x22\x27]?Bearer\s+[A-Za-z0-9._~-]{24,}'),
 'basic-secret':re.compile(r'(?i)authorization\s*[=:]\s*[\x22\x27]?Basic\s+[A-Za-z0-9+/]{16,}={0,2}'),
 'signed-download-url':re.compile(r'(?i)https?://[^\s\x22\x27<>]{5,}[?&](?:sig|signature|x-amz-signature)=[A-Za-z0-9%_+/-]{20,}'),
 'session-cookie-bearer':re.compile(r'__Host-FutureClassroom=[0-9a-fA-F]{64}'),
 'json-issued-web-secret':re.compile(r'(?i)[\x22\x27](?:csrf|pollSecret|sessionCookie|accessToken|refreshToken)[\x22\x27]\s*:\s*[\x22\x27][A-Za-z0-9_./+=-]{32,}[\x22\x27]'),
 'json-password-value':re.compile(r'(?i)[\x22\x27]password[\x22\x27]\s*:\s*[\x22\x27][^\x22\x27\n]{12,128}[\x22\x27]'),
 'potential-pairing-short-code':re.compile(r'(?i)\\?[\x22\x27]pairingCode\\?[\x22\x27]\s*:\s*\\?[\x22\x27][0-9]{8}\\?[\x22\x27]'),
}

def need(v,m):
 if not v:raise AssertionError(m)
def sha(d):return hashlib.sha256(d).hexdigest()
def original(path):
 need(stat.S_ISREG(path.lstat().st_mode),'nonregular original '+str(path))
 need(path.suffix.lower() not in PRIVATE_SUFFIXES,'private material forbidden by filename; contents not read '+str(path))
 if path.suffix.lower() in CERT_SUFFIXES:need(path.relative_to(SOURCE).as_posix() in CERTIFICATES,'unlisted certificate forbidden '+str(path))
 data=path.read_bytes()
 if path.suffix.lower() in CERT_SUFFIXES:public_certificate(path,data)
 return data,{'path':path.relative_to(SOURCE).as_posix(),'bytes':len(data),'sha256':sha(data)}
def write_new(path,data):
 path.parent.mkdir(parents=True,exist_ok=True)
 with path.open('xb') as stream:stream.write(data)
 need(path.read_bytes()==data,'write readback mismatch '+str(path))
def as_json(data):
 return json.loads(data.decode('utf-8-sig'))
def excluded(path,data=None):
 return (path.suffix.lower() in EXCLUDED_SUFFIXES or path.name.endswith(('.db-wal','.db-shm','.sqlite-wal','.sqlite-shm'))
  or any(part in CACHE_PARTS for part in path.relative_to(SOURCE).parts)
  or (data is not None and binary_magic(data)))
def binary_magic(data):
 return data.startswith((b'MZ',bytes.fromhex('d0cf11e0a1b11ae1'),b'PK\x03\x04',b'PK\x05\x06',b'PK\x07\x08',b'\x7fELF',b'!<arch>\n',b'\x1f\x8b',b'\xfd7zXZ\x00',b'7z\xbc\xaf\x27\x1c'))
def source_members():
 # Do not descend into any infrastructure subtree, even to gather metadata.
 result=[]
 def walk(directory):
  for path in sorted(directory.iterdir()):
   if directory==SOURCE and path.name in PRUNED:
    need(path.is_dir() and not path.is_symlink(),'pruned infrastructure root must be a real owned directory '+str(path));continue
   need(not path.is_symlink(),'source symlink forbidden '+str(path))
   if path.is_dir():walk(path)
   else:
    need(stat.S_ISREG(path.lstat().st_mode),'nonregular source forbidden '+str(path));result.append(path)
 walk(SOURCE)
 return result
def bound_path(relative):
 p=SOURCE/relative
 need(not Path(relative).is_absolute() and p.resolve().is_relative_to(SOURCE.resolve()) and not p.is_symlink(),'exact owned relative report path')
 need(not any(part in PRUNED for part in Path(relative).parts[:1]),'report cannot be in pruned infrastructure')
 return p

def exact_report(relative,digest):
 p=bound_path(relative);data=p.read_bytes()
 need(sha(data)==digest,'bound report SHA '+relative)
 return p,data,as_json(data)


def credential_text_views(data):
 views=[]
 try:views.append(('utf8',data.decode('utf-8-sig')))
 except UnicodeError:
  # A strictly bounded Windows PowerShell transport exception: only an actual
  # CLIXML envelope, strict CP936 roundtrip and validated PowerShell Objs XML.
  if data.startswith(b'#< CLIXML'):
   text=data.decode('cp936',errors='strict')
   need(text.encode('cp936',errors='strict')==data,'CP936 CLIXML exact roundtrip')
   match=re.match(r'\A#< CLIXML\r?\n',text)
   need(match is not None,'CP936 CLIXML exact header')
   xml=text[match.end():]
   need('<!DOCTYPE' not in xml and '<!ENTITY' not in xml,'CP936 CLIXML DTD/entity forbidden')
   try:document=ET.fromstring(xml)
   except ET.ParseError:raise AssertionError('CP936 CLIXML malformed XML') from None
   need(document.tag=='{http://schemas.microsoft.com/powershell/2004/04}Objs','CP936 CLIXML known PowerShell Objs namespace')
   views.append(('cp936-clixml',text))
 if data.startswith((b'\xff\xfe',b'\xfe\xff')) or (len(data)>4 and data.count(b'\0')>len(data)//8):
  for encoding in ('utf-16-le','utf-16-be'):
   try:views.append((encoding,data.decode(encoding).lstrip('\ufeff')))
   except UnicodeError:pass
 need(bool(views),'unrecognized retained evidence encoding')
 return views

EXACT_MSI_CONTAINERS={
 'evidence/root-registry-recovery-readonly-first.json':('0187609747ffdd20c06e584553cd63ad478087296ad2eb2894491e2a49153f94','/files/5/base64'),
 'evidence/root-registry-recovery-readonly-first.stdout.log':('07f267286de612bdd481b484c731acf9987efdbb76283cb06c5b751105d71bf8','/files/5/base64'),
 'evidence/root-registry-recovery-readonly-second.json':('916ff9c6c61d759c4efe085c6adf7ca060718dbf6b9b3af50a9f085899563a3c','/files/4/base64'),
}
EXACT_MSI_SHA='d3c6d5e4937d5dd673e9835a7fbc8d5c7d03af8b0527e1a12d565da1a0e4e248'
EXACT_BASE64_SHA='47f88ef0328f794db4fee0e3c3deef5404ab4e1297ed41bb0860fe06b4eac3e6'
OMISSION_QUALIFICATION='EXACT_SYNTHETIC_FIXTURE_MSI_BASE64_ONLY_REMOVED_ORIGINAL_RETAINED_OUTSIDE_GIT'

def strict_json(data):
 def object_hook(pairs):
  value={}
  for key,item in pairs:
   need(key not in value,'duplicate JSON key in exact binary container/derivative');value[key]=item
  return value
 def constant(_):raise AssertionError('nonfinite JSON in exact binary container/derivative')
 return json.loads(data.decode('utf-8-sig'),object_pairs_hook=object_hook,parse_constant=constant)

def expected_derivative(original,row):
 need(isinstance(original,dict) and '_archiveBinaryOmission' not in original,'original object and fresh omission metadata key')
 expected=copy.deepcopy(original)
 need(set(row)=={'path','bytes','sha256','derivedPath','derivedBytes','derivedSha256','omissions'},'closed exact container allowlist fields')
 need(row['path'] in EXACT_MSI_CONTAINERS,'closed exact synthetic MSI original path')
 digest,pointer=EXACT_MSI_CONTAINERS[row['path']]
 need(row['sha256']==digest and row['omissions']==[{'jsonPointer':pointer,'decodedBytes':10240,'decodedSha256':EXACT_MSI_SHA,'base64FieldSha256':EXACT_BASE64_SHA}],'exact single reviewed MSI leaf identity')
 # This allowlist deliberately supports only the actual observed files/index/base64 topology.
 index=int(pointer.split('/')[2]);files=expected['files']
 need(isinstance(files,list) and index<len(files) and isinstance(files[index],dict) and isinstance(files[index].get('base64'),str),'exact observed MSI leaf topology')
 encoded=files[index]['base64'];decoded=base64.b64decode(encoded,validate=True)
 need(base64.b64encode(decoded).decode('ascii')==encoded and len(decoded)==10240 and decoded.startswith(bytes.fromhex('d0cf11e0a1b11ae1')) and sha(decoded)==EXACT_MSI_SHA and sha(encoded.encode('utf-8'))==EXACT_BASE64_SHA,'exact CFBF fixture MSI only')
 del files[index]['base64']
 expected['_archiveBinaryOmission']={'originalPath':row['path'],'originalBytes':row['bytes'],'originalSha256':row['sha256'],'omissions':row['omissions'],'qualification':OMISSION_QUALIFICATION}
 return expected

def same_semantics(first,second):
 return json.dumps(first,sort_keys=True,separators=(',',':'),ensure_ascii=False,allow_nan=False)==json.dumps(second,sort_keys=True,separators=(',',':'),ensure_ascii=False,allow_nan=False)

def reviewed_binary_containers(path,expected_manifest_sha256):
 path=path if path.is_absolute() else SOURCE/path
 need(path.resolve().is_relative_to(SOURCE.resolve()) and path.is_file() and not path.is_symlink(),'root-owned binary-container manifest')
 need(all(not parent.is_symlink() for parent in path.parents if parent.is_relative_to(SOURCE)),'no symlink allowlist ancestor')
 need(path.suffix=='.json','root-reviewed manifest must be JSON')
 manifest_data=path.read_bytes();need(sha(manifest_data)==expected_manifest_sha256,'exact root-reviewed manifest SHA');manifest=strict_json(manifest_data)
 need(set(manifest)=={'status','sourceRoot','containers'} and manifest['status']=='ROOT_REVIEWED_EXACT_SYNTHETIC_MSI_CONTAINER_OMISSIONS_ONLY' and manifest['sourceRoot']==str(SOURCE),'exact root-reviewed binary container manifest scope')
 records=manifest['containers'];need(isinstance(records,list) and len(records)==3 and {r['path'] for r in records}==set(EXACT_MSI_CONTAINERS),'exact three observed containers only')
 result={};derived_paths=set()
 for row in records:
  original_path=bound_path(row['path']);data,identity=original(original_path)
  need(identity=={key:row[key] for key in ('path','bytes','sha256')},'exact excluded original container bytes')
  derived_path=bound_path(row['derivedPath']);need(derived_path.is_file() and not derived_path.is_symlink() and row['derivedPath'] not in EXACT_MSI_CONTAINERS and row['derivedPath'] not in derived_paths,'distinct exact derived JSON file')
  need(derived_path.suffix=='.json' and not excluded(derived_path),'ordinary retained derivative JSON')
  derived=derived_path.read_bytes();need(len(derived)==row['derivedBytes'] and sha(derived)==row['derivedSha256'],'exact reviewed derivative bytes')
  need(same_semantics(strict_json(derived),expected_derivative(strict_json(data),row)),'all original semantic values/statuses identical except one exact MSI leaf and closed omission metadata')
  derived_paths.add(row['derivedPath']);result[row['path']]=dict(row,manifestPath=path.relative_to(SOURCE).as_posix(),manifestSha256=sha(manifest_data))
 return result

def public_certificate_der(data):
    # Exactly one public certificate block, never a bundle, key or surrounding
    # text. Decode strictly, reject trailing/noncanonical DER, then parse X.509.
    match = re.fullmatch(br'-----BEGIN CERTIFICATE-----\r?\n([A-Za-z0-9+/=\r\n]+)-----END CERTIFICATE-----\r?\n?', data)
    need(match is not None, 'public certificate PEM envelope rejected')
    encoded = match.group(1).replace(b'\r', b'').replace(b'\n', b'')
    try:
        der = base64.b64decode(encoded, validate=True)
    except (ValueError, TypeError):
        raise AssertionError('public certificate base64 rejected') from None
    need(base64.b64encode(der) == encoded and len(der) >= 4 and der[0] == 0x30,
         'public certificate DER sequence rejected')
    first = der[1]
    if first < 0x80:
        header, length = 2, first
    else:
        count = first & 0x7f
        need(1 <= count <= 4 and len(der) >= 2 + count and der[2] != 0,
             'public certificate DER length rejected')
        header, length = 2 + count, int.from_bytes(der[2:2 + count], 'big')
        need(length >= 0x80 and (length.bit_length() + 7) // 8 == count,
             'public certificate DER nonminimal length rejected')
    need(header + length == len(der), 'public certificate DER trailing bytes rejected')
    try:
        ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT).load_verify_locations(cadata=der)
    except (ssl.SSLError, ValueError):
        raise AssertionError('public certificate X509 parse rejected') from None
    return der


def public_certificate(path,data):
 row=CERTIFICATES[path.relative_to(SOURCE).as_posix()]
 need(len(data)==row['bytes'] and sha(data)==row['sha256'],'exact listed public certificate')
 report=SOURCE/row['report'];need(report.resolve().is_relative_to(SOURCE.resolve()) and report.is_file() and not report.is_symlink(),'certificate original report scope')
 need(sha(report.read_bytes())==row['reportSha256'],'certificate original report identity')
 need(path.name=='tls-public.pem','only exact controlled tls-public.pem certificates are admissible')
 original=as_json(report.read_bytes());preserved=original['rawPreservation'];record=preserved['files']['tls-public.pem']
 need(preserved['status']==record['status']=='PASS' and Path(preserved['directory'])==path.parent and record['artifact']==str(path)
  and record['remote']==record['downloaded']=={'bytes':len(data),'sha256':sha(data)}
  and base64.b64decode(record['base64'],validate=True)==data,'controlled original public certificate provenance')
 der=public_certificate_der(data)
 return dict(path=path.relative_to(SOURCE).as_posix(),bytes=len(data),sha256=sha(data),derSha256=sha(der),x509ParseExitCode=0,report=row['report'],reportSha256=row['reportSha256'])

def main():
 global CERTIFICATES,BINARY_CONTAINERS
 parser=argparse.ArgumentParser(description=__doc__)
 parser.add_argument('--source-base-git',required=True)
 parser.add_argument('--acceptance-report',required=True)
 parser.add_argument('--expected-acceptance-sha256',required=True)
 parser.add_argument('--expected-acceptance-status',required=True)
 parser.add_argument('--readonly-audit',required=True)
 parser.add_argument('--candidate-producer',required=True)
 parser.add_argument('--expected-producer-sha256',required=True)
 parser.add_argument('--candidate-freeze',required=True)
 parser.add_argument('--expected-freeze-sha256',required=True)
 parser.add_argument('--candidate-joint-freeze',required=True)
 parser.add_argument('--expected-joint-freeze-sha256',required=True)
 parser.add_argument('--expected-readonly-sha256',required=True)
 parser.add_argument('--public-certificate-allowlist',type=Path)
 parser.add_argument('--binary-container-allowlist',type=Path,required=True)
 parser.add_argument('--expected-binary-container-allowlist-sha256',required=True)
 parser.add_argument('--root-controllers-terminal',action='store_true')
 parser.add_argument('--execute',action='store_true')
 args=parser.parse_args()
 need(re.fullmatch(r'[0-9a-f]{40}',args.source_base_git),'explicit source SHA')
 for value in (args.expected_acceptance_sha256,args.expected_readonly_sha256,args.expected_producer_sha256,args.expected_freeze_sha256,args.expected_joint_freeze_sha256,args.expected_binary_container_allowlist_sha256):need(re.fullmatch(r'[0-9a-f]{64}',value),'explicit terminal report SHA')
 need(args.expected_acceptance_status.startswith('PASS_R75_') and not any(x in args.expected_acceptance_status for x in ('STATIC','PREPARED','NOT_RUN')),'actual runtime acceptance status required')
 if not args.execute:
  print(json.dumps({'status':'PREPARED_ARCHIVE_ONLY_NOT_EXECUTED','sourceBaseGit':args.source_base_git,'sourceRoot':str(SOURCE),'destination':str(DEST),'sourcePayloadRead':False,'archiveExecuted':False,'prunedInfrastructureSubtrees':sorted(PRUNED),'scope':'Await final independent finite runtime audit and readonly terminal hashes; no archive or deletion.'}));return
 need(args.root_controllers_terminal,'root must confirm every original controller terminal and source membership frozen')
 owner=SOURCE/'.owner';owner_bytes=owner.read_bytes();need(owner_bytes==b'future-classroom-owned-r75-installer-maintenance/v1\n','exact R75 root owner marker')
 need((SOURCE/'evidence/.owner').read_bytes()==b'future-classroom-owned-r75-installer-maintenance-evidence/v1\n','exact R75 evidence owner')
 need(Path(__file__).resolve().parent==CONTROLLER and (CONTROLLER/'.owner').read_bytes()==b'future-classroom-owned-r75-archive-preparation-third/v1\n','exact new archive controller owner/location')
 need(DEST==Path('/workspace/共享/Eliora/未来教室计划/开发计划/验证记录/2026-11-01-round75-installer-maintenance'),'exact R75 destination')
 need(SOURCE.is_dir() and not SOURCE.is_symlink() and not DEST.exists(),'fresh archive required')
 need(not (SOURCE/RECEIPT).exists(),'fresh receipt required')
 acceptance_path=bound_path(args.acceptance_report);final_path=bound_path(args.readonly_audit)
 for path in (acceptance_path,final_path):
  need(path.resolve().is_relative_to(SOURCE.resolve()),'terminal report scope')
  if not path.exists():
   print(json.dumps({'status':'WAIT','reasonCode':'MISSING_FINAL_'+path.name,'reportCreated':False}));return
  need(path.is_file() and not path.is_symlink(),'terminal report regular file')
  try:pending=as_json(path.read_bytes())
  except (ValueError,UnicodeError):
   print(json.dumps({'status':'WAIT','reasonCode':'INCOMPLETE_FINAL_'+path.name,'reportCreated':False}));return
  if pending.get('status') in ('RUNNING','STARTED','OBSERVATION_INTERRUPTED_NOT_COMPLETION'):
   print(json.dumps({'status':'WAIT','reasonCode':'NONTERMINAL_FINAL_'+path.name,'reportCreated':False}));return
 final=as_json(final_path.read_bytes())
 need(final['status']=='PASS' and final.get('completedAtUtc') and sha(final_path.read_bytes())==args.expected_readonly_sha256,'exact terminal readonly audit')
 runtime=as_json(acceptance_path.read_bytes())
 need(runtime['status']==args.expected_acceptance_status and sha(acceptance_path.read_bytes())==args.expected_acceptance_sha256,'exact finite independent acceptance')
 producer_path,producer_bytes,producer=exact_report(args.candidate_producer,args.expected_producer_sha256)
 _,freeze_bytes,freeze=exact_report(args.candidate_freeze,args.expected_freeze_sha256)
 _,joint_bytes,joint=exact_report(args.candidate_joint_freeze,args.expected_joint_freeze_sha256)
 need(producer['status']=='PASS_CODEGEN_NOT_RUNTIME' and producer['sourceUnchanged'] is True and producer['sourceBefore']==producer['sourceAfter'],'actual successful final frozen producer')
 need(producer['freezeSha256']==sha(freeze_bytes) and producer['jointFreeze']['sha256']==sha(joint_bytes),'producer exact freeze and joint identities')
 canonical=lambda rows:{r['path']:(r['bytes'],r['sha256']) for r in rows}
 need(canonical(producer['sourceBefore'])==canonical(freeze['source'])==canonical(joint['sources']),'exact candidate source ledger equality; Git base is not candidate source commit')
 need(subprocess.check_output(['git','rev-parse','HEAD'],cwd=SOURCE/'worktree').decode().strip()==args.source_base_git,'candidate Git base only')
 need(producer['originalPerCommandBudgetSeconds']==900 and len(producer['steps'])==3 and all(r['status']=='PASS' and r['actualExit'] is True and r['exitCode']==0 and not r['timedOut'] for r in producer['steps']),'original producer commands terminal with original budgets')
 need((SOURCE/'target').is_dir() and not (SOURCE/'target').is_symlink(),'explicit artifact target root cannot be a symlink')
 tests=producer['testExecutables'];normals=producer['normalExecutables']
 need(len(tests)==22 and len(normals)==2 and {r['target'] for r in normals}=={'fc-bootstrap','fc-installer-lifecycle'},'exact 22 harness plus two normal GUI artifact set')
 current_payloads=[]
 for row in tests+normals:
  p=Path(row['path'])
  need(p.is_absolute() and p.is_relative_to(SOURCE/'target') and p.resolve().is_relative_to((SOURCE/'target').resolve()) and not p.is_symlink() and p.is_file(),'only explicit producer target PE path')
  need(all(not parent.is_symlink() for parent in p.parents if parent.is_relative_to(SOURCE)),'no symlink artifact ancestor')
  need(row['profile']['test'] is (row in tests) and row['peMachine']==0x14c and row['peOptionalMagic']==0x10b and row['peSubsystem']==(3 if row in tests else 2),'producer test/product PE role')
  data,identity=original(p)
  need(data.startswith(b'MZ') and identity['bytes']==row['bytes'] and identity['sha256']==row['sha256'],'current exact producer artifact bytes')
  identity.update(reason='Explicit final successful producer artifact; identity only. Entire target subtree is otherwise pruned.',producerSha256=sha(producer_bytes))
  current_payloads.append(identity)
 need(len({r['path'] for r in current_payloads})==24,'24 unique explicitly listed target PEs')
 if args.public_certificate_allowlist:
  p=args.public_certificate_allowlist;need(p.resolve().is_relative_to(SOURCE.resolve()) and not p.is_symlink(),'certificate allowlist scope')
  listing=as_json(p.read_bytes());need(listing['sourceSha']==args.source_base_git and listing['status']=='ROOT_REVIEWED_PUBLIC_CERTIFICATES_ONLY','exact reviewed public certificate list')
  CERTIFICATES=listing['certificates'];need(isinstance(CERTIFICATES,dict),'certificate list object')
 BINARY_CONTAINERS=reviewed_binary_containers(args.binary_container_allowlist,args.expected_binary_container_allowlist_sha256)
 paths=source_members()
 rows=[];omitted=[]
 for path in paths:
  need(path.suffix.lower() not in PRIVATE_SUFFIXES,'private material forbidden; contents not read '+str(path))
  if path.suffix.lower() in CERT_SUFFIXES:need(path.relative_to(SOURCE).as_posix() in CERTIFICATES,'unlisted certificate forbidden '+str(path))
 DEST.mkdir(parents=True)
 for path in paths:
  data,row=original(path)
  if row['path'] in BINARY_CONTAINERS:
   row.update(reason='Original JSON/text container embeds one exact reviewed synthetic fixture MSI base64 leaf; full raw identity only. Original bytes retained outside Git; exact semantic derivative is archived and scanned.',binaryContainerQualification=BINARY_CONTAINERS[row['path']])
   omitted.append(row);continue
  if excluded(path,data):
   row['reason']='Compiled/database payload, downloadable original CI ZIP, or build/interpreter cache; original exact identity preserved, bytes not committed.'
   omitted.append(row);continue
  rel=row['path']
  if COMPRESS.fullmatch(rel) and len(data)>=1024*1024:
   packed=gzip.compress(data,compresslevel=9,mtime=0)
   need(gzip.decompress(packed)==data,'gzip encoder roundtrip')
   row.update(storage='gzip_original_bytes',archivePath='compressed-derived-pe/'+rel+'.gz',storedBytes=len(packed),storedSha256=sha(packed))
   output=packed
  else:output=data
  write_new(DEST/row.get('archivePath',rel),output)
  rows.append(row)
 # Independent second pass: reopen each exact source and each stored file;
 # decompression and equality, rather than only trusting the first pass hashes.
 verified=[]
 for row in rows:
  data,identity=original(SOURCE/row['path'])
  need(identity=={k:row[k] for k in ('path','bytes','sha256')},'source changed during archive '+row['path'])
  stored=(DEST/row.get('archivePath',row['path'])).read_bytes()
  if row.get('storage')=='gzip_original_bytes':
   need(len(stored)==row['storedBytes'] and sha(stored)==row['storedSha256'],'stored gzip digest '+row['path'])
   unpacked=gzip.decompress(stored)
  else:unpacked=stored
  need(unpacked==data and len(unpacked)==row['bytes'] and sha(unpacked)==row['sha256'],'independent exact archived/source bytes '+row['path'])
  verified.append({'path':row['path'],'bytes':len(data),'sha256':sha(data),'archivePath':row.get('archivePath',row['path']),
   'storedBytes':len(stored),'storedSha256':sha(stored),'verification':'REOPENED_SOURCE_AND_STORED_BYTES_EQUAL_AFTER_OPTIONAL_GZIP_DECOMPRESSION'})
 for row in omitted+current_payloads:
  _,identity=original(SOURCE/row['path'])
  need(identity=={k:row[k] for k in ('path','bytes','sha256')},'excluded payload identity changed '+row['path'])
 need({p.relative_to(SOURCE).as_posix() for p in source_members()}=={r['path'] for r in rows+omitted},'source membership changed during copy')
 # Target identities are rechecked above, while target/worktree/tools are never recursively enumerated.
 # Credential scan checks UTF8 plus actual UTF16 views, parsed nested JSON raw
 # encodings and gzip decompressed originals. It reports names/kinds only, never
 # matched credential contents. Excluded executable/ZIP payloads are not scanned.
 findings=[];stats={'sourceFiles':0,'storedFiles':0,'gzipFiles':0,'utf16Views':0,'decodedRawPayloads':0,'jsonContainers':0,'textViews':0,'decodedJsonStringViews':0,'cp936ClixmlViews':0};decoder_provenance=[]
 def scan(data,label,depth=0):
  need(depth<=12,'credential decoder depth bound '+label)
  try:views=credential_text_views(data)
  except (AssertionError,UnicodeError):raise AssertionError('unrecognized or invalid retained evidence encoding '+label) from None
  stats['utf16Views']+=sum(encoding.startswith('utf-16-') for encoding,_ in views)
  if any(encoding=='cp936-clixml' for encoding,_ in views):
   stats['cp936ClixmlViews']+=1
   decoder_provenance.append({'path':label,'bytes':len(data),'sha256':sha(data),'encoding':'cp936','strictRoundtrip':True,'validatedXmlRoot':'PowerShell/2004/04 Objs','scope':'Only UTF8-failing actual CLIXML; original bytes preserved.'})
  for encoding,text in views:
   stats['textViews']+=1
   for kind,pattern in PATTERNS.items():
    if pattern.search(text):findings.append({'path':label,'kind':kind,'view':encoding})
   try:value=json.loads(text)
   except (ValueError,RecursionError):continue
   stats['jsonContainers']+=1
   budget=[0]
   def walk(item,trail):
    budget[0]+=1;need(budget[0]<=250000,'JSON credential traversal bound '+label)
    if isinstance(item,dict):
     for key,value in item.items():
      if isinstance(value,str) and (key.lower() in ('base64','encodedcommand','stdoutbase64','stderrbase64') or key.lower().endswith('base64')):
       try:decoded=base64.b64decode(value,validate=True)
       except (ValueError,TypeError):pass
       else:
        need(len(decoded)<=32*1024*1024,'decoded credential view bound '+label)
        need(not binary_magic(decoded),'unreviewed embedded binary forbidden '+label+'::'+trail+'/'+key)
        stats['decodedRawPayloads']+=1;scan(decoded,label+'::'+trail+'/'+key,depth+1)
      elif isinstance(value,str):
       stats['decodedJsonStringViews']+=1;scan(value.encode('utf-8'),label+'::json-string'+trail+'/'+str(key),depth+1)
      elif isinstance(value,(dict,list)):walk(value,trail+'/'+str(key))
    elif isinstance(item,list):
     for index,value in enumerate(item):
      if isinstance(value,str):
       stats['decodedJsonStringViews']+=1;scan(value.encode('utf-8'),label+'::json-string'+trail+'/'+str(index),depth+1)
      else:walk(value,trail+'/'+str(index))
   walk(value,'')
 for row in rows:
  source_data=(SOURCE/row['path']).read_bytes()
  if Path(row['path']).suffix.lower() not in CERT_SUFFIXES:scan(source_data,'source:'+row['path'])
  stats['sourceFiles']+=1
  stored_data=(DEST/row.get('archivePath',row['path'])).read_bytes()
  if row.get('storage')=='gzip_original_bytes':
   stats['gzipFiles']+=1;scan(gzip.decompress(stored_data),'stored-gzip-decoded:'+row['path'])
  elif Path(row['path']).suffix.lower() not in CERT_SUFFIXES:scan(stored_data,'stored:'+row['path'])
  stats['storedFiles']+=1
 need(not findings,'credential findings by path/kind only: '+json.dumps(findings))
 original_count=len(rows);original_bytes=sum(r['bytes'] for r in rows);stored_bytes=sum(r.get('storedBytes',r['bytes']) for r in rows)
 receipt={'schemaVersion':1,'status':'PASS_EXACT_ORIGINAL_AND_STORED_BYTES_AND_DECODED_CREDENTIAL_SCAN',
  'recordedUtc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'sourceRoot':str(SOURCE),'archiveRoot':str(DEST),
  'sourceBaseGit':args.source_base_git,'executedCandidateProducerSha256':sha(producer_bytes),'candidateSourceFreezeSha256':sha(freeze_bytes),'candidateJointFreezeSha256':sha(joint_bytes),'scope':'R75 exact root-admitted independent acceptance scope only; archive does not promote any static/phase/CI result to actual runtime, or Win11 target execution to Win7/10 OS acceptance. No result is promoted beyond the exact final independent acceptance; experimental MSI fixture observations are not production lifecycle acceptance.',
  'sourceFreeze':'Root confirmed all original controllers terminal and no further R75 evidence writes before archive; helper independently reread full source membership and original bytes.',
  'verifiedOriginalFilesBeforeThisReceipt':original_count,'verifiedOriginalBytesBeforeThisReceipt':original_bytes,'storedBytesBeforeThisReceipt':stored_bytes,
  'verified':verified,'excludedCompiledPayloads':omitted,'reviewedBinaryContainerOmissions':BINARY_CONTAINERS,'explicitFinalProducerTargetPeIdentities':current_payloads,'prunedInfrastructureSubtrees':sorted(PRUNED),
  'credentialScan':{'status':'PASS_NO_MATCHES','statistics':stats,'patterns':list(PATTERNS),'findings':[],
   'decoderProvenance':decoder_provenance,
   'coverage':'Strict CP936 only for UTF8-failing actual CLIXML with roundtrip and parsed PowerShell Objs namespace; every retained original and stored counterpart; actual UTF16 views, nested JSON base64/encodedCommand raw streams, all decoded JSON string values, and decompressed gzip. Excluded binary/ZIP bytes have exact identity only, are not committed and are not text-scanned.'},
  'finalReceiptBinding':'This receipt itself is copied byte-for-byte after its independent checks and added to the final archive index; its own identity and the final index identity are printed by the helper after final checks to avoid a circular self-hash.',
  'runtimeEvidence':{'path':args.acceptance_report,'sha256':sha(acceptance_path.read_bytes()),'status':runtime['status']},
  'windowsFinalAudit':{'path':args.readonly_audit,'sha256':sha(final_path.read_bytes()),'status':final['status']},
  'publicCertificates':[public_certificate(SOURCE/name,(SOURCE/name).read_bytes()) for name in sorted(CERTIFICATES)],
  'historicalScopes':'Exact three original recovery JSON/text containers retain identity only outside Git because they embed the reviewed fixture MSI; semantic derivatives omit only that bound base64 leaf with explicit metadata. All other original observations/failures/statuses/phase logs retained without edits. Static reports retaining RUNTIME_NOT_RUN remain static; actual runtime has separate final independent acceptance. No currently failed candidate is implicitly approved.', 'ownedRootMarker':{'path':str(owner),'bytes':len(owner_bytes),'sha256':sha(owner_bytes)}}
 receipt_data=(json.dumps(receipt,ensure_ascii=False,indent=2)+'\n').encode('utf-8')
 # Receipt contains no original result bodies/secrets, only reviewed identities.
 # Scan it too before creating either immutable copy.
 scan(receipt_data,'new-verification-receipt')
 need(not findings,'credential finding in verification receipt')
 write_new(SOURCE/RECEIPT,receipt_data);write_new(DEST/RECEIPT,receipt_data)
 receipt_row={'path':RECEIPT,'bytes':len(receipt_data),'sha256':sha(receipt_data)}
 rows.append(receipt_row)
 index={'schemaVersion':2,'status':'ORIGINAL_BYTES_ARCHIVED','sourceRoot':str(SOURCE),'files':rows,
  'originalFiles':len(rows),'originalBytes':sum(r['bytes'] for r in rows),'storedBytes':sum(r.get('storedBytes',r['bytes']) for r in rows),
  'excludedCompiledPayloads':omitted,'reviewedBinaryContainerOmissions':BINARY_CONTAINERS,'explicitFinalProducerTargetPeIdentities':current_payloads,'prunedInfrastructureSubtrees':sorted(PRUNED),'verificationReceipt':receipt_row,
  'scope':'Exact original R75 source/local compile/metadata/Windows runtime/PowerShell/final-audit results retained; no new Hosted CI or actual MSI acceptance; derived objdump is losslessly gzip-compressed with original and stored bytes/SHA. Unpruned compiled PE/MSI/ZIP/cache payloads excluded by exact identity; worktree/target/tools completely pruned except 24 explicit final producer PE identities. Tools package identities remain in original tool reports. Static and actual OS runtime claims remain separate.'}
 index_data=(json.dumps(index,ensure_ascii=False,indent=2)+'\n').encode('utf-8')
 write_new(DEST/'archive-index-first.json',index_data)
 for row in rows:
  data=(SOURCE/row['path']).read_bytes();stored=(DEST/row.get('archivePath',row['path'])).read_bytes()
  need(len(data)==row['bytes'] and sha(data)==row['sha256'],'final original bytes '+row['path'])
  if row.get('storage')=='gzip_original_bytes':
   need(len(stored)==row['storedBytes'] and sha(stored)==row['storedSha256'],'final stored gzip')
   stored=gzip.decompress(stored)
  need(stored==data,'final original versus archive '+row['path'])
 need({p.relative_to(SOURCE).as_posix() for p in source_members()}=={r['path'] for r in rows+omitted},'final complete source membership')
 need({p.relative_to(DEST).as_posix() for p in DEST.rglob('*') if not p.is_dir()}=={r.get('archivePath',r['path']) for r in rows}|{'archive-index-first.json'},'final exact archive membership')
 print(json.dumps({'status':'PASS','originalFiles':index['originalFiles'],'originalBytes':index['originalBytes'],'storedBytes':index['storedBytes'],
  'excludedCompiledPayloads':len(omitted),'compressedObjdumpFiles':stats['gzipFiles'],
  'indexSha256':sha(index_data),'verificationReceiptSha256':sha(receipt_data),'credentialScan':'PASS_NO_MATCHES',
  'decodedRawPayloads':stats['decodedRawPayloads'],'utf16Views':stats['utf16Views'],'sourceRemoved':False}))

if __name__=='__main__':main()
