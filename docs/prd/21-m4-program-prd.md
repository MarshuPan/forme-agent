# PRD 21：M4 单 Owner 联邦运行时实施计划

本文是 M4 的实施级总 PRD。它承接 `requirements/11-m4-scope-decisions.md`、`12-m4-verification-strategy.md`、`architecture/canonical-contract.md` §26、`architecture/07-m4-federated-runtime-architecture.md` 和冻结的 M0-M3 PRD。实现不得在模块内重定义 peer/authority/lease/receipt/replication，也不得因为跨进程而放松既有治理。

## 0. 定位与状态

- **目标**：把 owner control -> authority plan/approval -> remote lease/fence -> authenticated execution -> authority verification -> filtered replica -> checkpoint handoff 做成第一条可运行、可审计、可离线复核的联邦闭环。
- **冻结基线**：M3 封板时的 main、18 crates、89 EventKinds、S1-S69、M3 typed artifacts/release receipt、canonical §1-25 和全部 M0-M3 不变量。
- **M4 additive 目标**：93 EventKinds；所有 M4 DTO versioned；无内部 crate/edge 变化；历史前 89 strict prefix。
- **交付节奏**：A/B/C 三波。每波实现、全回归、typed artifacts、验收报告、owner review 后才进入下一波。
- **当前状态**：规格已冻结，M4-A/B/C 历史上实现并通过 owner 完整验收；首次 owner 验收的 Browser click 时序阻断已修复，M4 已封板。

## 1. 做与不做

### 1.1 做

- M4-A：owner-provisioned peer lifecycle、authority epoch/CAS、TLS identity、RemoteExecutorBackend、one-shot lease/fencing、unknown-outcome recovery、双进程真实 golden。
- M4-B：有限静态 peers、per-peer/per-aggregate filtered replication、read-only ReplicaMode、offline resume/cursor CAS、OwnerClient approval/cancel、revoke/retention honesty。
- M4-C：authorized executor placement、verified checkpoint handoff、authority-global scheduler/attention、authority+executor+replica 三进程 golden、federation threat/release audit。

### 1.2 不做

- 不做多租户、多组织 RBAC、匿名/公开公网 control plane、自动发现即授权或 TOFU。
- 不做多主 EventStore、CRDT/gossip、leader election、replica promotion、跨 owner 合并。
- 不做远端 Agent 大脑、自主 Agent team、worker memory/strategy/policy owner、远端自选模型/工具/任务。
- 不做 marketplace、远程自动安装、通用计算云、弹性调度或自研密码学。
- 不自动扩大 permission/trust/grant/envelope，不做 standing L5，不在 partition 中放宽自治。
- 不在线修改源码/tests/verifier/fixed identity，不自动发布。

## 2. 全局集成契约

### 2.1 Crate owner

| 事实/行为 | owner | 消费方 |
|---|---|---|
| M4 DTO/EventKind | protocol | 全部 crate |
| peer/epoch/lease/checkpoint authoritative events and projections | store | harness/eval/gateway |
| TLS/dispatch/probe/cancel/inner driver admission | execution | harness |
| remote risk/permission/approval | policy/approval | harness/gateway |
| executor lifecycle/profile and authorized candidates | capabilities | coordination/harness |
| placement/checkpoint route | coordination | harness |
| receipt/ground-truth/replication/artifact verification | eval | harness/gateway |
| snapshot bind、final recheck、lease CAS、dispatch、unknown裁决、append | harness | gateway/cli |
| owner/peer session and control authentication | gateway/communication | harness |
| read-only replica projection | store `ReplicaProjectionStore` | replica surface/eval |

禁止新增 `execution -> store/harness/gateway`、`store -> execution/gateway`、`coordination -> store/execution`、`gateway -> store`、`replica -> EventStore::append` 或任何 remote callback 直调 loop 的边。跨域只传 protocol DTO/ref/digest 和 Harness orchestration。

### 2.2 冻结接缝

