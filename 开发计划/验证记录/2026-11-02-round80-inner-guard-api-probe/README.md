# Type1 quiet-mode真实API探针：有限证据

本目录保存固定C Type1守卫的API探针原件，不是新Rust生产守卫或发布授权。核心事实见 `probe-facts-first.json`；原始runtime、首编译FAIL、第二静态编译、首工具FAIL、工具恢复和最终审计六份报告另直接保存，全部451份文本原件在 `evidence-originals-first.tar.gz`。原FAIL、NOT_RUN与恢复的状态不互相覆盖。

健康与提前失败分别实际Begin0/Install0或1603/EndRollback0/事件close0；两次原G进程、线程实际退出0并checked-close。健康INFO后有InstallInitialize，提前失败没有。24份运行文件完整下载，nonce根实际清理，五个精确系统路径只读确认不存在。生产授权NONE；Win7/Win10实机NOT_RUN。

`source-manifest-first.json` 保存每份原件路径/字节/SHA，以及22个二进制身份、179个可重建工具树文件身份和5个未跟随链接。工具树不提交payload或上游副本；原工具目录仍在使用，不删除。`archive-index-first.json` 绑定实际tar与核心原件；`archive-verification-first.json` 为全成员重放及UTF16/CP936/CLIXML/JSON/JSONL/嵌套base64凭据扫描回执。仅两份精确MSI日志的合法流名称控制字符有结构白名单，未知控制符拒绝。

归档控制器默认不执行。first/second原本地准备失败及源码保留；third仅修正精确元数据断言及原失败记账，实际退出0。归档未联系Windows、运行Cargo、读取私钥或活动W/target，也未删除原件。主进度文档顶部摘要及末尾API探针段落由本步骤更新。
