# M1 真实使用体验范围决策

本文承接 `02-capability-requirements.md`、`03-foundation-scope-decisions.md` 和 M0 验收报告，固定 forme M1 的范围。M1 的含义是**增强已经闭合的内核**，不是把 M0 重新解释为待补骨架。

## 1. M1 目标

M0 已证明完整受治理链路能运行。M1 要证明这条链路能够被持续使用、观察、纠正和评价：

1. 用户能从本地控制面提交、观察、审批、取消和恢复 run。
2. 用户能查看 trace、failure、verification 和 candidate，并做显式反馈。
3. 到期承诺和后台任务能在重启、并发和预算边界下可靠运行。
4. context、skills、MCP 和 plugins 在真实长任务中按需加载，而不是无限膨胀。
5. 真实模型任务能进入可重复的 golden-task/manual-eval 流程。

M1 的北极星不是“工具更多”，而是：**同一个受治理内核开始形成可日常使用、可反馈、可评价的工作闭环。**

## 2. 范围原则

- **Harness-first 不变**：控制台、scheduler、notification、plugin runtime 都不能直调 loop 或 backend。
- **事件仍是唯一事实交换**：控制面只读 event/projection；稳定变更仍经 store 单写者。
- **M0 契约不回退**：S1–S22 是 M1 的永久回归门。
- **体验增强不等于自治升级**：M1 以 L1/L2 为主，不默认开放 L3+。
- **先评价再扩触点**：trace/manual eval 先于 Browser、Computer Use 和外部平台 connector。
- **候选先于稳定**：控制台中的 review 是治理入口，不是绕过 candidate lifecycle 的编辑器。
- **复用现有事实模型**：background job 优先建立在 `ProspectiveIntention`、schedule source、run lifecycle 和 intent idempotency 上，不建立第二套不可审计队列事实。

## 3. 范围决策

| 编号 | 主题 | M1 决策 | 不进入 M1 核心的部分 |
|---|---|---|---|
| D20 | 阶段定义 | M1 是真实使用体验增强；M0 的 18-crate 核心闭环视为已完成。 | 重做 M0 crate 边界、把既有内核降级为占位。 |
| D21 | Local Gateway | 实现 loopback-only daemon/app-server，承载 run、event、approval、cancel、resume、trace 和 review API。 | 公网控制面、多租户、远程云端管理。 |
| D22 | 本地控制台 | 提供工作型本地 Web 控制台；所有数据经 Gateway API/projection，不直读 SQLite 或内部 crate。 | 营销站、完整桌面壳、多端同步。 |
| D23 | Surface 契约 | 增加 `SurfaceProfile`、`GatewayProfile`、cursor 和控制请求协议；CLI 与 Web 仍提交同一 `RunRequest`。 | 每个入口私有的 run/approval 协议。 |
| D24 | Trace 与 review | 展示 run timeline、DecisionTrace、verification、FailureEvidence、candidate lineage；review 只能触发已有治理迁移。 | 编辑历史事件、删除失败记录、直接改 stable projection。 |
| D25 | Manual eval | 建立项目自有 golden tasks、deterministic/human rubric 和带 trace refs 的报告；先测基线，不使用分数自动 promotion。 | 大规模 LLM judge、自动 prompt hill-climbing、自动策略提升。 |
| D26 | Background job | 用持久化 intention + lease + schedule run 实现 scheduler MVP；支持重启恢复、取消、预算、过期和 intent 去重。 | 分布式 scheduler、跨设备队列、未知副作用自动重试。 |
| D27 | 主动工作流 | 第一批只做到期 Commitment 和 verification/failure follow-up；继续经过 ValueGate、CompetenceGate、AttentionBudget、policy/envelope。 | 默认常驻观察、自动外发、L4/L5 自治。 |
| D28 | Notification/ApprovalChannel | 增加本地 notification backend 和审批投递；高风险 delivery 仍走 immutable plan digest 与 approval。 | Slack/邮件/公网消息平台 adapter。 |
| D29 | Context | 自动 compaction、session/project search 和 topic memory；压缩必须保留 approval/tool/decision/evidence lineage。 | 跨项目全局自动检索、全量个人数据湖。 |
| D30 | Skills/MCP | skill 按需加载；MCP dynamic discovery、schema 延迟加载 和 bounded tool search，解析后仍执行 policy recheck。 | MCP marketplace、远端托管生态。 |
| D31 | Plugin runtime | local plugin 支持 load/enable/disable/reload/failure isolation，贡献集随 lifecycle 原子更新。 | 动态 ABI、远端自动安装更新、managed plugin policy。 |
| D32 | User/Agent review | 增强 UserModel conflict review、CognitiveMap/AgentSelf candidate review 和 LearningAgenda 维护。 | 自动 promotion、自动扩大 trust/delegation。 |
| D33 | 高风险触点 | Browser/Computer Use/API connector 明确留在 M2；PTY 不作为 M1 完成条件。 | 真实桌面控制、任意公网 API、远程 executor。 |
| D34 | 协议演进 | M1 默认复用 M0 的 86 个 EventKind；如确需新增事件，必须先修改 canonical/architecture taxonomy、提供 migration note，再改 protocol。 | 在 gateway/UI 私自发明不可回放事实。 |
| D35 | 交付波次 | M1-A 控制/评价面 → M1-B 后台/主动工作流 → M1-C context/capability 效率。每波都保持 S1–S22 与合规门全绿。 | 横向同时铺开全部能力。 |

