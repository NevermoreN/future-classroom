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

可独立推进固定owner的真实TLS生命周期：接纳/IOCP域与固定SSPI调用worker分开；期限到达先逻辑失效并关socket，原物理slot及许可仍保留，直到内核IOCP完成和SSPI调用实际返回均已观察到，超时保留并故障闭锁，不启动替代owner。该切片尚未实现，不代表持续接纳、内核/SSP总资源或严格5秒已解。正常listener保持关闭，完整开发目标继续；现有期限与平台门槛没有放宽。
