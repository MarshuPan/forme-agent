# M2-B 验收报告

- 验收日期：2026-07-17（独立复核加固后重验）
- 工程规模：18 crates；240 个非忽略 Rust tests；1 个真实浏览器 test 由验收脚本单独实跑；9 compliance fixtures
- 权威规格：`docs/requirements/08-m2-verification-strategy.md` S43-S47；`docs/prd/19-m2-program-prd.md` §5
- 统一入口：`powershell -NoProfile -ExecutionPolicy Bypass -File tools/verify-m2.ps1`
- 最终结果：**M2-B 验收 PASS - S43-S47、S1-S42、真实浏览器/API golden 与原创合规门全绿**

首版 PASS 后的独立复核发现 4 类测试覆盖假阳性：model/loop 未证明 untrusted envelope 跨 turn 保留；Harness ingress 可由测试直接自报且 disclosure 未精确绑定；external grant 缺 owner 认证；device retention 只查内存标志、未查持久化。上述缺口均已先改冻结文档、再修实现和负向测试，并由统一门重新验收。本报告只封版 M2-B connector 与真实沟通。M2-C ResourceGraph、长期目标、能力成长、managed plugin policy、hot/cold memory 与单 peer sync 仍保持冻结；未经 owner review 不进入下一波。M2 整体尚未完成。

## 1. 场景证据

下表只接受事件序列、provider transition、外部地面真值和反向断言作为证据；driver 返回值或最终文案不单独计作通过。

| 场景 | 执行证据 | 事件序列断言 | 权威状态与反向断言 | 结果 |
|---|---|---|---|---|
| S42 外部输入治理回归 | models `s42_untrusted_model_message_preserves_source_trust_and_data_treatment`；loop `s42_untrusted_input_and_tool_result_stay_data_in_each_model_request`；Harness `s42_model_cannot_spoof_user_turn_source_from_untrusted_communication` | 未信任 communication 产 action 时停在 `ToolCallProposed -> ToolPolicyEvaluated{ask,untrusted-ingress-floor} -> ApprovalRequested -> RunWaiting`，无 `ActionStarted` | 初始外部输入与外部 tool result 在下一次 `ModelRequest` 中保持原 source、`Untrusted` trust 和 `UntrustedData` treatment；伪造 `ActionIntent.source=UserTurn` 被绑定回真实 ingress source；把 Untrusted 构造成 instruction 无法通过 request validation | PASS |
| S43 AppApi connector | `app_api_connector_contract` 两项；`m2_b_policy_contract`；`m2_b_app_api_contract` 两项；M2-B protocol contract | lifecycle transition 精确为 `Discovered -> Configured -> Enabled -> Trusted(ApprovedSource)`；active index 为 `CapabilityIndexed -> ToolsetResolved`；真实成功为 `ActionStarted -> ActionOutputDelta -> ActionCompleted -> CapabilityEvidenceRecorded`；未知 mutation 为 `ActionStarted -> ActionOutcomeUnknown -> CapabilityEvidenceRecorded` | repository-owned loopback API 收到真实 read 与 mutation；schema/endpoint/credential/participant/representation/timeout/method 在 policy 与 execution-time capability recheck 中全量匹配。disabled/untrusted/revoked 不可见；rate limit 阻断第二次请求；断连及发送后不可读响应各只产生一次请求且不重试；response receipt 为 `Untrusted`；content ref 注册后不可覆盖 | PASS |
| S44 真实 External CommunicationSession | `s44_real_delivery_reserves_bounded_session_budget_before_transport`；Gateway `s44_external_session_requires_owner_auth_and_records_owner_grant_provenance`；真实 golden `s44_s45_real_loopback_delivery_stays_inside_disclosure_and_harness_governance` | session 开始为 `ExternalCommunicationGranted{Owner/OwnerInput} -> CommunicationSessionOpened{System/VerifiedProcess}`；真实发送 run 精确为 `DisclosurePolicyApplied -> RunAccepted -> SessionBound -> ToolCallProposed -> ToolPolicyEvaluated -> ApprovalRequested -> RunWaiting -> ApprovalResolved -> RunResumed -> ToolPolicyEvaluated -> CompetenceGateEvaluated -> ActionPlanned -> ActionStarted -> ActionOutputDelta -> ActionCompleted -> CapabilityEvidenceRecorded -> VerificationStarted -> VerificationFinished -> RunComplete` | 非 owner `AuthContext` 在创建 grant 和任何 fabric event 前被拒；owner 审批前真实 delivery count=`0`，one-shot plan-bound 审批后严格为 `1`；真实 request body、participant 和 `agent` representation 与 approved plan 一致。第二条消息越界后 delivery count 保持 `1` | PASS |
| S45 Disclosure 与代表身份 | communication representation/one-shot contracts；Harness `s45_harness_requires_exact_allowed_disclosure_bound_into_the_action_plan`；S44/S45 真实 golden | 正确 disclosure 绑定后、执行前精确停在 `DisclosurePolicyApplied -> RunAccepted -> SessionBound -> ToolCallProposed -> ToolPolicyEvaluated -> ApprovalRequested -> RunWaiting`；完整 golden 中 disclosure 严格位于 `ActionPlanned/ActionStarted` 之前 | additive `DisclosureBinding` 精确绑定 request、session、participant、purpose、content ref 与 policy inputs。缺 binding、错 Harness authority stamp、替换任一绑定字段、`Refuse`、错误 representation/trust 均在 run/action 前拒绝；同 session 的 request ref 原子消费一次，换正文重放不产生第二个 disclosure/action；legacy `None` 只可 replay | PASS |
| S46 Device/Observation adapter | communication adapter contract；Gateway retention contract；Harness `s46_no_raw_device_text_reaches_events_transcript_or_fts` | 合法 Gateway 入站前缀为 `CommunicationEventReceived -> ObservationRecorded`，两者 provenance 均为 `Communication/System/Untrusted`；owner 撤销 audit 精确为 `RevocationEvent{Owner/OwnerInput}` | 原始 observation marker 不出现在 `AuthorizedDeviceObservation` debug、`RunAccepted`、事件序列化、transcript projection 或启用后的 FTS；retention TTL 到期时在 Harness 提交前拒绝且无 request/event；identity、modality、scope、foreground、owner revocation 继续 fail closed | PASS |
| S47 bounded A2A | communication `s47_external_agent_session_uses_the_external_membrane_and_hard_termination`；Gateway `s47_gateway_requires_mutual_channel_and_stamps_external_agent_untrusted` | purpose drift 精确为 `DisclosurePolicyApplied -> CommunicationSessionTerminated`；合法 external Agent ingress 首事件为 `CommunicationEventReceived`，provenance 为 `External(peer)/Untrusted` | 普通 token 不能进入 external Agent 通道，只有 identity 匹配的 mutual-channel 可入站；仍不获得 owner authority。purpose drift 在 transport 前终止；内部 subagent 身份不能复用外部 A2A 入口；无无限轮次、跨 scope disclosure 或 stable 写入 | PASS |

