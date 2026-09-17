# M4 单 Owner 联邦运行时最终验收报告

- 验收日期：2026-07-22
- 主线基线：M3 封板
- 范围：M4-A/B/C、S70-S84；S1-S69 为永久回归门
- 结论：**Owner 完整验收 PASS；未合并 `main`**

## 1. 完成范围

M4 保持一个 owner、一个 authority Harness、一个 authoritative EventStore 和一个 `stream_seq` 分配者，把 M0-M3 的受治理运行时延伸到受认证远端系统：

- M4-A：peer grant/epoch/revoke、mutual-TLS `RemoteExecutorBackend`、one-shot plan-bound lease/fencing、executor-local SecretRef、unknown-outcome probe-only recovery。
- M4-B：authority 到有限静态 Replica 的 filtered/redacted replication、per-peer/per-aggregate cursor CAS、独立 owner authentication 的跨设备 approval/cancel、revoked Replica retention receipt-only path。
- M4-C：authorized placement、verified durable checkpoint handoff、authority-global scheduler/AttentionBudget、独立 authority/executor/replica 三进程 golden、M4 release/federation threat audit。

## 2. S70-S84 逐场景证据

| 场景 | 可执行证据 | 关键事件/状态断言 | 反向断言 | 结果 |
|---|---|---|---|---|
| S70 Peer lifecycle | Harness enrollment + store single-writer tests | owner control `RunAccepted -> SessionBound -> PeerRegistered|Revoked -> RunComplete`；epoch 单调 | TOFU/external/stale CAS/role expansion 零写；revoke 后零新 lease/export | PASS |
| S71 Governed remote | Harness full chain + execution admission | `Plan -> Approval -> Gate -> Lease Acquired -> Start -> Complete -> Lease Released -> Verify Pass` | approval/plan/schema/grant/epoch/fence drift 在 driver 前拒绝 | PASS |
| S72 Unknown recovery | Harness restart/recovery + real mTLS durable dispatch | `Start -> OutcomeUnknown -> Waiting -> original receipt/probe -> terminal` | timeout/restart/duplicate 不重放；dispatch count=1 | PASS |
| S73 Secret/trust | Harness ground-truth/injection tests + protocol negatives | authenticated sender 内容仍 Untrusted；failure evidence 先于能力结论 | secret echo/self-report/prompt injection 不改 policy/graph/strategy/grant | PASS |
| S74 Network golden | three-process ignored exact + Gateway harness route + artifact verifier | real mutual TLS、approval、remote mutation、unknown/recovery、verification | driver=1、mutation=1；匿名/shortcut/tamper/extra file 拒绝 | PASS |
| S75 Filtered replication | store batch test + Gateway peer ingress | authenticated `batch+ack` 后 `ReplicationCheckpointAdvanced` | scope/raw/secret/private ref 不外泄；payload digest 不可自证 | PASS |
| S76 Resume/cursor | store + independent `forme-replicad` process tests | `(peer,aggregate)` cursor 0 -> A -> B，apply/projection/cursor 原子 | reorder/gap/tamper/stale/duplicate 不产生第二应用 | PASS |
| S77 Epoch/fence | store single writer + Harness drift tests | owner register/revoke 推进 epoch；旧 lease fenced | peer 自升 epoch/伪 authority/stale receipt 零事实写 | PASS |
| S78 Revoke/retention | Harness retention/restart + Gateway channel test | revoke 后 retention `Requested -> Verified|Unknown`，历史不可变 | 仅 provisioned revoked Replica receipt-only binding；缺/错 receipt 不显示 deleted | PASS |
| S79 Owner devices | Gateway dual proof + Harness control/restart | channel binding 与 owner auth 分离；approval nonce/plan/expiry one-shot | 单自报 context/role 冒充/replay/旧等待态均零 dispatch | PASS |
| S80 Placement | coordination/protocol/Harness 三层 tests | filter -> rank -> `DecisionTraceRecorded -> ActionPlanned` | score/strategy/self-report 不产生授权；空集不自动注册 | PASS |
| S81 Handoff | protocol + Harness checkpoint test | A `Verification{Pass}`/checkpoint；B 新 `RunAccepted -> SessionBound`/lease | mid-action/hidden state/旧 approval/lease/扩 budget/revoke 后接续拒绝 | PASS |
| S82 Scheduler | Harness scheduler/ledger + Gateway ingress | device signal -> authority intention CAS -> 最多一个 scheduled run | 未认证/duplicate/device clock/partition/cancel 后恢复不创建第二 run | PASS |
| S83 Federated E2E | independent authority/executor/replica golden | register -> approve -> remote once -> unknown/recover -> replicate cursor 23 -> revoke | mutation=1、revoke 后零第二 ActionStarted、secret=0 | PASS |
| S84 Release threat gate | eval negatives + 21 Python tests + M4 audit generate/verify/compare | 8 typed checks Pass，current clean tree 与 repository receipt 相同 | dirty/missing/pin/copy/secret/impersonation/replay/advisory/fixture drift blocking | PASS |

