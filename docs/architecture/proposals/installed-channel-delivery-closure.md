# 安装版渠道出站的有限代理补充

状态：2026-11-11 owner 已明确批准并要求实施（“批准有限出站代理方案并实施”）。授权范围为本文的本地有限代理；不是普通工具能力门例外，也不授权向真实联系人发送消息。文本接收、工具快照、插件与 Host 修复的原批准继续有效。

## 已核实的协议缺口

原已批准的 `installed-managed-consumers-closure.md` §3.6 要求渠道输出经 disclosure/termination/envelope、Harness 与隔离 backend。PRD24 §4.1.2 同时冻结“当前冻结的 Worker DTO 没有网络/credential grant，因此 worker 的直接网络和 credential 能力保持拒绝”，并要求“未来增加某类能力必须先变更协议/PRD并形成独立的 exact-scope grant”。现有 LoopbackChannelAdapter 生成 AppApi 动作，但安装版只有 File/MCP/Computer 的实际消费者；删掉 enabled 拒绝无法完成出站，也不能用 Brain 进程里的 HTTP driver 代替隔离链路。

## 建议的确定边界

保留现有 WorkerJob/Permit/ExecutionSpec 与五处原生边界。为已配置的本地 Loopback connector 增加独立、版本化的单次出站代理授权；不授予 AppContainer 直接网络能力、loopback exemption、共享代理环境或可复用密钥。Text 接收协议与其他 backend 原封不动。

- `ManagedChannelDeliveryGrantV1`：schema_version、grant_ref、installation_ref、owner、channel_ref、connector_ref、source_digest、participant、scope、job_id、action_ref、plan_digest、normalized_action_digest、permit_digest、authority_epoch、nonce、endpoint、method、content_digest、max_input_bytes、max_output_bytes、expires_at。固定 POST，endpoint 必须为预注册的精确 loopback IP/端口/路径，不接受 hostname、redirect、代理、userinfo、fragment 或模型选择的 URL。内容至多 64 KiB，回执至多 64 KiB，期限不超过原 job/permit/会话；不含 secret 值。
- `ManagedChannelDeliveryInputV1`：schema_version、execution、grant_ref、content_bytes。内容必须匹配既有最终计划引用的 content digest。Brain 仅提交自己有权读取且已通过 disclosure 的原内容；Host 独立重查 grant/source/当前 channel enabled/participant/plan/permit，拒绝跨工作区、跨接收方、重复 nonce 和替换正文。
- Host 来源目录增加版本化的可选 delivery 投影：精确 endpoint、注册 connector/schema、credential_ref、允许的 participant、单次容量。凭据仍由 Host 专用目录保管，只在代理构造请求时装载，不进入 AppContainer、脑事件、普通日志或浏览器。现有 channel 配置不得自带这些权威字段。
- 同一个隔离 worker 使用既有有界 stdio 接缝，只能请求本 job 的这一份固定消息；Host 代理只转发已经独立核验的确切请求。worker 仍在本 job 的 AppContainer/Job，无法直接访问网络/其他 job/keys。代理是 AgentExecution 的有限能力，不属于 OwnerMaintenance 任意 HTTP 服务；不提供浏览器可直调的发送路由。
- 原能力门、策略、ExternalCommunicationGrant、代表身份、披露、终止与最终审批全部保留。不会把 owner discovery 的窄例外扩展到发送。审批通过前不连接接收方；请求可能已发送后断连/崩溃只保留原 Unknown，不重派、不换 job。仅由原收件方按同一幂等键的受认证回执证明结果；不以 HTTP 200 或进程退出自行推断消息已被处理。
- 本补充不启用第三方平台。验证使用仓库自建 loopback 收件方；向真实联系人发送消息仍须用户对该消息/接收方的明确指令。

## 实施与验证

批准后先将上述字段、摘要域、期限/长度与拒绝表回填正式契约，再实现源注册、授权 journal、WorkerActionBackend 输入消费者、隔离 stdio worker、有限代理及原结果封存；不新增 crate/EventKind/unsafe 文件。

验证正向本地交付及原始回执；错误 owner/source/plan/recipient/body/nonce、过期/撤销、未知字段/版本、redirect/非 loopback、秘密不可见、审批前零连接、终止后零发送、发送后断连 Unknown/零重派、Host 重启保留原状态。最后以真实服务身份/AppContainer 和新安装候选联合取得 IP08/IP10/IP18/IP21/IP24 的相关证据。源码或模拟代理回执不能替代原生安装验收。
