# prd/15 失败 + 验证(Failure Evidence & Verification)

模块实施级 PRD。归属 `eval` crate。失败进入学习与治理;验证结果进 trace,成功不掩盖中间失败。照 `prd/01 §10` 模板。

## 0. 定位与承上

- **实现**:requirements/03 **D9**(FailureEvidence 分类 + digest)、**D14**(deterministic verifier + trace export);结构 **18**(FailureEvidence)、**19**(verifier/trace export)。
- **架构**:architecture/03 §4(eval crate)、§2.1.1 F 组 + Verification 组。
- **口径**:canonical **§11**(`FailureTaxonomy` 唯一失败分类;CoordinationError/loop stop reason 映射到它)。
- **验收**:主证 **S10**(FailureEvidence/digest)、Verification 矩阵行。
- **不偏离**:eval 只**观察/分类/验证**,不修失败(harness/coordination 决定);单一失败分类 = canonical §11。

## 1. 职责与边界

- **做**:`Verifier`(deterministic + tool result validator + final output validator)、`FailureEvidence`(经 `FailureTaxonomy` 分类)、`FailureDigest`、trace export。
- **不做**:修复失败、执行、决定行动路线(prd/12)。
- **crate**:`eval`(依赖 protocol/store)。**M0 范围**:deterministic verifier + 分类 + digest;LLM judge / auto-regression = M2。

## 2. 全局位置与集成契约(防各自为战)

- **上游依赖**:prd/02。(harness 调 verifier / 路由失败——prd/03 是本模块的**下游**,不是上游;eval 只依赖 protocol/store。)
- **下游被依赖**:prd/03(harness 调 verifier)、prd/10(cognition 读 FailureEvidence 产候选)、prd/13(能力门读 FailureEvidence)。
- **发出事件**:F 组 `FailureEvidenceRecorded`/`FailureDigestUpdated`;Verification 组 `VerificationStarted`/`VerificationFinished`。
- **实现 trait**:`Verifier`、`FailureClassifier`。
- **必守不变量**:**单一 `FailureTaxonomy`**(canonical §11)、**final answer 不掩盖失败**(S10)、**验证结果进 trace**、**验证失败不标记成功**。

## 3. 数据结构与类型

```rust
pub trait Verifier { fn verify(&self, evidence: &EvidenceBundle, done: &DoneContract) -> VerificationOutcome; }
// EvidenceBundle = 按 DoneCriterion.evidence 选择器聚合的证据(动作结果/事件段/文件态)——验收一个 run 需要的不止单个 ActionResult
pub enum VerifierKind { Deterministic, ToolResult, FinalOutput }
pub enum VerificationOutcome { Pass, Fail(FailureRef), Unverifiable(Reason) }  // Unverifiable ≠ Pass:进 trace 并降低承诺(能力门输入),不得标成功

pub enum FailureClass {   // canonical §11 全 13 类
    GoalFraming, Context, CognitiveMap, ResourceSelection, Execution, Verification,
    Trust, Proactivity, Learning, Handoff, SelfEvalTrap, SafetyPolicy, MemoryMisevolution,
}
pub struct FailureEvidence { pub class: FailureClass, pub impact: Impact, pub scope: Scope,
    pub related_refs: RelatedRefs, pub suggested_fix: Option<Fix> }   // refs: trace/capability/trust/map
pub struct FailureDigest { pub members: Vec<FailureRef>, pub summary: DigestSummary }
```

## 4. 接口与签名

```rust
pub trait FailureClassifier { fn classify(&self, ev: RawFailure) -> FailureClass; }
// Verifier 见 §3;FailureDigest::update(members);trace_export(run) -> TraceExport
```

## 5. 行为/算法/状态机

- **verify**:deterministic + tool result + final output validator 对照 `DoneContract` → `VerificationOutcome` → `VerificationFinished` 事件。
- **失败分类**:映射到 `FailureTaxonomy` 13 类;`CoordinationError` / loop stop reason 中属失败的映射到此(canonical §11)。`memory_misevolution` 由认知层主动失效自检产(canonical §8 / prd/10)。
- **digest**:聚类 + 记影响/scope/相关资源/验证/反馈/建议修正;关联 DecisionTrace/CapabilityEvidence/TrustProfileRef/CognitiveMapUpdateProposal。
- **trace export**:供 replay/可观测。
- **不掩盖**:final answer 成功不覆盖中间失败;验证失败不能标成功。

## 6. 错误/安全/权限/审批

- **`self_eval_trap` 守卫**:自检自己后不过早宣称完成。
- **验证失败不标记成功**;失败进证据链而非被吞。

## 7. 配置与可观测

- **config key**:`verifier.<kind>`、`failure.digest.clustering`。
- **可观测**:F/Verification 组事件;失败可解释(类/影响/相关)。

## 8. 实施步骤与顺序

1. `Verifier`(deterministic + tool-result + final-output)对照 DoneContract。
2. `FailureTaxonomy` 13 类分类。
3. `FailureEvidence` + related_refs。
4. `FailureDigest` 聚类。
5. trace export。

## 9. 测试

- **S10(总纲 §7.16)**:工具/验证/审批/主动/资源失败分类 + digest + 关联;final 成功不掩盖。
- **Verification 矩阵行**:验证结果进 trace;验证失败不标成功。
- **单元**:13 类映射;`self_eval_trap` 守卫;digest 聚类。

## 10. 验收

- 单一 FailureTaxonomy;verifier 结果进 trace;失败分类 + digest + 关联;成功不掩盖;S10 + Verification 行通过。

## 11. 原创性与合规

- 验证 + 失败分类是本项目自有设计(canonical §11)。
- **不复制**第三方 eval/测试 fixture 或错误分类命名(canonical §12,fixture 自建)。
