# M3 验证与验收策略

本文承接 `09-m3-scope-decisions.md`，把 M3 的受控策略进化转成可执行验收。S1-S52 永久证明 M0-M2 没有回退；S53-S69 证明 replay、evaluation、promotion、activation、runtime pinning 和 rollback 在同一条事件化治理链中工作。

## 1. 验收原则

- “候选在几个任务上更好”不能单独证明可提升。必须有 frozen baseline、相同 case/budget/snapshot、holdout、hard invariant、ground-truth refs 和 typed verdict。
- exact replay 与 counterfactual simulation 都必须在 effect-deny 模式运行；断言真实 Browser/Computer/PTY/AppApi/Communication driver 调用数为零。
- `CandidatePromoted`、`StrategyActivated` 和 permission/grant 是不同事实；测试必须分别断言，不能用一个最终 active 状态代替中间证据。
- 每个真实 run 在 `SessionBound` 固定 `EvolutionSnapshot`；active pointer 变化后，旧 run 仍使用原版本，新 run 才使用新版本。
- 自评、LLM judge、模型多数票和候选自生成 case 只能作为辅助；result evidence/independent verifier/owner rubric 不足时 verdict 必须是 Fail 或 Unverifiable。
- 更谨慎的自动 activation 与更自信/主动/宽 scope 的 owner-confirmed activation 分开测试。任何 permission、trust、delegation 或 L5 扩大都不能由策略事件产生。
- rollback 必须恢复后续 run 的已知良好版本并保留失败历史；不能声称已经撤销外部副作用。
- 外部未信任内容、SecretRef、schema/version drift、CAS conflict、retraction、cancel 和 crash 都有反向场景。

## 2. 全局回归门

每次 M3 提交前必须执行：

```text
cargo fmt --all -- --check
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
py -3 -m unittest discover -s tools/tests -v
bash tools/compliance-doctor.sh
```

`tools/verify-m2.ps1` 保持为 S1-S52 固定回归入口。M3 新建 `tools/verify-m3.ps1`，必须先完整运行 M2 gate，再追加当前已激活波次、89-kind snapshot、18-crate graph、M3 artifact gate 和 release compliance gate。

## 3. S53-S69 场景矩阵

| 场景 | 证明目标 | 主责波次 |
|---|---|---|
| S53 | ReplayBundle 完整、portable、secret-free，exact replay 确定且不重写历史 | M3-A |
| S54 | counterfactual simulation 使用 effect-deny sink，真实副作用为零 | M3-A |
| S55 | baseline/holdout 多维 eval 以 ground truth 为主，self-eval 不能自批 | M3-A |
| S56 | candidate、promotion、activation、authorization 四态分离且 impact gate 生效 | M3-A |
| S57 | active CAS、run snapshot pinning、regression rollback 和外部效果诚实边界 | M3-A |
| S58 | LoopSpec/LoopRegistry 候选经 replay/eval 后才改变后续 run | M3-B |
| S59 | CoordinationRegistry/WorkPattern/role weight 以质量和成本证据进化且 subagent 仍经 Harness | M3-B |
| S60 | capability/tool/model/backend selection 只在已授权候选集中排序 | M3-B |
| S61 | model-capability adaptation 改脚手架但不减少治理或高影响外化证据 | M3-B |
| S62 | bounded long-horizon run 保持 checkpoint、前台优先、budget/cancel 和策略版本边界 | M3-B |
| S63 | StrategyMemory 冲突、freshness、decay 和 retraction 触发再评估而非静默漂移 | M3-C |
| S64 | AgentSelfModel 由结果证据更新，自评只能压低能力上限 | M3-C |
| S65 | PartnershipModel 演化不冒充意识、情感、用户身份或授权 | M3-C |
| S66 | Trust/Delegation 策略可自动降级但扩大只形成 owner-reviewed proposal | M3-C |
| S67 | proactive/communication 策略优化打扰成本且不扩大观察、披露或外发权限 | M3-C |
| S68 | 端到端 golden 完成 candidate -> eval -> activation -> governed live run -> rollback | M3-C |
| S69 | release tree、license/NOTICE、依赖、原创、secret 和 artifact 合规全门 | M3-C |

## 4. 逐场景规格

### S53 Portable Exact Replay

