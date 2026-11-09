# prd/03 harness + loop(AgentHarness & LoopEngine)

模块实施级 PRD。归属 `harness` + `loop` crate。运行容器 + 执行状态机——把一次 `RunRequest` 推成可观察、可恢复、可审计的 run。照 `prd/01 §10` 模板。

## 0. 定位与承上

- **实现**:requirements/03 结构 **1**(AgentHarness 生命周期)、**2**(AgentLoop 执行闭环)、**3**(Run/Session/Turn 运行时侧;协议在 prd/02)、**D17**(并发一致性)。
- **架构**:architecture/03 §3(M0 数据流)、§4.1(LoopEngine 状态机)、§4.2(Run/Session/Turn 运行时)、§4.6(ResumeState/Compaction)、§4.7(`AgentHarness` 签名)。
- **口径**:canonical **§1**(Harness-first / Policy-at-execution / 工具是 proposal / 事件审计)、**§7**(并发/单写者)、**§17**(可观测与纠偏入口,并入本篇)、**§18**(调度不变量,harness 托管 tick)。
- **验收**:主证 **S1**(harness 侧同协议)、**S2**(loop 可恢复状态机)、**S19**(per-session 串行 / 后台快照);支撑其余全部(它是运行主干)。
- **不偏离**:状态机 = architecture/03 §4.1;`AgentHarness` = §4.7;停止条件 = §4.1。

## 1. 职责与边界

- **做**:
  - `AgentHarness`:绑定(session/policy/model/toolset)、驱动 loop、**咽喉点 enforce**、经 store 持久化事件、调协调相、spawn 子代理(Orchestrator 壳)、**托管背景 tick 调度**、路由 proposal/candidate、可观测/纠偏入口(§17)。
  - `LoopEngine`:model ↔ tool 交替状态机、turn 边界、停止条件、ResumeState。
- **不做**:policy 判定(prd/04,harness 在咽喉点**调用** policy)、协议定义(prd/02)、执行 backend(prd/08,harness 把 intent 路由给它)、cognition/coordination 逻辑(prd/10/12/13,harness 调其 trait)、能力解析(prd/07)、上下文装配(prd/09)。**harness 只 enforce + 编排,不含认知逻辑。**
- **crate**:`harness`、`loop`。
- **M0 范围**:单机单 harness;compaction 做接口 + 事件(自动策略 M1);tick 做调度器 + 不变量(激活扩散实现在 prd/13)。

## 2. 全局位置与集成契约(防各自为战)

- **上游依赖**:prd/02(protocol/store)。
- **下游被依赖**:prd/14(cli/gateway 调 `submit_run`);一切真实执行都经本模块。
- **发出事件**(经 store append,harness 是唯一编排发射者):A 组(Run/Session 生命周期)、B 组(Turn/Context 边界)、C 组(经 loop:Model*/OutputClassified)、D 组(`ToolCallProposed`/`Handoff*`;policy/approval 事件体由 prd/04 产)、E 组(路由 `ActionPlanned` 给 execution)、J 组(`SubagentSpawned/ResultReturned`)。
- **消费/调用 trait**:`EventStore`(prd/02)、`PolicyEngine`/`ApprovalBroker`(prd/04)、`CoordinationReasoner`(prd/12)、`CompetenceGate`/`ProactivityEngine`/`EvolutionGovernor`(prd/13/10)、`ActionBackend`(prd/08)、`ContextBuilder`(prd/09)、`ToolsetResolver`(prd/07)、`ModelProvider`(prd/16)。M0 未落地的依赖用 trait stub 占位。
- **实现 trait**:`AgentHarness`(§4.7);`LoopEngine`(内部)。
- **必守不变量**:**Harness-first**(真实动作只经 harness→execution 路径,无旁路);**Policy-at-execution**(loop 在 `ToolPolicyEvaluated` 相调 policy 重查);**工具是 proposal**;**能力门出口**(投递 outward/proactive intent 前调 `CompetenceGate::ceiling`);**单写者**(经 store append);**per-session 串行**(§7);**事件审计**(每咽喉点写事件)。

## 3. 数据结构与类型

