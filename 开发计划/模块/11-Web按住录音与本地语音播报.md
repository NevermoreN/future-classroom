# Web按住录音与教室本地语音播报

> 实施状态（2026-10-30）：用户已授权开始开发。本文保留 D11 设计基线；“仅计划、不开发/不测试/不同步”及“全部待实施”为历史状态，当前实现范围、证据与未完成门槛以[实施进度与验证记录](../10-实施进度与验证记录.md)为准。

总计划1.21 / D11，2026-10-30；本册保留Rust开发规格，当前实现与实测状态见实施进度记录。负责M18，落实按住录音、规范WAV、默认总3遍、队列/停止与Win7原生播放。 任务与验收以[任务清单](../04-开发阶段与任务步骤.md)和[测试清单](../05-测试验收与发布清理.md)为准。

Rust 实施：[Rust开发与验证计划](../07-Rust开发与验证计划.md)。本手册功能、权限、配额、故障和真机验收范围全部保留；Rust 类型安全不替代这些门禁。
## 1. 产品行为与范围

教师打开Web“语音播报”，核对当前白板/目标班级，设置“总播放次数”，按住说话、松开发送；上滑进入取消态，松开丢弃。默认**总共播放3遍（包含首次）**，可选1–10遍，每遍之间静音1秒。发送时锁定次数，不因后来修改下次录音设置而改变已排队任务。不是首次加重播3次。

录音最长60秒、最短0.5秒；达到60秒停止录音并按当前手势状态发送或取消。明确按钮“取消录音”及键盘替代可用；未授权麦克风先提示授权，若授权返回时按压已结束，丢弃本次动作，不能迟到偷偷录音。pointercancel、页面隐藏/离开、来电或设备中断、目标白板/班级切换均取消未发送录音并释放麦克风，不把意外失焦当松手发送。正常有效的pointerup或达到已告知的60秒上限才触发送出；同recordingId只允许一次finalize，随后松手不得重复上传/提交。

网页显示“录音中→上传中→已排队→第2遍/共3遍→播放完成/失败/已取消”，提供停止本人当前播报、取消本人待播。底部固定“管理今日提醒”保留。不是直播、不录教室麦克风、不进行语音识别、不依赖第三方消息平台或云端。白板必须在线且校园网络可达；校外仍需既有校园VPN，关机无可靠待投。

## 2. 权限与现有规则的关系

| 能力 | 全局主管理员 | 教师管理员 | 用户 |
| --- | --- | --- | --- |
| Web录音并向本白板已绑定班级提交语音 | 全部受管班级 | 仅本人有效任教且本白板实际绑定的班级，不要求正好是其授课时刻 | 拒绝 |
| 查询/取消/停止播报 | 全部 | 本人任务，不能读他人音频/停止他人任务 | 拒绝远程操作 |
| 本地托盘“停止全部语音” | 允许 | 允许 | 允许，属于现场紧急停止，不授予其他管理写权 |
| 启停远程播报、时长/队列/次数上限与设备策略 | 本地全局设置 | 拒绝 | 拒绝 |

这是新增加的第4类教师Web业务入口；原3类为本人作业、本人资料、共同提醒。组件/布局/科目外观/换课仍仅全局维护；提醒仍可跨同安装受管班级共同维护，**语音不沿用提醒的宽范围授权**。旧会话按新能力重新校验，不能因音频上传获得远程打开文件/任意路径或系统音量控制权。

远程语音默认随已授权Web入口可用，全局可关闭；无扬声器时明确不可用。默认允许在PPT等应用前台时播放，这是老师主动发起的独立音频动作，桌面展示层仍按原规则隐藏、不抢焦点、不弹字幕窗口、不自动暂停PPT或强制混音/调音量。系统静音或物理扬声器关闭时无法保证听见，保持系统音量，不主动解除静音。锁屏、切换会话、休眠、退出时停止并取消剩余任务，恢复后不突然继续播报。

## 3. 录音格式与Win7技术路线