逐场景的测试名、完整事件子序列和负向断言分别见 `m4-a-acceptance-report.md`、`m4-b-acceptance-report.md` 与 `m4-c-acceptance-report.md`。

## 3. M0-M3 永久回归

`tools/verify-m4.ps1` 首先运行 `tools/verify-m3.ps1 -RegressionOnly`，逐层保留 S1-S69；随后单独实跑 S38 真实 Browser golden 与 S68 真实 evolution/rollback Browser golden。M4 没有用“新分布式能力”替换或跳过旧入口、Policy、Approval、CompetenceGate、candidate/stable、promotion/activation、DisclosurePolicy、memory/graph provenance 或 release compliance。

workspace 默认套件列出 353 个 Rust tests，其中 350 个非忽略测试在 `cargo test --workspace --all-targets` 中运行；另外 3 个真实进程/浏览器测试由统一门以 `--ignored --exact` 显式执行。因此最终门实际覆盖全部 353 个列出测试，而不是把 ignored golden 当作默认已运行。

## 4. Frozen Architecture

- 18 crates，内部依赖边不新增、不反向；`tools/verify-m4-workspace-contract.ps1` 同时检查 exact pins 和 `forme-executord`/`forme-replicad` targets。
- 93 EventKinds；前 89 项是 M3 exact prefix，仅末尾追加 `FederatedPeerRegistered`、`FederatedPeerRevoked`、`RemoteExecutionLeaseChanged`、`ReplicationCheckpointAdvanced`。
- frozen `EventStore`、`VersionedEventStore`、`EvolutionEventStore`、`ActionBackend`、Harness/Loop/Coordination traits 不重设计；M4 只增加 DTO、companion traits 和既有 Harness orchestration。
- authority event append 与 federation version/registry/lease/checkpoint projection 在单一 SQLite transaction；ReplicaMode 不实现 authority append。
- Harness-first、最终执行前 policy/plan/grant/epoch/fence 重查、CompetenceGate 出口、plan-bound approval、unknown no-blind-retry、trust 不覆盖 permission 全部有正反场景。
- peer transport proof、owner authentication、content trust 三者分离；网页/API/stdout/remote participant 内容即使来自已认证设备仍是 UntrustedData。
- credential material 只由 executor-local SecretRef resolver 解析；event/log/trace/replication/release artifact 不保存 secret、SecretRef id、private key、endpoint 或 owner host path。

## 5. Typed Artifacts

S83 repository closed set 位于 `docs/acceptance/m4-artifacts/`：

| 证据 | 值 |
|---|---|
| 五件套 content receipt | `sha256:c24a84ef85d918654258a3108a5bf8bd5523361271e76dfdad529ce106319022` |
| authority driver calls | `1` |
| repository mutation ordinal | `1` |
| replica cursor | `23` |
| artifact secret matches | `0` |
| event lineage | owner commands、approvals、leases、actions、verifications、replication、recovery、revocation 全部 typed refs |

`forme-m4-artifact-verify` 对 schema、content address、closed five-file set、cross-artifact plan/lease/fence/cursor/trace lineage、unknown fields、extra/missing file、path escape 和 sensitive marker 执行 fail-closed 校验。repository set 与统一门临时重生成 set 均独立 PASS。

## 6. Release Audit

