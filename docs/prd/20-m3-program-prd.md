# PRD 20：M3 受控自进化实施计划

本文是 M3 的实施级总 PRD。它承接 `requirements/09-m3-scope-decisions.md`、`10-m3-verification-strategy.md`、`architecture/canonical-contract.md` §25、`architecture/06-m3-controlled-evolution-architecture.md` 和冻结的 M0-M2 模块 PRD。实现不得在模块内重定义同名对象或放松既有治理。

## 0. 定位与完成状态

- **目标**：把 StrategyCandidate -> Replay/Eval -> Promotion -> Activation -> pinned run -> Rollback 做成第一条可运行、可审计、可离线复核的闭环。
- **冻结基线**：18 crates、86 EventKinds、S1-S52、M2 typed artifacts、canonical §1-24 和 compliance doctor。
- **M3 additive 结果**：89 EventKinds；所有 M3 DTO versioned；无内部 crate edge 变化。
- **交付节奏**：A/B/C 三波。每波实现、全回归、报告、owner review 后才进入下一波。
- **当前状态（2026-07-18）**：M3-A/B/C 均已按 owner review 顺序完成；M3 final 通过 S1-S69、89-kind、18-crate、309+2 Rust tests、真实 browser evolution golden、typed artifacts、strict clippy、原创门与 clean-tree release audit。

## 1. 做与不做

### 1.1 做

- M3-A：portable ReplayBundle、exact replay、effect-deny simulation、baseline/holdout eval、StrategyEvolutionGovernor、promotion/activation 分离、run pinning、CAS rollback、typed artifacts。
- M3-B：LoopRegistry/LoopSpec、CoordinationRegistry/WorkPattern/role weight、SelectionPolicy、ModelAdaptation、bounded long-horizon run 的受控进化。
- M3-C：StrategyMemory conflict/decay、AgentSelfModel、PartnershipModel、Trust/Delegation recommendation、Proactivity/Communication strategy 与完整 golden/release audit。

### 1.2 不做

- 不改 fixed/constitutional 内核，不让模型改源码、测试、compliance/verifier、identity baseline 或模型权重。
- 不自动扩大 permission/trust/grant/envelope，不做 L5 standing grant。
- 不做多租户/多组织、公网控制面、marketplace、remote executor、多主同步或自主 agent team。
- 不做通用 AutoML、任意策略 DSL、大规模在线实验平台或自动 prompt hill-climbing。

## 2. 全局集成契约

### 2.1 Crate owner

| 事实/行为 | owner | 消费方 |
|---|---|---|
| M3 DTO/EventKind | protocol | 全部 crate |
| authoritative events/active projection/CAS | store | harness/eval/read surfaces |
| ReplayBundle/exact replay/evaluation/artifacts | eval | harness/cognition/gateway |
| strategy candidate semantics/governor/conflict/decay | cognition | harness/coordination |
| pinned LoopSpec execution | loop | harness |
| Coordination/WorkPattern resolution | coordination | harness |
| selection policy resolution | capabilities/models | harness/coordination |
| effect-deny、snapshot bind、impact gate、append activation/rollback | harness | gateway/cli |
| permission/approval/external action | policy/approval/execution/communication | harness |

禁止新增 `eval -> loop/cognition/coordination`、`loop -> store/eval`、`coordination -> store/execution` 或 gateway 直写 store 的边。跨域数据只通过 protocol DTO 和 Harness orchestration 交换。

### 2.2 冻结接缝

- 既有 `EventStore`、`VersionedEventStore`、`EvolutionGovernor`、`LoopDriver`/loop public contract、`CoordinationReasoner`、`OrchestrationLibrary`、`ActionBackend` 签名不改。
- 新能力使用 architecture/06 的 `ReplayEngine`、`EvolutionEvaluator`、`StrategyEvolutionGovernor`、`EvolutionProjection`、`EvolutionEventStore`、`LoopRegistry`、`CoordinationRegistry`、`SelectionPolicyRegistry` companion traits。
- cognition/eval 只产 candidate/report/decision；Harness 决定是否 append promotion/activation/rollback，store 分配 `stream_seq`。
- domain registry 只解析 immutable content ref；不能拥有 active pointer、permission 或 event writer。

### 2.3 事件顺序

Promotion 主链：

```text
CandidateCreated{strategy_candidate}
 -> VerificationStarted/Finished*
 -> EvolutionEvaluationRecorded
 -> CandidatePromoted|Rejected|Downgraded
 -> StrategyActivated (仅满足 impact/owner/CAS 时)
 -> SessionBound{evolution_snapshot} (后续新 run)
```

Rollback 主链：

```text
FailureEvidenceRecorded|RetractionEvent|owner control
 -> ReevaluationTaskCreated (需要时)
 -> EvolutionEvaluationRecorded{regression|unverifiable}
 -> CandidateDowngraded (适用时)
 -> StrategyRolledBack
 -> SessionBound{restored version} (后续新 run)
```

