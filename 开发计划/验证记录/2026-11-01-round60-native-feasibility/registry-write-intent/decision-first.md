# R60 注册表 write-intent 研究结论（仅研究）

**当前不能把临时子键方案认作严格 Run 比较删除。** 已复核归档 R56 原反例：phase2 关闭操作在读后、暂存删除前让普通竞争写 open/set/close 全成功，候选仍提交删除，mutated=true、最后 Absent。原严格失败保留，不能凭 TxR 名称或 broad Remarks 把它改成通过。

官方 `RegCreateKeyTransactedW` 的 NEW/EXISTING disposition 可支持“只处理同次实际新建 child”的归属判断。 reviewed 文档没有承诺：child 名称修改保护**父键的值**，也没有说明删除新 child/关闭句柄后这份冲突覆盖保持到何时。`RegDeleteKeyTransactedW` 的最后句柄规则说明删除生命周期，不等于父值写锁承诺。因此方案目前仅是待证伪候选。

`research-first.json` 列了有限 scratch 原生实验：保留原无保护先读红灯对照；增加正常无竞争进展、原 phase2、预先已打开写句柄、删除 child 后/提交前、第二真实事务、现存 child 及外部接管 child、取消/原期限。必须保留实际 native 状态、真实提交与最终合成字节；阻塞/超时/未知结果不算保护成功。几个样本通过也不能成为通用 CAS 证明，更不能放开 MSI 生命周期门槛。

9 份微软官方源码原字节已保存在 `primary/`，索引 `primary-originals-first.json`。仅读取和创建本研究目录文件；未改 main、建工作树、安装、编译或操作 Windows/CI。
