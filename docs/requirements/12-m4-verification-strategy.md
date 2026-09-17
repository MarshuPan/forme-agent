# M4 验证与验收策略

本文承接 `11-m4-scope-decisions.md`，把单 Owner 联邦运行时转成可执行验收。S1-S69 永久证明 M0-M3 没有回退；S70-S84 证明 peer identity、远端执行、lease/fencing、复制、跨设备控制和 checkpoint handoff 在同一条权威 Harness/EventStore 链中工作。

本文已随 M4 规格 owner review 冻结；S70-S84 是历史 M4 实现与最终 owner 验收的固定增量门，M4 工程结果已合入 main。

## 1. 验收原则

- **验证 authority，不只验证连接**：每个正向场景必须断言谁能提交 command、谁能 append event、谁分配 `stream_seq`；socket connected 不是通过证据。
- **事件序列 + 双端计数 + 地面真值**：remote action 同时断言 authority event order、worker driver calls、fixture mutation 和 replica cursor；最终回答不算证据。
- **网络失败必须成一等场景**：partition、duplicate、reorder、stale epoch、expired lease、lost receipt 和 revoke race 都要有反向断言。
- **认证不提升内容信任**：peer identity 可验证，但远端输出仍 `UntrustedData`；prompt-injection fixture 不得改变 policy、graph、rubric 或 active strategy。
- **副作用不假装 exactly-once**：通过 intent/plan/lease/receipt/idempotency/probe 证明未重复；不能仅凭重试库或 timeout 推断。
- **历史 artifact 与当前 release 分开**：M3 receipt 必须继续可独立 verify；M4 当前树生成 M4 receipt，不能拿旧 receipt 做错误的 current-tree comparison。

## 2. 全局回归门

每次 M4 提交前必须执行：

```text
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
py -3 -m unittest discover -s tools/tests -v
bash tools/compliance-doctor.sh
```

`tools/verify-m4.ps1` 必须包含：

1. S1-S69 的精确 scenario/protocol/artifact 回归，不把历史 M3 clean-tree digest 当当前 M4 tree digest。
2. S70-S84 当前已激活波次的正反测试。
3. 前 89 EventKinds 严格 prefix + 当前 93-kind snapshot。
4. 18-crate graph 与精确 dependency pins。
5. M4 real network golden 生成、独立 verifier 和 repository artifact verifier。
6. fmt/check/strict clippy/workspace tests/Python/compliance/current-tree release audit。

## 3. S70-S84 场景矩阵

| 场景 | 证明目标 | 主责波次 |
|---|---|---|
| S70 | peer 注册只由 authenticated owner 发起，identity/role/scope/TTL/epoch 固定且 revoke fail closed | M4-A |
| S71 | RemoteExecutorBackend 真实动作完整经过 plan/approval/gate/lease/fence/verification | M4-A |
| S72 | partition/timeout 进入 outcome unknown，同 lease 只 probe，不重复执行 | M4-A |
| S73 | executor-local SecretRef 与 remote output trust 边界不泄漏、不被 prompt injection 改写 | M4-A |
| S74 | 双进程真实网络 golden 产生一次真实 mutation 和可离线 receipt/trace | M4-A |
| S75 | multi-peer replication 仅从 authority 输出逐 aggregate 连续、过滤、redacted、content-addressed batch | M4-B |
| S76 | offline resume、重复/乱序 batch、per-peer/per-aggregate cursor CAS 保持连续且幂等 | M4-B |
| S77 | authority epoch 与 fence 阻断 stale peer、stale lease 和非 authority append | M4-B |
| S78 | peer revoke 停止新 export/lease，远端删除只在 receipt 可证时成立 | M4-B |
| S79 | 跨设备 owner approval/cancel 仍 one-shot、plan-bound、nonce/expiry-bound | M4-B |
| S80 | placement 只在已授权健康 executor 集中排序，score/策略不产生授权 | M4-C |
| S81 | long-horizon handoff 只发生于 verified checkpoint，每段重新绑定 snapshot/lease | M4-C |
| S82 | 多设备 tick/前台/AttentionBudget/cancel 由 authority 单一裁决且不重复任务 | M4-C |
| S83 | 三进程 federated golden 完成 approve -> remote action -> replicate -> partition/recover -> revoke | M4-C |
| S84 | federation threat/release audit 对 impersonation/replay/secret/corpus/artifact/current-tree 漂移 fail closed | M4-C |

