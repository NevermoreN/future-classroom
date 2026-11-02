# 教师课堂资料 API（D11 实施契约）

> **当前命令契约（2026-11-02）**：本篇 append/delete POST、COMMITTED/COMMAND_RESULT 外层、`materials/commands/{commandId}` 及末尾“只查询原 commandId”的说明，由[27-命令发行与结果留存](27-命令发行与结果留存.md)的服务器发行 C2/schema2 完整票据对契约覆盖。资料分页/目录读取、上传预留/字节/取消 DTO、下载及文件权限规则保持各自原格式；旧资料和已索引旧回执的兼容不等于允许 schema1 新写入。后文阶段性接线描述保留为历史，当前实现和验证状态以[实施进度](10-实施进度与验证记录.md)为准。

以下早期落点始于2026-10-30第十三轮；当前批量契约见本篇“P3-08教师Web批次”。落实P6-03/04/07/08；十二crate与33条依赖边不变。文件由既有Office、PDF隔离helper、WIC与TXT内容校验器处理，追加/删除与元数据、回执经唯一SQLite writer提交。普通JSON、受控文件工作端口、synthetic浏览器、真实TLS与校园网络分别验收。该轮当时正常监听尚未接线；后续正常HTTPS宿主及浏览器结果按实施记录各次来源验收，不能将本篇契约当作已部署或整项通过。

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
| POST | `materials/folders/{ownerTeacherId}/{subjectId}/append` | `{schemaVersion:2,commandId,commandTicket,uploadId,allowedClassIds}`；200 COMMITTED；真实校验/短COMMIT两段 |
| POST | `materials/folders/{ownerTeacherId}/{subjectId}/delete` | `{schemaVersion:2,commandId,commandTicket,materialId,resourceVersionId,expectedMaterialRevision}`；200 COMMITTED |
| GET | `materials/commands/{commandTicket}/{commandId}` | schema2 COMMAND_RESULT，完整原票据与当前有效原提交者 |
| GET | `materials/commands/{commandId}` | 有界旧格式历史兼容；不能替代C2完整票据查询或授权schema1新写入 |
| GET | `materials/download/{ownerTeacherId}/{subjectId}/{materialId}/{resourceVersionId}` | 固定版本受控attachment，最多64KiB一块 |

MATERIAL_SCOPES字段：`{schemaVersion:1,status:"MATERIAL_SCOPES",identityRevision,offset,total,nextOffset,items}`。条目为`{ownerTeacherId,ownerDisplayName,subjectId,subjectName,classes:[{classId,className}],classTotal,nextClassOffset}`；按ownerTeacherId/subjectId数值排序，classes按classId数值排序。总组合<=4096，单组总班级数classTotal<=128；scope响应只带首20班，nextClassOffset在未全时为20，其余为null。每页最多20组；长名称使JSON超限时只减少整组，nextOffset按实际条数前进。客户端以“已加载/总班级数”明确展示，并由教师点击加载剩余班级，不把首20伪装成全部。客户端遇身份修订变化停止旧分页和旧选择。班级名/教师显示名各128字符、科目名64字符，与领域边界一致。

MATERIAL_CLASSES字段：`{schemaVersion:1,status:"MATERIAL_CLASSES",identityRevision,scope,offset,total,nextOffset,items:[{classId,className}]}`。每页20班、total<=128，offset规范0..128且不超过total；scope、身份修订、总数与原选择保持一致，班级ID严格数值递增且不能跨页重复。变化后停止使用旧选择，不将文件/原命令转移到别的老师。该分页使合法128字符类名也保持每次响应64KiB，而不是扩大预算或截断标签。

MATERIALS字段：`{schemaVersion:1,status:"MATERIALS",scope:{ownerTeacherId,subjectId},folderId,folderRevision,offset,total,items,nextCursor}`。普通管理浏览沿用既有writer创建空目录语义，因此folderId/folderRevision总是非零字符串；空列表仍带真实目录基线。items最多20，按addedSequence倒序；nextCursor为null或`{folderId,folderRevision,offset}`，offset/total为0..2048的安全整数。目录修订变化返回409 STALE_CURSOR，不把旧游标静默重新绑定。

