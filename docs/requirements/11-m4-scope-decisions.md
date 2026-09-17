# M4 单 Owner 联邦运行时范围决策

本文承接 `02-capability-requirements.md`、`03-foundation-scope-decisions.md`、`09-m3-scope-decisions.md`、M3 最终验收报告和 `architecture/canonical-contract.md` §26，定义 forme M4 的冻结范围。M4 是**单 Owner 联邦运行时**：让已经能跨系统行动、能受控进化的内核，把执行、只读副本、审批和长任务连续性延伸到受认证的远端 peer，同时保持一个 owner、一个权威 Harness 和一个权威事件顺序。

M4 不是多租户云平台，也不是把远端 worker 变成另一个 Agent。它要证明：**forme 可以跨进程、跨设备执行和接续工作，但行动主权、事实写入权、授权、策略和认知仍留在唯一权威内核。**

本文已通过 owner review 并冻结；其实现与验收只能落实本文，不得在模块内另造权威口径；工程结果已合入 main。

## 1. M4 目标

1. 建立受认证的 peer lifecycle 与 `RemoteExecutorBackend`，远端真实动作继续经过 Harness、Policy、Approval、CompetenceGate、plan digest 和 verification。
2. 建立单权威、多 peer 的可恢复 event replication；按 scope/redaction 输出，只允许权威端分配 `stream_seq` 和稳定事实。
3. 建立跨设备 owner approval/cancel、远端 lease/fencing、断线后的 unknown-outcome 裁决和 checkpoint handoff。
4. 至少一个 repository-owned golden 由三个独立进程完成：权威 Gateway/Harness、远端 executor、只读 replica；产生真实副作用、断线/恢复和可离线复核的 typed artifacts。
5. S1-S69、18-crate 图、89 EventKinds、M0-M3 全部治理和 M3 release baseline 永久不回退。

## 2. 范围判据

- **单脑、单权威先于分布式广度**：peer 是 owner client、executor 或 replica，不拥有独立认知、active strategy、permission 或稳定事实写权。
- **网络只增加风险，不增加授权**：远端动作默认至少 L3；不可逆/高影响仍逐动作 L5。transport authentication、设备声誉或历史成功不能生成 permission。
- **结构不缺席、拓扑可收**：首版只支持一个 authority、有限静态 peer、有限远端 operation 和中心到 peer 的复制，不做自动发现、弹性集群或通用云执行。
- **分区时安全优先**：无法确认 authority epoch、lease、plan digest、revocation 或 action outcome 时 fail closed；不得用 lease timeout 推断动作未发生。
- **Git/事件/地面真值可携带**：跨设备身份、receipt、replication batch、trace 和 eval 都以 versioned ref/digest 表达，不以地址、主机名、时钟或本机路径充当 identity。
- **潮线继续生效**：远程传输、执行驱动和设备发现放在薄 trait 后；forme 做深的是授权接缝、fencing、事实捕获、恢复与审计，不自建调度云或密码学协议。

## 3. 范围决策

