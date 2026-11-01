"""Only path+original SHA+exact JSON trail+decoded SHA-bound native structures."""
import base64,hashlib,json,struct
EXPECTED_SID=bytes.fromhex('01050000000000051500000067f039e8ca6e546fa21f8475e9030000')
PENDING_SHA='6b385a9577182edeb9abc51bfa3d317386f0f070c126fd545534d1c437f9459d'
def digest(b):return hashlib.sha256(b).hexdigest()
def structural(data):
 if len(data)==106 and digest(data)==PENDING_SHA:
  header=b'FC-INSTALL-PENDING\0v1\0'+b'99378807-098D-5F04-8D4D-F181DFA218D5\0';assert data.startswith(header);tail=data[len(header):];assert len(tail)==47 and tail[:16]!=bytes(16) and tail[16]==5 and int.from_bytes(tail[17:19],'little')==len(EXPECTED_SID) and tail[19:]==EXPECTED_SID
  return 'EXACT_ORIGINAL_PENDING_V1',{'family':'99378807-098D-5F04-8D4D-F181DFA218D5','nonceHex':tail[:16].hex(),'tag':5,'sidHex':EXPECTED_SID.hex()}
 # Windows SECURITY_DESCRIPTOR_RELATIVE: exact protected user/System DACL,
 # complete contiguous owner SID and two closed ACCESS_ALLOWED_ACE records.
 assert len(data)==112 and data[:4]==b'\x01\x00\x04\x90';owner,group,sacl,dacl=struct.unpack_from('<IIII',data,4);assert (owner,group,sacl,dacl)==(20,0,0,48) and data[20:48]==EXPECTED_SID
 rev,sbz,size,count,sbz2=struct.unpack_from('<BBHHH',data,48);assert (rev,sbz,size,count,sbz2)==(2,0,64,2,0)
 offset=56;aces=[]
 for sid in [EXPECTED_SID,bytes.fromhex('010100000000000512000000')]:
  atype,flags,asize,mask=struct.unpack_from('<BBHI',data,offset);assert atype==0 and flags in (0,3) and asize==8+len(sid) and mask==0x1f01ff and data[offset+8:offset+asize]==sid;aces.append({'flags':flags,'mask':mask,'sidHex':sid.hex()});offset+=asize
 assert offset==len(data) and aces[0]['flags']==aces[1]['flags']
 return 'EXACT_PROTECTED_USER_SYSTEM_SECURITY_DESCRIPTOR_RELATIVE',{'control':0x9004,'ownerSidHex':EXPECTED_SID.hex(),'aces':aces}
def readable_views(data):
 kind,fields=structural(data)
 return [('strict-native-structure',json.dumps({'kind':kind,'bytes':len(data),'sha256':digest(data),'completeHex':data.hex(),'fields':fields},sort_keys=True)),('native-visible-boundary',''.join(chr(b) if 32<=b<=126 else '[BYTE-%02X]'%b for b in data)),('native-ascii-concatenated',''.join(chr(b) for b in data if 32<=b<=126))]
def attach(ns,forms,originals):
 parent={r['path']:r for r in originals};allowed={(r['sourcePath'],r['trail']):r for r in forms['views']};original=ns['text_views'];counts={'nativeStructuralDecodedViews':0}
 def wrapper(data,label=None):
  if label:
   prefix=next((v for v in ['source:','stored-tar-gzip-decoded:','promoted:'] if label.startswith(v)),None)
   if prefix:
    rest=label[len(prefix):];rel,sep,trail=rest.partition('::');row=allowed.get((rel,trail))
    if row is not None:
     assert parent[rel]['sha256']==row['sourceSha256'] and parent[rel]['bytes']==row['sourceBytes'];assert len(data)==row['decodedBytes'] and digest(data)==row['decodedSha256'];kind,_=structural(data);assert kind==row['textForm'];counts['nativeStructuralDecodedViews']+=1;return readable_views(data)
  return original(data,label)
 ns['text_views']=wrapper;return counts