## 2. 冻结契约与治理边界

- `EventKind::ALL.len()` 保持 86；M2-B 只 additive 追加 `BackendKind::AppApi`、`ProviderKind::AppApi`、`ConfigCheck::Connector`、`ActionParameters::AppApi` 与 versioned DTO。M2-A 及更早 wire 值不重排、不改义。
- workspace 保持 18 crates，内部依赖边与 M2-A/M1 基线完全相同。AppApi 实现既有 `ActionBackend`；communication adapter 只 normalize/prepare，真实网络副作用只能经 Harness 与 execution。
- 冻结的 `AgentHarness` 未改；`HarnessIngress` 现在提供 instance-bound authority，`IngressEvent` 字段私有且只能由该 authority 盖章。`HarnessActionIngress` 只接受 idempotency key、外部 action、bound envelope 与可信 prelude；`Deliver` 必须带完整 `DisclosureBinding`，legacy `None` 不授权新动作。
- external 默认至少 L3；API mutation 和对外发言均走审批。不可撤销、高影响或 ExternalCommit 继续是 L5 one-shot，standing grant、trust 和 CapabilityEvidence 不能替代该次 owner 审批。
- AppApi connector 默认 Untrusted，只有 configured/enabled/trusted/granted 且未撤销时才可 index/prepare；Harness 在 `ActionStarted` 前重查 policy、capability registry、envelope、CompetenceGate 与 plan digest。
- API/外部参与者/device 内容统一按 `UntrustedData` 处理；source/trust/treatment 贯通到后续 `ModelRequest`，外部 action result 也不升级为 instruction。Harness 把模型 action source 绑定回真实 run source并在 communication 出口至少提升为 Ask。真实 golden 明确断言没有 `CandidatePromoted`、stable memory node/edge 或 cognitive-map 写入。
- mutation 一旦请求终态不可信，只能 `ActionOutcomeUnknown -> RunWaiting`；测试对 transport drop 和 response decode failure 都断言 exactly-once，无 backend 重试。
- credential value 只经 `SecretRef` 在 execution resolver 边界解析；event、debug、portable trace 与验收报告无 resolved value、本机 secret path 或 credential body。header component 另有长度/字符集 fail-closed 检查。
- ConfigDoctor 从 13 行扩为 14 行；Connector row 检查 lifecycle、trust、grant、driver、schema、rate、timeout、SecretRef resolver 与 default ask/L5 one-shot posture。disabled/absent 且未注册为显式 PASS。

