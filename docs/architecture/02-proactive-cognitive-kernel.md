# 主动认知内核设计基准

本文是 forme 的认知内核设计基准。它把此前 `archive/planning/01-14`、`planning/15`、`requirements/01-04` 和 `archive/architecture/00-01` 收敛出的结论,以及一轮专题设计讨论(记忆图、认知地图、进化引擎、协调、主动机制、能力门、性格、对外膜、runtime 接缝、canonical 收敛)固化为一份单一真相源。

本文的作用:

- 作为后续统一调整文档时的**设计叙事基准**——说明内核怎么转、各对象怎么咬合;与 `architecture/canonical-contract.md` 的口径契约配套,一起作为单一真相源。
- 作为正式 Rust 技术方案 `architecture/03-foundation-architecture.md` 的概念输入。

本文**不是** Rust 技术方案,也不是实施型 PRD。它定义"主动认知内核应该怎么转、各对象怎么咬合、哪些边界不可破",不定义 crate、接口签名、数据库 schema 或实现步骤。

文档关系:

- 上游:`archive/planning/01-14`、`planning/15`、`requirements/01-04`、`archive/architecture/00-foundation-direction.md`、`archive/architecture/01-agent-harness-and-loop.md`。
- 下游:`architecture/03-foundation-architecture.md`(原计划的 `02-foundation-architecture.md` 顺延)、`prd/01-foundation-implementation-prd.md`。
- 配套:`architecture/canonical-contract.md` 是进入正式架构前的唯一口径契约(三内核术语、L0-L5、AutonomyEnvelope、稳定性枚举、对象 owner、CompetenceGate、并发、撤销派生失效、schema 版本、身份模型、失败分类、开源 CI 门)。**遇到同名对象或同类判断,以 canonical-contract 为准;本文是它背后的设计叙事。**

## 1. 愿景锚定

forme 是一个**只属于单一 owner、为其深度定制的受治理认知伙伴**。

- **北极星 = 高质量的主动性。** 但主动性风险不对称:错误的主动是负价值、灾难级。因此深度认知、可靠实践和对用户的深刻理解是**承重结构**,不是锦上添花。没有这个地基,主动性是空的、甚至有害。
- **关系 = 增强型伙伴。** 继承用户的方向与质量标准,不继承其盲区与短板,在能力维度超越用户,并长出自己受治理的独立认知。像父子:有相似、但不是同一个人。
- **对外代理 = 主动性朝外的一面,是核心。** Agent 在授权下跨越边界代表 owner 处理事务。
- **严格单 owner。** 身份只有三种角色:owner、external counterparty(对方/对方 Agent)、代表 owner 发声的 Agent。无多租户。
- **独立的是认知,不动的是治理。** 成长与"独立人格"只发生在认知层的候选→提升链里;"谁说了算、能不能越界"永远锁在固定内核。

固定内核(`archive/planning/04`)不变:Harness-first、Policy-at-execution、User-control、Event-sourced audit、Candidate-before-promotion、Verifiable-before-stable、Reversible-by-design、No-real-consciousness-claim、**Loyalty-to-owner**(永远服务 owner、不可被摄入内容改向;摄入内容是数据非指令,认知输入防投毒见 canonical §15),外加 trust 永不覆盖 permission、任何扩展不绕过执行治理。

## 2. 架构主轴:主动决策路径

forme 成立与否的单一检验点,是这条不可旁路的主轴:

```text
授权信号 / 事件 / 时间触发
  -> ObservationFabric(只读授权范围)
  -> OpportunityDetector
  -> [门1] ValueGate         值不值得打扰/出手?
  -> [门2] CompetenceGate     对此 scope 的认知/信任/证据够不够?
        不足 -> 结构性降级:只记录 / 先学习 / 先问
        足够 ↓
  -> CoordinationReasoner(产出约束,不是分步计划)
  -> AgentLoop(模型在约束内规划 + 调工具)
  -> Policy / Approval / Delegation / AutonomyEnvelope enforcement
  -> Execution Backend(对内)或 Communication Fabric(对外代理)
  -> Verifier
  -> EventStore(Trace / FailureEvidence / Evidence)
  -> CognitiveLoop(跨 run/idle,产候选)
  -> 候选 → (用户确认 / 多证据 / 验证) → 提升 / 回滚
```

执行上下文只有两个,不是三个并列的 loop:

