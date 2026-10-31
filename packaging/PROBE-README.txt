FutureClassroom 0.1.0 - Windows x86 native workbench development build
状态：NOT_VALIDATED。此包包含可运行原生管理工作台，完整未来教室应用尚未交付。

直接运行 fc-bootstrap.exe 打开工作台；--autostart 保持锁定并隐藏窗口。
首次运行在本机 ProgramData 的 FutureClassroom 专属目录准备保护ACL、部署标记、
数据库和当前Windows用户的DPAPI密钥。请自行设置应用主管理员口令；无默认密码。
支持学校/班级、基础科目、账号创建/编辑/停用、多班级科目任教范围、首次改密、恢复码、今日作业，
以及原生课表网格、固定数字时间输入、单双表深复制/保留历史、校历草稿。
课表页面先编辑草稿，“保存全部”才写数据库；取消需确认，锁定后同账号可继续草稿。
可交换单双周角色而保留原表内容；共享活动可按适用星期拆分，并将同时间、分组和名称且星期互斥的活动合并。
启用作息同步时两表原子拆合；更改行类型会先列出两表受影响课程、老师、教室及备注，再确认到草稿。取消保持原数据，最后仍需“保存全部”。
桌面显示设置支持三种时钟、秒数、日期倒计时、科目底色及逐组件显示/透明度/尺寸约束；
同样先编辑草稿，保存后生效。显示设置入口可管理公共布局与精确班级/科目/教师配置；公共布局与显示样式联合CAS保存，科目配置支持独立资料样式、1至5行和间距。
布局草稿提供原生图形预览、节点选择和拖动；内置组件使用示例，独立节点读取真实已发布内容，预览不是当前课堂截图。存在本账号未保存课表/显示/账号/布局/内容草稿时，正常退出会提示处理。
保存布局后可管理独立文字/TXT或最多50页有序PNG/JPEG/BMP，支持确认发布、清空、明确目标版本撤销和图片排序；教师网页没有通用内容或布局写权限。
常驻桌面已接入一致事实快照及原生卡片，在系统桌面前台显示课表、作业、时钟等；
外部应用、管理窗口和无效会话时隐藏，分页仅小按钮接收点击，其余区域穿透。
本阶段整份课表（含历史）上限64KiB，超限拒绝，不截断。学校/公共校历分别保存；
公共校历候选与明确采用已接入；来源留痕不等于自动核实政府通知正文，真实学校比对仍需验收。
关闭工作台锁定本地会话；普通正常退出须由主管理员授权。缺失数据不会重新开放首次注册。
无恢复码且决定放弃本机全部资料时，登录/恢复/修复页可进入独立清除向导；
原程序先实际关闭，也可在原程序关闭后运行 --repair。向导须分别确认全部资料丢失和全新设置，
确认前可取消；取消会关闭本次向导，正常重启仍须原账号认证。
确认清除包括学校、全部账号/凭据/恢复码、资料、图片、语音及部署目录内的导出备份。
已另存到部署目录外的副本、Windows自动启动/网络/SSH及其他应用不被修改。
完成后须重新启动正常程序，设置新学校和新的主管理员；不提供解锁或读取旧资料的入口。
完整备份页支持显式映射恢复：来源全部班级、科目和账号须逐项选择当前目标，同类目标不可重复，
相同编号或名称不会自动对应。缺少目标时先取消，通过现有学校/账号管理创建并发放新口令及任教权限。
映射保留目标学校、安装标识、完整目录名称/颜色、账号登录名/标签、当前密码、恢复码、角色与授权。
确认会完整替换教学集合，未映射目标目录保留但其原教学数据也被替换；不是部分合并。
全部映射预检通过后，还须分别确认教学替换和目标安全资料边界，等待实际关闭及冷切换，重新启动登录。

在 PowerShell 中解压后运行：
  .\fc-bootstrap.exe --version
  .\fc-bootstrap.exe --self-check
  .\fc-bootstrap.exe --system-probe
  .\fc-bootstrap.exe --host-probe 2
  .\fc-bootstrap.exe --application-probe
  .\fc-bootstrap.exe --reinitialization-probe
  .\fc-bootstrap.exe --migration-probe
  .\fc-bootstrap.exe --materials-probe
  .\fc-bootstrap.exe --audio-silent-probe
  .\fc-bootstrap.exe --web-pairing-probe
  .\fc-bootstrap.exe --native-probe 30