| 编号 | 主题 | M4 决策 | 不进入 M4 核心的部分 |
|---|---|---|---|
| D76 | 阶段定义 | M4 是单 Owner 联邦运行时；M0-M3 的 18-crate 运行时、S1-S69、canonical §1-26 和 M3 封板 receipt 是冻结基线。 | 重做 Harness、第二个大脑、第二事实源、以分布式为名弱化旧门。 |
| D77 | 身份与权威 | 仍只有一个 owner principal 和一个 authority epoch。peer role 仅为 OwnerClient、Executor、Replica，可组合但逐项授权；peer 永远不是 owner。 | 多租户、组织成员/角色、peer 多数票、设备自动成为 owner。 |
| D78 | 协议与依赖 | M4 protocol 只 additive；不新增内部 crate 或依赖边。新对象带 `SchemaVersion`，legacy 89-kind store 可安全打开且 federation projection 为空。 | 模块私有 wire protocol、adapter 私有授权库、remote crate 反向依赖。 |
| D79 | 传输安全 | peer transport 必须认证、加密、防 replay，并绑定 owner 预置的 transport identity digest；实现使用成熟库，不自研密码学。地址不是 identity。 | 首次请求 TOFU、匿名公网注册、明文 transport、自签内容自行声明 trusted。 |
| D80 | Peer lifecycle | owner 显式创建窄 `FederatedPeerGrant`，绑定 role/scope/capability/TTL/epoch；注册、更新、撤销均落事件。撤销立即阻止新 lease/export/approval。 | 自动发现即注册、无限 TTL、peer 自改 role/scope、删除撤销历史。 |
| D81 | RemoteExecutorBackend | `execution` 增加薄 `RemoteExecutorBackend` + transport/driver trait；只执行 lease 中的 typed operation。worker 无模型 loop、policy owner、event writer 或 stable memory。 | 通用远程 shell 云、远端 Agent 身份、worker 自选工具/模型/策略。 |
| D82 | Plan/Lease/Fencing | 每次远端动作绑定 ActionIntent、immutable plan digest、executor profile、peer grant、authority epoch、one-shot lease、fence token 和 expiry；worker 与 authority 双重重查。 | 只凭 job id 执行、lease 自动扩大 scope、过期后继续、stale worker 写回。 |
| D83 | Outcome 与重试 | dispatch 后无可信终态只能 `ActionOutcomeUnknown`；同 lease 仅可 probe/接收原 receipt，不能重新执行。新执行必须新计划、重新审批且证明不会重复副作用。 | exactly-once 宣称、timeout 即重试、换 executor 盲重放、伪造 completed。 |
| D84 | Secret 边界 | credential material 不穿过 control/event/trace channel。远端需要 credential 时只解析 executor 本地、owner 预置且 peer/scope-bound 的 SecretRef；receipt 不回显 ref id 或 secret。 | 中央下发明文 key、把 token 放 lease/URL、同步 secret store、artifact 保存证书私钥。 |
| D85 | 输出与防投毒 | transport identity 可认证发送方，但 remote stdout/file/API/page 内容仍是 `UntrustedData`；由 authority Harness 盖 provenance，Verifier 才能形成有限结果证据。 | “来自我的设备”即 trusted instruction、worker 自报 ground truth、远端内容改 policy/graph/active strategy。 |
| D86 | 权威事实源 | authority store 是唯一 append writer 和 `stream_seq` 分配者。peer 只能提交 typed command/receipt 或消费 replication batch；所有接受事实由 authority 追加。 | peer 直写 event log/stable projection、last-write-wins、按 wall clock 合并。 |
| D87 | 多 Peer 复制 | M4-B 支持有限静态 peer 的 content-addressed incremental batch、per-peer/per-aggregate cursor、scope filter、redaction、idempotency 和断线续传；aggregate 内只认 authority `stream_seq`。 | 多主 CRDT、任意拓扑 gossip、peer-to-peer 转发、跨 owner 数据合并、用 device clock 拼跨 aggregate 伪全序。 |
| D88 | Revocation/Retention | revoke 后停止新 export 和 action；已复制数据按 retention policy 形成删除请求/receipt。只有已 provision 的 Replica identity 获得 retention-receipt-only binding，且 request/epoch/digest lineage 同时通过，才可标记 verified；该入口不恢复 revoked session。 | 远程擦除的虚假保证、payload 自报 peer 即 trusted、receipt 重新激活 peer、撤销重写历史、retention 绕过审计。 |
| D89 | 跨设备审批/取消 | OwnerClient peer channel binding 与 configured owner authentication 是两个独立证明；approval 仍 one-shot、plan-bound、nonce/expiry-bound。取消进入 authority，再传播到 lease。authority 在审批前重启会失效旧等待态，继续必须新 run/plan/approval。 | 用单个自报 AuthContext 同时冒充 channel+owner、replica/executor 审批、push 点击直接授权、离线缓存 grant 重放。 |
| D90 | Placement | Coordination/M3 Selection 先过滤 active peer grant、health freshness、scope、policy 和 capability evidence，再在允许 executor 中排序；选择不授权。 | 自动注册/扩容、score 覆盖 deny、失败后静默换机执行。 |
| D91 | Checkpoint handoff | 长任务只能在 durable verified checkpoint 之间 handoff；每段是新 Harness run、新 pinned EvolutionSnapshot 和新 lease。进行中副作用不迁移。 | mid-action 迁移、复制模型隐状态、跨 checkpoint 扩权、无 DoneContract 接续。 |
| D92 | 调度与注意力 | authority scheduler 是 background/foreground/AttentionBudget 的唯一裁决者；多个 owner device/tick 只经 authenticated OwnerClient channel 提交信号，不各自创建重复任务。 | payload 自报 peer、每设备独立 cron 写事实、后台抢前台、分区时无限自治。 |
| D93 | 可观测与工件 | peer grant、authority epoch、lease/fence、plan digest、receipt、per-aggregate cursor、redaction、unknown outcome 和 revoke lineage 必须可离线复核且无 secret/private endpoint。 | 只保留 worker 日志、把网络连接成功当完成、手写 PASS artifact。 |
| D94 | EventKind | 新增 4 个末尾 additive 事实：`FederatedPeerRegistered`、`FederatedPeerRevoked`、`RemoteExecutionLeaseChanged`、`ReplicationCheckpointAdvanced`；89 -> 93，前 89 严格同序同名。 | 为每个 transport/worker 状态新增事件、重排旧 taxonomy。 |
| D95 | 交付波次 | M4-A trust plane/remote execution -> M4-B replication/owner devices -> M4-C distributed continuity/golden。每波全回归、报告、owner review 后才进入下一波。 | 三波并行激活、没有 fencing/unknown-outcome 就做 handoff。 |

