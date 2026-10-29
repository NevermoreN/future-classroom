# 未来教室 · 希沃白板桌面信息层

按 [D11 完整开发计划](开发计划/未来教室项目完整开发计划.md) 开发的原生 Windows 教室桌面项目。正式兼容目标为 Windows 7 SP1 x86、Windows 10、Windows 11。核心采用 Rust，教师端计划使用 TypeScript/React；白板端不嵌入浏览器。

当前为 **0.1.0 原生工作台开发里程碑**。正常 Windows 启动已接入原生首次设置/登录、学校与班级、基础科目、账号创建、首次改密/恢复码、本人作业，以及课表网格/数字时间/单双周和校历草稿；SQLite、DPAPI、精确会话和固定双 worker 均用于实际运行。完整桌面展示、其余设置编辑器、触控/DPI验收、HTTP.sys 教师端及文件/语音发布管线仍待实现。**完整产品尚未交付；Windows 7、10、11均未完成整体验收。**

[实施进度与验证记录](开发计划/10-实施进度与验证记录.md) 区分已实现范围、实测结果和未完成门槛。旧文档的“仅计划、不开发”是历史状态，已由最新开发授权取代；产品要求保持。

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
.\fc-bootstrap.exe --native-probe 30
```

诊断命令只写独立临时数据并清理；`--application-probe` 用隐藏原生控件执行真实工作台闭环，并验证课表网格、双表差异/历史恢复、独立校历、保存重读和锁定草稿；Office校验交给同一双worker池，不访问正式ProgramData。绘制探针限时运行、透明穿透、不激活窗口，仅在确认桌面前台时显示。`rendered_frames=0` 表示没有取得桌面可见绘制证据。完整录音播放和教师管理服务仍未交付。

新构建的 Windows 自检还覆盖 DPAPI 密钥和 SQLite 重开、真实登录限速端口、改密撤权及一次性恢复；系统探针只读，宿主探针限时创建自身隐藏窗口和托盘。现有历史开发 ZIP 的命令和范围以其包内 manifest 为准，不能用新源码的验证替代旧二进制证据。

Linux 上有 MinGW x86 时可交叉构建补充目标：

```sh
rustup target add i686-pc-windows-gnu
cargo build -p fc-bootstrap --release --locked --target i686-pc-windows-gnu
python3 scripts/audit-pe.py --exe target/i686-pc-windows-gnu/release/fc-bootstrap.exe
python3 scripts/package-probe.py --exe target/i686-pc-windows-gnu/release/fc-bootstrap.exe -o dist/FutureClassroom-0.1.0-timetable-dev.zip
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
