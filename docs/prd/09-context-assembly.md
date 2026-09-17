# prd/09 context 装配 + compaction(Context Assembly)

模块实施级 PRD。归属 `context` crate。每 turn 从 rules/history/memory-summary/skills-metadata/tool-schema 装配上下文;compaction 边界。照 `prd/01 §10` 模板。

## 0. 定位与承上

- **实现**:requirements/03 结构 **6**(Context/Memory/Skills metadata/Tool schema 分层的 context 装配侧)。
- **架构**:architecture/03 §4.1(ContextBuild 相)、§4.6(Compaction)、§3(数据流)。
- **口径**:canonical **§14**(记忆→上下文)、**§15**(装配处的防投毒)。
- **验收**:主证 **S4**(skill 按需——context 侧)、Context/Memory 矩阵行。
- **不偏离**:context 只**装配**、不拥有来源(memory 在 prd/06、skill 正文在 prd/07、tool schema 在 prd/07)。

## 1. 职责与边界

- **做**:`ContextBuilder`——把 rules/history/memory-summary/skills-metadata/tool-schema/context-slice 装配成每 turn 上下文;`Compactor`(compaction 接口 + 事件 + BeforeCompaction/AfterCompaction hook)。
- **不做**:memory 存储(prd/06,context 只**读** summary/recall)、skill 正文的加载权(prd/07 `load_body`,context 只纳入)、model call(prd/16)。
- **crate**:`context`(依赖 protocol/store/memory)。**M0 范围**:分层装配 + 按需;自动压缩策略 = M1。

## 2. 全局位置与集成契约(防各自为战)

- **上游依赖**:prd/02、prd/06(memory summary/recall)。
- **下游被依赖**:prd/03(loop 的 ContextBuild 相调 `ContextBuilder`)。
- **发出事件**:B 组 `ContextBuildStarted`/`ContextBuildFinished`、`CompactionStarted`/`CompactionFinished`。
- **消费**:`SkillMetadataExposed`/`SkillBodyLoaded`(prd/07);memory summary(prd/06)。
- **实现 trait**:`ContextBuilder`、`Compactor`。
- **必守不变量**:**skill 正文按需**(默认只 metadata,S4)、**memory summary 非 raw dump**、**compaction 不丢审批/工具结果/关键决策 lineage**(§4.6)、**防投毒**(未信任内容标注、作数据不作指令,canonical §15)。

## 3. 数据结构与类型

```rust
pub struct ContextSources {
    pub rules: Vec<Rule>, pub history: HistoryWindow, pub memory_summary: MemorySummary,   // prd/06
    pub skills_metadata: Vec<SkillMetadata>, pub tool_schema: ToolSchema,                  // prd/07
    pub slices: Vec<ContextSlice>,
}
pub struct Context { pub assembled: AssembledContext, pub token_cost: u64, pub provenance: Vec<Provenance> }
pub struct ContextBudget { pub max_tokens: u64, pub reserve: u64 }

pub struct CompactionPlan { pub boundary: CompactionBoundary, pub preserved: Vec<EventId>, pub summary: SummaryRef }
pub struct CompactionBoundary { /* lineage:审批/工具结果/关键决策不可丢 */ }
```

## 4. 接口与签名

```rust
pub trait ContextBuilder {
    fn build(&self, ctx: &RunCtx, budget: ContextBudget) -> Result<Context>;   // 装配 + token 预算
}
pub trait Compactor {
    fn compact(&self, session: SessionId) -> Result<CompactionPlan>;           // M0 手动触发
    // BeforeCompaction / AfterCompaction hook 点
}
```

## 5. 行为/算法/状态机

- **build**:装配 rules + history(近窗)+ memory summary(prd/06,**scoped**)+ skills metadata(**默认只这个**)+ tool schema(prd/07 toolset)+ context slices;**token 预算**内裁剪;每来源标 `Provenance`(未信任内容标注)。
- **skill 正文**:仅当被选中/被 CoordinationReasoner 引用时纳入(对应 prd/07 `SkillBodyLoaded`),**默认不塞**。
- **AgentWorkspace 前导装载(canonical §18/§23)**:大脑级调用(协调相/反思/主动判断)在 context 前导装载 `AgentWorkspace` 投影(活跃目标/进行中 run/未决念头/到期意图/待确认候选,有界 ~10 项)——三内核读同一份焦点(广播面);普通执行相调用不强制。context 装配 = 两级工作空间之间的装货码头。
- **compaction(§4.6)**:上下文超限 → `CompactionPlan`(产 summary 边界 + lineage refs)→ BeforeCompaction/AfterCompaction hook;**不丢审批/工具结果/关键决策 lineage**。M0 接口 + 事件,自动策略 M1。

## 6. 错误/安全/权限/审批

- **错误 → FailureTaxonomy**:`context_failure`(忽略关键文档/历史/用户偏好/当前状态)。
- **防投毒(canonical §15)**:装配是 prompt-injection 防火墙的一道——未信任来源内容标注、只作**数据**不作对 forme 的**指令**。
- **compaction 不丢 lineage**:审批/工具结果/关键决策必留。

## 7. 配置与可观测

- **config key**:`context.budget.max_tokens`、`context.history.window`、`context.compaction.threshold`(M0 手动)。
- **可观测**:B 组事件;装配可解释(哪些来源、占多少预算、哪些被裁)。

## 8. 实施步骤与顺序

1. `ContextBuilder` + `ContextSources` 模型。
2. build:rules/history/memory-summary/tool-schema 装配(token 预算)。
3. skills metadata 默认 + 按需正文纳入(经 prd/07)。
4. 装配处防投毒标注。
5. `Compactor`(接口 + BeforeCompaction/AfterCompaction hook + lineage 保留)——M0 手动。

## 9. 测试

- **S4(总纲 §7.7,context 侧)**:默认只 skill metadata;仅被引用时纳入正文。
- **Context/Memory 矩阵行**:来源有界;memory summary 非 raw dump。
- **单元**:token 预算裁剪;compaction 保留审批/工具结果/决策 lineage;未信任内容标注、不作指令。

## 10. 验收

- 上下文由分层来源装配;skill 正文按需;memory summary scoped;compaction 保 lineage;装配防投毒标注;S4 + Context/Memory 行通过。

## 11. 原创性与合规

- 上下文装配、压缩是通用模式。**不复制**第三方 context/compaction 的 prompt、策略表述或摘要模板。
- 引入 tokenizer/摘要第三方库时,记 source/license/copy_risk/implementation_rule/notice。
