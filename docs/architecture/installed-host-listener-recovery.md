# Host 接入中断处理

本细化落实 PRD24 的常驻服务与维护面可用性；不改变端口、凭据、域隔离、worker admission、动作恢复或协议。

2026-11-11 只读现场诊断显示已安装 Host 在 serving 阶段退出，reason_ref 对应 `host broker accept failed`。现有循环把除 WouldBlock 外的所有 TCP accept 错误都提升为整项服务退出；回执没有保存具体 OS 错误，不能据此断言是哪一次连接或网络变化触发。

对 Interrupted、ConnectionAborted、ConnectionReset、TimedOut、ConnectionRefused 和 Windows 临时网络/缓冲错误采用每 listener 有界退避（100 ms 起，最多 2 s），同一监督循环继续处理其他域、shutdown 与原 journal 协调。空轮询保持原节流。成功接入后清零退避；WouldBlock 不推迟下一次正常轮询。其他错误保持失败即停，并将非敏感 kind/raw code 纳入错误摘要，便于后续精确诊断。不重新绑定端口、不重发请求、不重启 worker；退出排空保持原计数。

验证覆盖瞬断→成功、有界退避、不同 listener 相互独立、WouldBlock 不退避以及权限/无效句柄等不可恢复错误仍终止。组件回归不证明旧现场具体 OS 错误；原安装后需另验 Host 连续运行及正常受认证请求。
