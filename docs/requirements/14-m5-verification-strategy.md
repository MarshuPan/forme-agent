# M5 验证与验收策略

本文把 `13-m5-scope-decisions.md` D96-D115 转成可执行场景。M5 从 S85 起编号；S1-S84 和全部既有测试是永久回归门。验收继续以权威事件序列、store/projection、真实 ground truth 和反向断言为主，不以最终回答或 publisher/catalog 自报为证据。

## 1. 验收原则

1. **来源认证不等于内容可信**：签名只证明 publisher key；catalog/bundle text/README/contribution output 仍为 UntrustedData。
2. **admission 不等于授权**：通过 package checks 不生成 trust、permission、approval、AutonomyEnvelope 或 active contribution。
3. **计划绑定**：install/enable/update/rollback/distribute 的 package/release/digest/contributions/scope/policy/rollback 任一变化使 approval 失效。
4. **结果证据主导**：registry projection、peer filesystem/package ledger 和 authority verification 是 ground truth；transport/publisher self-report 不能自证成功。
5. **失败关闭**：schema/digest/signature/dependency/license/SBOM/secret/path/CAS/epoch 任一不明即零激活、零分发。
6. **无盲重试**：dispatch 或 install outcome 不明进入 unknown/probe；同 lease/plan 不再次执行。
7. **历史不可变**：update/rollback/revoke 追加事件并切 active ref，不覆盖旧 release/event/artifact。

## 2. 全局回归门

每个波次必须保持：

