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
- **必守不变量**:**Harness-first**(只经 harness 调,无旁路)、**Policy-at-execution**(执行前重查由 prd/03 §5.1 保证顺序在前)、**审批绑定最终 plan**(action/resource digest、approval_ref、backend-specific 参数不可在批后替换)、**one-shot claim 在 driver 前**、**shell 不作默认兜底**、**文件写需 diff/变更摘要**、**least-privilege**、**可回滚边界按 backend 声明、不泛化承诺**。

## 3. 数据结构与类型

```rust
pub struct ExecutionPlan {              // 审批后不可变;digest 为审批绑定对象(prd/04,防 TOCTOU)
    pub digest: PlanDigest,             // 覆盖全部 driver-affecting 字段
    pub normalized_action_digest: SchemaDigest,
    pub affected_resources: Vec<ResourceRef>, // 排序去重;空集合显式存在
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

- **执行流**:`ActionIntent → ExecutionPlanner::plan → ExecutionPlan`(计算 action/resource binding) `→`(**policy/approval/能力门已由 harness §5.1 判过**)`→`(最终重查 + one-shot nonce 原子 claim)` `→ ActionBackend::execute`(流式写 `ActionStarted/OutputDelta/Completed|Failed`)`→ ResultNormalizer → Event + CapabilityEvidence`。claim 失败不得调用 driver。
- **ShellBackend**:sandbox 内执行;output budget/timeout/cancel;**不作默认兜底**(无匹配 backend 不 fallback 到 shell)。
- **FileBackend**:写前产 **diff/变更摘要**;路径受 `PathScope` 约束(least-privilege)。
- **McpBackend**:用 `ExecutionPlan` 内的 `McpTransport` handle(prd/07 spawn、protocol 类型)执行 stdio 调用;发 `McpCallEvent` + timeout/schema/server 错误分类。
- **cancel**:`CancelToken` 传播到 backend；已确认停止与清理时产 `ActionCancelled`，未确认副作用或清理时按 canonical §7 保留 `ActionOutcomeUnknown`，不把取消请求当作完成证明。
- **rollback**:`RollbackBoundary` 按 backend 声明(如 File 可提供反向 diff;Shell 一般不可回滚)——**不泛化承诺"都能回滚"**。

### 5.1 现有 MCP stdio 调用的 I/O 实施约束

这是既有 timeout/cancel/output/lifecycle 不变量的实现细化，不新增公开 DTO、trait、EventKind、crate 或依赖边，也不启用安装版 MCP placement。

- 调用的同一个 deadline 与 CancelToken 覆盖序列化、写入、flush 和响应等待；主执行线程不得被不读取 stdin 的服务端锁在同步写入内。写线程逐帧给出完成回执，回执等待持续检查原 deadline/cancel，不按阶段重置时限。
- 传输层按完整 JSON 行处理，单帧上限 8 MiB（含换行）；输入编码不得先无界增长再判断，响应也不得用无界 `lines()` 分配。待读响应最多两帧、待写队列最多一帧。超长帧按 schema 错误终止；普通有效帧之后的 ActionOutput 仍遵守已审批的 OutputBudget，传输上限不替代输出预算。
- 普通调用最多同时持有八个进程/I/O 槽位。停止后关闭队列、结束原 child、wait 并 join 原读写线程；主调用最多等待两秒清理。不能确认清理时只写 `ActionOutcomeUnknown`，不改写为成功、失败或取消，也不重派原动作。
- 原 child 与线程的清理句柄由有限 supervisor 保管，包括创建清理线程失败时尚未执行的清理材料。存在未结束的清理或已确认的清理失败时拒绝新的进程 admission；正常清理全部结束才释放槽位，稍后的清理结果不改写原 Unknown。Drop 只是兜底，正式终态须在显式清理结果后决定。
- 工具请求的写入尝试和工具结果确认分别记录在原会话中；序列化/排队不冒充已派发，写入尝试也不冒充服务端已完成。`tools/call` 一旦可能写入管道，若未取得匹配 id、有效 JSON-RPC envelope 和可用结果/明确协议错误，timeout、断线、无效响应或取消都只能产生 `ActionOutcomeUnknown`；保留原 MCP 错误分类。kill/wait/join 成功只证明清理，不撤销已经发生的副作用。尚未尝试写工具请求的拒绝/取消保持原语义；有效结果或明确协议错误与清理均确认后才可记普通终态。Harness 对 Unknown 挂起原 run，重启/重放不能重新派发原动作。
- 这不宣称普通 stdio 已拥有后代进程隔离或安装版 Host grant。受管 discovery/call、隔离工作区、受保护输入/结果与真实安装验收继续按 PRD24/25 和对应待审提案闭合。

