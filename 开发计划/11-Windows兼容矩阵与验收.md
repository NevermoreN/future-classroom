# Windows 7 / Windows 10 / Windows 11 兼容矩阵与验收

总计划1.41 / 详细设计D11，2026-10-31（北京时间）。用户明确要求在现有开发持续进行时，将Windows 10和Windows 11纳入正式兼容范围。本册扩展现有R09，不改变33项需求、71项任务或十二crate架构；当前程序进度见[实施记录](10-实施进度与验证记录.md)。

## 1. 正式支持目标与架构

| 系统目标 | 首轮程序架构 | 必须取得的证据 | 当前状态 |
| --- | --- | --- | --- |
| Windows 7 SP1 32位 | x86，正式候选i686-win7-windows-msvc | 干净系统、实际白板显卡/触控及必需补丁、完整功能与安装包 | BLOCKED：无Win7实机/正式MSVC证据；GNU补充探针已整改并在Win11局部复测 |
| Windows 10 32位 | x86 | 实际学校build/补丁、低权限完整运行、驱动/触控与安装恢复 | BLOCKED：当前未连接Win10测试设备 |
| Windows 10 64位 | x86经WOW64 | 完整功能，文件/注册表重定向，资料关联打开与系统配置实态 | BLOCKED：当前未连接Win10测试设备 |
| Windows 11 64位 | x86经WOW64 | 完整功能及当前系统安全/证书/音频/显示行为 | 局部PASS：build26300第二十七轮完整执行19个最终测试程序1080项，现代与补充程序在同一Win11完成25步本地语音设置；第二十五轮现代与补充Win7GNU均在同一Win11完成16步显式映射、三树切换和独立程序当前账号重开。第二十六轮另取得两批八连接的真实TLS1.2候选联调（见本册末节）；真实注销登录、GUI正式打包、生产TLS/浏览器、可见希沃及安装/长稳仍NOT_RUN；完整范围及原始证据见实施记录第二十五轮 |
| ARM64操作系统/原生ARM64包 | 待目标系统确认 | 独立工具链、驱动、设备与安装包 | NOT_RUN，不因存在Windows 11 ARM64就自动宣称支持 |