任何 path 都不能从 evaluation 直接写 active projection，不能从 activation 生成 permission/grant。

## 3. M3-A：进化控制面

### A0 Canonical/Protocol

1. 实现 `m3-a-protocol-compatibility.md`：Source Replay/Simulation、M3 DTO、89 EventKinds、payload additive defaults、invalid/legacy contract tests。
2. `EventKind::ALL` 精确为 89；既有前 86 顺序和字符串保持不变。
3. 所有 ID/ref/digest 采用 protocol newtype；fitness measured 使用带 unit 的整数；`HistoricalFalse` 只解码 false。
4. `SessionBoundPayload.evolution_snapshot/effect_mode`、`CandidateCreatedPayload.strategy_candidate`、`DecisionTraceRecordedPayload.evolution_snapshot` 使用 serde default。

### A1 Store projection 与 CAS

- 从 `CandidateCreated/Promoted` 重建 stable strategy catalog；只有 `StrategyActivated/RolledBack` 改 active projection。
- active key = `(EvolutionAggregateRef, StrategyDomain, Scope)`；记录 active version、event id、aggregate version、known-good history 和 spec digest。
- `EvolutionProjection::snapshot` 在一个读快照内解析所有 scope-relevant active refs/aggregate versions，按 aggregate/domain/scope/ref 稳定排序并计算 digest。
- 新增 additive `EvolutionEventStore`；不得把 RunId-bound `VersionedEventStore` 当跨 run strategy aggregate。activation/rollback 在 owner control run 中以显式 EvolutionAggregateRef 做 expected-version CAS，committed=expected+1；mismatch 零 event、零 ledger/projection write。
- projection rebuild 按 aggregate + committed version 连续 fold，不按跨 run timestamp/EventId 排序；禁止永久伪 control run 或非事件 active 真相。
- legacy 86-event store 打开后 active projection 为空，Harness 使用 frozen seed snapshot；不得合成历史 M3 event。

### A2 ReplayEngine 与 Effect-deny Simulation

- ReplayBundle builder 固定 event cursor range、checksum、upcaster implementation identity、policy/loop/model/tool/driver/evolution snapshots、case/holdout digest。
- exact replay 不调用 model/tool/backend/secret resolver；在独立 report 中输出 projection diff 与 completeness。
- Harness simulation 创建独立 run，安装 effect-deny guard。工具/outward proposal 显式 `ActionDenied`，不进入 ActionStarted。
- simulation 使用 deterministic scripted provider/recorded result；任何 live provider 模式必须另标 non-deterministic 且不能作为 sole promotion evidence。

### A3 Evaluation 与 Governor

- Evaluation 先执行 snapshot/case/budget comparability，再 hard-invariant，再逐维 fitness。
- `Pass` 要求 holdout 完整、所有 hard invariant pass、ground truth 非空且 candidate 不低于冻结阈值；缺证据为 Unverifiable。
- LLM judge/self-eval 只能写辅助 metric；不能写 hard-invariant pass 或 owner rubric。
- `StrategyEvolutionGovernor` 检查 fixed/constitutional、provenance、impact、conflict、evidence、evaluation verdict 和 current snapshot，只返回 Reject/Promote/NeedOwner/Downgrade/Rollback decision。

### A4 Promotion、Activation、Run bind、Rollback

- Promotion 复用 Candidate lifecycle；stable spec ref/digest 从 candidate 解析，不能批后替换。
- Harness activation 检查 promoted ref、evaluation digest、domain spec validator、impact、owner control、current policy/permission compatibility 和 expected version。
- `Cautious` 可自动；`Bounded` 首版默认 owner；`Expansive` 必须 owner；`Constitutional` 拒绝。
- Run bind 在 SessionBound 前读取一次 EvolutionSnapshot；无法解析 active spec时 fail closed或显式 rollback，不静默 seed fallback。
- rollback 恢复 known-good，记录 in-flight disposition 与 `external_effects_reverted=false`；外部补救必须新建 ActionIntent。

### A5 Gateway/CLI、Doctor 与 Artifacts

- owner-authenticated read/promote/activate/rollback/pause-auto-activation control；Gateway 不接受 external participant/subagent 冒充 owner。
- ConfigDoctor 覆盖 effect-deny、holdout、owner impact gate、CAS、known-good、artifact root、secret scan。
- typed artifact writer + independent verifier：ReplayBundle、Evaluation、Promotion/Activation、Rollback、portable trace。
- artifact 内容寻址、防 path escape、schema/digest/ref completeness、secret/private-path scan。

### M3-A gate

