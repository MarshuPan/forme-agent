# M3 最终验收报告

- 验收日期：2026-07-19
- 工程规模：18 crates；89 EventKinds；311 个 listed Rust tests（309 个常规 + 2 个真实浏览器 ignored tests 由统一门单独实跑）；19 个 Python gate tests
- 权威规格：`docs/requirements/09-m3-scope-decisions.md`、`10-m3-verification-strategy.md`、`docs/architecture/canonical-contract.md` §25、`docs/prd/20-m3-program-prd.md`
- 统一入口：`powershell -NoProfile -ExecutionPolicy Bypass -File tools/verify-m3.ps1`
- 最终结果：**PASS - S1-S69、真实受治理 evolution golden、typed artifacts 与 release compliance 全绿**

## 1. 验收方法

`tools/verify-m3.ps1` 不是只跑 M3 新测试。它先调用 `tools/verify-m2.ps1`，后者继续调用 M1/M0 固定门；随后逐项执行 M3-A/B/C 精确测试、真实 Browser 工件重生成、独立 artifact verifier、89-kind/18-crate 契约、fmt/check/strict Clippy/workspace tests、Python fixtures、原创 doctor 和 clean-tree release audit。

下表每个 `执行证据` 均由统一门调用。Rust 精确测试的等价命令为 `cargo test -p <package> --test <target> <test> -- --exact`；`::tests::` 项使用对应 crate `--lib` target。事件列只写测试直接断言的关键偏序或可重建状态，不以最终回答文案代替证据。

## 2. S1-S22 M0 永久回归

