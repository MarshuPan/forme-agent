# M4-A 协议兼容性冻结

状态：实现冻结。本文只解释 `canonical-contract.md` §26、`07-m4-federated-runtime-architecture.md` 和 `prd/21` 已批准的 A 波 wire 行为，不产生第二套口径。

## 1. EventKind 前缀

- `EventKind::ALL.len() = 93`。
- 索引 `0..89` 与 M3 严格同序、同名、同 payload mapping。
- 索引 `89..93` 依次为 `FederatedPeerRegistered`、`FederatedPeerRevoked`、`RemoteExecutionLeaseChanged`、`ReplicationCheckpointAdvanced`。
- transport connect、packet、heartbeat、receipt fetch 不新增事件；只有 authority 接受后的稳定事实进入上述四类或既有 action/verification 事件。

## 2. Additive wire surface

- `Source::OwnerControl`、`BackendKind::Remote`、`ActionParameters::Remote(Box<RemoteActionSpec>)`、`ConfigCheck::Federation` 和 `AuthMethod::FederatedPeer` 只在旧枚举尾部追加。
- `SessionBound.federation_snapshot`、`ActionPlanned.remote_placement`、action payload 的 remote lease/receipt、`DecisionTraceRecorded.federation_snapshot` 均为 serde optional default；旧事件解码为 `None`。
- legacy `None` 只能 replay，不能取得 remote permission、lease 或 peer role。
- M4 stable DTO 都带非零 `SchemaVersion`；identity/version/epoch/fence/cursor/ref 不用裸字符串互换。

## 3. TLS command schema

`RemoteWireEnvelope` 是 closed schema，绑定 request、authority、peer、nonce、有限 expiry、typed command 和全内容 digest。command 只允许 Dispatch、Probe、Cancel、Receipt；Dispatch 必须绑定同一 placement/grant/epoch/fence 的 Acquired lease。`RemoteWireReply` 同样 closed 且 content-addressed。

相同 request/dispatch id 与相同语义幂等；相同 id 不同 digest、未知字段、未知 enum、过期、zero epoch/fence、recursive Remote、中央 SecretRef 或 host path 全部 fail closed。worker receipt 不含 ground truth；authority verifier 才能生成 `RemoteExecutionReceipt`。

## 4. Store/open/replay

- M4 migration 只追加 federation/lease/export/replica ledger；旧 89-kind SQLite store 可打开，存储初始 epoch 为 0，registry version 为 0，active grants/leases/checkpoints 为空；首次 owner registration 在同一 CAS 提交中将 epoch 提升为 1。
- federation、run、evolution aggregate version 互不混用。
- register/revoke/lease/checkpoint 都以 expected version CAS；冲突写入 event/ledger/projection 均为零。
- 未知 schema、version gap、epoch rollback 或 digest mismatch 使 federation fail closed，不影响可证明安全的 local M0-M3 replay。

## 5. A 波验证定位

协议测试：`crates/protocol/tests/m4_contract.rs`。Store/remote/harness 场景证据归入 S70-S74；真实 TLS golden 不得以 mock、同进程 shortcut 或手写 PASS 代替。
