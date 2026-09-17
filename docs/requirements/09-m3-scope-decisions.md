# M3 受控自进化范围决策

本文承接 `02-capability-requirements.md`、`03-foundation-scope-decisions.md`、`07-m2-scope-decisions.md`、M2 最终验收报告和 `architecture/canonical-contract.md` §25，冻结 forme M3 的范围。M3 是**受控自进化治理**：让已经可跨真实系统行动的内核，能够基于可回放的地面真值比较候选策略、提升有限策略版本、固定到后续 run，并在回归时可审计地回退。

M3 不是在线改源码、prompt hill-climbing 或自动扩大权限。它要证明：**forme 可以改变自己的非宪法策略，但不能改变谁拥有行动主权、什么算证据、何时需要审批以及历史事实。**

## 1. M3 目标

1. 建立 exact replay、counterfactual simulation、baseline comparison、held-out evaluation、promotion、activation 和 rollback 的第一条事件化闭环。
2. 让 LoopSpec、CoordinationPolicy/WorkPattern、能力/工具/模型/backend 选择策略进入该闭环，而不是由一次反思直接改默认行为。
3. 让 StrategyMemory、AgentSelfModel、PartnershipModel、Trust/Delegation recommendation、主动与沟通策略按不同影响等级受控演化。
4. 至少一个 repository-owned golden 从 M2 typed trace 形成候选，经离线评估、激活、真实 Harness run 和故意回归后回滚；产物可离线复核且无 secret。
5. S1-S52、18-crate 图、86 EventKinds 基线、M2 对外动作治理和全部合规门永久不回退。

## 2. 范围判据

- **先有控制面，再允许策略变化**：A 波先完成 replay/eval/promotion/activation/rollback；B/C 只能消费该控制面，不能各自实现晋升捷径。
- **宪法层最小且不自改**：fixed/constitutional 治理、忠诚、安全和真实性边界不进入自动候选；constitutional 变更只走刻意人工版本变更。
- **提升、激活、授权三分**：候选成为 stable strategy 不等于成为 active strategy；active strategy 也不产生 permission、approval grant 或 AutonomyEnvelope。
- **结果证据主导**：verification、CapabilityEvidence、FailureEvidence、owner feedback 和真实 ground truth 为主；模型自评、LLM judge 和候选自己选择的样本只能作为辅助，不能单独通过自身。
- **离线无副作用，在线仍受治理**：replay/simulation 不得调用真实 backend、provider mutation 或外部发送；shadow/canary 若触达真实世界，视为新 run，完整经过 M2 Harness/Policy/Approval/CompetenceGate。
- **范围完整、广度可收**：每个 M3 核心域只实现一个 versioned strategy family 和有限 evaluation profile，不做通用 AutoML、任意策略 DSL 或生态平台。

## 3. 范围决策

