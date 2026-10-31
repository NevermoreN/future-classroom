# P0 平台 ADR：独立 TCP 接纳候选与期限负证据

2026-10-30，第十三轮，D11实施候选。状态：**诊断原型已实现，未采纳为生产传输，P0仍未通过**。不替换既有HTTP.sys、不启用正常监听、不修改全机HTTP/证书/防火墙。首连接5秒与资源门槛保持。

## 问题和候选范围

第十二轮研究未找到私有HTTP.sys配置能同时覆盖未归属的首TCP/TLS连接期限与接纳资源。现有HTTP.sys固定8请求/2头槽/2上传槽不能代表内核已接受但尚未产生RequestId的连接。继续堆请求层计时或扩大观察窗不会消除这一缺口。

本轮实现隔离`fc-windows::tcp_frontdoor`：只在127.0.0.1临时端口创建自身Winsock listener，4个固定Pin<Box<Slot>>，AcceptEx的接收长度0，单IOCP、无新增worker。每次发布记录单调绝对期限，不以收到字节或dequeue完成事件续期；诊断只读取有限明文头，不接业务/凭据/TLS。容量满立即关闭自己的listener，有限单批次域不伪装为可持续接纳服务。

## 原生所有权与可证明范围

每个OVERLAPPED/accepted socket/16KiB头缓冲属于固定slot，只有观察到对应IOCP完成后才结算。关闭先停listener/取消接纳，再排空；CancelIoEx或closesocket返回不当作完成。5秒排空失败保留原预分配owner、IOCP与Winsock所有权，并置进程级FAULTED禁止replacement；不为排空失败分配第二批槽或强行释放被内核引用的内存。

`fixed_owner_bytes`只量化Rust内联结构与4块固定slot内存，不包含诊断Vec、内核TCB/backlog/socket缓冲、Schannel或SSPI缓存。既不把listen backlog参数当实际内核队列长度证明，也不把SO_CONNECT_TIME秒级读数当精确TCP建立时刻。AcceptEx零接收长度有助于在连接接受后立即归属owner，但不能推断监听前/排队/调度各阶段全部已受控。

## 实测改变了什么

三周期验证覆盖空闲取消、完成事件延迟1.2秒、零字节/滴流、完整头及超长头；12次发布都观察到完成，4次空闲取消、8次接受、1个完整头、1个超长头拒绝、6次期限关闭。生命周期测试通过，不意味着期限通过。

精确纳秒观察以客户端connect_timeout返回为起点、逐poll观察EOF或RST。8个样本为：5000577000、5000505900、5000457500、5000411200、1200854500、5000019800、1222017900、4999886800纳秒。**5/8超出5000000000纳秒，最大超577000纳秒（577微秒），严格5秒观察FAIL。** 先前毫秒截断不能作为<=5秒证据；7秒只用于收集失败，不是扩大通过门槛。客户端connect返回也不等于服务端内核TCP建立时刻，TLS/SYN/backlog/休眠和拥塞未测。

SO_CONNECT_TIME实际读到1秒，配合人为延迟dequeue1.2秒，说明从应用收完成的时刻才开始5秒会漏算。官方文档未提供足以重建精确建立时刻的舍入误差保证，不能擅自用“秒数+1”证明原期限。

## 结论与下一步

保留自有接纳域作为后续Schannel组合的候选；本轮不把明文端口或4连接有限批次改名为正常HTTPS。后续须分别解决：持续接纳与未归属backlog、从真正TCP建立起计时的语义、提前关闭预留量与暂停/拥塞模型、SSPI同步调用超时后的有限owner保留、真实TLS1.2/证书信任及三系统证据、应用内/OS共享资源各账边界。普通用户态调度的实测关闭延迟不能靠整数截断消失，也不能未经变更要求就等同硬实时保证。

## 依据