## 4. 逐场景规格

### S70 Peer Enrollment 与 Revocation

- **Fixtures**：owner control session；预置 transport identity digest；Executor/Replica 两种 role；过期、错 fingerprint、external participant、TOFU 请求。
- **触发**：owner 创建窄 peer grant，peer 用匹配身份连接；随后 owner revoke；再尝试 reconnect、export 和 lease。
- **事件**：`RunAccepted{OwnerControl} -> SessionBound -> FederatedPeerRegistered{peer,roles,scope,ttl,epoch} -> RunComplete`；撤销链为 `RunAccepted{OwnerControl} -> FederatedPeerRevoked{peer,new_epoch,in_flight} -> RunComplete`。
- **Store/结果**：peer registry 从事件重建；role/scope/capability/expiry/identity digest/authority epoch 完整；revoke 后新 session/export/lease 均为零。
- **反向断言**：external/subagent/worker 自注册、fingerprint drift、TOFU、role expansion、TTL=unbounded、stale expected version 均零写入。

### S71 Governed Remote Action

- **Fixtures**：registered executor；repository-owned mutation fixture；L3 action；owner approval；recording remote driver；policy/plan/schema/epoch drift 反例。
- **触发**：普通 run 选择 RemoteExecutorBackend，经审批后 dispatch 并验证地面真值。
- **事件**：`RunAccepted -> SessionBound{federation_snapshot} -> ToolCallProposed -> ToolPolicyEvaluated -> ActionPlanned{remote,plan_digest} -> ApprovalRequested -> RunWaiting -> ApprovalResolved -> RunResumed -> CompetenceGateEvaluated -> RemoteExecutionLeaseChanged{Acquired} -> ActionStarted -> ActionCompleted{remote_receipt_ref} -> RemoteExecutionLeaseChanged{Released} -> VerificationStarted -> VerificationFinished{Pass} -> RunComplete`。
- **Store/结果**：driver_calls=1、mutation=1；lease 绑定 intent/plan/executor/grant/epoch/fence/expiry；receipt 绑定相同字段和 ground-truth ref。
- **反向断言**：approval 前 calls=0；plan/schema/grant/epoch drift、expired/revoked peer、wrong fence、worker 自选 operation 均在 driver 前拒绝。

### S72 Partition、Unknown Outcome 与 Fencing

- **Fixtures**：dispatch 后丢连接；worker 实际执行/未执行两支；重复 dispatch/receipt；stale worker；可证明 read-only probe。
- **触发**：authority 收不到终态，重连后分别提交原 receipt、probe 结果或无法裁决。
- **事件**：`... -> RemoteExecutionLeaseChanged{Acquired} -> ActionStarted -> ActionOutcomeUnknown -> RunWaiting`；可证明恢复追加 `ActionCompleted|ActionFailed -> VerificationFinished -> RemoteExecutionLeaseChanged{Released}`，否则保持 waiting/manual review。
- **Store/结果**：同 lease dispatch_count=1；duplicate receipt 幂等；stale fence write=0；没有证据时 outcome 保持 unknown。
- **反向断言**：lease timeout 自动 retry、换 executor 重放、unknown 记 failed/success、旧 epoch receipt 覆盖新事实均失败。

### S73 Secret 与 Remote Content Trust

- **Fixtures**：executor-local scoped SecretRef；中央无 credential material；remote stdout/page/API 中含 prompt injection、secret echo 和伪 owner command。
- **触发**：执行合法 read/mutation，分别让 worker 回显 secret、输出恶意指令或自报 trusted ground truth。
- **事件**：合法路径的 `ActionOutputDelta/ActionCompleted` provenance 保持 remote/authenticated sender + `UntrustedData` content；异常路径追加 `ActionFailed|ActionOutcomeUnknown -> FailureEvidenceRecorded`。
- **Store/结果**：secret resolver 只在 executor 边界调用；event/log/trace/replication/artifact 的 secret marker 和 SecretRef id 计数为零；Verifier 只接受独立 ground truth。
- **反向断言**：central lease 携带 key、worker debug 泄漏、authenticated content 改 policy/rubric/active strategy/graph、remote self-report 形成 CapabilityEvidence 均失败。