- 既有 `EventStore`、`VersionedEventStore`、`EvolutionEventStore`、`ActionBackend`、`PolicyEngine`、`ApprovalBroker`、`AgentHarness`、`CoordinationReasoner`、`CompetenceGate` 签名不改。
- 新能力使用 architecture/07 的 `FederationProjection`、`FederationEventStore`、`ReplicaProjectionStore`、`RemoteTransport`、`RemoteLeaseCoordinator`、`RemoteExecutorDriver` companion traits。
- execution 只执行/传输已经受治理的 plan+lease；Harness 是唯一能把 owner control、policy/approval/gate、store CAS 和 backend 串起来的组件。
- store 只接受 authority 构造并校验的 M4 event；peer command/receipt/ack 本身不是 event。
- M2 `SyncWriteBatch` 继续保留兼容，但 M4 replica 禁止调用 `into_authoritative_write/apply_sync_batch`。
- M3 active strategy 只能在已授权 peer 集合内排序；它不能注册 peer、发 lease、改 epoch 或扩大 scope。

### 2.3 事件主链

Peer lifecycle：

```text
RunAccepted{OwnerControl}
 -> SessionBound{current federation snapshot}
 -> FederatedPeerRegistered|FederatedPeerRevoked
 -> RunComplete
```

Remote action：

```text
RunAccepted
 -> SessionBound{evolution + federation snapshots}
 -> ToolCallProposed
 -> ToolPolicyEvaluated
 -> ActionPlanned{Remote, placement, plan digest}
 -> ApprovalRequested -> RunWaiting
 -> ApprovalResolved -> RunResumed
 -> CompetenceGateEvaluated
 -> RemoteExecutionLeaseChanged{Acquired}
 -> ActionStarted
 -> ActionCompleted|ActionFailed|ActionOutcomeUnknown
 -> RemoteExecutionLeaseChanged{Released|Expired|Fenced}
 -> VerificationStarted/Finished
 -> RunComplete|RunWaiting|RunFailed
```

Replication：

```text
authority export{peer, aggregate, from, to, digest}
 -> replica atomic apply + cursor CAS
 -> authenticated ack
 -> ReplicationCheckpointAdvanced
```

Checkpoint handoff：

```text
segment A VerificationFinished{Pass}
 -> MemoryNodeAppended{durable checkpoint}
 -> new RunAccepted(segment B)
 -> SessionBound{new pinned snapshots}
 -> new placement/approval/lease
```

任何链都不能让 network connected、worker log、receipt self-report 或 replica projection 直接生成 permission、stable evidence、active strategy 或 terminal success。

## 3. M4-A：Peer Trust Plane 与远端执行

### A0 Specification/Protocol Compatibility

1. 建立 `architecture/m4-a-protocol-compatibility.md`，冻结 89 -> 93 exact prefix、newtype/DTO/enum/payload defaults、invalid/legacy/replay/open-store contract。
2. 实现 `Source::OwnerControl`、`BackendKind::Remote`、`ActionParameters::Remote(Box<RemoteActionSpec>)`、`ConfigCheck::Federation`、communication federated auth method 和四个 P 组 EventKinds；Box 只打断 Rust 递归尺寸，wire shape 保持单层 Remote。
3. `EventKind::ALL` 精确为 93；前 89 顺序/string/payload mapping 与 M3 snapshot 完全一致。
4. 所有 ID/ref/digest/epoch/version/lease/fence/cursor 使用 protocol newtype；zero/empty/duplicate/overflow/unknown/recursive Remote fail closed。
5. legacy 89-kind store 可打开，federation projection 为空；legacy optional `None` 不授权远端行为。
6. 选择 TLS/HTTP transport dependencies：只用成熟库，精确 pin，记录 source/license/copy risk/NOTICE/advisory；不复制 example/fixture/error/name system。

### A1 Store Registry、Epoch and Lease CAS

- 新增 federation schema migration：registry version ledger、peer projection、authority epoch、lease ledger、dispatch idempotency ledger、outbound/receipt ledger；迁移在 immediate transaction 完成。
- `FederationEventStore::append_federation_expected` 校验 event kind/payload aggregate、expected/committed version、epoch transition 和 projection mutation；conflict 零写入。
- Register/update/revoke 都由 authenticated owner control run 构造 event。每次 mutation epoch+1，并在同一事务把旧 epoch nonterminal leases 标为 fenced disposition。
- Snapshot 在一个一致性读事务中解析 current grants，按 peer/ref 稳定排序并计算 digest。过期/revoked/unknown schema grant 不进入 active snapshot。
- lease acquisition 验证 immutable plan/placement、current grant/version/epoch、approval、risk/gate refs 和 dispatch id；acquire/terminal transition 单调、一次性、可重建。
- crash recovery 扫描 Acquired/dispatch-attempted lease：有可证明 no-dispatch 才 release；否则 outcome unknown + probe/manual review，绝不再 dispatch。