```rust
pub struct Harness<S: EventStore, P: PolicyEngine, /* … 其余 trait 依赖 */> {
    store: S, policy: P, approval: Box<dyn ApprovalBroker>,
    coordination: Box<dyn CoordinationReasoner>, competence: Box<dyn CompetenceGate>,
    proactivity: Box<dyn ProactivityEngine>, backends: ExecutionBackendRegistry,
    context: Box<dyn ContextBuilder>, models: Box<dyn ModelProvider>,
    queues: SessionQueues,            // per-session 串行队列
    tick: TickScheduler,
}

// LoopEngine 状态机(承接 architecture/03 §4.1)
pub enum LoopState {
    RunAccepted, SessionBound, TurnStarted,
    ContextBuild, ModelCall, OutputClassified(OutputKind),
    ToolProposed, ToolPolicyEvaluated(PolicyDecision), ApprovalWait, ApprovalResolved(ApprovalOutcome),
    ActionPlanned, ActionRunning, ActionDone,
    Handoff, Verification, Compaction,
    TurnComplete,
    Suspended(PendingKind),           // 可恢复挂起(approval 等待/工具中断/handoff)——非终态
    Terminal(RunStatus),              // Complete/Aborted/Failed/Limited(不含 Waiting:挂起 ≠ 终态)
}
pub enum OutputKind { Final, Tool, Handoff }

pub struct ResumeState {             // 落盘,支撑 resume(§4.6)
    pub run: RunId, pub at: LoopState,
    pub pending: PendingKind,         // ApprovalWait{approval_id} / ToolInterrupt{action_id} / Handoff{target}
    pub snapshot_ref: EventId,        // 从该事件 + 本状态重建
}
pub enum ResumeInput {                // 与 PendingKind 一一对应,覆盖全部挂起因
    Approval(ApprovalGrant),
    ToolOutcome(ActionId, ResolvedOutcome),   // 含 ActionOutcomeUnknown 的人工裁决结果
    Handoff(HandoffResolution),
}

pub enum StopReason {
    FinalOutput, MaxTurns, BudgetExhausted(BudgetKind), UserCancel, ApprovalDenied,
    RetryExhausted, ContextOverflow, HandoffNoTargetOrLoop, VerifyUnfixable, HitlWait,
}
pub struct Budget { pub tokens: Option<u64>, pub wall_time: Option<Duration>, pub cost: Option<Cents>, pub tool_calls: Option<u32>, pub max_turns: u32 }

pub enum TickTrigger { PostTurn, Idle, Schedule, Diff }   // harness 调度,cognition(prd/13)执行
```

## 4. 接口与签名

```rust
impl<..> AgentHarness for Harness<..> {
    fn submit_run(&self, req: RunRequest) -> Result<RunId>;   // 入队 + 返回,异步推进
    fn stream_events(&self, run: RunId) -> EventStream;       // 订阅该 run 事件
    fn wait(&self, run: RunId) -> Result<RunResult>;
    fn cancel(&self, run: RunId) -> Result<()>;               // → StopReason::UserCancel
    fn resume(&self, run: RunId, input: ResumeInput) -> Result<()>;    // 从 ResumeState 续;覆盖 approval/工具裁决/handoff 全部挂起因
    fn drain(&self, session: SessionId) -> Result<()>;
}

// 内部:loop 驱动
trait LoopEngine {
    fn drive(&self, run: RunId, ctx: &mut RunCtx) -> Result<LoopState>;  // 单步推进,直到 Suspended/Terminal
}

// §17 可观测与纠偏入口
pub trait Observability { fn decision_trace(&self, run: RunId) -> DecisionTrace; fn state(&self, run: RunId) -> RunView; }
pub enum SteeringCommand { CorrectMisunderstanding(..), AdjustProactivity(Scope, Level), Forget(ObjectRef), ResolveCandidate(CandidateId, Resolution), SetDelegation(..) }
// 每个 SteeringCommand → 事件,且作为强证据喂 A3(prd/10 EvolutionGovernor)
```

## 5. 行为/算法/状态机

- **submit_run**:accept(写 `RunAccepted`)→ **per-session 入队**(§7 串行)→ 出队后 bind(session/policy/model/toolset,写 `SessionBound`)→ **协调相**(调 `CoordinationReasoner`,写 J 组)→ **loop** → Suspended(写 `RunWaiting`)或 Terminal(写 `RunComplete|Aborted|Failed|Limited`)。提交按 `RunRequest.idempotency_key` 去重(同键返回原 run_id)。
- **LoopEngine 主循环**(§4.1):`TurnStarted → ContextBuild(调 ContextBuilder)→ ModelCall(调 ModelProvider)→ OutputClassified`;
  - `Final` → `Verification → Compaction? → TurnComplete → Terminal(Complete)`;
  - `Tool` → **咽喉点序列**(§5.1)→ 结果入上下文 → 回 `TurnStarted` 续;
  - `Handoff` → `HandoffRequested/Resolved`(无目标/成环 → `StopReason::HandoffNoTargetOrLoop`)。
