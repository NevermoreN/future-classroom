# R42归档及实施记录建议（第二份；首时点准备稿保留）

本文件只整理已保存原件；不修改主仓库，不复制或删除构建、程序、ZIP、网页资源，不代表归档或最终清理已经完成。首份准备稿在主代理保存intentional副本前形成，保留该历史时点；本稿以`package-intentional-artifact-first.json`补证主代理已经实际保存的开发ZIP。拟归档相对目录为`开发计划/验证记录/2026-11-01-round42-ci/`，实际目录名由主代理确认。

本轮CI源为`bc2a86fb4413dcb3e8acf6b0d4bb2dfe601c22dc`，覆盖R39最终设备提示实现、此前Win7 MSVC静态TLS误报处理和清理文档；**不包含R41首头期限改动**。`live-ci-handoff-first.json`及各`*-first`/`*-second`的运行中快照保留原状态，由另名终态记录补证，不覆盖历史。

## 建议追加到实施记录的事实

现代MSVC run41244281175/job124664360125实际SUCCESS。artifact13058962672完整ZIP为40,772,166B，SHA256 `93638e76769f5d3910c4e3e9a713a93bedc318c55ebc8e78f24ff42ac4ff265a`。192成员CRC、191条producer ledger、170份非binary原件、39条实际终结命令及构建清理独审通过；22份EXE通过精确SHA、PE32 x86 CUI与禁止动态CRT导入检查。完整6444项Git输入中，4124项与原Git blob完全一致，2320项与原LF→CRLF转换完全一致；不能改称6444项全部raw Git字节相同。参见`modern-full-download-first.json`、`modern-original-member-ledger-first.json`、`modern-programs-independent-first.json`及`modern-artifact-raw/{result,cleanup,input-sha256,artifact-sha256,executables}.json`。

同一正常程序10,324,480B，SHA256 `6c82d3dfd5352849c4291bf06c2ed0228ceab9edbf368a06eb753dab2014d8ac`在授权Win11完整运行21份测试程序，1413passed/0failed/0ignored/0filtered，cleanupPASS；公共桌面20步及配色22步独立实际PASS/cleanupPASS。`runtime-count-extraction-first-failure.json`记录首次本地计数脚本误以为`output`为字符串、实际为`list[str]`的guardFAIL；Windows原成功报告未改，也未为计数重跑测试。随后`modern-runtime-independent-first.json`仅对原完整列表逐行统计成功，不应把本地guardFAIL说成Windows测试失败。参见`modern-msvc-runtime-first.json`、`modern-msvc-public-native-first.json`、`modern-msvc-visual-native-first.json`及其原命令/stdout/stderr。

实际完整Native TLS＋持续接纳＋隔离Edge REMINDER流程PASS/cleanupPASS，独立重算68条原client events包含64条HTTP响应与4条负向观察；浏览器17条观察中14条为真实wire、3条仅cache；原生78heads/78responses精确等于64＋14。浏览器恰9API/1POST/0visual、2次底部快捷点击、3个DOM阶段revision0/1/1；原2个共享worker实际join、native drain及upload/download Hub/quarantine均0。Node actualexit0且contextdispose/CDP断开，Edge原owner supervisor actualexit0、job进程0、句柄与profile/root清理均核原件；Edge primary自身在`capability-stop`后实际exit1，不能改记为child exit0。参见`modern-msvc-voice-capabilities-browser-first.json`、对应`.browser-node.json`、`.browser-edge-owner.json`、两个raw目录及`voice-capabilities-browser-independent-first.json`（163项独立核验）。

新增设备capabilities三次真实GET状态401/200/200，两次授权成功响应的正文长度及SHA相同，实际设备提示`UNVERIFIED`。20字段严格校验绑定实际runner源码；原正文按保密契约未保留，独立审查没有从不存在的正文重新解析字段。`nativeQueryCount=NOT_OBSERVED`、`cacheBehavior=NOT_DETERMINED_BY_GET`，不得由GET次数推定实际native query次数或cache命中，也不能据此声称AVAILABLE、当前可听或真实设备播放验收。该阶段没有新增录音/submit/播放；完整旧流程单独保留合成全零PCM WAV上传，不等于播放。

