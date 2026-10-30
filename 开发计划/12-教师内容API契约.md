# 教师当天内容 API（D11 实施契约）

2026-10-30，第十二轮。沿用已有配对 Cookie、精确 Web lease、Origin/Host/CSRF、64KiB 请求/响应边界；本契约落实 P6-03/04/06/08，不改变权限和真实 TLS 验收门槛。正常产品监听未启用；接口、真实 SQLite 应用端口、synthetic 浏览器和实际 HTTPS 分别取证。

## 路由

相对于可信构建前缀 `api/v1/`：

| 方法 | 路径 | 用途 |
| --- | --- | --- |
| GET | `content/homework/scopes/{offset}` | 可管理的有效老师/班级/科目，每页20项 |
| GET | `content/reminders/scopes/{offset}` | 本安装学校全部有效受管班级，每页20项，不限教师任教班 |
| GET / POST | `content/homework/{classId}/{subjectId}/{ownerTeacherId}/{date}` | 读取/提交该老师当天作业 |
| GET / POST | `content/reminders/{classId}/{date}` | 读取/提交班级共同提醒 |
| GET | `content/commands/{commandId}` | 原提交者查询持久结果，不产生或重发写入 |

所有 ID 为非零规范 u64 十进制字符串；修订允许 `"0"`，不能转为 JavaScript Number。offset 为规范十进制 0..4096，线上的 offset/total/nextOffset 为安全 JSON 整数。路径不接受编码别名、额外查询、尾斜线、actor/origin 或文件路径。GET 无正文；POST 必须同源 JSON、已认证 Cookie 和精确 `X-FC-CSRF`。

日期为 `YYYY-MM-DD`，合法 Gregorian 年 0001..9999；本地日期时间为 `YYYY-MM-DDTHH:mm:ss`，不带时区，均以白板系统时区为准，不用教师设备时区决定业务日期。无可表示的次日时不得伪造到期时间。

## 范围分页

`{schemaVersion:1,status:"SCOPES",kind:"HOMEWORK"|"REMINDER",today,identityRevision,offset,total,nextOffset,items}`。

作业条目：`{classId,className,subjectId,subjectName,ownerTeacherId,ownerDisplayName}`；提醒条目：`{classId,className}`。只输出这些非秘密字段，不输出账户完整对象、凭据版本或角色编辑能力。

作业按 classId/subjectId/ownerTeacherId 数值排序；提醒按 classId 排序。作业 total<=4096，提醒 total<=128；offset<=total，offset==total 可空页。items<=20；`nextOffset` 仅当 `offset+items.length<total` 时等于该和，否则为 null。身份修订变化后不能拼接旧分页或将旧选择绑定到新老师；页面要求明确重新核对，未提交草稿不自动保存。

主管理员可管理全部有效任教关系的内容；教师仅本人有效任教范围，与现有原生工作台的 `is_teacher` 目标检查一致。提醒是本安装学校共同当天槽，教师可维护其他有效受管班级，不能按本人任教范围错误收窄。最终权限在 writer 读取的真实身份及 COMMIT 前再次验证；UI 列表不构成授权。

## 正文与读取

CONTENT：`{schemaVersion:1,status:"CONTENT",today,serverNow,expiresAt,key,revision,text,lastActorId,updatedAt}`。

作业 key 为 `{kind:"HOMEWORK",classId,subjectId,ownerTeacherId,date}`；提醒 key 为 `{kind:"REMINDER",classId,date}`。today、key.date 和 serverNow 的日期必须一致；expiresAt 是该日次日零点。不存在的槽为 revision `"0"`、text 空、lastActorId/updatedAt 均 null；清空过的槽保留非零修订和最后修改人/时间，不能将其读成 revision0。非零修订有非 null 修改人及同日更新时间；旧日正文不返回。

正文为 UTF-8 纯文本，最多16384字节，不允许 C0 控制符（保留 Tab/换行/回车）及 U+007F..U+009F。沿用已有领域规范：持久记录去掉首尾 Unicode White_Space，纯空白即清空；正文中间的合法 Unicode、换行/回车/Tab 保留。原始正文仍参与命令指纹，不能先归一化再重放相同 ID；客户端以服务器确认的记录为准。若客户端校验规范化结果，应匹配 Unicode White_Space，不能使用会额外去除 U+FEFF 的 JavaScript trim。客户端按文本呈现，不使用 innerHTML。该约束使 JSON 文本转义最大两倍，64KiB 正文与响应封装预算可同时满足。存量非法/超限记录读取失败，不截断或静默改写。

合法 Web 日期敏感读取也要持久观察新日期：不能在浏览器已经读过新日、桌面隐藏且没有写入时，因系统时钟回拨复活昨日正文。沿用唯一 writer 与持久 day floor，普通同日读不产生无谓写入；每次读取、提交、查询及结果释放都重查日期与当前授权。

## 提交与查询

POST 仅接受 `{schemaVersion:1,commandId,expectedRevision,text}`。commandId 为浏览器 CSPRNG 生成的非零 u64 字符串；原 key/date/修订/正文/命令 ID 作为同一不可变提交保留在内存，不写浏览器持久存储。拒绝重复/未知字段及权限注入。

成功为 `{schemaVersion:1,status:"COMMITTED",commandId,content:CONTENT}`。COMMITTED 仅表示已持久化，不能声称白板当前可见；同日并发写入可能已产生更新修订。相同原命令与 actor/payload 的重放不二次写，不同 payload 重用命令 ID 拒绝。不得自动重发 POST 或无条件覆盖新修订。

查询为 `{schemaVersion:1,status:"COMMAND_RESULT",commandId,state:"COMMITTED"|"NOT_FOUND",content:CONTENT|null}`。仅原提交账户在当前有效 Web 权限/任教/日期下可查；主管理员也不能用此端口查看他人的 receipt。foreign 与不存在统一 NOT_FOUND，不泄露存在性。NOT_FOUND 只说明当前未找到，排队中的写入可能尚未完成，不能据此自动重发。

查询 COMMITTED 是同日较早原结果，不表示当前槽仍是该版本；页面单独展示并提供显式 GET 核对最新内容，不自动塞回当前草稿。昨日 receipt 返回 DAY_CHANGED，不再返回正文。查询不会刷新会话或赋予新权限。

## 错误与草稿

沿用 `{schemaVersion:1,code}`。明确的修订冲突、日期变化、命令 ID 不同内容重用分别为409 `REVISION_CONFLICT` / `DAY_CHANGED` / `IDEMPOTENCY_CONFLICT`；无权限为403，失效认证为401，不返回未授权目标当前修订。

写入的非确定失败为503 `RESULT_UNKNOWN`。尤其 COMMIT 成功后日期/权限改变而无法释放结果，或序列化失败，不能伪装成写前明确拒绝。页面保留原命令的内存元数据并只按用户动作查询；冲突保留草稿，显式重读新修订后才可重新编辑提交。401/403 停止全部自动操作并清除账号镜像和敏感编辑状态。跨午夜旧草稿不能自动发送到新日；切换目标/离页/注销提供未保存提示，不暗中提交。

任意已认证业务页底部保留“管理今日提醒”。资料及录音接口未接入时明确未就绪，不能以示例数据、虚假上传或成功提示替代真实业务。
