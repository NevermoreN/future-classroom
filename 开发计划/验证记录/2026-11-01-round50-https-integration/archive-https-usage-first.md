# R47–R51 非二进制原件归档准备

当前只准备 helper 和合成模型；没有扫描五轮最终集合、没有创建主仓归档、没有封存或清理源根。R50 原始 900 秒 Host 超时必须与后续缓存编译重跑终态分别保留。不能把本文件或模型通过当作 Host/Windows/产品通过。

`archive-https-rounds.py` 默认五轮，也接受重复的 `--round 47` 等参数以选定完整单轮。五个 owner 来自实际原件；R48 使用 `.owner.json`，不会另造 `.owner`。目标固定在主仓 `开发计划/验证记录/2026-11-01-round47-https-settings`、`round48-certificate-query`、`round49-https-ui`、`round50-https-integration`、`round51-browser-diagnostics`（各带 `2026-11-01-` 前缀）。

准备扫描只能由主协调者在需要时执行；本阶段没有执行：

```sh
python3 /tmp/fc-round50-https-integration-evidence/archive-https-rounds.py \
  --report /tmp/https-rounds-preparation-next.json
```

准备状态为 `PREPARED_ONLY_NOT_SEALED`，并不声明消费者结束。报告及每轮 `.round47.json` 等均 CreateNew。发现敏感或类型问题时只报告位置/类型，不输出发现值。

全部最终命令实际终态、验证过的源字节已集成后，主协调者在五个 evidence 根之一创建一个**真实** final-authority JSON。它需要：

- `schema`: `fc.https-rounds.final-archive-authority/v1`
- `status`: `ALL_CONSUMERS_FINISHED_SOURCE_INTEGRATED`
- `activeConsumers`: 空数组。
- `sourceIntegration`: `EXACT_VALIDATED_BYTES_INTEGRATED`
- `gitHead`: 当时完整 40 位小写 SHA；它不会替代源码差异的原 SHA 清单。
- `proofs`: 5–100 行，每行具有 `role`、`path`、`sha256`、`status`，可选 `cleanupRequired: true`。path 必须是五个 owned evidence 根内的普通 JSON，SHA 与原 bytes 完全一致，status 必须与原件相同且以 PASS 开头。cleanupRequired 时原件还必须有 cleanup PASS。至少覆盖 `source-integration`、`host-full-matrix`、`windows-runtime`、`browser-final`、`readonly-final` 五个角色。

这是一份协调者显式声明，加上被列原件的精确哈希与 PASS 检查；不是脚本重新编译/执行或从基线 Git SHA 推断未提交代码通过。不要提前构造成功声明。

真实封存命令示意（路径必须换成主协调者真正创建的 final-authority；此处不执行）：

```sh
python3 /tmp/fc-round50-https-integration-evidence/archive-https-rounds.py \
  --write-archive --all-consumers-finished \
  --final-proof /tmp/fc-round50-https-integration-evidence/final-archive-authority-first.json \
  --report /tmp/https-rounds-archive-final-first.json
```

写入报告必须在所有源根和主仓之外，避免自递归；每个目标必须尚不存在。五轮全部先扫描，逐个创建归档，再全局复核来源 bytes/成员和 final proof。每轮有精确 INDEX 和外部独立报告；总报告只有 `PASS_EXACT_NONBINARY_ARCHIVES` 才说明整次成功。中途失败不会删除或覆盖部分归档，`mainModified` 与 `partialArchiveMayRemain` 明示实际情况。

每轮路径 ≤30000、普通文件 ≤10000、单件 ≤64MiB、非二进制总 ≤512MiB；五轮合计 ≤1GiB。source 快照/helper/model/command/stdout/stderr/原失败/原 raw/PS controls/owner 均在允许类型内。private/key/database/LIVE control、EXE/ZIP/native binary/cache 内容从不读取或哈希；排除目录整棵不遍历，明确不声称验证其内部成员。只允许原 owned `tls-public.pem`；未知 PEM 不读取。所有读取通过逐祖先 O_NOFOLLOW，发现符号链接拒绝。不会沿 evidence 中任意外部引用读取文件；仅 final-authority 明确列出的安全同根 proof 会被检查。

递归 JSON/PowerShell base64 扫描继承 R46 的有界策略；真实敏感发现将阻止写入，显式已退休的合成能力值仅按原拥有者清理证据分类。扫描不能证明任意编码绝无秘密。原 R46 NETWORK_LOAD_FAILED 仍根因未定，后续有限 PASS 不等于已修复。

脚本不调用 SSH/Cargo/Windows，不开进程/网络，不清理任何真实根、target、数据库、证书、SSH 或授权。最终源目录清理由主协调者在提交与字节证明后另行执行。