- **工具消息续接**：按 prd/16 §5.1 保留原始 assistant proposal 与 call ID，结果关联同一调用并保持 provenance/data treatment；审批等待/恢复不得丢失关联。该内部消息补全不改变冻结 `ModelProvider`、Harness 或事件签名。
- **§5.1 工具咽喉点序列(不可乱序)**:
  ```
  ToolCallProposed
   → policy.evaluate(ctx, intent)            // Policy-at-execution:重查 toolset/参数/路径/网络/命令/sandbox
       deny  → ActionDenied → (可修则续 / 否则 StopReason)
       ask   → approval.request → RunWaiting → resume:ApprovalResolved(denied→ActionDenied / granted→继续)
       allow → 继续
   → [出口] 若 intent.source∈{proactive,schedule,idle} 或 expected_effect=outward:
              competence.ceiling(scope,risk,inputs) 不足 → 降级 level(canonical §6)
   → policy.enforce_envelope(grant, envelope, intent)   // Delegation/AutonomyEnvelope;trust 不覆盖 permission
   → ActionPlanned(最终不可变 plan;权威检查与审批绑定作用于 plan digest——与 architecture §4.5 一致,批后改 plan = 作废重批)
   → backend.execute(plan, sink) → ActionStarted/OutputDelta/Completed|Failed
   → eval.verify(result, done_contract)      // prd/15
   → 结果入上下文
  ```
- **resume**:读 `ResumeState` + `snapshot_ref` 事件 → 校验 `ResumeInput` 与 `PendingKind` 匹配 → 恢复到 `at` 状态 → 续推。
- **UnknownOutcome 与启动恢复扫描(canonical §7)**:崩溃窗口 = `ActionStarted` 已落盘而终态动作事件未落盘(副作用可能已发生)。启动时恢复扫描:逐 run 找无终态的 `ActionStarted` → 追加 `ActionOutcomeUnknown{intent_id}` → run 置 Suspended(ToolInterrupt);裁决:backend 可按 `intent_id` 幂等键探测副作用则自动补记终态,否则升人工裁决(owner 经 `ResumeInput::ToolOutcome` 定结果)。**绝不盲目重试**(防重复副作用)。
- **cancel/stop**:cancel 请求停止进一步执行；没有未确认副作用时，仍在运行的 run 按既有 `StopReason::UserCancel` 收尾，已落盘的动作终态不改写。已经发生但结果未确认的副作用必须保持 `Suspended(ToolInterrupt)`，再次 cancel 也不替代 receipt/probe/owner 裁决，不得补造 ActionCancelled 或把原 run 结束来隐藏 Unknown。停止条件见 §3 `StopReason`(§4.1 全列)。
- **背景 tick 调度(canonical §18)**:**前台优先、绝不打断**——执行中的 run 不被新念头劫持(承诺守卫);后台只在 `PostTurn/Idle/Schedule/Diff` 触发,无有意义触发不跑;调 `ProactivityEngine::tick(trigger, snapshot)`(只读快照,产 Impulse/candidate,不动手);后台内部按价值排队。长任务/多 Agent 跑时后台重活暂停。
- **并发(§7)**:同 session run 进 `SessionQueues` 串行;所有事件经 store 单写路径;子代理 child run 只读快照、只回 summary/result、不写父 session 稳定态。

### 5.2 恢复与未知副作用的实施契约

`LoopState::Suspended` 是可恢复状态，`LoopState::Terminal` 才是终态。实现不得把 `Waiting` 作为 terminal，也不得用“进程重启后从最后一条模型消息重新跑”代替恢复。

启动恢复扫描按以下顺序执行：

