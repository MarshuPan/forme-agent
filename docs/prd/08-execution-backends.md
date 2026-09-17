# prd/08 执行 backend(Execution Backends)

模块实施级 PRD。归属 `execution` crate。`ActionIntent → ExecutionPlan → ActionBackend(Shell/File/Mcp)→ 结构化结果`。只在 harness 咽喉点放行后被调。照 `prd/01 §10` 模板。

## 0. 定位与承上

- **实现**:requirements/03 结构 **11**(Shell/File/MCP backend 第一版执行能力)。
- **架构**:architecture/03 §4.5(执行 backend)、§4.7(`ActionBackend` 签名)、§3(数据流执行相)。
- **口径**:canonical **§1**(Harness-first、Policy-at-execution)。
- **验收**:支撑 **S2**(执行侧:放行后执行、拒后不执行)、**S3**(MCP 调用执行)。
- **不偏离**:执行只在 harness 咽喉点(prd/03 §5.1)放行后发生;backend 边界 = §4.5。

## 1. 职责与边界

- **做**:`ExecutionPlanner`(ActionIntent→ExecutionPlan)、`ActionBackend`(ShellBackend/FileBackend/McpBackend)、`ExecutionBackendRegistry`、`ResultNormalizer`、output budget/timeout/cancel、action 事件。
- **不做**:policy 判定(prd/04,execution 在放行**后**被调)、能力解析(prd/07)、编排(prd/03)。**execution 不自决要不要执行**——那是 harness §5.1 的咽喉点。
- **crate**:`execution`(依赖 protocol/policy/prd/07 for MCP)。**M0 范围**:Shell/File/MCP;Browser/PTY/Computer/Remote = M1–M2。

## 2. 全局位置与集成契约(防各自为战)

- **上游依赖**:prd/02、prd/04(policy 先判)。**不反向依赖 prd/07**:MCP server stdio handle 为 protocol 类型(`McpTransport`),由 capabilities spawn、经 `ExecutionPlan` 传入(避免 07↔08 循环)。
- **下游被依赖**:prd/03(harness 把 `ActionPlanned` 路由到 `execute`)。
- **发出事件**:E 组执行事件 `ActionStarted`/`ActionOutputDelta`/`ActionCompleted`/`ActionFailed`/`ActionCancelled` + `McpCallEvent`(MCP)+ `CapabilityEvidenceRecorded`。(`ActionPlanned` 由 harness 路由时发;`ActionDenied` 由 policy/harness 发——归属见 prd/03。)
- **实现 trait**:`ActionBackend`(§4.7)、`ExecutionPlanner`。
- **必守不变量**:**Harness-first**(只经 harness 调,无旁路)、**Policy-at-execution**(执行前重查由 prd/03 §5.1 保证顺序在前)、**shell 不作默认兜底**、**文件写需 diff/变更摘要**、**least-privilege**、**可回滚边界按 backend 声明、不泛化承诺**。

## 3. 数据结构与类型

```rust
pub struct ExecutionPlan {              // 审批后不可变;digest 为审批绑定对象(prd/04,防 TOCTOU)
    pub digest: PlanDigest,             // 规范化摘要(参数/backend/scope/timeout/rollback)
    pub intent: ActionIntent, pub backend: BackendKind, pub scope: Scope,
    pub approval_ref: Option<ApprovalId>, pub verify_ref: Option<VerifyRef>,
    pub rollback_boundary: RollbackBoundary,   // 按 backend 声明
    pub budget: OutputBudget, pub timeout: Duration,
}
pub enum BackendKind { Shell, File, Mcp }   // 声明于 protocol(prd/02);execution 定义各 backend 实现

pub trait ActionBackend {
    fn kind(&self) -> BackendKind;
    fn plan(&self, intent: &ActionIntent) -> Result<ExecutionPlan>;
    fn execute(&self, plan: ExecutionPlan, sink: &EventSink, cancel: CancelToken) -> Result<ActionResult>;
    fn cancel(&self, action: ActionId) -> Result<()>;
}
pub struct ShellBackend { sandbox: Sandbox }
pub struct FileBackend  { roots: Vec<PathScope> }     // 写前产 diff
pub struct McpBackend   { }           // 用 ExecutionPlan 内 McpTransport handle(protocol 类型)执行 stdio 调用

pub struct ActionResult { pub status: ActionStatus, pub output_ref: OutputRef, pub evidence: CapabilityEvidence }
pub struct OutputBudget { pub max_bytes: u64, pub truncate: bool }
pub struct ExecutionBackendRegistry { /* BackendKind → backend */ }
```