- **前台 Run(同步)**:唯一产生真实动作的地方。`协调相 -> 执行相 -> 验证相`。
- **后台自发执行上下文(异步,即"背景 tick")**:跨 Run、只读快照、只产候选与机会,绝不直接动手。

三内核(Evolution / Coordination / Proactivity)是能力域,不是执行上下文。两个执行上下文承载三内核的工作。`AgentHarness` 是容器与治理者,不是 loop。

及格线一句话:**能证明"认知/信任/证据不足时主动动作结构上发不出去;足够时能经协调内核落成受 harness 治理的行动,全程可解释、可撤销、可审计;独立认知的成长碰不到'谁说了算'。"**

## 3. 认知内核

记忆和认知分三层:`Raw Storage(原始事件)-> 记忆图(情景/事实)-> CognitiveMap(蒸馏 schema)`。进化引擎让后两层在治理下长大。

**记忆 vs 认知的分界(口径见 canonical §14)**:记"发生了什么/什么连着什么"是**记忆**(Raw Storage + 记忆图,含 Event/Episodic/Reflection);已在指导"怎么判断/行动"是**认知**(Semantic/Procedural/Principle/Strategy + CognitiveMap/UserModel/AgentSelfModel…)。分界点 = 原始记录 vs 已指导判断。session/每日/长期 memory 是记忆层的时间切片,认知层在其上——这是 forme 比常规 agent 多出来的一层。

### 3.1 记忆图(A1)

"链"分两种,过去被混用:

- **纵向链(成熟/提升)**:raw → episodic → reflection → candidate → stable。关于时间和证据(`archive/planning/13` 已有)。
- **横向链(连接/联想)**:一条记忆/概念/目标怎么连到另一条,激活在上面传播。过去几乎没有——它才是联想式主动需要的东西。

记忆建成一张**带类型、带权重、带稳定性层级的图**:

- **节点**:事件、片段、反思、事实、用户属性、Agent 自我属性、原则、思维痕迹。所有东西都是节点。多时间尺度属性("1 到 100")就是其中一类节点。
- **边(带类型/权重)**:时间先后、因果、关于(关乎哪个属性/项目/目标)、相似、矛盾、支持、派生自(provenance)、属于片段。
- 节点带稳定性层级(见第 10 节 D4):稳定节点重而黏,易变节点轻而易衰减。

关键关系:

- **记忆是地图,思维是走图 + 画图。** 一条思维链是在图上走的路径 + 铺下的新节点/边;走完沉淀回图,变成片段记忆。记忆与思维是一套结构、两个角色。
- **联想 = 在图上的激活扩散(spreading activation)。** 输入点亮节点,激活沿边扩散(权重 + 衰减),过阈值的节点浮上来成为念头。这是"突然想到某事就想着他"的机制,可实现、可追溯。
- **一张图,两种走法**:协调做**刻意遍历**(目标导向、抽丝剥茧),主动做**激活扩散**(自发浮现)。这张图是协调引擎和主动机制**共同的地基**——这就是"先有记忆链/思维链,内核才转得起来"的确切含义。
- **背景 tick 负责整理与遗忘**:加强常走的边、淡化/修剪无用节点,防止图爆炸;激活扩散本身受预算约束(接第 5.3 节)。

记忆图是事件日志之上的投影(projection),不是独立事实源,故永远可重建、可 replay。

### 激活扩散:真正的主动触发(A1 的动力学)

记忆图不只是静态结构,它有动力学——这是"**真正的主动**"(区别于定时器扫描)的所在。为支持激活,节点/边补几个字段:

- **节点**:`resting_activation`(静息值——目标/强关切/owner 在意的东西静息更高,"一直挂心上")+ `current_activation`(瞬时,扩散状态)。粒度取**有意义单元**(片段/事实/属性/目标/认知对象/实体),比原始事件粗。
- **边**:`weight` 可学——常走且有用的边变粗、没用的淡掉(Hebbian + 遗忘);稳定节点重而黏(慢衰减),易变节点轻(快衰减)。

核心循环:

```text
点燃(seed:新输入 / 当前上下文 / 高静息目标 / 变化节点)
  -> 扩散(沿"边权 × 每跳衰减"流向邻居;多路汇聚则激活叠加)
  -> 衰减(随时间 + 距离)
  -> 浮现(过阈值的节点 → 候选念头 → 过价值门/能力门)
  -> 预算(限跳数 / 限 top-K 前沿 / 限每 tick 总量;无 seed 不扩散)
```