## 3. 真实世界证据与离线复核

- M2-B 真实 API golden 使用 repository-owned TCP loopback participant 和真实 HTTP mutation，不使用 mock transport；审批前后 server delivery count 从 `0` 变为 `1`，第二次越界发送保持 `1`。
- 真实 request 的 body、participant 与 representation 由 server 侧捕获并与 plan 断言；API response/action output receipt 由 Harness 盖为 `Untrusted`。
- Windows nonblocking loopback fixture 显式把 accepted socket 切回 blocking，修复了测试服务器首读 `WouldBlock` 的时序抖动；修复后连续 50 次真实 golden 均通过。production unknown-outcome 语义未放松。
- M2-A 的 typed eval `eval:m2-a-real-browser-mutation` 与 portable trace 继续作为 M2 至少一个可离线复核、无密钥的真实世界工件；本波最终门再次实跑真实 Chrome golden，并重过 `verify-m2-a-artifacts.ps1` 四项校验。
- M2-B protocol compatibility 固定在 `docs/architecture/m2-b-protocol-compatibility.md`；旧/缺字段可以读取，但缺 disclosure binding 的 `Deliver` 永远不可执行。
- S46 使用启用 FTS 的真实 `SqliteEventStore` 验证 no-raw retention：原文在 authoritative event、transcript projection 与全文检索中均无命中；同时验证正常 sanitized input 可被 FTS 命中，排除“FTS 未启用”的假阳性。

## 4. 依赖与原创合规

- 新直接依赖精确锁定：`ureq=3.3.0`；既有 `url=2.5.8` 现在也由 capabilities/communication 直接精确引用。workspace contract 同时复核 M2-A 的五个 driver pin。
- `ureq` 只通过公开 blocking HTTP API 使用，并封装在 project-owned `AppApiDriver` 后；connector、adapter、fixture、错误文案和测试均为 repository-owned。
- source、license、copy risk、implementation rule 与 NOTICE action 已写入 `docs/compliance/third-party-dependencies.md` 和 `compliance/borrowing-records.json`。
- 9 个自建 compliance fixtures 与真实树 upstream/license/copy 三门全部 PASS；无 `research/upstreams/` 引用或借用记录兜底式豁免。

## 5. 最终门结果

通过：

```text
cargo fmt --all -- --check
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets            # 240 passed; 1 real-browser golden ignored here
cargo test ... m2_a_browser_golden -- --ignored # 1 real Chrome golden passed
py -3 -m unittest tools.tests.test_compliance_doctor # 9 passed
powershell ... tools/verify-m2-workspace-contract.ps1
powershell ... tools/verify-m2-a-artifacts.ps1
powershell ... tools/verify-m2.ps1               # S1-S47 + new negative gates + real Chrome PASS
bash tools/compliance-doctor.sh
git diff --check
```

`tools/verify-m2.ps1` 先执行固定的 M1 S1-S37 与 M0 S1-S22 回归门，再逐条执行 M2-A S38-S42、M2-B S43-S47、真实 Chrome/API golden、workspace/artifact/strict-clippy 门，最终输出：

```text
M2-B ACCEPTANCE: PASS (S1-S47 + real browser/API golden + artifacts + strict clippy)
```

结论：**M2-B 验收 PASS，等待 owner review。M2-C 未激活，M2 整体未完成。**
