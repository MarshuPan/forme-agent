# M3-A 进化控制面验收报告

- 验收日期：2026-07-17
- 验收范围：M3-A only；M3-B/M3-C 未激活
- 工程规模：18 crates；89 EventKinds；274 个 listed Rust tests（273 个常规 + 1 个真实浏览器 ignored test 单独实跑）；9 compliance fixtures
- 权威规格：`docs/requirements/10-m3-verification-strategy.md` S53-S57；`docs/prd/20-m3-program-prd.md` §3
- 最终结果：**PASS - 等待 owner review**

## 1. S53-S57 逐场景证据

| 场景 | 可执行证据 | 事件序列断言 | 反向断言 | 结果 |
|---|---|---|---|---|
| S53 Portable Exact Replay | `forme-eval::s53_portable_exact_replay_is_deterministic_read_only_and_effect_free`、`s53_portable_replay_rejects_partial_secret_and_private_path_inputs`；`forme-harness::s53_exact_replay_writes_a_separate_effect_free_audit_run` | 独立审计 run 精确为 `RunAccepted{Replay} -> SessionBound{ExactReplay,evolution_snapshot} -> VerificationStarted -> VerificationFinished{Pass} -> RunComplete`；原 source run 事件逐项不变 | partial range、secret marker、credential field、私有绝对路径、live effect mode、checksum/manifest 不一致均拒绝；`effect_calls=0` | PASS |
| S54 Effect-deny Simulation | `forme-harness::s54_simulation_denies_before_backend_planning_or_execution` | 核心子序列为 `RunAccepted{Simulation} -> SessionBound{CounterfactualDeny} -> ToolCallProposed -> ToolPolicyEvaluated -> ActionDenied{simulation_effect_denied} -> VerificationStarted -> VerificationFinished{Pass} -> EvolutionEvaluationRecorded -> RunComplete` | 无 `ActionPlanned/ActionStarted/ActionCompleted/CapabilityEvidenceRecorded`，未注册 backend 不被解析或调用，预期 deny 不写 FailureEvidence/RunAborted | PASS |
| S55 Ground-truth Evaluation | `forme-eval::s55_ground_truth_and_hard_invariants_dominate_self_score_and_cost`，并由 S54 的 verification -> evaluation 事件链落事实 | `VerificationStarted -> VerificationFinished -> EvolutionEvaluationRecorded{baseline,candidate,verdict}`；typed evaluation 保存 train/holdout digest、逐维 metric、hard invariant 与 ground-truth refs | hard failure 不能被成本抵消；无 ground truth、self-eval-only 为 Unverifiable；metric failure 为 Fail；train=holdout 直接拒绝 | PASS |
| S56 Promotion/Activation/Authorization 分离 | `forme-cognition::s56_governor_keeps_promotion_owner_and_rollback_decisions_distinct`、`s56_untrusted_or_stale_baseline_cannot_promote`；`forme-store::s56_strategy_promotion_requires_a_recorded_passing_evaluation`；`forme-harness::s56_cautious_activation_is_automatic_but_never_authorizes`、`s56_s57_candidate_promotion_activation_run_pinning_and_rollback_are_separate` | `CandidateCreated -> EvolutionEvaluationRecorded{Pass} -> CandidatePromoted -> StrategyActivated`；Cautious 无 owner 可激活，Bounded 无 owner 零 `StrategyActivated`、owner-confirmed 后才激活 | missing/Fail evaluation、Untrusted candidate/evaluation、Actor/TrustTier 或 `User`/`Auto` provenance 不匹配、同 StrategyVersionRef 绑定不同 spec 均事务零写入；stale baseline/Constitutional 拒绝；promotion 后 active 仍为旧版本；无 `ApprovalResolved/AutonomyEnvelopeSet/ExternalCommunicationGranted`，permission projection 不变 | PASS |
| S57 Snapshot/CAS/Rollback | `forme-store::s57_promotion_activation_snapshot_cas_and_rollback_remain_separate`、`s57_active_control_cannot_bypass_cas_or_owner_impact_gate`、`s57_legacy_store_has_no_synthetic_active_history`；M3 harness full-chain test | `StrategyActivated{v1->v2} -> SessionBound{old:v1} -> SessionBound{new:v2} -> FailureEvidenceRecorded -> EvolutionEvaluationRecorded{Fail} -> StrategyRolledBack{v2->v1,false} -> SessionBound{restored:v1}` | stale CAS、direct append、伪造 VerifiedProcess actor、owner gate bypass、空 owner confirmation、unrecorded automatic rollback trigger 均零控制写入；旧 run 不热换；历史 v2/失败保留；`external_effects_reverted` wire true 拒绝 | PASS |