| 场景 | 执行证据 | 事件或状态序列 | 反向断言 | 证据落点 | 结果 |
|---|---|---|---|---|---|
| S1 CLI/Gateway 同协议 | `forme-harness::s1_final_run_is_event_sourced_and_idempotent` | `RunAccepted -> SessionBound -> TurnStarted -> ContextBuildStarted/Finished -> ModelCallStarted/Finished -> OutputClassified -> VerificationStarted/Finished -> TurnComplete -> RunComplete` | 重复 idempotency key 无第二个 run；入口无私有 loop | `m0-acceptance-report.md` S1 | PASS |
| S2 审批拒绝 | `forme-harness::s2_denied_approval_suspends_then_aborts_without_execution` | `ToolCallProposed -> ToolPolicyEvaluated -> ApprovalRequested -> RunWaiting -> ApprovalResolved -> ActionDenied -> FailureEvidenceRecorded -> FailureDigestUpdated -> RunAborted` | backend calls=0；无 `ActionStarted/Completed` | `m0-acceptance-report.md` S2 | PASS |
| S3 MCP 治理 | `forme-capabilities::s3_stdio_discovery_allowlist_prepare_call_and_disable_are_governed` + execution MCP contracts | `McpDiscovered -> CapabilityIndexed -> ToolsetResolved -> McpCallEvent -> ActionCompleted` | 非 allowlist/disabled 不可见；timeout/schema/server error 分类 | `m0-acceptance-report.md` S3 | PASS |
| S4 Skill 按需 | `forme-capabilities::s4_skill_metadata_is_default_and_only_selected_body_is_loaded` | `SkillMetadataExposed x2 -> SkillBodyLoaded x1` | 未选正文不加载；Untrusted body 拒绝 | `m0-acceptance-report.md` S4 | PASS |
| S5 Plugin 边界 | `forme-capabilities::s5_manifest_contributions_are_governed_and_disabled_plugins_leave_the_toolset` | `PluginContributionRegistered -> PluginToggled -> CapabilityIndexed -> ToolsetResolved` | disabled contribution 消失；hook 仍经 policy deny | `m0-acceptance-report.md` S5 | PASS |
| S6 Coordination 路线 | `forme-harness::s6_coordination_events_are_real_plan_outputs_with_the_decision_workspace_snapshot` | `GoalFramed -> ResourcePlanned -> DoneContractSet -> AutonomyEnvelopeSet -> DecisionTraceRecorded{workspace_snapshot}` | blocked goal 不伪造可执行路线；trace 不事后补 snapshot | `m0-acceptance-report.md` S6 | PASS |
| S7 主动闭环 | `forme-harness::s7_authorized_tick_persists_the_proactive_loop_and_rejection_never_executes` | `ObservationRecorded -> OpportunityDetected -> ValueGateEvaluated -> ImpulseRaised -> CompetenceGateEvaluated -> DecisionTraceRecorded -> ProactiveProposalEmitted/Resolved` | reject 后无 action；未授权观察无事件 | `m0-acceptance-report.md` S7 | PASS |
| S8 UserModel 候选 | `forme-cognition::s8_user_model_is_temporal_candidate_first_and_history_is_bootstrap_only` | `ImportedHistoricalEvidenceRecorded -> UserAttributeCandidateCreated -> CandidatePromoted -> RetractionEvent` | 单次/导入历史不写 stable；仅 owner 可提升 | `m0-acceptance-report.md` S8 | PASS |
| S9 CognitiveMap 候选 | `forme-cognition::s9_reflection_creates_low_confidence_candidate_without_mutating_stable_map` | `ReflectionProduced -> CognitiveMapUpdateProposed -> CandidateCreated -> CandidateRejected` | stable map 为空；reject 不提升 | `m0-acceptance-report.md` S9 | PASS |
| S10 FailureEvidence | `forme-eval::tests::s10_failure_ledger_updates_digest_and_final_pass_does_not_hide_history` | `(FailureEvidenceRecorded -> FailureDigestUpdated) x3 -> VerificationFinished{Pass}` | final pass 不覆盖三条失败 | `m0-acceptance-report.md` S10 | PASS |
| S11 Subagent 受限委派 | `forme-harness::s11_subagent_is_spawned_by_harness_with_fresh_context_and_scoped_denial` | parent `SubagentSpawned -> SubagentResultReturned -> DecisionTraceRecorded`；child `RunAccepted -> SessionBound -> ToolPolicyEvaluated -> ActionDenied` | child fresh/最小 toolset；无 stable write/action bypass；只回 summary/result | `m0-acceptance-report.md` S11 | PASS |
| S12 标准沟通入口 | `forme-communication::tests::s12_text_adapter_only_normalizes_and_defaults_untrusted` + Gateway contract | `CommunicationEventReceived -> RunAccepted` 标准 Harness 链 | adapter 默认 Untrusted，不能直调 loop | `m0-acceptance-report.md` S12 | PASS |
| S13 外部会话终止 | `forme-communication::tests::s13_external_session_is_bounded_and_terminates_on_round_limit` | `ExternalCommunicationGranted -> CommunicationSessionOpened -> CommunicationSessionTerminated` | round/TTL/budget 到界后不可继续 | `m0-acceptance-report.md` S13 | PASS |
| S14 DisclosurePolicy | `forme-communication::tests::s14_sensitive_disclosure_is_refused_or_blurred_and_audited` | `ExternalCommunicationGranted -> CommunicationSessionOpened -> DisclosurePolicyApplied{Refuse/Blur}` | 私密未确认内容不披露 | `m0-acceptance-report.md` S14 | PASS |
| S15 代表身份 | `forme-communication::tests::s15_representation_never_claims_owner_identity_and_uncertainty_needs_approval` | `CommunicationSessionOpened -> DisclosurePolicyApplied{AgentRepresentingOwner}` | 不伪装 owner；不确定高影响表达需审批 | `m0-acceptance-report.md` S15 | PASS |
| S16 Device 授权 | `forme-communication::tests::s16_device_requires_active_grant_and_revocation_blocks_new_events` | active grant 仅产 `CommunicationEventReceived{Hardware}` | 未授权/revoked normalize 失败且零事件 | `m0-acceptance-report.md` S16 | PASS |
| S17 A2A bounded | `forme-communication::tests::s17_agent_session_has_hard_purpose_budget_and_termination` | `ExternalCommunicationGranted -> CommunicationSessionOpened -> CommunicationSessionTerminated{purpose_drift}` | purpose drift、TTL、budget、round 为硬边界 | `m0-acceptance-report.md` S17 | PASS |
| S18 CompetenceGate | `forme-harness::s18_competence_downgrade_is_evidence_backed_in_event_and_trace` | `ValueGateEvaluated -> CompetenceGateEvaluated -> DecisionTraceRecorded -> ProactiveProposalEmitted` | 高自评不抬上限；FailureEvidence 可降 L4 到 L0；无 action | `m0-acceptance-report.md` S18 | PASS |
| S19 并发一致性 | `forme-harness::s19_same_session_is_serial_and_tick_cannot_derail_foreground` | run A `RunComplete` 不晚于 run B `TurnStarted`；后台只 `... -> CandidateCreated` | foreground 占用时 tick 不运行；后台不写 stable | `m0-acceptance-report.md` S19 | PASS |
| S20 撤销/派生失效 | `forme-cognition::s20_retraction_traverses_all_derived_objects_without_deleting_history` | `RetractionEvent -> ReevaluationTaskCreated -> CandidateCreated x2` | target/derived inactive，历史对象不删除 | `m0-acceptance-report.md` S20 | PASS |
| S21 Schema/Replay | `forme-store::s21_read_time_upcast_and_replay_leave_authoritative_history_unchanged` | raw event 保持 `stream_seq=1`；读取 `v1 -> v2 -> v3` upcast | migration 缺边失败；raw bytes 不变；replay 不覆盖历史 | `m0-acceptance-report.md` S21 | PASS |
| S22 原创合规 | `py -3 -m unittest discover -s tools/tests -v` + `bash tools/compliance-doctor.sh` | doctor 固定 `upstream -> license -> copy`；linked worktree 经 common dir 使用同一 upstream corpus；真实树均 PASS | import/include、GPL/AGPL、unknown、全文/token/naming/directory copy 与 corpus 缺失 fixtures 均 blocking；通用 Rust derive 不触发长表达误报 | `m0-acceptance-report.md` §4-5；`m3-c-acceptance-report.md` §4 | PASS |

