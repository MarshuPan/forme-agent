# M4-B 协议兼容性冻结

状态：实现冻结。B 波不新增 EventKind，继续保持 93 项及前 89 严格前缀。

## 1. Replication 类型边界

- M4 使用 `ReplicationCursor/Batch/Ack/ReplicaApplyReport`；不得与 M2 `SyncPeer/SyncWriteBatch` 互传，也不得调用 `into_authoritative_write`。
- cursor key 固定为 `(FederatedPeerRef, RunId)`，位置只认 authority aggregate `stream_seq`；device time、timestamp、ULID 不参与排序。
- batch from/to 必须同 peer、aggregate、epoch，事件逐项连续，content digest 覆盖 grant、cursor、redaction 和全部 envelope。
- equal duplicate 幂等；gap、B-before-A、stale expected cursor、same id/new semantics、tamper、overflow、wrong peer/scope/epoch 均零推进。

## 2. Read-only replica

ReplicaMode 使用独立 schema 和 `ReplicaProjectionStore`，不实现 `EventStore`、`VersionedEventStore` 或 `EvolutionEventStore`。batch、projection 和 cursor 在一个事务提交；crash-before-commit 全回滚，crash-after-commit 重放返回 Duplicate。

authority-only federation control payload、raw input/model/tool/action output、SecretRef、credential、private endpoint/path 必须 omit 或 redacted。脱敏 envelope 保留 event id/kind/source stream_seq 和安全 digest/reason，不伪造完整内容。

## 3. OwnerClient 与 retention

`FederatedControlEnvelope` 只证明 message binding，不证明 transport identity。Gateway 必须先把 TLS 观测到的 identity 绑定成 opaque peer channel context，并以独立参数验证 configured owner principal；channel peer/session 与 envelope 必须一致。nonce 是 one-shot，过期、replay、wrong role、wrong owner、plan digest drift 全部在 Harness control 前拒绝。

replication ack、device signal 与 retention receipt 均必须从上述 authenticated channel context 进入 Harness；payload 自报 peer/identity 不构成认证。retention 状态仅为 `NotRequested -> Requested -> Verified|Unknown`。只有 authenticated Replica channel + digest/request/epoch lineage 可形成 Verified；offline/revoke/timeout 不等于删除，且历史 event/artifact 不重写。

## 4. Legacy/default

旧 M2 sync reader 和 M4-A remote run 均保持可 replay。B 波无字段缺省可扩大 scope；不存在 replication/OwnerClient DTO 时只代表功能未激活。
