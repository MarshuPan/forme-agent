# 多 Agent 执行编排：认知集中，执行分布

本文档是 forme 的 V14 规划补强。它承接 `10-coordination-kernel-and-resource-orchestration.md`（协调内核）、`14-agent-self-model-and-partnership-growth.md`（Agent 自我模型）、`../architecture/02-proactive-cognitive-kernel.md`（认知内核设计基准）和 `../requirements/02-capability-requirements.md` R12（subagent 与 delegation），回答一个此前未正式定义的问题：

> 一个目标或任务设定后，forme 以什么路径、调用多少个 Agent 去完成，而**不破坏"只有一个大脑"**？

本文仍然是规划文档，不定义最终 Rust crate/API、数据库 schema 或实现步骤。本文涉及的同名对象、对象 owner、子代理边界等口径，以 `../architecture/canonical-contract.md` 为准。

## 核心原则：认知集中，执行分布

> forme 只有一套认知（记忆链/思维链/认知地图/AgentSelfModel/性格）——就是认知内核设计基准里一直在定义的那个"大脑"。**子代理不持有认知**，它们只是大脑派出去的**执行单元**，用完即弃，或长期但仍无独立认知。

我们一路给"大脑"设定的属性（思维链、记忆链、自我模型、性格）都是**大脑独有的、单数的**。子代理从大脑借一片**被裁剪过的上下文 + 一套被分配的资源**去干活，干完把结果交回大脑，由大脑决定什么进候选/稳定层。

## 明确否决：多独立 Agent 路线

一种常见做法是：每个 Agent 独立，各自对接 MCP、Skills、消息渠道和身份——等于装一套之后定义多个不同身份协作。

forme **不走这条**。原因不是风格偏好，而是它会摧毁 forme 的命题：

- 记忆碎片化：每个 Agent 各学各的，没有统一的用户理解。
- 认知不累积：自我模型和认知地图无法长在一个主体上。
- 信任对象不唯一：用户要面对多个身份。
- 协调开销：Agent 之间互相协商。

forme 的全部价值在于**那一个会成长的大脑**。一旦拆成各自带记忆的独立 Agent，就等于亲手拆掉它。

## 单脑模型

1. **单一中心**：forme 可创建无数子代理和流程，但核心大脑只有一个；用户只跟主大脑沟通。
2. **代理透明**：为执行目标创建的子代理由大脑调度，用户不直接与它们打交道。
3. **资源统一**：MCP/Skills/工具/插件只长在大脑里，由大脑按需把**最小够用的子集**分给它控制的子代理或 workspace 使用。

## 不是第四内核，是"执行编排"子系统

- 大脑还是三内核（Evolution / Coordination / Proactivity）——它们是"思考"。
- 多 Agent 是"执行"，不该升成第四内核（升了就违背"认知集中"）。
- 它落在两处：**协调内核决定编排拓扑**（这是 `ExecutionRoute` 的一部分）+ **`Orchestrator` 执行编排**（spawn / 限定 / 分配资源 / 按拓扑跑 / 交接 / 整合）。
- `Orchestrator` 是 harness 下的一等子系统，不是内核。**协调选打法，Orchestrator 执行打法，资源始终在大脑。**

## 三层概念

| 层 | 是什么 | 与已有概念的关系 |
|---|---|---|
| **Workspace** | 项目隔离边界：文件、约束记录、认知地图 scope、记忆 scope。大脑跨 workspace，但每个 workspace 隔离。 | 一等 scope 边界（memory/cognitive-map 已是 scoped 的，workspace 把它实体化）。 |
| **Agent（子代理）** | 执行单元。临时（为节点而生、用完即弃）或长期固定（可复用的专长角色）。**都无独立认知**，长期的也只是固定 profile。 | = R12 的 SubagentRun（临时）+ 可选的长期 AgentProfile（M1+）。 |
| **约束性记录** | 该 workspace 的目的/意义/任务约束，**不是 Agent 记忆**。 | 声明式、自上而下的 charter（这个空间干什么、红线是什么）；区别于 CognitiveMap 的学习式、自下而上（从经历蒸馏的判断框架）。两者都 scoped 到 workspace。 |