## 3. S23-S37 M1 永久回归

| 场景 | 执行证据 | 事件或状态序列 | 反向断言 | 证据落点 | 结果 |
|---|---|---|---|---|---|
| S23 CLI/Web 同协议 | `forme-gateway::s23_http_web_surface_uses_real_gateway_harness_and_event_stream` | HTTP `202` 后真实 `RunAccepted -> SessionBound -> Goal/Resource/Done/Envelope/Trace -> Model -> Verification -> RunComplete` | 请求真实经 Gateway/Harness/model/SQLite；无 Web 私有 loop | `m1-a-acceptance-report.md` S23；M1 real-model trace | PASS |
| S24 Stream 重连 | `forme-harness::s24_event_page_reconnect_is_contiguous_deterministic_and_read_only` + SSE E2E | cursor 7 后严格 `8..snapshot_upper_bound`，`stream_seq` 连续 | 同 cursor 返回相同；读取前后权威事件不变；不按时钟排序 | `m1-a-acceptance-report.md` S24 | PASS |
| S25 控制往返 | `forme-harness::s25_gateway_approval_is_one_shot_plan_bound_and_resumes_the_run` + cancel E2E | approval `ApprovalResolved -> RunResumed -> ActionStarted`；cancel `ActionStarted -> ActionCancelled -> RunAborted` | 错 digest 零事件/backend；错误 owner 403；one-shot 只一次 | `m1-a-acceptance-report.md` S25 | PASS |
| S26 Trace Viewer | `forme-harness::s26_trace_view_resolves_failure_and_verification_without_writing_history` | 按 `stream_seq` 折叠 `DecisionTraceRecorded/FailureEvidenceRecorded/VerificationFinished` | read-only，前后 event 相等；不输出 chain-of-thought | `m1-a-acceptance-report.md` S26 | PASS |
| S27 Candidate Review | `forme-harness::s27_candidate_review_compares_state_and_retraction_schedules_reevaluation` | `CandidatePromoted{User} -> RetractionEvent -> ReevaluationTaskCreated` | stale CAS/非 owner/不存在 candidate 零写入；历史不删 | `m1-a-acceptance-report.md` S27 | PASS |
| S28 Manual Eval | `forme-harness::s28_manual_eval_is_repeatable_trace_bound_and_never_promotes_policy` + eval negative | report refs 与完整 run events 一一对应；同 snapshot 重跑相等 | verification fail 不转 success；eval 不增加事件/不 promotion | `m1-a-acceptance-report.md` S28；`m1-real-model-golden-report.json` | PASS |
| S29 到期任务 | `forme-harness::s29_due_background_intention_creates_one_governed_schedule_run` + Jobs E2E | `IntentionCreated -> claim -> fired -> done`；schedule `RunAccepted{Schedule} -> SessionBound -> ... -> RunComplete` | not-due 零 run；缺 capability 拒绝；同 intention 只一个 run | `m1-b-acceptance-report.md` S29 | PASS |
| S30 Lease/恢复 | `forme-harness::s30_restart_reclaims_safe_lease_and_duplicate_intent_never_repeats_delivery`、unknown outcome test | safe lease 可 re-claim；unknown `RunAccepted -> ActionStarted -> ActionOutcomeUnknown -> RunWaiting` | 双 handle 仅一 winner；unknown 不重试；重复 intent 一次 delivery | `m1-b-acceptance-report.md` S30 | PASS |
| S31 前台优先 | `forme-harness::s31_foreground_precedes_schedule_and_budget_or_cancel_blocks_new_actions` | foreground 时 deferred；其 `RunComplete` 后才起 schedule；零预算 `RunLimited` | 不抢占前台；cancel/zero budget 后无 action/notification | `m1-b-acceptance-report.md` S31 | PASS |
| S32 主动 follow-up | `forme-harness::s32_failure_followup_uses_three_gates_attention_budget_and_reject_suppression` + verification cases | `Observation -> Opportunity -> ValueGate -> Impulse -> CompetenceGate -> Trace -> Proposal -> Resolved` | quiet hours hitchhike；Pass 不触发；reject suppression 防重复；无 side effect | `m1-b-acceptance-report.md` S32 | PASS |
| S33 Local Notification | `forme-harness::s33_local_notification_is_plan_bound_and_high_risk_waits_for_approval` + execution plan contract | low risk `Policy -> Planned -> Started -> Completed`；high risk `ApprovalRequested/Resolved -> Started` | target/scope/body/digest 篡改拒绝；合法 one-shot 一次 | `m1-b-acceptance-report.md` S33 | PASS |
| S34 Auto Compaction | `forme-harness::s34_automatic_compaction_preserves_lineage_and_done_contract` + context contract | `CompactionStarted -> CompactionFinished -> ContextBuildFinished` | 无 pressure 零 compaction；clone 验证后才切换；治理 refs/DoneContract 不丢 | `m1-c-acceptance-report.md` S34 | PASS |
| S35 Skill Disclosure | `forme-capabilities::s35_skill_search_is_bounded_explained_and_loads_only_the_selection` + long-context golden | `SkillMetadataExposed{selected} -> SkillBodyLoaded{selected} -> ContextBuild/Compaction` | disabled/untrusted/unselected body 不可加载或进 context | `m1-c-acceptance-report.md` S35 | PASS |
| S36 MCP Dynamic | `forme-harness::s36_mcp_refresh_search_schema_digest_and_execution_recheck_are_governed` + protocol/execution contracts | `McpDiscovered -> CapabilityIndexed -> ToolsetResolved -> ToolCallProposed -> Policy -> Planned -> Started -> McpCallEvent -> Completed` | schema/grant/lifecycle 漂移在 started 前拒绝；legacy no digest 不执行 | `m1-c-acceptance-report.md` S36 | PASS |
| S37 Plugin/Scoped memory | `forme-capabilities::s37_plugin_reload_is_atomic_and_runtime_failure_is_isolated` + memory scoped review | plugin `Registered/Toggled -> Capability/Toolset`；memory `CandidateCreated -> CandidatePromoted{Owner}` | invalid reload 保留旧 generation；failure 隔离；session 不污染 project/global | `m1-c-acceptance-report.md` S37 | PASS |