D33 是当前 M1 范围的收敛结论：它以较晚形成的 `requirements/03`（M2 扩展 browser/computer use）为准，覆盖 `requirements/02` 和早期 planning 中曾把 Browser/PTY 列为 M1 候选的旧切分。后续若要提前，必须先正式修改本文件及 M1 验收范围，不能仅凭旧规划直接实现。

## 4. 核心用户工作流

### 4.1 本地 run 控制

用户从 Web 控制台提交与 CLI 相同的请求，按 `stream_seq` 查看实时事件，在同一界面完成 approval、cancel、resume，并在结束后查看 trace。

### 4.2 失败与候选 review

用户从 run trace 进入 FailureEvidence、CognitiveMap/UserModel/AgentSelf candidate，查看 evidence lineage 后选择 confirm/promote/reject/retract。控制台不能直接写 stable 状态。

### 4.3 到期承诺与后台任务

用户创建或 defer 一个 intention；daemon 到期 claim lease，生成 `source=schedule` 的 run。重启后恢复未完成 lease，副作用不明时进入 waiting/manual review，不盲重试。

### 4.4 受控主动跟进

任务存在 verification gap 或 failure hotspot 时，系统在 attention budget 内提出 follow-up proposal。拒绝、defer、accept 都进入事件和 suppression/evidence。

### 4.5 高效能力装载

长任务先获取 skill metadata、MCP/tool 摘要和 plugin contribution；只有协调选择后的正文/schema 才进入 context，执行出口仍由 policy/approval 重查。

## 5. 交付范围

### M1-A：控制面与评价面

- Local Gateway daemon/app-server。
- 本地 Web 控制台。
- run/session/event/approval/cancel/resume API。
- trace/failure/candidate viewer 与 review。
- golden task + manual eval runner。

### M1-B：后台与主动工作流

- scheduler MVP、lease、restart recovery、cancellation。
- Commitment reminder。
- verification/failure follow-up。
- local Notification/ApprovalChannel。
- AttentionBudget 与 feedback 指标。

### M1-C：上下文与能力效率

- automatic compaction 与 lineage preservation。
- topic memory、session/project search、candidate conflict review。
- skill 按需加载。
- MCP dynamic discovery/schema 延迟加载/tool search。
- plugin runtime lifecycle。

## 6. M1 非目标

- BrowserBackend、ComputerUseBackend、AppApi connector、远程 executor。
- Slack/邮件/企业协作平台等真实外部 adapter。
- 多租户、公网 Gateway、跨设备同步。
- LLM judge、自动 regression generation、自动 promotion。
- managed policy、plugin marketplace、远端动态 ABI。
- L4/L5 默认自治、自动外发、高风险动作自动批准。
- loop/coordination/capability/trust strategy 自进化。

## 7. 成功指标

M1 验收必须报告，而不是只收集以下指标：

- **治理正确性**：harness bypass = 0；未经授权 action/delivery = 0。
- **事件正确性**：stream reconnect 丢失/乱序 = 0；trace 关键引用完整率 = 100%。
- **后台可靠性**：重启/lease expiry fixture 中重复副作用 = 0；unknown outcome 盲重试 = 0。
- **评价能力**：golden tasks 覆盖 final-only、审批工具、长 context、background/proactive 四类任务，并保存模型/profile/policy/tool/schema snapshot。
- **主动质量**：记录 proposal accept/reject/defer、打断与 hitchhike；M1 先建立基线，不用单一接受率驱动权限升级。
- **效率**：compaction 和 按需加载 必须报告 token/context 变化，同时证明治理 lineage 未丢失。

## 8. M1 完成定义

M1 完成需要同时满足：

1. S1–S22 全绿且合规门 PASS。
2. `06-m1-verification-strategy.md` 的 S23–S37 全绿。
3. 三个波次均有事件序列、store/projection 和反向断言。
4. 至少一次真实配置模型 golden-task run 产生完整 trace/eval report。
5. 控制台、scheduler、notification、plugin runtime 均无 harness 绕行入口。
6. 输出 `docs/acceptance/m1-acceptance-report.md`。

## 9. 下游约束

- `architecture/04-m1-experience-architecture.md` 必须逐项实现 D20–D35，不得重新扩展范围。
- `prd/18-m1-program-prd.md` 必须按 M1-A/B/C 顺序拆解并映射 S23–S37。
- M1 实现若修改 protocol、crate 依赖边或 EventKind，必须先完成对应文档变更和迁移说明。