record字段：`{materialId,folderId,displayName,resourceVersionId,kind,bytes,sha256,allowedClassIds,addedAt,addedSequence,status,revision}`。kind为TXT/PDF/PNG/JPEG/BMP/PPTX/DOCX/XLSX；bytes为安全整数、SHA256为64位小写hex，addedAt为白板本地民用日期时间。当前列表仅PUBLISHED；历史删除结果为DELETED。record修订不得超过所在目录修订。P3-08批量实现中，目录修订每次原子操作推进一次，新增序号按每件推进一次；每批最多20件，序号校验上界为目录修订乘20（饱和计算），零状态及唯一性等约束继续保留。该修正的运行验收状态见实施记录。追加创建record修订1，删除保留原资源/文件名/班级/时间/新增序号，只改变状态和条目修订；同名文件是独立条目。

UPLOAD_READY为`{schemaVersion:1,status:"UPLOAD_READY",uploadId,expiresInSeconds:300,maxChunkBytes:65536}`；uploadId为服务端随机非零u64，并等于该上传预留的resourceVersionId。它不是bearer，必须原精确Web设备lease、原owner/subject、当前权限和单调期限同时有效。UPLOADED为`{schemaVersion:1,status:"UPLOADED",uploadId,bytes}`；只表示已收到受控字节，尚未追加。UPLOAD_CANCELLED为`{schemaVersion:1,status:"UPLOAD_CANCELLED",uploadId}`，只清理原设备未提交临时内容，不撤回既有COMMIT；取消无法确认则保留不确定提示、期限清理，不自动重试。

单件当前COMMITTED为`{schemaVersion:2,status:"COMMITTED",commandId,commandTicket,scope,record}`。COMMAND_RESULT为`{schemaVersion:2,status:"COMMAND_RESULT",commandId,commandTicket,state,scope,record}`；NOT_FOUND时scope/record均null，COMMITTED时均非null。原提交者在当前有效权限下可查较早结果，主管理员无跨人查询覆盖；NOT_FOUND不证明排队写入永远未发生。原v1材料回执和raw指纹不改，Web归属侧车与原操作同COMMIT。重复相同命令不追加/删除第二次，payload/actor不符为409 IDEMPOTENCY_CONFLICT；不自动重发POST。

## P3-08教师Web批次（当前实现契约）

教师先选择1–20件文件，在有界清单中排序、移除并明确确认；确认页展示最后一件为本批最新项。清单、老师/科目及班级选择在创建组时固定，成员按确认顺序串行上传。每件保持原格式、哈希和内容校验，全部准备完成后，才显式发行一次Materials命令并以一个完整票据对提交。准备/上传成功均不表示资料已发布，不能用重复单件append代替一次批次事务。

以下路径同样相对`api/v1/`，沿用Cookie、同源Origin、CSRF和当前授权检查：

| 方法 | 路径 | 请求与成功响应 |
| --- | --- | --- |
| POST | `materials/batches/{ownerTeacherId}/{subjectId}` | `{schemaVersion:1,files:[{displayName,bytes}],allowedClassIds}`；201 `{schemaVersion:1,status:"MATERIAL_BATCH_READY",batchId,expiresInSeconds:300,maxChunkBytes:65536,items:[{uploadId,displayName,bytes}]}` |
| POST | `materials/batches/{batchId}/files/{uploadId}/bytes` | 完整Content-Length的`application/octet-stream`；200 `{schemaVersion:1,status:"MATERIAL_BATCH_MEMBER_UPLOADED",batchId,uploadId,bytes}` |
| POST | `materials/batches/{batchId}/cancel` | 仅`{schemaVersion:1}`；实际整组负向清理确认后200 `{schemaVersion:1,status:"MATERIAL_BATCH_CANCELLED",batchId}`，不撤销已保存COMMIT |
| POST | `materials/folders/{ownerTeacherId}/{subjectId}/append-batch` | `{schemaVersion:2,commandId,commandTicket,batchId,uploadIds,allowedClassIds}`；200 `{schemaVersion:2,status:"BATCH_COMMITTED",commandId,commandTicket,scope,records}` |
| GET | `materials/batch-commands/{ownerTeacherId}/{subjectId}/{commandTicket}/{commandId}` | 200 `{schemaVersion:2,status:"COMMAND_BATCH_RESULT",commandId,commandTicket,state,scope,records}`；COMMITTED带原结果，NOT_FOUND的scope/records均null |