教师设备沿用TypeScript/React/shadcn；`getUserMedia({audio:...})`只采教师设备麦克风。浏览器需要可信HTTPS与明确麦克风权限，校园IP的普通HTTP不作为可录音部署方案；P0-08验证实际手机/浏览器与校园证书。依据：[MDN getUserMedia](https://developer.mozilla.org/en-US/docs/Web/API/MediaDevices/getUserMedia)。

录音路径为Web Audio/AudioWorklet按实际采样率采集PCM并显式混合为单声道，不假定channelCount请求一定被设备遵守；达到上限即停止。录完用受限OfflineAudioContext重采样到16000Hz，Worker编码为**WAV、PCM16 little-endian、单声道、16000Hz**，上传前再次检查帧数/时长。前端不假定MediaRecorder一定输出WAV；不同浏览器的容器/编码有差异。依据：[OfflineAudioContext构造参数](https://developer.mozilla.org/en-US/docs/Web/API/OfflineAudioContext/OfflineAudioContext)、[MediaRecorder MIME类型](https://developer.mozilla.org/en-US/docs/Web/API/MediaRecorder/mimeType)。格式能力探测失败明确“当前浏览器不支持录音，请使用已验证浏览器”，不向Win7发送未验证WebM/Opus/AAC，也不悄悄引入转码服务器或大套运行时。

60秒PCM数据为1,920,000字节，加固定44字节WAV头为1,920,044字节。首版只接收此规范的最小44字节头WAV；采样帧8000–960000，数据大小、RIFF长度、blockAlign=2、byteRate=32000、位深16和声道1须一致，拒绝额外未知块、奇数长度、截断、压缩格式、伪造时长/扩展名与整数溢出。HTTP上传上限2MiB、解析后音频数据上限严格按60秒检查；浏览器侧原始采样缓存上限按实际采样率计算并限制到32MiB，超过或采样率不受支持时停止取消。

白板采用Rust/WinMM `waveOutOpen`/`waveOutWrite`，通过`WAVE_FORMAT_QUERY`先探测PCM格式与默认输出设备，CALLBACK_EVENT通知受限播放执行域。文件流式读取、小块缓冲（例如3×16KiB），不整文件解码、不常驻浏览器/Node/FFmpeg、不调用外部播放器。WinMM接口依据：[waveOutOpen](https://learn.microsoft.com/en-us/windows/win32/api/mmeapi/nf-mmeapi-waveoutopen)、[waveOutWrite](https://learn.microsoft.com/en-us/windows/win32/api/mmeapi/nf-mmeapi-waveoutwrite)。这是Win7原型候选，x86真机输出/驱动仍须P0-08实测；ARM64实际系统未定，AudioOutputPort保持可替换。

## 4. 模块、存储与API契约

M12拥有录音手势、权限/采样生命周期、格式归一化与UI；M11负责可信Web来源、HTTPS/CSRF/Origin、上传限额和路由；M03校验角色/任教/白板绑定；M06保存受控音频资源/任务/审计；新增M18负责有界队列、重播策略和AudioOutputPort。M17本地文档打开不接音频播报命令，M01窗口可见性不作为主动语音播放的必要条件。

数据：VoiceAsset含assetId、ownerPrincipalId、deviceId/classId、format、byteLength、frameCount、durationMs、hash、受控路径及上传到期；VoiceBroadcastJob含jobId、commandId、assetId、owner、目标、repeatTotal、completedRepeats、currentRepeat、state、revision、submittedAt、latestStartAt、playbackGeneration、错误码。owner和路径来自服务器，不接受自报owner或远程URL。次数/目标在提交后不可修改；再次播同一录音需要用户新动作与新commandId，不后台重放。

| API | 作用与限制 |
| --- | --- |
| GET `/api/v1/voice/capabilities` | 当前已授权白板/班级、录音格式、上限、后端设备可用性及本机功能开关，不泄露他人任务 |
| POST `/api/v1/voice/uploads` | 可信Web上传规范WAV、绑定当前身份/目标；有界临时资源，未提交10分钟清理 |
| POST `/api/v1/voice/broadcasts` | assetId、commandId、目标、repeatTotal；原子消费上传句柄/扣配额/提交任务；同commandId同负载返回原任务，异负载冲突 |
| GET `/api/v1/voice/broadcasts/{jobId}` | 查询自己或全局有权任务的队列/第几遍/终态，上传或排队不声称已听到 |
| POST `/api/v1/voice/broadcasts/{jobId}/cancel` | 带expectedRevision和幂等requestId；本人或全局停止当前条并取消其剩余遍数，或取消该条排队；不顺带取消别人的任务 |

路径为接口设计建议，随现有API前缀统一版本化。M11调用应用用例，不直接waveOut。音频资源不是通用课堂资料，不增加MP4/WebM等到文档打开白名单。

音频位于`%ProgramData%/<产品目录>/voice/{staging,active}`，配置/任务元数据在现有SQLite；子目录沿用安装实例ACL和磁盘配额，不放网页可猜测公开路径。终态且驱动释放句柄后删除音频正文，只有最近24小时且最多100条的脱敏状态审计；不把教师录音纳入常规长期教学资料备份。备份序列化时将非终态语音只保留为INTERRUPTED审计或排除，并去除活动音频引用；不修改源数据库正在播放的任务。恢复不恢复活跃队列/音频，不产生悬空可播放引用，崩溃后清理孤立文件；未提交10分钟、全局音频磁盘16MiB上限，按引用保留活动文件，磁盘满拒绝新任务且不损坏已接纳任务。

## 5. 并发、重播与恢复

单白板单播放流；最多4条非终态任务（含当前播放）、每老师最多2条。先成功提交的先播放，整条任务的3遍播完才轮到下一条，不交叉混音、不用“新消息覆盖旧消息”的单槽模型。满额返回QUEUE_FULL，不显示发送成功。上传阶段配额也计总量，不能用多份未提交文件绕过16MiB上限；沿用全局上传并发2，每教师最多2份未提交VoiceAsset，并对创建上传/提交播报限速。

```text
上传/校验 → 命令COMMITTED与任务QUEUED原子提交
QUEUED → STARTING → PLAYING(第1遍) → GAP(1秒) → PLAYING(第2遍)... → COMPLETED
QUEUED/STARTING/PLAYING/GAP → CANCELED / FAILED / EXPIRED / INTERRUPTED
```

上传结束不等于入队；命令COMMITTED只表示任务已持久化。任务只有在waveOutWrite成功提交缓冲后为PLAYING；完整一遍的全部缓冲自然完成且当前generation未取消，才递增completedRepeats。停止时`waveOutReset`会把待播缓冲也标为完成，必须先推进取消generation并标终态意图，随后reset/回收，**不能把取消回调计作完整播完**。依据：[waveOutReset](https://learn.microsoft.com/en-us/windows/win32/api/mmeapi/nf-mmeapi-waveoutreset)。任务COMPLETED仅表示本机驱动报告各遍正常结束，不保证扬声器开关/人耳听到。

首次开始播放最晚为服务器接纳后5分钟，超时未开始标EXPIRED，避免晚到很久才播放旧通知；已经开始的任务运行上限为实际时长×次数+间隔+5秒容差，不能因排队有效期截断合法的10遍长录音。排队/每遍开始前复核权限/账号/班级绑定/功能开关和当前代次；撤权/关闭功能停止对应活动任务，语音文件不因创建时有权限而永久可播。

任一音频设备错误、设备拔出/切换或会话不可用即FAILED/INTERRUPTED并取消剩余，不自动重开设备后重头播放。应用重启/崩溃恢复时所有非终态任务标INTERRUPTED，不自动播任何旧队列，先读取终态过滤，再回收音频。音频输出与数据库无法原子化，不能声称物理声音严格恰好一次；网络重试同commandId去重，崩溃以不自动重播为恢复原则。Web断网不撤回已接纳任务；重连查询同jobId，禁止自动创建新任务补发。

停止本人任务是序列化的取消动作；UI显示“正在停止”，只有本机停止/回收确认后显示“已停止”。本地托盘紧急“停止全部语音”取消当前及待播，防止停止后下一条马上又响，不需要管理员解锁。系统音量/其他应用音频保持原样。

## 6. 执行域与资源预算

录音与重采样在教师浏览器，白板无录音输入。M18按需创建一个有界Audio执行域，事件驱动供给缓冲，队列空后释放设备/线程，不在UI/HTTP/单写者线程等待播放，也不每遍新建进程/线程。PCM、上传缓冲、队列及日志都纳入Q12现有总预算；活动语音另测1/3/10遍及与PPT/课间大字/上传并发，不能把声音播放归为外部播放器以漏报。状态页仅前台且有活动任务时最多1Hz查询，其余15秒或手动，后台停止轮询，不增加WebSocket常驻链。

所有音频状态更新不触发全屏绘制。没有新桌面语音磁贴，不占8个展示卡名额；本地紧急停止在托盘，远端录音状态只在Web中。

## 7. 具体开发步骤与发布门槛

1. P0-08：实际教师浏览器HTTPS麦克风/手势/采样转WAV原型，以及Win7 x86静音/设备/事件停止原型；整理能力与数据，不留原型录音。P0-07等待此结果记录再冻结。
2. P3-11：M18领域值、规范WAV解析、配额/队列/幂等、任务元数据与恢复、WinMM适配/停止/进度；通过假输出端口验证状态，再用Win7设备验证。所有输出均受权限代次控制。
3. P6-09：类型化音频API、绑定上传句柄、原子提交、限额/幂等查询/本人取消与全局停止、状态审计/临时文件回收。
4. P6-10：四个业务入口、按住/取消/授权迟到保护、最长/最短录音、PCM归一化、1–10总次数默认3、发送与设备状态、底部提醒入口；调用P6-09并覆盖真实移动浏览器。
5. P6-05集成与P5-01须等待全部新增任务；Q24及受影响Q01/Q02/Q09/Q10/Q11/Q12/Q13/Q22通过后才能宣布完整首版。上述实现及完整验收均待执行。

## 8. 错误与验收

MIC_DENIED/INSECURE_CONTEXT/RECORDING_UNSUPPORTED由网页提示，取消后不传；INVALID_WAV/DURATION_OUT_OF_RANGE/REPEAT_OUT_OF_RANGE/QUEUE_FULL/VOICE_QUOTA/AUDIO_UNAVAILABLE/FORBIDDEN/TARGET_MISMATCH分别指向格式、输入、容量、设备、权限或目标；失败不改已有作业/资料/提醒。停止/播放反馈必须带jobId、revision、代次，过期回调不可结束下一条任务。

Q24覆盖0.5/60秒及越界、重复次数1/3/10与0/11/非整数、授权弹窗期间松手、上滑/触摸取消/键盘/后台中断、WAV边界/畸形/资源耗尽、并发4条/每教师2条/配额、默认3遍实际计数与间隔、排队过期、取消reset回调、撤权/锁屏/休眠/设备拔出、PPT前台播放但桌面仍隐藏、断网同ID重试、崩溃不自动重播和升级备份不恢复队列。测试录音使用合成短音/隔离设备并控制音量，结束后清理浏览器麦克风状态、音频文件、任务、账号与原型/构建残留，正式资料不动。


## D11 Rust音频适配补充

Rust WinMM适配仍使用原规范PCM与CALLBACK_EVENT、小块流式缓冲和取消代次；waveOut/WAVEHDR和PCM使用固定地址，内存必须在驱动归还且waveOutUnprepareHeader成功后释放，不能仅靠Rust作用域结束提前Drop。原生音频回调不跨ABI panic，不等待UI或拿数据库长锁；默认总3遍、自然完成才计数、停止与驱动DONE分开。队列纯规则通过不等于实际扬声器/停止/恢复通过。

## Rust实施落点与完成条件

主责：fc-audio / fc-application / fc-windows；实施任务：P3-11/P3-13/P6-09/P6-10。

P3-11先完成纯队列/元数据与AudioOutputPort替身；P3-13再接WinMM。Audio独占waveOut、事件及固定堆分配WAVEHDR/PCM；驱动持有时地址/容量不变，不把Vec扩容或栈对象地址交驱动。缓冲状态ALLOCATED→PREPARED→QUEUED→RETURNED→UNPREPARED→RELEASED；终止代次后reset，收齐归还，waveOutUnprepareHeader成功才释放缓冲，waveOutClose成功后关闭事件。STILLPLAYING/reset/unprepare/close失败或超时不强行Drop，不无限新建缓冲；停止接纳，有限保留至归还或受控进程结束。下一遍实际开始也需健康闸门和新鲜许可，取消DONE不计遍数。Q24/Q25分别验证假驱动状态机、编译期所有权边界与Win7真实设备。

共同规范：[所有权与FFI生命周期](../08-Rust所有权与FFI生命周期.md)；[构建验证与发布门槛](../09-Rust构建验证与发布门槛.md)。本节是实施步骤，程序/目标系统验证均待执行。


### 第八轮实施边界（2026-10-30）

已实现声音领域/端口与唯一SQLite writer的预留、提交/查询/取消、逐遍驱动CAS、脱敏幂等、100条/24小时审计、过期维护、恢复及备份脱活跃引用。4条非终态/每教师2、2个未提交上传预约、16MiB正文和预留共同计费；另限32个包括待清退对象的资产，防止文件长期占用导致元数据无限增长。生产body适配直接写已计费唯一staging，完整真实规范WAV和哈希验证后，以降权同一文件句柄发布并保持至DB提交。每主体上传/提交各每60秒最多10次。

WinMM使用WAVE_MAPPER/规范PCM16单声道16kHz及CALLBACK_EVENT，固定3×16KiB缓冲；实际首Write后才持久PLAYING，自然完成与释放分别提交，取消Reset的DONE不计遍数。原始Web精确会话保存在可信进程映射，持久actor不能替代原lease。首次实际Write前重新核验5分钟期限；source/open跨期限使用ExpireBeforeWrite，不伪造FirstWrite；已开始后续遍数不套首次期限。实际首写后计算有界单调预算，实际自然返回后计满1秒间隔。驱动错误停止接纳，STILLPLAYING/迟释放保留全部device/event/源pin/headers/PCM；不能通过Drop释放驱动仍持有的内存。

实现参考：[waveOutOpen](https://learn.microsoft.com/en-us/windows/win32/api/mmeapi/nf-mmeapi-waveoutopen)、[waveOutWrite](https://learn.microsoft.com/en-us/windows/win32/api/mmeapi/nf-mmeapi-waveoutwrite)、[waveOutReset](https://learn.microsoft.com/en-us/windows/win32/api/mmeapi/nf-mmeapi-waveoutreset)、[waveOutUnprepareHeader](https://learn.microsoft.com/en-us/windows/win32/api/mmeapi/nf-mmeapi-waveoutunprepareheader)、[waveOutClose](https://learn.microsoft.com/en-us/windows/win32/api/mmeapi/nf-mmeapi-waveoutclose)、[DuplicateHandle](https://learn.microsoft.com/en-us/windows/win32/api/handleapi/nf-handleapi-duplicatehandle)。源码核对固定windows 0.62.2绑定；正式支持仍取三系统实测及PE证据，不能据这些API文档直接宣称产品兼容。

当前只有显式全零PCM隔离诊断串起真实持久/文件/输出链；教师网页录音、HTTP超时及实际上传worker预约、正常运行播报域/托盘停止和WTS/电源联动尚未接入。正常应用启动恢复并回收旧正文，每15秒共享worker维护元数据/正文；不将原型探针列为M18全部交付。最终测试状态见实施记录。
