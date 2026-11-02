# Win7 目标固定分块写法修订及 0cdf 构建核心证据

当前修订源码提交为 `c8ba14d29f0a2c61a56389c8b47db1bc5f49d664`，父提交 `0cdfd5a70fd90df3e04a570c8dcb22e48162b9e1`。恰两处固定尺寸分块表达式改用 `as_chunks` / `as_chunks_mut`；未增加 lint allow，未改变布局、权限、句柄上限或期限。GNU 全 workspace all-targets 严格 Clippy、格式、12 crate/33 边架构及 diff 四命令实际退出 0，原子进程均已归还，独占 target 已清理。检查报告的 1774 条 before/after 是递归 Rust/Cargo 文件快照，包含历史验证目录中的副本，**不是编译器实际读取输入计数**。

此前 0cdf 的现代 MSVC run 41387127138 成功，51 条原命令均 0；Win7 目标 run 41387130749 失败，53 条中 Clippy 为 101、另外 52 条 0。原错误为 `predecessor.rs:48` 的 `chunks_exact` 和 `controlled_owner.rs:219` 的 `chunks_exact_mut` 触发新严格 lint；旧 WindowRole 位置错误已不再出现在该批失败中。原失败完整 stdout/stderr/result 另存本目录，不改写为成功。

root 已独立核完整 GitHub ZIP 摘要和每个成员、655 项实际 checkout 输入对应 Git blob 或唯一 LF→CRLF 转换、19926 条完整 Git 元数据以及原命令日志摘要。两个 ZIP 只记身份，完整原 ZIP 继续保留在仓库外；**本目录仅保存 source-copy-ledger 列出的有限核心全文，未声称归档两 ZIP 的全部原日志**。资格脚本与固定参数完整保存，原下载预备命令因非交互 stdin EOF 未执行下载的包装器错误没有保存原 stderr，仅为会话工具事实，不补造文件。

c8ba 修订尚待新的现代/Win7 MSVC 构建，不能用旧 0cdf 或 GNU 检查代替。所有本批新 Windows 原生运行均 NOT_RUN；Win7 SP1 x86、Win10 各自系统实机状态仍 NOT_RUN。此修订不完成 G02 命令留存、G03 浏览器或整项目交付。

原 Windows JSON 的 CRLF 及 Clippy stderr 第304行的原 Cargo 描述尾空格完整保留；Git 样式检查按单命令 CRLF 模式运行，仅排除逐摘要核同的该一份原始日志。该有限排除不改变源码、扫描规则或归档原字节，见 raw-whitespace-preservation.json。