**这一跃 = 扫描 → 涌现**:浮上来的常是"从多个方向被汇聚点亮"的节点,这就是"突然想到"的来源——非脚本、涌现。边权随"有用的浮现"被 A3 强化,**联想越用越准**。与协调对照:协调是**有向搜索**(顺边朝目标走、答问题),主动是**无向扩散**(从 seed 泛流、浮注意力),同图两走法。调参(衰减/阈值/预算/边权学习率/静息模型)是涌现行为旋钮,靠"建+观察+调",核心张力是噪声 vs 漏接。

**两级工作空间(2026-07 补)**:模型在单次调用**内部**已经承担了小容量、广播式的注意与推理。这不改变本节设计,而是明确了分界:**任务内的注意与推理让给模型那级(毫秒、单次前向、不可见、随即蒸发);forme 建的是它结构性没有的那级(秒到年:持久、事件化、受治理、可审计、换模型存活)**。两级同构、各占一层、互不越界;context 装配是两级之间的装货码头。口径见 canonical §23(模型潮线判据)与 §18(AgentWorkspace)。

### 3.2 认知地图(A2)

`CognitiveMap` = 从记忆图里**跨多次经历蒸馏出来的高置信 schema 层**——"在这个场景里,什么维度重要、怎么算好、哪里会错"。它是语义的、schema 的,不是"哪件事发生过"。

核心判断:**模型管通用认知,Agent 管特定场景的认知地图。** 这是模型从通用训练里拿不到的东西,是护城河。

对象:

| 对象 | 是什么 |
|---|---|
| `Scope` | 每张图都有作用域(用户/项目/领域/workflow/能力/风险)。不是一张全局图,是很多张分场景的图。 |
| `MapNode / MapEdge` | 记忆图里被精修过的高置信子图。 |
| `JudgmentFrame` | 结晶化的遍历模板——"遇到这类问题,按这个顺序看这些维度"。协调有 frame 就套,没 frame 就探索式走、走对了沉淀成候选 frame。 |
| `QualityModel` | 这个场景"好"的标准/红线/反例。直接喂 DoneContract 当验收标准。 |
| `BlindSpotModel` | Agent/模型/用户/workflow 老犯的判断错误。元认知层,从 FailureEvidence 蒸馏,预先避坑。 |
| `MapConfidence` | 每个节点/边/frame 的置信度 + 稳定性 + 证据。 |

要点:

- **绝不由单次事件生成**。背景 tick 的整理/反思注意到记忆图里的复现模式 → 候选 → 经 A3 提升。
- 每个地图节点链回它蒸馏自哪些片段(provenance),底层片段被推翻时上层 schema 要重评(接 D7)。
- **用法**:协调套 JudgmentFrame(怎么想)+ QualityModel(算不算好)+ BlindSpotModel(避坑);主动靠 BlindSpot/Quality 产念头、靠地图给联想加权;能力门读 MapConfidence 作"挣得上限"。
- **地图长厚 = 在该 scope 挣到专长 = 能力门为该 scope 打开 = 自治解锁**。这与第 6 节能力门、A0–A5 成长是同一个引擎。
- 与属性的切分:UserModel/AgentSelfModel 是**属性**(节点);CognitiveMap 是**schema/frame**(用节点搭的更高阶结构,引用属性)。属性是"是什么",地图是"怎么在情景里干得好"。

### 3.3 进化引擎(A3)

`EvolutionGovernor` 把候选治理成稳定知识。这是北极星"进化"那一半,是把"经历"换挡成"更能干伙伴"的变速箱。

**一条管线管所有**:用户属性、自我能力、JudgmentFrame、原则、trust、协调策略,都走同一条受治理的提升管线。候选带:改什么、目标稳定层级、证据、provenance、scope、冲突。

**中心原则(不对称)**:

> 让 Agent 更谨慎/更谦逊的知识,提升门槛低;让 Agent 更自信/更自治/更敢断言的知识,提升门槛高。

这一条同时实现防漂移、主动风险不对称、"自治是挣来的"。系统天然偏向谨慎(新增盲区、失败后降能力置信/降 trust、加质量红线 → 易升;调高能力置信、可被依赖的 frame、提升 trust/自治 → 难升、常需用户确认)。

**提升要过的闸(Governor 检查)**:

1. 证据阈值(够不够、是不是对的种类:过程证据 > 历史导入证据,阈值随目标层级放大)。
2. 冲突检查(矛盾绝不自动覆盖,记录冲突、要更多证据或用户裁决——防漂移核心)。
3. 来源/provenance 有效性(只有历史导入撑着不能进稳定层)。
4. 影响/可回滚(高影响或难回滚无论证据多强都要用户确认)。
5. (M3)replay/仿真。

**层级即治理旋钮,批准分三档**:稳定性层级(D4)设提升门槛与变化节奏;批准 = 自动(低层/证据强/无冲突/可回滚)/ 用户确认(高层/有冲突/高影响)/ 永不自动(固定内核,仅人工版本)。

**失败两速**:即时**快速、保守、可逆**的降级(下次小心)+ 慢速、需累积的稳定判定(不"一次失败永久自我否定")。

**全生命周期**:提升 / 降级 / 回滚 / 衰减。回滚靠 provenance/派生链传导(D7);衰减 = A1 的遗忘。

**运行位置**:织在背景 tick 里;需用户确认的提升作为主动 proposal 浮到前台过门。

**防漂移总保证**:稳定的东西不过门不动;新信息永不静默覆盖稳定认知;内核仅人工版本;每次提升可回滚;高影响提升要用户。这是"认知自由演化、治理纹丝不动"的落地。

## 4. 协调:以目标为基础的资源编排

协调 = 一个相对稳定的"求最优解"引擎,作用在一堆多时间尺度、各自按不同周期变化的资源/知识项上。**固定 + 动态的张力不在引擎里,在它读的数据里**:引擎稳定,动态住在那"1 到 100 项"各自的变化周期里。

完整形态(自我盘点是地基,过去被忽略的"隐藏逻辑层"):

```text
目标
  -> 【自我盘点】我已经有什么(AgentSelfModel:知识/能力/可靠的东西)   ← 地基
  -> 【看缺口】 我有的 和 目标要的 之间差什么
  -> 【定来源】 去哪个平台/来源找最对(靠 CognitiveMap)
  -> 【抽丝剥茧】顺图一层层 narrow 到结果
  -> 【出路线】 ResourcePlan + 去找的任务 + DoneContract + AutonomyEnvelope + DecisionTrace
```

边界与要点:

- **协调产"约束",不产"分步计划"**:GoalFrame、DoneContract、AutonomyEnvelope、资源白名单、DecisionTrace 种子;实际分步规划与选工具交给 AgentLoop 里的模型。消除双重规划,抗 stale_strategy。
- **能去找缺失资源**:最优解需要一个没有的资源时,协调产出"获取任务"(learning task / 子 Run),连着学习/进化闭环。
- **两个锚**:UserModel(用户要什么)+ AgentSelfModel(我有什么),协调坐在两者之间。AgentSelfModel 必须证据养成、不断进化——自我认知错,协调就错。
- **协调"选",harness"enforce"**:它产 AutonomyEnvelope,harness 在执行时强制;协调自己不 enforce、不授予执行权。

### 4.1 多 Agent 执行编排(认知集中、执行分布)

当协调判定一个任务值得拆成多 Agent,它产出的不是扁平并行,而是一张**带角色的分层 DAG**(planner/researcher/builder/evaluator/integrator 等节点)。核心原则:**认知集中、执行分布**——forme 只有一个大脑(记忆链/思维链/认知地图/自我模型/性格都是它独有的),子代理是它**无认知的手**,用完即弃,资源统一在大脑、按需最小授予。

- **不是第四内核**:协调决定拓扑(= `ExecutionRoute`),`Orchestrator`(harness 下子系统)执行;大脑仍三内核。
- **编排是学来的认知技能**:编排模式库与 A2 的 JudgmentFrame 同构——匹配优先(快/稳)、定制兜底(慢/糙/低置信、配更强验证)、有效则经 A3 沉淀生长;匹配靠"任务长相 6 维 + 记忆图案例锚 + 学到的契合函数",过契合度阈值才用。
- **整合回大脑**:输出整合由 integrator 拼装 + 检测冲突,能机械合就合、真冲突上抛大脑裁(= A3 多源冲突);认知整合就是把整趟 DAG 当一个 episode 进 A1/A3,子代理不直接写稳定层。
- **内部编排 ≠ 对外 A2A**(见 §8 对外膜)。

完整设计见 `../planning/15-multi-agent-orchestration.md`;口径见 `canonical-contract.md` §13。

## 5. 主动机制

### 5.1 念头的五个来源

