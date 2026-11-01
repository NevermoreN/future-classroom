from pathlib import Path
import subprocess,hashlib,json,shutil,ast,difflib
P=Path(__file__).parent

def info(p):
 b=p.read_bytes();return {'path':str(p),'bytes':len(b),'sha256':hashlib.sha256(b).hexdigest()}
def put(p,b):
 with p.open('xb') as f:f.write(b)
compiler=shutil.which('i686-w64-mingw32-gcc');assert compiler
source=P/'registry-value-intent.c';exe=P/'registry-value-intent.exe';assert not exe.exists()
argv=[compiler,'-std=c11','-Os','-Wall','-Wextra','-Werror','-ffreestanding','-fno-builtin','-nostdlib','-mconsole','-Wl,--entry,_entry','-o',str(exe),str(source),'-ladvapi32','-lktmw32','-lkernel32','-lgcc']
c=subprocess.run(argv,cwd=P,capture_output=True,timeout=120)
put(P/'compile-second.stdout.log',c.stdout);put(P/'compile-second.stderr.log',c.stderr)
r={'status':'PASS_LOCAL_CROSS_COMPILE_ONLY' if c.returncode==0 else 'FAIL_LOCAL_CROSS_COMPILE','argv':argv,'cwd':str(P),'actualExit':True,'exitCode':c.returncode,'source':info(source),'stdout':info(P/'compile-second.stdout.log'),'stderr':info(P/'compile-second.stderr.log'),'windows':'NOT_RUN'}
if c.returncode==0:r['executable']=info(exe)
put(P/'compile-second.json',(json.dumps(r,ensure_ascii=False,indent=2)+'\n').encode())
print(json.dumps(r,ensure_ascii=False))
raise SystemExit(c.returncode)