--self-check 在隔离临时目录中验证 SQLite 提交、权威快照发布、命令幂等、
Windows 随机源、PBKDF2 与目录互斥；还验证DPAPI密钥与SQLite重开、
持久登录限速接口、精确会话、原子改密与恢复码一次轮换；退出时清理隔离数据。
--system-probe 只读发现系统适配，不创建正式数据目录或修改自启动。
--host-probe <seconds> 接受1至30秒，验证自身隐藏窗口、托盘和会话通知生命周期。
--application-probe 使用隐藏原生控件及隔离临时数据执行真实账号/科目/教师/作业、课表/双表/校历/锁定草稿闭环，
并在同一固定双worker池校验合成Office文件，不写正式ProgramData或自启动。
它还验证显示设置保存重读、锁定草稿、临时换课/恢复/CAS拒绝、退出保护及真实事实到原生离屏场景；
新增39步公共布局联合CAS、精确科目配置保存/停用及原生预览选择/拖动流程；不截图用户桌面。
另有52步独立文字/分页图片和65步作息角色交换/活动拆合/确认取消/锁定恢复/真实CAS读回，均使用自有隐藏窗口及临时数据。
--reinitialization-probe 只使用自有临时合成部署，在独立当前程序子进程中验证取消、双确认、
完整清除与正常重启的全新设置；不操作当前用户的正式ProgramData部署。
--migration-probe 使用两个独立受保护合成部署、隐藏原生控件及隔离选择能力，验证逐项映射、双确认、
真实资料和两版图片、实际关闭/三树切换及独立程序重新登录；不打开可见系统选择器或修改正式部署。
--native-probe <seconds> 接受 1 至 300 秒，验证原生透明窗口、3% 玻璃底色、
时钟文字、点击穿透与前台避让。需在已登录的交互桌面观察实际效果。
SSH/无交互桌面的输出不能代替前台、遮挡、缩放、点击和视觉验收。
rendered_frames=0 代表没有确认桌面前台绘制，不能视作视觉通过。

资料管理可选择本机PNG/JPEG/BMP/TXT/PDF/PPTX/DOCX/XLSX，实际校验、哈希和WIC解码后存入受控原件；
管理页20项、桌面当前课/可信课间最多5项（精确配置可选1至5行），同名独立追加。点击固定版本先复制再请求关联程序打开。
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
管理登录的后台锁不再阻挡音频停止。第十四轮已接语音Web应用端口和录音页面；正常TLS/网络调度尚未接入，不能据此称录音产品已交付。
--audio-silent-probe 额外验证正常按需执行域、独立Web会话、停止全部待播和退出顺序。
--web-pairing-probe 在隔离临时部署中使用真实Windows CNG随机源、DPAPI及SQLite，
直接调用配对service验证8位一次请求、本地主管理员明确批准、256位轮询秘密及独立设备租约，
再通过合成的可信TLS请求metadata验证session/CSRF/logout端口；不代表POST配对dispatch全链。
运行不需要交互桌面，退出清理自身数据。
此探针没有建立真实HTTPS/TLS连接、启动正式监听或浏览器，不是这些交付或校园可达的验收证据。
--http-iocp-probe <port> <FutureClassroom-Test-marker> <1..120> 仅供隔离传输诊断，
在127.0.0.1显式测试前缀运行两轮真实IOCP；需要相应系统权限/URL预留，程序不自行提权或修改配置。
它拒绝凭证与业务请求，不能作为HTTPS、校园可达或浏览器配对通过证据；正常启动默认没有Web监听。
--web-pairing-probe还验证同一隔离部署的作业/提醒POST、清空后再写、命令幂等/查询与真实桌面事实；仅请求端口metadata，未开启网络监听。
教师网页已接配对/会话、当天作业与共同提醒的真实应用端口，包含范围分页、读取/保存/清空、修订冲突和原命令查询；第十三轮补齐资料网页与受控文件工作端口，第十四轮补齐语音录音、规范WAV上传、持久任务/停止端口与未知结果查询。
内容日期观察与写入走唯一SQLite writer，清空保留CAS；跨午夜草稿不能自动改日期发送。COMMITTED只表示已持久化，不表示当前可见；未知写入只能手动查询，不自动重发。
正常启动仍没有正式TLS/Web监听；静态文件入包不代表网页已由应用托管或通过真实浏览器验收。
完整图形参数和滑块仍待实现；真实TLS到白板语音工作流及三系统产品验收仍未交付。
独立文字最多10,000个Unicode标量/40,000 UTF-8字节；TXT支持严格UTF-8或带BOM的UTF-16。图片每页10MiB/800万像素，与资料共用2GiB原件额度；当前和一个撤销版本保留引用，物理删除成功后才退额度。
文字缓存4MiB、图片缓存32MiB，实际引用仍存活时不会因取消或清空查找表提前返还。重启先核对受保护命名空间、注册表及原件摘要，损坏数据进入修复；不按超时擅自删除仍被worker占用的原件。
布局和原生排版使用同一完整内容块描述，按实际文字宽度逐块测量作业标签/正文、资料行距和至少44 DIP操作行；
宽高足够时完整显示，最大高度受限时保持全文分页，不再使用16KiB截断文字样本估高。
组合器应用RGBA准入上限16MiB，计入DIB缓存、同时存活的CPU合成暂存与保守软件目标预留；组透明度在子树合成后施加一次。
预览先释放隐藏常驻场景的图形缓存，输出最多960×540/2MiB，不创建全屏中间图。
这些应用侧限制不代表操作系统背景/DWM总内存或4K真实显示设备验收通过。
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