约束性记录与 `DoneContract` 分工不重叠：**约束性记录是 workspace 级常驻、选编排"前"的输入**（它过滤哪些编排在此空间合规/合适、哪些红线不能破）；**`DoneContract` 是实例化"后"每个节点/整体的完成标准**（拓扑定了之后才形成）。一前一后，各管一段。

### 工作空间记忆 vs 大脑记忆（认知集中的再确认）

- **大脑两层**：核心层 = 思维链 + 记忆链（每日/长期/会话）+ 记忆图（A1）+ 认知地图（A2），**进化（A3）和主动性都在这层**；基础层 = 常规数据记录（= Raw Storage）。
- **工作空间记忆是独立的"常规"一层**：目标 + 历史动作 + 操作流程 + 向量找回，为的是隔一阵不碰也能快速找回状态；它**没有自己的大脑**，不带思维链/记忆链/主动性。
- **但项目不是"没人想、不进化"**——只是这些都放在主大脑里：大脑对每个项目持有并**主动生长**它的 CognitiveMap（怎么判断、什么质量、哪里会错），并**长期监控、监管、反思、调整**（针对该项目的 ProactiveJob）。开一个长期工作空间不是 fire-and-forget，大脑要"养"它（"管生不管养"被禁止）。
- 一句话：**项目"状态"在工作空间（常规、被动、可找回）；项目"认知 + 长期照看"在大脑（认知、主动、会进化）。**

## 编排库：角色（积木）+ 编排模式（拼法）

多 Agent 编排 = 大脑把认知用在"如何组织自己的执行"上。它和 A2 认知地图的 `JudgmentFrame` 同构：

| A2 认知地图 | 多 Agent 编排 |
|---|---|
| JudgmentFrame = 结晶化的"怎么想" | 编排模式 = 结晶化的"怎么执行" |
| 有 frame 就套，没就探索式走 | 有合适编排就套，没有就现生成定制 |
| 走对了沉淀成候选 frame | 现生成的有效就沉淀成新编排 |
| 经 A3 提升、带置信和适用范围 | 经 A3 提升、带适用签名和 fitness |

所以**编排库住在大脑的认知里**（过程/策略记忆），走和认知地图、进化引擎**同一套机器**。两层：

- **角色库（积木，多数内置）**：planner / researcher / builder / evaluator / integrator… 每个角色 = 一份"行为 profile + toolset 切片 + DoneContract 形状"。子代理被实例化成某个角色。
- **编排模式库（拼法，seed + 沉淀生长）**：把角色拼成拓扑的模板，带适用签名和 fitness。

行为规则：

- **匹配优先**（快、稳、被验证过）是默认路径；**现生成定制**（慢、糙、未验证）是兜底。
- **契合度闸**：只有现成编排的适用签名对当前任务契合得足够好才用；不够好 → 走定制，不硬套。契合度阈值本身学来的（被匹配后失败 → 调低契合分）。
- **低置信配更强验证**：匹配到的高置信编排正常验证强度跑；现生成的低置信定制编排加更多评估关卡/checkpoint，必要时先让用户确认打法再开跑（能力门原则套到编排上）。

## 适用签名：6 维长相 + 案例锚 + 学到的契合函数

适用签名不是写死的特征表，而是**种子粗起步、证据磨准**的契合函数。匹配分两步：

```
1. 推断任务"长相"（认知动作：大脑用 CognitiveMap + 目标框定，读出编排相关特征）
2. 拿长相给各编排签名打契合分 → 最高且过阈值就用，否则定制
```