S1-S52 不是抽样运行：`tools/verify-m3.ps1` 首先完整调用 `tools/verify-m2.ps1`，因此 M0-M2 场景、真实 Browser/API/communication、M1/M2 typed artifacts、MCP race regression 与既有合规门均为本波永久回归证据。

## 2. 冻结契约与治理边界

- `EventKind::ALL.len() == 89`；前 86 项与 M2 snapshot 严格同序同名，末尾只 additive 增加 `EvolutionEvaluationRecorded`、`StrategyActivated`、`StrategyRolledBack`。
- workspace 保持 18 crates，`tools/verify-m2-workspace-contract.ps1` 证明内部依赖边无新增、无反向；冻结 `EventStore`、`VersionedEventStore`、loop、coordination、ActionBackend trait 未改签名。
- legacy `SessionBound/CandidateCreated/DecisionTraceRecorded` 缺 M3 optional 字段时读为 `None`；legacy store 不合成 active strategy 或历史 activation。
- stable catalog、active projection 与 evolution version ledger 全部从事件重建。activation/rollback 只经显式 `EvolutionAggregateRef` expected-version CAS，append、ledger 与 projection 在同一事务提交。
- `StrategyVersionRef` 是 immutable identity：同一 ref 不能绑定不同 domain/spec，同一 scope 不能出现第二个候选，防止旧 evaluation 给不同内容背书。
- Harness 在 run bind 时读取一次 `EvolutionSnapshot`，同时写入 `SessionBound` 与 `DecisionTraceRecorded`；active pointer 后续变化不影响已绑定 run。
- candidate、stable、active、permission/grant 四态分离。策略事件不产生 approval、permission、DelegationGrant、AutonomyEnvelope 或外部沟通授权。
- counterfactual simulation 在 backend planning 前 deny；exact replay 不调用 model/tool/backend/connector/communication/SecretResolver。
- 自动 rollback 必须引用同一 control run 中已记录的 Failure/Evaluation/Retraction/Revocation trigger；owner 显式 rollback 仍保留 owner provenance 与非空 trigger。
- Gateway/CLI 控制面保持 bearer + same-origin CSRF；owner 首次摄入 candidate 时由 authenticated surface 盖 provenance，后续 evaluate/promote/activate 保留已记录 immutable candidate；external participant/subagent 不能调用 evolution control。

## 3. 真实浏览器 Golden 与 Typed Artifacts

- 真实 run：`run:1784266949195980100:1`
- 原始 Browser Harness 事件：35 条；`stream_seq=1..35` 连续
- 完整 M3 lineage：42 条，额外 7 个控制事实为 `CandidateCreated -> EvolutionEvaluationRecorded{Pass} -> CandidatePromoted -> StrategyActivated -> FailureEvidenceRecorded -> EvolutionEvaluationRecorded{Fail} -> StrategyRolledBack`
- 激活：`loop:m3-a-browser:v1 -> loop:m3-a-browser:v2`，aggregate `1 -> 2`，Bounded 且 owner-confirmed
- 回滚：`loop:m3-a-browser:v2 -> loop:m3-a-browser:v1`，aggregate `2 -> 3`，trigger=`evidence:m3-a-browser-regression-injected`，`external_effects_reverted=false`
- ReplayBundle 内部 content digest：`fnv64:72f6ae315f7c5aee`；exact replay event count=35、projection match=true、history unchanged=true、effect calls=0

