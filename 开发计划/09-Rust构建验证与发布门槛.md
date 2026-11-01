# Rust构建、验证矩阵与发布门槛

> 实施状态（2026-10-30）：用户已授权开始开发。本文保留 D11 设计基线；“仅计划、不开发/不测试/不同步”及“全部待实施”为历史状态，当前实现范围、证据与未完成门槛以[实施进度与验证记录](10-实施进度与验证记录.md)为准。

总计划1.22 / D11，2026-10-30。本册给出可实施的构建/证据步骤，已授权运行构建与隔离验证，实际证据以实施记录为准。P0填写固定版本之前，带尖括号的命令是参数模板，不是已验证构建说明。

## 1. 必须先冻结的构建清单

P0-02建立候选，P0-09使用完整依赖图验证后冻结，P0-07统一审查。记录：Rust/Cargo/nightly日期与commit、rust-src同commit、edition/MSRV、target规范、MSVC或clang-cl/linker版本、SDK/CRT具体版本、host OS、Windows补丁/CPU指令/显卡触控、所有crate精确版本/features/build.rs与原生C库源码/哈希/许可证。

Win7正式构建候选为i686-win7-windows-msvc；Windows 10/11同属首版目标，其包目标须按兼容矩阵另行冻结。官方把Win7专门目标列为Tier 3且不分发预编译目标工件，需要自行构建工具链或标准库；普通i686-pc-windows-msvc及Windows11上的运行都不能代表Win7通过。[官方Win7目标](https://doc.rust-lang.org/rustc/platform-support/win7-windows-msvc.html)

nightly与rust-src必须匹配，启用build-std的相关构建/Clippy/test都使用同一配置。Cargo官方指出cargo test还需要构建test crate；模板统一使用-Z build-std而不误用只包含core/alloc或遗漏test的显式列表。[Cargo要求](https://doc.rust-lang.org/cargo/reference/unstable.html#build-std)

P0-09的探针必须实际链接并运行SQLite bundled、精简libcurl/Schannel、选定windows绑定、CSPRNG/PBKDF2、COM/WIC/绘制、unwind及WinMM/HTTP生命周期的必要调用。空main或仅cargo check不能冻结可用组合。GNU仅为补充对照，不替代正式MSVC目标证据。ARM64OS未明确前不新增发布目标承诺。

## 2. 拟定配置责任

| 配置 | 责任与校验 |
| --- | --- |
| rust-toolchain.toml | 固定工具链日期与rust-src/rustfmt/clippy组件；P0确认这些组件可同时获得 |
| Cargo.toml/workspace | 12生产crate、resolver=2、edition2021候选、明确rust-version；禁止基础层反向依赖平台 |
| Cargo.lock | 固定全部传递依赖，CI使用--locked；软件包特性白名单单独记录，锁文件本身不等于已锁定features |
| .cargo/config.toml | target linker、目标专用flags及build-std；不将Win7链接参数误施加到host的build.rs/proc-macro |
| release profile | panic=unwind候选，与FFI捕获策略一致；LTO/codegen/调试/strip由实际体积与性能校准，不默认追逐最小二进制 |
| C库构建 | SQLite/curl明确target与CRT；/MT或兼容动态CRT在同一ADR里选定，检查实际导入/包内依赖，不能混用未审计下载二进制 |
| API/features | 按使用模块列windows features、禁额外WinRT/WebView/媒体解码及无关网络运行时；不得为方便全开features |
| 协议与schema | 固定JSON/指纹规则版本、SQLite迁移/密码参数，TypeScript/后端兼容测试随包 |

可选择缓存工具链/依赖源码以便复现，按许可处理SDK/工具链分发，不把用户凭据/整套开发环境放进白板安装包。P5-06检查净环境可复现并生成组件/许可/哈希清单，P5-03再做最终ZIP/MSI。

### 当前Hosted构建的输入取证域

R45修正仅将精确路径前缀 `开发计划/验证记录/` 的历史档案与实际编译输入分开；不隐藏它们的Git对象或路径数量。完整HEAD的root tree、所有跟踪路径及普通blob的mode/type/Git SHA1/原blob大小分别保存于 `input-git-tree.json`，与 `git ls-files` 精确匹配；历史域仍检查文件存在、单件大小、文件及祖先目录重解析属性，并记录检出大小，但不声称读取其内容SHA256。Git原blob大小和对象SHA1不等于Windows检出大小或SHA256，CRLF转换单独说明。

除该精确前缀外的全部跟踪文件仍逐件计算实际检出SHA256，保持 `input-sha256.json` 数组形状；`input-domains.json` 使用 `future-classroom.ci-input-domains/v1` 显式列两域。所有路径合计仍最多10000，单blob及检出文件仍不超过64MiB，实际SHA输入合计仍不超过256MiB，元数据输出和这三类JSON各受原16MiB限制。构建后重新核对HEAD、路径、Git树、两域清单、普通文件SHA及干净检出，保留六份before/after清单及三个初始摘要。独立产物审查须按该schema分别证明全Git元数据和SHA子集，不能沿用旧“全部Git文件均读取内容SHA”的计数。

Rust/Cargo构建输入出现显式历史域路径（含Windows分隔符）即拒绝；真正需要的fixture应进入SHA域，不能静默当档案跳过。这是当前输入政策及文字检查，不能证明任意动态build.rs I/O已经被沙箱隔离。原工具链/CRT、完整workspace/all-targets/严格Clippy、jobs2及各命令原超时参数均保持：Hosted默认120秒，测试编译2400秒、产品编译1200秒、Clippy1500秒；本地Rust矩阵每命令900秒属于另一验证范围。

实现和实际验证层级见[实施记录](10-实施进度与验证记录.md)R45；Windows PowerShell5.1抽取函数通过，不替代要求PowerShell7.2及以上的Hosted主体、完整MSVC编译或目标系统运行。

## 3. 命令矩阵（后续建工程后执行）

工具链占位符必须由P0固定值替换。Windows目标参数使用相同锁定features；以下不自动创建rustup target或假设Tier3标准库可下载。

```text
cargo +<固定工具链> fmt --all -- --check
cargo +<固定工具链> clippy -p fc-domain -p fc-ports -p fc-runtime -p fc-application -p fc-security -p fc-materials -p fc-audio -p fc-presentation -p fc-protocol --locked -- -D warnings
cargo +<固定工具链> test -p fc-domain -p fc-ports -p fc-runtime -p fc-application -p fc-security -p fc-materials -p fc-audio -p fc-presentation -p fc-protocol --locked
cargo +<固定工具链> test -p fc-storage --locked
cargo +<固定工具链> build -p fc-bootstrap --release --locked --target i686-win7-windows-msvc -Z build-std --no-default-features --features win7
cargo +<固定工具链> clippy -p fc-bootstrap --release --locked --target i686-win7-windows-msvc -Z build-std --no-default-features --features win7 -- -D warnings
cargo +<固定工具链> test --workspace --release --no-run --locked --target i686-win7-windows-msvc -Z build-std --no-default-features --features fc-bootstrap/win7
```

win7为拟定bootstrap feature边界，P0建探针时明确其向各适配器传递规则；不能把模板解释成当前已有manifest。Clippy参数通过`--`分隔；新版本Cargo也可按固定工具链采用warnings配置。[Clippy用法](https://doc.rust-lang.org/clippy/usage.html)

host纯规则/真实SQLite测试只证明该host环境。--no-run仅编译目标测试，不记录运行PASS；测试exe及所需DLL按清单复制到隔离Win7环境运行，包含实际Release产物的启动/输入/驱动动作，记录物理机/虚拟机/Wine各自范围。Wine可做补充，不能替代希沃触控/显卡/驱动或真实扬声器证据。

带故障注入的隔离Harness调用同一平台/用例实现，需记录其与正式Release的features/profile差异；生产包不得含可远程触发故障的调试入口或测试账号。unwind测试配置与正式Release必须一致；测试框架默认展开不能证明panic=abort发布版也可捕获。

## 4. 验证层级和可报告结果

| 层级 | 范围与证据 | 不能替代 |
| --- | --- | --- |
| L0 文档/契约 | crate/任务DAG、R/M/F/C/P/Q映射、RR关闭证据、本地链接 | 程序和依赖兼容 |
| L1 host纯规则 | Clock/Repository/Audio替身、类型/权限/版本/布局/队列状态 | Windows文件/FFI/设备 |
| L2 host持久性 | 真SQLite、独立合成数据、关闭重开、CAS/journal/恢复/指纹 | 各目标Windows文件耐久、ACL、DPAPI |
| L3 各Windows目标构建 | 完整crate/C图、PE32、CRT/静态和动态API筛查、锁定features/许可；按Win7/10/11包目标分别记录 | 目标运行和完整功能 |
| L4 各Windows目标运行 | Win7干净SP1 x86及Win10/11指定build分别验证启动、控件/文件/TLS/浏览器录音/音频/故障排空 | 希沃触控/显卡或长期资源趋势 |
| L5 白板/网络/长稳 | 实际希沃/教师手机、PPT避让、4K/DPI、录音播放、72小时、安装/卸载 | 未测的ARM64或别的学校环境 |

每条证据记录任务/Q号、提交与依赖锁哈希、配置/设备/输入修订、动作/预期/实测、失败/未覆盖与清理。只允许PASS/FAIL/BLOCKED/NOT_RUN，缺环境记BLOCKED，未实施记NOT_RUN。计划层已补齐不等于L1–L5通过。

## 5. 71项任务的阶段门槛

- G0：P0-01/02/03/05/06/08/09原型/环境证据由P0-07审查。失败可继续无关host工作；Win7/网络/音频未过仍阻止对应承诺的首版发布。
- G1：P1核心crate/单writer/HealthGate/队列/协议/布局原型形成完整闭环，类型和故障路径可独立验证。
- G2：P2身份/设置/恢复/系统日志、P3受限资源与音频、P4联合版本/课间来源、P6实际网页与重叠IO完成；全部敏感路径使用同一闸门/代次。
- G3：P5-06构建复现与P5-07故障Harness完成后P5-01集成，受影响Q01–Q07、Q09–Q11、Q15–Q25不得漏测。
- G4：P5-02性能/长稳、P5-03干净系统包与恢复/卸载、P5-04许可/管理/兼容/清理；发布依赖包含全部首版任务，P7扩展与P5-05仓库同步不作为前置。

已有业务限制和所有权要求始终有效；未达总资源预算应优化/修订ADR，不能通过漏计大卡、解码或线程开销标PASS。

## 6. 估算、交付与清理

Rust计划初估：P0=8–13、P1=12–19、P2=10–17、P3=14–22、P4=14–22、P6=11–20、P5=12–20，合计81–133人日；增加的12项任务与旧任务重排都计入，不重复叠加工时。此为待P0校准估算，不是日期承诺；外部等待/72小时观察等待、VPN、ARM64、原生PDF/Office渲染、远程基础课表另计。

交付保留正式Rust源码/锁定构建、生产包及许可/脱敏证据，清理临时target/测试构建、录音/上传/副本、测试账号、专属URLACL/防火墙/证书项和浏览器麦克风轨道。只按创建清单清理本项目项，不删除教师正式资料/学校CA/其他服务。源码内有长期价值的测试/fixture属于正式代码，不当作“测试残留”删除。

历史规划阶段仅授权计划重拆解，L0代表当时的规划完成。当前用户已授权按计划开发、Windows隔离测试和向既有自有仓库同步；最新实施与实际证据见实施记录。完整产品仍须逐项完成L1–L5，不能用L0、可编译或候选探针代替。

## Windows 10与Windows 11补充门槛

两者均为首版正式兼容目标，不能以“只在现代系统开发/测试”代替产品支持。Win7专门目标及完整依赖/真机门槛保留；现代目标另检API最低版本、WOW64与包内依赖。L3/L4/L5对Win10和Win11同样分别收集构建/运行/设备证据；GNU现代探针通过不自动冻结正式MSVC包。Win11上的x86程序不等于32位Win11系统，也不证明原生x64/ARM64；详见[兼容矩阵](11-Windows兼容矩阵与验收.md)。

## 第二十六轮TLS候选证据门槛

只读选用当前用户MY证书；测试唯一证书/私钥的创建清单、实际客户端信任/主机名拒绝、加密请求/响应、IOCP与SSPI完成账、真实线程退出和原生释放结果分别取证。四槽Frame/IO/头缓冲与新增1MiB线程栈计入原Web增量16MiB目标，不把结构大小当SSP/LSASS/内核或完整进程内存证明。原始日志先完整流式保存并核对SHA后才删除临时域；异常保留的远端域必须后续补收与精确清理，不能沉默丢失失败证据。

有限批次TLS通过不改变原严格五秒FAIL，不证明TLS close_notify、持续接纳、生产业务/浏览器可信部署或实际Win7/Win10。Windows上的假慢Driver回归只能证明持有物理Frame/IOCP及线程退出的关闭顺序，不声称真实SSPI加密可中断；超时保留的子进程明确由进程退出回收，不混为正常Closed。

## 第三十三轮：分离现代与Win7候选MSVC编译

现代手动流程继续使用固定`1.97.1-x86_64-pc-windows-msvc`及`i686-pc-windows-msvc`。新增独立的[Win7候选流程](../.github/workflows/windows-win7-msvc.yml)，复用[有界采集脚本](../scripts/windows-msvc-ci.ps1)的显式`Win7`基线，固定`nightly-2026-10-29-x86_64-pc-windows-msvc`与compiler commit `92ffebeceaa0a071066f233873c2438e73d70fe7`；同一次工具链安装取得rust-src，不尝试下载Tier3目标的预编译标准库。目标为`i686-win7-windows-msvc`，test/build/Clippy均使用不带crate列表的`-Z build-std`及明确target，fmt/metadata不传该构建参数。

Win7的target专用linker、CC、`_WIN32_WINNT/WINVER=0x0601`、静态CRT与PE32 subsystem6.1分别检查，保留实际VS/SDK/CRT和选定rust-src/lock哈希。两个基线使用不同的run根、所有权标记、并发组和artifact名；仍按原135分钟job/105分钟构建、单命令预算、每流16MiB上限、固定双构建并发、精确进程归还与原件不覆盖规则执行。编译流程不运行任何生成EXE。

实机PowerShell的28组/421断言仅证明完整源码可解析、三个纯函数的参数/PE边界和现代参数等价；完整采集脚本仍要求Hosted PowerShell 7.2及以上。新的共享采集代码和Win7候选都须以同步后的完整SHA各取得实际Hosted CI结果，不能用修改前的现代CI通过替代。即使所有编译及subsystem门槛通过，Win7 API动态路径、SDK/CRT兼容、真实SP1 x86运行、Win10与希沃设备证据仍须独立验收。


## 第三十四轮：Win7 Clippy参数与持续接纳定向验证

93414d5的现代共享helper Hosted CI已实际成功。Win7候选原CI的workspace测试/正常程序编译及22份PE采集通过，但Clippy缺少core而失败，整体不得填成功。固定同一nightly的独立离线最小复现确认：把build-std放在外层Cargo全局参数位置不能传入Clippy内部check；改为`cargo clippy -Z build-std … -- -D warnings`后实际编译core并成功。最小复现使用GNU Tier3和core子集，仅作因果诊断，不能代替完整MSVC依赖或运行证明。

共享helper只调整Win7 Clippy的参数位置，生产继续完整无子集`build-std`；Win7 test/build及现代参数不变。修正后实际Win11 PowerShell纯函数28组/421断言通过，完整新源码仍须Hosted PowerShell>=7.2执行及真实Win7另取证。连续接纳Rust增量须核对冻结源码、受影响Windows测试程序/正常程序哈希、实际Win11运行、真实Schannel四请求容量矩阵与自有资源清理；上述应用层结果不解除严格首TCP五秒、内核/SSP资源、Win7/Win10及可见希沃门槛。

## 第三十七/三十八轮：原MSVC产物终态与固定nightly新诊断

原 `b76ad738100a722ee5c23804a489644a93593a1f` 的[现代run41235609015](https://github.com/NevermoreN/future-classroom/actions/runs/41235609015)实际SUCCESS，artifact13056400983为40531990字节/SHA `119833672c476ee9b51fd9528c7b6e996b5fb54fff333f9ef7a0046e70a2d06a`。完整ZIP192成员CRC、170份非二进制生产者账和22份PE32/静态CRT独审PASS；[独立输入审查](验证记录/2026-11-01-round38-compat/modern-programs-independent-first.json)对5165项分别核对2862项原Git blob和2303项精确LF→CRLF检出字节，不将后者描述为raw Git blob一致。原件拟归档到上述相对目录，完成归档另记。该现代程序在授权Win11实际21EXE1399/0/0、20步公开桌面、22步配色和有限真实TLS/连续接纳/Edge REMINDER均PASS且cleanup PASS，详见[实施记录](10-实施进度与验证记录.md)；不外推到R38源码、Win7/Win10或正式发布。

同源码[Win7候选run41235621154](https://github.com/NevermoreN/future-classroom/actions/runs/41235621154)整体FAIL：测试/正常程序及22PE原采集成功，严格Clippy实际因 `chunks_exact_to_as_chunks` 失败，不是旧build-std缺core或运行错误。artifact13056180652为41202325字节/SHA `9eff3b281e7374e0484e8e8b0ff16841bdf624765e99c4f477af3ade32dc4bc1`，完整201成员CRC/179份非二进制账通过，[5165项源码证明](验证记录/2026-11-01-round38-compat/win7-all-source-input-proof-second.json)具有相同2862/2303分层；CRLF导致的首次直接比较失败保留。[22份PE32/静态CRT/subsystem6.1独审](验证记录/2026-11-01-round38-compat/win7-all-pe-independent-first.json)仅PASS_STATIC_INSPECTION_ONLY，未执行这些EXE，不足以冻结Win7正式可用组合。

R38仅将17文件21处常量2/4分块改为 `as_chunks(_mut)` 完整数组迭代（两处像素比较作必要解引用），再对local-first实际strict Clippy暴露的清理布尔返回及七天字段循环作两处等价style修正，总计19文件23处；保留原始失败/完整差异及前后冻结记录，不加lint allow、不更换工具链或放宽 `-D warnings`。[local-second](验证记录/2026-11-01-round38-compat/chunk-lint-local-second.json)固定nightly全workspace/all-targets Windows GNU严格Clippy PASS，后续Host debug因过慢明确取消并记录FAIL/exit -2/NOT_COMPLETED，不计作测试通过。首次pidfd准备API不可用且未发信号，第二次按UID/argv/PPid/startTicks精确确认后仅停止原自有进程并清理独有scratch，原部分日志不覆盖。

截至本段，[local-third](验证记录/2026-11-01-round38-compat/chunk-lint-local-third.json)完整workspace/all-targets release测试和stable严格Clippy仍RUNNING，保持每命令原900秒、jobs2/tests2、无过滤、不减KDF；尚无Host最终PASS。新现代/Win7 Hosted CI未重派，须先完成本地验证、原件归档及自有残留清理、同步新完整SHA，再各唯一dispatch。旧ZIP/22EXE自有副本已清理，活跃target和未归档证据保留。严格首TCP原5_000_000_000ns FAIL、生产listener关闭、真实Win7/Win10/可见希沃与发布门槛均不改变；不创建Release/tag，完整目标保持active。

### R38完整本地矩阵终态（追加，不覆盖原RUNNING/FAIL）

[第三次](验证记录/2026-11-01-round38-compat/chunk-lint-local-third.json)最终为冷release编译超时FAIL，host-test原900秒exit124、尚未运行测试；原controller/Cargo/compiler均已实际退出，保留原日志及不变源码。[第四次独立操作](验证记录/2026-11-01-round38-compat/chunk-lint-local-fourth.json)则实际PASS：nightly完整Windows GNU严格Clippy4.05秒，显式完整release `test --no-run` 296.83秒，随后同一完整workspace/all-targets release Host测试71.82秒，stable完整release Host严格Clippy118.72秒，fmt/架构检查亦exit0。每条命令分别保持原900秒预算、jobs2/tests2；没有过滤、KDF降级或lint放行。

[原stdout/stderr计数证明](验证记录/2026-11-01-round38-compat/chunk-lint-host-count-fourth.json)核对实际Running21份与21组完整summary，为915通过/0失败/0忽略/0过滤；406份sourceBefore/sourceAfter逐字节一致。该矩阵不含独立R39工作树，不替代新R38现代/Win7 MSVC CI或目标系统运行。新CI尚未dispatch/通过，完整归档和活跃构建/工作副本清理尚待实际执行；原先失败不被后续成功覆盖，原严格首TCP五秒FAIL及正式发布限制不变。

## 第三十九/四十轮补证：严格构建通过与实际测试失败分别保留

R39原 [voice-capability-local-first.json](验证记录/2026-11-01-round39-voice-capability/voice-capability-local-first.json) 因新增测试fixture解构Drop类型的E0509实际FAIL，原件与失败源码保留；仅修fixture所有权后，[voice-capability-local-second.json](验证记录/2026-11-01-round39-voice-capability/voice-capability-local-second.json) 全矩阵实际PASS：nightly Windows GNU严格Clippy13.41秒、完整release显式编译685.93秒、Host全量测试50.44秒、stable release严格Clippy30.07秒。原stdout21组summary与stderrRunning21份独立对应929通过/0失败/0忽略/0过滤，410份输入前后变化0；仍是原900秒/命令、jobs2/tests2，不过滤、不减KDF、不放行警告。

独立R40原现代MSVC run41240782662（`3b3d3abbfb80240989e32bc0858234d25bc277fc`）实际SUCCESS，不能误记为R39 feature源码通过。[modern-full-download-first.json](验证记录/2026-11-01-round40-ci/modern-full-download-first.json) 的完整artifact13058056589为40564846字节/SHA `d1ad2500aeb9d1e76d68e87fa01d990e4200e0ffd5c7e83cbdd30dce4229a976`，192 ZIP成员CRC、191份producer ledger、170份非二进制原件及22份PE32/静态CRT独审PASS。5829项完整提交输入分为3521项原Git blob直接匹配和2308项精确LF→CRLF检出匹配，不泛称全部原字节一致。两个证据目录按计划归档位置以相对链接引用，完整归档清理终态另记。

其授权Win11实际 [modern-msvc-runtime-first.json](验证记录/2026-11-01-round40-ci/modern-msvc-runtime-first.json) 则FAIL/cleanup PASS：仅首bootstrap库314通过/1失败，material_stream_tests:344的worker admission helper失败，未执行后续程序。原拒绝kind未记录，不能断言唯一Busy原因；不得将CI编译成功或同正常EXE `d5dd4d4c07331128804ff4faf24e12dcc8336a527871b4cbef72b504d045954c` 的独立20步公开桌面、22步配色及有限TLS/continuous/Edge REMINDER PASS/cleanup PASS合成该次全套1399通过。

R39只将该Windows测试helper改为同Task/FnOnce、原15秒内明确未接纳Busy才重投，其它kind明确失败，不改生产池或测试断言。其 [voice-capability-local-third.json](验证记录/2026-11-01-round39-voice-capability/voice-capability-local-third.json) 最初RUNNING与第二次结果分别记录；随后第三次自身完整矩阵实际PASS，nightly交叉lint11.48秒/release编译0.26秒/Host测试44.97秒/stable lint0.27秒，独立原21组summary/Running账为929/0/0/0，410输入变化0。以上并不证明修改后Windows库实际运行或R39 MSVC已通过。Python同源码第二次纯self-test完整原stdout/stderr/command现已持久化，16组含63负向model PASS仍仅工具验证。

Win7候选原run41240791635仍live、未artifact且未重派；R39新Windows能力查询/完整实机回归、后续源码MSVC、正式Win7/Win10/希沃和完整归档清理继续待实际证据。原严格首TCP5_000_000_000ns FAIL、生产listener关闭和发布门槛保持，不创建Release/tag或把目标标记完成。

Win7原run `41240791635` 现追加实际终态 **FAILURE**（job `124654072346`）；artifact `13058086743` 的服务元数据为41254371字节/SHA `43587d8992441a5602c47d49b63dcc45f59ff34b7395a2882d9ffb8b2f8f201f`。前述live为保留的历史时点；完整原件正在下载，尚不报告失败原因或完整ZIP/CRC/源码/PE检查结果，不用artifact已存在将失败CI改为通过。未重派，目标系统实际运行及其它发布门槛保持。

### R40固定nightly TLS宏误报：原件核实与窄候选验证边界

[精确完整ZIP下载](验证记录/2026-11-01-round40-ci/win7-full-download-first.json)及[独立原件审查](验证记录/2026-11-01-round40-ci/win7-original-independent-first.json)现已通过：201成员CRC、200 producer ledger、22份x86 PE32/subsystem6.1/静态CRT分别核对，5829项输入分3521直接Git blob/2308精确LF→CRLF检出。41条原命令中仅strict Clippy exit101，原cleanup PASS；整体FAIL及Win7 EXE未运行不得被产物存在或静态通过覆盖。

[固定源码分析](验证记录/2026-11-01-round40-ci/win7-thread-local-analysis-second.json)确认五static已是显式const初始化；固定Win7 MSVC标准库OS TLS fallback宏生成非const内部函数导致Clippy误报。[原const最小复现101](验证记录/2026-11-01-round40-ci/thread-local-repro-first.json)与[单static expect0/独立未注解nonconst sentinel101](验证记录/2026-11-01-round40-ci/thread-local-repro-followup.json)分别保留，不对整个crate或其它目标allow。vendor-only候选第四轮本地PASS仅作为[原候选结果](验证记录/2026-11-01-round39-voice-capability/voice-capability-local-fourth.json)；现在[五处条件已收窄](验证记录/2026-11-01-round40-ci/thread-local-msvc-only-fix-applied-first.json)为 `all(clippy, windows, target_vendor="win7", target_env="msvc")`，初始化器/Drop/线程退出及全局-D warnings不改。精确条件最小复验和第五完整矩阵各自另记终态，当前不声明最终Win7全workspace、MSVC或实际Win7运行PASS。

[REMINDER独立32check](验证记录/2026-11-01-round40-ci/reminder-independent-first.json)核对9API/1POST/0visual/快捷2、75原生Head/响应=61raw HTTP+14browser wire，原worker2join和物理排空、Hub/quarantine0；[结束Windows只读审计](验证记录/2026-11-01-round40-ci/windows-after-first.json)PASS且测试目录/进程/任务0、SSH和Run未改。这些结果不替代失败整套或R39设备功能Windows实测；完整归档/清理、严格首TCP原FAIL、生产listener关闭及所有正式平台/发布限制仍各自保留。


R39最终本地矩阵以[第五次原报告](验证记录/2026-11-01-round39-voice-capability/voice-capability-local-fifth.json)及[当前字节冻结核验](验证记录/2026-11-01-round39-voice-capability/final-frozen-verification-first.json)为准：410份Rust/构建输入、Host21程序929项、Windows GNU严格Clippy及Web300项分别实际PASS。R40固定nightly重建Win7标准库的五个已const TLS误报使用每static且仅Win7 MSVC Clippy的expect；[精确小例和独立未注解哨兵](验证记录/2026-11-01-round40-ci/thread-local-repro-msvc-only.json)证明0/101，原全局-D warnings保留。仍须修正后完整Win7 MSVC CI与Windows增量运行；原CI/实测FAIL没有被覆盖。原日志、源码冻结及失败均归档到round39/round40 INDEX.json，后续提交和本地构建副本清理独立取证。

## R42/R43最新独立补证

`bc2a86f`的现代MSVC run41244281175实际SUCCESS，授权Win11完整21程序[1413/0/0/0](验证记录/2026-11-01-round42-ci/modern-runtime-independent-first.json)、公开20步、配色22步及真实TLS/连续接纳/隔离Edge提醒+能力GET分别PASS/cleanup PASS；[设备流程163项独审](验证记录/2026-11-01-round42-ci/voice-capabilities-browser-independent-first.json)只证明实际401/200/200和严格20字段，原生查询次数/缓存命中未观察，不证明当前有声。两CI完整6444项输入分别为4124项Git blob及2320项精确LF→CRLF检出匹配。

同源码Win7 run41244342769整体FAIL：41命令仅strict Clippy101，另三处已const TLS宏误报，22份静态PE检查不代表EXE执行。[原第二次完整独审](验证记录/2026-11-01-round42-ci/win7-original-independent-second.json)与第一次错误诊断数守卫FAIL均保留。R43仅补三处同条件窄expect，[固定nightly三原型](验证记录/2026-11-01-round43-win7-tls/tls-repro-first.json)原const101/定向0/未注解哨兵101通过；原初始化/Drop/全局-D warnings及未报错项保持，修正后完整CI待新源码。完整结果与开发包边界见[实施记录](10-实施进度与验证记录.md)R42/R43，不覆盖独立R41、严格首TCP原FAIL或正式平台/发布门槛。


### R41最终源码与有限关闭证据

七Rust/两Python文件精确集成后，410份Rust输入绑定最终第三GNU全workspace/all-targets编译与两套严格Clippy；实际授权Win1121份程序1420通过/0失败/0忽略，正常程序SHA `b1f25364f488d6e8ffdb45ef53d5e99ff7fa717e3a226dd9ddf75d48bf7dee6f`，另Host受影响fc-windows31项及严格Clippy通过。最终公开桌面20步、配色22步、默认HOMEWORK/连续接纳/语音GET/隔离Edge均PASS并物理清理，不能替代可见希沃或实际Win7/Win10。保留首次900秒编译、SSH解析、Edge stop共享异常及检查器原失败，详见[实施记录](10-实施进度与验证记录.md)R41最终补证。

原post+5秒不变，25毫秒提前alarm及受检abortive close只构成候选整改。最终四TLS滴流/一个裸TCP有限样本最大4978421422纳秒，小于原5000000000；不解除精确ESTABLISHED/backlog/调度硬界、内核/SSP和原第十三轮FAIL。生产listener关闭，新整合源码Hosted MSVC和所有正式发布门槛继续独立验收。


### R46完整MSVC补证

`e23bd538…` 现代run41252067358与Win7候选run41252096527均实际SUCCESS。新合同核对7508项完整Git元数据、550项实际SHA及6958项历史元数据；历史不进入实际SHA总量。550项中的6份直接Git字节与544份精确完整CRLF转换分别证明，不将Git LF脚本SHA冒充Windows检出SHA。各套21程序在授权Win11实际1420/0/0/0，公开20步/配色22步均PASS；Win7候选在Win11运行仍不等于Win7实机通过。

现代正常程序的最终默认HOMEWORK/连续接纳/语音能力/隔离Edge有限流程及最后清理通过，首次浏览器真实Network.loadingFailed仍保留且原因未知，后续PASS不称修复；首次运行漏用前轮900秒完整测试参数导致默认60秒超时亦保留。完整原件、二进制来源、检出域区别与失败边界见[实施记录R46](10-实施进度与验证记录.md)及[INDEX](验证记录/2026-11-01-round46-msvc/INDEX.json)。原严格五秒整体FAIL、生产listener关闭及正式平台/发布门槛不变，不提前套用到后续HTTPS配置源码。


### R47–R51本地配置与证书检查批次

代码提交 `b9c24b62…` 精确包含33Rust+3测试观测文件，429份已知Rust/构建输入逐字节绑定最终候选。GNU完整测试编译/正常构建/stable及固定nightly严格Clippy均PASS，Host独立完整21组973/0/0/0与strict PASS；原编译加测试900秒超时保留，缓存编译后完整测试819.79秒仍使用原900秒、jobs2/threads2。实际授权Win11同GNU正常程序21EXE1481/0/0/0、公开20步/配色22步、证书配置22步及完整默认浏览器流程分别PASS/cleanup PASS。构建时HEAD与工作树改动通过精确字节映射到后续代码提交，不宣称该提交已完成Hosted CI或动态build.rs全沙箱。

22份GNU PE32/x86/CUI全部原objdump保留，系统msvcrt.dll与现代/update风险不隐去，不等于Win7兼容。证书四项检查时Satisfied不授予监听或信任许可；缓存/名称/链逐项如实报告。R51二版有限失败诊断保留原立即FAIL、实际exit0、原期限和物理清理门槛；首次浏览器真实中断仍根因未知。新源码完整MSVC、三系统实机及生产发布门槛单独验收，详见[实施记录](10-实施进度与验证记录.md)与[最终本地整批证明](验证记录/2026-11-01-round50-https-integration/final-local-batch-first.json)。


## 第五十三轮 MSI 候选门槛

[MSI构建器](../scripts/package-msi-candidate.py)当前只准入真实Win7目标MSVC开发ZIP及完整Web资源/许可，用固定wixl 0.103生成每用户x86候选。输入CRC/逐文件SHA、实际安装表/私有固定目录/HKCU组件KeyPath、事务内旧版删除顺序及14件完整解包已取证，见[实施记录](10-实施进度与验证记录.md)。该路径用于实现和检查安装格式，未改变P5依赖：P5-01/02/06、运行中升级拒绝、Run归属清理、实际三系统安装/修复/升级回滚/卸载与完整ICE审查仍须单独完成。候选工具不得在打包时调用Windows Installer、启动产品或写业务/系统配置；不把Linux构建或静态导入报告升格为Win7加载或安装通过。