签名 = 三块：**结构特征区**（下面 6 维手写种子，可解释可治理）+ **案例锚**（指向记忆图里该编排成功/失败过的真实任务）+ **学到的契合函数/fitness**。其中案例锚就是 A1 记忆图节点，所以"匹配编排" = 在记忆图上**刻意遍历（查结构特征）+ 联想（找相似成功案例）**，复用 A1 的两种走法。

种子 6 维"编排相关长相"：

| 维度 | 取值 | 决定 |
|---|---|---|
| 可分解性 | 整块 / 可并行拆 / 须串行分阶段 / 迭代精炼 | 要不要拆、怎么拆 |
| 依赖结构 | 子部分独立 / 互相依赖 | 并行 vs 串行/分层 |
| 可验证性 | 自验即可 / 需独立 evaluator / 需环境实测 | 要不要 evaluator 角色 |
| 目标清晰度 | 清楚 / 需先澄清 / 需先探索 | 要不要 planner/澄清前置 |
| 尺度 | 单发 / 多阶段 / 长任务 | 要不要 checkpoint/持久 artifact |
| 风险/可逆 | 低可逆 / 高不可逆 | 关卡/审批强度（接能力门） |

6 维是粗种子，**可增删**：用一阵后案例锚堆起来、契合函数磨准，甚至发现"原来这一维才是关键"。推断长相本身也可能错（→ `cognitive_map_failure`/`goal_framing_failure`），同样被证据校准。

## 种子编排库（出厂自带）

前 4 套不 spawn 子代理、后 4 套才拆——"单 vs 多"本身就是匹配里的一步。

| 编排模式 | 角色 / 拓扑 | 6 维种子签名（只列决定性维度） | 何时命中 |
|---|---|---|---|
| single-run | 大脑直接做 | 整块 · 单发 · 低险 · 自验即可 · 清楚 | 简单、清楚、低险、能自验 |
| retrieve-then-answer | 检索/读取 → 答 | 整块 · 单发 · 需先读本地或检索 | 答案在已有资料里，先取再答 |
| tool-loop | 工具 → 观察 → 续 | 整块 · 需环境实测 · 迭代 | 必须靠环境反馈一步步推进 |
| clarify-then-act | 先澄清 → 再做 | 清晰度=需先澄清 | 目标/约束/成功标准不清 |
| generator-evaluator | generator ⇄ evaluator（对抗循环） | 可验证=需独立 evaluator · 迭代精炼 | 质量难自判、易自评陷阱 |
| parallel-research-fanout | N×researcher（并行）→ integrator | 可分解=可并行 · 依赖=独立 · 领域=research | 子问题可并行且互不依赖 |
| planner-builder-evaluator-integrator | planner → builders（并行）→ evaluator → integrator | 可并行+须分阶段 · 需 evaluator · 多阶段 · 需先规划 | 复杂、多组件、要构建+验证 |
| long-running-harness | initializer → 循环[挑一项→建→验→提交]（每轮 独立上下文） | 长任务 · 须持久 artifact + checkpoint | 跨多轮/多小时的大任务 |

每套带：角色组合（拓扑模板）+ 6 维种子签名 + 空案例锚（待积累）+ 初始 fitness。库之后只增不删地长（定制有效 → A3 沉淀成新模式）。

## 13 种常见模式按三层归位

常见的 13 种 Agent 模式**不是 13 个平级编排**——它们分属三层，在 forme 里各有家；**绝不能平铺进编排库**：

| 层 | 模式 | 在 forme 的家 |
|---|---|---|
| **L1 单 Agent 执行/推理策略**（一个 Agent 或一个 DAG 节点怎么跑/怎么想） | Single / ReAct / Plan-and-Execute / Reflection / Self-Refine / Tree-of-Thoughts / Graph-of-Thoughts / 多层循环 | LoopSpec / 单 Agent WorkPattern；有几个种子库已有（ReAct≈tool-loop、Single≈single-run） |
| **L2 能力底座**（每个模式都用，不是编排） | Tool Use / Memory | 执行触点（工具运行时）/ A1 记忆图 + 上下文引擎——**不进编排库当"模式"** |
| **L3 多 Agent 编排拓扑**（多个子代理怎么组合） | Multi-Agent / Supervisor / Router | 编排库正主：coordinator-fanout-merge / supervisor-schedule-merge / intent-router-dispatch |