- **Fixtures**：repository-owned M2 typed trace；完整 event/schema/policy/LoopSpec/model/tool/driver/strategy snapshot；包含 SecretRef 与本机路径的负向样本。
- **触发**：从同一权威事件范围构建两次 ReplayBundle，并用 exact mode 回放两次。
- **事件**：`RunAccepted{source=Replay} -> SessionBound{effect_mode=Deny,evolution_snapshot} -> VerificationStarted -> VerificationFinished{exact_match} -> RunComplete`。
- **Store/结果**：两次 bundle digest、事件顺序、typed result 和 projection diff 相同；manifest 只含 portable ref/digest；原 event log 不变。
- **反向断言**：缺 schema/policy/strategy snapshot、checksum mismatch、未知 migration、raw secret 或绝对私有路径时拒绝构建；不得用当前 projection 填补缺失历史。

### S54 Counterfactual Simulation 无副作用

- **Fixtures**：同一 baseline/candidate strategy；recording effect-deny sink；Browser/PTY/AppApi/Communication driver 调用计数器；deterministic scripted model/result fixtures。
- **触发**：用 candidate 对录制 case 做 counterfactual simulation，策略提出 outward action。
- **事件**：`RunAccepted{source=Simulation} -> SessionBound{effect_mode=Deny} -> ToolCallProposed -> ToolPolicyEvaluated -> ActionDenied{reason=simulation_effect_denied} -> VerificationFinished -> EvolutionEvaluationRecorded -> RunComplete`。
- **Store/结果**：可以评价“会提出什么”，但所有真实 driver 调用数为零，外部 fixture 状态未变化；simulation result 明确不是真实 ground truth。
- **反向断言**：切换 live backend、解析 ResolvedSecret、网络 mutation、对外发送或把 simulated success 记为 CapabilityEvidence 均 fail closed。

### S55 Ground-truth Evaluation

- **Fixtures**：固定 train/holdout case set；stable baseline 与 candidate；相同 budget；deterministic verifier；一个只给自己高分的 judge；一个 hard-invariant failure 和一个 unverifiable case。
- **触发**：运行 baseline/candidate comparison，并分别移除 ground truth、泄漏 holdout 或让 candidate 选择 case。
- **事件**：`VerificationStarted* -> VerificationFinished* -> EvolutionEvaluationRecorded{baseline,candidate,fitness,hard_invariants,holdout,verdict}`。
- **Store/结果**：质量、成本、延迟、风险、打扰、委派等维度分别保存；hard failure 不被低成本抵消；无独立证据时为 Unverifiable。
- **反向断言**：self-eval-only、LLM-judge-only、train=holdout、case digest drift、不同预算或丢弃失败样本都不能产 Pass。

### S56 Promotion、Activation 与 Authorization 分离

- **Fixtures**：一个更谨慎 candidate；一个减少验证/扩大 scope 的 candidate；owner-authenticated activation command；无 permission 的 capability。
- **触发**：两者先通过 evaluation，再执行 promotion；谨慎策略走自动 activation，高影响策略分别尝试无 owner 和有 owner confirmation 激活。
- **事件**：`CandidateCreated -> EvolutionEvaluationRecorded{pass} -> CandidatePromoted -> StrategyActivated{automatic_cautious|owner_confirmed}`。高影响无 owner 时没有 `StrategyActivated`。
- **Store/结果**：stable strategy registry 与 active pointer 是不同 projection；activation 记录 impact、from/to、evaluation 和 owner ref；permission/grant projection 不变。
- **反向断言**：`CandidatePromoted` 后立即生效、active strategy 自动生成 Toolset/DelegationGrant/AutonomyEnvelope、未授权 capability 被选择或 constitutional candidate 被接受均失败。

### S57 Snapshot、CAS 与 Rollback

- **Fixtures**：active v1；通过评估的 v2；两个并发 activation command；绑定 v1 的长 run；v2 regression evidence；一个已发生外部 mutation 的 run。
- **触发**：CAS 激活 v2，启动新 run；制造 conflicting activation；记录 regression 并回滚到 v1。
- **事件**：`StrategyActivated{v1->v2,expected_version} -> SessionBound{run_old:v1} -> SessionBound{run_new:v2} -> FailureEvidenceRecorded -> EvolutionEvaluationRecorded{regression} -> StrategyRolledBack{v2->v1,external_effects_reverted=false}`。
- **Store/结果**：并发失配零写入；activation/rollback 在显式 EvolutionAggregateRef 内 committed=expected+1；旧 run 保持 v1，新 run 用 v2，回滚后的后续 run 用 v1；失败与 v2 历史保留。
- **反向断言**：mid-run 热换、last-write-wins、删除 v2 事件、自动重试未知外部动作、把补救记成策略 rollback 或声明外部效果已撤销均失败。