Win7 run41244342769/job124664537017实际FAILURE。artifact13058948360完整ZIP为41,487,448B，SHA256 `35ce3793f54327a948113f891f3dfda6cbe623af1459412eda212443dab7133b`。201成员CRC、200条producer ledger、179份非binary原件、22份PE32 x86 CUI/subsystem6.1/staticCRT、6444项输入（4124直接＋2320精确CRLF）独审通过；41命令只有严格Clippy exit101，其余40条exit0、原进程均实际终结、无超时/日志截断，构建清理PASS。**编译成功及静态PE检查不能取代整个Win7 CI失败或Win7实机验收。**

完整原Clippy stderr确认三个已const初始化的`thread_local`触发`missing_const_for_thread_local`：`crates/fc-bootstrap/src/reinitialization/tests/tls.rs:17`的`EXIT`、`src/voice_runtime/tls_exit_tests.rs:45`的`EXIT`、`src/voice_pipeline_probe.rs:31`的`PHASE`。快速诊断最初只注意前两项；`win7-original-independent-first.json`独立核验脚本误设诊断总数为2，完成22PE/6444输入后在计数断言FAIL，原报告与原脚本均保留；`win7-original-independent-second.json`用新文件严格绑定全部三个路径/行号并通过原件核验，CI仍标FAILED。该新失败不自动归因于此前五static问题，修正与新CI另轮另证。参见`win7-failure-diagnosis-first.json`、`win7-original-independent-{first,second}.json`及`win7-artifact-raw/logs/clippy.stderr.log`。

开发候选`FutureClassroom-0.1.0-voice-capability-dev.zip`为5,203,704B，SHA256 `5e280a84e726f0366db3e0ea1bd704158e26ac1b3fdb4e542f397b48fc1706c0`。13成员/12payload全CRC/size/SHA、精确EXE、6份Web资产及manifest、许可证/notice原件匹配；同basename独立重包全ZIP字节一致，重复ZIP及新owned临时目录已经实际删除，首候选保留。参见`package-independent-first.json`、helper及两次原命令/stdout/stderr。主代理已另行把完全同SHA的开发ZIP实际保存到`/mnt/data/FutureClassroom-0.1.0-voice-capability-dev.zip`，`package-intentional-artifact-first.json`原件记载5,203,704B/同SHA；本次只读核对该副本，不执行复制或发布。TMP候选与Web资产仍供R41使用：**不清理、不加Release/tag，不当作完整产品安装包**。

`windows-before-first.json`及`windows-after-first.json`只读审计PASS：sshd仍Running、公钥授权SHA及Run双视图不变、持续测试私钥仅检查权限0600且未读取内容，项目测试目录/进程/任务均0。本次报告只确认R42该次Windows前后观察；R41仍在使用的Linux候选包/资产和其它独立工作树不纳入R42已清理结论。

原first TCP→header严格5秒门槛仍FAIL/未通过、普通生产listener关闭。Win7/Win10真实运行、希沃可见交互/触控、人耳/麦克风/校园TLS、完整产品验收均不能由本轮补证升格。合成已批准会话的REMINDER浏览器验证不代表实际teacher角色或手机验收。全部历史失败原件保留，不重派旧bc2 workflow。

## 归档范围与复核动作建议

- 将R42证据根内全部普通原件、下载/审查helper与完整命令日志、两个CI的非binary原件、实际native/Edge raw目录、首次失败与后续补证一起按原字节保存。根`.owner`只用于本机临时目录所有权，不入归档。
- 当前没有活跃`.control.json`文件；已结束Edge owner对应的`.control.json.stop-*.json`是历史stop请求回执，先保留失败/成功语义，再按凭据扫描和实际owner结束事实复核；不要把路径名称当作活跃控制能力。不得归档私钥或仍活跃control文件。
- CI两份大ZIP、22份现代EXE、开发包、Web构建资源仅以外部路径/字节数/SHA绑定；不把二进制、target或node_modules提交到计划证据目录。保留引用不表示本次已删除。
- machine清单是后补当前快照`archive-candidate-manifest-second.json`（首份`archive-candidate-manifest-first.json`保留其原时点），不是最终`INDEX.json`，也没有实际复制到仓库。主代理实际归档后应重算全部成员SHA/数量/字节、保留所有失败、扫描明文及JSON base64字段中的凭据，并生成正式INDEX；若后续追加证据须新名补充清单，不覆盖原快照。