三个补充判断：

- **同一意图常有 L1/L3 双实现**：如"提质量"——低 stakes 用 L1 Self-Refine，高 stakes 用 L3 generator-evaluator（独立 critic）。质量关键场景偏多 Agent，因为自评是陷阱（self_eval_trap）；大脑按 stakes/成本/能力门挑。
- **完整组合链（Router→Supervisor→Planner/agents→ReAct/Tool→Memory→Reflection→Final）是库里一个"重型组合模式"，不是默认主干**。只用于复杂大任务；简单问答绝不走这一整条（否则又慢又贵又不稳）。
- **索引大脑**：库里每个模式都登记 功能 / 原理 / 设计初衷 / 适用场景 / 操作方法 + 6 维适用签名——这构成编排库的“索引大脑”，也是模式匹配的输入。

## 拓扑表达：模板 → 实例化 → 具体 DAG

```
编排模式（模板，库里）—— 抽象：角色槽 + 拓扑模板 + 适用签名
   ↓ 对具体任务实例化（绑子任务、分资源、定每节点 DoneContract）
具体子任务 DAG（实例）—— 节点=子任务+角色+资源切片+DoneContract；边=依赖/交接；拓扑=并行/串行/分层+评估关卡
   ↓
Orchestrator 按 DAG 执行（spawn/限定/交接）
```

映射到现有协调对象：**编排模式 = 一种 `CoordinationSpec`/`WorkPattern`（模板）**，**DAG = `ExecutionRoute`（实例）**。完全在现有协调对象体系里，只是把"扁平 fanout"换成"带角色的分层 DAG"。

两个一致性细节：

- **资源最小授予**：大脑给每个子代理只分该节点最小够用的工具子集（least-privilege），子代理不能越界——接 `AutonomyEnvelope` + R12 限定 toolset。
- **不直接写稳定层**：子代理读快照/裁剪上下文、只回传结果，大脑整合后才进候选→提升——接并发模型（canonical §7）。

## 整合回大脑：两半

"整合回大脑"是两件事：

### (A) 输出整合（前台，run 的一部分）

DAG 通常以 integrator 角色收尾，但它只做"执行层能做的"：按总 DoneContract 拼装 + 对账 + **检测冲突**。真正涉及判断的"裁"**上抛大脑**——子代理永远不能悄悄把一个真冲突自己拍板（"认知集中"在整合层的落地）。

规则 = "能机械合就合，真冲突就上抛"，按冲突类型：

- **事实冲突**（子代理说法矛盾）：不许静默择一；按来源可信度+证据加权，裁不动 → 保留冲突关系 + 上抛/记成带冲突候选。**这就是 A3 的多源冲突检查**——子代理来的冲突和反思来的冲突同一套处理。
- **构建/设计冲突**（不兼容方案）：integrator 检测 → 能调和就调和，不能 → 大脑重新协调（带约束重跑节点或换编排）。
- **质量冲突**（evaluator 判产物不行）：触发该节点重跑；反复不过 → 上抛大脑（可能编排选错 → 重新协调）。

裁判权在大脑，跑腿在 integrator。整合的尺子始终是协调一开始定的总 DoneContract。

### (B) 认知整合（即时记一笔 + 背景 tick 深加工）

```
整个 DAG run → 一个 episode 进记忆图（A1），连着：目标 / 选的编排 / 各节点 / 结果 / 失败
  → 编排模式更新：成功 → +契合分 + 成功案例锚；失败 → −契合分 + 反例锚
  → 学到的（关于用户/项目/领域/自己的编排能力）→ 候选 → A3 提升
  → 节点失败/选错编排/整合冲突 → FailureEvidence
  → 子代理无认知、不直接写稳定层；大脑是唯一写者（接 canonical §7 单写者）
```

