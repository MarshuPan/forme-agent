# M3-B Protocol Compatibility Note

本文固定 M3-B 的 Loop、Coordination、Selection 和 ModelAdaptation strategy spec 兼容边界。它承接 M3-A 的 89 EventKinds；B 波不新增 EventKind、不修改冻结 trait，也不允许 domain spec 承载治理字段。

实施状态（2026-07-18）：本兼容边界已由 M3-B 实现激活并通过 S58-S62，owner review 已通过；89 EventKinds 与 18-crate 图保持不变。

## 1. Domain spec

下列对象均带 SchemaVersion、StrategyVersionRef、scope、content digest 和 compatibility constraints：

- `LoopStrategySpec`：已知 phase set/order、trigger、checkpoint/verification cadence、软 budget profile、failure fallback。
- `CoordinationStrategySpec`：applicability signature、WorkPattern refs、single/multi selection、resource/role weights、checkpoint topology。
- `SelectionStrategySpec`：在 prefiltered candidate refs 上的 evidence features、ordering/tie-break 和 fallback。
- `ModelAdaptationSpec`：ModelProfile capability predicate 与外化 step/verification/checkpoint profile。

这些 spec 以 immutable content ref 进入 M3-A common StrategyCandidate；不向 CandidateCreated 添加新的 domain-specific optional 字段。

## 2. Forbidden fields

domain decoder/validator 必须拒绝任何试图表达以下语义的字段或 unknown extension：

- permission、approval allow/deny、DelegationGrant、AutonomyEnvelope expansion；
- Harness/event/audit/verification disable；
- L3/L5 floor、DisclosurePolicy、SecretRef resolution；
- fixed identity、constitutional rule、arbitrary prompt/source/model weights；
- unbounded turns/time/tokens/subagents 或动态代码/脚本。

未知字段默认拒绝，不用 serde flatten 忽略安全相关扩展。

## 3. Legacy 与 compatibility

- M3-A reader 可读取 B 波 StrategyCandidate envelope，但未安装对应 domain registry 时 spec resolution 显式 unsupported，不能 fallback 到相近 domain。
- seed strategy version 永久可解析；新 spec 可声明 minimum runtime/schema/model/tool compatibility，缺失/不匹配时 run bind fail closed或进入 rollback review。
- old active version 继续按其原 spec digest运行；registry升级不能用相同 StrategyVersionRef 绑定不同内容。
- B 波不改变 M0-M2 Loop/Coordination/Toolset event payload；DecisionTrace 的 M3-A evolution snapshot 已足够引用实际版本。

## 4. Contract tests

- 四种 spec round-trip；unknown/zero/empty/overflow/unbounded/duplicate phase 或 weight 拒绝。
- forbidden governance field fixtures 全部 fail closed。
- M3-A common DTO/89-kind snapshot不变；M0-M2 regression decode 全绿。
- same version/different digest 拒绝；unsupported domain 不 fallback；seed fallback 只能经显式 rollback/diagnostic。
- Selection spec 不能编码未过滤 resource；Coordination role weight 不能改变 child permission/toolset/budget；Loop spec 不能超过 hard runtime limits。
