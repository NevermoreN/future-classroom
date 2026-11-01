# Rust所有权、线程、FFI与故障协议

> 实施状态（2026-10-30）：用户已授权开始开发。本文保留 D11 设计基线；“仅计划、不开发/不测试/不同步”及“全部待实施”为历史状态，当前实现范围、证据与未完成门槛以[实施进度与验证记录](10-实施进度与验证记录.md)为准。

总计划1.22 / D11，2026-10-30。此册是所有原生模块共同实施契约，当前实现和未完成验收以实施记录为准。对应P1-08/09/10、P2-10、P3-12/13、P4-12/13、P6-11和Q25；与[模块接口](01-模块划分与接口契约.md)、[调用链](03-调用链与线程模型.md)合读。

## 1. 线程归属与消息

| 执行域 | 独占所有权 | 跨线程内容与约束 |
| --- | --- | --- |
| UI（1） | HWND/窗口userdata、控件、Direct2D/DirectWrite与UI COM、绘图DC/DIB | 接收Arc<ViewSnapshot>/ID/代次；UI对象使用不可Send/Sync标记；不等数据库/网络/关闭join |
| Writer（1） | Connection、事务、命令/权限权威状态与发布器 | 接收owned命令、提交结果与端口描述；Connection不共享给callback；无KDF/解码/文件大复制 |
| Worker（最多2） | 密码派生、文件准备等单任务状态 | 队列8项且4MiB描述预算；准备并发2，容器验证与WIC解码分别最多1；KDF最多2，均计入同一worker额度，不额外扩线程 |
| HTTP（启用时1） | 请求队列/IOCP、in-flight固定分配、HTTP状态/截止 | 不在完成回调里鉴权写库；解析成owned命令，通过票据回传响应 |
| TLS SSPI（候选启用时最多1） | 四连接 Schannel 上下文、单一证书凭据、同步握手/加解密和释放 | 显式1MiB栈；自然Send的唯一拥有Frame跨固定邮箱；原生句柄与上下文不离开该线程，不借共享双worker扩容 |
| Audio（有任务时1） | waveOut句柄/事件/WAVEHDR/PCM固定缓冲 | 只收身份/代次/资源租约及播放意图；查询权威经内存Reader；空队列释放资源/退出 |
| Shell STA（有动作时1） | COM apartment、本次打开动作/句柄 | 仅本地主动受控open；无每文件线程、不等待外部PPT退出 |
| Final coordinator（关闭/独立修复时最多1） | 原共享worker/音频的实际关闭、耐久候选与冷切换；独立修复进程的清除协调 | 先建协调器再转交原owner；同流程最多1，不在UI等待；栈与原有域并存阶段另计 |
| Calendar（下载时1） | libcurl传输/取消状态 | 最多1次候选拉取，与任务描述/字节额度联动；网络结果不得直接写状态 |

上表包括原有恢复/重新初始化协调器，不能在本轮新增SSPI后继续漏计关闭阶段线程；独立修复与正常工作台分属互斥进程流程，Shell先实际退役再移交恢复协调器。保守最大自有线程预算为上述执行域之和（常驻UI+Writer；按需域不得重复创建），标准库/HTTP.sys/驱动系统线程另报，不借此漏计进程资源。Worker上的WIC对象在实际解码线程创建/销毁，初始化相应COM apartment；不能把另一线程的COM对象塞进Send消息。

普通命令邮箱初始128项且合计4MiB（只传小描述，文件/像素不进队列）；观察者快照只保留最新1版，渲染在用旧版最多额外2版，超限合并/取消旧准备。控制面用独立事件/原子状态和预留消息位承载Stop/Fault/AuthorityInvalidated/ForegroundHidden，不能与上传/KDF争最后一个普通槽。禁止UI或FFI callback在满队列上blocking send；返回Busy或合并，控制信号不丢。

所有后台消息含jobId、target/revision、authEpoch、scene/window/transport generation、CancellationToken。Arc仅表示内存存活；接纳/提交/副作用开始仍复核授权，不自动延长lease。同步Win32调用/模态消息泵前结束可变借用；try_borrow_mut失败只延迟可合并操作，销毁/停声/撤权信号必须走独立控制状态，不能静默丢弃。

## 2. HealthGate与提交发布（RR-01）

全局状态：STARTING→HEALTHY→AUTHORITY_UPDATING→HEALTHY；任何状态可转FAULTED或STOPPING。FAULTED为本次进程不可逆状态，只经受控重启恢复。HealthGate和安全代次不依赖可能失败的UI/日志发布；所有敏感读取、下载、打开/播音开始、展示和业务写入都检查闸门。