### S74 M4-A 双进程 Golden

- **Fixtures**：独立 authority 与 executor 进程；真实 TLS loopback authenticated transport；repository-owned state server；one-shot approval；portable exporter。
- **触发**：从 Gateway 提交一个 remote mutation，执行后断开并用 receipt 完成核验。
- **事件**：S71 全链，必要时包含 S72 unknown/recovery；`stream_seq` 由 authority 连续分配。
- **Store/结果**：真实 server mutation ordinal 精确为 1；worker driver_calls=1；typed peer/lease/receipt/trace 工件内容寻址、无 secret/private endpoint、可离线验证。
- **反向断言**：mock-only、同进程 shortcut、未认证 socket、手写 PASS、额外 artifact 文件、tamper/path escape 均不计通过。

### S75 Filtered Multi-peer Replication

- **Fixtures**：authority store；两个静态 Replica peers；两个 aggregate；不同 workspace/channel scope；secret/private/raw content；redaction profiles。
- **触发**：分别导出增量 batch 并在 peer 应用到只读 replica projection。
- **事件**：authority 追加 `ReplicationCheckpointAdvanced{peer,aggregate,from,to,batch_digest,redaction,epoch,committed_version}`；batch 内 event order 仍以该 aggregate 原 `stream_seq` 为权威。
- **Store/结果**：每个 peer 只收到 scope 允许的 typed envelope/ref；cursor key 为 `(peer,aggregate)`；batch content-addressed；secret/raw sensitive value/private path 不离开 authority；peer projection 可丢弃重建。
- **反向断言**：跨 workspace/channel 泄漏、peer-to-peer 转发、replica 生成权威 event、按 timestamp 重排、redaction 后仍保留敏感 ref 均失败。

### S76 Offline Resume、Idempotency 与 Cursor CAS

- **Fixtures**：断线 peer；连续 batch A/B；B-before-A、duplicate A、tampered digest、stale expected cursor、crash before/after commit。
- **触发**：按各类顺序 apply 并重连续传。
- **事件**：只有完整 `(peer,aggregate)` cursor CAS commit 才追加/投影新的 `ReplicationCheckpointAdvanced`；duplicate semantic batch 不增加 checkpoint。
- **Store/结果**：cursor 单调且无 gap；batch apply/projection/cursor 在一个事务；重复/崩溃恢复幂等；tamper/stale 返回 typed conflict。
- **反向断言**：last-write-wins、跳过缺 batch、只前移 cursor 不落 projection、同 batch 双应用、wall-clock 合并均失败。

### S77 Authority Epoch 与 Single-writer Fence

- **Fixtures**：epoch n/n+1；stale executor/replica；伪 authority event envelope；有效/无效 fence token；并发 owner control。
- **触发**：rotate/revoke 产生新 epoch 后，让旧 peer dispatch、receipt、sync 和 append。
- **事件**：epoch 改变来自 owner control 的 `FederatedPeerRegistered|FederatedPeerRevoked`；旧 lease 必须 `RemoteExecutionLeaseChanged{Fenced|Expired}` 或在 append 前拒绝。
- **Store/结果**：只有 authority store 分配新 `stream_seq`；stale epoch 所有 authoritative writes=0；并发 expected-version 仅一个 commit。
- **反向断言**：peer 自升 epoch、replica promotion 为 writer、多数 peer 投票改 authority、旧 receipt 覆盖新 terminal outcome 均失败。

### S78 Revocation 与 Retention Honesty

