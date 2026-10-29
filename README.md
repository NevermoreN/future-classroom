# 未来教室 · 希沃白板桌面信息层

按 [D11 完整开发计划](开发计划/未来教室项目完整开发计划.md) 开发的原生 Windows 教室桌面项目。正式兼容目标为 Windows 7 SP1 x86、Windows 10、Windows 11。核心采用 Rust，教师端计划使用 TypeScript/React；白板端不嵌入浏览器。

当前为 **0.1.0 基础开发里程碑**，包含十二 crate、课表/权限纯规则、运行健康闸门、有界队列、真实 SQLite 单写者与一致备份、密码学、严格协议、布局和声音状态机，以及 Windows 原生绘制探针。完整桌面工作流、设置工作台、HTTP.sys 教师端、原生文件/语音管线仍待实现。**不能作为完整产品安装；Windows 7、10、11均未完成整体验收。**

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

```powershell
.\fc-bootstrap.exe --self-check
.\fc-bootstrap.exe --native-probe 30
```

自检只写独立临时数据并清理；绘制探针限时运行、透明穿透、不激活窗口，仅在确认桌面前台时显示。它不提供学校内容编辑、录音播放或教师管理服务。`rendered_frames=0` 表示没有取得桌面可见绘制证据。

Linux 上有 MinGW x86 时可交叉构建补充目标：

```sh
rustup target add i686-pc-windows-gnu
cargo build -p fc-bootstrap --release --locked --target i686-pc-windows-gnu
python3 scripts/audit-pe.py --exe target/i686-pc-windows-gnu/release/fc-bootstrap.exe
python3 scripts/package-probe.py --exe target/i686-pc-windows-gnu/release/fc-bootstrap.exe -o dist/FutureClassroom-0.1.0-windows-x86-dev.zip
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
