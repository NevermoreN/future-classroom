# R46–R51 最终收尾（仅13个固定自有根）

本目录包含独有 owner、roots.json 与 finalize.py。当前只准备工具；未执行 proof/删除。根协调者已提交归档与计划后，必须指定真实当前完整 HEAD；本工具仍独立重验，不单凭外部 PASS 摘要清理。

1. 六份 INDEX 所有源成员及各 INDEX.json/INDEX.md：本地完整成员集合与 Git HEAD 完整成员集合相同，逐块对 Git blob 字节完全相等；预期1372件（1360原件＋12索引）。实际数量在 proof 计算。
2. 六 evidence 根：原 owner 精确匹配，所有非二进制源字节等于已验证归档；R46额外44个EXE＋2份ZIP按已归档原审计逐项 SHA；未列文件/缺项/私钥/数据库/LIVE control/链接都阻断。
3. 四 worktree：注册路径、branch、原HEAD与.git指针精确匹配；无 assume-unchanged/skip-worktree。staged/unstaged/untracked/ignored 文件全部清点，每个变化字节必须等于当前HEAD同路径或已验证archive某成员；旧修改保留完整archive出处。任何不明文件、删除或未证明字节阻断。
4. 三输出根：原owner与精确topology；独立programs根的22EXE匹配已归档compiler-artifact。两个cache仅做可再生产物lstat清点，不当产品协议建模；拒特殊文件/链接/私钥/DB，无内容递归读取。记录dev/inode去重与mtime/ctime成员指纹。
5. 物理引用：仅实际owner UID1026的cwd/exe/fd/mmap及已归档ownedProcessGroup；不可完整检查的本UID进程阻断。他UID仅记NOT_INSPECTED，不宣称全系统无引用，不提权、不发信号。映射/fd同时按路径和dev/inode匹配。
6. `proof`只写本目录新报告。`cleanup`要求原proof及显式确认，重新完整证明同HEAD/成员/字节/owner，并逐根删除前再次lstat与同UID引用检查。单一工具flock拒并发。部分失败诚实保留动作，不自动重试。

```sh
python3 /tmp/fc-round46-51-finalization-review/finalize.py proof \
  --commit <真实当前完整HEAD> \
  --report /tmp/fc-round46-51-finalization-review/proof-first.json

# 仅在原proof通过并由根协调者决定执行后：
python3 /tmp/fc-round46-51-finalization-review/finalize.py cleanup \
  --commit <与proof相同且仍当前的完整HEAD> \
  --proof /tmp/fc-round46-51-finalization-review/proof-first.json \
  --confirm-thirteen-owned-roots \
  --report /tmp/fc-round46-51-finalization-review/cleanup-first.json
```

不清理main、全局cache、repo target/debug、SSH、/mnt/data、R52及本收尾目录；不删分支refs。没有原子冻结其他进程未来启动的保证；明确以协调者所有本轮消费者已结束和每步实时同UID负向预检为边界。任何失败先保留报告、审查原因，再另编号处理，不改写原FAIL。