1. 读取 authority store 的 integrity、owner binding、aggregate version 和最后 `stream_seq`；
2. 找出已持久化 `ActionStarted` 但没有 `ActionCompleted`/`ActionFailed`/`ActionCancelled` 的 `intent_id`，追加 `ActionOutcomeUnknown`（重复扫描幂等）；
3. 找出未完成 approval、handoff、tool interrupt 和 lease，重建 `ResumeState`，过期或不匹配的输入直接失效；
4. 只有在 unknown 已由原 receipt、只读 probe 或 owner 裁决解决后，才允许继续；否则保持 `Suspended(ToolInterrupt)`；
5. 恢复 scheduler/tick lease，最后才接受新的入口请求。

恢复输入必须与挂起原因匹配：

| `PendingKind` | 允许的 `ResumeInput` | 额外校验 | 禁止行为 |
|---|---|---|---|
| `ApprovalWait` | 当前 approval 的 `ApprovalGrant` | approver、nonce、expiry、最终 plan digest、normalized action digest、affected_resources、approval_ref、policy/tool schema snapshot | 用旧 grant 批新 plan |
| `ToolInterrupt` | 原 intent 的可信 receipt/probe 或 owner `ToolOutcome` 裁决 | `intent_id`、attempt、scope、plan digest、unknown 状态 | 未知时直接 retry |
| `Handoff` | 当前 durable checkpoint 的 `HandoffResolution` | checkpoint、owner、scope、目标与版本 | 搬运未验证模型状态 |

任一恢复校验失败都必须零 driver call、零稳定状态推进，并记录可审计的 failure/blocked 原因。`ActionOutcomeUnknown` 不是失败的同义词，也不是成功的同义词；它是“副作用可能已发生、当前不能证明结果”的独立状态。

#### 5.2.1 Owner 结果裁决的持久命令绑定

`RunControl::ResolveToolOutcome` / `ResolveHandoff` 的完整 command（含幂等键和 resolution 正文）不得丢弃。保持既有 trait、payload 字段和 99-kind taxonomy，通过 `RunResumed.resume_ref` 的保留编码 `control-resolution-v1|command_digest|resolution_digest|target_digest` 记录一次性裁决 claim；三个摘要分别绑定命令号、owner/run/完整裁决语义，以及原待决对象/检查点。来源必须是 OwnerControl/Owner/OwnerInput。store 在同一 append 事务中将 command digest 纳入领域幂等账本，跨 run 异义复用同样冲突。

claim 正文存入现有应用层加密 `protected_content`：使用专属 object namespace 和与原 run 受保护事件相同的 scope/subject；object 由 owner 与 command digest 确定。记录内含 schema version、owner/run、完整 `RunControl` 和上述三个绑定摘要。密文记录插入与 claim event 必须由 `forme-store` 单写者放在同一 SQLite `IMMEDIATE` transaction；两者全成或全不成。相同内容可幂等复用，不同 run/target/resolution 复用同一 command 必须冲突且不留下孤儿记录。该数据不是第二事实源，也不扩大 EventKind；裁决结果仍以既有事件为唯一事实。

同 session 串行锁内先验证真实 PendingKind，再原子提交密文 command 与 claim，最后只提交原 action 的结果事实或当前 handoff resolution。重复同号同义读取原 claim 后的 Action 终态/HandoffResolved；异义拒绝。claim 后尚无结果时保持 Unknown，换号也不能重新裁决该检查点，更不能调用 driver。直接 ResumeInput 同样不能绕过已经持久化的 claim。原引用、结果摘要或 command 不进入事件、日志或错误文本。

认证后的 pending 查询在 `ClaimWithoutResult` 时从密文伴随记录核对绑定后附带原 `RunControl`，供 owner 刷新页面后显式“核实原裁决”；查询不变更状态，页面不自动提交。密文缺失、解密失败或任一绑定不符时仍返回 Unknown 且不提供替代命令；同一 command 不能被新裁决号替代。伴随记录的 scope/subject 与原 run 相同，按该 run 的数据生命周期一起删除。

启动扫描额外闭合“已裁决 Action 终态落盘、Run 终态尚未落盘”的窗口，只补对应 Run 终态，不执行工具或模型。未解决的 handoff 只在原默认 context/policy/model/toolset/workspace/composition 可复现时重建；专用 evolution/federation/child 上下文不得升级成默认主脑。已解决 handoff 的续推中断保留待核实状态，不因重启重新调用模型或重做动作。无关事件不得把 Waiting 投影成 Running。