- **Fixtures**：已复制 sensitive-but-allowed ref；active lease；peer online/offline；retention delete request 与 signed receipt。
- **触发**：owner revoke peer，取消/处置 in-flight，要求删除 replica 数据；分别返回/不返回可信 receipt。
- **事件**：`FederatedPeerRevoked{in_flight}` 后无新 lease/export；删除 receipt 作为受限 evidence 进入 control run/trace，不重写历史 event。
- **Store/结果**：authority 标记 peer revoked 与 retention status；有 receipt 才显示 remote deletion verified，无 receipt 显示 requested/unknown。
- **反向断言**：未绑定 authenticated Replica channel、channel/receipt peer 不同、payload 自报 identity、revoke 删除历史、离线即宣称已擦除、旧 peer 继续 sync/action、retention 作为 permission bypass 均失败。

### S79 Cross-device Owner Approval 与 Cancel

- **Fixtures**：两个 OwnerClient devices；同 approval 的 nonce/plan digest/expiry；executor/replica/external 伪响应；cancel race。
- **触发**：device A 请求，device B 批准；重放 grant；随后从另一 owner device 取消 active lease。
- **事件**：既有 `ApprovalRequested -> ApprovalResolved` 仍绑定 authenticated owner control ref；取消链为 owner command -> `ActionCancelled|ActionOutcomeUnknown -> RemoteExecutionLeaseChanged{Released|Fenced}`。
- **Store/结果**：approval one-shot；第二响应/过期/nonce mismatch=0；cancel 由 authority 决定并传播，terminal race 按事件/CAS 唯一化。
- **反向断言**：peer channel 与 owner auth 折叠为单个自报 context、channel/session mismatch、push receipt 直接授权、executor/replica 批准、离线 grant 缓存重放、cancel 后新 output 覆盖 terminal state 均失败。authority 在审批前重启时旧 approval driver_calls=0，继续必须新 run/plan/approval。

### S80 Authorized Placement

- **Fixtures**：healthy/expired/revoked/stale/unauthorized executors；CapabilityEvidence pass/fail；M3 SelectionPolicy；managed deny。
- **触发**：为同一 action 生成 placement，改变 score、health 和 strategy version。
- **事件**：`SessionBound{evolution,federation snapshots} -> ResourcePlanned -> DecisionTraceRecorded{candidate peers,evidence,selection} -> ActionPlanned{chosen peer}`。
- **Store/结果**：先按 grant/role/scope/policy/freshness/capability filter，再排序；选择 ref/digest 固定进 plan。无候选时 ask/local fallback/stop，但不自动注册。
- **反向断言**：score/历史成功覆盖 deny、策略扩大 peer role、失败后静默换机执行、provider self-report 抬高 ceiling 均失败。

### S81 Verified Checkpoint Handoff

- **Fixtures**：三 checkpoint long-horizon goal；executor A/B；pinned M3 v1/v2；durable artifacts；in-progress outward action；cancel/revoke。
- **触发**：A 完成 verified checkpoint，authority 选择 B 继续；另尝试 mid-action 迁移和缺 artifact 接续。
- **事件**：segment A `SessionBound{snapshots} -> ... -> VerificationFinished{Pass} -> MemoryNodeAppended{checkpoint}`；segment B 是新 `RunAccepted -> SessionBound{new snapshots} -> ...`，各自独立 lease。
- **Store/结果**：DoneContract/checkpoint/artifact/evolution/federation refs 可追；active strategy 变化只影响新 segment；进行中 action 不迁移。
- **反向断言**：复制模型隐状态、无 verify handoff、跨 segment 复用 approval/lease、扩大 toolset/budget、revoke 后继续均失败。

### S82 Global Scheduling 与 Attention

- **Fixtures**：两个 device tick、一个 foreground run、background intention、global budget、AttentionBudget、offline duplicate signal。
- **触发**：并发 tick/foreground/cancel/resume。
- **事件**：device signal 先进入 authority，最多一个 `ProspectiveIntentionResolved{fired} -> RunAccepted{Schedule}`；foreground 时 background `RunWaiting`，cancel/budget terminal 后无新 action。
- **Store/结果**：authority 对 intention claim、budget、foreground priority 和 attention rate 单一 CAS；duplicate device signal 不创建第二 run。
- **反向断言**：未认证或 channel/signal peer 不同、每设备独立 cron、partition 中无限执行、重复提醒不计 attention、device clock 决定顺序、cancel 后恢复均失败。

### S83 M4 端到端 Federated Golden

