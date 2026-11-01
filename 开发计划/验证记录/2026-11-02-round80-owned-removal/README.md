本目录保存本轮已完成的有限开发证据，候选源码为 `33c11f5e41cce727746aeb5b5801b0e87eaf0f41`。十六份核心原报告按原路径外放，逐项清单和二进制身份见 `source-manifest-first.json`、`archive-index-first.json`；其余原文本在 `evidence-originals-first.tar.gz`。

Win11 生命周期组件 69 项包含 controlled 六项。首次 feature Clippy 实际 101、后续 feature/default Clippy 实际 0 均按原件保留。原 Pending 只读捕获本身成功，首次传输 inventory 失败，后续完整取回及其新临时目录清理成功；原失败未被重分类。显式测试移除经过两个新原生事务，原 B 返回 0、两 G 与线程归还、独立产品及 Pending 缺失核对后通过；其审计最后阶段为 RemovalAuthorized，审计本身不证明删除，旧事务 End 始终 UNKNOWN。独立清理随后精确删除原空 gate/root 及新旧两个已下载 nonce。

正常 GNU 开发包随后真实安装、精确 LICENSE 缺损修复、卸载三阶段通过；十四项恢复，产品、组件、注册、Pending、gate 及本轮 nonce 最终缺失均有原件。此结果与此前 c508 卸载失败分开记录；此前完整 376 原件归档仅作引用，不被重写。另附固定 7c CI 的现代 MSVC 编译检查通过且运行 NOT_RUN、Win7 目标真实 Clippy 失败诊断；这些不能代替 Win7/Win10 实机、正式 MSVC 产品联动、ARP 或生产恢复证明。

实际压缩包含 584 份原文本，共 7,919,248 字节，压缩后 1,466,111 字节；逐成员重放和十六份外放报告逐字节一致。扫描原字节、严格 UTF-16/CP936、CLIXML、JSON/JSONL 和嵌套 base64，未发现凭据。仅三份 MSI 日志各一处控制字符有精确结构白名单；声明 base64 中的原 Pending 和两个保护 ACL 使用精确父路径/字节数/SHA、JSON 位置、解码身份和完整原生结构派生视图扫描。未知控制符仍拒绝。三个版本 stdout 虽以 jsonl 命名却为普通文本，采用精确身份限定并逐行完整扫描；首归档因此负向停止的原 controller/收据保留，第二次实际归档成功。

scope.hex/bin 未读取或收入，PE/ZIP 仅保留身份；两个官方 CI ZIP 内容未打开。重复 PE 反汇编及重复 API ANSI 日志仅留身份，完整有效 Win7 Clippy 下载诊断保留。未遍历活动 a470、W2、后续 postwrite、工作树、target、工具或 Web 缓存。本归档未连接 Windows、运行 Cargo、删除原件、修改进度或提交推送。
