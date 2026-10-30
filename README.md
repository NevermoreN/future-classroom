# 未来教室 · 希沃白板桌面信息层

按 [D11 完整开发计划](开发计划/未来教室项目完整开发计划.md) 开发的原生 Windows 教室桌面项目。正式兼容目标为 Windows 7 SP1 x86、Windows 10、Windows 11。核心采用 Rust，教师端计划使用 TypeScript/React；白板端不嵌入浏览器。

当前为 **0.1.0 原生桌面与工作台开发里程碑**。正常 Windows 启动已接入原生首次设置/登录、学校与班级、基础科目、账号创建、首次改密/恢复码、本人作业，以及课表网格/数字时间/单双周和校历草稿；SQLite、DPAPI、精确会话和固定双 worker 均用于实际运行。常驻桌面已接入一致事实快照、三种时钟、课表/作业全文分页、课间局部刷新及显示草稿/CAS保存和退出保护。第六轮已接入教师资料目录、真实文件校验/导入、删除回收和固定版本副本的系统关联打开；隔离资料闭环已通过。4K课间背景已采用低内存路径，并通过自身合成场景的原生定向预算测试，操作系统背景内存和可见真机验收仍未完成。第七轮已接入本地主管理员临时换课/恢复原课、实际老师区间持久化及桌面顶部局部输入；第八轮补齐跨夜稳定基础课证明、主动副本管理及持久声音/流式WinMM隔离链；Host279项、Win11 x86 350项/104步及全零声音链通过，完整声音产品流程仍待教师 Web 与浏览器接入，验证结果见实施记录。第九轮已接正常按需声音执行域、托盘/WTS/电源/设备停止栅栏及 HTTP.sys 固定槽 IOCP 基础适配，Host306项、Win11 x86 387项及工作台/资料/全零声音链通过；HTTP两周期完成排空通过，整项仍为部分验收，具体范围见实施记录。完整profile/布局编辑、触控/DPI、HTTPS部署/配对、教师网页及语音发布仍待完成。**完整产品尚未交付；Windows 7、10、11均未完成整体验收。**

[实施进度与验证记录](开发计划/10-实施进度与验证记录.md) 区分已实现范围、实测结果和未完成门槛。旧文档的“仅计划、不开发”是历史状态，已由最新开发授权取代；产品要求保持。

第十一轮新增原生教师设备管理页，以及 [React 教师配对/会话页面](web/README.md)。配对审批、设备精确撤销、明确注销和严格响应校验已接入。第十二轮继续接入今日作业和共同提醒的范围分页、读取、保存/清空、CAS冲突与原命令查询；资料和语音网页入口仍待接入。正常 HTTPS 未就绪时原生启用/短码按钮保持禁用。静态构建随开发包供复核，尚未由正常产品监听提供；浏览器 synthetic 拦截测试与真实 TLS/校园网络验收分开记录。

构建 `web/dist` 后可用 `cargo run -p fc-bootstrap --example verify_web_assets --release --locked -- web/dist` 将实际前端清单、大小/SHA、64KiB资源分块、缓存和 API 边界交叉检查。开发打包显式传 `--web-dist web/dist` 可加入经验证的静态文件与许可；不传时仍保持原五成员探针包，不把源码、node_modules 或任意目录放入 ZIP。

## 本地验证

固定开发工具链 Rust 1.97.1，依赖锁见 Cargo.lock：

```sh
cargo fmt --all -- --check
python3 scripts/check-architecture.py
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo run -p fc-bootstrap --locked -- --self-check
```

`scripts/verify.sh` 使用独立临时构建目录并在结束时清理。核心和SQLite测试可在Linux运行，原生Windows能力必须在Windows验证。

## Windows 开发探针

直接运行新构建的 `fc-bootstrap.exe` 打开工作台；`--autostart` 保持锁定并隐藏窗口。首次运行会在本机 ProgramData 的项目专属目录准备保护ACL、部署标记、数据库和当前用户DPAPI密钥，需自行设置应用主管理员口令。缺失或损坏的既有数据不会重新开放首次注册。应用角色和Windows账号权限彼此独立。

```powershell
.\fc-bootstrap.exe --self-check
.\fc-bootstrap.exe --system-probe
.\fc-bootstrap.exe --host-probe 2
.\fc-bootstrap.exe --application-probe
.\fc-bootstrap.exe --materials-probe
.\fc-bootstrap.exe --audio-silent-probe
.\fc-bootstrap.exe --native-probe 30
```

诊断命令只写独立临时数据并清理；`--application-probe` 用隐藏原生控件执行真实工作台闭环，并验证课表网格、双表差异/历史恢复、独立校历、显示设置保存重读、锁定草稿、临时换课/恢复/旧CAS拒绝、退出保护及真实事实到原生离屏场景；Office校验交给同一双worker池，不访问正式ProgramData。绘制探针限时运行、透明穿透、不激活窗口，仅在确认桌面前台时显示。`rendered_frames=0` 表示没有取得桌面可见绘制证据。`--materials-probe`已通过真实双worker/SQLite/文件校验/固定副本/私有系统关联接收器闭环，使用合成文件和临时关联，不调用日常软件或更改默认关联。本地主管理员可查看并清理已到期未占用副本。`--audio-silent-probe`只生成全零PCM，验证真实文件/原Web会话/SQLite/WinMM自然与取消释放链，并检查正常按需音频域的本地锁定独立性、紧急停止、后续显式任务与关闭顺序；不录音、不改音量或前台，教师管理服务仍待接入。