权限/凭据/绑定变更的顺序：

1. Writer预校验，构造下一权威快照及不可变CommitIntent，分配可能失败的内存、检查版本；不得COMMIT后才临时解析大JSON/分配整个模型。
2. 在短控制临界区进入AUTHORITY_UPDATING、提升待变更epoch并阻止新敏感动作起始；不持此锁跨数据库/系统调用。普通内容变更不无故更新认证epoch。
3. Writer复核lease/CAS并事务提交。已开始的文件/音频按取消代次收到停止；不等驱动/HTTP/UI确认才提交数据库。
4. COMMIT成功后先交换预构造权威快照（统一版本/epoch），再恢复HEALTHY，最后通知非权威观察者。仅通知/日志丢失可合并重发，不把它当权威交换失败。
5. 若事务回滚且可证明数据库与内存都仍是旧权威，恢复旧数据版本下的HEALTHY，但healthEpoch继续单调递增，已失效许可不复活；COMMIT后权威交换失败/panic、或提交结果不明且不能证明一致，则首先FAULTED。
6. FAULTED拒绝所有旧权限敏感读/写/新副作用（HTTP返回不含正文的DEVICE_FAULTED状态），取消已排队/进行中的准备与声音，撤展示输入并隐藏，停止正常HTTP接收；仅本地受控诊断/退出可用。UI/Audio等通过控制通道确认；超时由独立关闭协调器受控终止本进程，不能继续用旧缓存服务。

副作用的线性化点为实际执行域在HEALTHY且epoch有效时登记StartPermit；登记与进入AUTHORITY_UPDATING用同一短控制仲裁，不持锁跨Shell/驱动调用。排队许可不得直接转为执行许可。撤权前已经登记且已不可撤回的系统动作可能完成，已播出的声音/已打开的PPT不能收回；撤权后不登记新起始意图，未进入不可撤回阶段的动作继续检查取消代次。真实延迟和驱动停止时间须记录，不能声称物理零延迟撤回。

Fault后的COMMITTED事实保留在数据库，本地授权诊断或重启后按新权限可查询；不能开放旧Web权限只为查询结果。快照不含panic的自定义析构副作用；Drop错误只做无敏感正文的受限记录，不在展开中再次panic。

## 3. Win32/COM与FFI边界

