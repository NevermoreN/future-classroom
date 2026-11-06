# 单文件图形安装程序

公开版本为 `1.0.1.0`，唯一显示源是 `crates/fc-domain/PRODUCT_VERSION`。Cargo、MSI、TUF及升级收据继续使用三段安装身份 `1.1.0`；公开1.0.x.y映射内部1.x.y，此映射保留原安装、精确前驱与版本比较协议。关于页、安装向导和EXE版本资源显示公开版本。

当前升级链从已发布1.0.0.3开始，废除0.1.9及以前编号作为当前发布/安装/更新入口。前驱基线见`release-baselines/1.0.0.3.json`：原官方EXE和MSI摘要、源码与CI固定后，重放原完整来源，生成原协议精确前驱；安装时仍核验实际本机，不要求重复三阶段测试，也不制造运行收据。旧版本原件仅用于历史复现。

先按既有正式MSVC流程生成并核验原始 sealed `Setup.exe` 与 `FutureClassroom.msi`，再执行：

```sh
python3 scripts/package-standalone-exe.py \
  --helper /原CI产物/product/fc-bootstrap/fc-installer-lifecycle.exe \
  --bundle /正式打包输出 \
  --report-sha256 正式report.json的SHA256 \
  --previous-setup /交付目录/FutureClassroom-1.0.0.3-Setup.exe \
  --legacy-setup /交付目录/FutureClassroom-1.0.0.2-Setup.exe \
  --output /交付目录/FutureClassroom-1.0.1.0-Setup.exe
```

工具核对原CI helper、正式成员与完整MSI绑定。公开EXE使用v3八成员：当前Setup.exe/MSI/LICENSE/NOTICE、previous/Setup.exe和previous/FutureClassroom.msi（官方1.0.0.3原内层）、legacy/Setup.exe和legacy/FutureClassroom.msi（官方1.0.0.2原内层）。两个旧源分别与受审官方EXE/MSI及同CI原生代码固定摘要一致；不递归包含旧外层。120字节尾部为16字节 `FC-SFX-PACK\0v3\0\0`、前缀长度和八个成员长度u64LE、SHA256（尾部前88字节＋全部成员）。上限32/128/2/4/32/128/32/128MiB，前缀512B至32MiB。四成员v1/88字节、六成员v2/104字节继续读取；sealed内层、四成员fcupdate及ABI2/能力3保持。旧2经原源修复及正常2→3→本版串联，当前MSI与TUF精确前驱仍为3，中间产品不启动。

首次安装显示同风格向导；已有安装时显示“修复并保留数据”和“卸载”。修复使用本版或精确旧版原源恢复软件文件，再按原精确资格升级，成功后自动启动；账号、课表、资料和样式保留。后台LAN自动更新继续正常退出、安装并重启，不弹此人工选择。Windows权限确认仍由系统决定。

卸载默认保留数据，可选择删除全部数据并再次确认。点击安装、修复或卸载即自动结束产品，不询问且无结束程序复选框；先正常退出，5秒超时后仅终止同Windows用户、固定安装路径所对应的原进程句柄，并等待真实结束。不结束MSI/guardian，不清除未结算Pending，不放宽资源归还或安装资格。数据仅在实际卸载成功且产品注册消失后删除，包括当前及已识别旧数据、更新缓存和本产品证书/私钥；失败或部分清理分别显示错误。无注册的已知软件残留在独占维护门限内按当前/精确旧源清单清理，未知文件不擅自删除。

成功安装/修复（含后台安装）在当前用户受保护LocalAppData/FutureClassroom-Maintenance缓存本版四成员完整源；自定义Windows卸载入口使用该EXE，显示产品logo、版本和同风格修复/卸载界面。原始MSI条目隐藏，避免绕过受控生命周期。卸载成功后，独立清理进程等缓存EXE实际结束，再按代次和互斥锁删除本产品维护缓存；重装生成新代次，旧清理不能删除新源。错误不会冒称全部清理完成。

`install`、`repair`为无外层UI的修复/安装调用；`uninstall`打开带保留/删除选择的界面。外层仅解压、协调产品退出及等待真实sealed Setup返回，不直接调用MSI。内部维护参数为固定闭合值。封套完整性校验不是Authenticode代码签名。

首次初始化仅将授权桌面布局、尺寸及配色样式作为预设，与首次账号和凭据同一事务保存。不含原学校、账号、课表、作业、资料、提醒、倒计时内容或显示器ID；历史默认模板与正常升级已有设置保持独立。

固定安装路径只读发现允许Programs/产品目录尚不存在；全部已存在父目录继续拒绝文件和reparse。目录缺失不创建任意路径，交互入口异常显示错误框。旧2/3已卸载但Shell收尾报错，仅在实际无注册、独占门限无Pending、已知软件残留清理并重做快捷方式后结束收尾，不放宽安装资格。
