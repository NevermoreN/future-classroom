FutureClassroom 0.1.0 - Windows x86 development probe
状态：NOT_VALIDATED。此包是 Rust D11 开发里程碑，不是完整未来教室应用。

在 PowerShell 中解压后运行：
  .\fc-bootstrap.exe --version
  .\fc-bootstrap.exe --self-check
  .\fc-bootstrap.exe --native-probe 30

--self-check 在隔离临时目录中验证 SQLite 提交、权威快照发布、命令幂等、
Windows 随机源、PBKDF2 与目录互斥；退出时清理隔离数据。
--native-probe <seconds> 接受 1 至 300 秒，验证原生透明窗口、3% 玻璃底色、
时钟文字、点击穿透与前台避让。需在已登录的交互桌面观察实际效果。
SSH/无交互桌面的输出不能代替前台、遮挡、缩放、点击和视觉验收。
rendered_frames=0 代表没有确认桌面前台绘制，不能视作视觉通过。

完整桌面产品、课表/内容/资料/语音工作流、设置界面和教师管理 Web 均未交付。
本探针不配置开机启动，不提供常驻服务、远程管理端或安装程序。

兼容状态：
  Win7 SP1 x86、Windows 10、Windows 11 均为产品兼容目标。
  当前探针的 Win11 局部实测不等于整机全功能验收；Win10/Win7实机待测。
  Win7 SP1 x86：NOT_VALIDATED。
  正式 i686-win7-windows-msvc 基线与真机门槛：NOT_VALIDATED。
  modern GNU 构建只供开发验证；Win7 GNU 构建也仅为补充证据。
  静态 PE DLL/API 审查不能证明系统兼容，风险详见 manifest.json。
  依赖的系统更新或运行库不在此包中；须在实际测试机器验证。
  未通过正式 MSVC 与真机门槛，不得作为正式发布或创建发布标签。

包内仅含 fc-bootstrap.exe、LICENSE、THIRD-PARTY-NOTICES.txt、此说明与 manifest.json。
manifest.json 记录目标、工具链声明、文件 SHA256、探针范围和 PE 风险。
LICENSE 为项目许可；第三方组件、Rust/MinGW/GCC 运行时许可与版权见 THIRD-PARTY-NOTICES.txt。