| 工件 | 内容寻址 receipt | SHA-256 文件校验 |
|---|---|---|
| `replay-bundle-e487f7ca74e2c821.json` | `fnv64:e487f7ca74e2c821` | `40760931a302e80025b117b512f56bec73d2caecb10deb76b0bf151235d7b118` |
| `evolution-evaluation-368c79cee355e24c.json` | `fnv64:368c79cee355e24c` | `e27b2c8dcdee36eafb616348e7182a9ecd64b7c9f1d5bc5ff965654045a47cce` |
| `promotion-activation-fcefa8629cb45e6d.json` | `fnv64:fcefa8629cb45e6d` | `be60a23a15ae956ca2763a08ce813f6e2516ad68a87850de325be067e5236a07` |
| `rollback-e3392da83ad3691b.json` | `fnv64:e3392da83ad3691b` | `7a4ba88543b0ab90a34204b88da4df6830546d820052a01c1ae709d6a3b84d6f` |
| `trace-manifest-e985f16324d21011.json` | `fnv64:e985f16324d21011` | `e7b9c639c0ed82d934c46939e2f23574f8402099eff113b16dd43caa7a17e277` |

`forme-m3-artifact-verify` 对五类工件执行 schema、content address、跨工件 lineage、path escape、event/checksum count、封闭五件套目录与 sensitive marker 校验；任何额外文件/子目录都阻断 complete-set。仓库工件和全门临时重生成工件均独立 PASS；未发现 SecretRef identifier、ResolvedSecret、credential、Authorization、raw model delta、tool args、raw DOM/body、endpoint 或 owner 私有绝对路径。

## 4. 最终门

统一入口：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools/verify-m3.ps1
```

该入口依次执行：

```text
S1-S52 permanent regression via tools/verify-m2.ps1
S53-S57 exact scenario contracts and negative cases
89-kind protocol compatibility and legacy decode
real installed Chrome/Edge golden + temporary artifact regeneration
repository-owned five-artifact independent verification
frozen 18-crate graph and dependency pins
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
py -3 -m unittest discover -s tools/tests -v
bash tools/compliance-doctor.sh
git diff --check
```

最终输出：

```text
M3-A FINAL ACCEPTANCE: PASS (S1-S57 + 89 EventKinds + 18 crates + real typed artifacts + compliance)
```

## 5. 残余边界

- M3-A 只交付共同进化控制面；Loop/Coordination/Selection/ModelAdaptation 的可演化 domain spec 属于 M3-B，Self/Partnership/Trust/Proactivity/Communication 属于 M3-C。
- 自动 rollback 首版要求 regression trigger 已在同一 control run 事件化；跨 run 自动 trigger resolution 尚未开放。owner 可显式回滚，但仍不能删除历史或声称撤销外部效果。
- Artifact addressing 沿用仓库现有 deterministic FNV64 checksum 体系，便于稳定复核，但不具备密码学碰撞抗性；报告额外记录 SHA-256 文件校验值。
- 真实 golden 复用 repository-owned loopback Browser 任务，证明真实进程、真实 mutation 和完整 M3 lineage；它不宣称公网、多租户、remote executor 或 release-ready。
- ConfigDoctor 的 ReleaseAudit row 已存在，但完整 LICENSE/NOTICE、依赖安全与 release-tree audit 是 S69/M3 final 完成线，不在 A 波提前宣称。

## 6. 结论

M3-A 的 replay、effect-deny simulation、ground-truth evaluation、candidate/stable/active/authorization 分离、run snapshot pinning、CAS activation、evidence-led rollback、Gateway/CLI owner control 和五类 typed artifacts 已在同一条事件化治理链中工作。S1-S57、89 EventKinds、18-crate 冻结图、真实 Browser golden、strict Clippy 与原创合规门均满足。**M3-A 验收 PASS，停止在 owner review 边界；M3-B/M3-C 未进入实现。**
