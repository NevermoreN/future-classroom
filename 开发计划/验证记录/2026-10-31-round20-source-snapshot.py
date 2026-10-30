import hashlib,json,subprocess,sys
from pathlib import Path
paths=subprocess.check_output(["git","ls-files","--cached","--others","--exclude-standard","-z"]).decode().split("\0")
files=[]
for name in sorted(set(paths)):
 if not name or name.startswith(("开发计划/","UI设计/")): continue
 path=Path(name)
 if path.is_file(): files.append({"path":name,"sha256":hashlib.sha256(path.read_bytes()).hexdigest()})
body=json.dumps(files,ensure_ascii=False,separators=(",",":"))
result={"scope":"round20 code/frontend/build/package inputs; development-plan evidence, UI design atlas and caches excluded","fileCount":len(files),"files":files,"sha256":hashlib.sha256(body.encode()).hexdigest()}
Path(sys.argv[1]).write_text(json.dumps(result,ensure_ascii=False,indent=2)+"\n")
print(result["fileCount"],result["sha256"])
