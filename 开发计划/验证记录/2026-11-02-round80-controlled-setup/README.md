本目录保存受控 Setup 的有限开发证据，最新私有候选为 `7c135c3dcb2dd4e78f70f39b97fb53746454fec2`，相对 `d2dd16bb25ce5d9a8b7e61546a167a1dd61e5e96` 的十二件源码、完整补丁与真实同步收据均已保留。完整逐项路径、字节数和 SHA-256 见 `source-manifest-first.json` 与 `archive-index-first.json`；十五份核心报告另以原字节外放，其余原文本在 `evidence-originals-first.tar.gz`。

GNU 开发包静态验证通过，十四项 payload 提取字节一致。原 Windows 11 产品三阶段实测仍为失败：首次安装返回 0；精确删除本测试 LICENSE 后修复返回 0、十四项恢复；卸载 Setup 返回 1603、MSI 日志返回 1602。原 Pending 未结算，原失败、控制器三项发现和第二版修正准备均保留。后续 65/66 生命周期组件回归各包含 controlled 六项，不能作为修正候选的新产品联测。第二版控制器及清理研究均未在 Windows 执行。本目录不授予 Pending 清除或新的安装许可。

实际归档一次执行成功：376 件原文本共 13,278,445 字节，压缩包 2,462,856 字节；全成员逐字节重放，外放报告逐字节核对。十三件二进制及三件 producer artifacts 仅记录身份；重复派生目录不递归。扫描原字节、UTF-16、CP936、CLIXML、JSON/JSONL 与嵌套 base64，未发现凭据。仅三份按精确路径、字节数和 SHA 限定的结构字符白名单：许可证十八个换页符，两份 MSI 日志各一个 DigitalSignature 控制字符；未知控制符仍拒绝。扫描原收据见 `archive-verification-first.json`。

本次操作未连接 Windows，未读取活动源码工作树或构建缓存，未删除任何原件，未修改实施进度，未提交或推送。Windows 7/10 实机、正式 MSVC 产品联动、ARP 与正式发布均不由本目录证明。更新的活动 Pending/恢复工作不在本归档范围。
