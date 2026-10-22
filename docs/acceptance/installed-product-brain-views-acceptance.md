# 工作台主脑投影与认知处置组件验收

2026-10-22，施工分支 `shu/installed-product`，实现提交 `477ef52`；设计说明 `f5ca7bf`、`c014a68`。这是整体交付中的组件检查点，不是 IP20/IP24 或完整产品验收。

## 实际实现

工作台经认证 Web → 签名 bridge → 正式 Gateway → 同一 ProductionRuntime，读取记忆、候选、稳定认知、重评任务、AskToLearn、主动提案和失败的安全投影。分页绑定实际投影 digest，每项保留原 run/event/stream_seq；查询不调用模型、不写脑事件。原始内容引用没有冒充已经解密的正文。

候选处置只接受命令号、原 run/candidate、预期状态与决策。主脑重取证据、稳定目标及传递派生关系，然后调用既有 governor。Owner 确认不能代替结果证据。命令回执持久化；同号同绑定返回原回执，换参数拒绝；started 没有终态时返回 Unknown，同一候选换命令号也不能重复处置。

新增认知页和收件箱连接真实投影、候选处置与 AskToLearn 回答/拒绝/延后，移除旧统计页；修正 Charter、jobs 和 learning 的浏览器 API 前缀。认知控制从超大 harness lib 按职责移出，public trait、事件类型和依赖方向保持。

## 验证与边界

`pwsh -NoProfile -File tools/verify-code-quality.ps1` 在上述源码上完整 PASS：结构回归、规模棘轮（448 文件、63 处历史规模债务未增长）、fmt、all-target check、完整 Rust 回归、strict Clippy、依赖审计、前端类型与 15 个行为测试、嵌入资源可重复验证、bundle 9/9、Windows 临时 NTFS 组件 7/7、默认完整合规、diff whitespace。

新增 harness 5/5 覆盖只读/owner/分页、持久幂等/重启/冲突、证据不足不提升、Unknown 原回执/新号阻断、三层血缘撤回与重启。真实 loopback HTTP 集成覆盖 Web 登录/CSRF/伪造 actor/key 拒绝、正式处置事件、同号零新增、冲突和陈旧快照。该 HTTP 测试使用计数本地模型 fixture，不是实际 provider 或浏览器验收；默认 ignored 的原生浏览器/多进程测试未因此转为通过。

尚未完成：普通主动建议的完整采纳/拒绝/延后、Unknown 的探测裁决、认知纠正与真正遗忘、原始内容授权读取、附件/产物、完整设置与数据页，以及原生安装/IP 联合验收。内部投影当前扫描 owner 事件，输出有界，后续增量优化不能形成第二事实源。所有这些继续列在整体交付清单中。
