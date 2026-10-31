from pathlib import Path
import tempfile,zipfile,json,hashlib,runpy,re,shutil
root=Path.cwd();ev=Path('/tmp/fc-round37-ci-evidence');base=ev/'win7-artifact-raw';out=ev/'win7-all-pe-independent-first.json';assert not out.exists()
rows=json.loads((base/'executables.json').read_text(encoding='utf-8-sig'));assert len(rows)==22;audit=runpy.run_path(str(root/'scripts/audit-pe.py'))['audit_pe'];owned=Path(tempfile.mkdtemp(prefix='fc-round38-win7-pe-'));r={'status':'RUNNING','scope':'Full original failed-Clippy Win7 candidate artifact22EXEs independent bytes/SHA/PE32/static CRT/subsystem6.1 inspection only. These executables are never run, and this does not turn failed CI or untested Win7 OS into PASS.','artifacts':[]}
try:
 with zipfile.ZipFile('/tmp/fc-round37-win7-original.zip') as z:
  for row in rows:
   name=Path(row['relative_path']).name;data=z.read(row['relative_path']);assert len(data)==row['bytes'] and hashlib.sha256(data).hexdigest()==row['sha256'];p=owned/name;p.write_bytes(data);pe=audit(p);assert not pe['structural_errors'];offset=int.from_bytes(data[60:64],'little');assert data[offset:offset+6]==b'PE\0\0L\x01';optional=offset+24;major=int.from_bytes(data[optional+48:optional+50],'little');minor=int.from_bytes(data[optional+50:optional+52],'little');assert (major,minor)==(6,1)
   assert not any(d.startswith(('vcruntime','msvcp','api-ms-win-crt','libgcc_s_','libstdc++','libwinpthread')) or d=='ucrtbase.dll' or re.fullmatch(r'msvcr[0-9]+\.dll',d) for d in pe['imports'])
   r['artifacts'].append({**row,'independentSubsystem':[major,minor],'independentPeAudit':pe});p.unlink()
 r['status']='PASS_STATIC_INSPECTION_ONLY'
except Exception as e:r.update(status='FAIL',errorKind=type(e).__name__)
finally:shutil.rmtree(owned);r['ownedScratchRemoved']=not owned.exists();out.write_text(json.dumps(r,indent=2)+'\n')
print(json.dumps({'status':r['status'],'executables':len(r['artifacts']),'ownedScratchRemoved':r['ownedScratchRemoved']}))
raise SystemExit(0 if r['status']=='PASS_STATIC_INSPECTION_ONLY' else 1)