- **Fixtures**：authority/Gateway、executor、replica 三独立进程；owner device approval；repository-owned mutation server；partition/reconnect；peer revoke；artifact exporter。
- **触发**：owner 提交 -> 远端执行一次 -> replica 接收 redacted trace -> 模拟 receipt 丢失并恢复 -> revoke executor -> 尝试第二次执行。
- **事件**：`FederatedPeerRegistered* -> RunAccepted -> SessionBound{evolution,federation} -> ... approval/gate/lease/action/verification ... -> ReplicationCheckpointAdvanced -> ActionOutcomeUnknown? -> recovered terminal -> FederatedPeerRevoked`；revoke 后无 `ActionStarted`。
- **Store/结果**：server mutation ordinal=1；authority `stream_seq` 连续；executor dispatch=1；replica cursor 精确；最终 peer revoked；portable artifact 可离线重建整条 lineage。
- **反向断言**：任一进程共享私有 shortcut、重复 mutation、replica 含 secret/raw sensitive value、stale executor 再执行、artifact tamper/extra entry 均失败。

### S84 Federation Security 与 Release Gate

- **Fixtures**：clean M4 tree；M3 historical receipt；M4 receipt；impersonation/replay/downgrade/stale epoch/secret/private endpoint/copy/license/advisory fixtures。
- **触发**：运行 S22 doctor、19+ Python gates、peer/transport threat fixtures、artifact verifier、dependency/license/RustSec/current-tree audit。
- **报告**：typed checks 至少覆盖 release tree、license/NOTICE、dependencies、borrowing、upstream corpus、artifacts、vulnerability、federation threat profile；保存 tool/version/input digest。
- **结果**：M3 receipt 独立 verify；M4 current tree receipt 与重生成结果一致；transport/crypto 新依赖精确 pin 并登记；所有 secret/private identity material 扫描为零。
- **反向断言**：scanner 缺失、corpus 缺失、旧 receipt 被当当前树、可达 advisory、identity private key/artifact 泄漏、fixture 漏报均 blocking。

## 5. 三类正确性

- **Runtime**：peer lifecycle、lease/fence、dispatch/probe/receipt、replication/cursor 和 checkpoint handoff 正确终止，partition/restart 可恢复。
- **Governance**：单 owner/authority、Harness-first、L3/L5、approval/plan digest、CompetenceGate、SecretRef、Untrusted content、M3 strategy boundary 全部不回退。
- **Distributed correctness**：authority `stream_seq` 唯一，epoch/cursor/lease CAS 明确，duplicate/reorder/stale/revoke race 不产生第二事实或重复副作用。

## 6. Artifact Gate

M4-A 起必须生成可离线读取的 typed artifacts：

- `FederatedPeerManifest`：peer role/scope/identity digest/epoch/TTL，不含证书、公钥私钥材料或地址凭据。
- `RemoteExecutionReceipt`：intent/plan/executor/lease/fence/outcome/ground truth refs 和 rollback boundary。
- `ReplicationManifest`：peer、aggregate、from/to cursor、batch/event digests、redaction profile、authority epoch。
- `FederatedTraceManifest`：owner command、approval、lease、action、verification、replication、partition/recovery/revoke event refs。
- `FederationGoldenReport`：双端 call/mutation/cursor/secret-scan/negative assertions。

Verifier 必须先验证 schema、content address、closed artifact set、event order、authority identity、lease/plan binding、cursor continuity、tamper/path/secret scan，再允许报告引用 PASS。

## 7. 分波与最终门

- **M4-A gate**：S70-S74 + S1-S69 scenario regression + 93-kind prefix/snapshot + 18-crate graph + real two-process golden + fmt/check/clippy/tests/compliance；报告后停下 owner review。
- **M4-B gate**：S75-S79 + A/M3/M2/M1/M0 全回归；multi-peer replication 与 cross-device control artifacts；报告后停下 owner review。
- **M4-C gate**：S80-S84 + 全回归；三进程 golden、federation threat audit、M4 release receipt 和最终报告。

任何场景不得用 mock-only、ignored 未单独实跑、最终字段、worker 日志或手写 PASS 代替 authority event sequence、真实 driver count 和 ground truth。