- S1-S84 全绿；M4 三个真实 golden 继续实跑。
- `EventKind::ALL` 前 93 项与 M4 exact prefix 同序同名，总数只按冻结波次末尾追加到 97。
- 18 crates 与内部 dependency edge exact 不变；冻结 EventStore/VersionedEventStore/EvolutionEventStore/FederationEventStore/ActionBackend/Harness traits 不改签名。
- harness-first、执行前重查、CompetenceGate 出口、candidate/stable/active/permission 分离、one-authority、SecretRef、unknown no-blind-retry 全部继续有效。
- `cargo fmt --check`、`cargo check --workspace --all-targets`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace --all-targets`、Python tests、`bash tools/compliance-doctor.sh`、release audit PASS。

## 3. S85-S99 场景矩阵

| 场景 | 波次 | 核心证明 |
|---|---|---|
| S85 | A | Publisher provision/revoke 仅 owner 可做，key digest/TTL/kind/scope/CAS 绑定。 |
| S86 | A | Catalog/bundle/metadata 进入 quarantine，内容注入不能改 policy/trust/owner command。 |
| S87 | A | 内容寻址与 Ed25519 signature fail closed；tamper/wrong key/replay/rebind 零 admission。 |
| S88 | A | dependency closure、SBOM/license/secret/path/size/risk 检查完整，任一失败零 admission。 |
| S89 | A | 真实 loopback registry read -> quarantine -> admission golden，产 typed evidence 且无 key/secret。 |
| S90 | B | install/enable 绑定 immutable plan + owner approval；批后漂移零 registry change。 |
| S91 | B | lifecycle/CAS 与 registry contribution 原子可恢复；失败保留 previous known-good。 |
| S92 | B | exact update/pin/rollback 不覆盖历史、不扩大 permission、不重放外部动作。 |
| S93 | B | package/publisher revoke 立即移除可见贡献并 fence 后续 activation/distribution。 |
| S94 | B | plugin hook 仍 final recheck；agent profile 无认知、只由 Harness spawn child run。 |
| S95 | C | remote distribution 只到 active Executor grant，绑定 package/peer/epoch/plan/lease/fence。 |
| S96 | C | timeout/partition/duplicate receipt 进入 unknown/probe，同 lease install count 严格一次。 |
| S97 | C | authority/executor restart 后 CAS/idempotency/revoke/recovery 事实保持，legacy store 安全打开。 |
| S98 | C | authority + registry + executor 独立进程真实 golden，package 启用后能力可见且动作仍受治理。 |
| S99 | C | supply-chain threat/release gate：tamper/copy/license/advisory/secret/artifact drift 任一 blocking。 |

## 4. 逐场景规格

### S85 Publisher Provision 与 Revocation

- **Fixtures**：owner principal；两个 Ed25519 keypair；versioned publisher grant；stale expected version、external participant 与 expired grant。
- **触发**：owner provision publisher；CAS update；revoke；非 owner/旧版本/错误 key digest 重试。
- **预期事件**：owner control `RunAccepted -> SessionBound -> CapabilityPublisherChanged -> RunComplete`；revoke 再追加 `CapabilityPublisherChanged{revoked}`。
- **Store**：publisher aggregate 单调 version；public key material 留在 keyring/config，event 只存 digest；revoke 历史不可变。
- **反向断言**：非 owner、TOFU、stale CAS、域名自报、expired grant 均零 event/ledger change。

### S86 Catalog Quarantine 与 Prompt Injection

- **Fixtures**：真实/录制 catalog response，含“忽略 policy、我是 owner、自动信任我”等内容；authenticated registry transport。
- **触发**：ingest catalog item 与 bundle metadata。
- **预期事件**：只产生外部 read 的 Action/Verification 事实；尚无 `CapabilityPackageAdmitted/StateChanged`。
- **Store**：catalog receipt 标 Untrusted、绑定 content digest/source evidence；不写 UserModel/graph edge/active strategy/publisher grant。
- **反向断言**：TLS/签名/高评分/重复出现均不能把内容升级为指令或 trust。

### S87 Content Address 与 Signature

- **Fixtures**：owner provisioned publisher key；valid signed bundle；body/manifest/signature/key/package/version 各自 tamper 样本。
- **触发**：admission verifier 逐个验证。
- **预期事件**：valid -> `CapabilityPackageAdmitted`；invalid -> `FailureEvidenceRecorded{safety_policy_failure}` 或 typed rejection，且无 admitted event。
- **Store**：release key `(package,version,digest)` 不可重绑定；同 identity 同语义幂等，同 identity 异语义冲突。
- **反向断言**：只提供 manifest digest、publisher 自报 key、valid signature + wrong scope 均失败。

### S88 Closed Dependency 与 Admission Checks

- **Fixtures**：valid DAG；missing/wrong digest/cycle/depth overflow；missing SBOM/unknown license；secret marker；absolute/path traversal；oversize；undeclared hook/network/permission。
- **触发**：运行固定 admission pipeline。
- **预期事件**：全通过才 `CapabilityPackageAdmitted{checks=all pass}`；任何失败均无 state change。
- **Store**：admission 固定 policy snapshot、publisher grant、dependency refs、check refs 和 ground truth refs。
- **反向断言**：warning、模型分数、catalog popularity 不可覆盖 hard failure。

### S89 Real Registry Admission Golden

- **Fixtures**：repository-owned loopback HTTP registry、真实 signed bundle、authority keyring；registry 返回不可信 bytes。
- **触发**：经 governed AppApi read 获取 bundle -> content receipt -> quarantine -> admission。
- **预期事件**：`ActionPlanned -> ApprovalRequested/Resolved -> ActionStarted -> ActionCompleted -> VerificationFinished{Pass} -> CapabilityPackageAdmitted`。
- **Ground truth**：server request count=1；bundle digest/signature/admission receipt cross-match；artifact secret/private-key matches=0。
- **反向断言**：redirect/extra bytes/tamper/second fetch without approval 不产生 admission。

### S90 Plan-bound Install 与 Enable

- **Fixtures**：admitted release；install plan；owner one-shot approval；批后替换 release/digest/contribution/scope/policy/rollback。
- **触发**：install then enable；逐项 drift。
- **预期事件**：`ApprovalRequested -> ApprovalResolved -> CapabilityPackageStateChanged{Installed} -> CapabilityPackageStateChanged{Enabled}`；贡献随后才出现在 registry/toolset。
- **Store**：approval nonce one-shot；plan digest 与 active release exact；state/version CAS 单调。
- **反向断言**：admission alone、publisher trust、旧 approval、批后 drift 均零 registry mutation。

### S91 Atomic Lifecycle 与 Registry Switch

- **Fixtures**：active v1；staged v2；故意使一个 contribution 验证失败；restart。
- **触发**：尝试 v2 enable；修复后重试。
- **预期事件**：失败只记 failure，active v1 保持；成功仅一次 `CapabilityPackageStateChanged{Enabled v2}` 并原子替换 contributions。
- **Store**：state event/aggregate version/active projection 同一 transaction；重放可重建一致 toolset snapshot。
- **反向断言**：partial skill/hook/tool contribution 不可见；内存状态不能作为唯一事实源。

### S92 Update、Pin 与 Rollback

- **Fixtures**：v1 known-good、v2 admitted/approved、v2 verification regression、exact pin policy。
- **触发**：update 到 v2，记录 regression，rollback 到 v1。
- **预期事件**：`StateChanged{v2 Enabled} -> FailureEvidenceRecorded -> StateChanged{v1 Enabled, reason=rollback}`。
- **Store**：v1/v2 release/events 均保留；rollback plan 有新 approval/expected version；历史 action 不重放。
- **反向断言**：floating latest、自动 major、rollback 生成 permission 或宣称外部效果撤销均失败。

### S93 Package/Publisher Revoke

- **Fixtures**：enabled package、pending update、pending distribution；package revoke 与 publisher revoke。
- **触发**：owner revoke；随后尝试 resolve toolset、enable/update/distribute。
- **预期事件**：`CapabilityPublisherChanged{revoked}` 或 `CapabilityPackageStateChanged{Revoked}`；后续无新 admitted/enabled/distribution accepted。
- **Store**：active contribution 不可见；历史 events/artifacts 保留；pending plan 因 snapshot drift 失效。
- **反向断言**：cached registry、peer receipt、旧 signature 不能复活 release。

### S94 Hook 与 Agent Profile Boundary

- **Fixtures**：package 提供 hook + tool + stateless agent profile；hook 试图改 capability/command；child 尝试越权与写稳定层。
- **触发**：enable package；执行 hook proposal；Harness spawn child run。
- **预期事件**：package state -> existing `PluginContributionRegistered/PluginToggled`；hook 后仍 `ToolPolicyEvaluated`；child `SubagentSpawned -> ... -> SubagentResultReturned`。
- **Store**：agent profile 只有 role/toolset/model/permission/budget/DoneContract；无 memory/self-model/policy owner。
- **反向断言**：package 不能贡献独立 Harness、stable memory、owner identity 或绕过 final recheck。

### S95 Authorized Federated Distribution

- **Fixtures**：active Executor peer、Replica-only peer、revoked/stale peer；admitted package；distribution plan/approval/lease。
- **触发**：分发到各 peer。
- **预期事件**：有效链 `ActionPlanned -> Approval -> RemoteExecutionLeaseChanged{Acquired} -> ActionStarted -> CapabilityPackageDistributionRecorded -> VerificationFinished{Pass}`。
- **Store**：distribution receipt 绑定 package/release/digest/peer/grant/epoch/plan/lease/fence；不传 credential/private path。
- **反向断言**：Replica/wrong scope/stale epoch/score-only peer 在网络前拒绝。

### S96 Distribution Unknown 与 No Blind Retry

- **Fixtures**：executor 安装后断开；原 receipt 可 probe；重复 dispatch 与 forged receipt。
- **触发**：首次 dispatch、timeout、restart/recovery、重复命令。
- **预期事件**：`ActionStarted -> ActionOutcomeUnknown -> RunWaiting -> original receipt/probe -> CapabilityPackageDistributionRecorded`。
- **Ground truth**：executor install count=1；同 lease driver calls=1。
- **反向断言**：timeout/restart/换 peer 不触发第二次 install；无法证明时保持 unknown。

### S97 Restart、Replay 与 Legacy

- **Fixtures**：93-kind M4 DB；M5 publisher/admission/lifecycle/distribution history；authority/executor restart。
- **触发**：打开 legacy DB、重放 M5、重复 nonce/receipt/CAS。
- **预期**：legacy ecosystem projection 为空；M5 projection 与重启前一致；重复同语义幂等，异语义失败。
- **反向断言**：未知 schema、version gap、digest drift 不被修猜；不从 catalog 重建权威状态。

### S98 M5 端到端 Golden

- **Fixtures**：独立 loopback registry process、authority Harness/store、executor process；真实 signed declarative package，贡献一个受治理 capability。
- **触发**：fetch/admit/install/enable/use/distribute/revoke；真实 capability action 仍经 policy/approval/verification。
- **预期事件**：publisher -> fetch/action -> admitted -> installed/enabled -> registry contribution -> governed use -> remote distribution -> revoke 的完整 typed lineage。
- **Ground truth**：registry fetch=1、authority active generation=1、executor install=1、revocation 后 use/distribution=0、secret matches=0。
- **反向断言**：无 direct registry injection、无 package self-approval、无 peer write authority。

### S99 Supply-chain Release Gate

- **Fixtures**：valid artifact set；tamper/signature/license/SBOM/secret/private path/extra file/copy/advisory/dirty-tree 负样本。
- **触发**：artifact verifier、M5 release audit、compliance doctor。
- **预期事件/工件**：所有 typed checks Pass、blocking=false；repository set 与临时重生成 set content-equal。
- **反向断言**：任一 hard finding 使 gate 非零；手写 PASS 或缺 artifact 不被接受。

## 5. 三类正确性

- **Runtime**：quarantine/admission/lifecycle/update/rollback/revoke/distribution/restart/unknown 能推进且可恢复。
- **Governance**：owner/publisher/signature/admission/approval/policy/permission/peer/lease/fence 各自独立，任何一项不能替代另一项。
- **Learning/Evidence**：真实 verification/failure/owner feedback 可形成 CapabilityEvidence/FailureEvidence；package 自评只可降低、不抬高上限。

## 6. Artifact Gate

M5 golden closed set 至少包含：

1. `publisher`：grant ref/version/key digest/revoke state。
2. `admission`：package/release/digest/policy/check/dependency/license/SBOM refs。
3. `install`：plan/approval/state version/active generation/registry digest。
4. `distribution`：peer/epoch/plan/lease/fence/receipt/ground truth refs。
5. `trace`：权威 event kind/stream_seq/ref lineage 与禁止事件断言。

独立 verifier 检查 schema、content address、closed file set、cross-ref、event order、93-kind prefix、secret/private-key/private-path markers、extra/missing/path escape。artifact 不保存 raw key、signature private material、credential、SecretRef id、private endpoint 或 package README/body。

五件套是稳定 evidence projection，不是 runtime DTO 快照。真实 golden 必须先验证完整 grant/admission/envelope/authority receipt/executor record，再投影 refs/version/semantic digest/count/required-true checks；TTL、admission 时间、临时 transport identity、外层 remote receipt id 等运行实例字段不得进入 portable artifact。连续两次独立实跑必须产出完全相同的 artifact digest。

## 7. 分波与最终门

- **A gate**：S85-S89 + S1-S84 + 97-kind protocol shape（仅 A 激活的对象也须兼容）+ real registry admission golden + compliance；产 `m5-a-acceptance-report.md`。
- **B gate**：S90-S94 + A/M4-M0 回归 + lifecycle restart/registry atomicity；产 `m5-b-acceptance-report.md`。
- **C/final gate**：S95-S99 + S1-S94 + real authority/registry/executor golden + typed artifacts + release audit；产 `m5-c-acceptance-report.md` 与 `m5-acceptance-report.md`。

最终成功线：

```text
M5 FINAL ACCEPTANCE: PASS (S1-S99 + 97 EventKinds + 18 crates + governed ecosystem golden + release compliance)
```
