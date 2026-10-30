# 未来教室教师工作台（配对与会话阶段）

本目录是教师浏览器运行的静态应用，使用 TypeScript、React、Vite、Tailwind CSS 与本地 shadcn/ui 组件。白板提供同源静态文件和 Rust API；白板不安装 Node，不使用独立云服务。本轮实现配对、等待本地批准、读取真实会话和明确注销。四个业务入口标明“尚未接入服务”，没有模拟发布或虚构教师数据。

当前 Rust 的 request-port 配对契约已经存在；本目录的静态构建通过**不代表正常产品监听、真实 TLS、可信证书部署或办公室到白板浏览器配对已经验收**。生产静态宿主由仓库根目录的实现与验证记录管理。

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
- 四个业务入口目前禁用。页面显示服务器 ID，未接入班级/科目名称查询时不猜测名称；没有远程账号、角色、课表、布局或文件启动接口。

## 产物与预算

`npm run build` 在 `dist/` 生成 `index.html`、带指纹的 `assets/*.js/css` 和 `fc-assets.json`。清单包含精确 schema/API/UI 版本、文件相对路径、字节数与 SHA-256，不包含自己；没有任意文件映射或 API/SPA fallback。生产脚本和样式均为同源外部文件，无内联 script/style。

`scripts/budget.mjs` 硬检查：最多 64 个声明文件、静态总资源 `<=5MiB`、所有首包 JS+CSS 合计 gzip `<=512KiB`。准确数值见每次构建产生的 `build-budget.json`（构建证据，不放进静态部署目录）。测试使用 synthetic 请求/响应与 fake clock；测试绿灯不是 TLS、真实监听、Windows 或学校浏览器验收。

## 组件来源与许可证

按官方文档手动配置静态 Vite/React 项目：[React](https://react.dev/learn/build-a-react-app-from-scratch)、[Vite](https://vite.dev/guide/)、[Tailwind Vite](https://tailwindcss.com/docs/installation/using-vite)、[shadcn Vite](https://ui.shadcn.com/docs/installation/vite)。本地 Button/Input/Card 来自 shadcn 官方 `new-york` registry，保留 [MIT 许可](SHADCN-LICENSE.md)。不依赖托管 Sites 或 Cloudflare 服务。

`scripts/licenses.mjs` 按 `package-lock.json` 生产依赖闭包从已安装包读取完整 license/notice，生成 `THIRD-PARTY-NOTICES.txt`，并在生产 JS 的 post-minification banner 中保留。涵盖 React、Radix、lucide、CVA、clsx、tailwind-merge 等；开发工具不打包到浏览器。
