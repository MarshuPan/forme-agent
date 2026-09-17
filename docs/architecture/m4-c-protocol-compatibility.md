# M4-C 协议兼容性冻结

状态：实现冻结。C 波不新增 EventKind，placement/checkpoint/scheduler 通过 versioned DTO 和既有 event 扩展点表达。

## 1. Authorized placement

`FederatedExecutorCandidate` 只描述 owner 已授权集合中的候选。固定过滤顺序为 grant/role/scope/capability/expiry、managed policy、schema/profile、health freshness、result-led capability/failure evidence，最后才使用 M3 pinned selection score 排序。

`FederatedPlacementDecision` 必须保存完整 candidate trace、deny reason、chosen peer、placement ref 和 digest。被过滤 peer 即使 score=10000 也不能 chosen；无候选只可 local fallback、ask、prepare 或 stop。

## 2. Verified checkpoint handoff

`FederatedCheckpointArtifact` 必须携带 DoneContract、`VerificationOutcome::Pass` 事件、portable artifact refs、spent/remaining budget、external-effect lineage、policy/tool/model/evolution/federation snapshot 并通过内容 digest。

`FederatedHandoffPlan` 的 next run 必须不同于 source run，且重新固定 placement/evolution/federation/budget。对象没有模型隐状态、旧 approval 或旧 lease 字段；mid-action 与 unverifiable checkpoint 不可 handoff。

## 3. Global signals

`FederatedDeviceSignal` 绑定 peer/session/nonce/expiry/content digest。device observed time 只作诊断；authority 的 event/CAS 顺序决定 intention claim、foreground、AttentionBudget、cancel 和 budget。duplicate/replayed/offline signal 不得产生第二 scheduled run。

## 4. Legacy/default

没有 federation snapshot 的 M2/M3 历史 run 可 replay，但不能原地续成 remote segment；必须新建 `RunAccepted -> SessionBound`。C 波所有 DTO 缺席都不生成 placement、handoff 或调度权限。