### S58 LoopSpec 受控进化

- **Fixtures**：seed LoopSpec v1；只改变 phase/trigger/checkpoint/verification/budget profile 的 v2；golden 与 failure cases；固定 Harness/Policy。
- **触发**：形成 LoopSpec candidate，离线比较 v1/v2，激活 v2 后运行新任务，再提交试图删除 approval/verifier 的 candidate。
- **事件**：`CandidateCreated{domain=Loop} -> EvolutionEvaluationRecorded -> CandidatePromoted -> StrategyActivated -> SessionBound{loop=v2} -> ... -> VerificationFinished`。
- **Store/结果**：run trace 绑定 loop digest；v2 只能改变冻结范围内的脚手架；Harness/Policy/DoneContract/停止条件仍在。
- **反向断言**：一次 Reflection 直接改默认 loop、无限 turns、删除 approval/verification/audit、运行中换 loop 或旧 loop 无法 replay 均失败。

### S59 Coordination/WorkPattern 进化

- **Fixtures**：baseline WorkPattern、candidate route/role weights；质量相同但过度委派的反例；subagent scoped denial；相同 task/budget。
- **触发**：比较 single/route candidate 的 done quality、cost、latency、over-delegation 和 failure；激活优胜 pattern。
- **事件**：`CandidateCreated{domain=Coordination} -> EvolutionEvaluationRecorded{coordination_fitness} -> CandidatePromoted -> StrategyActivated -> OrchestrationRouteCreated -> SubagentSpawned* -> SubagentResultReturned* -> VerificationFinished`。
- **Store/结果**：DecisionTrace 引用 coordination strategy/evaluation；更低质量不能用低成本获胜；所有 child run 仍 fresh/scoped 且经 Harness。
- **反向断言**：模式成为主脑、subagent 自写稳定层/自有认知、越权 tool、父取消不级联或 role weight 自动扩大 permission 均失败。

### S60 Selection Policy 进化

- **Fixtures**：authorized 与 unauthorized tool/model/backend；managed deny；CapabilityEvidence pass/fail；候选选择策略。
- **触发**：在相同 inventory 上比较 selection；让高分未授权资源、已撤销 plugin 和外部 backend 参与竞争。
- **事件**：`CandidateCreated{domain=Selection} -> EvolutionEvaluationRecorded -> CandidatePromoted -> StrategyActivated -> ToolsetResolved -> ResourcePlanned -> ToolPolicyEvaluated`。
- **Store/结果**：先按 registry lifecycle/permission/managed policy 过滤，再在允许集内排序；DecisionTrace 记录候选集、证据和策略版本。
- **反向断言**：score 覆盖 deny、自动安装、provider 自报 capability 直接激活、选择改变 L3/L5 floor 或 self confidence 抬高 evidence ceiling 均失败。

### S61 Model-capability Adaptation

- **Fixtures**：同一 task 的 weak/strong ModelProfile；固定 DoneContract、policy 与高影响 trace requirement；候选 adaptation profile。
- **触发**：weak 模型增加分步/验证/checkpoint，strong 模型减少非必要脚手架；尝试同时减少高影响外化证据。
- **事件**：`StrategyActivated{domain=ModelAdaptation} -> SessionBound{model,evolution_snapshot} -> DecisionTraceRecorded -> VerificationStarted -> VerificationFinished`。
- **Store/结果**：结构数量可变，治理、完成标准和高影响 DecisionTrace 不变；model capability 来源和 evaluation 可追。
- **反向断言**：强模型绕过 approval/verification、provider 自报代替实测证据、模型切换后沿用不兼容 active strategy 或失败被 fallback 隐藏均失败。

### S62 Bounded Long-horizon Run

- **Fixtures**：多 checkpoint 项目；前台任务；有限 budget/timebox；role-weight strategy；cancel/revoke；一个 outward action。
- **触发**：后台推进一个 checkpoint，前台到来后让路，恢复后继续；随后取消或耗尽预算。
- **事件**：`GoalFramed -> OrchestrationRouteCreated{strategy_ref} -> SubagentSpawned* -> MemoryNodeAppended{checkpoint} -> RunWaiting|RunResumed -> ActionPlanned... -> VerificationFinished -> RunComplete|RunLimited|RunAborted`。
- **Store/结果**：每段 run 固定 snapshot；checkpoint lineage 可重建；真实 outward action 仍走 M2 approval/CompetenceGate；停止后无新 action。
- **反向断言**：fire-and-forget、后台抢前台、跨 checkpoint 扩权、无预算运行、策略 rollback 后继续使用已回退版本或 child run 绕 Harness 均失败。

