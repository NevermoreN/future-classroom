# Round 80：a470 官方 MSVC 构建与 Win11 原生组件回归

本目录只保存 `a4702b56bff66da5f5cbe74551d8751eb33fa520` 的有限原始证据，不表示发布准入。两份官方 CI 报告均为 `COMPILE_CHECKS_PASSED_RUNTIME_NOT_RUN`：正常 product/helper/DLL 与测试由同源 MSVC 构建；CI 编译成功与随后 Windows 11 上的实际运行分别记录。

实际运行覆盖现代 `i686-pc-windows-msvc` 和 `i686-win7-windows-msvc` 两套 PE。每套原父进程分别运行 controlled 6 项和完整 lifecycle 66 项，原进程 Wait/Exit/checked-close 全部通过；6 项包含在 66 项中，不相加。源码 feature 测试总数 69，其中三项编译期 owned-removal scope 测试在普通 default-off CI 中未编入，因此默认实际范围为 66。嵌套子测试汇总不冒充父汇总。

现代目标首份 wrapper 报告的 `FAIL_RETAIN_ORIGINALS` 原样保存：两个测试父进程已实际返回 0、完成 checked-close，Python 资格解析随后因缺失 `re` 导入触发 NameError。后续独立补取只读取该次原生结果、完整核验下载并精确清理原 nonce，`nativeRerun=false`。Win7 目标另一独立 nonce 实际运行成功并精确清理。Win7 和 Win10 操作系统、希沃真机、这些官方产物的真实 MSI 联动均仍 NOT_RUN。组件中的 cfg(test) Begin/End 或纯合同测试不能当作生产 MSI API 回执。

`source-manifest-first.json` 列出 400 份原文本的来源、原 bytes/SHA 和 `storedPath`；相同内容逐字节比较后压缩包只保存 307 个完整原件。四份 CI before/after Git 树原 stdout 均为相同的 19,297 条严格 NUL 分隔记录，保留四条来源映射而仅存一份内容。307 份原件全部可通过该 manifest 恢复各原文本，未修改原字节。两官方 ZIP、PE、冗余巨大 input-domain/tree JSON 与四份非法首版 download 元数据只保存身份；合法 second 元数据及原修复回执直接保留，未放宽 JSON 解析。

`evidence-originals-first.tar.gz` 包含有限 monitor/dispatch、必要旧 runner/BASE/CAP 依赖、两轮准备源、官方 ZIP 内选定原 CI 输入/编译/metadata 文本、AST、原失败与补取及成功报告和独立合入审阅。8 份核心报告另以原字节直接保存。完整 tar 逐成员回放、来源身份复核、严格凭据扫描见 `archive-verification-first.json`；扫描器来自已提交 R79 原件，唯一额外控制符形式是精确 path/size/SHA 的四份 Git 树，完整 raw/visible/concatenated 视图均扫描。

未纳入 raw scope、私钥、凭据、学校资料、tools/target、活动源码、后续 MSI 封包、尚未终态的 host 测试。本次归档没有 Windows/SSH/Cargo/Git 操作，没有修改进度或旧归档，也没有删除来源。
