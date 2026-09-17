# M0 验收报告

- 验收日期：2026-07-14
- 工程规模：18 crates；164 Rust tests；8 compliance fixtures
- 权威规格：`docs/prd/01-foundation-implementation-prd.md` §7（S1–S22）
- 统一入口：`powershell -NoProfile -ExecutionPolicy Bypass -File tools/verify-m0.ps1`
- 最终结果：**PASS — S1–S22 全绿，原创合规门 PASS**

## 1. 场景证据

下表中的箭头表示测试直接断言的事件顺序；`×N` 表示事件数量也被断言。所有反向断言都在相同场景测试或其同模块契约测试中执行。

| 场景 | 执行证据 | 事件序列断言 | Store / 反向断言 | 结果 |
|---|---|---|---|---|
| S1 CLI/Gateway 同协议 | `forme-harness::s1_final_run_is_event_sourced_and_idempotent` | `RunAccepted → SessionBound → TurnStarted → ContextBuildStarted → ContextBuildFinished → ModelCallStarted → ModelCallDelta → ModelCallFinished → OutputClassified → VerificationStarted → VerificationFinished → TurnComplete → RunComplete` | 重复 idempotency key 不产生第二条 run；online/pure projection 同构 | PASS |
| S2 审批拒绝 | `forme-harness::s2_denied_approval_suspends_then_aborts_without_execution` | `ToolCallProposed → ToolPolicyEvaluated → ApprovalRequested → RunWaiting → ApprovalResolved → ActionDenied → FailureEvidenceRecorded → FailureDigestUpdated → RunAborted` | backend 执行计数为 0；无 `ActionStarted/ActionCompleted` | PASS |
| S3 MCP 治理 | `forme-capabilities::s3_stdio_discovery_allowlist_prepare_call_and_disable_are_governed` + execution MCP tests | `McpDiscovered`; `CapabilityIndexed → ToolsetResolved`; 调用面 `McpCallEvent → ActionCompleted` | 非 allowlist 能力不进 toolset；timeout/schema/server error 分类；禁用后 toolset 为空 | PASS |
| S4 Skill 按需加载 | `forme-capabilities::s4_skill_metadata_is_default_and_only_selected_body_is_loaded` | `SkillMetadataExposed ×2 → SkillBodyLoaded ×1` | 未选 skill 无 body event；untrusted body 加载失败 | PASS |
| S5 Plugin 边界 | `forme-capabilities::s5_manifest_contributions_are_governed_and_disabled_plugins_leave_the_toolset` | `PluginContributionRegistered → PluginToggled → PluginToggled`; `CapabilityIndexed ×3 → ToolsetResolved`; disable 后 `PluginToggled` | disabled 贡献从 toolset 消失；hook 产物仍经 `ToolPolicyEvaluated{deny}` | PASS |
| S6 Coordination 路线 | `forme-harness::s6_coordination_events_are_real_plan_outputs_with_the_decision_workspace_snapshot` | `RunAccepted → SessionBound → GoalFramed → ResourcePlanned → DoneContractSet → AutonomyEnvelopeSet → DecisionTraceRecorded → … → RunComplete` | trace 绑定决策时 `AgentWorkspaceSnapshotRef`；blocked goal 不伪称有路线 | PASS |
| S7 主动最小闭环 | `forme-harness::s7_authorized_tick_persists_the_proactive_loop_and_rejection_never_executes` | `ObservationRecorded → OpportunityDetected → ValueGateEvaluated → ImpulseRaised → CompetenceGateEvaluated → DecisionTraceRecorded → ProactiveProposalEmitted → ProactiveProposalResolved` | reject 后无 `ActionPlanned/ActionStarted`；未授权 observation 不进 event stream | PASS |
| S8 UserModel 候选优先 | `forme-cognition::s8_user_model_is_temporal_candidate_first_and_history_is_bootstrap_only` | `ImportedHistoricalEvidenceRecorded → UserAttributeCandidateCreated ×2 → CandidatePromoted → RetractionEvent`（关键位置顺序） | 单次/导入历史不写 stable；owner 确认后才 promotion；retraction 后查询为空 | PASS |
| S9 Reflection 候选 | `forme-cognition::s9_reflection_creates_low_confidence_candidate_without_mutating_stable_map` | `ReflectionProduced → CognitiveMapUpdateProposed → CandidateCreated → CandidateRejected` | stable map 保持为空；拒绝不提升 | PASS |
| S10 FailureEvidence | `forme-eval::tests::s10_failure_ledger_updates_digest_and_final_pass_does_not_hide_history` | `(FailureEvidenceRecorded → FailureDigestUpdated) ×3 → VerificationFinished{pass}` | final pass 后 3 条失败仍在 trace/digest 中 | PASS |
| S11 Subagent 受限委派 | `forme-harness::s11_subagent_is_spawned_by_harness_with_fresh_context_and_scoped_denial` | parent：`SubagentSpawned → SubagentResultReturned → DecisionTraceRecorded`; child：`RunAccepted → SessionBound → ContextBuildStarted → ContextBuildFinished → ToolCallProposed → ToolPolicyEvaluated → ActionDenied → FailureEvidenceRecorded` | child fresh workspace/空 toolset；child 无 `ActionStarted/MemoryNodeAppended`；result 只写 parent 流 | PASS |
| S12 标准化入口 | `forme-communication::tests::s12_text_adapter_only_normalizes_and_defaults_untrusted` + gateway S12 contract | adapter 产 `CommunicationEventReceived`; gateway 再进入标准 `RunAccepted` 路径 | adapter 默认 untrusted，不直接调用 loop | PASS |
| S13 外部会话终止 | `forme-communication::tests::s13_external_session_is_bounded_and_terminates_on_round_limit` | `ExternalCommunicationGranted → CommunicationSessionOpened → CommunicationSessionTerminated` | round limit 后 session 状态为 Terminated | PASS |
| S14 DisclosurePolicy | `forme-communication::tests::s14_sensitive_disclosure_is_refused_or_blurred_and_audited` | `ExternalCommunicationGranted → CommunicationSessionOpened → DisclosurePolicyApplied{refuse} → DisclosurePolicyApplied{blur}` | 私密未确认请求拒绝；敏感可答内容降为 blur | PASS |
| S15 代表身份 | `forme-communication::tests::s15_representation_never_claims_owner_identity_and_uncertainty_needs_approval` | `ExternalCommunicationGranted → CommunicationSessionOpened → DisclosurePolicyApplied` | representation 固定为 `AgentRepresentingOwner`，不冒充 owner | PASS |
| S16 Local Device 授权 | `forme-communication::tests::s16_device_requires_active_grant_and_revocation_blocks_new_events` | active grant 时仅产 `CommunicationEventReceived{carrier=hardware}` | 未授权与 revoke 后 normalize 均失败且不产事件 | PASS |
| S17 A2A bounded session | `forme-communication::tests::s17_agent_session_has_hard_purpose_budget_and_termination` | `ExternalCommunicationGranted → CommunicationSessionOpened → CommunicationSessionTerminated{purpose_drift}` | purpose drift 立即终止；预算/TTL/round 均为硬边界 | PASS |
| S18 CompetenceGate 降级 | `forme-harness::s18_competence_downgrade_is_evidence_backed_in_event_and_trace` | `ObservationRecorded → OpportunityDetected → ValueGateEvaluated → ImpulseRaised → CompetenceGateEvaluated → DecisionTraceRecorded → ProactiveProposalEmitted` | 高自评/高 map confidence 不能抬高；FailureEvidence 将 L4 降至 L0；无 action event | PASS |
| S19 并发一致性 | `forme-harness::s19_same_session_is_serial_and_tick_cannot_derail_foreground` | run A `RunComplete` 时间不晚于 run B `TurnStarted`; post-turn tick：`ValueGateEvaluated → ImpulseRaised → CompetenceGateEvaluated → CandidateCreated` | 前台占用时 tick 不运行；后台只产 candidate，不写 stable | PASS |
| S20 撤销/派生失效 | `forme-cognition::s20_retraction_traverses_all_derived_objects_without_deleting_history` | `RetractionEvent → ReevaluationTaskCreated → CandidateCreated ×2` | target 与两级 derived 均 inactive；3 个历史 stable object 仍保留 | PASS |
| S21 Schema/Replay | `forme-store::s21_read_time_upcast_and_replay_leave_authoritative_history_unchanged` | 权威流始终只有原 `ConfigDoctorReport(stream_seq=1)`；读时按 `v1 → v2 → v3` upcast | 缺 migration edge 显式失败；raw row 字节不变；replay 不覆盖历史 | PASS |
| S22 原创 CI/Doctor | `tools/tests/test_compliance_doctor.py` 8 个 fixture + 真实 `research/upstreams` 扫描 | 检查顺序：`upstream → license → copy`，三项均 PASS | import/include、GPL/AGPL、未知 vendor、exact/normalized/long/token/naming/directory copy 均有 blocking fixture；窄路径+SHA 记录才可放行 | PASS |