## 4. 接口与签名

```rust
pub trait ExecutionPlanner { fn plan(&self, intent: &ActionIntent) -> Result<ExecutionPlan>; }
// ActionBackend 见 §3(kind/plan/execute/cancel)
impl ExecutionBackendRegistry { pub fn backend(&self, kind: BackendKind) -> &dyn ActionBackend; }
```

## 5. 行为/算法/状态机

- **执行流**:`ActionIntent → ExecutionPlanner::plan → ExecutionPlan →`(**policy/approval/能力门已由 harness §5.1 判过**)`→ ActionBackend::execute`(流式写 `ActionStarted/OutputDelta/Completed|Failed`)`→ ResultNormalizer → Event + CapabilityEvidence`。
- **ShellBackend**:sandbox 内执行;output budget/timeout/cancel;**不作默认兜底**(无匹配 backend 不 fallback 到 shell)。
- **FileBackend**:写前产 **diff/变更摘要**;路径受 `PathScope` 约束(least-privilege)。
- **McpBackend**:用 `ExecutionPlan` 内的 `McpTransport` handle(prd/07 spawn、protocol 类型)执行 stdio 调用;发 `McpCallEvent` + timeout/schema/server 错误分类。
- **cancel**:`CancelToken` 传播到 backend,产 `ActionCancelled`。
- **rollback**:`RollbackBoundary` 按 backend 声明(如 File 可提供反向 diff;Shell 一般不可回滚)——**不泛化承诺"都能回滚"**。

## 6. 错误/安全/权限/审批

- **错误 → FailureTaxonomy**:`execution_failure`(工具/命令/文件/MCP 执行失败)。
- **budget/timeout/cancel** 强制;超预算截断并标记。
- **least-privilege**:backend 只在授予 scope 内动;shell 不默认;file 需 diff。
- **内容≠指令**(canonical §15):MCP/命令输出作数据。
- **审批**:execution 不发起审批(那在 §5.1 之前);但携带 `approval_ref` 供审计。

## 7. 配置与可观测

- **config key**:`exec.shell.{sandbox,allowlist}`、`exec.file.roots`、`exec.mcp.timeout`、`exec.output_budget`。
- **可观测**:E 组事件 + `CapabilityEvidence`;每次执行可追(backend/scope/结果/耗时)。

## 8. 实施步骤与顺序

1. `ActionBackend` trait + `ExecutionBackendRegistry` + `ExecutionPlanner`。
2. `ShellBackend`(sandbox/budget/timeout/cancel)。
3. `FileBackend`(写前 diff / PathScope)。
4. `McpBackend`(经 prd/07)。
5. `ResultNormalizer` + `CapabilityEvidence` + E 组事件。

## 9. 测试

- **S2 执行侧(总纲 §7.2)**:放行后动作执行;拒绝后**无 `ActionStarted`/`ActionCompleted`**。
- **S3 执行侧(总纲 §7.6)**:MCP 调用产 `McpCallEvent`;错误分类。
- **单元**:output budget/timeout/cancel 生效;file 写前必产 diff;shell 不作默认兜底(无匹配→报错非 fallback);rollback 边界按 backend 声明。

## 10. 验收

- backend 都在 `ActionBackend` 后;仅经 harness §5.1 放行后被调;budget/timeout/cancel;shell 不默认、file 需 diff;MCP 可调;rollback 按 backend;S2/S3 执行侧通过。

## 11. 原创性与合规

- shell/file 执行、backend registry 是通用模式;MCP 调用经 prd/07(公开协议)。
- **不复制**第三方 backend/sandbox 源码、命令封装或错误文案。
- 引入 sandbox/进程管理第三方库时,记 source/license/copy_risk/implementation_rule/notice。