substrate 不是静态仓库,是持续产生"念头"的场。每个属性/目标/证据不停产生候选念头:

| 来源 | 触发 | 冲动 |
|---|---|---|
| 缺口 | 对重要的东西置信太低 | 该去查/去问 |
| 变化 | 属性漂移或新证据与旧判断冲突 | 该提醒/该调 |
| 张力 | 两个属性/目标打架 | 该标记冲突 |
| 联想 | 新信息点亮某目标/兴趣 | 该分享/该建议 |
| 压力 | 长期任务该推进、deadline 近 | 该跟进 |

**五来源 = 激活的五种形状,不是五个独立检查**:有了 §3.1 的激活扩散,这五个其实是图上激活的五种可辨认形状——联想 = 汇聚激活;变化 = 节点被重新 seed;压力 = 目标静息值上升;缺口 = 被点亮但 confidence 低;张力 = 两个被同时点亮、又被 `contradicts` 边连着的节点。所以念头是"**一套激活动力学的涌现产物**",不是硬编码查五样。(2026-07 补:念头来源共六个——五个涌现 + 确定性的 `Commitment`(前瞻性意图到期触发,不经扩散),见 canonical §22。)

### 5.2 念头路由与触达梯度

念头的去向 = 一条触达半径梯度:

```text
念头
  ├ 内化        留在思想里,转成认知沉淀(走 candidate 治理)
  ├ 独立行动     伸手向"世界"要输入(学习/巩固),只作用于自己(source policy + 预算)
  └ 协作 / 对外代理  伸到"人"(owner / 外部),作用于关系(能力门强封顶 + 披露)
```

- **风险随触达半径单调上升**,故越往外门越严。
- **向内重力**:门只能让念头往回缩(协作 → 先问 → 只记下),永不往外推;往外触达必须靠念头自己的显著性 + 价值 + 能力挣得。这是 Agent 不变话痨/不越界的结构保证,也对应"大部分思考留在脑子里"。
- 三类人类主动映射:随意(联想念头,冲动自由、出口受治理)/ 驱动(压力念头,长期任务)/ 自发(联想 × 关系过滤,最依赖 substrate)。
- **内化是"私有默认、按需透明",不是"藏起来"**:大部分念头止于内化,形成 Agent 一片 owner 平时看不见的内心思考;但 owner 随时可问"你最近在想/琢磨什么",Agent 能把内化的思考流摊开。区分"没外化"与"隐瞒"——内化念头同样进事件日志,既是伙伴信任,也是审计要求。
- **独立行动(自发学习)不能乱跑**:它的学习议题必须从用户目标、已知缺口或失败热点长出来,受 source policy 和预算约束,**不是凭好奇心自由上网**(M0 非目标里的"无监督上网学习"对应的就是这条红线)。

### 5.3 控制力(执行调节层)

放开"生成",管"执行"。自发 = 生成自由;不乱跑 = 执行受控。执行调节层(Agent 的理性自控)叠几股力:

1. **预算/注意力经济(最该先建的核心)**:念头进优先队列竞价,优先级 = 目标相关度 × 价值 × 能力门;预算花完只能内化/排队。"追不起"本身就是最强控制力。
2. **目标层级(方向)**:不服务任何在册目标的念头默认止于内化。
3. **角色边界**:Agent 角色是"owner 的受治理伙伴/代理",框住哪些念头配执行。
4. **承诺守卫(抑制)**:执行中的 Run 不被新念头劫持,新念头进后台排队。
5. **节奏/批处理 + 问责/审计**:按节奏批量复盘;一切进事件、用户可审。

### 5.4 涌现的可抓机制

> substrate 冒念头(五来源)→ 念头路由给初始触达半径 → 沿向内重力,价值门 + 能力门 + 性格曲线决定缩到哪一层、以什么分寸落地。大部分止于内化,少部分独立行动,更少部分协作。

"涌现"不抓结果、抓来源:每个念头都指向 substrate 里一个具体来源,可枚举、可追溯、可治理;看着随性,实则有源、有据、有度。内核"转起来"就是这个产念头—浮现—过门的不停循环;定时器 tick 只是最朴素的兜底节拍,真正的转动来自 substrate 自己。

## 6. 两道门:价值门 + 能力门

`ProactiveEmissionGuard = ValueGate AND CompetenceGate`,在 harness 出口对任何主动/对外动作强制。