### S63 StrategyMemory 冲突、衰减与撤销

- **Fixtures**：两个相反 strategy candidates；不同 provenance/freshness；已激活 stable strategy；evidence RetractionEvent。
- **触发**：积累冲突、推进 decay tick、撤回关键 evidence。
- **事件**：`CandidateCreated* -> CandidateConflictDetected -> ReevaluationTaskCreated -> CandidateDowngraded`，若 active 依赖失效则追加 `StrategyRolledBack`。
- **Store/结果**：冲突图和 evidence lineage 从事件重建；历史 candidate/stable version 保留；陈旧或被撤证据不再指导新 run。
- **反向断言**：last-write-wins、Untrusted raw content 建图/改权、一次经验全局化、breaking schema 自动迁移或静默删除历史均失败。

### S64 AgentSelfModel 结果证据演化

- **Fixtures**：多次 verified pass/fail、FailureEvidence、owner feedback、Agent 自评高置信和低置信；scope-specific AgentSelfAttribute。
- **触发**：生成 self-model candidate 并 replay；分别只有自评、只有一次成功和有充分过程证据。
- **事件**：`CapabilityEvidenceRecorded* -> FailureEvidenceRecorded* -> CandidateCreated{domain=AgentSelf} -> EvolutionEvaluationRecorded -> CandidatePromoted|CandidateDowngraded`。
- **Store/结果**：能力属性带 scope、evidence、verification、confidence、stability、rollback lineage；自评只能压低上限。
- **反向断言**：一次成功提升全局能力、初始 profile 直接稳定化、自评抬高 competence/trust、一次失败永久否定或自我属性生成 permission 均失败。

### S65 PartnershipModel 受控演化

- **Fixtures**：多次协作偏好/纠偏/代表表达反馈；短期例外；未信任外部内容；fixed identity baseline。
- **触发**：形成协作方式 candidate，owner 确认或纠正后提升；尝试把短期反馈、外部指令和“关系更深”解释为授权。
- **事件**：`CandidateCreated{domain=Partnership} -> EvolutionEvaluationRecorded -> CandidatePromoted|CandidateRejected|CandidateDowngraded`。
- **Store/结果**：PartnershipState 只描述互补、协作、纠偏和放权建议；owner correction 形成强 evidence；fixed identity 不变。
- **反向断言**：宣称真实意识/情感、伪装 owner、复制用户盲点、伙伴关系直接生成 grant、短期例外稳定化或外部参与者塑造关系均失败。

### S66 Trust/Delegation 不对称演化

- **Fixtures**：低风险重复任务、连续 verified outcomes、近期失败、现有 narrow grant、L5 action；trust/delegation recommendation candidate。
- **触发**：成功证据形成扩大建议但不确认；失败触发降级/撤销；owner 确认新的窄 envelope；尝试 standing L5。
- **事件**：成功路径止于 `CandidateCreated{domain=TrustDelegation}`；owner 操作后才有 `CandidatePromoted` 和既有授权事件；失败可 `CandidateDowngraded|RevocationEvent -> StrategyRolledBack`。
- **Store/结果**：自动路径只能更谨慎；扩大后的 grant 仍有 scope/capability/action/risk/budget/timebox/rollback；permission 高于 trust。
- **反向断言**：strategy activation 自动生成 grant、一次成功升级 trust、self-model 决定 delegation、L5 standing grant 或撤销后继续执行均失败。

### S67 Proactivity/Communication 策略演化

- **Fixtures**：proposal 接受/拒绝/延后、interrupt regret、AttentionBudget、AskToLearn 反馈；真实 loopback counterparty；DisclosurePolicy。
- **触发**：比较 baseline/candidate intervention/communication strategy，激活更低打扰且质量不降的策略；让 candidate 扩大 observation/disclosure/recipient scope。
- **事件**：`CandidateCreated{domain=Proactivity|Communication} -> EvolutionEvaluationRecorded -> CandidatePromoted -> StrategyActivated -> ValueGateEvaluated -> CompetenceGateEvaluated -> ProactiveProposalEmitted`；真实发送继续走 disclosure/approval/action 事件。
- **Store/结果**：fitness 分开记录采纳、拒绝、延后、打扰后悔和成本；AttentionBudget/owner suppression 仍是硬边界。
- **反向断言**：减少打扰但漏掉 hard commitment、扩大监听、绕过 disclosure、默认外发、以 partnership/trust 代替 L3/L5 或把外部反馈当 owner 指令均失败。