## 4. S38-S52 M2 永久回归

| 场景 | 执行证据 | 事件或状态序列 | 反向断言 | 证据落点 | 结果 |
|---|---|---|---|---|---|
| S38 BrowserBackend | execution S38 + ignored `s38_real_browser_golden_runs_harness_approval_and_observes_server_mutation` | `ToolCallProposed -> Policy{Ask} -> ApprovalRequested/Resolved -> CompetenceGate -> Planned -> Started -> Output -> Completed -> Evidence -> Verification -> RunComplete` | approval 前 mutation=0，后=1；target drift/origin escape/file URL 拒绝 | `m2-a-acceptance-report.md` S38；M2 Browser report/trace | PASS |
| S39 ComputerBackend | `forme-execution::s39_computer_backend_rejects_out_of_bounds_before_driver_effect` + policy recheck | valid `ActionStarted -> Output -> Completed -> Evidence` | bounds/surface/coordinate 越界在 driver 前拒绝；production 默认不控制 owner 桌面 | `m2-a-acceptance-report.md` S39 | PASS |
| S40 PtyBackend | `forme-execution::s40_pty_backend_uses_real_pty_minimal_env_and_secret_ref` | success `Started -> Output* -> Completed -> Evidence`；timeout/fail/cancel 有各自 terminal | program/root/env 越界拒绝；secret 跨 chunk redaction；失败无 Completed | `m2-a-acceptance-report.md` S40 | PASS |
| S41 外部动作治理 | Harness S41 L3/L4/L5/unknown contracts | 默认/L5 `ApprovalRequested -> Waiting -> Resolved -> Recheck -> Gate -> Planned -> Started`；unknown 只 `ActionOutcomeUnknown -> RunWaiting` | L4 需窄 evidence/envelope；L5 standing grant 拒绝；unknown 不重试 | `m2-a-acceptance-report.md` S41 | PASS |
| S42 Provenance/Injection | Harness/model/loop S42 + artifact scan | external output/evidence 全为 `Untrusted`，下一 ModelRequest 保持 `UntrustedData` | driver 不能自报 Owner；注入正文不生 candidate/stable/graph；artifact 无 secret | `m2-a-acceptance-report.md` S42；`m2-b-acceptance-report.md` S42 | PASS |
| S43 AppApi | AppApi connector/policy/execution contracts | lifecycle `Discovered -> Configured -> Enabled -> Trusted`；run `Started -> Output -> Completed/OutcomeUnknown -> Evidence` | disabled/revoked/rate-limited 不执行；schema/endpoint/SecretRef 绑定；unknown 不重试 | `m2-b-acceptance-report.md` S43 | PASS |
| S44 External Session | communication/Gateway contracts + real loopback delivery | `ExternalGrant -> SessionOpened -> Disclosure -> RunAccepted -> Approval -> Gate -> Action -> Verification -> RunComplete` | 非 owner 无 grant；审批前 delivery=0，后=1；第二条越界仍=1 | `m2-b-acceptance-report.md` S44 | PASS |
| S45 Disclosure binding | communication + `s45_harness_requires_exact_allowed_disclosure_bound_into_the_action_plan` | `DisclosurePolicyApplied -> RunAccepted -> ... -> ApprovalRequested/Waiting`，完整链中 disclosure 早于 plan/action | 缺/错 binding、authority、participant/content/refuse 均在 action 前拒绝；one-shot 防重放 | `m2-b-acceptance-report.md` S45 | PASS |
| S46 Device retention | adapter/Gateway + `s46_no_raw_device_text_reaches_events_transcript_or_fts` | `CommunicationEventReceived -> ObservationRecorded`，provenance `Communication/System/Untrusted` | raw marker 不进 event/transcript/FTS；TTL/revoke/scope 错误零 run | `m2-b-acceptance-report.md` S46 | PASS |
| S47 bounded A2A | `forme-communication::s47_external_agent_session_uses_the_external_membrane_and_hard_termination` + Gateway | purpose drift `DisclosurePolicyApplied -> CommunicationSessionTerminated`；合法 ingress 首个 `CommunicationEventReceived` | mutual channel 也无 owner authority；内部 subagent 不复用外部入口；不写 stable | `m2-b-acceptance-report.md` S47 | PASS |
| S48 ResourceGraph | `forme-coordination::s48_resource_graph_is_event_derived_deterministic_and_never_authorizes_by_score` | 按 `stream_seq` fold capability/evidence；Trace 绑定 `resource_graph_snapshot` | Untrusted delta 不建边；未授权高分资源先被 filter | `m2-c-acceptance-report.md` S48 | PASS |
| S49 长期目标 | `forme-memory::s49_long_term_goal_lineage_rebuilds_and_yields_replans_or_stops_before_action` | `GoalFramed -> IntentionCreated -> RouteCreated -> IntentionResolved{Cancelled}`；checkpoint 可 `Yield/Replan/Stop` | foreground 优先；budget/expiry/revoke/cancel 后零 action；reopen lineage 一致 | `m2-c-acceptance-report.md` S49 | PASS |
| S50 能力成长 | `forme-memory::s50_capability_growth_is_result_led_candidate_only_and_owner_grants_narrowly` | review 前 `CandidateCreated`；owner 后 `CandidateCreated -> CandidatePromoted` | self score 只降；一次成功最高 L2；非 owner 零事件；不改 spec/trust/permission | `m2-c-acceptance-report.md` S50 | PASS |
| S51 Managed Plugin | `forme-capabilities::s51_managed_plugin_policy_verifies_then_switches_atomically_and_revokes_without_ghosts` | activation `PluginContributionRegistered{managed_snapshot}`；revoke `PluginToggled{false}` | 坏签名/replacement 保留旧 generation；managed deny 不被 user 放宽；revoke 无 ghost | `m2-c-acceptance-report.md` S51 | PASS |
| S52 Memory/CAS/Sync | memory hot-cold test + store expected-append/sync/export tests | memory `MemoryNodeAppended* -> RetractionEvent`；sync 在单事务 `compare -> append/projection -> ledger -> cursor` | scope/retention/secret fail closed；stale CAS/peer mismatch 零写；batch collision 全回滚；redacted transfer 不导入 | `m2-c-acceptance-report.md` S52 | PASS |

