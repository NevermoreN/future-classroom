# R80：实际 post-write 强制失败及回滚验证

本记录仅归档 Win11 已授权测试机上这一有限步骤，不修改计划进度，不作为生产发布、ARP 卸载、通用崩溃恢复或 Win7/Win10/希沃实机通过证据。

核心原件：[实际运行报告](controlled-post-write-runtime-first/report.json)，SHA256 `c877f6972c67ebd07ef6eb28d3aead351281978e2a9fe5b3ff9cf2f4a7a56387`。外层原进程 actual0、已归还，12.2340525秒；Windows 原600秒整体预算内实际9331毫秒。五个 B 的真实业务返回依次1603/0/1603/0/0，五个实际捕获的原 G 均 Wait0、退出0、checked-close，并与原 B 实例匹配。业务1603与物理归还分别记录。

第一次 forced install 在全新 Absent 前像下完成实际写入见证后失败，产品、文件、注册、组件标记及 Pending 恢复 Absent。随后 canonical install0，全14项健康；仅以原 HANDLE 精确删除自有 LICENSE，并将原 PROBE-README.txt 写为51字节固定测试前像。forced repair 的实际失败回滚恢复 LICENSE 缺失、PROBE 完整51字节及其余12项字节，注册、组件和标记前像一致。最后 canonical repair0 恢复全14项，再 canonical uninstall0 完整移除。

两次 forced 场景的同一原 MSI 日志实际出现 guard Return1 → InstallExecute Return1 → full14 只读 witness Return1 → 固定 Type19 fail Return3 → ScriptType2 rollback header。只读 witness 的实际编译源、由完成的 canonical 包生成的14项清单、实际 DLL 身份和 MSI stream 字节闭包均保留。witness 在该时刻证明全14项健康；损坏修复中只有两项不同，不能宣称其余12项也发生替换。

当前产品没有直接序列化 Begin/End 数字。各阶段 `terminalEvidence` 明确为固定源码路径、G0、checked controls、实际效果及 PendingAbsent 的推论，`beginActualNumber/endActualNumber=NOT_SERIALIZED`。外部原 G 线程能力没有单独捕获，仍为代码路径推论；日志和 UI 回调不冒充原 API 直接返回收据或权限。

最终20条精确日志派生 Installer 路径、2条实际查询的缓存路径、产品/全14项/注册/标记/Pending/gate 均 Absent。46文件、8目录已逐 SHA 下载核验，再精确删除本次 nonce；用户持续保留的 SSH 服务与授权不在归档操作范围。

`evidence-originals-first.tar.gz` 保存291份白名单文本原件，共3,248,203字节；压缩528,084字节，SHA256 `7bafbc66326ff2df4df568180ccd7425cf848feb0dd997073f38bd500159c519`。tar 内每个成员完成来源 bytes/SHA 核对和全量解压逐字节回放。八份核心原报告另外直接保存，未经改写。37项 PE/MSI/DLL/CAB/raw payload/原始 MSI stream 只存身份，不收二进制、学校材料、scope.hex/bin、凭据、活动 CI/W2/target/tools 或包2重复大源码。完整白名单、排除身份和来源见 `source-manifest-first.json` 与 `archive-index-first.json`。

派生脚本/witness 源、85条实际工具原stdout/stderr（原始 stream 二进制输出仅身份）、controller Python/PS/C#、9项纯资格拒绝检查、默认 NOT_RUN、首次 root AST 包装器 `KeyError(text)` 失败、随后7块实际 PS AST0、实际五份 MSI 日志及 root 回执均保留。准备阶段 NOT_RUN 状态与随后实际 PASS 分开，未改写历史报告。

凭据扫描复用已提交 R79 扫描器的精确成员及 SHA，严格解析 UTF8/UTF16/CP936、CLIXML、JSON/JSONL 字符串及受限嵌套 base64。五份 MSI 日志中四份各含固定 `DigitalSignature` 控制字符，卸载日志无此控制字符；另有两份固定 stream 名称列表、两份固定23字节 `_ForceCodepage` IDT，以及三份同一派生报告中各两个 `SummaryInformation` argv 子视图。例外均绑定精确路径、字节数、SHA、行/字段结构，完整原文、可见边界和去控制字符拼接视图均扫描，无通用控制字符宽容。

首次归档扫描严格拒绝真实 JSON argv 结构字符，actual1 与原 stderr 保留为 `archive-operation-first.*`；未生成 tar。第二版仅补齐六个父文档 SHA/JSON 路径/19字节值的精确白名单，actual0、完整回放、无凭据匹配，见 `archive-operation-second.*` 和 `archive-verification-first.json`。归档器未连接 Windows、未运行 Cargo、未读取私钥、未删除源材料、未修改进度或执行 Git 操作。