- **ValueGate**:值不值得打扰/出手。其阈值就是"性格/有度"的旋钮,按用户学。
- **CompetenceGate**:不是 yes/no,是**等级上限**——`CompetenceGate(competence, scope, risk) → 允许的最高 InterventionLevel`,低于阈值封顶在 Observe/Ask。
- **统一公式**:`有效主动等级 = min(AutonomyEnvelope 授予上限, CompetenceGate 挣得上限)`,ValueGate 在其内决定动不动。
- **放在 harness 出口**(权威 enforcement),keyed on `ActionIntent.source` + `expected_effect`;门在治理者里,内核不能自己给自己开门。
- **输入**:MapConfidence、UserModel/AgentSelfModel 置信度、TrustProfile、近期 FailureEvidence、证据类型(ImportedHistoricalEvidence 降权)。这是 substrate 上那些 confidence/stability/evidence 元数据的**主消费点**。
- **冷启动 = 成长模型的运行时实现**:seed agent(A0)地图全空 → 封顶在 Observe/Ask;靠观察 + 追问挣 competence → 逐步解锁 Suggest/Prepare/Act。自治是挣来的,不是配置的。
- **校准闭环**:主动对了(被采纳/验证)→ competence↑ → 上限↑;主动错了 → FailureEvidence → competence↓ → 上限↓ → 更保守。
- Q4/Q5/Q6 决定:阈值与细 scope 评分**学习机制 v1 全实现**,但从种子值起步、随证据成长(机制满,值随用而长);冷启动默认 Observe + Ask。

## 7. 性格:真实运作指纹,不造情感

Agent 性格是真的,但扎根在"怎么运作",不扎根在"怎么感受"。**假装情感是空的、假的**;MBTI/星座是给有内在感受者做的情感分类,不适用。

真实维度(都不需要情感):主动气质(触达衰减曲线)、认知风格、审慎画像(门阈值)、能力形状、沟通风格、互补姿态。

- 这些都是连续、可学、有证据的;组合空间连续且巨大,规模上自然聚成可发现的"原生性格簇"。
- **性格不用建,会自己长出来**:它是 substrate 稳定层参数 + 陪这个用户长大的轨迹的涌现指纹(AgentSelfModel + PartnershipModel + 门阈值 + 协调偏好)。
- **"看起来感性"的真身 = 高分辨率 UserModel 关注 + 响应精度**,是行为事实不是感情:不造感情,但真的可以很贴。
- 控制力(第 5.3 节)的设置本身就是性格的一半——调控制力即塑造性格。
- 对外时(第 8 节)性格服务于代表 owner,绝不夹带自己的议程。

## 8. 对外膜(B5)

对外 = 同一个内核 + 一层"对外膜",因为对面不是 owner,要多收紧三道边界。这是风险最高、收得最紧的面。

- **披露边界(能说什么)**:默认最小披露;DisclosurePolicy 按 对面 × 目的 分 scope;**还要挡住可被推断出来的信息**(最难)。
- **代表边界(怎么说)**:永远以"owner 的 Agent"身份说,绝不冒充本人;三档置信(授权断言 / 带保留代表 / 回到本人)折叠进 `DisclosurePolicy` 的 representation 字段(不作为独立对象,见 canonical-contract §10)。**代表置信 = 实际 competence**(能力门对外硬封顶,绝不替 owner 断言它自己没把握的东西)。
- **会话边界(容器)**:每次对外是 CommunicationSession(目的/参与者/形式/披露 scope/TTL/预算/终止/transcript);ExternalCommunicationGrant 授权创建;防无限套话、烧 token、跑题。

机制映射:对外主动需 standing grant + envelope,否则升级给 owner;对面输入不可信,只进 evidence/candidate(既防泄露也防操纵注入);协调资源集扩出沟通资源,DoneContract 含披露 scope 与终止条件。

A2A:两边都有膜,短会话、强目的、强终止、弱记忆写入;更可控(双方受治理)也更危险(无人在环),故终止策略要硬、结果通知 owner。

## 9. 与 runtime 的接缝(B4)

身体是已有的 runtime(harness、Run/Session/Turn、事件日志、policy/approval/delegation、执行 backend、持久化)。统一接缝原则:

> 内核负责"想和提议",harness 负责"enforce 和持久化"。内核很强但无特权——绝不自我门控、绝不直接写稳定态、绝不直接行动、绝不绕过。每个产出都重新进入 harness 治理。

具体接缝:

| 内核部件 | 接法 | 不变量 |
|---|---|---|
| 背景 tick | 受治理的内部 surface;机会变 source=proactive 的 RunRequest,候选走治理写入 | 只读快照、只发提议,不直接行动/写稳定态;不是后门 |
| 协调相 | harness 绑定 Run 后、AgentLoop 前调用,产 envelope | 协调选,harness enforce |
| 能力门 | harness 出口不变量,读内核置信度 | 门在治理者里,内核不能自门控 |
| A1/A2/A3 写入 | 内核提议候选,store + A3 governor 决定稳定 | 无组件直接写稳定认知 |
| 事件化 | 一切写 append-only 事件,背景 tick 读事件日志 | 记忆图是事件日志的投影,可重建、可 replay |
| 对外膜 | 对外是出口动作,harness/gateway 强制;ChannelAdapter 归一 | 无 adapter 直接调内核 |
| 执行 backend | 模型提工具调用 → toolset 重查 → policy → approval → backend | 连协调都不能授予执行权 |

runtime 相对现有文档要补:内部发起的 Run;EvolutionGovernor 作为 harness 子系统;记忆图作事件日志投影;能力门出口不变量;DecisionTrace 作一等 join 对象。

这条原则同时化掉两个评审隐患:harness 是薄治理者(生命周期 + 咽喉 enforce + 事件日志 + 路由),丰富逻辑在子系统里,不是胖单体;模块 owner 切清——存储归 `store`、认知语义归 `cognition`、协调归 `coordination`、enforcement 归 `policy/approval`、对外归 `communication/gateway`。

## 10. 口径对齐(以 canonical-contract.md 为准)

canonical 口径(三内核术语、L0-L5、AutonomyEnvelope、稳定性枚举、对象 owner、CompetenceGate、并发、撤销派生失效、schema 版本、身份模型、失败分类、开源 CI 门)以 `architecture/canonical-contract.md` 为唯一真相源,遇到同名对象或同类判断以它为准。本节是这些口径的设计侧回声,便于对照阅读;两者冲突时以 canonical-contract.md 为准。

此外,三处文档一致性修复已在本轮 pass 落实(属文档修复,不属契约口径):L-1(`requirements/04` 的 S13–S17 已改写为对外膜的数据模型 + enforcement 点验收,用模拟对手方,不要求真实信道)、L-2(`archive/planning/02` 的 M0 已含最小运行的背景 tick,不再“只预留事件”)、C-1(AgentGrowthModel 幽灵引用已删除)。

### D1 — 唯一的等级体系

两个正交轴 + 两道上限:

- **触达 Reach**:`内化 / 独立行动 / 协作 / 对外代理`。
- **介入强度 InterventionLevel(唯一 L0–L5,采 requirements/01 语义)**:`Observe / Suggest / Prepare / ActWithApproval / ActAutonomously / HighImpact`。
- **两道上限**:`AutonomyEnvelope`(授予)、`CompetenceGate`(挣得);有效上限 = min(两者),触达越外门越严,默认向内重力。
- 废止:archive/planning/04–05 的 record_only/ask_or_warn 别名语义;archive/planning/11 的 D0–D5 平行梯子(并入 AutonomyEnvelope);archive/planning/08 的 L0–L4 截断(补全 L5)。

### D2 — 介入对象去重(每个只回答一个问题)

| 类别 | 对象 | 回答 |
|---|---|---|
| 持久模型 | UserModel | 用户是什么 |
| 持久模型 | AgentSelfModel | 我是什么 |
| 持久模型 | PartnershipModel | 长期怎么协作(关系状态) |
| 策略 | InterventionPolicy | 何时/以何强度介入(性格旋钮) |
| 策略 | DelegationPolicy | trust → 自治 映射 |
| 策略 | DisclosurePolicy | 对外能披露什么 |
| (折叠) | DisclosurePolicy.representation | 对外代表的置信档(= 能力门输出);RepresentationPolicy 不作为独立对象 |
| 决策输出(不存) | augmentation move(原 AugmentationModel) | 此刻具体怎么补强 |

### D3 — 三者运行关系

见第 2 节:两个执行上下文(前台 Run + 背景 tick),非三个并列 loop;CoordinationReasoner 产约束、模型产步骤;CognitiveLoop 在背景跑、M0 即有最小运行版。回改 archive/planning/02 的"M0 只预留事件"。

### D4 — 唯一的稳定性枚举

每个认知节点/属性/地图元素声明其一,决定写入规则 + A3 提升门槛 + 变化节奏 + 回滚策略:

```text
fixed         内核/身份        仅人工改版本
constitutional 治理/原则        高门槛,人工确认
stable         长期特质/能力画像/稳定地图节点   多证据、可回滚、常需用户确认
working        当前focus/偏好/伙伴状态  近期过程证据
session        会话状态/当前自我状态    会话内有效
ephemeral      假设/瞬时激活      默认过期,永不稳定化
```

archive/planning/06(两套)、13(5 档)、14(7 档)全部映射到这一套。

### D5 — 能力门

见第 6 节。等级上限模型,min(授予, 挣得),AgentSelfModel/MapConfidence 为输入,harness 出口 enforce,冷启动 Observe/Ask。

### D6 — 并发/一致性

前台每 session 串行队列;事件日志唯一事实源,append-only,每聚合单写者;背景 tick 只读快照、只发候选/机会,绝不直接写稳定态;稳定写入走 A3 governor 按聚合串行;记忆图作投影。

### D7 — 撤销/派生失效

每个稳定认知对象记双向血缘(证据 → 对象 → 派生物);撤销/降级 = 事件,触发对派生物的重评估(失效候选,不自动删);A2 地图节点链回记忆片段,A3 回滚顺链传导。

### D8 — 唯一的失败分类

FailureTaxonomy(archive/planning/11 的 12 类)= canonical;CoordinationError(archive/planning/10)= 协调域视图;AgentLoop 停止原因中属于失败的映射进去。失败喂 A2 的 BlindSpotModel + A3 的降级侧。

### 逻辑矛盾裁决

- **L-1(S13–S17 vs D16)**:M0 对外验收 = 对外膜的"数据模型 + enforcement 点"测试(模拟对手方走标准事件路径,证明披露/代表/能力门/终止会正确拦截/限定/终止),不要求真实信道。S13–S17 据此改写。
- **L-2(M0 生成候选 vs 只预留事件)**:M0 含最小但在运行的背景 tick/CognitiveLoop。回改 archive/planning/02。
- **C-1(AgentGrowthModel 幽灵)**:删除。成长 = AgentSelfModel + 阶段 A0–A5 + A3 引擎。

## 11. 未决设计项与诚实定位

以下是方向已对齐、但**刻意留到后续专门深挖**或**属于实现阶段**的事项,记录在此避免遗漏:

1. **真正的主动式触发(已设计)**:见 §3.1"激活扩散"(记忆图动力学:点燃→扩散→衰减→浮现)+ §5.1"五来源 = 激活形状"。定时器 tick 是 M0 地板,激活扩散是把主动升级为"涌现式"的那一跃(M2+);扩散参数属涌现行为,留实现阶段建+观察+调。
2. **内化的内心活动接口**:私有默认、按需透明(见 §5.2),其检索/呈现方式待细化。
3. **M0 深度分配(架构阶段)**:结构不可缺(requirements/03),但深度优先给承重墙(harness/protocol/event/policy)+ 主轴(主动决策路径)+ 伙伴脊柱(AgentSelfModel/成长链);异域沟通对象先建结构、浅实现且放在可替换 trait 后面。认知本体论会在接触现实后改形状,故为"会改"而建(可替换 + schema versioned)。
4. **诚实定位**:M0–M2 是"受治理的候选生成器 + 人工(用户)提升";自进化闭环(replay/eval/promotion 驱动)只在 M3 合上。不要在 M0 语境把 forme 说成"已会自进化"。

## 12. 当前结论与下游关系

forme 的主动认知内核已收敛为一条可检验的主轴:

> substrate(对用户和自己的全维度认知)冒念头 → 路由给触达半径 → 双门 + 性格曲线决定缩到哪层、以什么分寸落地 → 协调把"挣得上限"翻译成约束 → AgentLoop 在约束内执行 → harness 在出口 enforce → 验证/失败/反馈回到候选 → A3 治理提升 → 专长长厚 → 能力门打开 → 自治解锁 → A0 走向 A5。认知自由演化,治理纹丝不动。

下一步:

1. 以本文第 10 节 canonical 决定为基准,统一过 `requirements/`、`archive/architecture/00-01`、`planning/` 文档,该优化优化、该补充补充、该修复修复。
2. 编写 `architecture/03-foundation-architecture.md`,把本文映射为 Rust 模块、协议、数据流、存储与治理边界。
3. 编写 `prd/01-foundation-implementation-prd.md`。
