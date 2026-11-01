from pathlib import Path
import sys,hashlib,json,gzip,stat,re
source=Path(sys.argv[1]);dest=Path(sys.argv[2]);assert source.is_dir() and not dest.exists();dest.mkdir(parents=True)
rows=[];excluded=[]
for p in sorted(source.rglob('*')):
 if p.is_dir():continue
 assert stat.S_ISREG(p.lstat().st_mode),str(p)
 rel=p.relative_to(source).as_posix();d=p.read_bytes();sha=hashlib.sha256(d).hexdigest();row={'path':rel,'bytes':len(d),'sha256':sha}
 if p.suffix.lower() in {'.exe','.zip','.pdb','.dll','.lib','.rlib','.rmeta','.pyc'} or '__pycache__' in p.parts:
  row['reason']='Compiled payload, downloadable CI archive, or interpreter cache; original identity retained, no build cache committed';excluded.append(row);continue
 if re.fullmatch(r'(modern|win7)-artifact-original/pe-[0-9]+\.objdump\.stdout\.log',rel):
  stored='compressed-derived-pe/'+rel+'.gz';packed=gzip.compress(d,compresslevel=9,mtime=0);assert gzip.decompress(packed)==d;row['storage']='gzip_original_bytes';row['archivePath']=stored;row['storedBytes']=len(packed);row['storedSha256']=hashlib.sha256(packed).hexdigest();out=packed
 else:stored=rel;out=d
 q=dest/stored;q.parent.mkdir(parents=True,exist_ok=True);q.open('xb').write(out);assert q.read_bytes()==out;rows.append(row)
r={'schemaVersion':2,'status':'ORIGINAL_BYTES_ARCHIVED','sourceRoot':str(source),'files':rows,'originalFiles':len(rows),'originalBytes':sum(x['bytes'] for x in rows),'storedBytes':sum(x.get('storedBytes',x['bytes']) for x in rows),'excludedCompiledPayloads':excluded,'scope':'Exact source/raw results retained; derived PE objdump logs are losslessly gzip-compressed with both original and stored hashes. Executables and ZIP cache not committed; compilation and actual OS claims remain separate.'};q=dest/'archive-index-first.json';q.open('x').write(json.dumps(r,indent=2)+'\n');print(json.dumps({'files':len(rows),'originalBytes':r['originalBytes'],'storedBytes':r['storedBytes'],'indexSha256':hashlib.sha256(q.read_bytes()).hexdigest()}))