S53-S57 + S1-S52 + protocol 89 snapshot + 18-crate graph + fmt/check/strict clippy/all tests/compliance/artifact gate。输出 `docs/acceptance/m3-a-acceptance-report.md` 并停下 owner review。

## 4. M3-B：Loop、协调与选择策略

### B0 Compatibility 与 domain validators

- 实现 `m3-b-protocol-compatibility.md`，不新增 EventKind。
- versioned `LoopStrategySpec`、`CoordinationStrategySpec`、`SelectionStrategySpec`、`ModelAdaptationSpec` 有 round-trip、legacy、unknown/zero/overflow 和 forbidden-field tests。
- 每个 spec 明确 allowed fields；出现 policy/permission/approval/audit/L5/fixed identity 字段即拒绝。

### B1 LoopRegistry

- seed v1 永久可解析；candidate spec 仅允许已知 phase/trigger/checkpoint/verification/budget/fallback 组合。
- Harness 从 pinned snapshot resolve LoopSpec 并传入 loop；loop 不读 active store。
- max turns/token/time 硬上限由 runtime config/policy 决定，strategy 只能取更窄值。
- failure/timeout/cancel/approval/verification event contract 不随 LoopSpec 改变。

### B2 CoordinationRegistry/CoordinationFitness

- candidate 调整 applicability signature、single/multi selection、resource/role weights、checkpoint topology；不包含 permission/toolset expansion。
- baseline/candidate 在相同 goal、ResourceInventory、DoneContract、budget 下比较。
- fitness 包含 correctness、verification、cost、latency、over-delegation、handoff/failure；任何越权/不完整 result contract hard fail。
- Orchestrator 仍经 Harness spawn；父预算预留、取消级联、child 无认知/稳定写权保持。

### B3 Selection 与 Model Adaptation

- selection pipeline 固定为 lifecycle/managed policy/permission/scope filter -> strategy rank -> ToolsetResolved/ResourcePlan -> execution-time recheck。
- CapabilityEvidence/FailureEvidence/verification 主导；provider/profile/self confidence 只能降低 ceiling。
- weak model profile 可增加外化步骤、verification/checkpoint；strong profile 可减少非必要结构，但不能去掉高影响 trace/approval/verifier。
- model/tool/backend schema digest 漂移使 active strategy incompatible，进入 fail closed/rollback review。

### B4 Bounded Long-horizon Golden

- 多 checkpoint，每段普通 Harness run + pinned snapshot；active 变化只影响下一 checkpoint。
- foreground priority、AttentionBudget、time/token/cost budget、cancel/revoke、unknown outcome 和 M2 outward action治理全部回归。
- role weight/WorkPattern rollback 后，新 checkpoint 使用 restored ref；旧 artifact 保留原 ref。

### M3-B gate

S58-S62 + M3-A/M2/M1/M0 全回归，domain compatibility/forbidden-field/long-horizon artifact PASS。输出 `m3-b-acceptance-report.md` 并停下 owner review。

实施状态（2026-07-18）：上述 gate 已 PASS 且 owner 已通过，证据见 `docs/acceptance/m3-b-acceptance-report.md`；M3-C 随后按冻结顺序实施。

## 5. M3-C：认知、信任与伙伴策略

### C0 Compatibility

- 实现 `m3-c-protocol-compatibility.md`，不新增 EventKind。
- StrategyMemory/AgentSelf/Partnership/TrustDelegation/Proactivity/Communication spec 使用 M3-A common envelope；domain payload versioned、scoped、content-addressed。
- legacy M0-M2 candidate/self/partnership/trust/proactivity/communication events 可 replay，但不能凭缺失 M3 evaluation/activation 字段合成 active strategy。

### C1 StrategyMemory

- candidate/stable/active/evidence/derived edges从 event投影；Untrusted raw content不得创建结构边。
- evidence freshness、conflict、decay、retraction 只产 reevaluation/downgrade/rollback proposal；不 silent delete。
- additive schema candidate 必须保留旧 reader/replay；breaking schema 不自动 promotion。

### C2 AgentSelf/Partnership

- AgentSelfAttribute 更新读取 verified outcome、CapabilityEvidence、FailureEvidence、owner feedback；self observation 只可压低。
- PartnershipState 更新读取多时间点协作/纠偏证据；短期例外、外部参与者内容和用户盲点不稳定化。
- fixed identity/No-real-consciousness-claim/Loyalty-to-owner 不出现在可变 spec；domain validator hard deny。

### C3 Trust/Delegation/Proactivity/Communication

- verified success 只能形成窄 trust/delegation proposal；owner control 后才调用既有 grant lifecycle。
- failure/revoke 可自动降低 intervention/active recommendation；permission projection 不由 M3 event改变。
- proactivity/communication 可调整 trigger、summary、surface preference、AttentionBudget cost model 和表达 profile；ObservationScope、recipient、DisclosurePolicy、TTL/budget、L3/L5 不可扩大。
- external feedback 仍 Untrusted；只有 Gateway-authenticated owner feedback 可作为 owner evidence。