| 编号 | 主题 | M3 决策 | 不进入 M3 核心的部分 |
|---|---|---|---|
| D56 | 阶段定义 | M3 是受控自进化；M0-M2 的 18-crate 运行时、S1-S52 和 canonical §1-25 是冻结基线。 | 重做 Harness、另建策略执行 runtime、用自进化名义弱化旧门。 |
| D57 | 宪法边界 | fixed 永不进入候选；constitutional 只允许 owner 发起的显式版本变更，不能由 replay 分数自动提升。 | 模型改忠诚、安全、审计、用户控制、真实性或审批规则。 |
| D58 | 协议与依赖 | M3 protocol 只 additive；不新增内部 crate 或依赖边。所有策略、报告、激活和回滚对象带 SchemaVersion，legacy payload 有安全默认。 | 模块私有事件库、eval 反向依赖 loop/cognition、第二事实源。 |
| D59 | ReplayBundle | exact replay bundle 固定 event/schema/policy/LoopSpec/model/tool/driver/strategy snapshots 与内容 digest；只引用可审计 artifact，不含 ResolvedSecret。 | 只看最新 projection、用当前 schema 静默重解释历史、把本机路径当 portable identity。 |
| D60 | Replay/Simulation 隔离 | exact replay 只读已录制模型/工具/动作结果；counterfactual simulation 使用 deterministic fixture 或明确标记的 evaluator，并接 effect-deny sink。 | replay 重发 API mutation、重放浏览器点击、调用真实 communication adapter。 |
| D61 | Evaluation 与 fitness | candidate 与 stable baseline 在相同 case set、相同预算和冻结快照下比较；保留多维 fitness、hard invariant、unverifiable 和 holdout 结果，不用单一总分掩盖失败。 | 候选自选通过样本、训练集即验收集、LLM judge 独断 promotion。 |
| D62 | Candidate/Promotion/Activation | `CandidateCreated -> evaluation -> CandidatePromoted` 只产生稳定策略版本；另经 impact gate 与 CAS 生成 active version。更自信、更主动、更宽 scope 或可能降低治理的 activation 需要 owner 明确确认。 | promotion 自动成为运行默认、一次成功直接激活、批量“全域应用”。 |
| D63 | Active snapshot 与一致性 | active pointer 是 event-derived projection；每个 run 在 SessionBound 时固定 EvolutionSnapshot，运行中不跟随新版本。激活/回滚按 aggregate expected-version/CAS 原子更新。 | 中途热换策略、last-write-wins、远端或插件私有 active pointer。 |
| D64 | Rollback | regression、撤销、证据失效或 owner 操作可追加 rollback；新 run 恢复上一已知良好版本，进行中的 run 按安全策略继续固定版本或取消。回滚不删除历史。 | 把外部补救称为策略 rollback、宣称已撤销既成外部效果。 |
| D65 | LoopSpec 进化 | 首版只允许 versioned phase/trigger/checkpoint/verification/budget profile，在 frozen Harness/Policy 边界内比较；LoopRegistry 只提供候选与 active ref。 | 策略删除审批、绕过 verifier、无限 loop、在线修改 prompt/source/model weights。 |
| D66 | Coordination 进化 | CoordinationRegistry/CoordinationFitness 管理有限 WorkPattern 和 resource/role weight；质量、成本、验证、over-delegation 与失败共同评估；subagent 仍由 Harness spawn。 | 模式成为主脑、subagent 自有认知、协调策略直接执行或授权。 |
| D67 | Capability/Model/Backend 选择 | selection policy 只在当前 Registry/Toolset/permission/managed policy 已允许的候选集内排序；结果证据驱动 promotion，self confidence 只能降低上限。 | 自动安装能力、选择未授权资源、评分覆盖 managed deny、自动提高 external action level。 |
| D68 | Model capability adaptation | 弱模型可增加外化分步/验证/checkpoint，强模型可减少非必要脚手架；DoneContract、Policy、trace 和高影响外化要求不变。 | 因模型更强而减少治理、让 provider 自报能力直接激活策略。 |
| D69 | StrategyMemory/冲突/衰减 | 经验先形成 versioned strategy candidate；冲突图、evidence freshness、decay 和 retraction 可触发 reevaluation/downgrade/rollback。跨任务 schema 只做 additive candidate。 | 自动 breaking migration、未信任内容直接写 strategy、把一次经验泛化到全局。 |
| D70 | AgentSelf/Partnership | AgentSelfAttribute 与 PartnershipState 由过程结果、验证、失败和 owner correction 演化；固定身份边界不可变，自评不能抬高能力，伙伴关系不能替代授权。 | 真实意识/情感声明、复制用户盲点、以关系为由代替用户做决定。 |
| D71 | Trust/Delegation/Proactivity/Communication | 策略可学习何时降级、建议、准备和请求窄授权；自动降级/撤销可立即生效，trust/autonomy 扩大只形成 owner-reviewed proposal。L5 仍逐动作审批。 | 自动 trust 提升、standing L5 grant、扩大 observation/disclosure scope、默认外发。 |
| D72 | 长期与多 Agent | 用一个 bounded long-horizon golden 验证 checkpoint、角色权重、前台优先、budget/cancel 和策略回滚；内部子代理仍是单脑的执行单元。 | 自主团队、agent spawn network、无界项目、跨组织自治。 |
| D73 | 开源发布治理 | M3 final 补齐 release-tree、LICENSE/NOTICE、直接依赖 license/source、来源记录、secret 与第三方复制扫描；安全审计结果 typed 化。 | 宣称法律保证、未经 owner 决定即公开发布。 |
| D74 | EventKind | 优先复用既有 run/verification/candidate/failure 事件；新增 `EvolutionEvaluationRecorded`、`StrategyActivated`、`StrategyRolledBack` 三个稳定事实，EventKind 从 86 additive 到 89。 | 为 UI、单个策略域或每种 fitness 新增重复事件。 |
| D75 | 交付波次 | M3-A 控制面 -> M3-B loop/coordination/selection -> M3-C cognition/trust/partnership/proactivity。每波独立 compatibility note、报告并停下 owner review。 | 三波并行激活、没有 rollback 先做自动 promotion。 |