Win32回调依官方绑定使用extern "system"（尤其x86），必要C库按其声明ABI；不可用C ABI手工强转stdcall回调。WndProc/WinEvent等入口必须在ABI内部处理展开式panic，普通API失败使用Result；panic=unwind为需捕获边界的发布候选，P0与正式Release一致验证。OOM/进程abort、无效指针或外部SEH不承诺被catch_unwind恢复。[Rust异常边界](https://doc.rust-lang.org/std/panic/fn.catch_unwind.html)

HWND userdata记录唯一UI所有者和generation，WM_NCDESTROY解除映射后拒收旧回调；后台不直接持其&mut。有界PostMessage/句柄通知失败必须回收仍归发送方的owned载荷或改用预存票据，不能悬挂裸Box指针。COM初始化成功（含S_FALSE）与CoUninitialize在同一线程配对；UI/渲染、WIC、Shell分别维护线程域。GDI SelectObject替换对象后恢复原对象再释放DIB/DC，半初始化失败走逆序清理。

### 第五十八轮：COM task 内存释放能力

`SHGetKnownFolderPath`、文件选择器 `IShellItem::GetDisplayName` 和音频端点 `IMMDevice::GetId` 返回的 task 内存，统一由 Windows 适配层先取得可失败的释放能力，再调用可能分配内存的 API。只从完整 OS 系统目录对应的已加载 Ole32.dll 按精确名称解析 `CoTaskMemFree`；WOW64 使用 `GetSystemWow64DirectoryW`，原生 x86/x64 使用 `GetSystemDirectoryW`。以有界 `K32GetMappedFileNameW` 从真正模块基址取得 NT 映像名，与 OS 系统目录及该驱动器 `QueryDosDeviceW` 当前第一项映射构成的完整 NT 路径进行 Unicode ordinal 比较；前后映射必须一致，历史映射不能授权。各路径及映射缓冲均限 1024 个 UTF-16 单元，截断、缺失或不符均拒绝，不将 System32 与 SysWOW64 字符串视为等价，不新增文件或映射句柄。ARM64 另行确认。

全进程单个 `OnceLock` 缓存函数指针或失败 HRESULT，一次初始化对该已加载系统模块执行进程寿命 `PIN`。PIN 到进程终止才结束，后续检查失败也不声称卸载或撤销；不另建替代引用，不调用 LoadLibrary 或改变 DLL 搜索策略，不新增线程或手工 Send/Sync。函数类型严格为 `unsafe extern "system" fn(*const c_void)`，保留 x86 stdcall 单指针参数和 void 返回；原 owned 指针在转换成功或失败时均于原位置恰好释放一次，null 按系统契约为空操作。解析失败发生在 task 分配之前，不以 Rust allocator、HeapFree、LocalFree 或 BSTR 释放器替代。测试的配对 `CoTaskMemAlloc` 也从同一模块按名取得，避免测试重新引入静态 task 符号。[微软释放契约](https://learn.microsoft.com/en-us/windows/win32/api/combaseapi/nf-combaseapi-cotaskmemfree)、[模块 PIN](https://learn.microsoft.com/en-us/windows/win32/api/libloaderapi/nf-libloaderapi-getmodulehandleexw)。

首版直接比较 `GetModuleFileNameW` 的 loader 拼写，在真实 WOW64 中因 SysWOW64 与 System32 的返回差异失败；原件保留，不能以编译通过覆盖。NT 映像修正的实际编译、Windows 分配释放及新双 MSVC 导入表证据见实施记录；源码中的链接库提示或 GNU 导入结果不足以证明 MSVC 最终选择了同一 DLL。Win11 上执行 Win7 target 程序不等于 Win7 实机运行，各系统与希沃可见桌面分别验收。[映像查询与 Win7 K32 导出](https://learn.microsoft.com/en-us/windows/win32/api/psapi/nf-psapi-getmappedfilenamew)、[DOS 设备当前及历史映射](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-querydosdevicew)。

## 4. WinMM固定地址与释放（RR-02）

WinMM对象仅由Audio执行域持有。WAVEHDR与其PCM指向拥有型固定堆分配（例如Box控制块/固定长度切片并维护不移动pointee不变量），地址、容量和事件句柄在驱动持有期间不变；禁止对Vec内结构扩容/移位，也不把栈上WAVEHDR地址提交后再返回。Pin必须配合正确的不变量封装，不能因WAVEHDR实现Unpin而误以为类型名已经禁止移动。

每个缓冲状态：ALLOCATED→PREPARED→QUEUED→RETURNED→UNPREPARED→RELEASED。重复播放只在RETURNED且允许重新提交时复用同一固定地址；prepared大小不可被无界扩大。用CALLBACK_EVENT唤醒后检查驱动归还状态，原生写入字段不通过长期安全&mut借用与驱动竞争。

正常/取消顺序：终止播放代次→（取消时）waveOutReset→收齐各个QUEUED缓冲RETURNED→waveOutUnprepareHeader成功→释放缓冲→waveOutClose成功→最后关闭事件。WAVERR_STILLPLAYING或reset/unprepare/close失败不伪装成功，也不释放仍被驱动持有的数据。超时停止接纳下一任务，保留至资源归还或受控结束本进程；保留数受现有in-flight上限限制，不能重试中无限新增缓冲。[微软解除准备要求](https://learn.microsoft.com/en-us/windows/win32/api/mmeapi/nf-mmeapi-waveoutunprepareheader)

自然完成才递增遍数；reset返回的DONE不计完整播放。资源释放进度和业务CANCELED/FAILED分别记录；UI只在停止确认后显示已停止，不能因为收到取消命令就声称已静音。

## 5. HTTP.sys与IOCP（RR-04）

首版选HTTP.sys API v2队列+OVERLAPPED/IOCP模型，1个HTTP执行域管理全部请求。初始最多8个in-flight请求、2个头接收槽、2个上传体流槽；头部16KiB、普通JSON64KiB；上传上限仍按图片/资料/语音各自策略。请求体/响应分块64KiB，不把50MiB原件读入Vec。截止初值为头部5秒/上传120秒，P0按校园实际网络校准，超时通过取消协议而非释放指针强行结束。

每个IoSlot拥有满足HTTP_REQUEST对齐的固定缓冲、OVERLAPPED、完成key/transport generation及相关响应数据；不能假定Vec<u8>的类型对齐满足原生结构。申请/重试ERROR_MORE_DATA时先确认上一次操作完成，再在额度内更换完整槽；headers/body中的内部指针只在该缓冲有效期间使用，解析后转owned受限DTO。提交前登记in-flight，ERROR_IO_PENDING保持所有权，立即成功路径和IOCP完成路径只能统一结算一次，完成通知策略在P0固定。[HTTP接收语义](https://learn.microsoft.com/en-us/windows/win32/api/http/nf-http-httpreceivehttprequest)

关闭状态：RUNNING→QUIESCING→CANCELING→DRAINING→CLOSED。先撤URL group的新接纳并关闭HTTP接纳闸门/配对，调用HttpShutdownRequestQueue取消尚在接收的请求；按活跃RequestId调用HttpCancelHttpRequest取消传输，不对请求队列句柄泛化使用CancelIoEx。继续处理IOCP直至所有接收/体/发送in-flight完成，才释放槽和关闭队列/IOCP。取消调用成功不等于原操作已完成；旧generation完成只结算原槽，不进入新监听周期。[队列停止](https://learn.microsoft.com/en-us/windows/win32/api/http/nf-http-httpshutdownrequestqueue)、[按请求取消](https://learn.microsoft.com/en-us/windows/win32/api/http/nf-http-httpcancelhttprequest)

关闭初始等待预算5秒，由关闭协调器管理，不在UI/Writer join；未排空进入FAULTED并保留原生持有槽直至受控结束本进程，不能早Drop或继续新建监听替代旧句柄。正常禁用Web只停HTTP及其会话/任务，桌面本地功能保持；全局FAULTED才使用全进程规则。

### 第二十六轮：独立 TLS 候选的额外账与释放证明

`tls_frontdoor` 仅由显式诊断命令进入，固定四槽、有限批次 loopback；正常生产接纳仍受 P0 阻挡。SSPI worker 与 IOCP 域独立：四份唯一拥有 Frame 各含64KiB输入和64KiB输出，四份 pinned 原生IO各64KiB，四份HTTP头各16KiB，四份Accept地址各64字节。显式 SSPI 线程栈1MiB，通道和结构元数据另计；四个排队描述只是移动这四个Frame的所有权，不重复分配四套payload。该账必须纳入原Web增量16MiB目标；Rust结构/缓冲的大小不能证明SSP、LSASS、socket、系统缓存或完整进程提交内存上界。

调用中的Frame只由原worker拥有。关闭socket或发Stop不返还SSPI许可；排队工作只在worker真实归还Frame后取消结算。最终关闭必须收齐IOCP账和SSPI结果、检查原生释放结果，并用Windows线程句柄零等待查询确认实际线程终结后join。5秒排空失败保留原有限owner并置进程级故障，禁止新建替代listener/worker；一般Drop误用另由固定单隔离槽保留活worker，不能无限detach。

## 6. 实际与来源课表版本（RR-03）

ResolvedScheduleVersion含actualSemesterId/revision、sourceSemesterId/revision、calendarModeRevision、appliedCalendarRevision、makeupMappingRevision、sourceTableId/sourceRowId与规则版本。相同学期时两处指向同一版本，不建立另一套单学期代码。实际日期必须在当前展示学期；源日期允许属于另一已配置且唯一可解析启用学期，只读其基础周表，不递归日期覆盖。源校历不参与实际日安排。

LessonInstanceKey包括actualDate/classId、上述一致版本、实际begin/end；SceneSnapshot、解析缓存、Override和打开Context都比较该版本。任一实际/源版本或实际映射变化失效旧覆盖/准备/缓存，再同代次重算；subjectId/teacherId变化即使时间/rowId不变也不能复用旧场景。事务读同一仓储快照，期间版本变化则丢结果重试，不能分别读A/B拼成新旧混合。

LastLessonMaterialContext持独立sourceLessonKey、ResolvedScheduleVersion、最终实际老师/科目/目录和生效区间证据；已完成课时的覆盖历史保留至当天不再需要，不因到点EXPIRED就删掉。源表之后改名/换课不能直接把过去老师反推为新老师；可证明的当日历史用于课间保留，不能证明则中性空态，绝不借用下一节或其他老师。跨日/换班/撤权失效，目录本身按最新版本读取/新增顺序倒序，课间仍可主动打开。

## 7. 容器与协议额度（RR-05）

首版容器校验按输入50MiB以内、单条目实际展开32MiB、累计展开128MiB、最多2048条目、XML深度64/总节点250000、单文本事件1MiB/单属性64KiB、条目路径512字符作为初始硬上限。只允许产品白名单的ZIP容器/压缩方法，不递归解开嵌套压缩包；不把128MiB展开上限当成内存缓存额度。

fc-materials按32KiB以内流式块累计实际输出字节/时间，不能相信归档声明长度或先read_to_end再查额度。XML解析前/扫描过程中执行字节事件预算，不等巨型事件已分配后才数节点；禁DTD/外部实体，拒绝路径遍历、重复规范路径、加密/宏及不支持结构。验证过程中不将DOCX压缩条目解到任意文件系统路径。初值CPU10秒/墙上30秒（不含排队），每块检查取消/截止；P0/样例校准可调整数值但须保留有限上限和Q04/Q12证据。

容器验证仅1个Worker活动，流/解析缓冲合计8MiB上限；另一个准备任务可流式复制，但不并行持第二套解压缓存。WIC像素、图片32MiB缓存、打开副本及大卡DIB继续单独计账并受总64/96/160MiB目标约束。超限拒绝本任务并清理临时文件，保留旧已发布内容。

fc-protocol的JSON上限64KiB、深度32，严格Visitor拒绝重复键/未知字段/枚举和整数溢出，不先反序列化为会丢重复键信息的Value。命令指纹规定schema版本和规范字段顺序/类型，文本按已规定编码保持，不通过浮点往返整数；hash与commandId职责分开。响应DTO和备份schema各自版本化，不把教师上传文件塞进JSON。

## 8. 关闭与故障验证

先提升取消/健康代次并停止新接纳→发独立控制信号→在各自线程解除观察者/原生回调入口→HTTP与Audio排空→Writer完成可确定的短事务并flush→各线程确认后关闭数据目录锁。UI从不等待join；系统注销不弹口令阻止关闭。超时/进程崩溃不依赖Drop，启动从持久状态恢复、语音统一标INTERRUPTED不自动重播。

Q25矩阵必须覆盖：COMMIT后权威交换失败但旧token下载/音频起始并发；音频结构移动尝试/STILLPLAYING/迟到归还；HTTP无请求关闭/上传取消/立即完成+完成队列/迟到完成；A/B源学期任一修订变化；压缩比/巨XML/并发超限；队列饱和仍能停声/隐藏。部分“移动/Send”限制用编译期反例或类型边界检查，不能为通过测试加入unsafe绕过。所有程序验收当前NOT_RUN。


### 第二十九轮：HTTP借用正文与TLS退休回执

候选的每槽16KiB头从内联数组改为固定容量Parser arena，仍保留到槽退休；有界响应头Vec另逐项记入实际保留容量，正文sender只持token/长度/一块唯一回执，不存64KiB正文副本。解析头后的同Frame剩余明文不会被Parser吞掉；未来stage桥必须取得原物理上传许可后显式准入，保留Frame到文件worker真实ACK，不能在解析器外用未计费临时文件预收或重复取得许可。本轮诊断拒收任何正文，不声称上传已连通。

正常响应在原密文全部真实IOCP完成后ACK正文、生成并实际发送close_notify，再关闭socket；原IOCP和Frame均已归还才请求同token Retire。worker收到并真正poll原Retired后才可接受新serial；排队Retire遇stop只负向返回，不能标退休。停止仍由实际Schannel close和线程对象终结证明归还；物理超时保留原owner并闭锁，不用另一个线程替代。见[候选ADR](14-P0独立TCP接纳候选ADR.md)，完整生产资源、持续接纳与严格五秒门槛仍未完成。


## 第三十四轮：独立接纳内存与真实完成债务

HTTP候选新增唯一pinned `AcceptMemory`（OVERLAPPED与64字节地址区）和唯一接纳socket，与原四业务槽分离；不是新增四套Frame/解析器/上下文或worker。原非HTTP有限诊断保持原四接纳槽。HTTP候选的应用上限变为五个原生在途操作、六个自有socket descriptor；Ready业务槽无活动期限或socket。接纳socket移交必须先从原owner移出，保留原post+5秒截止，禁止dequeue后续期。

独立接纳到期/停止后仍保留原固定内存及IOCP债务，直到观察到对应实际完成。关闭失败的descriptor不得覆盖，后续偶然关闭成功也不清除故障。任何接纳债务、业务IO/Frame/Retired或实际socket未释放均禁止IOCP/WSACleanup最终释放及替代域；超时保留原owner。新增分配计入应用自有字节，内核/SSPI及完整Web资源账另验。测试只读快照和有限探针的进度输出不改变原所有权规则，执行状态见实施记录第三十四轮。