| 项 | 值 |
|---|---|
| typed receipt | `sha256:4ce2cc4b13ddfd0b15404ad7f6503347ec970f15f25afb25035da239d83239e2` |
| release root digest | `sha256:ab4ef932e986a555bd9ce878b99a801f42db42c84ee099af7a903e15e58a9568` |
| federation threat input | `sha256:cb45bd93631ae75272e66d94724eef80886cdeecd1dc2faddddb6eb152f56823` |
| audit profile | `forme-release-audit/4` |
| checks | `8/8 pass, blocking=false` |

S84 先独立验证历史 M3 receipt，再针对当前 M4 clean tree 运行 release tree、LICENSE/NOTICE、直接依赖 pin、borrowing records、upstream copy、acceptance artifacts、RustSec reachability 与 federation threat fixtures。临时结果必须独立 verify，并与 repository-owned M4 receipt 精确 compare；M3 receipt 不能冒充 M4 当前树证据。

`cargo-audit 0.22.2` 对锁文件中的 `quick-xml 0.30.0/0.39.4` 报告 `RUSTSEC-2026-0194/0195` 四个命中；逐项 target reachability 证明它们在当前 `x86_64-pc-windows-gnu` release target 不可达，因此作为 typed observations 保留。任何当前 target 可达 advisory 仍阻断 final gate。

## 7. Reproducible Final Gate

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools/verify-m4.ps1
```

该命令覆盖：S1-S84、93-kind exact prefix、18-crate graph、29 个 M4 exact contracts、独立 executor/replica 进程、S38/S68/S83 三个真实 goldens、temporary/repository artifact verification、fmt/check/strict Clippy、完整 workspace tests、21 Python tests、原创合规、whitespace、M3 receipt 和 M4 clean-tree release audit。

成功终线：

```text
M4 FINAL ACCEPTANCE: PASS (S1-S84 + 93 EventKinds + 18 crates + three-process federation golden + release compliance)
```

## 8. Owner 验收阻断修复

- 首次 owner 统一门在 S68 真实 Browser golden 暴露 click 导航时序问题：Harness 已完成，但 loopback mutation 尚未形成地面真值。
- 修复保持冻结 `BrowserDriver` trait 不变，在 click 前观察 main-frame lifecycle 与 document response；URL 变化不再等同于完成。
- 导航已开始但没有可信终态时，execution 现在发出 `ActionOutcomeUnknown`，不会伪造 `ActionCompleted` 或 `ActionFailed`，继续满足 no-blind-retry 不变量。
- 修复后 5 轮连续 S38 -> S68 真实 Chrome 顺序压测全部通过；新增状态机与 Unknown 事件序列负向测试纳入 workspace gate。

## 9. 残余边界

- 这是单 owner、单 authority、有限静态 peer 的 M4，不是多租户、多主、组织 RBAC、自动发现、弹性集群或公网匿名控制面。
- loopback mutual TLS 与独立进程证明真实 transport/process boundary，但不宣称生产证书运营、互联网可用性、HA 或灾难恢复成熟度。
- unknown outcome 始终需要原 receipt/probe/owner disposition；不承诺通用 exactly-once，不把 lease timeout 当作副作用未发生。
- remote deletion verified 是 purpose-limited receipt evidence，不是物理介质擦除保证；历史 authority events 与 prior manifests 保持不可变。
- M4 没有规格自动提升、远端 Agent 自治或第二策略 owner；后续里程碑不得以联邦广度削弱这些边界。

## 10. 结论

M4-A/B/C 已拼成一条完整的单 Owner 联邦运行时闭环：owner-provisioned peer -> governed remote plan/approval/gate/lease -> mutual-TLS真实动作 -> unknown-outcome 无盲重试恢复 -> authority ground-truth verification -> filtered read-only replication -> authenticated cross-device control -> verified checkpoint handoff -> revoke/retention/release audit。S1-S84、93 EventKinds、18 crates、全部列出 Rust tests、21 个 Python gates、三进程真实 golden、typed artifacts、strict Clippy 和 clean-tree compliance 均满足，且 M0-M3 治理不变量没有削弱。

**M4 owner 完整验收 PASS，已达到封板标准。**