[AcceptEx](https://learn.microsoft.com/en-us/windows/win32/api/mswsock/nf-mswsock-acceptex)、[getsockopt](https://learn.microsoft.com/en-us/windows/win32/api/winsock2/nf-winsock2-getsockopt)、[SO_CONNECT_TIME等选项](https://learn.microsoft.com/en-us/windows/win32/winsock/sol-socket-socket-options)、[CancelIoEx](https://learn.microsoft.com/en-us/windows/win32/api/ioapiset/nf-ioapiset-cancelioex)、[closesocket](https://learn.microsoft.com/en-us/windows/win32/api/winsock2/nf-winsock2-closesocket)、[GetQueuedCompletionStatus](https://learn.microsoft.com/en-us/windows/win32/api/ioapiset/nf-ioapiset-getqueuedcompletionstatus)、[listen](https://learn.microsoft.com/en-us/windows/win32/api/winsock2/nf-winsock2-listen)、[AcceptSecurityContext/Schannel](https://learn.microsoft.com/en-us/windows/win32/secauthn/acceptsecuritycontext--schannel)。本轮浏览内容、事实/推断、命令原文、精度修正前后结果及清理均归档在第十三轮原生证据；原型未运行Schannel。


## 第十七轮只读平台复核

[平台复核报告](验证记录/2026-10-31-round17-p0-platform-review.json)重新核对当前候选与微软文档，未改传输源码、未执行Cargo/SSH/Windows测试。当前有限批次改成持续rearm后，backlog中可能已有建立的连接；新投递AcceptEx时重新起算5秒，会给旧连接额外期限。已有秒级连接年龄、后续新系统专用TCP信息和绑定时刻都不能直接作为三系统精确ESTABLISHED起点证明。旧577微秒超时只是一组失败观察，不是未来调度/网络/关断延迟的上界。

可独立推进固定owner的真实TLS生命周期：接纳/IOCP域与固定SSPI调用worker分开；期限到达先逻辑失效并关socket，原物理slot及许可仍保留，直到内核IOCP完成和SSPI调用实际返回均已观察到，超时保留并故障闭锁，不启动替代owner。第十七轮时该切片尚未实现，第二十六轮实现与证据另列下节；不代表持续接纳、内核/SSP总资源或严格5秒已解。正常listener保持关闭，完整开发目标继续；现有期限与平台门槛没有放宽。

## 第二十六轮：真实 Schannel 执行域（有限批次实测通过）

`fc-windows::tls_frontdoor` 增加固定一个 SSPI 线程，独占一份入站凭据和四个连接上下文。原生接纳、接收与发送使用同一 IOCP 域和四份固定 pinned 内存；握手、解密、加密通过四个唯一拥有 Frame 移交 SSPI 线程。每连接稳定 token 关联同一原生上下文，新监听周期使用新 generation。`INCOMPLETE_MESSAGE` 保留输入等待补齐，`EXTRA` 保留原输入后缀，握手输出全部发送完成后才处理后续密文。握手最终复核 TLS1.2、至少128位加密和 stream/confidentiality 属性及TLS记录保护，拒绝重协商；诊断只接收固定 nonce GET，不把该窄端点当成生产 HTTP 解析器。

证书只按40位十六进制 SHA1 身份从当前用户 MY 只读选取，复核有效期、显式 serverAuth EKU 和对应私钥；不导入证书或创建/删除密钥。测试 runner 单独创建唯一 MY 测试证书及 CNG 私钥，客户端只信任导出的公共 PEM 并校验 localhost，结束后精确删除该证书和该密钥，核对 MY 摘要恢复。错误 CA/错误主机名须取得真实证书验证异常，不能以关闭证书验证代替通过。SSH、Root、HTTP.sys、URLACL 和防火墙不被配置变更。

批次期限在 listen 之前记录，停止可关闭 socket 而不等待同步 SSPI。SSPI 的当前调用、排队 Frame 及物理许可要等真实结果取回；原生释放结果、IOCP 完成账和 Windows 线程对象终结须逐项确认。释放失败有专门 `Cleanup` 错误，包含部分初始化的清理失败；故障闭锁后不能构造替代域。线程栈及四槽缓冲的新增账已写入 D11 线程与所有权册，仍需完整 Web 增量资源验收。

现代与补充Win7GNU均在同一Win11实际通过两批八连接，覆盖真实加密分片请求/响应、不完整 HTTP、不完整 TLS、TLS 后断开、错误主机名及错误 CA；各有4次握手、2次响应，IOCP45/45、SSPI25/25、Frame归还25、EXTRA2次62字节。完整Windows1053项回归另覆盖真实IOCP加假慢Driver的在途关闭/晚到结果/故障保留，不声称真实OS加密可中断。证据见实施记录第二十六轮；这组测试不证明生产持续接纳、校园可达、完整业务路由、TLS close_notify、真正 ESTABLISHED 起算的严格五秒、三系统证书部署或内核/SSP 总资源上界。正常监听继续关闭，原严格五秒 FAIL 保留。

Schannel 不支持通用 `ASC_REQ_INTEGRITY` 签名请求位，不能据此设置请求或强制对应返回位；TLS1.2、加密强度、记录保护及真实客户端信任校验仍保持。第二次实机握手失败后按[微软通用契约](https://learn.microsoft.com/en-us/windows/win32/secauthn/acceptsecuritycontext--general)更正该实现错误，后续阶段3起真实握手成功，阶段5完整矩阵通过。

原生契约依据：[Schannel 接纳](https://learn.microsoft.com/en-us/windows/win32/secauthn/acceptsecuritycontext--schannel)、[EXTRA 缓冲](https://learn.microsoft.com/en-us/windows/win32/secauthn/extra-buffers-returned-by-schannel)、[旧凭据结构](https://learn.microsoft.com/en-us/windows/win32/api/schannel/ns-schannel-schannel_cred)、[私钥获取](https://learn.microsoft.com/en-us/windows/win32/api/wincrypt/nf-wincrypt-cryptacquirecertificateprivatekey)。上述文档约束与本轮实测分别记账；没有把同步调用的取消请求视作实际结束保证。

真实Decrypt还暴露 input155/used124 的布局：HEADER13@0、DATA95@13、TRAILER47@108、EXTRA31@124。未使用header/trailer仅校验在原输入范围，DATA仍严格在已消费范围，EXTRA保留精确后缀；不能把合法trailer视图跨入EXTRA误判为越界，也不能让DATA越入EXTRA。保留失败原日志、加入布局/越界回归后再以真实TLS重测通过。


## 第二十九轮：共用 HTTP 分帧与显式 TLS 退休

新增 `fc-protocol::http1::Parser`：可信传输提供不可伪造的 `RequestToken` 和 TLS 事实；每请求独占16KiB头部 arena，头总限额包括请求行/终止CRLF，最多64字段/2048字节目标，严格HTTP/1.1和CRLF。头完成立即返回准确 consumed，后缀仍留在调用者原Frame；先通过路由、权限、配额及受控stage准入，再显式 `admit_body`。正文只借用原明文、每次最多64KiB，不缓存整个文件；调用方仍须等待文件worker实际ACK才再次接收。提前EOF、长度超限、额外正文/pipeline均拒绝；重复头保留给既有业务边界，重复Content-Length及TE/Expect/Trailer/Upgrade/Content-Encoding等不支持分帧直接拒绝。OWS规范去除，禁止obs-fold及冒号前空白。该严格端点选择不接纳部分通用HTTP合法写法，并非通用代理。[HTTP/1.1 分帧与消息语法](https://www.rfc-editor.org/rfc/rfc9112.html#section-6.3)。

`http1::response` 冻结成功编码的有界响应头，自动生成唯一Content-Length/Connection: close；204不发送Content-Length，204/205/304沿既有端口仅允许零正文。拒绝连接控制/升级/隐含分帧及CRLF注入。正文sender只记长度和一个不可Clone回执，不拥有正文；同token、同sender、同块且完整真实发送后才ACK。短发送仍由原生owner累计，过短/过量ACK和源提前EOF不能假完成；模型ACK本身不是IOCP证明。

TLS新增 `Shutdown` 和 `Retire`：同一SSPI线程先ApplyControlToken(SCHANNEL_SHUTDOWN)，以空输入调用ASC，将实际输出发完；socket关闭、原IOCP归还和普通Frame返回后，才能用原token请求实际DeleteSecurityContext。只有原 `Retired` 被worker真正poll收回后允许同槽新serial；Stopped或socket已关闭均不代替退休确认。错误后的上下文身份仍保留到显式退休，清理错误永久闭锁；停止路径仍等实际worker close及Windows线程对象终结。原绝对五秒与排空期限不因关闭调用重置，也不声明SSPI可硬抢占。[微软正常关闭流程](https://learn.microsoft.com/en-us/windows/win32/secauthn/shutting-down-an-schannel-connection)。

当前仅把这些共用基础接入原有限两批loopback诊断，诊断仍只分派一个合成GET，未接通账号/资料/语音的生产worker和计费stage，也未实现持续listener复用。新增四份头arena和实际保留响应头容量计入报告，瞬时解码DTO/通道/分配器/SSPI/内核另计；这不是完整16MiB/进程资源门槛证明。实测范围与原始失败见本轮实施记录，不以诊断成功替代生产交付。

### 条件接纳候选的只读排除依据

本轮复核SO_CONDITIONAL_ACCEPT/WSAAccept官方契约：该选项改变完整接纳时机，同时禁用内建SYN保护和backlog；另一页又明确，为抵御SYN攻击先完成握手的路径会使选项不起作用，即使callback仍被调用。因此仅捕获callback时间不足以证明真正ESTABLISHED的下界；文档也未给出可用于D11的半开连接硬资源界，不能以四个应用槽替代证明。没有在用户电脑启用此选项或改变原接纳路径。[选项说明](https://learn.microsoft.com/en-us/windows/win32/winsock/so-conditional-accept)、[WSAAccept限制](https://learn.microsoft.com/en-us/windows/win32/api/winsock2/nf-winsock2-wsaaccept)。严格首TCP五秒仍保留第十三轮5/8超限、最大超577微秒的FAIL；生产监听继续关闭。
