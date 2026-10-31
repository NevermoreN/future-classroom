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
