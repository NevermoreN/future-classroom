import subprocess,sys
from pathlib import Path
exe=Path(sys.argv[1]);prefix=sys.argv[2]
for mode in ['application','recovery','autostart','reinitialization']:
 base=Path('/tmp/fc-round25-evidence')/(prefix+'-'+mode+'-stage1')
 args=['python3','scripts/windows-probe.py','--host','elior@192.168.10.212','--identity','/config/.local/share/future-classroom-test-access/win11-192.168.10.212','--exe',str(exe),'--'+mode,'--native-seconds','1','--report',str(base.with_suffix('.json'))]
 with base.with_suffix('.log').open('w') as log:
  result=subprocess.run(args,stdout=log,stderr=subprocess.STDOUT)
 print(mode,'actualExitCode='+str(result.returncode),flush=True)
 if result.returncode:sys.exit(result.returncode)