未显式提供打包参数 --web-dist 时，包内仍仅含 fc-bootstrap.exe、LICENSE、THIRD-PARTY-NOTICES.txt、此说明与 manifest.json，
manifest.json 的 frontend.included=false 表示未附浏览器产物。
提供 --web-dist <已构建目录> 时，额外加入 web/fc-assets.json 逐项声明的静态文件及 web/THIRD-PARTY-NOTICES.txt。
打包只读取清单，不扫描其他文件；严格检查版本、大小、SHA-256、普通文件及路径，拒绝符号链接和Windows大小写别名。
清单不超过64KiB，声明资源最多64个、合计5MiB；清单和许可文本另有独立读取上限。
frontend 字段记录本包实际是否附带静态产物及其范围；有无网页文件均保持 NOT_VALIDATED。
manifest.json 记录目标、工具链声明、文件 SHA256、探针范围和 PE 风险。
LICENSE 为项目许可；第三方组件、Rust/MinGW/GCC 运行时许可与版权见 THIRD-PARTY-NOTICES.txt。

Round13 includes teacher material JSON and controlled file worker ports (TXT/Office/WIC/PDF helper, <=50MiB) plus browser management UI. WEB_MATERIALS_PASS is an owned DPAPI/SQLite/files diagnostic with synthetic request metadata, NOT actual TLS or network upload cancellation. Normal listener remains disabled. An isolated finite Winsock accept candidate has lifecycle evidence and a strict five-second observation FAIL; it is not a production TLS replacement.

--web-voice-probe 使用自有合成账号/真实SQLite/规范零PCM上传和原生音频域，验证总3遍自然完成、原任务查询和停止/驱动归还；应由交互会话的隐藏测试任务运行。它仍只使用synthetic RequestHead，不开启TLS/网络listener，不请求硬件麦克风或改变音量/前台；普通--web-pairing-probe不启动原生声音。

Round15 adds local global-admin account paging/edit/disable/multiple teaching scopes, review/cancel/CAS and self-change re-login. Existing timetable/default/history reference conflicts reject atomically. Identity and receipt each retain the 64KiB bound; over-limit edits return Capacity without truncation or authority changes. The sole registered lock action remains available while busy and carries no form secrets; all other busy actions remain disabled.

公共校历：在课表校历页打开候选下载/来源差异/明确采用。候选缓存与已采用版本分开，人工映射优先，未映射补班待确认。自动纯放假变化默认关闭，须主管理员明确授权，学校模式与人工安排不被覆盖。--calendar-probe 使用独立合成资料及隐藏原生控件，--calendar-download-probe 仅固定2026来源实际HTTPS下载一次且不写应用数据；同步WinHTTP硬抢占未证明，不能替代教师入站TLS/P0与三系统实机验收。

登录自启动：主管理员在本机工作台核对期望、实际与当前程序路径后，明确确认开启/关闭/修复。SQLite意图和系统操作分阶段持久记录；启动时仅核对实际状态，不重放、不反复修复。--autostart-probe 只使用独立合成数据库、隐藏窗口和自动生成的临时Registry项，结束后清理；真实HKCU Run不变。259个UTF16命令单元加终止符为保守260上限。真实注销/登录、Win7/Win10/MSVC、安装升级与可见希沃设备验收仍未完成。
