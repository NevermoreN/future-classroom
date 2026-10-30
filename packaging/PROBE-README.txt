FutureClassroom 0.1.0 - Windows x86 native workbench development build
状态：NOT_VALIDATED。此包包含可运行原生管理工作台，完整未来教室应用尚未交付。

直接运行 fc-bootstrap.exe 打开工作台；--autostart 保持锁定并隐藏窗口。
首次运行在本机 ProgramData 的 FutureClassroom 专属目录准备保护ACL、部署标记、
数据库和当前Windows用户的DPAPI密钥。请自行设置应用主管理员口令；无默认密码。
支持学校/班级、基础科目、账号创建、首次改密、恢复码、今日作业，
以及原生课表网格、固定数字时间输入、单双表深复制/保留历史、校历草稿。
课表页面先编辑草稿，“保存全部”才写数据库；取消需确认，锁定后同账号可继续草稿。
桌面显示设置支持三种时钟、秒数、日期倒计时、科目底色及逐组件显示/透明度/尺寸约束；
同样先编辑草稿，保存后生效。存在本账号未保存课表/显示草稿时，正常退出会提示处理。
常驻桌面已接入一致事实快照及原生卡片，在系统桌面前台显示课表、作业、时钟等；
外部应用、管理窗口和无效会话时隐藏，分页仅小按钮接收点击，其余区域穿透。
本阶段整份课表（含历史）上限64KiB，超限拒绝，不截断。学校/公共校历分别保存；
公共候选资料下载及来源核验尚未实现，不宣称公共假期已自动核验。
关闭工作台锁定本地会话；正常退出须由主管理员授权。缺失数据不会重新开放首次注册。

在 PowerShell 中解压后运行：
  .\fc-bootstrap.exe --version
  .\fc-bootstrap.exe --self-check
  .\fc-bootstrap.exe --system-probe
  .\fc-bootstrap.exe --host-probe 2
  .\fc-bootstrap.exe --application-probe
  .\fc-bootstrap.exe --materials-probe
  .\fc-bootstrap.exe --audio-silent-probe
  .\fc-bootstrap.exe --native-probe 30

--self-check 在隔离临时目录中验证 SQLite 提交、权威快照发布、命令幂等、
Windows 随机源、PBKDF2 与目录互斥；还验证DPAPI密钥与SQLite重开、
持久登录限速接口、精确会话、原子改密与恢复码一次轮换；退出时清理隔离数据。
--system-probe 只读发现系统适配，不创建正式数据目录或修改自启动。
--host-probe <seconds> 接受1至30秒，验证自身隐藏窗口、托盘和会话通知生命周期。
--application-probe 使用隐藏原生控件及隔离临时数据执行真实账号/科目/教师/作业、课表/双表/校历/锁定草稿闭环，
并在同一固定双worker池校验合成Office文件，不写正式ProgramData或自启动。
它还验证显示设置保存重读、锁定草稿、临时换课/恢复/CAS拒绝、退出保护及真实事实到原生离屏场景；不截图用户桌面。
--native-probe <seconds> 接受 1 至 300 秒，验证原生透明窗口、3% 玻璃底色、
时钟文字、点击穿透与前台避让。需在已登录的交互桌面观察实际效果。
SSH/无交互桌面的输出不能代替前台、遮挡、缩放、点击和视觉验收。
rendered_frames=0 代表没有确认桌面前台绘制，不能视作视觉通过。

资料管理可选择本机PNG/JPEG/BMP/TXT/PDF/PPTX/DOCX/XLSX，实际校验、哈希和WIC解码后存入受控原件；
管理页20项、桌面当前课/可信课间5项，同名独立追加。点击固定版本先复制再请求关联程序打开。
原件总量2GiB/单件50MiB，图片10MiB/8MP；打开副本独立200MiB，按副本最后写入时间保留24小时宽限，被占用不强删。
全局本地管理员可在资料管理中查看占用，并确认清理最后写入已满24小时且未占用的副本。PDF真实解析在80MiB进程提交内存/CPU10秒/墙钟30秒的隔离子进程运行。
--materials-probe 使用真实双worker/SQLite、合成文件及私有临时ProgID和无界面接收器验证资料链，
不会调用本机日常Office/记事本或改变默认文件关联，不写正式ProgramData。
主管理员可在当前课程中临时换课或恢复原课；提交捕获课程实例和修订，过节/旧版本拒绝。
实际老师区间及覆盖持久化，已记录的课程可在课间重启恢复资料；稳定的课前基础课信任可跨夜补全未观测课程；旧格式或无充分证明时显示中性空态。
桌面顶部换课入口只有按钮接收点击；外部应用前台时隐藏并撤销未提交意图。
--audio-silent-probe 只生成全零PCM，不录音、不调整音量或前台；验证原生自然完成/取消及
受控WAV→精确Web会话→SQLite持久任务→WinMM→释放后正文清理。无兼容设备记NOT_RUN。
声音持久核心有4任务/每教师2、16MiB（含预留）、100条/24h审计和重启中断不重播；
按需声音执行域已绑定托盘紧急停止、当前Windows会话/休眠/退出和默认输出设备变化；
管理登录的后台锁不再阻挡音频停止。尚无对外教师网页提交入口，不能据此称录音产品已交付。
--audio-silent-probe 额外验证正常按需执行域、独立Web会话、停止全部待播和退出顺序。
--http-iocp-probe <port> <FutureClassroom-Test-marker> <1..120> 仅供隔离传输诊断，
在127.0.0.1显式测试前缀运行两轮真实IOCP；需要相应系统权限/URL预留，程序不自行提权或修改配置。
它拒绝凭证与业务请求，不能作为HTTPS、校园可达或浏览器配对通过证据；正常启动默认没有Web监听。
完整profile/布局编辑、产品语音工作流和教师管理 Web 均未交付。
应用图形DIB合计上限16MiB；4K自身离屏场景实测4806080字节，完整低Alpha区域背景保持。
这不代表操作系统背景/DWM总内存或4K真实显示设备验收通过。
原生网格尚未完成真实触控/白板笔、多DPI和全部视觉验收，当前仍为CUI开发目标。
此开发构建不主动修改开机启动，不提供远程管理端或安装程序。

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