### 5.2 现有 Shell 子进程与输出清理的实施约束

本节细化既有 timeout/cancel/output/Unknown 与有界资源不变量；不改变公开 DTO、trait、EventKind、crate 或依赖边，也不启用安装版 Shell placement。

- stdout/stderr 由两个受管理 reader 读取，每块最多 4 KiB、共享最多十六块的有界队列；读取错误须显式返回，不能当作 EOF。最终结果继续遵守原计划的 OutputBudget，严格预算失败也要经过清理。
- 超时、取消、预算错误、读/观察错误和正常退出均进入同一显式清理路径。直接 child 的 kill/wait 与两个 reader 的 join 由原清理任务持有，主调用最多等待两秒清理，不做无期限 recv/drain/join。仅有直接 child exit code 不证明继承输出管道已关闭。
- 普通 Shell 同时最多八个进程/I/O 槽位。未确认清理时原动作只产唯一 `ActionOutcomeUnknown`，不产普通完成/失败/取消终态；原 child/reader 句柄留在有界 supervisor，新 backend 实例也不能绕过原清理准入。清理线程无法创建时仍保管原任务；确认清理失败继续阻断，不释放为假成功。
- 原清理稍后确认结束只释放资源，不改写 Unknown、不重新执行原动作、不推进原 run；结果仍经原 receipt/probe/owner 裁决。派发前取消、有效退出、已确认清理的取消/超时保持各自既有语义。
- 普通 Shell 只证明直接 child 与其输出线程的生命周期，不宣称后代隔离或 OS sandbox。安装版仍必须沿 Host/WorkerBroker 的真实隔离路径；不能拿本节组件测试关闭 IP10/IP11/IP24。

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
- **MCP I/O 回归**:真实自建子进程在 initialize 后不再读取输入，分别验证大请求写入中的 timeout/cancel；超长无换行响应被限制；噪声/不匹配帧不越过输出预算；后代持有管道时原动作保持 Unknown、新动作 admission 被阻止，原清理 join 后只允许新的不同动作运行。有限 fixture 不代替安装版后代隔离或全平台原生验收。
- **MCP 副作用未确认回归**:真实自建工具先持久写 marker，再分别丢响应、返回无效响应和等待取消；原 marker 恰一次，脑事实只能为 Unknown。正式 Harness 核对 Waiting、恢复原号和重放零额外工具派发；有效成功/协议错误和工具请求派发前错误继续区分，不把所有错误统一掩盖为 Unknown。
- **Shell I/O 生命周期回归**:真实有限后代持有继承 stdout/stderr，分别触发 timeout、cancel、strict budget 及直接父进程先退出；原调用有界返回唯一 Unknown、原 marker 恰一次、新 backend 零派发，原清理 join 后仅新的不同动作可执行且不改写原 Unknown；正常成功/失败与已确认停止的取消、派发前取消仍保持区分。

## 10. 验收

- backend 都在 `ActionBackend` 后;仅经 harness §5.1 放行后被调;budget/timeout/cancel;shell 不默认、file 需 diff;MCP 可调;rollback 按 backend;S2/S3 执行侧通过。

## 11. 原创性与合规

- shell/file 执行、backend registry 是通用模式;MCP 调用经 prd/07(公开协议)。
- **不复制**第三方 backend/sandbox 源码、命令封装或错误文案。
- 引入 sandbox/进程管理第三方库时,记 source/license/copy_risk/implementation_rule/notice。


## 安装版 MCP 发现执行边界（2026-11-10）

owner 已批准[显式发现动作契约](../architecture/proposals/installed-managed-mcp-discovery-action.md)。`ActionParameters::McpDiscovery(ManagedMcpDiscoverySpec)` 单独表示受管 metadata discovery，复用原 BackendKind::Mcp、job/permit、审批与恢复；旧 Mcp 工具调用形状不变。Host 来源和当前设置绑定、有限 catalog、保护原件及重启恢复按该契约执行。安装版不使用 Brain 进程内 discovery，未配置或缺原生隔离时保持 unavailable；不新增依赖边、EventKind 或 unsafe 边界。开发 stdio 的既有协议保持，不能作为受管 fallback。
