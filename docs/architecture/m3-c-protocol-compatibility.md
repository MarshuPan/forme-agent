# M3-C Protocol Compatibility Note

本文固定 M3-C 的 StrategyMemory、AgentSelf、Partnership、TrustDelegation、Proactivity 和 Communication strategy 兼容边界。C 波继续保持 89 EventKinds，复用 M3-A common candidate/evaluation/activation/rollback envelope。

实施状态（2026-07-18）：本兼容边界已激活并通过 S63-S69、legacy/unknown/forbidden-field contracts 与完整 M3 final 回归；89 EventKinds 与 18-crate 图保持不变。

## 1. Domain payload

每个 domain spec 带 SchemaVersion、scope、content digest、evidence requirements、expiration/decay 和 rollback policy：

- `StrategyMemorySpec`：conflict/freshness/decay/reevaluation policy，只引用结构化 evidence refs。
- `AgentSelfStrategySpec`：scoped reliability/gap aggregation 与 evidence windows；self observation 只可降低 ceiling。
- `PartnershipStrategySpec`：协作/纠偏/表达偏好，不含 identity/permission/grant。
- `TrustDelegationStrategySpec`：降级/建议阈值，只产 recommendation，不含 grant body 或 approval bypass。
- `ProactivityStrategySpec`：trigger/value/AttentionBudget/delivery preference，不能扩大 ObservationScope。
- `CommunicationStrategySpec`：表达/summary/surface preference，不能指定未授权 recipient、DisclosurePolicy allow 或 L3/L5 override。

## 2. Fixed 与 authority boundary

- fixed identity、No-real-consciousness-claim、Loyalty-to-owner、permission、approval、L5、DisclosurePolicy 和 owner identity 不可出现在可变 payload；unknown field fail closed。
- AgentSelf/Partnership/Trust 自报的 owner confirmation、CapabilityEvidence、VerificationResult 或 provenance 无效；只接受权威 event refs。
- external/Untrusted content ref 可作为待判断输入引用，但不能成为 sole ground truth、rubric、activation command 或 graph edge source。
- TrustDelegation promotion/activation 不生成 grant；grant 继续由既有 owner-authenticated protocol/lifecycle表达。

## 3. Legacy replay

- M0-M2 AgentSelfAttribute、PartnershipState、TrustProfile、ProactiveProposal 和 CommunicationSession 事件继续按原语义读取。
- 缺 M3 strategy envelope/evaluation/activation 的 legacy 对象只进入 seed/read projection，不能被合成为 stable/active strategy。
- C 波 domain spec 内容升级需要新 StrategyVersionRef/digest；相同 ref 不可覆盖。
- retraction/decay/rollback 追加事件，不删除或重写 legacy evidence。

## 4. Contract tests

- 六种 domain spec round-trip；empty scope/evidence policy、unknown enum/field、invalid decay/expiry、same ref/different digest拒绝。
- fixed/identity/permission/grant/disclosure/L5 forbidden fixtures 全部拒绝。
- self assessment 只能压低；success-only trust path 最多形成 candidate；owner-authenticated grant 保持独立。
- external content不能构造 owner feedback/activation/ground truth；Gateway provenance spoof拒绝。
- M3-A 89-kind snapshot、M3-B specs 和全部 M0-M2 legacy fixtures继续可读。