### C4 End-to-end Golden 与 Release Audit

- 使用 repository-owned M2 real browser/API typed trace，构建 v2 candidate，运行 holdout，owner 激活，真实 Harness run，注入 regression，rollback 到 v1，再跑 v1。
- golden 真实 action完整走 M2 approval/CompetenceGate/plan digest/provenance/verification，不因“实验”豁免。
- release audit 覆盖 tracked/release tree、LICENSE/NOTICE、direct dependency/source/license/security、borrowing records、upstream copy、secret/private paths。

### M3-C/final gate

S63-S69 + M3-A/B/M2/M1/M0 全回归；完整 golden/artifact/release audit PASS。输出 `m3-c-acceptance-report.md` 与 `m3-acceptance-report.md`。

实施状态（2026-07-18）：上述 final gate 已 PASS。完整证据见 `docs/acceptance/m3-c-acceptance-report.md` 与 `docs/acceptance/m3-acceptance-report.md`；`tools/verify-m3.ps1` 是固定复跑入口。

## 6. 错误分类与用户可见结果

| 条件 | FailureTaxonomy/结果 |
|---|---|
| replay snapshot/checksum/migration 不完整 | verification/learning failure；Unverifiable，不 promotion。 |
| simulation effect attempt | safety policy failure；ActionDenied，无 driver call。 |
| self-eval trap/holdout 泄漏 | self_eval_trap；evaluation Fail。 |
| fixed/constitutional candidate | safety policy/learning failure；candidate reject。 |
| CAS conflict | explicit conflict；零写入；重读后由 owner/Harness 重决策。 |
| active spec incompatibility | config/learning failure；run bind fail closed或 rollback review。 |
| runtime regression | FailureEvidence + reevaluation + rollback candidate。 |
| trust/proactivity 过度扩张 | trust/proactivity/safety policy failure；不 activation/authorization。 |
| release/license/secret failure | ComplianceCheckResult blocking；M3 final FAIL。 |

最终用户视图必须区分：候选未评估、评估失败、不可验证、已稳定未激活、已激活、已 supersede、已 rollback、需要 owner、被 governance 拒绝。不能统一显示为“已学习”。

## 7. 测试策略

### 7.1 Protocol/Store

- 89 EventKind exact snapshot、one-to-one payload、legacy first-86 order/string、M2 payload decode。
- M3 DTO round-trip；unknown/zero/non-finite/overflow/invalid refs/`HistoricalFalse=true` fail closed。
- stable/active projection rebuild、CAS conflict zero write、duplicate semantic idempotency、rollback lineage。

### 7.2 Eval/Cognition

- exact replay determinism、snapshot completeness、upcaster identity、history unchanged。
- effect-deny driver/secret call counters = 0。
- hard invariant > aggregate fitness、Pass/Fail/Unverifiable、train/holdout separation、self-eval trap。
- fixed/constitutional reject、不对称 impact、conflict/decay/retraction、ground truth > self assessment。

### 7.3 Harness/Domain

- candidate/promotion/activation/authorization separation；owner provenance；run pinning；in-flight disposition。
- Loop/Coordination/Selection forbidden-field tests；subagent harness spawn；M2 external-action full regression。
- long-horizon foreground/budget/cancel；proactive AttentionBudget；communication disclosure/identity。

### 7.4 Artifacts/Compliance

- portable schema/digest/ref completeness；tamper detection；path escape；secret/value/name/private path scan。
- S22 fixtures继续 9/9；新增 release-tree/license/NOTICE/dependency negative fixtures。
- acceptance report 只接受 verifier 生成的 digest，不信任手写 PASS 字段。

## 8. 验收与提交门

每次提交前：

```text
cargo fmt --all -- --check
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
py -3 -m unittest discover -s tools/tests -v
bash tools/compliance-doctor.sh
```

每波另运行 `tools/verify-m3.ps1` 当前已激活范围。验收报告必须记录 HEAD、命令、S# 精确测试、关键事件序列、反向断言、89-kind/18-crate 结果、artifact digest、secret scan 和已知 residual risk。

## 9. 原创性与依赖

- replay/evolution control plane、fixtures、策略 spec、错误文案和测试全部 repository-owned；不复制参考 Agent 的 prompt、evolution loop、benchmark fixture、目录或命名体系。
- A/B、replay、canary、CAS、rollback 是通用工程概念；forme 的四态分离、事件序列和治理由本项目实现。
- 新第三方依赖必须精确 pin，并更新 `docs/compliance/third-party-dependencies.md` 与 borrowing record；M3-A 优先零新增 runtime dependency。
- final release audit 是工程证据，不宣称替代律师意见。