## 5. S53-S69 M3 场景

| 场景 | 执行证据 | 事件或状态序列 | 反向断言 | 证据落点 | 结果 |
|---|---|---|---|---|---|
| S53 Exact Replay | eval S53 exact/negative + Harness `s53_exact_replay_writes_a_separate_effect_free_audit_run` | audit run `RunAccepted{Replay} -> SessionBound{ExactReplay} -> VerificationStarted/Finished{Pass} -> RunComplete` | partial range、secret/path、checksum/manifest drift、live effect 拒绝；effect calls=0；source history 不变 | `m3-a-acceptance-report.md` S53；M3-A replay/trace artifacts | PASS |
| S54 Simulation | `forme-harness::s54_simulation_denies_before_backend_planning_or_execution` | `RunAccepted{Simulation} -> SessionBound{Deny} -> ToolCallProposed -> Policy -> ActionDenied -> Verification -> EvolutionEvaluationRecorded -> RunComplete` | 无 plan/start/complete/evidence；未注册 backend 不解析；无 secret/driver call | `m3-a-acceptance-report.md` S54 | PASS |
| S55 Ground Truth Eval | `forme-eval::s55_ground_truth_and_hard_invariants_dominate_self_score_and_cost` | `VerificationStarted/Finished -> EvolutionEvaluationRecorded{baseline,candidate,verdict}` | hard fail 不被成本抵消；无 ground truth/self-only=Unverifiable；train=holdout 拒绝 | `m3-a-acceptance-report.md` S55；evaluation artifact | PASS |
| S56 四态分离 | cognition governor/untrusted、store stable gate、Harness cautious/full-chain tests | `CandidateCreated -> Evaluation{Pass} -> CandidatePromoted -> StrategyActivated`；Bounded 需 owner | promotion 不改变 active；activation 不生成 approval/permission/grant/envelope；Untrusted/stale/constitutional 拒绝 | `m3-a-acceptance-report.md` S56；promotion/activation artifact | PASS |
| S57 Snapshot/CAS/Rollback | store CAS/owner/legacy + Harness full-chain | `Activated{v1->v2} -> SessionBound{v1/v2 pinned} -> Failure -> Eval{Fail} -> RolledBack{v2->v1} -> SessionBound{v1}` | stale/direct/伪 owner/无 trigger 零写；不热换；历史保留；不声称外部效果已撤销 | `m3-a-acceptance-report.md` S57；rollback artifact | PASS |
| S58 Loop strategy | loop registry/pinning + M3-B Harness | run 先 `SessionBound{snapshot}`，再按 pinned Loop budget 进入原 Harness 事件链 | immutable seed；same ref/different digest、未知 phase、无限 budget 拒绝；strategy 只收紧 | `m3-b-acceptance-report.md` S58 | PASS |
| S59 Coordination strategy | coordination authority/fitness + Harness child route | `SessionBound -> GoalFramed -> RouteCreated -> SubagentSpawned/Returned -> MemoryNodeAppended -> Verification -> Complete` | 只选已有 role/route；不扩大 child authority；低成本不覆盖不完整/越权 | `m3-b-acceptance-report.md` S59 | PASS |
| S60 Selection policy | capabilities filter/evidence/immutable + Harness | `SessionBound -> ToolsetResolved -> GoalFramed -> ResourcePlanned -> DecisionTraceRecorded{selected refs}` | lifecycle/managed/permission/scope 先过滤；provider self-declare 不抬 ceiling；不注册新能力 | `m3-b-acceptance-report.md` S60 | PASS |
| S61 Model adaptation | models scaffold/evidence/immutable + Harness | adaptation 固定进 model input；同 snapshot 保存到 Trace；verification 按 active spec | effective=min(provider, outcome)；强模型不删高影响治理；不兼容在 SessionBound 前 fail closed | `m3-b-acceptance-report.md` S61 | PASS |
| S62 Long horizon | Harness `s62_long_horizon_checkpoints_yield_to_foreground_pin_versions_and_stop_cleanly` + artifact tests | checkpoint snapshots `v1 -> v2 -> v1`；每段普通 Harness run；唯一 outward run含 `Approval -> Gate -> Action -> Verification` | foreground 不建 run；approval 前 calls=0；active 只影响下一 checkpoint；cancel 后零增长；tamper/secret/path 拒绝 | `m3-b-acceptance-report.md` S62；`fnv64:1325de753c05441a` | PASS |
| S63 StrategyMemory | memory conflict/untrusted/history/invalid 四项精确测试 | `CandidateCreated* -> Conflict -> Promoted -> Activated -> Retraction` 产 reevaluate/downgrade/rollback；落地 `Retraction -> ReevaluationTask -> Downgraded -> RolledBack` | Untrusted 零 lineage/建议；malformed envelope/version fail closed；历史不删 | `m3-c-acceptance-report.md` S63 | PASS |
| S64 AgentSelf | `forme-cognition::s64_agent_self_uses_result_evidence_and_self_assessment_only_lowers` | verified outcome 先得 result ceiling；self assessment 仅 `effective=min(result,self)`；单次失败 NeedMoreEvidence | self-high 不抬升、一次成功不全局化、self 属性不生成 permission | `m3-c-acceptance-report.md` S64 | PASS |
| S65 Partnership | `forme-cognition::s65_partnership_requires_process_timepoints_and_authenticated_owner_correction` | verified collaboration + owner correction 两时间点才 `PromoteCandidate` | short-term/external 保持未决；伪 owner correction 报错；fixed identity 不演化 | `m3-c-acceptance-report.md` S65 | PASS |
| S66 Trust/Delegation | `forme-cognition::s66_trust_success_stops_at_owner_proposal_and_failure_downgrades` | success x3=`OwnerReviewProposal`；owner review 后只 narrow request；failure=`DowngradeAutomatically` | 无自动 grant/trust 提升/L5 standing；空 owner principal 拒绝 | `m3-c-acceptance-report.md` S66 | PASS |
| S67 Proactivity/Communication | cognition S67 proactivity + communication 两项 | 低 regret/cost 且 commitment 完整才 promote；AttentionBudget 降级；communication `Refuse -> NeedActionApproval -> PrepareAuthorized` | missed commitment 自动 downgrade；不扩 observation/recipient/disclosure/outward authority | `m3-c-acceptance-report.md` S67 | PASS |
| S68 End-to-end Golden | ignored real Browser S68 + eval artifact 正/反四项 + independent verifier | `Candidate -> Verify/Eval Pass -> Promote -> Activate v2 -> live governed v2 -> Failure/Eval Fail -> Rollback v1 -> live governed v1` | 缺 phase、隐藏回归、最终 v2、篡改、额外文件、secret/private path 均拒绝；真实 action 三次均走 approval/gate | `m3-c-acceptance-report.md` S68；`fnv64:b4f141e5d930f47e` | PASS |
| S69 Release Compliance | release audit Python tests；`release_audit.py --verify/--compare`；S22 doctor | typed `release_tree -> license_notice -> dependencies -> borrowing -> copy -> artifacts -> vulnerability` 全 Pass；copy evidence 绑定 release tree 与 upstream corpus；clean tree 摘要绑定 canonical Git blobs | dirty/缺 notice/unpinned/未登记/copy/corpus 缺失/secret/path/可达 advisory/scan failure 均 blocking；LF/CRLF checkout 不得改变 receipt | `m3-c-acceptance-report.md` S69；`sha256:fc88e2184e29da666c5fc6373ee08d9829ae127899b901ddbd413364e5ba2c3e` | PASS |

