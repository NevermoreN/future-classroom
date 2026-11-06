# 单文件图形安装程序

公开版本为 `1.0.0.2`，唯一显示源是 `crates/fc-domain/PRODUCT_VERSION`。Cargo、MSI、TUF及升级收据继续使用三段安装身份 `1.0.2`；此映射保留原安装、精确前驱与版本比较协议。关于页、安装向导和EXE版本资源显示公开版本。

当前升级链从已发布1.0.0.1开始，废除0.1.9及以前编号作为当前发布/安装/更新入口。前驱基线见`release-baselines/1.0.0.1.json`：原官方EXE和MSI摘要、源码与CI固定后，重放原完整来源，生成原协议精确前驱；安装时仍核验实际本机，不要求重复三阶段测试，也不制造运行收据。旧版本原件仅用于历史复现。

先按既有正式MSVC流程生成并核验原始 sealed `Setup.exe` 与 `FutureClassroom.msi`，再执行：

```sh
python3 scripts/package-standalone-exe.py \
  --helper /原CI产物/product/fc-bootstrap/fc-installer-lifecycle.exe \
  --bundle /正式打包输出 \
  --report-sha256 正式report.json的SHA256 \
  --output /交付目录/FutureClassroom-1.0.0.2-Setup.exe
```

工具核对原CI helper、正式包成员与内层Setup对完整MSI的绑定。最终EXE由原normal helper、固定四成员串联和88字节尾部组成，成员仅为Setup.exe、FutureClassroom.msi、LICENSE、THIRD-PARTY-NOTICES.txt。尾部为16字节 `FC-SFX-PACK\0v1\0\0`、前缀长度u64LE、四个成员长度u64LE、SHA256（尾部前56字节＋成员串联）。大小上限依次32/128/2/4MiB，前缀512B至32MiB。

首次安装双击打开与产品同风格的原生安装向导；已安装时，无参数双击直接执行更新，不再显示安装选择或成功后启动按钮。原sealed Setup先只读核验精确前驱及完整来源，通过后向同用户、相同安装路径的隐藏宿主发送正常退出请求，等待原进程真实结束，再按原独占维护门限重新核验并安装。外层等待安装成功、清理解压目录后，从正式安装目录自动启动新版并退出；失败才显示错误。不强杀、不删除账号课表资料、不放宽前驱资格，不绕过Windows权限确认。未支持的旧版本仍拒绝，不能因自动关闭流程而获得升级资格。

`install`、`repair`、`uninstall`参数保留无外层界面的受控调用。外层仅在当前用户受保护唯一目录解压，固定成员防写pin，等待真实内层进程退出并清理；不直接调用MSI，不改变原source seal、安装资格或用户数据。首次安装中不能直接关闭或抛弃子进程，成功后可打开程序。该封套校验不是代码签名。

首次初始化仅将授权桌面布局、尺寸及配色样式作为预设，与首次账号和凭据同一事务保存。不含原学校、账号、课表、作业、资料、提醒、倒计时内容或显示器ID；历史默认模板与正常升级已有设置保持独立。
