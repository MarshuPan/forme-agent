# M4-C 连续性、三进程 Golden 与发布门验收报告

- 验收日期：2026-07-22
- 验收范围：M4-C S80-S84，并永久回归 S1-S79
- 权威规格：`docs/requirements/12-m4-verification-strategy.md` S80-S84；`docs/prd/21-m4-program-prd.md` §5
- 结果：**PASS - M4 工程自验收候选，等待 owner 完整验收**

## 1. S80-S84 逐场景证据

| 场景 | 可执行证据 | 事件或状态序列断言 | 反向断言 | 结果 |
|---|---|---|---|---|
| S80 Authorized placement | coordination `s80_filters_authority_before_ranking_and_score_never_authorizes`、`s80_no_candidate_returns_a_governed_no_placement_decision`；Harness `s80_authority_filters_candidates_and_records_selection_before_action_plan`；protocol placement test | `SessionBound{evolution,federation} -> ResourcePlanned -> DecisionTraceRecorded{candidates,evidence,selection} -> ActionPlanned{chosen peer}`；filter 先于 rank，选择 ref/digest 固定进 plan | revoked/expired/stale/wrong-role/wrong-scope/managed-deny/failed-evidence candidate 在排序前消失；score、历史成功和 M3 strategy 都不能产生 permission；空集只 ask/local/stop | PASS |
| S81 Verified handoff | Harness `s81_handoff_requires_a_verified_checkpoint_current_snapshot_and_new_segment`；protocol handoff test | segment A 只有 `VerificationFinished{Pass}` 后才能登记 durable checkpoint；segment B 是新 `RunAccepted -> SessionBound{new snapshots}`，重新生成 plan/approval/lease | 缺 artifact、failed verify、stale snapshot、mid-action、复用 approval/lease、扩大 toolset/budget、revoke/cancel 后接续均拒绝；模型 hidden state 不迁移 | PASS |
| S82 Global scheduler/attention | Harness `s82_two_owner_devices_share_one_authority_scheduler_budget_and_cancel`、S78/S82 与 restart tests；Gateway channel ingress test | 两设备信号先经 authenticated channel 进入 authority；intention CAS 最多产生一个 `ProspectiveIntentionResolved{fired} -> RunAccepted{Schedule}`；foreground/budget/cancel 由 authority 单点裁决 | payload 自报 peer、channel/signal mismatch、重复/offline tick、device clock、partition continuation 不创建第二 run；cancel/budget terminal 后 action count 不再增长 | PASS |
| S83 Three-process federation golden | ignored exact `s74_s83_three_process_tls_unknown_recovery_replication_revoke_and_artifacts`；`forme-executord`、`forme-replicad` 独立构建/启动；repository 与临时五件套均由独立 verifier 检查 | 三个独立进程完成 `PeerRegistered* -> RunAccepted/SessionBound -> approval/gate/Lease{Acquired}/ActionStarted -> ActionOutcomeUnknown -> recovery/ActionCompleted/Lease{Released}/Verification{Pass} -> ReplicationCheckpointAdvanced -> PeerRevoked` | authority driver=1、mutation ordinal=1、replica cursor=23、secret matches=0；原 receipt 恢复后不重放；revoke 后无第二 `ActionStarted`；额外文件/tamper/path escape 拒绝 | PASS |
| S84 Federation/release security | eval 三个 artifact negative tests；Gateway/execution/store/harness threat fixtures；`tools/tests/test_m4_release_audit.py`；M3 receipt verify；M4 release audit generate/verify/compare；S22 doctor | 8 个 typed checks 固定为 `release_tree -> license_notice -> direct_dependencies -> borrowing_records -> upstream_copy -> acceptance_artifacts -> vulnerability_audit -> federation_threat_profile`，每项保存 tool/version/input digest 且 `blocking=false` | dirty tree、缺 NOTICE/登记/pin/corpus/scanner、copy 命中、secret/private endpoint、artifact 漂移、impersonation/replay/stale fence、可达 advisory 或 fixture 缺失均 blocking | PASS |

## 2. S83 Repository Golden

闭合工件目录：`docs/acceptance/m4-artifacts/`