## 6. Typed Artifacts 与真实运行证据

| 里程碑/工件 | 内容寻址与结论 |
|---|---|
| M1 real model | `m1-real-model-golden-report.json` + trace；configured model 真实 Gateway/Harness/Verifier run；18 个 event refs 完整 |
| M2 real Browser | `m2-a-browser-golden-report.json` + trace；35 events；真实 Chrome/Edge mutation=1；Untrusted receipt；无 secret |
| M3-A replay/control 五件套 | `fnv64:e487f7ca74e2c821`、`368c79cee355e24c`、`fcefa8629cb45e6d`、`e3392da83ad3691b`、`e985f16324d21011`；exact replay/eval/promotion/activation/rollback/trace 可独立复核 |
| M3-B long horizon | `fnv64:1325de753c05441a`；checkpoint `v1 -> v2 -> v1`；foreground yield、唯一 outward action、cancel terminal |
| M3-C governed golden | `fnv64:b4f141e5d930f47e`；baseline 35 + live v2 36 + restored v1 36 events；118-event lineage；真实 mutation ordinal 1/2/3；最终 v1 |
| Release audit | `sha256:fc88e2184e29da666c5fc6373ee08d9829ae127899b901ddbd413364e5ba2c3e`；root `sha256:92b83e18617c4631582216671393c554c6314133f65f02c784464b7d07ecae49`；7 typed checks Pass；blocking=false |