### A2 TLS Transport and Executor Process

- 在 `forme-execution` 现有 crate 建立独立 executor daemon `[[bin]]`，不新增 crate；不得放进 gateway 后再新增 `gateway -> execution` 依赖边。
- TLS 1.3、双向/预置 identity、certificate/public identity digest pin、bounded body、timeout、nonce/replay ledger、epoch/lease/plan/fence digest validation。
- endpoint/origin 使用结构化 URL parser；redirect、identity drift、protocol downgrade、unknown schema、oversize body 在 admission 前拒绝。
- executor 本地 provision authority identity、peer grant/profile、credential slot -> SecretRef mapping；private key/secret/endpoint 只在本地 config/secret provider。
- `RemoteExecutorDriver::admit` 按 architecture/07 §5.3 顺序重查；zero admission = zero inner driver call。
- inner operation 首版只启用 repository-owned golden 所需的一个窄 backend/operation；结构支持 registered set，但不把首版做成任意 shell 云。
- dispatch acceptance、`RemoteDriverReceipt`、probe、cancel 使用 typed closed schema。相同 id/相同 digest 幂等；相同 id/不同 digest blocking。Worker receipt 只有 observations，不能携带 ground_truth。

### A3 RemoteExecutorBackend and Harness Choke Point

- `RemoteExecutorBackend::plan` 验证 RemotePlacementPlan/inner operation，并让既有 ExecutionPlan digest覆盖其完整语义。
- Harness remote path 复用正常 Toolset/Policy/Approval/CompetenceGate；Remote 默认至少 L3，L5 逐动作 owner approval。
- approval 绑定最终 plan digest。批后 peer/grant/epoch/profile/operation/schema/slot/rollback 变化必须重计划重批。
- approval 后 final recheck 通过才 CAS acquire lease；backend binding 缺失、stale 或不匹配时 transport_calls=0。
- authority 在 authenticated dispatch acceptance 后追加 ActionStarted；eval 将 untrusted `RemoteDriverReceipt` 与独立 ground truth 封装成 authority `RemoteExecutionReceipt` 后，Harness 才追加 terminal event。
- disconnect 分 dispatch-before/after 两类。可能已 dispatch 的路径必须 ActionOutcomeUnknown；同 lease只 probe/收原 receipt。
- remote output 强制 Untrusted provenance；prompt injection、secret echo、fake owner command、self-ground-truth 不得改 policy/graph/rubric/active strategy/grant/CapabilityEvidence pass。

### A4 Gateway、ConfigDoctor and Artifacts

- owner APIs：list/register/update/revoke peers、inspect epoch/snapshot/lease/unknown outcome、resolve manual probe disposition。全部 require configured owner AuthContext。Peer ingress 使用独立 opaque channel binding，不能复用 owner AuthContext 伪造 TLS 身份。
- executor APIs only accept TLS peer messages；不能暴露 owner control API。Gateway 不把 peer identity映射成 owner principal。
- ConfigDoctor Federation row 覆盖 mode/identity/TLS/grants/TTL/epoch/lease/replay/credential slots/L3-L5/artifact scanners。
- typed writer/verifier 首批输出 FederatedPeerManifest、RemoteExecutionReceipt 安全视图、FederatedTraceManifest、FederationGoldenReport。
- artifact closed set/content address/schema/ref/event order/plan-lease-fence binding/ground truth/tamper/path/secret/SecretRef/private endpoint scan 全部 blocking。

### A5 S70-S74 and real two-process golden

- S70：owner register/revoke；TOFU、wrong identity、external/subagent、role expansion、unbounded TTL、CAS conflict 零写入。
- S71：remote mutation 完整事件链；approval 前 driver=0；plan/grant/epoch/fence drift driver=0。
- S72：lost receipt/partition/reconnect；dispatch=1、mutation<=1、same lease retry=0；unproven outcome remains unknown。
- S73：executor-local credential slot、secret scan、prompt injection、fake ground truth。
- S74：authority/executor 两独立进程 + real TLS loopback + repository-owned state server；mutation ordinal=1、driver=1、portable artifacts offline verify。

### M4-A gate

S1-S74、93-kind prefix/snapshot、18-crate graph、all protocol/store/execution/harness/gateway/eval tests、real TLS golden、fmt/check/strict clippy/Python/compliance/current-tree audit 全部 PASS。输出 `docs/acceptance/m4-a-acceptance-report.md` 并停下 owner review。

