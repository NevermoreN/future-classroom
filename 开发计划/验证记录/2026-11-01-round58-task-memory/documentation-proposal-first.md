# R58 COM task 释放文档增量草案（尚未合入计划）

本文件只供主代理在实际验证终态后选用；不修改主树计划或现有验收结论。已逐件复核主树 R58 的 7 个文件与 `implementation-freeze-first.json` 完全同字节，冻结 SHA256 为 `13a1caef80d20bfa475f80b069b93619d457be68d597fba88261eab8232099d5`。主代理集成记录 `main-integration-first.json` 的 SHA256 为 `1e0fac537941aa1a432f17f09f096817a32f7a4870e31c2e5fe537dd11947fce`。此项是源码绑定核对，不能替代编译、原生执行或新 MSVC 导入表证明。

## 拟插入《08-Rust所有权与FFI生命周期.md》第 3 节末尾、第 4 节之前

### 第五十八轮：COM task 内存与精确系统分配器

`SHGetKnownFolderPath`、文件选择器的 `IShellItem::GetDisplayName` 和音频端点 `IMMDevice::GetId` 所返 task 内存，由 Windows 适配层先取得可失败的释放能力，再进入可能返回内存的调用。释放能力只经完整 OS 系统目录对应的**已加载 Ole32.dll**、按精确名称 `CoTaskMemFree` 解析；不用调用点的链接库提示推断最终 PE 导入来源。WOW64 使用 `GetSystemWow64DirectoryW`，原生 x86/x64 使用 `GetSystemDirectoryW`；用有界 `GetModuleFileNameW` 与 Unicode `CompareStringOrdinal` 核对完整路径，不把 System32 与 SysWOW64 当作等价目录，也不用转发后的 COM 函数地址判断模块。ARM64 仍待另行确认。

全进程一个 `OnceLock` 缓存原生函数指针或失败 HRESULT，初始化只对该已加载系统 Ole32 执行一次明确的进程寿命 `PIN`。这份 PIN 到进程终止才结束，不声称发生模块卸载或撤销 PIN；后续路径或导出检查失败也不另建替代引用。无 `LoadLibrary`、basename 搜索或全局 DLL 搜索策略变更，不新增线程/池，也不为原生 owner 手工实现 Send/Sync。目录和实际模块路径各受固定 1024 UTF-16 单元缓冲限制，截断、缺失或身份不符均拒绝。

释放函数的类型严格为 `unsafe extern "system" fn(*const c_void)`，保留 x86 stdcall 的一个指针参数及 void 返回 ABI。成功取得的每个原 owned task 指针，在转换成功或返回转换错误时均恰好释放一次；null 按系统契约为空操作。不以 Rust allocator、HeapFree、LocalFree 或 BSTR 释放器替换 COM task 分配器，不把释放能力放入领域/协议，也不改变原文件权限、选择器取消与音频端点长度边界。解析失败发生在 task 分配之前。真实分配/释放测试也从同一已 PIN 的 Ole32 按名取得 `CoTaskMemAlloc`，避免测试再次引入生成绑定的静态 task 符号。[微软 task 释放契约](https://learn.microsoft.com/en-us/windows/win32/api/combaseapi/nf-combaseapi-cotaskmemfree)、[模块引用及 PIN](https://learn.microsoft.com/en-us/windows/win32/api/libloaderapi/nf-libloaderapi-getmodulehandleexw)、[精确导出解析](https://learn.microsoft.com/en-us/windows/win32/api/libloaderapi/nf-libloaderapi-getprocaddress)。

本轮源码候选的编译、原生执行及新双 MSVC PE 复证以实施记录的最终原件为准，当前均待补。Windows 7 SP1 x86、Windows 10、Windows 11 与希沃前台各自记录实机结果；在 Win11 执行 Win7 target 程序不等于 Win7 本机运行。构建成功、已加载模块查询成功或 GNU 导入表无 combase，均不能代替新 Modern/Win7 MSVC 全 22 程序的实际导入表核验及分平台运行证据。

## 拟追加实施记录的范围与待验段落（结果出现前不升级状态）

### 第五十八轮：精确 Ole32 COM task 释放能力（源码已集成，统一验证进行中）

- 实际源码范围：新增 `crates/fc-windows/src/com_task.rs`、`com_task/tests.rs`；`src/lib.rs` 接入 private 模块；`src/native.rs`、`src/materials/chooser.rs`、`src/audio/device.rs` 三处移除同名静态 task 释放声明并先取得能力；`crates/fc-windows/Cargo.toml` 仅给既有 windows 依赖增加 `Win32_Globalization`。Cargo.lock、crate 数和依赖方向不变。
- 首冻结前实际完成 rustfmt、diff check 与架构元数据检查（12 crate / 33 边）。新增 6 个 Windows 测试源码，覆盖实际系统 Ole32 alloc/free/null、真实缺失导出阻止后续分配、已固定模块/能力寿命、实际 WOW64 目录选择、错误路径/模块及 Unicode 大小写。此处只记录源码和静态检查，尚不记这些测试执行成功。
- 原 R55 Modern 与 Win7 MSVC 原件均曾真实留下 `combase.dll!CoTaskMemFree`。原 Win7 22 PE 中，涉及该符号的 bootstrap 两个测试程序和 normal 同时已有 Ole32 的三项 COM 静态导入，支持当前程序“已有系统 Ole32”的前提；不外推任意外部库用户。现有记录未保存全部相关 import-library 成员和链接选择图，不把唯一原因归为 SDK redirect，也不更改 R55 原成功编译与兼容风险的分层。
- 待主代理补充本轮统一源码的实际编译、lint、6 个测试及真实 WOW64 解析结果；新 Modern 与 Win7 MSVC 22 程序应逐件检查最终静态导入，确认 task 释放不再造成 combase/WinRT 依赖并保留原完整 PE 门槛。新产物未到前不得先写已修复 MSVC/Win7 导入。平台运行、希沃可见前台、P0、生产接纳和长稳等原门槛不因本切片改变。

## 原件说明（不直接粘贴为验收结果）

R55 Win7 只读前提核对来源：`/tmp/fc-round55-ci-evidence/win7-original-independent-first.json`，SHA256 `891d22904d0daa884125c51b333c57ed0e8639ac5a7c46ac69ba14aa7bde3e00`，22 程序中的 3 个同时含 task combase 和 Ole32 COM 入口；其 normal SHA256 为 `38d30fd9f661aee5740ac876dddf262a6f538a19cbfc0396568e1d47c07b7656`。这是旧产物的实际限制，不是 R58 修复证据。

本草案形成时主代理的最终 main 统一编译尚未提供终态；未查询其 session、未运行任何 Cargo/Windows/网络操作。以后若源码变更，须先重新核对冻结，不能直接沿用本草案的同字节结论。