若恢复扫描发现 `ModelCallStarted` 没有同一 `call_id` 的后续 `ModelCallFinished`，应追加 `RunFailed{stop_reason: model_call_outcome_unknown_after_restart, result_ref: None}` 并停止该 run；不补造模型完成/用量/回答，不重试该请求，也不启用 fallback。用户可在确认原请求结果无法核实后另起新 run。此规则只处理模型调用回执未知，不把它伪装成 `ActionOutcomeUnknown`。

### 5.3 最终计划与执行前双重重查

工具 proposal 到 driver 之间至少经过两次计划相关检查：

- `ActionIntent` 阶段检查候选 scope、toolset、风险和能力门，决定 allow/ask/deny；
- `ActionBackend::plan` 生成不可变 `ExecutionPlan` 后，把规范化 action digest、排序去重的 `affected_resources`、backend、参数、timeout、rollback、capability、scope 和 schema 计算为 `PlanDigest`；审批返回后、实际 driver 调用前再次检查这些 binding 与 policy/grant/envelope/competence/owner epoch，并在全部通过后原子 claim one-shot nonce。

`ActionPlanned` 记录最终计划；任何审批后字段变化都必须废弃原 approval 并重新规划、重审、重批。不能只在 `ActionIntent` 上检查一次后允许 backend 自行补字段。nonce claim 成功后即使 driver 失败、崩溃或结果未知也不能回收 grant。driver 返回后再由 `Verifier` 按 `DoneContract` 取证，结果不明不能被 loop 当作成功。

批准恢复到 driver 的固定顺序如下，不得提前消费 grant，也不得先写 `ActionPlanned`/`ActionStarted` 再补检查：

1. `ApprovalGrant.approver` 与 configured owner 的 `VerifiedPrincipal` 精确一致；直接调用 Harness `resume` 也执行此检查。
2. `approval_id` 与当前 pending ticket、`ExecutionPlan.approval_ref` 精确一致。
3. 从 `ActionIntent` 重算 normalized action digest/affected resources，并校验完整 `PlanDigest`。
4. 重新读取当前 policy 并判定；deny/ask 漂移均使旧 grant 失效。
5. 重读 capability/tool-schema snapshot，验证 backend、tool 与参数仍可用。
6. 重读 `DelegationGrant`/`AutonomyEnvelope`，permission 仍是硬边界。
7. 重跑 `CompetenceGate`；能力证据下降可以收紧，不能用旧 ceiling 放行。
8. 重查 owner epoch、schema/version 与 production authority writer fence。
9. 由 durable `ApprovalConsumptionLedger` 原子 claim nonce；同义重放返回已消费，异义重用返回 conflict。
10. 只有第 9 步首次成功后，才允许写 `ActionPlanned`/`ActionStarted` 并调用 driver。

以上任一步失败都必须 `driver call = 0`。durable claim 已成功后的任何失败都保持 nonce consumed；后续恢复只走 receipt/probe/owner 裁决，不复用 grant。

## 6. 错误/安全/权限/审批

- **错误 → FailureTaxonomy**(canonical §11):`handoff_failure`(交接不完整/成环)、`safety_policy_failure`(咽喉点缺失/误判)、`self_eval_trap`(自检过早宣称完成)、`execution_failure`(重试耗尽);loop stop reason 中属失败的映射到分类。
- **Harness-first**:除经 harness→execution 无真实动作路径;入口无 loop 直调口(S1 反向)。
- **Policy-at-execution**:每个 action 执行前重查(§5.1);模型可见 ≠ 执行许可。
- **能力门出口**:outward/proactive intent 投递前过 `CompetenceGate::ceiling`,不足降级。
- **审批**:ask → 等待/恢复;deny → 不执行;超时 → `ApprovalResolved{expired}`。
- **重启后的审批读取与续接**：两个待审批查询外观共用持久化事件投影，按 session、原 approval id 与 TTL 过滤；已解决或 run 已终态的请求不再列为 pending。查询只读，不重做模型调用、不重新签发审批。正式工具审批在显式 `ResumeInput::Approval` 到来时才重建内存等待态；按 session 串行化重建，复核原计划/action digest、policy/schema 与有效期，仍执行 §5.1 的完整出口检查。终态、已消费、结果不明或配置不能复现的请求拒绝恢复，不以服务重启获得新授权。
- **恢复上下文不得升级**：默认主脑只能自动恢复 `RunAccepted.source = UserTurn`、workspace/policy/model/toolset/composition 绑定与当前实例一致、且不带专用 evolution/federation snapshot 的正式工具审批。回放、模拟、子代理及其他专用执行上下文不得回落为默认主脑；项目子代理必须先由原项目恢复入口重建其受限 Harness，再按原审批继续。缺失、重复或不能复现的绑定在调用模型、重建工具计划和落新事件前拒绝；不得静默清除 snapshot 或扩大 scope。
- **防投毒入口(canonical §15)**:入站内容在 `Provenance.trust_tier` 标注;harness **不把内容当指令**;未信任内容不驱动稳定写入(治理在 prd/10,harness 保证 provenance 正确落标)。

