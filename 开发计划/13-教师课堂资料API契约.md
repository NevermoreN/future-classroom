# 教师课堂资料 API（D11 实施契约）

2026-10-30，第十三轮。落实P6-03/04/07/08；十二crate与33条依赖边不变。文件由既有Office、PDF隔离helper、WIC与TXT内容校验器处理，追加/删除与元数据、回执经唯一SQLite writer提交。普通JSON、受控文件工作端口、synthetic浏览器、真实TLS与校园网络分别验收。正常监听尚未接线，不能据此声称已部署教师网页。

## 路由与响应

路径相对固定同源`api/v1/`。所有标识/修订/新增序号是规范非零u64十进制字符串，不经过JavaScript Number。普通请求/响应JSON上限64KiB，严格拒绝重复、未知字段、非规范ID与客户端actor/路径注入；GET无body，POST使用当前Cookie/同源Origin/精确CSRF。

| 方法 | 路径 | 请求/响应 |
| --- | --- | --- |
| GET | `materials/scopes/{offset}` | MATERIAL_SCOPES |
| GET | `materials/classes/{ownerTeacherId}/{subjectId}/{offset}` | MATERIAL_CLASSES，显式加载后续任教班级 |
| GET | `materials/folders/{ownerTeacherId}/{subjectId}` | 首屏MATERIALS |
| GET | `materials/folders/{ownerTeacherId}/{subjectId}/{folderId}/{folderRevision}/{offset}` | 带目录修订游标MATERIALS |
| POST | `materials/uploads/{ownerTeacherId}/{subjectId}` | `{schemaVersion:1,displayName,bytes}`；201 UPLOAD_READY |
| POST | `materials/uploads/{uploadId}/bytes` | 完整Content-Length的`application/octet-stream`；200 UPLOADED；走独立流式端口 |
| POST | `materials/uploads/{uploadId}/cancel` | 仅`{schemaVersion:1}`；200 UPLOAD_CANCELLED |
| POST | `materials/folders/{ownerTeacherId}/{subjectId}/append` | `{schemaVersion:1,commandId,uploadId,allowedClassIds}`；200 COMMITTED；真实校验/短COMMIT两段 |
| POST | `materials/folders/{ownerTeacherId}/{subjectId}/delete` | `{schemaVersion:1,commandId,materialId,resourceVersionId,expectedMaterialRevision}`；200 COMMITTED |
| GET | `materials/commands/{commandId}` | COMMAND_RESULT，仅当前有效原提交者 |
| GET | `materials/download/{ownerTeacherId}/{subjectId}/{materialId}/{resourceVersionId}` | 固定版本受控attachment，最多64KiB一块 |

MATERIAL_SCOPES字段：`{schemaVersion:1,status:"MATERIAL_SCOPES",identityRevision,offset,total,nextOffset,items}`。条目为`{ownerTeacherId,ownerDisplayName,subjectId,subjectName,classes:[{classId,className}],classTotal,nextClassOffset}`；按ownerTeacherId/subjectId数值排序，classes按classId数值排序。总组合<=4096，单组总班级数classTotal<=128；scope响应只带首20班，nextClassOffset在未全时为20，其余为null。每页最多20组；长名称使JSON超限时只减少整组，nextOffset按实际条数前进。客户端以“已加载/总班级数”明确展示，并由教师点击加载剩余班级，不把首20伪装成全部。客户端遇身份修订变化停止旧分页和旧选择。班级名/教师显示名各128字符、科目名64字符，与领域边界一致。

MATERIAL_CLASSES字段：`{schemaVersion:1,status:"MATERIAL_CLASSES",identityRevision,scope,offset,total,nextOffset,items:[{classId,className}]}`。每页20班、total<=128，offset规范0..128且不超过total；scope、身份修订、总数与原选择保持一致，班级ID严格数值递增且不能跨页重复。变化后停止使用旧选择，不将文件/原命令转移到别的老师。该分页使合法128字符类名也保持每次响应64KiB，而不是扩大预算或截断标签。

MATERIALS字段：`{schemaVersion:1,status:"MATERIALS",scope:{ownerTeacherId,subjectId},folderId,folderRevision,offset,total,items,nextCursor}`。普通管理浏览沿用既有writer创建空目录语义，因此folderId/folderRevision总是非零字符串；空列表仍带真实目录基线。items最多20，按addedSequence倒序；nextCursor为null或`{folderId,folderRevision,offset}`，offset/total为0..2048的安全整数。目录修订变化返回409 STALE_CURSOR，不把旧游标静默重新绑定。

