# 显式 MCP 发现与主动能力门的窄边界补充（待确认）

原三项方案及 McpDiscovery 动作已批准，授权不变。此补充只处理正式实现过程中发现的治理语义矛盾，不授权一般工具调用或扩大 Agent 自治。未批准前，保持现有执行拒绝。

## 已复核的冲突

canonical §21 规定冷启动应能作为反应式助手用工具，§6 规定主动路径能力不足必须降级。architecture/03 §6 和 PRD03 §5.3 又要求所有 outward 动作出口重跑能力门。目前 action_policy 把 High/Outward 的一次性审批动作映射为 L5；EvidenceCompetenceGate 的证据分支最多给到 L4，高风险最多 L3。prepare_action 在消耗审批前继续比较上限，因此正式用户即使批准首次发现，也不能执行。仅替换为 FixedCompetenceGate(L5) 会伪造生产事实，不可采用。

## 建议的精确增补

canonical §6/§21、architecture/03 §6、PRD03 §5.3 增加同一窄说明：

> 已认证 owner 明确发起、无模型参与选择的受管 MCP 目录发现，是一个受限配置动作。Harness 仍在审批前后读取和记录真实 CompetenceGate 结果；该结果限制 Agent 自主提出和扩大行为的上限，不把缺少历史学习记录等同于拒绝 owner 本次有限操作。执行资格另由已认证 owner 原请求、该次 exact-plan 一次性批准、当前 policy/permission/envelope、完整注册和配置摘要、Host 当前性核对、已验证隔离 profile、严格预算与期限共同确定。任何拒绝、撤销、未知原动作或来源缺失仍阻断。真实学习上限不提升，不制造能力/信任证据。

此分支仅在以下条件全部满足时适用：

1. 专用认证 owner 入口生成私有、加密、可验证的持久请求绑定，关联 owner/run/session/command/input/scope/composition/worker authority/spec；普通 RunRequest 自报 OwnerControl、模型、定时/主动任务不能生成该绑定。
2. 参数只允许已批准 ManagedMcpDiscoverySpec，无 tools/call、资源读取、shell、任意环境、网络或凭据。首个已验证 profile 为 windows-appcontainer-mcp-stdio-v1；新 profile 不继承此窄例外。
3. per-action Session Ask + Owner delegation 只由该绑定生成；所有 Deny 优先。原申请、计划摘要、审批 nonce/版本/期限必须一致；未经批准零进程启动。
4. 最终执行仍经过原 Harness、Host 独立当前注册/configuration 重查、Low AppContainer/Job 与严格输出边界。批准不允许绕过 OS 身份、注册撤销、来源变更、生命周期删除或实际写入重查。
5. pending 重启只还原同一加密原件和原不可变计划，零模型调用、零新发现；已批准/已消费/已规划/已开始的模糊状态不得还原为可再执行的 pending。Unknown 只核实原 job。
6. 完成后的目录只由保护原件和原事件链发布，发布失败仅恢复发布；不得重跑程序、追加矛盾动作终态或伪造学习成功。

## 明确保持的边界

普通 MCP 工具调用、桌面操作、Shell、PTY、外发消息和全部主动/定时/模型选择的动作继续适用现有能力门。本补充不改变任何冻结 trait、Action/Worker/Approval DTO、99 EventKinds、18-crate 依赖图和 unsafe 文件；不把新路径当成完整 IP 验收。若需要把同一设计扩展到其他动作，须另有明确文档范围，不能自动泛化。

## 实施验收

使用真实 Harness、生产 EvidenceCompetenceGate、原审批 broker 和真实持久 store；运输层用计数夹具验证组件，原生 Host/安装证据独立保留。正例证明冷启动 owner 请求先待批，批准后仅发现一次，能力事件仍记录真实 L0/L1；反例覆盖伪造来源/owner、缺私有绑定、主动/模型入口、无批准、Deny、旧 nonce、过期/漂移、撤销、删除、pending 重启和发布中断。不能用固定 L5 夹具证明此路径可用。