| 工件 | 内容寻址 digest | 关键事实 |
|---|---|---|
| Federated peer manifest | `sha256:151420f3780687d4a1476e7adbc781a4a4a717b6f30afff290860c583daad8bb` | Executor grant、transport identity digest、scope、epoch=3 |
| Authority remote receipt | `sha256:5360463e64184e265899bbc7d66d0fbe91f6f04022041b63d91e5fba27d3b6ff` | plan/operation/lease/executor/ground truth 绑定，authority verified |
| Replication manifest | `sha256:95153e3638fafc96a474c14a4ff035133af264d44fdd5d597dc3ca5bfbe4a119` | cursor 0 -> 23，23 个有序 event digest，redaction 固定 |
| Federation golden report | `sha256:ebb9638c1badc355005b730d7ccf3f950f01e3c12482555c9987291a4de7cc21` | driver=1、mutation=1、replica cursor=23、secret=0 |
| Federated trace manifest | `sha256:f0fbb6037ca563a6682ee71fb6d627488882fba0b0a879fdb240570851b15b1f` | owner/approval/lease/action/verification/replication/recovery/revoke refs |

封闭五件套 receipt：`sha256:c24a84ef85d918654258a3108a5bf8bd5523361271e76dfdad529ce106319022`。repository artifact 与每次统一门临时重生成 artifact 都必须独立 verify；二者不要求随机 identity/digest 相同，只要求 schema、lineage、行为断言与安全约束相同。

## 3. S84 Clean-tree Release Audit

- typed receipt：`sha256:eaf892bf94055c104daf352612fad7cb48a771162771bbcc5afc9ed2a2ed6787`
- release root digest：`sha256:6a10d95ede9579be7128100cadee5f7ac45901712e4cefdbb7a15d4459954c0f`
- federation threat input digest：`sha256:cb45bd93631ae75272e66d94724eef80886cdeecd1dc2faddddb6eb152f56823`
- audit version：`forme-release-audit/4`
- checks：8/8 Pass，`blocking=false`

该 receipt 从第一份干净的 M4 implementation/report/index 提交生成；artifact 目录与四份自引用报告不进入自身 root digest。封板提交只增加被排除的 receipt 与报告补录，因此最终 clean tree 可重新生成同一 typed body 并做精确 compare。

`cargo-audit 0.22.2` 对锁文件中的 `quick-xml 0.30.0/0.39.4` 报告 `RUSTSEC-2026-0194/0195` 四个命中。审计没有忽略它们，而是逐项运行当前 release target 的反向依赖检查；四项在 `x86_64-pc-windows-gnu` 均不可达，因此保留为 `TARGET_UNREACHABLE_ADVISORY` observations。release target 或 feature 变化会重新计算，任何当前 target 可达 advisory 仍为 blocking finding。

## 4. 最终工程门

统一门执行：S1-S69 永久回归、29 个 M4 exact contracts、18-crate/精确依赖检查、独立 executor/replica 构建、三进程 mTLS golden、临时与 repository artifacts 双重验证、S38/S68 真实 Chrome/Edge goldens、fmt/check/strict Clippy、348 个非忽略 workspace tests、21 个 Python tests、原创合规 doctor、whitespace gate、M3 historical receipt verify，以及 M4 receipt generate/verify/current compare。

最终成功行固定为：

```text
M4 FINAL ACCEPTANCE: PASS (S1-S84 + 93 EventKinds + 18 crates + three-process federation golden + release compliance)
```

## 5. 残余边界

- golden 使用真实独立进程与 mutual TLS loopback，证明 transport/进程/副作用/恢复链，不宣称公网部署、NAT 穿透、自动发现或云级高可用。
- M4 仍是单 owner、单 authority、有限静态 peers；Replica 不是备选 writer，Executor 不是第二 Agent。
- remote “exactly once” 仅指 one-shot lease + durable dispatch claim + fixture driver 的观测结果；网络未知结果仍显式 `ActionOutcomeUnknown`，不泛化为分布式 exactly-once 保证。
- retention verified 只证明匹配 request 的已认证 Replica receipt，不证明对任意远端介质的物理擦除。
- release audit 是工程合规证据，不替代法律意见，也不执行 push、merge 或公开发布。

## 6. 结论

M4-C 把 placement、checkpoint handoff、全局 attention/scheduler、三进程真实运行和 release threat audit 接到 M4-A/B 的同一权威链上。没有第二事实源、远端自治扩权、盲重试、secret 下发或未认证 ingress。**M4-C 自验收 PASS，整个 M4 已进入 owner 完整验收候选。**