所有 portable artifacts 都经过 schema/content-address/lineage/封闭目录/tamper/path escape/sensitive field/value scan。真实 credential 只在 SecretResolver 执行边界出现；事件、DecisionTrace、artifact 和报告不保存明文 credential、Authorization、gateway token、raw DOM/body 或 owner 私有绝对路径。

## 7. 冻结架构与治理复核

- workspace 精确为 18 crates；`tools/verify-m2-workspace-contract.ps1` 复核内部依赖边无新增、无反向，第三方依赖精确 pin。
- `EventKind::ALL.len() == 89`；M2 前 86 项同序同名，M3 只 additive 增加 `EvolutionEvaluationRecorded`、`StrategyActivated`、`StrategyRolledBack`。
- `EventStore`、`VersionedEventStore`、AgentHarness、Loop、Coordination、ActionBackend 冻结签名保持不变；stable/active/version ledger 都从 event 重建。
- Harness-first、执行前 policy/capability/plan digest 重查、CompetenceGate 出口、candidate-before-stable、promotion/activation/authorization 分离、owner provenance、CAS、run pinning、unknown outcome no-retry 全部有正反测试。
- 外部网页/API/参与者/PTY/desktop 内容保持 `UntrustedData`；provenance 由 Harness/Gateway 盖章；外部内容不能改 graph、rubric、active strategy、trust、permission 或 stable cognition。
- strategy rollback 只改变后续 run 的选择，明确 `external_effects_reverted=false`；外部补救仍作为新的 L3/L5 ActionIntent 审批。
- 合规扫描在 linked worktree 与主 worktree 共用同一 upstream corpus；缺失、不可读或 scanner 失败均 fail closed。`upstream_copy.input_digest` 同时绑定 release-tree 与 corpus 摘要，语料变化会改变 typed evidence。tracked release inputs 按 Git index mode + blob identity 摘要，因此同一 clean tree 不受 CRLF/LF checkout filter 影响。

