from pathlib import Path
import runpy,json,hashlib
D=Path(__file__).resolve().parent
m=runpy.run_path(str(D/'admission.py'))
for label in ('modern','win7'):
 j=m['qualify'](label);p=D/(label+'-report.json');p.write_text(json.dumps(j,indent=2)+'\n');print(json.dumps(dict(label=label,status=j['status'],sources=j['committedInputRowsVerified'],commands=len(j['allCommands']),reportSha256=hashlib.sha256(p.read_bytes()).hexdigest())),flush=True)
