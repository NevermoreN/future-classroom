# R45归档及实施记录建议（准备稿，尚未归档）

本文件仅整理当前已保存证据，不修改主仓库，不执行Windows/Cargo/CI，不删除工作树或scratch。拟归档目录可采用`开发计划/验证记录/2026-11-01-round45-ci-input-domains/`；正式归档与清理须由主代理另行实际完成。历史首次失败及当时的待测状态均保留原文，以另名终态补证。

本轮只改`scripts/windows-msvc-ci.ps1`，冻结SHA256为`ed26c9932325d377bbbf4eaf63aad29d75fc0cc8ad77561b1121f3389024c418`。R44两条CI的原错误是源码库存检查在编译前失败，不是Rust/Clippy代码错误；确切原Git树6940文件/286,613,906B，其中历史验证归档6390文件/265,890,446B，使全量原件超过原256MiB总读取界限。`r44-original-inventory-diagnosis.json`保留该原件诊断及其限制：原CI没有记录首触发相对路径或当时的逐文件属性。

R45将输入分为两个明确域。完整Git树仍保存每个路径、regular mode/type、blob SHA1 OID、原Git blob字节数及root tree OID；历史验证归档还检查当前checkout文件长度及叶子/祖先reparse属性，但**不由input hasher读取历史正文SHA256**。所有历史前缀外的tracked文件仍实际读取SHA256，构建前后分别重新读取并比较，同时复核HEAD、完整路径/tree集合及clean Git。`input-sha256.json`因此只代表声明的非历史checkout域，不能继续称为全部Git原文件rawSHA清单。完整metadata集合与SHA子集必须明确分区，无遗漏或混入。Git blob大小/OID、metadata文档摘要和实际checkout SHA256须分开表述，保留精确CRLF区分。

原10000路径/单件64MiB/实际SHA域256MiB/输出与metadata16MiB及reparse检查未提高，新增祖先reparse检查；固定toolchain、build-std、CRT、jobs2和原Cargo参数保持。**本CI的Invoke-Logged默认120秒，test-compile2400秒、product-build1200秒、strictClippy1500秒等原显式参数未改；900秒只属于主代理的本地Rust矩阵，不能把本CI描述为统一900秒。**明确历史引用检查只拒绝Rust/Cargo文本中的直接路径，不是任意动态build.rs IO的沙箱。

`input-domain-model-final.json`实际记录42项Python模型和本地自有Git夹具检查PASS；模型不等于PowerShell执行。原`head-tree-original*.bin`为完整NUL分隔Git元数据字节，归档必须保持字节与NUL，不按文本重写。`checkout-sha256-model*.json`与`checkout-history-metadata-model*.json`只说明本地模型域；其中有意修改的PS脚本不是clean CI源码结果。自有稀疏Git夹具已经移除，独立工作树保留供父代理集成，不应误写全部本地工作目录已经清空。

`windows-input-helpers-first.json`及原stdout/stderr记录授权Win11上Windows PowerShell5.1实际执行41项AST提取helper用例，原始完整脚本AST零错误；源码绑定同一ed26。验证了路径/metadata/数量/单件/reparse/真实hash/同长度内容变更/CRLF域分离/引用拒绝/JSON输出界限等。完整Hosted CI入口要求PowerShell7.2，没有在该夹具执行；没有编译器或EXE运行。256MiB聚合正负边界属于42项模型和原生产guard静态证明，PS夹具没有真实分配256MiB。

`windows-input-helpers-after-first.json`原始观察已显示原进程不在、匹配夹具根0、sshd Running，但本地先误用`utf8-sig`编码别名，随后又把正常PowerShell progress CLIXML作为stderr失败；该报告的exitCode曾由事后重构，不能作为独立原始退出证明。原失败记录及stdout/stderr保留。随后新的`windows-input-helpers-after-second.json`以实际捕获returncode0/原始bytes完成只读审核PASS，确认同一精确owned进程已不存在、fixture根0、sshd Running。`windows-helpers-freeze-first.json`明确区分这两个层次。

`root-main-integration-first.json`绑定main中同一ed26脚本；`service-readonly-review-first.json`记录独立静态审查22项绑定及9类后续artifact validator要求，没有确定阻断发现，但不是完整CI成功。新validator必须核对六份before/after domain/tree/SHA JSON、三类摘要、全Git OID/size及精确非历史SHA子集；不能沿用39/41命令、192/201成员、全6940rawSHA或旧direct/CRLF计数。

`product-gap-readonly-first.json`是另一份只读产品需求盘点，记录17份源码/计划绑定及读前后未变，不属于新的功能实现、运行验证或本轮CI测试结论。归档保留完整原件，不把只读建议写为已开发完成。

本轮实现、模型、Windows helper与清理审核的证据层级如上；新完整Hosted CI、完整Rust矩阵及各目标OS运行须以后续真实终态单独记录，不从现有helper/model推定。原first TCP→header严格5秒门槛、普通生产listener关闭及Win7/Win10/希沃实际可见验收边界不变。

归档时选当前evidence根内全部普通原件（排除临时目录`.owner`），包括首次失败、两个after审核、两份原`.bin`、fixture完整PS源码与其嵌入base64SourceBytes、完整命令/原stdout/stderr、diff、freeze、review和17源绑定盘点。本准备清单不是正式INDEX：实际复制后须逐件复核SHA并重新生成正式归档INDEX，不覆盖现有历史失败。此时没有ZIP或EXE预期；不得混入target、私钥或控制能力文件。
