# 教师语音 API（D11 实施契约）

2026-10-30，第十四轮，总计划1.27。落实P6-03/04/08/09/10；复用既有VoiceService、唯一SQLite writer、受控语音文件与WinMM执行域，十二crate/33条依赖边保持。浏览器录音、受控字节工作端口、真实Windows输出、正常网络/TLS分别取证；正常产品监听仍未接入。

## 路由、输入与响应

全部路径相对固定同源`api/v1/`。GET无body；POST使用精确Cookie/Origin/CSRF。JSON最多64KiB，拒绝重复/未知字段与客户端owner/path/actor/generation注入。ID、修订和runtimeGeneration为规范非零u64字符串，不能经过JavaScript Number。语音音频没有Web下载或远程open端点。

| 方法 | 路径 | 行为 |
| --- | --- | --- |
| GET | `voice/capabilities` | 200 VOICE_CAPABILITIES，当前白板实际绑定班级与真实权限/准入快照 |
| POST | `voice/uploads?classId={id}` | 固定唯一query、`audio/wav`及完整Content-Length；typed受控流端口，201 VOICE_UPLOADED |
| POST | `voice/broadcasts` | 原严格`{schemaVersion:1,commandId,assetId,expectedAssetRevision,classId,repeatTotal}`；200 COMMITTED |
| GET | `voice/broadcasts` | 200 VOICE_JOBS，最多4个非终态与最近20个有权终态任务，sequence倒序 |
| GET | `voice/broadcasts/{jobId}` | 200 VOICE_JOB，读取任务当前持久状态 |
| POST | `voice/broadcasts/{jobId}/cancel` | `{schemaVersion:1,commandId,expectedRevision}`；200 CANCEL_REQUESTED，只操作这个任务 |

VOICE_CAPABILITIES：`{schemaVersion:1,status:"VOICE_CAPABILITIES",identityRevision,runtimeGeneration,serverNow,currentClass,enabled,admission,targetAuthorized,deviceAvailability,defaultRepeatTotal:3,minRepeatTotal:1,maxRepeatTotal:10,minDurationMillis:500,maxDurationMillis:60000,sampleRate:16000,channels:1,bitsPerSample:16,maxWavBytes:1920044,uploadTtlSeconds:600}`。

currentClass为null或`{classId,className}`，班级名最多128字符；serverNow为白板本地民用日期时间。admission为OPEN/CLOSED，只表示当前运行代次接纳状态；deviceAvailability当前诚实返回UNVERIFIED，不能从interactive session或is_admitting冒认有扬声器。实际执行域首次打开时仍用WAVE_FORMAT_QUERY检查输出格式，并以真实驱动状态反馈失败。页面显示“将在实际播放时核验”，不把能力返回、上传或排队说成已听到；可信设备能力缓存仍须后续集成。

VOICE_UPLOADED：`{schemaVersion:1,status:"VOICE_UPLOADED",assetId,assetRevision,classId,bytes,durationMillis,expiresAt}`。上传完成不等于已排队；bytes为44+2×帧数，durationMillis为帧数×1000/16000向下取整。临时上传最长600秒，由服务端生成assetId并绑定原精确Web设备、原实际班级和原runtimeGeneration，停止再恢复不得让旧流跨代次重新取得许可。

JOB：`{jobId,commandId,classId,repeatTotal,completedRepeats,currentRepeat,state,revision,failure}`。jobId等于提交commandId；次数1–10、默认总3遍，包含首次。状态为QUEUED/STARTING/PLAYING/GAP/STOPPING/COMPLETED/CANCELED/FAILED/EXPIRED/INTERRUPTED。failure为null或DEVICE/SESSION/REVOKED/DISABLED/TIMEOUT/CLOCK_REGRESSION/STORAGE，不输出内部authorization、音频路径、原actor、SHA或原生job代次。

COMMITTED：`{schemaVersion:1,status:"COMMITTED",commandId,job:JOB}`；VOICE_JOB：`{schemaVersion:1,status:"VOICE_JOB",job:JOB}`；VOICE_JOBS：`{schemaVersion:1,status:"VOICE_JOBS",jobs:[JOB]}`；CANCEL_REQUESTED：`{schemaVersion:1,status:"CANCEL_REQUESTED",commandId,job:JOB}`，其中外层commandId是停止命令，job.commandId仍是最初播报命令。

## 权限、提交与停止