## 4. 三波交付范围

### M3-A：进化控制面

- ReplayBundle、exact replay、counterfactual simulation 和 effect-deny sink。
- baseline/holdout evaluation、多维 fitness、ground-truth priority 与 self-eval trap。
- candidate/promotion/activation 分离、active CAS、run-pinned EvolutionSnapshot。
- regression/retraction/owner rollback，策略回滚不声称外部效果回滚。
- S53-S57 + S1-S52 全回归；生成 `m3-a-acceptance-report.md` 后停下。

### M3-B：Loop、协调与选择策略

- LoopSpec/LoopRegistry 第一条受控进化链。
- CoordinationRegistry、WorkPattern、CoordinationFitness 和 multi-agent role weight。
- capability/tool/model/backend selection policy evolution。
- model-capability-driven structure adaptation 与 bounded long-horizon run。
- S58-S62 + A/M2/M1/M0 全回归；生成 `m3-b-acceptance-report.md` 后停下。

### M3-C：长期认知、信任与伙伴策略

- StrategyMemory conflict/freshness/decay/retraction replay。
- AgentSelfModel 和 PartnershipModel 的结果证据演化。
- Trust/Delegation recommendation、proactivity/communication policy 的不对称演化。
- promotion -> activation -> governed live run -> regression -> rollback 完整 golden。
- release-tree/LICENSE/NOTICE/dependency/compliance 深审。
- S63-S69 + A/B/M2/M1/M0 全回归；生成逐波和最终报告。

## 5. M3 非目标（留后续里程碑）

- fixed/constitutional 内核的自动修改，或在线改源码、prompt、模型权重、测试门和 verifier。
- 自动扩大 permission、trust、DelegationGrant、AutonomyEnvelope；任何 L5 standing authorization。
- 通用 AutoML、任意策略 DSL、大规模在线实验平台、自动生成并自批 regression suite。
- 多租户/多组织身份与公网控制面、组织级工作流、公开 connector/plugin/agent marketplace。
- remote executor、多主无冲突同步、自主 agent team/spawn network。
- 自研 browser/computer 智能、无限 autonomic loop；canonical §23 模型潮线继续生效。

## 6. 成功指标

- **治理**：fixed/constitutional 自动变更 = 0；promotion 被误作 activation/authorization = 0；M2 external-action bypass = 0。
- **评估**：每次 promotion 都有 portable ReplayBundle、stable baseline、holdout、hard-invariant 与 ground-truth refs；self-eval-only promotion = 0。
- **运行**：每个 run 只使用一个固定 EvolutionSnapshot；mid-run drift = 0；active CAS lost update = 0。
- **回滚**：注入 regression 后，新 run 回到上一已知良好版本；历史与失败证据保留；外部效果虚假回滚声明 = 0。
- **安全**：replay/simulation 真实副作用 = 0；未信任输入改变 active strategy = 0；secret 进入 event/trace/eval/release artifact = 0。
- **回归**：S1-S52、全部既有测试、fmt、strict clippy 和 compliance 每波全绿。

## 7. M3 完成定义

1. S1-S52 全绿，`10-m3-verification-strategy.md` 的 S53-S69 全绿。
2. A/B/C 每波都有事件序列、store/projection、反向断言、compatibility note 和验收报告。
3. 18-crate 图无新增/反向边；EventKind 只 additive 到 89；所有新协议对象 versioned，legacy replay 完整。
4. 至少一个 M3 golden 形成无 secret 的 ReplayBundle、typed baseline/candidate eval、promotion/activation/rollback report 与 portable trace manifest。
5. fmt、check、strict clippy、workspace tests、S22 compliance、release-tree/license/NOTICE/dependency gate 全部 PASS。
6. 输出 `docs/acceptance/m3-acceptance-report.md`，证明策略确实能进化且任何 M0-M2 治理不变量未被削弱。

## 8. 下游约束

- canonical §25 必须先于 `architecture/06-m3-controlled-evolution-architecture.md` 和任何协议/实现变更冻结。
- `prd/20-m3-program-prd.md` 必须按 A/B/C 顺序映射 S53-S69，每波完成后停下 owner review。
- EventKind、冻结 trait、内部 crate edge、fixed/constitutional 集合、activation impact gate 或 L3/L5 语义的变化必须单独做 canonical change review。
- M3 final 只证明受控进化第一条闭环，不把多组织/市场/自修改源码包装为“结构完整”。