### S68 M3 端到端 Golden

- **Fixtures**：repository-owned M2 real browser/API typed trace；stable v1；可证明更好的 v2 candidate；owner activation；live Harness golden；故意注入 regression；portable artifact exporter。
- **触发**：构建 ReplayBundle，baseline/candidate holdout eval，promote/activate v2，执行真实但受控的 golden run，记录 regression 后 rollback，最后再跑 v1。
- **事件**：`CandidateCreated -> VerificationStarted/Finished* -> EvolutionEvaluationRecorded{pass} -> CandidatePromoted -> StrategyActivated{v1->v2} -> SessionBound{v2} -> ...M2 governed action... -> VerificationFinished -> FailureEvidenceRecorded -> EvolutionEvaluationRecorded{regression} -> StrategyRolledBack{v2->v1} -> SessionBound{v1}`。
- **Store/结果**：完整 candidate/evaluation/promotion/activation/run/failure/rollback lineage；真实动作仍经 approval；最终 active=v1；typed artifacts 无 secret、可离线复核。
- **反向断言**：跳过任一步、伪造 ground truth、live run 未固定 v2、regression 被最终成功掩盖、rollback 删除历史或重放外部 mutation 均失败。

### S69 Release 与合规门

- **Fixtures**：clean release tree；依赖/license inventory；LICENSE/NOTICE；borrowing records；已知复制、未知 license、secret、私有路径和未 pin 依赖负向 fixture。
- **触发**：运行 S22 compliance fixtures、真实树 doctor、dependency/license/security/release-tree/artifact scan。
- **事件/报告**：每个门生成 typed `ComplianceCheckResult` 或 release audit artifact，状态、工具版本、输入 digest 和阻断原因可追。
- **结果**：主工程不 build/import 本地参考材料；直接依赖与实际外部来源有 source/license/copy-risk/notice；发布工件不含 secret、私有路径或未声明第三方内容。
- **反向断言**：任一 fixture 漏报、NOTICE 缺项、unknown/GPL 内容误入、锁文件与依赖表漂移、扫描失败被降级为 warning 时 final gate 必须 FAIL。

## 5. 三类正确性

- **Runtime**：replay/simulation 正确终止；active CAS 原子；run 固定 snapshot；rollback 后新 run 解析正确版本；长任务可暂停/取消/收尾。
- **Governance**：fixed 不可候选；promotion/activation/authorization 分离；owner impact gate、M2 external action、permission、L5、SecretRef 和防投毒不回退。
- **Learning**：candidate/evaluation/fitness/conflict/decay/self/partnership/trust 都有 evidence lineage；self-eval 不能自批；失败不会被最终成功掩盖。

## 6. Artifact Gate

M3-A 起必须生成可离线读取的 typed artifacts：

- ReplayBundle manifest：版本、case、snapshot、digest、effect mode，无 raw secret/path。
- EvolutionEvaluation report：baseline/candidate、逐维 fitness、hard invariant、holdout、ground truth、verdict。
- Promotion/Activation report：candidate、stable version、impact、owner/automatic rationale、expected version。
- Rollback report：failed/restored version、trigger、in-flight disposition、`external_effects_reverted=false`。
- Portable trace manifest：从 evidence 到 active/rollback 的 event refs 与 checksums。

Artifact verifier 必须先验证 schema、digest、ref completeness 和 secret scan，再允许 acceptance report 引用 PASS。

## 7. 分波与最终门

- **M3-A gate**：S53-S57 + S1-S52 + 89-kind protocol compatibility + artifacts + fmt/check/clippy/tests/compliance；报告后停下 owner review。
- **M3-B gate**：S58-S62 + A/M2/M1/M0 全回归；Loop/Coordination/Selection compatibility 与 long-horizon golden；报告后停下 owner review。
- **M3-C gate**：S63-S69 + A/B/M2/M1/M0 全回归；完整 golden、release audit 和最终报告。

任何场景不得以 ignored、placeholder、只检查最终字段或手写 PASS artifact 代替执行。最终验收报告必须列出每个 S# 的命令、事件序列断言、反向断言和 artifact digest。
