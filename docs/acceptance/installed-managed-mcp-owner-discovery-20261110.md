# 显式 MCP 发现入口与恢复组件（2026-11-10）

实现 `3cb1819ba1ff951684cba9e95cbe00e11f1f8f3c` 按已批准的[能力门窄补充](../architecture/proposals/installed-owner-directed-discovery-competence.md)接通认证 owner 发现入口、exact pending 恢复与完成目录发布。生产 EvidenceCompetenceGate 的真实结果仍被记录，未修改普通工具、主动任务或模型选择动作的执行上限。本批是源码与组件闭合，不是安装版 MCP 全链路或 IP01–IP24 完成。

## 行为与边界

- 只有专用 owner 入口可生成加密、不可变的 owner/run/session/command/spec/composition/worker 绑定。普通 RunRequest 自报 OwnerControl、模型提议或定时输入均不能获得该分支。
- 每次操作按原 Harness、Policy、exact-plan 审批和持久一次性 nonce 执行。Session Ask 和 Owner delegation 仅对私有原请求有效；所有政策 Deny、显式 envelope/delegation Deny、当前配置与生命周期 fence 保留。不会给生产能力门填造 L5，也不记录目录发现的学习成功。
- 等待批准时保存完整计划字段、原 ApprovalRequest、SessionBound 与 verification pass 数。重启只还原未解决、未开始且当前绑定一致的原 pending；不重新调用模型、不重新规划发现。旧批准、漂移、缺原件和已解决状态不回退为可执行 pending。
- 完成后从保护原件、原事件链和 Host 当前来源发布唯一 McpDiscovered。发布中断后只重做发布，零再次执行；已完成动作不会被补写为 ActionDenied。被拒绝的运行不能通过发布接口返回成功。
- Unknown 保持挂起；通用人工结果接口在控制 claim 前拒绝替代原发现回执。**原 job 的只读结果导入尚未接通**，本批不将保守拒绝写成 Unknown 恢复完成。
- Core 与 Web bridge 仅开放发现和原 run 发布两个封闭入口，绑定认证 owner、CSRF、版本、原命令和有界正文，不接受调用方 owner、进程参数、环境、凭据或授权字段。

## 验证

fmt、workspace/all-targets check/test、strict Clippy、code-quality、compliance 全部 exit 0。整仓 **200 targets、1558 passed / 0 failed / 35 默认 ignored**。独立核验 1099 份源码和全量日志摘要，确认 15 个新增场景实际通过；代码提交后 A 波在干净源码上重新执行并封存。

Harness 10 项使用真实 EvidenceCompetenceGate、审批 broker、持久加密 SQLite/authority guard 与计数 worker transport，覆盖冷启动真实 L0、无批准零执行、批准只执行一次、伪造来源/owner、政策与全局 Deny、配置漂移、错误/过期批准、pending 重启、已解决拒绝恢复、生命周期删除、发布中断重取、Unknown 拒绝人工完成以及拒绝态发布。它们不是原生隔离服务验收。

Gateway/Web 5 项覆盖封闭路由/参数、命令绑定、受管配置映射、真实 HTTP 未认证/CSRF/伪造字段/版本/超限/缺运行环境时零 run 与零模型。Windows 的正式装配测试证明普通进程缺少 Brain Service SID 时拒绝且不回退；未宣称普通测试进程完成受管 native dispatch。跨平台分支的正向 pending 重启不计为本机 Windows 已验证事实。

先行测试捕获并修正了通用 Unknown 人工完成、执行层错误学习成功事件和拒绝态发布假成功。第一次全量驱动在前述学习事件反例暴露后主动中止，原日志和 INTERRUPTED 说明保留；正式通过来自独立新前缀 `owner-discovery-verified`，没有覆盖旧失败。

本工作树 ignored `target/product-integration-evidence/` 保留原件：

| 原件 | SHA-256 |
| --- | --- |
| owner-discovery-verified-gates.json | 1bd9ef6b41381727e89b0b6e753296981c034738405e782b0f737cef5ec22817 |
| owner-discovery-verified-independent.json | c2ca9ca7caef598e92172c42d4daa2c519dd92377ece61cbb65f1da9c7da1cd9 |
| owner-discovery-clean-a-gates.json | 5f7a9843e71d388cc55ba7316adfeba6a3891f6abc3e03c7a651825461f16620 |
| owner-discovery-verified-build-resources.json | cbd69c798984bb8ff79b2989ce85bbf4042ad867ca5e328de0023acdd2540165 |

## 保持开放

新目录对新 run 的正式工具快照生效、Host 独立 placement 与公开 enabled、设置 UI、Unknown 原 job 读取，以及插件/渠道的原接线目标继续开放。普通 MCP 工具仍按原能力门与 High 风险规则处理。本批没有部署、UAC 或服务管理操作，没有修改真实 owner 数据。完整 M5、原审计替换和最终缓存占用见独立 [M5 记录](m5-acceptance-report.md)；缓存报告不把历次压缩回收量相加为净节省。
