# 私有Rust guard组件有限证据

候选 `d2dd16bb25ce5d9a8b7e61546a167a1dd61e5e96`，父提交b7e4。事实摘要见 `batch-facts-first.json`；九份核心报告直接保存，149份文本原件在 `evidence-originals-first.tar.gz`，精确路径/大小/SHA在 `source-manifest-first.json`。

最终GNU测试编译、Windows目标严格Clippy、单独动态库导出及正常helper编译通过；Win11完整安装生命周期60项包含维护基础6项。两个原测试父过程实际退出0并checked-close，四件完整下载后nonce根已删除。所有原编译/Clippy FAIL、修正前源码、原未执行准备和重复patch拒绝NO_SOURCE_CHANGE保留。

本步骤是私有组件和封包模板验证：真实B/G/CA联合MSI明确NOT_EXERCISED，正常helper仅编译，正式Setup用户入口和生产修复未验收。Win7 SP1 x86、Win10各自实机NOT_RUN；GNU DLL导出不代替MSVC导出验证。单独动态库与helper依赖动态库具有不同原producer身份，分别保留，不相互覆盖。

归档控制器默认不执行，实际一次执行退出0；全部tar成员逐字节重放并执行UTF16/CP936/CLIXML/JSON/JSONL/嵌套base64凭据扫描，未知控制符无例外。两个测试EXE只保存身份；五项编译产物身份来自原producer报告，未读取target。未执行Windows/Cargo、删除、提交或推送；本地工具、源码与缓存仍在使用。