认知整合**完全复用 A1/A3**（episode + 候选 + 提升），不是新机制。**唯一真正新增的是输出整合那条"能机械合就合、真冲突上抛、按总 DoneContract 验收"的规则**。这趟经历也让大脑下次更会编排——契合函数更准、可能沉淀新模式。

## 内部编排 ≠ 对外 A2A

- **内部子代理编排（本文）**：大脑的手在并行，无独立认知，Orchestrator 管。
- **对外 Agent-to-Agent（`12-unified-gateway-and-communication-fabric.md`）**：forme 的大脑代表 owner 去和另一个 owner 的 Agent 对接，走对外膜。

两者别混：一个是"我的手"，一个是"我代表你对外"。

## 与已有文档的关系

- `requirements/02` R12（subagent = child run、独立上下文、限制 toolset、summary 回传、不绕过 parent policy）= 单脑地基，本文把它从"扁平 fanout"升级为"分层编排 + 编排库"。
- `archive/planning/10` 的 `CoordinationSpec`/`WorkPattern`/`ExecutionRoute`/`CoordinationFitness` = 编排模式/DAG/沉淀的对象体系。
- `architecture/02-proactive-cognitive-kernel.md` 的 A1（记忆图）/A2（认知地图）/A3（进化）/能力门 = 编排库的载体、生长引擎和置信治理。
- `archive/planning/12` 的 Agent-to-Agent = 对外，区别于内部编排。

## 关键边界（不可破）

1. **模式 ≠ 大脑**：13 种编排模式都是大脑的工具，永不当 forme 的主脑或最外层循环；大脑永远在它们之上决定用谁、何时用。
2. **选择动态、通用、非死表**：输出可以是"不进库、大脑直接处理"（简单任务），可以是辅助编排减负，可以是工作空间 + 单/多编排。举例不固定，大脑按实际任务用签名匹配 + 重评决定。
3. **长期任务的编排随时间变**：开了长期工作空间不是 fire-and-forget；编排可能一开始不需要、后期需要，或换/加。大脑周期性重评（接 re-coordination）。
4. **工作空间无独立大脑，但项目被大脑长期照看**：状态在工作空间（常规、被动），认知 + 监控 + 反思 + 进化在主大脑（scoped 到该项目、主动、会生长）。详见上文"工作空间记忆 vs 大脑记忆"。
5. **Loop ≠ 进化**：任务里的循环纠错（Loop 模式）是执行层纠错；大脑进化只发生在 A3 认知整合那一步。跑多少轮 Loop 本身不让大脑变强。

## M0/M1/M2/M3

- **M0**：固定单脑模型边界、Orchestrator 数据边界、R12 subagent、最小种子编排库（如 single-run / retrieve-then-answer / tool-loop + 一套多 Agent 如 generator-evaluator）、适用签名数据边界、Workspace 与约束性记录边界。匹配可用显式规则或单次模型判断，不做完整动态编排进化。
- **M1**：编排匹配体验、更多种子编排、DAG/编排 trace viewer、契合度用户反馈。
- **M2**：编排库沉淀生长、案例锚积累、编排 fitness scoring、long-running harness artifacts、跨 workspace 编排。
- **M3**：编排模式 replay/eval/promotion/rollback（接 `CoordinationFitness`/WorkPattern promotion），编排策略受控进化。

## 当前结论

forme 的多 Agent 协作应坚持**认知集中、执行分布**：只有一个会成长的大脑，子代理是它无认知的手；编排是大脑学来的一项认知技能（匹配优先、定制兜底、沉淀生长、契合度治理），由协调决定拓扑、Orchestrator 执行、资源统一在大脑、结果整合回大脑。它不是第四内核，而是协调的产出 + harness 下的执行子系统，全程不绕过能力门、A3 和并发/冲突治理。