## 8. 工程与发布门

统一门通过：

```text
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
py -3 -m unittest discover -s tools/tests -v
bash tools/compliance-doctor.sh
git diff --check
```

Python 门共 19 项；其中新增 fixture 覆盖 linked-worktree corpus、corpus 缺失 fail closed、通用 Rust derive 误报、corpus 变化驱动 audit evidence 变化，以及同一 canonical Git tree 在 LF/CRLF worktree 间保持 receipt 一致。

`cargo-audit 0.22.2` 发现 `RUSTSEC-2026-0194/0195` 对锁文件中 `quick-xml 0.30.0/0.39.4` 的四个命中。审计器逐项以 `cargo tree --locked --target x86_64-pc-windows-gnu -i package@version` 计算 reachability；四项在当前 release target 均不可达，作为 typed observations 保留。任何当前 target 可达 advisory 仍阻断 final gate。

最终终行：

```text
M3 FINAL ACCEPTANCE: PASS (S1-S69 + 89 EventKinds + 18 crates + governed evolution golden + release compliance)
```

## 9. 残余边界

- M3 只允许非宪法策略的受控进化；不在线修改 source/tests/verifier/fixed identity/model weights，不自动扩大 permission/trust/grant/envelope，不提供 standing L5。
- real-world golden 使用已安装 Chrome/Edge 与 repository-owned loopback fixture，证明真实副作用和地面真值，但不宣称公网、多租户、remote executor、marketplace 或自主 agent team。
- M3-C artifact 的 FNV64 receipt 用于仓库既有 deterministic addressing；报告另记录 SHA-256 文件校验，S69 release artifact 本身使用 SHA-256。
- quick-xml advisory 的不可达结论只对当前 `x86_64-pc-windows-gnu` release target 成立；目标或 feature 变化必须重新审计。
- `research/upstreams/` 是本地 ignored corpus，不进入 release tree；其存在性与内容摘要由合规门单独绑定，不能从 Git clean 状态推断。
- release audit 是工程合规证据，不替代法律意见；本验收不执行 push、merge 或公开发布。

## 10. 结论

M3-A/B/C 已形成第一条完整的 `candidate -> replay/eval -> promotion -> owner-gated activation -> run-pinned real action -> regression -> rollback` 闭环。S1-S69、89 EventKinds、18-crate 冻结图、真实跨系统行动、typed artifacts、strict Clippy、原创门和 clean-tree release audit 全部满足；任何 M0-M2 治理不变量均未削弱。**M3 完成，当前分支达到可交付状态。**