## 4. 三波交付范围

### M4-A：Peer Trust Plane 与远端执行

- FederatedPeerGrant、owner-provisioned enrollment、transport identity、epoch 与 revocation。
- RemoteExecutorBackend、plan-bound lease、fencing、executor-local SecretRef 和 authenticated receipt。
- partition/timeout/duplicate receipt/unknown outcome 的无盲重试恢复。
- S70-S74 + S1-S69 回归；至少一个双进程真实网络/真实 mutation golden。

### M4-B：多 Peer 复制与跨设备控制

- authority -> finite peers 的 filtered/redacted incremental replication。
- per-peer/per-aggregate cursor、batch digest、idempotency、offline resume、authority epoch 与 stale-peer fencing。
- OwnerClient approval/cancel；peer revocation、retention request/receipt 的诚实边界。
- S75-S79 + A/M3/M2/M1/M0 全回归。

### M4-C：分布式长任务连续性

- authorized executor placement 与 evidence-led health/capability filtering。
- verified checkpoint 间 handoff、foreground priority、global budget/cancel/revoke。
- authority + executor + replica 三进程 end-to-end golden，包含断线、恢复、撤销和无重复副作用。
- M4 typed artifacts、release/security audit、S80-S84 + 全回归。

## 5. M4 非目标（留 M5 或更后）

- 多租户/多组织 identity、RBAC、组织级 workflow、公开或匿名公网 control plane。
- 多主写入、CRDT/gossip、跨 owner 数据合并、无中心自治。
- connector/plugin/skill/MCP/agent marketplace、远程自动安装或供应链分发平台。
- 自主 Agent team/spawn network；子代理仍是唯一大脑派出的无认知 child run。
- fixed/constitutional 自动修改、在线改源码/tests/verifier/model weights、自动生成并自批 regression suite。
- 自动扩大 permission/trust/grant/envelope、standing L5、分区期间无限 L4/L5 自治。
- 自研 TLS/密码学、硬件 attestation、通用容器云、自动扩缩容或无限 executor fleet。

## 6. 成功指标

- **主权**：非 authority event append = 0；peer 生成 permission/grant/active strategy = 0；peer 冒充 owner = 0。
- **执行**：未经有效 grant/plan/approval/lease/fence 的 remote driver call = 0；unknown outcome 盲重试 = 0；重复副作用 = 0。
- **安全**：明文 secret/私钥/私有 endpoint 进入 event/trace/replication/artifact = 0；authenticated remote content 被当指令 = 0。
- **一致性**：同一 aggregate 的 event 只认 authority `stream_seq`；cursor gap/lost update/epoch rollback = 0；revoke 后新 lease/export = 0。
- **连续性**：checkpoint handoff 不丢 DoneContract/evolution/policy/plan lineage；foreground/cancel/budget 跨设备保持一个裁决结果。
- **回归**：S1-S69、全部既有测试、fmt、strict clippy、19 个 Python gate 和 compliance 每波全绿。

## 7. M4 完成定义

1. S1-S69 全绿，`12-m4-verification-strategy.md` 的 S70-S84 全绿。
2. A/B/C 每波有 protocol compatibility note、事件序列、store/projection、反向断言、typed artifact 和验收报告。
3. 18-crate 图无新增/反向边；EventKind 仅末尾 additive 到 93；新协议对象 versioned，legacy replay 完整。
4. 三进程 golden 产生真实远端副作用、可证明 ground truth、partition/recovery/revoke 证据，且无重复副作用。
5. fmt、check、strict clippy、workspace tests、19+ Python gates、S22 doctor、M4 release/security audit 全部 PASS。
6. 输出 `docs/acceptance/m4-acceptance-report.md`，证明跨设备/进程没有削弱 M0-M3 的行动主权与事实边界。

## 8. 下游约束

- canonical §26 必须与本文一起 owner review；同名 federation/peer/lease/receipt/replication 判断只以 canonical 为准。
- `architecture/07-m4-federated-runtime-architecture.md` 与 `prd/21-m4-program-prd.md` 只能实现 D76-D95，不得扩成 M5。
- `tools/verify-m4.ps1` 必须复跑 S1-S69 的场景/协议/artifact 回归，并对当前 M4 tree 生成新的 release receipt；历史 M3 receipt 只做不可变 artifact 验证，不能要求它等于已变化的 M4 tree。
- EventKind、crate edge、authority writer、L3/L5、owner identity、transport trust 或 secret 边界变化必须单独 canonical change review。