新构建的 Windows 自检还覆盖 DPAPI 密钥和 SQLite 重开、真实登录限速端口、改密撤权及一次性恢复；系统探针只读，宿主探针限时创建自身隐藏窗口和托盘。现有历史开发 ZIP 的命令和范围以其包内 manifest 为准，不能用新源码的验证替代旧二进制证据。

Linux 上有 MinGW x86 时可交叉构建补充目标：

```sh
rustup target add i686-pc-windows-gnu
cargo build -p fc-bootstrap --release --locked --target i686-pc-windows-gnu
python3 scripts/audit-pe.py --exe target/i686-pc-windows-gnu/release/fc-bootstrap.exe
python3 scripts/package-probe.py --exe target/i686-pc-windows-gnu/release/fc-bootstrap.exe -o dist/FutureClassroom-0.1.0-desktop-dev.zip
```

现代 Windows GNU 产物不能代表 Win7。正式 `i686-win7-windows-msvc`、完整依赖原型、干净 Win7 SP1 x86、希沃驱动/触控与72小时长稳等门槛均保留。

## 文档

- [开发导航与需求追踪](开发计划/00-开发导航与需求追踪.md)
- [任务与依赖](开发计划/04-开发阶段与任务步骤.md)
- [Rust 工程边界](开发计划/07-Rust开发与验证计划.md)
- [所有权与故障协议](开发计划/08-Rust所有权与FFI生命周期.md)
- [验证和发布门槛](开发计划/09-Rust构建验证与发布门槛.md)
- [Windows 7 / 10 / 11 兼容矩阵](开发计划/11-Windows兼容矩阵与验收.md)
- [界面图册](UI设计/01-最新界面图册.md)

原始学校参考图保留本地，不随开源仓库分发。效果图是设计稿，不能代替程序截图或运行验收。测试访问密钥保存在仓库外，不纳入源码、日志或交付包。

## 第六轮资料链路与4K进展

本地资料管理按老师/科目稳定ID隔离；主管理员可选全部目录，教师仅本人有效任教范围。管理页20项、桌面5项，按成功加入序号倒序分页；同名添加是独立新条目。导入校验实际内容与SHA256，原件固定版本保存在受控目录；文件名点击先准备独立副本，再以新鲜授权交给专用STA，使用系统关联`open`和`SEE_MASK_NOASYNC`。资料写入、权限重查及幂等结果通过唯一SQLite writer完成；外部程序已打开的文件不会被强行关闭。

单原件50MiB、原件总量2GiB，图片10MiB/8MP；打开副本独立200MiB、按副本最后写入时间保留24小时宽限，锁定文件不强删。PDF采用`pdf-syntax 0.6.1`的`std`特性，受80MiB进程提交内存/CPU10秒/墙钟30秒的隔离helper限制；WIC执行完整图片解码。主动清理已到期副本的本地主管理员入口已在第八轮接入。

[4K原生定向记录](开发计划/验证记录/2026-10-30-windows11-round6-native-4k-materials.json)已通过：1080p/4K的应用DIB合计分别为3025792/4806080字节，保持16MiB上限。记录仅覆盖自身合成场景和原生离屏路径；旧失败保留，Win7/Win10、希沃、可见桌面/PPT/触控、操作系统背景内存、完整性能与72小时门槛不变。第六轮最终受测源码/EXE哈希、测试总数、失败历史及开发包均见[实施记录](开发计划/10-实施进度与验证记录.md)。

## 第七轮当前课程与实际老师历史

临时换课只由本地主管理员提交，捕获完整课程实例与覆盖修订；下课或版本变化后拒绝旧操作，不能改到下一节。有任教候选必须明确老师，无候选需二次确认未分配。恢复原课终止当前覆盖，周表不改。

实际生效区间以有界64条记录持久化；课内重启可恢复活动覆盖，已经记录的课程在课间重启仍保留最终实际老师。课表/校历变化、撤权、换班、跨日与已观察的时间回拨会重新校验或终止；课后编辑不反推新老师。完全没有可信依据时保守显示中性状态；第八轮已增加稳定基础课证明，以补全可证明的无观测基础课程。

第七轮Host243项、Win11 x86 305项及98步隐藏工作台/资料链通过，最终证据见实施记录。

桌面顶部控制独立于8种显示组件，仅换课按钮接受点击，新增表面计入原16MiB总DIB预算；当前格名称/颜色与资料来源同一Scene切换。隐藏控件和离屏验证不替代三系统可见桌面、白板笔/PPT、profile布局或72小时验收。
