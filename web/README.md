# 未来教室教师工作台（当天作业与提醒阶段）

本目录是教师浏览器运行的静态应用，使用 TypeScript、React、Vite、Tailwind CSS 与本地 shadcn/ui 组件。白板提供同源静态文件和 Rust API；白板不安装 Node，不使用独立云服务。本目录实现配对、等待本地批准、读取真实会话、明确注销，以及真实 API 的当天作业/共同提醒范围分页、读取、保存、清空、修订冲突和原命令查询。第十三轮新增资料目录/班级分页、File流式上传、明确校验后添加、固定版本删除、下载到当前设备、原命令结果查询与临时上传取消；第十四轮新增当前白板班级能力、按住录音/上滑取消/键盘替代、规范WAV上传、任务状态与停止。没有模拟上传、虚构教师数据或假成功。

当前 Rust 的 request-port 配对与[教师内容契约](../开发计划/12-教师内容API契约.md)已经存在；本目录的静态构建通过**不代表正常产品监听、真实 TLS、可信证书部署或办公室到白板浏览器配对已经验收**。生产静态宿主由仓库根目录的实现与验证记录管理。

## 构建和本地查看

开发工具仅运行于现代开发环境，Node `22.12+`（当前验证 `22.20.0`），npm `10.9.3`。直接依赖版本和完整 npm 锁已固定；安装使用 `npm ci`。

```sh
cd web
npm ci
npm run typecheck
npm test
npm run build
npm run preview -- --port 4173 --strictPort
```

查看 `http://127.0.0.1:4173/fc/`。此 HTTP 预览供布局检查，应用不会通过 HTTP 发送账号、配对码、会话或轮询 secret，因此会提示使用可信 HTTPS。开发热更新可使用 `npm run dev -- --port 4173 --strictPort`；Vite 开发服务器不是正式白板接收端。

生产默认部署前缀 `/fc/`，通过构建时可信配置更换：

```sh
FC_WEB_BASE=/school/ npm run build
```

前缀只接受绝对、规范、ASCII 的目录路径。API 固定解析为同源 `${BASE_URL}api/v1/`，不读取查询参数、用户输入或 API 响应来选择服务 origin。入口不使用 `<base>`；不需要外部 CDN、网络字体、Service Worker 或浏览器存储。教师端面向受维护的现代浏览器，不能将此视为 Win7 旧浏览器支持证据。

## 实际请求与安全边界

- 首先 GET `status` 核对 schema/API/UI 版本，再 GET `session` 确定实际账号。没有服务或未通过版本校验时不能申请。
- POST `pairing/requests` 只提交 `schemaVersion/code/loginName/deviceName`。每次明确申请仅发送一次；超时和未知结果不自动重发。
- POST `pairing/requests/{canonicalDecimalId}/poll` 的 original `pollSecret` 只在请求体中传递。服务返回 PENDING 才继续，间隔至少 500ms，总等待不超过 120 秒。secret 不进入 UI 状态、URL、日志、localStorage 或 sessionStorage；离页/卸载/停止等待使用 AbortController 取消。
- AUTHORIZED 不直接构造登录状态。HttpOnly Cookie 由浏览器处理，随后必须 GET `session` 取得白板确认的姓名、角色、当前班级和实际任教范围。
- Cookie 使用 fetch `credentials: same-origin`，禁止重定向、跨源 API 和非 HTTPS 凭据传输。浏览器设置 Origin；客户端不伪造该请求头。私有 POST `session/logout` 必须附服务端会话的 `X-FC-CSRF`。
- 所有响应渐进读取并核验真实 UTF-8 总字节 `<=65536`；严格拒绝非 JSON、错误 UTF-8、重复/转义重复字段、未知或缺失字段、类型/版本错误、非规范/溢出的 64 位十进制字符串。ID 不转换成 JavaScript Number。
- 401/403 清除本页会话信息并停止自动操作。注销的 503 `RESULT_UNKNOWN`、网络失败或无效响应显示“注销结果尚未确认”，清除本页账号镜像，只有用户明确点击才 GET 核对；不假称注销成功，也不自动重发注销。
- 登录后可进入当天作业与共同提醒；班级、科目和所属老师的名称只取自已验证的 scope 响应，绝不从页面输入构造 authority。作业按实际任教老师/班级/科目；共同提醒显示学校全部可管理班级，不按本人任教范围收窄。每页最多20项，使用服务器 continuation，身份修订/总数/日期变化阻断旧分页；资料另按owner/subject目录与显式任教班级分页；语音只允许当前实际绑定且有权班级，状态/停止不随新录音目标变化。
- 正文与日期由白板返回。日期为 Gregorian 0001..9999 的 `YYYY-MM-DD`，本地时间不带时区；客户端不以教师设备日期决定业务日期。正文最多16384 UTF-8字节，支持正常换行/Tab/CR，拒绝不可显示的C0/DEL/C1与孤立代理项。白板所报当天有效时长结束后只将草稿标为过期，不自动发送到新日；同日调时回拨可能使 updatedAt 晚于 serverNow，按原值显示，不伪造时间顺序。
- POST `content/homework/{classId}/{subjectId}/{ownerTeacherId}/{date}` 或 `content/reminders/{classId}/{date}` 只发 `schemaVersion/commandId/expectedRevision/text`，附精确会话CSRF。commandId来自 `crypto.getRandomValues` 的完整非零64位数，不用Math.random。原始正文保留给同一命令指纹，不先trim；服务按Unicode White_Space去首尾，FEFF保留。成功响应只表示持久化，不能称为当前已上屏。
- 409修订/幂等冲突保留草稿，只有用户明确载入最新修订、对照并确认合并后才允许新命令；清空同样检查CAS并保留非零修订。结果不确定时保留原key/date/revision/text/id，仅手动GET `content/commands/{id}` 查询。NOT_FOUND不证明从未写入，不能重发；COMMITTED原结果单独标为历史提交结果，需另GET当前内容核对，不自动塞回当前编辑器。跨日查询的DAY_CHANGED不返回旧正文，也不把旧草稿改日期。
- 切换目标/业务页、刷新会话、离页或注销遇到草稿/未知命令时必须明确丢弃才能继续；取消不提交。401/403立即清除内容、命令元数据与配对控制器的账号镜像，停止所有操作。草稿与未知结果只在内存，不写浏览器存储。每个已认证页面底部保留“管理今日提醒”入口并预留手机安全边距。
- 没有远程账号、角色、课表、布局或文件启动接口。