提交只允许白板当前实际绑定班级：教师还须该班有效任教，主管理员也不能把请求任意投向未绑定班级。语音不沿用共同提醒的宽范围。查询/停止沿现有authorize_job：教师本人、全局有权任务；换班、功能关闭或admission关闭不能阻止查看/停止旧任务。列表在服务器按同一规则过滤，不能向教师暴露他人音频/任务。网页没有任意班级选择器。

保留4条非终态/每教师2、全局2个未提交上传、16MiB正文与预留共同计费、32个含待删除物理资产、主体上传/提交分别每60秒10次的既有门槛。上传先SQL预留最大1920044字节，再直接流入唯一已计费受控对象，完整校验固定44字节头PCM16LE/单声道/16000Hz、8000–960000帧与实际SHA后发布。资料与语音共用进程全局两个物理预约；typed proof持有真实pin及预约至实际Drop，不能因过期/撤权/替换WebApplication提前释放。校验不持Backend/KDF长锁。失败/丢弃且确定未尝试publish的proof可清理自己的未发布文件，但SQLReserved预约仍按600秒维护到期清退，不能宣称Drop立即归还SQL配额。

任何已尝试COMMIT后的停止代次、授权、编码或结果交付失败统一503 RESULT_UNKNOWN，不能误报确定403并删除可能已提交文件。提交同commandId同负载读回现有任务当前状态，不重播；异负载409 IDEMPOTENCY_CONFLICT。结果未知只手动GET原jobId（等于原commandId），404不证明从未提交，不能自动生成新命令补发。

CANCEL_REQUESTED表示停止命令持久化，STOPPING仍等待驱动归还；只有终态才显示已取消/已中断/驱动播放完成。reset导致的DONE不计自然播完。完成也不保证扬声器开启或人耳听到。关闭运行域/锁屏/休眠后不自动续播。

错误沿既有401/403、409 REVISION_CONFLICT/IDEMPOTENCY_CONFLICT、429 RATE_LIMITED/CAPACITY、503 SERVICE_UNAVAILABLE/RESULT_UNKNOWN，并增加VOICE_DISABLED、TARGET_CHANGED、INVALID_WAV、QUEUE_FULL、OWNER_LIMIT、EXPIRED。停止未知继续显示未确认，不把普通播放进度误认为已接纳停止。

## 浏览器采集与资源

按住录音、正常松开发送，上滑48px进入取消态，松开丢弃；支持空格/Enter按住、Esc/明确取消按钮及点击开始/结束替代。权限返回时原按压已结束，则立即stop全部迟到track，不偷偷开始。pointercancel、失焦、隐藏、离页、音轨/AudioContext中断和已观察目标/身份/运行代次变化都取消未发送录音。每recordingId只finalize一次，60秒到期按当时手势发送/取消，随后松手不重复提交。

使用实际采样率、显式多声道均值混为mono；AudioWorklet按真实帧数累计、不假定render quantum恒为128。采集缓存最多32MiB、最长60秒；支持率另行检查。OfflineAudioContext重采样16000Hz，Worker输出规范WAV，不使用未验证MediaRecorder编码、不加云转码/FFmpeg/新npm依赖。Offline渲染不能被强制中断，取消后旧render未settle仍占唯一处理owner，不无限创建context；worker可终止，track/图节点优先释放。采集缓存与额外规范化AudioBuffer/输出/编码副本分别列有界预算，32MiB不是整个浏览器heap保证。

录音开始固定班级/identityRevision/runtimeGeneration/次数；录完上传前主动重读能力，变化就丢弃，不重新绑定目标。周期总请求最多1Hz：录音时只GET能力，有活动任务时只GET任务列表；其余15秒或手动，后台停止所有周期查询。用户刷新可同时读取能力与任务。未知提交不自动轮询或POST，仅保留原命令主动查询。录音、上传及未知命令纳入导航/注销/beforeunload保护，所有业务页保留底部共同提醒入口。

Worklet及Worker均为同源平铺assets/*.js，由构建清单逐文件核验；CSP仅增加`worker-src 'self'`，没有blob脚本或外部脚本放行。开发自签HTTPS、拦截API、合成MediaStream与ignoreHTTPSErrors测试都属于synthetic，不能算校园可信证书/硬件麦克风/生产HTTPS宿主通过。实际教师手机权限、麦克风、校园TLS、Win7/Win10与可听PPT共存矩阵保持各自未验收状态。

同步Read端口只接受已到达的受控File/Cursor等有限字节。前后单调超时检查不能物理中断任意阻塞网络Read；真实完成事件到计费stage、物理上传worker/取消完成与120秒期限仍待正常网络调度集成，不能据受控字节测试扩张为网络期限PASS。
