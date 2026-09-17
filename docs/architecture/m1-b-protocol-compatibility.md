# M1-B Protocol Compatibility Note

本文记录 M1-B background/proactive/notification 协议相对 M1-A 的兼容、拒绝与 replay 边界，承接 `04-m1-experience-architecture.md` §4/§8/§9。

## 不变项

- `EventKind::ALL` 仍为 86；没有新增、改名或复用 EventKind。
- M0/M1-A 的 `stream_seq`、append-only store、单写者与 replay 顺序不变。
- 没有新增 workspace crate 或内部 crate 依赖边。Gateway 仍只经 Harness façade 访问 scheduler/memory/execution。
- Shell/File/MCP 参数与 M1-A control DTO 的 JSON 形状不变。

## 新增对象

- `ProspectiveIntention`、`ScheduleBinding`、`ScheduleCommand`、`SchedulerConfig`、`ScheduleClaim`、`ScheduledJob`、`SchedulerTickReport`、`RecoveryReport` 均为 versioned protocol DTO。
- `DeliveryMode` 固定为 `Hitchhike | Interrupt | Digest | Internal`。
- `ConfigCheck` 新增 `Scheduler` 与 `Notification`。旧 Rust exhaustive match 和拒绝未知 enum 的旧 JSON consumer 必须升级后才能读取这两个 finding。

## 既有 payload 增量

- `OpportunityDetected.activation_shape` 从必填 shape 变为 optional。旧 shape JSON 继续读为 `Some(shape)`；确定性 Commitment 写 `None`。不能读取 `null` 的旧 consumer 必须先升级。
- `ProactiveProposalEmitted` 新增 `delivery` 与 `attention_cost`。读取旧事件时安全默认是 `Hitchhike` 与 `0`，避免把历史 proposal 推断成一次打断。
- `ProspectiveIntentionCreated` 新增 optional `schedule`，保存 session、AutonomyEnvelope 与 budget。读取旧事件时默认为 `None`，旧 intention 仍按非 scheduler intention 重建。
- `CompetenceInputs` 新增 `verification_evidence`。读取旧 `CompetenceGateEvaluated` 时默认为空；新 verification fail/unverifiable follow-up 必须写实际结果 event ref。

## 迁移与拒绝规则

- SQLite/event log 不做就地数据重写；升级后的 reader 通过 serde 默认读取旧 payload，权威历史保持原样。
- `schedule=None` 的旧 intention 不得被 scheduler 私自补 envelope 或 budget；只有新 owner command 可创建可调度 binding。
- 新 lease ref 带 prior intention event generation，用于 store domain idempotency；legacy lease ref 仍可 replay，但只有新格式参与跨 handle 原子 claim。
- 旧 consumer 遇到 `activation_shape=null`、新 `DeliveryMode` 或新 `ConfigCheck` 时允许显式拒绝，禁止降级映射成其他事件、backend 或检查类型。
- Notification target、scope 或 body ref 的任何变更都会使原 plan digest/approval 失效；compatibility fallback 不得绕过执行前重查。
- protocol contract tests 必须同时覆盖新对象 round-trip、旧 payload decode、未知 enum 拒绝与 `EventKind::ALL.len() == 86`。