## 4. M4-B：多 Peer 复制与跨设备控制

### B0 Compatibility

- 建立 `architecture/m4-b-protocol-compatibility.md`。默认不新增 EventKind；若 common M4 DTO 在 A 未激活，必须说明 additive fields、legacy defaults 和 93-kind invariant。
- ReplicationBatch/Cursor/Ack/ApplyReport 与 FederatedControlEnvelope 有 round-trip、unknown/zero/gap/reorder/tamper/overflow tests。
- M2 SyncPeer/SyncTransferBatch legacy reader 保持；类型不可与 M4 FederatedPeer/ReplicationBatch 混传。

### B1 Multi-peer Export

- `FederationEventStore::export_replication` key = `(peer, aggregate)`；验证 Replica role/scope/current grant/epoch/cursor/redaction/limit。
- exporter 在一致性 snapshot 上选择连续 `stream_seq`，构建 closed content-addressed batch；不得按 timestamp/device clock 合并。
- per-peer scope/filter/redaction：raw input/model delta/tool args/action output/SecretRef/private endpoint/path/owner-only control 必须 redacted/omitted。
- outbound attempt 不推进 authoritative checkpoint。只有 verified ack 经 CAS 后追加 `ReplicationCheckpointAdvanced`。
- 同一 batch retry 可重发同 bytes；重新序列化产生不同 digest 或重用 id 改语义必须拒绝。

### B2 Read-only ReplicaMode

- `ReplicaProjectionStore` 使用独立 schema/table/API；在 `forme-store` 现有 crate 提供 replica daemon `[[bin]]`，不新增 crate。编译/运行接口不暴露 EventStore append、VersionedEventStore apply、candidate/stable/evolution write。
- batch digest、peer/grant/epoch、from/to、event kind/payload relation、contiguous seq、redaction completeness 全验证后，在一个事务写 envelope/projection/cursor。
- duplicate equal batch幂等；B-before-A、gap、stale cursor、wrong peer/scope/epoch、tamper 全部零 cursor/projection write。
- crash before commit 全回滚；crash after commit 重放返回 Duplicate。Projection 可从 retained batches 重建相同 digest。

### B3 OwnerClient Approval/Cancel

- peer TLS identity only opens an opaque channel binding；separate Gateway owner auth produces VerifiedPrincipal owner。两个证明是两个独立参数，单个调用方可构造 AuthContext 不得同时充当二者。
- OwnerClient control binds session/peer/owner/nonce/expiry/command digest。wrong role/executor/replica/external/expired/replay fail before Harness control。
- resolve approval 继续验证 approval id、plan digest、choice、nonce、expiry、one-shot principal；second response zero write。
- cancel first commits authority disposition，then remote cancel；late output cannot overwrite terminal event。If side effect uncertain，remain unknown。

### B4 Revocation and Retention Honesty

- revoke blocks new session/lease/export/approval relay in the same epoch/CAS transition。
- active lease disposition explicitly KeepProvenTerminal/Cancel/Fence/WaitOwner；never claim remote effect reverted。
- retention request/receipt state machine = NotRequested -> Requested -> Verified|Unknown。只有已 provision 的 Replica identity 获得 retention-receipt-only binding，且 peer 与 receipt/request epoch 一致时可进入 Verified；该窄入口不恢复 revoked peer。Offline/revoke/timeout/payload 自报 identity 不能产生 Verified。
- history/events/artifacts immutable；remote deletion receipt is limited evidence，not event erasure。

### B5 S75-S79 and artifacts

- S75：two replica peers、two aggregates、different scopes；batch closed/redacted/contiguous and authority-only。
- S76：offline resume、A/B reorder、duplicate、tamper、cursor CAS、crash before/after commit。
- S77：epoch rotation、stale executor/replica、fake authority envelope、non-authority append=0。
- S78：revoke and retention with online/offline branches；honest status。
- S79：device A request/device B owner approval、replay denial、cross-device cancel race。
- output ReplicationManifest and extend FederatedTrace/FederationGoldenReport；independent verifier checks cursor/ack lineage。

### M4-B gate

S1-S79、M4-A golden/artifacts、M3/M2/M1/M0 all regressions、93-kind、18-crate、fmt/check/strict clippy/tests/Python/compliance/current-tree audit PASS。输出 `m4-b-acceptance-report.md` 并停下 owner review。

