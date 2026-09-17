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
- **cancel/stop**:任何时刻 cancel → `StopReason::UserCancel`;停止条件见 §3 `StopReason`(§4.1 全列)。
- **背景 tick 调度(canonical §18)**:**前台优先、绝不打断**——执行中的 run 不被新念头劫持(承诺守卫);后台只在 `PostTurn/Idle/Schedule/Diff` 触发,无有意义触发不跑;调 `ProactivityEngine::tick(trigger, snapshot)`(只读快照,产 Impulse/candidate,不动手);后台内部按价值排队。长任务/多 Agent 跑时后台重活暂停。
- **并发(§7)**:同 session run 进 `SessionQueues` 串行;所有事件经 store 单写路径;子代理 child run 只读快照、只回 summary/result、不写父 session 稳定态。

## 6. 错误/安全/权限/审批

- **错误 → FailureTaxonomy**(canonical §11):`handoff_failure`(交接不完整/成环)、`safety_policy_failure`(咽喉点缺失/误判)、`self_eval_trap`(自检过早宣称完成)、`execution_failure`(重试耗尽);loop stop reason 中属失败的映射到分类。
- **Harness-first**:除经 harness→execution 无真实动作路径;入口无 loop 直调口(S1 反向)。
- **Policy-at-execution**:每个 action 执行前重查(§5.1);模型可见 ≠ 执行许可。
- **能力门出口**:outward/proactive intent 投递前过 `CompetenceGate::ceiling`,不足降级。
- **审批**:ask → 等待/恢复;deny → 不执行;超时 → `ApprovalResolved{expired}`。
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
- **S19(总纲 §7.3)**:同 session A、B 串行(B.`TurnStarted` 不早于 A.`RunComplete`);后台 tick 只产 candidate、不产 stable 写;承诺守卫:执行中 run 不被 tick 劫持。
- **单元**:状态迁移全覆盖;每个 `StopReason` 可达且干净;resume 往返;咽喉点乱序被拒(policy 必先于 execute);tick 无触发不跑。

## 10. 验收

- Harness-first 无旁路;loop 可恢复、状态不丢;per-session 串行;咽喉点顺序正确且能力门在出口;tick 前台优先、不打断、无触发不跑;每相事件化可审计;S1/S2/S19 通过。

## 11. 原创性与合规

- Agent loop(ReAct 式 model↔tool 循环)与 harness 运行容器是通用模式,非特定项目专有。
- **不复制**第三方的 loop 源码、prompt、状态命名或停止条件表述;状态机命名源自本项目 architecture/03 §4.1。
- 若引入第三方异步/任务库,记 source/license/copy_risk/implementation_rule/notice。
