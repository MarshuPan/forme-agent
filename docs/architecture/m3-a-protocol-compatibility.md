# M3-A Protocol Compatibility Note

本文固定 M3-A 对 `forme-protocol` 的 additive 变更、legacy decode、安全默认和 replay 边界。它承接 canonical §9/§25、`06-m3-controlled-evolution-architecture.md` 与 `prd/20`；兼容读取不等于候选有效、策略可激活或动作获授权。

## 1. EventKind 增量

- 既有 86 个 EventKind 的顺序、字符串、payload mapping 和 wire 语义不变。
- 在既有第 86 项 `ComplianceCheckResult` 后以 O 组 additive 追加 `EvolutionEvaluationRecorded`、`StrategyActivated`、`StrategyRolledBack`。
- `EventKind::ALL.len()` 从 86 变为 89；前 86 项必须与 M2 固定 snapshot 完全相同。
- 新 reader 能读取 M0-M2 事件；旧 reader 对新 kind 必须显式 unknown error，不能误解释为 Candidate/Config 等相近事件。

## 2. Additive 类型与字段

- `Source` 末尾追加 `Replay`、`Simulation`；旧值不重排。
- 新增 architecture/06 §3 的 StrategyDomain、EvolutionImpact、EffectMode、StrategyCandidate、ReplaySnapshot/Bundle、FitnessMetric、EvolutionEvaluation、EvolutionSnapshot、StrategyActivation/Rollback 及所有 ref/newtype。
- `SessionBoundPayload` 追加 optional `effect_mode/evolution_snapshot`。
- `CandidateCreatedPayload` 追加 optional `strategy_candidate`。
- `DecisionTraceRecordedPayload` 追加 optional `evolution_snapshot`。
- `ConfigCheck` 追加 `Evolution/ReleaseAudit`；Compliance scope 追加 dependency/notice/release-tree/secret。
- 所有 optional 字段 serde default 为 `None`；`None` 只表示历史没有该 M3 事实，不能授权新的 live M3 run、promotion 或 activation。

## 3. 安全 decode

- 所有新稳定对象要求当前非零 SchemaVersion；空 domain/scope/ref/digest/case/metric ground-truth relation、unknown enum 和 invalid aggregate version fail closed。
- fitness measured 是带 unit 的整数；不接受浮点 NaN/Infinity、未知 unit、负数用于仅允许非负的维度或溢出累计。
- `StrategyRollback.external_effects_reverted` 使用只接受 false 的 `HistoricalFalse`；wire true 必须拒绝。
- `EvolutionImpact::Constitutional` 可被读取和审计，但不能通过 M3 activation validator。
- Replay/Simulation provenance 由 Harness 盖 `VerifiedProcess`；bundle 或 candidate 自报 owner/trust 不生效。

## 4. Legacy replay

- legacy SessionBound 缺 EvolutionSnapshot 时，M3 reader 构造“无 M3 active refs”的 read view；它不写回事件，也不合成 StrategyActivated。
- legacy CandidateCreated 缺 strategy candidate 时只能按原 memory/cognition/capability candidate 处理。
- legacy DecisionTrace 缺 evolution ref 保持可读；M3 新 live run 必须在 SessionBound/DecisionTrace 绑定 snapshot。
- 历史事件不可重写。M3 active projection 只消费三个新事件；M0-M2 历史不能从 config 或当前 default 反推 active history。
- ReplayBundle 必须记录具体 upcaster implementation identity；same schema edge/different implementation 拒绝，与 store 既有 migration contract 一致。

## 5. 冻结 trait

`EventStore`、`VersionedEventStore`、`EvolutionGovernor`、loop、CoordinationReasoner、OrchestrationLibrary 和 ActionBackend 原签名不改。M3-A 只增加 companion traits，包括显式 `EvolutionAggregateRef` CAS 的 `EvolutionEventStore`；旧实现不实现 companion trait 时保持 M2 行为，且 evolution 自动 activation 默认 disabled。

## 6. Contract tests

- M2 86-kind snapshot 是 M3 89-kind snapshot 的严格前缀。
- 三种新 payload 一一对应 EventKind，round-trip 且 kind/payload mismatch 拒绝。
- 所有新 DTO round-trip；zero/unknown/overflow/invalid digest/unit/false-only boundary 拒绝。
- M0-M2 event fixtures 在 M3 reader 中保持原语义；legacy optional fields 均为 None，不凭空产生 active strategy。
- 旧 reader 遇新 EventKind 明确失败；M3 payload 不能被解释成旧 Candidate/Compliance payload。
- replay/simulation mode 不能被 legacy `Source::Internal` 静默替代。
- activation/rollback aggregate、expected/committed version 必须一致且 committed=expected+1；跨 run rebuild 按 aggregate version 连续，不按 timestamp/EventId。
