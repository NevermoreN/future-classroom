"""Finite local manifest construction only; no Windows/Cargo/Git actions."""
from pathlib import Path
import hashlib,json,re,runpy,tarfile
M=Path('/workspace/共享/Eliora/未来教室计划');D=Path(__file__).resolve().parent;R=Path('/tmp/fc-round80-controlled-repair')
def sha(b):return hashlib.sha256(b).hexdigest()
def raw(j):return (json.dumps(j,ensure_ascii=False,indent=2)+'\n').encode()
def main():
 with tarfile.open(M/'开发计划/验证记录/2026-11-02-round79-msi-maintenance/evidence-originals-first.tar.gz') as t:b=t.extractfile('archive-preparation-second/archive-second.py').read()
 assert sha(b)=='da2906c450ea8c2ac7d18e358507023f519dbcc6faf1a9ac1abb1ae16af8c30f';ns={'__name__':'scanner_for_selector'};exec(compile(b,'<committed-scanner>','exec'),ns)
 paths,raw_payloads=runpy.run_path(str(D/'select-first.py'))['collect']();originals=[];excluded=[];forms=[];stream_forms=[];codepage_forms=[]
 stream_expected={'original-streams.stdout.log':['payload.cab','Binary.FcControlledGuardDll','Binary.FcGuardClientTemplate','\x05SummaryInformation'],'derived-streams.stdout.log':['payload.cab','Binary.FcPostWriteWitness','Binary.FcControlledGuardDll','Binary.FcGuardClientTemplate','\x05SummaryInformation']}
 for name in paths+raw_payloads:
  p=R/name;data=p.read_bytes();row={'path':name,'bytes':len(data),'sha256':sha(data)};kind=ns['binary_kind'](data)
  stream=bool(re.fullmatch(r'controlled-post-write-package-first/(?:original-stream-\d+|derived-stream-\d+|derived-witness-stream)\.stdout\.log',name))
  if name in raw_payloads or kind or stream or p.suffix.lower() in ns['BINARY_SUFFIXES']:
   row['kind']='RAW_PAYLOAD_IDENTITY_ONLY' if name in raw_payloads else 'RAW_MSI_EXTRACT_STREAM_IDENTITY_ONLY' if stream else kind or 'BINARY_SUFFIX';excluded.append(row);continue
  originals.append(row)
  if name.startswith('controlled-post-write-package-first/') and p.name in stream_expected:
   text=data.decode('ascii');assert text=='\n'.join(stream_expected[p.name])+'\n';assert text.count('\x05')==1
   stream_forms.append({**row,'textForm':'EXACT_ASCII_MSI_STREAM_NAME_ENUMERATION','linesEscaped':[v.replace('\x05',r'\x05') for v in stream_expected[p.name]],'controlLineNumber':len(stream_expected[p.name])});continue
  if name in ('controlled-post-write-package-first/original-table-_ForceCodepage.stdout.log','controlled-post-write-package-first/derived-table-_ForceCodepage.stdout.log'):
   assert data==b'\r\n\r\n0\t_ForceCodepage\r\n\0';codepage_forms.append({**row,'textForm':'EXACT_FORCECODEPAGE_IDT23_WITH_SINGLE_NUL_TERMINATOR','exactBytesHex':data.hex()});continue
  try:ns['text_views'](data,'source:'+name)
  except AssertionError as e:
   assert name.endswith('.LOG') and '/download/' in name,(name,str(e));assert data.startswith(b'\xff\xfe');text=data.decode('utf-16');bad=[(i+1,line) for i,line in enumerate(text.split('\n')) if any(ord(c)<32 and c not in '\r\t' for c in line)]
   assert len(bad)==1 and text.count('\x05')==1 and re.fullmatch(r'MSI \(s\) \([0-9A-F]{2}:[0-9A-F]{2}\) \[[0-9]{2}:[0-9]{2}:[0-9]{2}:[0-9]{3}\]: Note: 1: 2262 2: \x05DigitalSignature 3: -2147287038 \r',bad[0][1]),(name,bad)
   forms.append({**row,'textForm':'EXACT_UTF16_MSI_DIGITAL_SIGNATURE_STREAM_NOTE','controlLineNumber':bad[0][0],'controlLineEscaped':bad[0][1].replace('\x05',r'\x05')})
 assert len(forms)==4 and len(stream_forms)==2 and len(codepage_forms)==2,(len(forms),len(stream_forms),len(codepage_forms))
 manifest={'status':'FROZEN_FINITE_POST_WRITE_EVIDENCE_WHITELIST','originals':sorted(originals,key=lambda r:r['path']),'originalCount':len(originals),'originalBytes':sum(x['bytes'] for x in originals),'excludedBinaryIdentities':sorted(excluded,key=lambda r:r['path']),'rawPayloadIdentityPaths':raw_payloads,'selectorSha256':sha((D/'select-first.py').read_bytes()),'excludedWholeTrees':['active checkout/W2/target/tools/CI','canonical package-second source tree except report','all scope.hex/scope.bin/private/cert/school material'],'scope':'Five actual Win11 Setup calls and one owned damage operation only; no source deletion'}
 (D/'source-manifest-first.json').write_bytes(raw(manifest));(D/'text-forms-first.json').write_bytes(raw({'status':'EXACT_FOUR_NOTES_IN_FIVE_MSI_LOGS_TWO_ENUMERATIONS_TWO_FORCECODEPAGE_TABLES','reviewedMsiLogs':[r for r in originals if '/download/' in r['path'] and '-msi-logs/' in r['path'] and r['path'].endswith('.LOG')],'files':forms,'streamEnumerations':stream_forms,'forceCodepageTables':codepage_forms,'otherControls':'REJECT_NO_GENERIC_EXCEPTION'}))
 print(json.dumps({'text':len(originals),'textBytes':manifest['originalBytes'],'binaryIDs':len(excluded),'forms':len(forms),'streamForms':len(stream_forms),'manifestSHA':sha((D/'source-manifest-first.json').read_bytes())}))
if __name__=='__main__':main()
