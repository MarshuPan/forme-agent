# M4-B 只读复制与跨设备控制验收报告

- 验收日期：2026-07-22
- 验收范围：M4-B S75-S79，并永久回归 S1-S74
- 权威规格：`docs/requirements/12-m4-verification-strategy.md` S75-S79；`docs/prd/21-m4-program-prd.md` §4
- 结果：**PASS - 已纳入 M4 最终 owner 验收候选**

## 1. S75-S79 逐场景证据

| 场景 | 可执行证据 | 事件或状态序列断言 | 反向断言 | 结果 |
|---|---|---|---|---|
| S75 Filtered replication | store `s75_s76_replication_is_filtered_contiguous_atomic_and_idempotent`；Gateway `s75_s78_s82_peer_ingress_is_channel_bound_before_harness`；golden replication manifest | authority 导出 content-addressed `ReplicationBatch`，authenticated ack 后才 CAS 追加 `ReplicationCheckpointAdvanced{peer,aggregate,from,to,batch,redaction,epoch}` | workspace/channel scope 外事件、secret/raw/private ref 被过滤；Replica 不能 append authority event；ack 的 channel peer、batch、digest、cursor 任一不匹配均零 checkpoint | PASS |
| S76 Resume/idempotency/cursor CAS | store S75/S76；`s76_replicad_process_is_atomic_restart_idempotent_and_rejects_reorder_or_tamper` | `(peer,aggregate)` cursor 只从完整 batch A 前移，再接 B；apply/projection/cursor 原子提交；restart 从 durable cursor 接续 | B-before-A、gap、tamper、stale cursor、同 batch 不同语义拒绝；duplicate semantic batch 零第二应用；wall clock 不参与排序 | PASS |
| S77 Epoch/single writer | store S70/S71/S77；Harness S70/S71 drift/restart tests | owner control 的 register/revoke 单调推进 federation version/authority epoch；旧 lease 只可 fenced/expired；权威 event 仍由单一 store 分配 `stream_seq` | stale executor/replica receipt、伪 authority envelope、peer 自升 epoch、并发 stale CAS 全部零权威写；不存在 replica promotion 或多数投票 | PASS |
| S78 Revoke/retention honesty | Harness `s78_s82_retention_is_honest_and_device_signals_have_one_authority_decision` 与 restart test；Gateway channel-bound ingress test | `FederatedPeerRevoked{in_flight}` 后新 lease/export=0；retention 为 `NotRequested -> Requested -> Verified|Unknown`，历史事件不改写 | receipt 必须来自已 provision 的 Replica transport identity；revoked peer 只有 receipt-only binding，不能恢复 general session；channel/receipt peer、request/epoch/digest 不符或缺 receipt 均不得显示 deleted | PASS |
| S79 Cross-device owner control | Gateway `s79_owner_client_requires_peer_channel_and_independent_owner_before_harness_control`；Harness `s79_cross_device_owner_control_is_dual_bound_one_shot_and_fences_late_receipts` 与 restart test；protocol nonce/digest test | peer channel binding 与 configured owner auth 是两个独立证明；有效 control 才进入既有 `ApprovalRequested -> ApprovalResolved`/cancel 链，nonce/plan/expiry/owner 均绑定 | 单个自报 AuthContext、channel/session mismatch、executor/replica/external response、重放/过期/第二响应均零写；authority 在审批前重启使旧 approval driver_calls=0，必须新 run/plan/approval | PASS |

## 2. 复制事实与 ingress 边界

- `ReplicationBatch` 是 authority 输出的 closed typed envelope；ReplicaMode 只实现 apply/read projection，不实现 `EventStore::append`。
- authenticated ingress companion 固定接收 `authenticated_peer + batch + ack + expected`。Harness/Store 同时比较 channel peer、active Replica grant、epoch、export ledger、batch content digest、projection digest 与 cursor，不能只信任 payload 自报摘要。
- cursor key 固定为 `(peer,aggregate)`，aggregate 内排序只认 authority `stream_seq`；apply/projection/cursor/checkpoint ledger 在同一 SQLite immediate transaction。
- OwnerClient transport authentication 只开控制通道，独立 owner authentication 才能批准/取消；设备可信度、Replica/Executor role 和 push payload都不产生 owner permission。
- nonce、device signal、retention 和 remote recovery 使用 authority-local durable typed ledger；这些 ledger 不分配事件序号、不授予权限，也不复制给 Replica。

## 3. 真实复制证据

三进程 golden 的 repository artifact 记录：Replica `peer:golden-replica` 对 aggregate `run:s83-federated-golden` 从 cursor 0 原子推进到 23；23 个 event digest 顺序闭合，redaction profile 固定，checkpoint 由 authority 接受 authenticated ack 后追加。closed artifact receipt 为 `sha256:c24a84ef85d918654258a3108a5bf8bd5523361271e76dfdad529ce106319022`，secret scan 为零。

## 4. 回归与工程门

`tools/verify-m4.ps1` 先调用 `tools/verify-m3.ps1 -RegressionOnly`，因此 S1-S69 不是抽样；随后精确执行 S70-S79、Replica 独立进程、mTLS executor、Gateway channel binding、authority restart recovery、artifact positive/negative fixtures，并在最终阶段再运行完整 workspace tests、Python gates、strict Clippy、原创合规和 clean-tree audit。

## 5. 结论

M4-B 已证明跨设备没有引入第二事实源或第二 owner：复制是 authority 到只读 Replica 的 scope/redaction 投影，控制命令必须同时具备受认证 peer channel 与独立 owner principal，revoke/retention 不夸大远端删除事实。**M4-B 自验收 PASS。**