## 7. 配置与可观测

- **config key**:`harness.max_turns`、`harness.budget.*`、`harness.tick.cadence`、`harness.tick.commitment_guard`(默认 on)、`harness.compaction.enabled`(M0 手动)。
- **可观测(§17)**:每相写事件(A–E/J 组);`Observability::decision_trace/state` 提供只读视图;`SteeringCommand` 入口把纠偏落成事件并喂 A3;确认负载按 owner 节奏批处理(canonical §17,不逐条打断)。

## 8. 实施步骤与顺序

1. `Run/Session/Turn` 运行时 + `SessionQueues`(per-session 串行)。
2. `AgentHarness` 最小闭环:`submit_run`(accept→bind→loop→complete)+ `stream_events/wait/cancel`。
3. `LoopEngine` 状态机:context→model→classify→(final)verify→complete。
4. 工具咽喉点序列(§5.1):policy→approval→competence→envelope→execute→verify(依赖 trait 用 stub,随 prd/04/07/08/15 落地替换)。
5. `ResumeState` + `resume`/`cancel` + 全部 `StopReason`。
6. 背景 `TickScheduler`(前台优先 + 承诺守卫 + 价值排队)+ 调 `ProactivityEngine::tick`。
7. §17 可观测/纠偏事件 + `SteeringCommand` 入口。

## 9. 测试

- **S1(harness 侧,总纲 §7.1)**:CLI/Gateway 提交同 `RunRequest` → 同构事件序列(`RunAccepted→SessionBound→…→RunComplete`);**无 loop 直调口**;Gateway 提交仍经 `SessionBound` 绑 policy。
- **S2(总纲 §7.2 的 loop 侧)**:max_turns / cancel / tool error / approval deny 后**不丢状态**——可从 `ResumeState` 续或干净收尾;咽喉点序列正确(policy 早于 execute)。
- **S19(总纲 §7.3)**:同 session A、B 串行(B.`TurnStarted` 不早于 A.`RunComplete`);后台 tick 只产 candidate、不产 stable 写;承诺守卫:执行中 run 不被 tick 劫持;重复 command/恢复扫描不重复 driver 副作用。
- **V1 closure recovery**:模拟 `ActionStarted` 后崩溃，重启必须得到唯一 `ActionOutcomeUnknown` 和挂起态；无裁决时 driver call=0；合法 receipt/owner 裁决后只恢复一次。
- **V1 closure plan binding**:审批后修改 backend、参数、scope、timeout 或 rollback 任一字段，最终重查必须拒绝并要求新 approval；旧 grant 不得复用。
- **审批恢复上下文**：持久化待审批重启后，默认用户任务仍可按原 plan 续接一次；默认 Harness 对子代理/回放/模拟/专用快照和配置漂移的恢复必须零模型调用、零 driver、零新增事件。原项目入口重建 child 后的合法审批恢复继续通过。
- **单元**:状态迁移全覆盖;每个 `StopReason` 可达且干净;resume 往返;咽喉点乱序被拒(policy 必先于 execute);tick 无触发不跑。

## 10. 验收

- Harness-first 无旁路;loop 可恢复、状态不丢;per-session 串行;咽喉点顺序正确且能力门在出口;tick 前台优先、不打断、无触发不跑;每相事件化可审计;S1/S2/S19 通过。

## 11. 原创性与合规

- Agent loop(ReAct 式 model↔tool 循环)与 harness 运行容器是通用模式,非特定项目专有。
- **不复制**第三方的 loop 源码、prompt、状态命名或停止条件表述;状态机命名源自本项目 architecture/03 §4.1。
- 若引入第三方异步/任务库,记 source/license/copy_risk/implementation_rule/notice。