## 产物与预算

`npm run build` 在 `dist/` 生成 `index.html`、带指纹的 `assets/*.js/css` 和 `fc-assets.json`。清单包含精确 schema/API/UI 版本、文件相对路径、字节数与 SHA-256，不包含自己；没有任意文件映射或 API/SPA fallback。生产脚本和样式均为同源外部文件，无内联 script/style。

`scripts/budget.mjs` 硬检查：最多 64 个声明文件、静态总资源 `<=5MiB`、所有首包 JS+CSS 合计 gzip `<=512KiB`。准确数值见每次构建产生的 `build-budget.json`（构建证据，不放进静态部署目录）。测试使用 synthetic 请求/响应与 fake clock；测试绿灯不是 TLS、真实监听、Windows 或学校浏览器验收。

## 组件来源与许可证

按官方文档手动配置静态 Vite/React 项目：[React](https://react.dev/learn/build-a-react-app-from-scratch)、[Vite](https://vite.dev/guide/)、[Tailwind Vite](https://tailwindcss.com/docs/installation/using-vite)、[shadcn Vite](https://ui.shadcn.com/docs/installation/vite)。本地 Button/Input/Card 来自 shadcn 官方 `new-york` registry，保留 [MIT 许可](SHADCN-LICENSE.md)。不依赖托管 Sites 或 Cloudflare 服务。

`scripts/licenses.mjs` 按 `package-lock.json` 生产依赖闭包从已安装包读取完整 license/notice，生成 `THIRD-PARTY-NOTICES.txt`，并在生产 JS 的 post-minification banner 中保留。涵盖 React、Radix、lucide、CVA、clsx、tailwind-merge 等；开发工具不打包到浏览器。

资料协议见[材料API契约](../开发计划/13-教师课堂资料API契约.md)。普通JSON仍64KiB；单文件50MiB/图片10MiB，上传发送浏览器File本身，不读取arrayBuffer/base64；120秒绝对客户端预算不随进度延长，服务器受控字节适配不能替代真实网络取消完成。下载逐块限长并核对固定版元数据，完整成功才生成有界Blob到本浏览器，离页释放对象URL；服务器不提供远程open/Shell。已上传不等于已追加，结果未知只手动GET原命令，不自动重POST；取消仅针对原临时上传，不能撤回COMMIT。正常TLS/listener和HTTP事件调度仍未接线，synthetic浏览器不替代真实部署。

语音录音需要可信HTTPS和教师设备麦克风权限，原始PCM只在本页内存保留。AudioWorklet显式mono采集，OfflineAudioContext重采样至16000Hz，独立Worker编码固定44字节头PCM16LE WAV；worker/worklet均同源平铺资源，CSP需worker-src self。0.5–60秒，总次数默认3（包含首次）可选1–10；上滑/取消/隐藏/失焦/离页及权限迟到不发送。录完再次核对原班级、identityRevision和runtimeGeneration，变化即取消。

上传不等于排队，COMMITTED不等于声音已听到；STOPPING必须等驱动归还后才显示终态。输出设备当前为UNVERIFIED，实际播放时由WAVE_FORMAT_QUERY核验；不能从会话可用推断扬声器可用。未知提交只能手动GET原任务ID，不自动补发；后台停止周期查询。正常TLS/生产监听、真实教师手机麦克风和校园网络仍须单独验收。详见[语音契约](../开发计划/15-教师语音API契约.md)。
