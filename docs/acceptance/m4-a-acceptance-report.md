# M4-A Peer Trust Plane 与远端执行验收报告

- 验收日期：2026-07-22
- 验收范围：M4-A S70-S74，并永久回归 S1-S69
- 权威规格：`docs/requirements/12-m4-verification-strategy.md` S70-S74；`docs/prd/21-m4-program-prd.md` §3
- 结果：**PASS - 已纳入 M4 最终 owner 验收候选**

## 1. S70-S74 逐场景证据

| 场景 | 可执行证据 | 事件或状态序列断言 | 反向断言 | 结果 |
|---|---|---|---|---|
| S70 Peer enrollment/revoke | `forme-harness::s70_enrollment_binds_real_snapshot_and_stale_cas_or_revoke_fails_closed`；`forme-store::s70_s71_s77_peer_epoch_lease_and_dispatch_claim_are_single_writer` | 注册控制 run 为 `RunAccepted{OwnerControl} -> SessionBound -> FederatedPeerRegistered -> RunComplete`；revoke 追加 `FederatedPeerRevoked` 并推进 authority epoch；新 run 绑定 event-derived federation snapshot | stale expected version、external actor、TOFU、identity/role/scope/TTL 漂移均零写；revoke 后新 session/lease/export 为零 | PASS |
| S71 Governed remote action | Harness `s71_s72_remote_action_dispatches_once_then_recovers_the_original_receipt`、`s71_plan_schema_grant_and_epoch_drift_stop_before_remote_driver`；execution `s71_s73_stale_fence_and_unscoped_credentials_stop_before_the_inner_driver` | `RunAccepted -> SessionBound{federation} -> ToolCallProposed -> Policy -> ActionPlanned{remote} -> ApprovalRequested/Resolved -> CompetenceGateEvaluated -> Lease{Acquired} -> ActionStarted -> ActionCompleted -> Lease{Released} -> Verification{Pass} -> RunComplete` | approval 前 driver=0；plan/schema/grant/epoch/fence/credential scope 任一漂移均在 inner driver 前拒绝；worker 不能自选 operation | PASS |
| S72 Unknown outcome/no retry | Harness S71/S72 与 `s72_s78_s79_s82_authority_restart_preserves_recovery_and_replay_ledgers`；execution `s72_s74_real_mutual_tls_dispatch_is_durable_and_exactly_once_at_the_driver` | dispatch claim 后网络终态丢失为 `Lease{Acquired} -> ActionStarted -> ActionOutcomeUnknown -> RunWaiting`；重连只接受原 receipt/probe，随后 `RunResumed -> ActionCompleted -> Lease{Released} -> Verification{Pass}` | 同 lease durable dispatch count=1；duplicate receipt 幂等；timeout、restart、换 executor 均不重放；旧 epoch/fence receipt 零事实写入 | PASS |
| S73 Secret/untrusted remote content | Harness `s73_mismatched_ground_truth_records_failure_before_unknown_and_never_passes_capability`、`s73_authenticated_remote_injection_and_secret_echo_never_enter_authority_facts`；protocol remote wire negatives | remote sender authentication 与 content trust 分离；不匹配地面真值走 `ActionFailed|ActionOutcomeUnknown -> FailureEvidenceRecorded`，只有 authority verifier 可形成 terminal evidence | SecretRef 只在 executor-local resolver 边界解析；prompt injection、secret echo、伪 owner command、自报 ground truth 不进入 policy/graph/strategy/grant/CapabilityEvidence；工件 secret matches=0 | PASS |
| S74 Real mTLS golden | ignored exact test `s74_s83_three_process_tls_unknown_recovery_replication_revoke_and_artifacts`；execution mTLS test；Gateway `s74_gateway_remote_action_requires_owner_auth_and_routes_only_through_harness`；独立 `forme-m4-artifact-verify` | 独立 authority、`forme-executord` 与 mutation fixture 经 mutual TLS 完成一次动作；事件包含 approval、gate、lease、unknown/recovery、verification；authority 分配连续序列 | driver_calls=1、mutation ordinal=1；无同进程执行 shortcut、匿名 socket、第二 dispatch、手写 PASS、额外文件、tamper/path escape | PASS |

## 2. 冻结契约与安全边界

- `EventKind::ALL.len() == 93`，前 89 项与 M3 snapshot 同序同名；M4 只末尾追加 4 项。
- workspace 仍为 18 crates，没有新增或反向内部依赖边。`RemoteExecutorBackend` 复用冻结 `ActionBackend`，transport/lease 走 companion traits。
- authority 是唯一 Harness、EventStore writer 与 `stream_seq` 分配者；executor 没有 loop、policy、strategy、stable memory 或 permission owner 身份。
- one-shot lease 同时绑定 intent、plan digest、peer/grant、authority epoch、fence、expiry 与 typed operation；worker receipt 仍是 UntrustedData，不能自报完成事实。
- 新 TLS/HTTP 依赖精确 pin；`NOTICE`、第三方依赖表和 borrowing records 已登记。每 crate 保持 `#![forbid(unsafe_code)]`。

## 3. Golden 与工件

仓库闭合五件套位于 `docs/acceptance/m4-artifacts/`，独立校验 receipt 为：

`sha256:c24a84ef85d918654258a3108a5bf8bd5523361271e76dfdad529ce106319022`

其中 remote receipt 绑定 plan/operation/lease/executor 与独立 ground truth；golden report 记录 `authority_driver_calls=1`、`mutation_ordinal=1`、`secret_scan_matches=0`。工件不含 credential、SecretRef id、私钥、private endpoint、host path、raw stdout/body。

## 4. 工程门

M4 最终门统一复跑 348 个非忽略 Rust tests，并显式执行 3 个 ignored real-process goldens；M4-A 自身 protocol/store/execution/harness/gateway/eval 正反契约全部精确运行。`fmt`、workspace check、strict Clippy、Python gates、原创合规、workspace graph 与 `git diff --check` 均为阻断门。

## 5. 结论

M4-A 已证明远端真实动作没有形成新的执行旁路：peer enrollment、owner approval、CompetenceGate、plan-bound lease/fence、unknown-outcome recovery、authority verification 和事件落盘仍在一个权威 Harness 咽喉点内。**M4-A 自验收 PASS。**