## 5. M4-C：分布式长任务连续性

### C0 Compatibility

- 建立 `architecture/m4-c-protocol-compatibility.md`；默认不新增 EventKind。
- placement/checkpoint/scheduler additions use existing ResourcePlan/DecisionTrace/GoalCheckpoint/ProspectiveIntention payload extension points or versioned artifacts。
- legacy M2/M3 runs without federation snapshot can replay but cannot be resumed as remote segment without new RunAccepted/SessionBound。

### C1 Authorized Placement

- filter order固定：current grant/role/scope/capability/expiry -> managed policy -> schema/profile compatibility -> health freshness -> result-led capability/failure evidence -> M3 pinned selection rank。
- candidate set、filter reasons、evidence refs、chosen peer/profile/placement digest enter DecisionTrace。
- no candidate：local allowed fallback/ask/prepare/stop。No auto registration、scope expansion or silent executor swap。
- score/self-report/history can only lower/choose within allowed set；never authorizes。

### C2 Verified Checkpoint Handoff

- checkpoint writer records DoneContract、VerificationFinished Pass、artifact refs、spent/remaining budget、external effects、policy/tool/model/evolution/federation snapshots。
- handoff verifier checks complete/portable/scope-compatible before ResourcePlan selects next peer。
- next segment is new Harness run and new snapshots/plan/approval/lease。No model hidden state、mid-action transfer、old approval/lease reuse。
- cancel/revoke/budget/foreground priority re-evaluated before segment start；active strategy/peer changes affect only new segment。

### C3 Global Scheduling and Attention

- all device ticks/reconnect signals first bind to an authenticated OwnerClient channel and then normalize to authority command；device time or payload self-identity cannot set authority/order/due state。
- existing intention claim CAS guarantees at most one scheduled run。Duplicate/offline signal creates zero second run。
- foreground priority、AttentionBudget、global time/token/cost/message budgets、cancel/revoke are authority single decisions。
- partition does not allow executor autonomous continuation beyond current lease/operation；no infinite background work。

### C4 Three-process Golden and Release Audit

- independent authority/Gateway、executor and replica processes；real TLS；owner device approval；repository-owned mutation server。
- path：register peers -> submit -> independently authenticated owner approve over bound peer channel -> remote mutation once -> filtered replicate -> authenticated replica ack -> lose receipt/partition -> recover original receipt/probe -> revoke executor -> second attempt denied。
- server mutation ordinal=1、executor inner driver=1、authority stream_seq per aggregate continuous、replica cursor exact、revoke after no ActionStarted。
- artifacts contain no secret/SecretRef/private key/private endpoint/host path；tamper/extra file/path escape fail。
- federation threat audit covers impersonation、TOFU、replay、downgrade、stale epoch/fence、receipt forgery、cursor tamper、prompt injection、secret echo and retention overclaim。
- release audit verifies immutable M3 receipt separately，then generates M4 current-tree receipt with Git-index canonical identity；license/NOTICE/dependency/RustSec/borrowing/originality/artifacts all blocking。

### C5 S80-S84

- S80 authorized placement and deny precedence。
- S81 verified checkpoint handoff/new run/new snapshots/new lease。
- S82 duplicate ticks/foreground/budget/cancel authority single decision。
- S83 full three-process federated golden with partition/recovery/revoke。
- S84 threat/release/current-tree/historical-receipt gate and negative fixtures。

### M4-C/final gate

S1-S84、A/B/C golden/artifacts、93-kind、18-crate、all tests、fmt/check/strict clippy、19+ Python gates、compliance、federation threat and M4 release receipt PASS。输出 `m4-c-acceptance-report.md` 与 `m4-acceptance-report.md`。

## 6. Error Classification and User-visible Results

| Condition | Failure/result |
|---|---|
| peer identity/TOFU/role/scope/TTL invalid | safety policy/trust failure；register/connect denied。 |
| federation CAS conflict | typed conflict；zero event/ledger/projection write；reload and re-decide。 |
| TLS/downgrade/replay/schema/digest invalid | safety policy/verification failure；zero driver/apply/control。 |
| lease/epoch/fence/plan mismatch | safety policy failure；zero inner driver call。 |
| dispatch may have happened but no terminal proof | ActionOutcomeUnknown + execution failure evidence；wait/probe/manual review。 |
| worker self-report without ground truth | unverifiable/self_eval_trap；no CapabilityEvidence pass。 |
| replica gap/reorder/tamper/stale cursor | verification failure；cursor unchanged。 |
| owner device auth missing | control denied；peer role does not become owner。 |
| revoke/retention without deletion receipt | revoked + retention unknown/requested；never “deleted”。 |
| checkpoint incomplete/unverified | handoff failure；no next segment ActionStarted。 |
| secret/private endpoint/artifact leak | blocking compliance/security failure；wave FAIL。 |