## 2. prd/12 收口证据

- `GoalFrame`、`SituationModel`、`ResourceInventory` 约束进入 `ResourcePlan` 与可执行 `DoneContract`。
- `DecisionTrace` 记录决策时的 `AgentWorkspaceSnapshotRef`，不是事后伪造引用。
- 三个 M0 seed orchestration pattern 与六维 applicability signature 已落地。
- route DAG 节点运行语义覆盖 `Pending → Running → Done/Failed/Cancelled/OutcomeUnknown`，并实现 abort/continue/replan、预算原子预留回收、父取消级联、intent 去重和未知副作用不盲重试。
- subagent 由 harness 创建独立 child run，收窄 session/workspace/model/toolset/scope/envelope/budget；父流只接 summary/result 与 trace。

## 3. prd/13 收口证据

- timer tick 提供 M0 简单启发地板：Gap、Change、Tension、Association、Pressure。
- Commitment 只经 `IntentionStore::claim_due` 确定触发，claim 后完成 trigger，避免 lease 到期重复打扰。
- ValueGate 与 EvidenceCompetenceGate 由结果/验证证据主导；MapConfidence、AgentSelf、Trust 只能压低或限制，FailureEvidence 主动降级。
- AttentionBudget 覆盖 quiet hours、interrupt rate、urgency 与 hitchhike 不计打断预算。
- AskToLearn、evidence-bound DelegationProposal（需确认、零副作用）、defer intention、reject suppression 均有测试。
- AgentWorkspace 是有容量上限、按 value × urgency 排序、只从事件重建的投影，并注入协调后的 brain context。

## 4. prd/17 合规深化证据

- upstream build/import/include 隔离。
- GPL/AGPL 与未知 vendor/third-party 默认阻断。
- exact SHA-256、规范化全文、长表达、20-token 连续序列、distinctive naming overlap、目录结构镜像检测。
- borrowing record 强制字段、主工程相对路径与 SHA-256 双绑定；copyleft 需显式 review/notice。
- Windows 下从 WSL 调用 doctor 时切换宿主 Python，仅优化 NTFS 扫描 I/O，不缩小扫描范围或放宽门槛。

## 5. 最终门结果

统一验收脚本的最终输出：

```text
[S1] PASS
...
[S22] PASS
[PASS] upstream
[PASS] license
[PASS] copy
compliance-doctor: PASS
M0 ACCEPTANCE: PASS (S1-S22 + compliance)
```

同时通过：

```text
cargo fmt --all -- --check
cargo check --workspace
cargo test --workspace --all-targets
py -3 -m unittest discover -s tools/tests -v
bash tools/compliance-doctor.sh
```

结论：**M0 完成。**