新批次写入必须schema2和原服务器发行的`commandId+commandTicket`，没有schema1或缺票据回退。创建/取消的schema1是非命令包络，不授予写入权限。所有字段严格解析，重复/未知字段、非规范u64、无效票据、空/超过20件、重复uploadId及非法班级拒绝；uploadIds顺序、数量和allowedClassIds必须与已确认组完全一致。uploadId等于原成员resourceVersionId，组ID及文件ID均不是bearer授权。单件上传/追加/删除保持独立端点与各自兼容边界。

Writer一次校验全批权限、配额和指纹，并在同一SQLite事务保存全部记录、原结果及一次全局发布；没有中间可见半批。文件夹修订仅推进一次，逐件分配连续addedSequence，当前列表仍倒序。历史records保留确认顺序、同一folder/time/classes，ID和资源版唯一、PUBLISHED且初始revision1；因此历史顺序与当前列表倒序不同。预提交用真实准备元数据及最大合法数字/票据长度同时预算POST和历史包络，任一无法在64KiB编码则在Writer提交前拒绝，不先COMMIT再以RESULT_UNKNOWN掩盖不可编码结果。

历史先核原完整票据、Materials family、原owner/账号epoch、当前有效lease与老师/科目权限，再读不可变原批次回执，不要求临时组仍存在。后续删除或当前目录刷新不会改写原批次结果。已保存结果不因负向物理清理忙而变成UNKNOWN；原组归还失败仍保留其实际占用。NOT_FOUND、取消、超时和当前目录读取均不能证明回滚或确认原命令；结果未知仅手动查询原完整pair，不自动重发或发行新票。过期/未发行按命令窗口返回410 COMMAND_EXPIRED/404 COMMAND_NOT_ISSUED，不能当作历史提交成功。

物理容量沿用两个组owner/permit和两个Hub槽；每组最多20成员、同一时刻一个成员流，组期限是创建时固定的单调300秒，不因每个成员重新续期。故最多40成员，临时原件上界单独为`2×20×50MiB=2000MiB`，图片仍限10MiB。受管原件、Retiring及profile媒体继续独立接受Writer的2GiB库存检查；不声称staging与正式目录合计不超过2GiB，也不引入未实现的SQL staging预留。UNKNOWN、TTL和取消通知不提前归还原文件、pin或permit；无法确认清理的整组转原隔离保留，真实回执与物理归还分别核验。实现接线和编译/组件通过不替代教师浏览器批次流程实测，最新结果见实施进度。

## 文件与授权

教师仅本人有效任教老师/科目目录，主管理员可管理全部有效任教组合；allowedClassIds至少一个、无重复且均是该owner/subject当前有效任教班级。Web无远程Shell/open、任意路径、静态ProgramData根、缩略图或角色/布局写端点。下载仅在教师当前浏览器保存。

文件名沿用领域规则：最多180字符/720 UTF-8字节，拒绝控制符、路径分隔/ADS字符、Windows保留设备名、尾空格/句点及不允许的扩展名。单文件最大50MiB；图片最大10MiB/8MP；原件总量2GiB。实际格式、内容、哈希、Office容器/主动内容、PDF helper与WIC完整解码全部通过后，才建立PreparedMaterialAppend并原子发布不可变文件。上传成功不能绕过校验，客户端后缀/Content-Type不作格式证明。

最多两个真实上传/准备owner；新批次每owner为一个最多20成员组，单件端点仍按原owner管理，包括接收、Ready、CPU校验和持有prepared proof阶段。文件数据不进入JSON或SQLite队列；64KiB分块，不把50MiB原件收进白板Vec。准备阶段不持Backend互斥；通过最新AuthorityReader与精确设备ledger的内存检查撤权，短COMMIT时仍由writer重查。已尝试COMMIT而结果未知的final文件不得由proof Drop删除，留待既有库存恢复/回收核对。