User surfaces must distinguish peer registered/expired/revoked、connected/stale observation、lease acquired/fenced/released、dispatch accepted、outcome unknown、receipt received but unverifiable、verified terminal、replication pending/applied/checkpointed、retention requested/verified/unknown。不能统一显示“远端完成”。

## 7. Test Strategy

### 7.1 Protocol/Store

- 93 EventKind exact snapshot、first-89 prefix/string/payload、legacy 89-kind open/replay。
- M4 DTO round-trip and invalid zero/empty/duplicate/overflow/recursive Remote/unknown enum。
- peer registry/epoch/lease/checkpoint projection rebuild；expected-version conflict zero write；duplicate semantics idempotency。
- per-peer/per-aggregate batch/cursor contiguous、redaction、crash atomicity；ReplicaMode has no authority append path。

### 7.2 Execution/Harness/Policy

- Remote default L3、L5 explicit owner、approval plan digest、final recheck、CompetenceGate、grant/epoch/fence drift。
- TLS identity/replay/downgrade/oversize/redirect；executor admission order and driver call counter。
- dispatch before/after disconnect、duplicate dispatch/receipt、cancel race、unknown no retry/probe only。
- remote content Untrusted；secret resolution only executor boundary；fake owner/ground-truth/graph/strategy mutations = 0。

### 7.3 Gateway/Coordination/Continuity

- peer identity vs owner principal separation；OwnerClient one-shot approval/cancel and nonce/expiry/replay。
- placement filter before rank；score does not authorize；no candidate safe outcomes。
- checkpoint verified/new run/new snapshots/new lease；mid-action/hidden-state/old approval reuse rejected。
- multi-device tick/intention CAS/foreground/AttentionBudget/cancel/revoke no duplicate run。

### 7.4 Artifacts/Compliance

- two-process and three-process real TLS golden，not mock-only or same-process shortcut。
- closed artifact set、content address、event refs、plan/lease/fence、ground truth、cursor/ack/revoke lineage。
- secret value/name/SecretRef/private key/certificate/session material/private endpoint/absolute path scan。
- tamper、extra entry、path escape、handwritten PASS、missing corpus/scanner、historical/current receipt confusion all negative fixtures。

## 8. Acceptance and Commit Gates

Every implementation commit：

```text
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
py -3 -m unittest discover -s tools/tests -v
bash tools/compliance-doctor.sh
```

`tools/verify-m4.ps1` must：

1. run exact S1-S69 scenario/protocol/artifact regressions；
2. run activated S70-S84 wave scenarios and real process goldens；
3. assert 86 -> 89 -> 93 strict prefixes and 18-crate exact graph；
4. verify repository artifacts with independent verifier；
5. verify immutable M3 release receipt without comparing it to M4 current tree；
6. generate/compare current M4 release receipt；
7. run fmt/check/strict clippy/all tests/Python/compliance。

Each wave report records branch/HEAD、baseline、commands/tool versions、test counts、S# event sequences、negative call/write counts、93-kind/18-crate、artifact digests、secret/private-endpoint scan、dependency/license state and residual risks。Ignored tests must be run explicitly and reported；mock-only does not satisfy a real golden。

## 9. Originality and Dependencies

- federation protocol、lease/fence state machine、fixtures、errors、wire objects、tests and docs are repository-owned；do not copy third-party remote/node protocols、source、prompts、fixtures、errors、naming or directory structure。
- TLS、lease、fencing、cursor、replication and checkpoint are common engineering mechanisms；use mature library public APIs and standards, not upstream Agent implementation material。
- every new direct dependency must be exact-pinned and added to `docs/compliance/third-party-dependencies.md` and borrowing records before merge；include license、MSRV、NOTICE、reachable advisory and public-API-only rule。
- `research/upstreams/**` remains excluded from build/import/include and is scanned through `.git/commondir` fail-closed behavior in linked worktrees。
- M4 release/security audit is engineering evidence，not legal opinion or authorization to expose a public service。