Windows 10和Windows 11是产品运行目标，不是仅开发机构建环境。首轮复用x86程序；x64操作系统下运行x86程序与交付原生x64二进制是不同事项。WOW64是微软提供的兼容机制，但仍必须做真实产品验证，不能据此跳过测试。[微软WOW64说明](https://learn.microsoft.com/en-us/windows/win32/winprog64/running-32-bit-applications)

P0-01登记实际学校使用的Windows 10版本（包括适用的LTSC/教育环境）、Windows 11 build、补丁、GPU/触控/音频驱动及系统位数。最低可支持的具体build要由工具链/API审计和实测共同确定，不能擅自用最新开发机版本取代用户的学校系统，也不宣称全部历史build已经通过。系统厂商是否继续提供系统更新与本软件功能验证分开记录。

## 2. 构建与依赖路线

- Win7正式候选继续采用专门Tier 3目标`i686-win7-windows-msvc`和同commit nightly/rust-src/build-std，冻结MSVC/SDK/CRT及原生C图后取证。
- Windows 10/11可验证现代Windows目标；初期现代GNU产物只是开发探针，不自动冻结为正式包。默认目标标准库可能静态导入新API，不能把现代目标文件直接贴上Win7兼容标签。
- 可评估三系统共用一个经全部目标验证的兼容包，也可按系统分开构建/打包；在完整API/CRT/安装测试前不承诺共用包。文件名及manifest必须说明真实target和验证范围。
- 任何Win10/11特有增强按运行时能力探测和窄边界隔离；共同二进制不能静态导入Win7不存在的API。安全回退保留基础绘制/功能，不能通过关闭安全保护来解决工具链故障。
- x86程序加载的C库/DLL必须为x86，不能直接加载x64 Office/插件DLL；资料通过受控系统关联打开，跨位数真实软件行为仍要验证。

当前已观察：现代GNU开发探针导入WaitOnAddress、GetSystemTimePreciseAsFileTime等新API；Win7GNU专门目标初次静态筛查没有已知风险项，却在Win11启动前崩溃；更换为保留ASLR/NX的LLD链接后，Win11隔离自检、原生离屏及第三轮实际工作台闭环通过。因此“静态扫描零风险”不能单独证明可运行，整改后的Win11证据也不能替代正式MSVC/Win7验收。

## 3. 每个系统分别验收

| 验收域 | 必须覆盖的内容 | 关联任务/测试 |
| --- | --- | --- |
| 桌面与输入 | 分层窗口、3%材质、穿透、PPT/希沃前台避让、恢复不抢焦点、Win+D、任务栏/Explorer恢复、鼠标/触摸/白板笔 | P0-03、P1-03/04/07；Q01/Q21/Q23 |
| 显示与电源 | 系统DPI/适用的每显示器DPI、多屏拔插、4K、锁屏/切换用户、睡眠唤醒、改时/跨日 | P1-03/04/05、P4；Q01/Q07/Q12 |
| 数据与授权 | ProgramData/最小ACL、固定账号、目录锁、WOW64路径/注册表行为、DPAPI、KDF、撤权、崩溃/磁盘故障、备份/恢复 | P1-02/06/08/09、P2；Q02/Q11/Q15/Q25 |
| 资料 | 本地受控副本、系统已有Office/PDF软件关联、删除/锁文件/重解析点及返回桌面 | P3-02/08/09、P4-10；Q04/Q18/Q19 |
| 网络与教师网页 | HTTP.sys/证书/URLACL/防火墙、校园可达、TLS及浏览器可信HTTPS、四类业务和关闭排空 | P0-05、P6；Q09/Q10/Q24/Q25 |
| 声音 | 格式、设备查询、自然完成、停止/驱动归还、拔出/睡眠/撤权、PPT前台独立声音及旧任务不复播 | P0-08、P3-11/13、P6-09/10；Q24/Q25 |
| 安装与长稳 | ZIP/MSI、最低权限启动、真实注销登录自启动、升级/卸载/保留资料、依赖完整性、性能及72小时 | P2-04、P5全部；Q03/Q12/Q13 |

Win7的系统DPI与新系统的显示能力要分别取证，不能把一种DPI路径推断为所有路径可用。Windows 11上通过的规则/数据库测试，仅证明对应机器、程序架构和测试范围；不能代替Windows 10、Win7或希沃设备。

## 4. 证据与完成定义

每项记录OS build/位数、程序target/工具链/锁哈希、设备/驱动、测试步骤、预期/实测、限制及清理。只允许PASS/FAIL/BLOCKED/NOT_RUN。一个系统中“部分原型通过”不等于该系统产品验收通过。

P5-01/02/03/06/07对Windows 7、Windows 10和Windows 11逐个闭环后才能声称三系统正式兼容。当前不标“全平台兼容”、不创建正式发布标签、不用未通过的Win7GNU实验产物替代正式包。新增平台需求不打断正在进行的Rust基础、Windows11隔离验证及故障整改。

第三轮平台证据：[Win11最终169项及工作台闭环](验证记录/2026-10-30-windows11-application-round3.json)、[最终Win7 GNU在Win11的补充运行](验证记录/2026-10-30-win7-gnu-application-on-windows11.json)。固定源树/锁哈希与二进制SHA256均可追溯；测试临时部署与正式ProgramData隔离。当前CUI子系统、不可见控件测试与离屏渲染不能代替正式GUI、真实自启动、DPI/触控或Win10实机验收。

第四轮新增原生网格/数字掩码、双表历史/独立校历/草稿恢复及持久高水位：[Win11最终205项与66步闭环](验证记录/2026-10-30-windows11-timetable-round4.json)、[Win7 GNU静态审计](验证记录/2026-10-30-win7-gnu-round4-imports.json)、[Win7 GNU在Win11补充运行](验证记录/2026-10-30-win7-gnu-timetable-on-windows11.json)。原生14项与100次重建稳定检查不能代替实际白板笔、多DPI、可见桌面或72小时证据；实际Win7/Win10状态不变。


第五轮已将显示设置独立CAS、一致桌面事实、真实常驻D2D卡片、三种时钟/局部倒计时与全文分页接入生产组合：[Win11 242项及90步闭环](验证记录/2026-10-30-windows11-desktop-round5.json)、[原生18项离屏专项](验证记录/2026-10-30-windows11-round5-resident-desktop.json)。API与依赖仍受三系统共同基线约束；Win10设备尚未连接，继续独立标BLOCKED，不因Win11通过改为PASS。第五轮当时4K课间单张25501952字节超过16MiB总DIB预算并隐藏/诊断；该失败保留，第六轮低DIB路径已通过自身场景复测，真实设备门槛仍未完成。

## 5. 第六轮增量：资料适配与低DIB背景（隔离回归已归档）

本轮新增实际文件选择/固定stage、WIC完整图片decode、PDF隔离helper及系统关联STA；`pdf-syntax=0.6.1`关闭默认特性仅开std，helper限制80MiB进程提交内存/CPU10秒/墙钟30秒。系统打开使用固定受控副本及NOASYNC，准备后fresh2s许可，实际调用前仍核对精确会话或当前Scene。新API/传递依赖及其Win7导入必须针对本轮最终产物重新审计，不沿用第五轮PE结果。

| 本轮证据 | Windows 11 | Windows 10 | Windows 7 SP1 x86 |
| --- | --- | --- | --- |
| 自身合成1080p/4K原生区域背景/资料行输入专项 | PASS（定向、离屏），[报告](验证记录/2026-10-30-windows11-round6-native-4k-materials.json) | BLOCKED，未连接设备 | BLOCKED，无真实Win7环境 |
| 最终受测源码/现代构建/全测试与实际materials-probe | PASS（局部功能），296项/90步/资料链及107输入哈希已归档 | BLOCKED，不能拿Win11替代 | BLOCKED，正式MSVC/实机证据未齐；GNU补充另记 |
| 可见4K/PPT/希沃/触控/DPI及OS背景内存/工作集 | NOT_RUN | BLOCKED | BLOCKED |
| 完整D11/主动副本清理UI/产品安装及72小时 | NOT_RUN | BLOCKED | BLOCKED |

原生定向应用DIB合计：1080p为3025792字节，4K为4806080字节，保持16MiB上限；系统管理的backing/compositor内存单独计，未取得可见真机性能证据。[旧修复前记录1](验证记录/2026-10-30-windows11-round6-native-4k-materials-before-fix-1.json)、[记录2](验证记录/2026-10-30-windows11-round6-native-4k-materials-before-fix-2.json)、[记录3](验证记录/2026-10-30-windows11-round6-native-4k-materials-before-fix-3.json)不删除。通过自身合成区域预算不等于实际显示器/白板4K验收，旧25501952字节背景FAIL也不再被描述为新路径当前预算。

原件2GiB与launchcopies200MiB分账、管理20/桌面5项、固定版本/实际哈希、stage到提交同inode与删除回收已接代码和定向测试；副本最后写入后24小时宽限，锁定不强删，已接本地全局管理员主动清理及真实双worker争用保留会话反例。本轮最终开发package及隔离资料链证据已归档，不提前宣称Win7/10/11正式兼容或创建产品Release；Windows SSH长期测试授权继续保留。

第六轮最终[Win11 x86 296项与资料/90步工作台](验证记录/2026-10-30-windows11-materials-round6.json)、[107构建输入绑定](验证记录/2026-10-30-materials-round6-build-inputs.json)、[Win7GNU PE审计](验证记录/2026-10-30-win7-gnu-round6-imports.json)、[Win7GNU在Win11的相同资料/工作台复测](验证记录/2026-10-30-win7-gnu-materials-on-windows11-final2.json)通过对应局部门槛。错误信息路径引入的RoOriginateErrorW已移除，静态Win7已知风险触发数归零；这依然不是实际Win7/Win10、正式MSVC或完整产品验收。

## 6. 第七轮增量：临时换课与持久实际老师

[Win11现代x86最终回归](验证记录/2026-10-30-windows11-lesson-history-round7.json)305项/15测试exe、98步隐藏工作台及合成资料链通过，原生28项包含在305中。新增独立顶部按钮、有效科目/颜色/资料来源整包与64个持久区间；保持16MiB全部应用DIB预算。Windows7GNU补充构建按同一112输入重新审计/运行，不能视为真实Win7或正式MSVC。

实际Win10/Win7设备仍未连接，PPT/触控/DPI/完整profile布局、教师Web/WinMM、安装与72小时保留原状态。无充分课程历史时中性隐藏，可信基础课程无观测恢复尚未完成。各目标独立取证；SSH服务和长期授权保留。

[第七轮Win7GNU PE审计](验证记录/2026-10-30-win7-gnu-round7-imports.json)零结构错误/零已知风险触发；[该补充EXE在Win11的98步/资料链](验证记录/2026-10-30-win7-gnu-lesson-history-on-windows11-round7.json)通过。它与[现代目标最终305项](验证记录/2026-10-30-windows11-lesson-history-round7.json)共用112输入哈希；补充nightly编译诊断保留，正式MSVC/Win7/Win10仍未验收。

第八轮证据：[现代x86完整局部回归](验证记录/2026-10-30-windows11-voice-copy-round8.json)、[Win7GNU同码在Win11补充运行](验证记录/2026-10-30-win7-gnu-voice-copy-on-windows11-round8.json)。声音只输出全零PCM，未调整音量/录音/前台；不能替代真实音频通知效果或Win7/Win10设备证据。


## 7. 第九轮增量：正常音频域与 HTTP 完成端口

正常按需音频域已接入独立Web精确会话、托盘/WTS/电源/设备停止栅栏与先音频后writer关闭。现有Win11 x64上的x86回归只使用全零PCM与自有隐藏窗口；不改变用户前台、音量、麦克风、实际锁屏或休眠。Win7GNU补充产物的导入审计与在Win11运行分开归档，不能替代实际Win7 SP1 x86或Win10实机。

HTTP.sys v2基础适配仅由显式127.0.0.1合成诊断运行，正常生产监听未开放。首请求尚未归属应用前，应用级HeaderWait不能覆盖driver默认预算；连接QoS、完整TLS/配对/教师浏览器与三系统异常矩阵保留NOT_RUN。测试只创建并删除自己的URLACL，不修改系统全局HTTP超时、证书绑定、防火墙或持续SSH授权。

本轮最终哈希、各类通过/失败与清理记录见[第九轮实施记录](10-实施进度与验证记录.md)。系统门槛继续独立：Win11局部回归、Win10未连接、Win7实机/正式MSVC未验证。

## 8. 第十轮增量：配对、Web会话与持久账号注销

本轮在共同Rust业务层实现短时码/256-bit秘密、本地精确主管理员审批、多设备独立Web lease、单设备撤销及持久self logout；应用端口新增公开状态、POST配对/POST轮询、GET session和POST logout，并将来源/framing preflight置于Cookie认证前。源码不因当前测试机为Win11而允许静态引入仅Win11可用的依赖。最终现代/Win7GNU导入和二进制必须按本轮源码重建、重新绑定证据，不能沿用前轮哈希。

| 验证层 | 本轮现状 | 三系统证据边界 |
| --- | --- | --- |
| Host规则、真实SQLite及真实Backend应用端口 | 最终release 352项/28结果组PASS，绑定145输入 | 命令总293.56秒含编译；证明规则/持久/端口行为，不证明Windows HTTP.sys、系统证书或浏览器 |
| Win11 x86完整隔离回归及`--web-pairing-probe` | 最终433项/16个debug测试exe及release主EXE组合PASS，远端清理PASS | 探针直接调用服务进行配对，再经request-port验证session/CSRF/logout；不是POST配对HTTP全链或实际TLS/browser |
| Win7GNU PE及在Win11补充运行 | 最终PE结构错误0/已知触发0，补充组合probe PASS，清理PASS | 本次未运行测试exe；只作为补充工具链/API与Win11运行证据，不是Win7或Win10实机 |
| Win7 SP1 x86、Win10 32/64位及实际希沃设备 | 保留BLOCKED/NOT_RUN的原独立门槛 | 不以Win11、Host或静态扫描代验 |

本轮曾取得修复前debug Host351项/28组通过，随后发现真实UI输入进入配对账本后，旧tick签发会被ClockRegression拒绝的可用性时序。已用真实`note_input`无sleep反例在旧源码复现FAIL，生产改为签发紧前重新读取时间，修复后定向及最终release Host352项已通过。旧debug结果独立保留，不与新结果相加；现代Windows和Win7GNU补充目标均已按最终145输入重建运行，不能用旧351项或前轮Win11计数代替。具体原文与源树绑定见第十轮实施记录。

Win11首轮整体FAIL并清理PASS：主release EXE的配对隔离探针通过，14/16个测试exe已有通过结果，bootstrap库debug测试exe超过runner60秒而无内部轨迹；不将首轮超时唯一归因于慢。本轮测试exe实际为未优化debug，与第九轮release测试配置不同；harness以同源码/同exe哈希、600秒有界预算及逐项日志复测后最终PASS。库exe实际36项107.44秒，bin为36项3.85秒，完整433项/16exe见[最终报告](验证记录/2026-10-30-windows11-web-pairing-round10.json)；首轮失败独立保留。主EXE仍为release，可见绘制为0及实际TLS/browser/listener均未验证，不能用局部通过代表完整产品完成。

[Win7GNU在Win11的补充报告](验证记录/2026-10-30-win7-gnu-web-pairing-on-windows11-round10.json)仅运行组合probe，没有额外测试exe；[开发包](验证记录/2026-10-30-web-pairing-dev-package.json)固定五成员且独立重包字节一致，仍为NOT_VALIDATED。真实Win7/Win10、正式MSVC和产品安装门槛不随补充PE/运行通过而改变；[最终清理](验证记录/2026-10-30-round10-cleanup.json)PASS，本地清除约6.39GB本轮残留，自有Windows目录/任务/probe进程均0，SSH服务、公钥和仓库外0600私钥保留。

Web业务服务默认关闭，逻辑enable不创建监听、URLACL、证书绑定或防火墙规则；生产HTTPS和教师浏览器仍未交付。P0/P6-11首连接头期限缺口继续保留，完整HTTP验收不能因配对/注销端口通过而完成。后续三系统分别检查实际TLS信任、Cookie/CSRF、关闭/撤权/重启、并发预算和首连接头期限；可见PPT/触控/DPI、设备音频、正式安装及72小时仍按原矩阵独立取证。

测试继续沿用已授权Windows机器，保留SSH服务/公钥及仓库外长期私钥；仅清理本轮自建进程、目录、计划任务及测试构建，不中断用户已有前台工作。最终结果与清理报告统一归档至[第十轮实施记录](10-实施进度与验证记录.md)。


## 9. 第十一轮增量：设备管理与浏览器资源

新增原生设备/申请管理页、一致的ManagementSnapshot与React配对/会话页面；正常就绪状态保持Unavailable，启用/短码请求在UI及worker两层拒绝，cfg(test)私有模拟只验证原生控件与业务服务。静态资源仅按受限清单核验后从共享内存响应，不开启正常产品监听。浏览器synthetic拦截验证、Host规则、Win11原生和真实TLS分别记账，不能将HTTPS测试URL的route.fulfill视为真实握手。

本轮缓存304修复在自有HTTP loopback诊断核对实际wire headers/body，未修改系统全局超时、证书、防火墙或SSH服务。Win7GNU继续只作补充导入/Win11运行证据；实际Win7 SP1 x86、Windows10 32/64位与正式MSVC门槛仍各自保留。最终源码/测试/清理绑定见[第十一轮实施记录](10-实施进度与验证记录.md)。

本轮最终Win11为449项/16个debug测试EXE（未优化）与release主EXE配对/隐藏工作台/资料/全零音频链PASS，[报告](验证记录/2026-10-30-windows11-teacher-web-round11.json)；新原生页7项已包含，不另加。浏览器30项属于Linux headless synthetic拦截，未走真实Windows TLS；HTTP loopback两轮304实际wire和完成排空通过但首连接期限仍NOT_VALIDATED。现代PE0/5、Win7GNU0/0，后者在Win11组合probe通过且未运行额外test EXE。三系统整体验收仍未完成。


## 10. 第十二轮增量：今日作业与共同提醒

教师内容API、真实SQLite读写/原命令查询与React编辑均按D11接入；Host380项release与浏览器45项synthetic检查分开记录，现代Windows与Win7GNU均重建。strict Web lease、CSRF、CAS、清空保留修订、原提交者回执、跨日持久高水位和结果不确定语义见[第十二轮实施记录](10-实施进度与验证记录.md)及[内容契约](12-教师内容API契约.md)。

浏览器在Linux headless使用生产静态资源及合成拦截API，真实Windows内容probe使用DPAPI/SQLite与应用请求端口并标明synthetic_request_metadata=true。两者均不能代替Windows真实TLS/校园网络/办公室浏览器或可见希沃验收。正常监听保持关闭，HTTP.sys首连接P0保留；29项官方来源研究未解除该门槛。实际Win7 SP1 x86、Win10 32/64位、正式MSVC、安装/设备矩阵与72小时仍独立保留。

第十二轮[Win11最终报告](验证记录/2026-10-30-windows11-teacher-content-round12.json)为468项/16个未优化debug测试EXE PASS，主release EXE独立断言WEB_PAIRING_PASS与WEB_CONTENT_PASS，清理PASS；bootstrap库47项154.65秒、bin43项11.28秒，单EXE预算600秒。真实DPAPI/SQLite仅证明端口业务与隔离原生链，视觉仍NOT_RUN。

[第十二轮Win7GNU在Win11补充运行](验证记录/2026-10-30-win7-gnu-teacher-content-on-windows11-round12.json)也通过两Web端口标记、隐藏原生/资料/全零声音及清理，无额外test EXE；PE为0结构错误/0已知触发。不能替代实际Win7 SP1 x86、Win10 32/64位或正式MSVC的独立证据。

## 第十三轮增量：教师资料与TCP接纳隔离候选

[现代x86最终487项/16个debug测试程序](验证记录/2026-10-30-windows11-teacher-materials-round13.json)与release主程序组合通过，三项Web标记分别覆盖配对、作业/提醒、受控材料端口；新增Windows单测实际执行全局两个物理stage预约、proof跨实例存活及下载权限回调中的删除竞态反例。真实文件探针覆盖TXT、Office、WIC、PDF helper和50MiB流式传输，但使用合成RequestHead/受控字节，不是网络Read期限或TLS证据。隐藏窗口/全零声音没有取得可见/PPT/触控验收，清理PASS且长期SSH授权保留。

[Win7GNU最终产物PE](验证记录/2026-10-30-win7-gnu-round13-imports.json)为0结构错误/0已知触发；[在同一Win11上的补充运行](验证记录/2026-10-30-win7-gnu-teacher-materials-on-windows11-round13.json)PASS、0额外测试EXE、cleanup PASS。[现代PE](验证记录/2026-10-30-modern-round13-imports.json)仍有5个已知Win7风险。两产物绑定相同220个冻结输入，现代SHA516b20a7…、Win7GNU SHA591d70df…；实际Win7/Win10及正式MSVC仍未取证，不能由补充运行推定三系统正式兼容。

[独立TCP接纳候选](验证记录/2026-10-30-tcp-frontdoor-round13.json)仅loopback自有四槽实验。生命周期排空通过，但严格5秒观测FAIL（5/8超限，最大超577微秒）；不改系统全局HTTP、证书、防火墙或持续SSH配置，P0保持UNRESOLVED，正常HTTPS监听关闭。网页109单测/48项synthetic浏览器及开发包NOT_VALIDATED范围见[本轮实施记录](10-实施进度与验证记录.md)。


## 第十四轮增量：网页语音应用端口与受控录音

[Win11最终494项/16程序](验证记录/2026-10-30-windows11-teacher-voice-round14.json)和[Win7GNU构建在同一Win11的补充探针](验证记录/2026-10-30-win7-gnu-teacher-voice-on-windows11-round14.json)均PASS、cleanup PASS，绑定最终247输入SHA `b64b0d64606dbb6d480645b1a6b5b09317e650e836ec69dfc530f61612a051ba`。Win7GNU补充没有额外运行单测，不能把它重复计入494。两份组合报告均断言四个Web标记，规范全零WAV经真实DPAPI/SQLite、默认总3遍WinMM自然完成、查询、停止及驱动归还；web-voice-probe只在自有隐藏交互任务中运行，旧非交互配对探针仍保持三个标记且不触发音频。

两个新Windows语音proof反例与既有资料容量用例均实际通过：跨资料/语音共享两个物理上传owner，SQL过期或旧App销毁不提前释放proof；测试fixture间共享锁只隔离相互干扰，生产全局容量与runner并发2不变。COMMIT后授权变化一律不确定结果；角色/精确班级/CSRF门禁保持。

[浏览器31检查](验证记录/2026-10-30-voice-ui-browser-synthetic-round14.json)及[真实Worklet→Offline→Worker通路](验证记录/2026-10-30-recorder-browser-synthetic-round14.json)采用合成零PCM MediaStream。自签HTTPS仅用于加载真实Worklet资源，不是校园证书信任或正常HTTP/TLS监听验收；无实际硬件麦克风、PPT共存或扬声器可听证据。现代PE仍5项已知Win7风险；Win7GNU扫描0风险仅静态补充。实际Win7/Win10/正式MSVC、可见希沃/触控/DPI、安装和72小时状态不变。

[最终Windows只读清理](验证记录/2026-10-30-windows11-owned-cleanup-round14.json)确认自有测试目录、任务和进程均0，sshd Running，仓库外私钥0600保留。全部开发目标继续active，正常监听与严格5秒P0门槛仍待完成。


## 第十五轮增量：账号编辑、多任教范围与独立busy锁定

[Win11现代x86最终512项/16程序](验证记录/2026-10-31-windows11-accounts-round15.json)PASS、cleanup PASS；新增3个原生锁定、9个账号Editor、1个应用草稿/锁定及5个Backend用例包含在512中。隐藏工作台通过账号创建、多班级/科目、课表引用拒绝、最后管理员保护、本人变更撤权后重登录和取消流程。独立LOCK在busy时只发送空表单并清秘密；探针仅写自己的隐藏非秘密Text控件，未使用全局输入或抢前台。

[Win7GNU补充程序在同一Win11运行](验证记录/2026-10-31-win7-gnu-accounts-on-windows11-round15.json)也通过上述组合、四个Web标记及清理，0额外单测EXE。两产物均绑定250输入SHA `f82ea02da0deee4145c74bdd55cc85a5d1ddd762fbd1d649e96950edb48d47dd`；现代0159f7e1…与Win7GNU 0b178c13…分别记录，PE仍为0结构错误/5个已知风险与0/0。实际Win7 SP1、Win10 32/64位、正式MSVC、真实希沃/PPT/触控/DPI、可信TLS、安装和72小时状态不变。正式兼容目标不以同一台Win11上的两种程序代替。


[本轮远端只读清理审计](验证记录/2026-10-31-round15-remote-cleanup.json)确认自有测试目录/任务/进程均0、sshd Running及仓库外私钥0600保留；[110项最终绑定复核](验证记录/2026-10-31-round15-final-review.json)与[本地清理](验证记录/2026-10-31-round15-cleanup.json)分别归档。测试通过与三系统完整产品验收继续分开记录。


## 第十六轮增量：公共布局与科目场景

[Win11现代x86最终585项/16程序](验证记录/2026-10-31-round16-windows-final.json)PASS、runner cleanup PASS，绑定[266输入](验证记录/2026-10-31-round16-source-inputs-final.json)SHA `9c66fb8a7d3c2df4dee2626885db17269fe1172f510ae7214ccb93c89dd25520`。测试程序为debug opt0/debuginfo2、线程2，主程序release；原生39步profile标记、既有工作台/资料/全零声音和四个Web标记分别通过，仍使用自身隐藏窗口、合成内容与可信请求元数据，不是可见白板或实际TLS链。现代主EXE SHA `de186ac4a885dcf6ee54f42a0acd9de8f4dfb5a81c88e196413d75a7897170ed`，8682626字节。

两轮旧bin各68通过/6失败和[精确回溯](验证记录/2026-10-31-round16-idle-fault-diagnostic.json)继续保留；显式reaper修复后的单项1通过不与最终585累计。Win7GNU补充EXE SHA `fd538a5c604c9a7d81c4ea44fa533fa4473f5dca132fd0289fa2eae0338a90ee`，8794112字节；[PE检查](验证记录/2026-10-31-round16-win7-imports-stable.json)0/0；[Win11同机补充运行](验证记录/2026-10-31-round16-win7-on-windows11.json)组合与清理PASS，新增单测EXE为0。[现代PE](验证记录/2026-10-31-round16-modern-imports-stable.json)仍有5项已知Win7风险。[开发包13成员及重包一致性](验证记录/2026-10-31-round16-package.json)、[最终绑定复核](验证记录/2026-10-31-round16-final-readonly-review.json)和[远端清理](验证记录/2026-10-31-round16-remote-cleanup.json)PASS，[整轮本地清理](验证记录/2026-10-31-round16-cleanup.json)PASS，同步详见[第十六轮实施记录](10-实施进度与验证记录.md)。

Windows 7 SP1 x86、Windows 10、Windows 11继续是三个正式目标。实际Win7/Win10设备本轮尚未测试；补充GNU构建/导入审计或在同一Win11上运行不能代验。隐藏原生示例画布/合成拖动不是可见希沃/PPT/触控/DPI与真实桌面图标空间的验收，独立文字/图片/TXT/分页图片和P4完整布局也未完成。严格5秒P0仍FAIL，正常可信TLS/校园网络、正式安装与72小时门槛不变。完整目标继续active，长期Windows SSH授权保留。


## 第十七轮增量：统一内容测量与原生排版

[契约18](18-统一内容测量与原生排版.md)统一场景与DirectWrite的完整正文分块、可用宽度、块内留白、资料行距、操作44 DIP最小高度及末块padding；固定/百分比父宽先解析再传给子树。动态钟面/课间区域、0%布局、资源上限与实时文件授权保持。

[Host实际457/28 PASS](验证记录/2026-10-31-round17-host-final.json)；[Windows最终600个不同用例/16个产物覆盖通过](验证记录/2026-10-31-round17-windows-final.json)：完整16程序600 PASS后，最终目录夹具变更只改变1个lib测试程序，再补跑65，其余15程序哈希未变。不是同一最终快照单次600，也不合计为665。新增10项纯规则、4项DirectWrite/分页和1项实际画布间距回归均在这些数内。[原生39步及资料/全零音频/Web端口组合](验证记录/2026-10-31-round17-windows-post-handshake.json)绑定最终相同release EXE，runner cleanup通过；实际主机仍是Win11 Pro x64 build26300/WOW64，不是可见希沃或Win10/Win7设备证据。

最终269仓库输入SHA `d2cd0aec…`；实际Rust构建/Host快照`e069d16f…`后只修改非编译runner的远端哈希采集，Rust、EXE和包内容未变。初始语音两项失败、单项波动、ExitCode=null采集错误、Host目录冲突及旧runner读取后来本地hash的限制全部保留，详见[本轮进度与历史](10-实施进度与验证记录.md#第十七轮统一内容测量与原生排版)。临时分阶段诊断已还原；只保留测试预算预留握手和目录序号，生产100ms及成功断言未放宽。后续成功不唯一证明原语音失败原因。

[开发包](验证记录/2026-10-31-round17-package-final.json)13成员、CRC/SHA和同名独立复现通过，全部平台与完整应用门禁仍未完成；静态导入现代GNU0结构错误/5已知Win7风险，补充Win7GNU0/0。[Win7GNU同一Win11的原生/系统/39步工作台/资料及cleanup通过](验证记录/2026-10-31-round17-win7-on-win11-final.json)；[修复后runner实际远端哈希核验通过](验证记录/2026-10-31-round17-runner-hash-runtime-verification.json)。该核验随行的50项纯规则来自现代GNU测试EXE，不作Win7用例或新增600计数；未请求音频/Web组合重复运行。[整轮清理PASS](验证记录/2026-10-31-round17-cleanup.json)：157个自有顶层路径、13,662文件、6,199,262,816逻辑字节，命名空间剩余0；远端目录/任务/进程均0、sshd Running、私钥0600。预存IDE target/dist/node_modules及全局缓存保留，整轮验证后同步既有仓库main；实际提交SHA以Git历史及本轮回复为准。

正式Windows7 SP1 x86、Win10设备/MSVC、希沃可见触控/DPI/PPT、安装及72小时仍待各自证据。P0严格5秒保留FAIL，正常可信TLS/生产监听关闭，完整目标继续active；长期Windows SSH服务、公钥和仓库外私钥保留。


## 第十八轮增量：独立内容与共享WIC准入

[整轮核验](验证记录/2026-10-31-round18-final-verification.json)覆盖Host分阶段504项与Windows分阶段691个不同用例/16个非零测试程序，完整运行、受影响补跑和失败历史分开。新增独立文字/TXT、最多50页图片、清空/Undo、实际内容预览、共享32MiB光栅/4MiB文字缓存、原件共用2GiB与恢复/回收。原生85项、真实文件/SQLite及96项工作台范围均有独立源/EXE绑定。

现代GNU与专门Win7GNU的最终release均在同一Windows11 Pro build26300 x64/WOW64上通过原有39步布局、新增52步内容工作台及隐藏原生/资料/全零音频/Web端口组合，远端实际EXE逐次SHA核对。新WIC准入保留原30秒墙钟/10秒CPU和单物理解码器；发布及翻页实际预COMMIT重复原权限/场景与任务取消/期限。所有Windows测试进程、目录与计划任务均按独占标记清理，持续SSH授权保留。

没有新增真实Win7 SP1 x86、Win10、正式MSVC、可见希沃/触控/DPI/PPT、TLS/生产监听、安装或72小时证据，相应状态保持NOT_RUN/NOT_VALIDATED。补充Win7GNU在Win11成功不能写作Win7通过；完整目标继续active。


## 第十九轮：课表复合草稿操作与实际范围

[总报告](验证记录/2026-10-31-round19-final-verification.json)记录Host525项与Win11分阶段718个不同Windows用例/17个非零程序。首个完整718项通过后，因诊断页路径修正而变化的唯一工作台测试EXE又完整102项通过；其他16个最终EXE的SHA与原受测程序一致，不把重复测试加入总数，也不称为修正后一次完整执行718项。

[现代GNU](验证记录/2026-10-31-round19-modern-verified-combined.json)及[补充Win7GNU](验证记录/2026-10-31-round19-win7-verified-combined.json)均在同一Win11 Pro build26300 x64/WOW64完成65步角色交换/逐日活动拆合/类型损失确认/CAS读回与原语义恢复，并通过原39步布局、52步内容、隐藏宿主及合成资料/Web请求端口。首次两个组合运行的探针页路径失败保留，修正为真实返回列表/取消/重读后通过。

最终两产物PE结构错误均0，现代已知风险5、补充Win7GNU0。当前没有新增Win7、Win10或正式MSVC实际证据，也没有新增可见希沃/触控/DPI/PPT或真实TLS/安装/72小时证据；三系统状态继续分别维护。SSH及清理见本轮总报告与证据索引。


## 第二十轮：公共校历候选与采用

[最终核验](验证记录/2026-10-31-round20-final-verification.json)记录Host同冻结输入594项和Windows18个测试程序799项完整通过；当前实际机器仍为Win11 Pro build26300 x64/WOW64。现代与补充Win7GNU分别完成30步合成候选原生复核/采用/策略/锁定/CAS，以及原有39步布局、52步独立内容、65步课表组合。两个独立下载诊断实际通过固定2026来源WinHTTP HTTPS和严格解析，声明通知正文及同步硬抢占仍未证明；不能替代教师入站TLS/P0。

最终321输入、两EXE实际远端SHA、完整测试和清理分别绑定。现代PE0结构错误/5个已知Win7风险，补充Win7GNU0/0；后者仅在同一Win11上运行，没有新增实际Win7 SP1、Win10或正式MSVC证据。可见希沃/PPT/触控/DPI、安装、72小时及完整产品状态不变，原目标继续active。长期SSH服务和公钥授权保留。


## 第二十一至二十四轮：自启动、普通恢复及无恢复码重新初始化

[实施记录](10-实施进度与验证记录.md)分别归档自启动25步、普通备份/恢复23步及独立重新初始化cancel/commit/reopen链。本轮最终Windows为1001个不同测试/19程序，Host为693项；受影响程序完整重跑，未变程序只在实际EXE SHA完全一致时复用本轮成功结果，不能描述成最终一次全19执行。重新初始化还实际覆盖超过260字符的候选目录准备/ACL回读/取消，不修改系统长路径策略，不把路径修复当作Win7兼容认证。

实际机器继续是Win11 Pro build26300 x64/WOW64。现代GNU与专门Win7GNU均在同一授权机器验证，后者不是Win7或Win10实机。普通同安装恢复保持当前安全身份；无恢复码清除在独立从未打开旧库的进程确认放弃13树资料，后续创建新DPAPI/学校/主管理员/随机安装标识，旧包直接恢复被原安装门禁拒绝。外部另存副本与Windows/SSH设置保留。

实际Win7 SP1 x86、Win10 32/64位、正式MSVC、可见希沃/PPT/触控/DPI、真实注销登录、安装升级卸载及72小时证据均未新增。生产TLS/监听、严格P0与跨安装显式映射仍需继续，完整开发目标保持active。


## 第二十五轮：跨安装显式映射恢复

当前[Win11完整1037项/19程序及16步映射](验证记录/2026-10-31-round25-modern-windows-tests-and-migration-stage1.json)通过，Host706项通过。普通直接恢复仍拒绝其他安装；独立映射链要求全部来源目录/账号逐项对应，保留目标安装、完整目录、当前凭据和权限，整体替换教学数据。两个默认未确认的原生确认项之后才真实关闭writer/worker/audio并进行三树切换，独立程序正常Open后使用目标现有账号登录。

[补充Win7GNU同机迁移](验证记录/2026-10-31-round25-win7-migration-on-win11-stage1.json)同样16步通过，实际系统仍是Win11 Pro build26300 x64/WOW64。源包、目标密钥及真实项目Run不变，探针隔离目录/任务均清理。现代PE保持0结构错误/5项既有Win7风险，补充PE0/0；没有新增真实Win7、Win10或正式MSVC证据。

冻结398输入，最终仅有打包说明段落次序调整，Rust生产和受测程序SHA保持实际绑定；前端61输入未变，复用旧资源，不声称新浏览器或npm执行。最终两种EXE的既有原生回归、ZIP逐SHA/CRC、清理及持续SSH授权以[实施记录](10-实施进度与验证记录.md)和本轮最终核验总表为准。可信TLS/生产监听、严格P0、可见希沃/PPT/触控/DPI、真实注销登录、安装升级卸载与72小时验收继续独立待完成；完整开发目标保持active。

## 第二十六轮：Schannel隔离联调与执行域边界

现代GNU x86在同一Win11 Pro build26300/WOW64上实际完成两批八连接：4次受信TLS1.2握手、2个分片HTTP加密响应、3次原批次截止关闭；错误CA与错误主机名分别取得真实OpenSSL证书验证错误18/62。实际IOCP45/45、SSPI25/25和Frame25次归还，2次EXTRA共62字节真实保留处理；每批检查原生释放并等待实际SSPI线程退出后才开始下一generation。见[现代TLS实测](验证记录/2026-10-31-round26-modern-tls-stage5.json)，具体构建与完整最终回归另见实施记录。

客户端只信任本次公共PEM且校验localhost，不安装系统Root；测试结束证书、精确CNG私钥、进程和目录清理通过，MY集合摘要恢复，SSH授权保留。此测试使用现代系统的New-SelfSignedCertificate夹具，不能拿该cmdlet推断Win7；服务端旧SCHANNEL_CRED/SSPI代码与补充目标仍须各自审计及真实Win7/Win10运行。

已保留监督脚本读取共享日志失败、通用SSPI参数误用于Schannel和分片时TRAILER元数据误限已消费范围的原始失败。最后一项按真实155字节输入/124字节消费布局修复：未读取的HEADER/TRAILER限定原输入，实际DATA仍限定已消费范围，EXTRA保留31字节；没有扩大64KiB输入或16KiB明文额度。

本节证明有限诊断生命周期，严格首TCP五秒、持续接纳、close_notify、生产业务/教师浏览器/校园可信部署、完整资源及三系统验收均未通过，正常监听保持关闭。

第二十六轮最终完整现代GNU测试为19个程序1053项全部通过，逐一绑定构建/上传/执行SHA。补充Win7GNU静态导入无已知较新平台触发，并在**同一Win11**重复上述八连接TLS矩阵通过；这不填补真实Win7/Win10和正式MSVC栏。完整原生离屏探针 rendered_frames=0，无新增可见希沃/前台验收。两种TLS测试及Windows完整回归的自有证书/密钥/任务/进程/目录均已清理，持续SSH授权保留。

## 第二十七轮：本地远程语音设置的验收范围

现代GNU x86与补充Win7GNU x86均在同一台Windows 11 Pro build26300/WOW64执行25步原生设置流程：真实原生控件、固定双worker、SQLite关闭重开、锁定与旧确认拒绝、并发CAS后刷新及确切清理。该设置探针不创建音频任务；实际零PCM播放/取消、紧急停止和Web上传另由现代程序独立验证，原报告、失败迭代及最终执行SHA绑定见实施记录第二十七轮。它们不证明实际麦克风、可听性、前台触控或希沃屏幕效果。

首个候选超出原主页12按钮限制而失败，入口已移到“桌面与系统设置”，没有扩大原预算。Web上传初次第二次请求失败的具体状态未取得；单次加诊断复测通过，后续独立真实writer回归证实并修复权威发布期间的即时Closed缺陷，二者因果仍仅推断。最终完整Host及Windows测试数字和包身份以实施记录的最终报告为准。

本轮不填补真实Windows 7、Windows 10或正式MSVC证据，也不解除严格首TCP五秒FAIL；生产持续TLS/业务/浏览器、设备策略、GUI正式安装和72小时长稳仍须分别推进。持续Windows SSH授权保留。