下载响应为`application/octet-stream`、`Content-Disposition: attachment; filename*=UTF-8''...`、no-store、nosniff和X-FC-SHA256。文件以受控不可变句柄固定，首字节前验证完整SHA256，验证和每块前重新检查权限/健康/资源上下文。期间任何全局事务修订变化保守停止原下载；删除同条目或retire不能让旧流继续出块。教师浏览器限制接收长度并在完整成功后生成单个有界Blob保存；失败/截断不产生下载文件，对象URL在定时或离页时释放。

## 失败与尚未接线边界

沿用401/403清镜像、409 REVISION_CONFLICT/IDEMPOTENCY_CONFLICT、503 RESULT_UNKNOWN；新增409 STALE_CURSOR/UPLOAD_EXPIRED/UPLOAD_NOT_READY、400 INVALID_RESOURCE、429 CAPACITY。确定的不存在/不匹配固定资源版返回404，不能伪装为503不确定写入。提交后的成功结果未能明确送达时保留RESULT_UNKNOWN；已读到的真实历史COMMITTED不因独立负向清理忙而改写未知。保存回执不等于当前目录或当前白板可见状态。

网页区分本地File、已上传、校验提交、结果未知和原历史结果。取消/导航/注销不会自动追加；未知C2命令只能手动查询原commandId与commandTicket，不能用新ID重发；当前目录读取不代替原结果。所有业务页保留底部今日提醒入口。

以下保留早期同步适配的边界；正常HTTPS生产接线与新批次当前状态分别见上文及实施记录。同步`Read`工作适配只接受已到达、受控的本地File/Cursor或合成有界reader；read前后检查120秒不能使任意阻塞网络Read物理取消。真实HTTP完成事件到受控staging的有界调度、取消完成归属及生产路由须按正常HTTPS宿主的实际实现与运行另行验收，不能将本工作端口的通过扩张为网络上传120秒硬期限通过。长校验经无Backend锁的prepare，随后短commit；通用slice dispatch拒绝大二进制，不能为方便测试收集50MiB。


## P3-08批量删除（当前实现契约）

教师管理页按当前页固定资料记录选择1–20件，显示确认清单后整批删除。每件目标为`{materialId,resourceVersionId,expectedMaterialRevision}`；空、超限、重复ID/资源版本、非规范数值和未知字段拒绝。不能用页码、可见行号或文件名作为删除目标；不以目录修订作整批CAS，无关新增不使原选择自动失效。当前页全选只选择实际已加载的这一页，换页或重读目录清空未提交选择。

| 方法 | 路径（相对api/v1/） | 请求与成功响应 |
| --- | --- | --- |
| POST | `materials/folders/{ownerTeacherId}/{subjectId}/delete-batch` | `{schemaVersion:2,commandId,commandTicket,targets:[{materialId,resourceVersionId,expectedMaterialRevision}]}`；200 `{schemaVersion:2,status:"BATCH_DELETE_COMMITTED",commandId,commandTicket,scope,records}` |
| GET | `materials/delete-batch-commands/{ownerTeacherId}/{subjectId}/{commandTicket}/{commandId}` | 200 `{schemaVersion:2,status:"COMMAND_BATCH_DELETE_RESULT",commandId,commandTicket,state,scope,records}`；NOT_FOUND时scope/records为null |

沿用同源HTTPS、精确Web设备lease、CSRF、当前角色及老师/科目权限。每件必须仍为PUBLISHED、固定资源版本与预期条目修订一致，并满足该条目的班级权限。唯一Writer先验证完整清单和库存，再在一次事务中删除所有有效索引、标记DELETED、逐条推进条目修订、整批推进目录/全局修订各一次，并写独立原回执。任何一件校验失败均不产生部分删除。结果按确认顺序保留原文件名、资源、班级、加入时间及加入序号，只改变状态与条目修订；允许不同加入时间/序号/班级，不套用批量加入的连续新序号规则。

批量删除有独立receipt种类，与单件、批量加入互斥复用原命令；原完整C2和提交人/epoch绑定。原结果与当前目录分开，后续添加和物理原件回收不改变历史；UNKNOWN/NOT_FOUND只允许明确查询原票据，不自动再次删除。实际POST及历史编码在提交前均受64KiB预算。资源先置Unreferenced，仍占原件库存；沿用持久Retiring→受控原件物理删除→成功或确认不存在后释放配额的回收链，不强关已打开的外部软件副本。