record字段：`{materialId,folderId,displayName,resourceVersionId,kind,bytes,sha256,allowedClassIds,addedAt,addedSequence,status,revision}`。kind为TXT/PDF/PNG/JPEG/BMP/PPTX/DOCX/XLSX；bytes为安全整数、SHA256为64位小写hex，addedAt为白板本地民用日期时间。当前列表仅PUBLISHED；历史删除结果为DELETED。record修订和新增序号不得超过所在目录修订。追加创建record修订1，删除保留原资源/文件名/班级/时间/新增序号，只改变状态和条目修订；同名文件是独立条目。

UPLOAD_READY为`{schemaVersion:1,status:"UPLOAD_READY",uploadId,expiresInSeconds:300,maxChunkBytes:65536}`；uploadId为服务端随机非零u64，并等于该上传预留的resourceVersionId。它不是bearer，必须原精确Web设备lease、原owner/subject、当前权限和单调期限同时有效。UPLOADED为`{schemaVersion:1,status:"UPLOADED",uploadId,bytes}`；只表示已收到受控字节，尚未追加。UPLOAD_CANCELLED为`{schemaVersion:1,status:"UPLOAD_CANCELLED",uploadId}`，只清理原设备未提交临时内容，不撤回既有COMMIT；取消无法确认则保留不确定提示、期限清理，不自动重试。

COMMITTED为`{schemaVersion:1,status:"COMMITTED",commandId,scope,record}`。COMMAND_RESULT为`{schemaVersion:1,status:"COMMAND_RESULT",commandId,state,scope,record}`；NOT_FOUND时scope/record均null，COMMITTED时均非null。原提交者在当前有效权限下可查较早结果，主管理员无跨人查询覆盖；NOT_FOUND不证明排队写入永远未发生。原v1材料回执和raw指纹不改，Web归属侧车与原操作同COMMIT。重复相同命令不追加/删除第二次，payload/actor不符为409 IDEMPOTENCY_CONFLICT；不自动重发POST。

## 文件与授权

教师仅本人有效任教老师/科目目录，主管理员可管理全部有效任教组合；allowedClassIds至少一个、无重复且均是该owner/subject当前有效任教班级。Web无远程Shell/open、任意路径、静态ProgramData根、缩略图或角色/布局写端点。下载仅在教师当前浏览器保存。

文件名沿用领域规则：最多180字符/720 UTF-8字节，拒绝控制符、路径分隔/ADS字符、Windows保留设备名、尾空格/句点及不允许的扩展名。单文件最大50MiB；图片最大10MiB/8MP；原件总量2GiB。实际格式、内容、哈希、Office容器/主动内容、PDF helper与WIC完整解码全部通过后，才建立PreparedMaterialAppend并原子发布不可变文件。上传成功不能绕过校验，客户端后缀/Content-Type不作格式证明。

最多两个真实上传/准备owner，包括接收、Ready、CPU校验和持有prepared proof阶段。文件数据不进入JSON或SQLite队列；64KiB分块，不把50MiB原件收进白板Vec。准备阶段不持Backend互斥；通过最新AuthorityReader与精确设备ledger的内存检查撤权，短COMMIT时仍由writer重查。已尝试COMMIT而结果未知的final文件不得由proof Drop删除，留待既有库存恢复/回收核对。

下载响应为`application/octet-stream`、`Content-Disposition: attachment; filename*=UTF-8''...`、no-store、nosniff和X-FC-SHA256。文件以受控不可变句柄固定，首字节前验证完整SHA256，验证和每块前重新检查权限/健康/资源上下文。期间任何全局事务修订变化保守停止原下载；删除同条目或retire不能让旧流继续出块。教师浏览器限制接收长度并在完整成功后生成单个有界Blob保存；失败/截断不产生下载文件，对象URL在定时或离页时释放。

## 失败与尚未接线边界

沿用401/403清镜像、409 REVISION_CONFLICT/IDEMPOTENCY_CONFLICT、503 RESULT_UNKNOWN；新增409 STALE_CURSOR/UPLOAD_EXPIRED/UPLOAD_NOT_READY、400 INVALID_RESOURCE、429 CAPACITY。确定的不存在/不匹配固定资源版返回404，不能伪装为503不确定写入。COMMIT后的任何结果释放失败返回RESULT_UNKNOWN；保存回执不等于当前目录或当前白板可见状态。

网页区分本地File、已上传、校验提交、结果未知和原历史结果。取消/导航/注销不会自动追加；未知命令只能手动查询原commandId，不能用新ID重发。所有业务页保留底部今日提醒入口。

同步`Read`工作适配只接受已到达、受控的本地File/Cursor或合成有界reader；read前后检查120秒不能使任意阻塞网络Read物理取消。真实HTTP完成事件到受控staging的有界调度、取消完成归属及生产路由仍须在正常HTTPS宿主组合中接入，不能将本工作端口的通过扩张为网络上传120秒硬期限通过。长校验经无Backend锁的prepare，随后短commit；通用slice dispatch拒绝大二进制，不能为方便测试收集50MiB。
