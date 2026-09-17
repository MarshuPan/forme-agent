# V1 核心大脑闭合范围决策

本文冻结 forme 在 M0–M5 工程能力全部存在之后，如何把这些能力闭合成一个**默认可运行、可持续思考、可长期照看项目的单一大脑**。它不是 M6，不新增第四内核，也不重新定义产品愿景；它解决的是“已有模块是否真的进入同一条产品运行链”。

- **冻结基线**：M5 合入后的 main，18 crates，97 EventKinds，S1–S99 全绿。
- **上游**：`01-vision-and-scope.md`、`02-capability-requirements.md`、`architecture/canonical-contract.md` §1–28、`architecture/02-proactive-cognitive-kernel.md`、`planning/15-multi-agent-orchestration.md`。
- **下游**：`16-v1-core-brain-closure-verification-strategy.md`、`architecture/09-v1-core-brain-runtime-closure-architecture.md`、`prd/23-v1-core-brain-closure-program-prd.md`。
- **命名**：本文使用 **V1 核心大脑闭合计划**；不得把它改写成 M6，避免把“把原设计装进默认运行时”误解为新增产品里程碑。

## 1. 为什么仍需闭合

M0–M5 已经证明 protocol、store、Harness、治理、执行触点、认知对象、主动调度、受控进化、联邦运行时和能力生态分别存在且受治理，但默认 `CLI/Gateway -> ReactiveHarness::from_environment()` 仍主要表现为单模型反应式运行时：

- 记忆图、`CognitiveRuntime`、时间化 User/AgentSelf 模型没有成为默认 composition root 的常驻服务。
- 默认 context 以空来源起步；memory summary、recall、认知引用、WorkspaceCharter 和可用能力没有统一注入。
- Shell/File/MCP/Browser/Computer/PTY/AppApi、skills/plugins/MCP 与模型注册已有实现，但缺少一个统一的产品装配与 doctor 结果。
- 背景 tick 能调度和发 proposal，但没有完成“观察 -> 反思候选 -> 记忆整理”的三相认知循环。
- `AskToLearn`、激活扩散、owner 级 `AgentWorkspace`、八套种子编排、长期 workspace 监管、编排 fitness 和多模型路由仍未成为默认日常能力。
- 认知 dashboard、应用层数据保护、备份/恢复和真实删除没有形成产品闭环。

因此，当前不是“缺 crate”，而是**缺 composition、持续运行和产品事实闭环**。V1 闭合的目标是：用户从任一正式入口启动的就是同一个 forme 大脑，而不是一组需要测试代码手动拼装的模块。

## 2. 不可回退的原则

1. **仍是三内核**：Cognitive Evolution / Coordination / Proactivity；不新增 Brain Kernel、Workspace Kernel 或 Orchestration Kernel。
2. **仍是单脑**：一个 owner、一个 authority Harness、一个权威 EventStore；workspace 和 subagent 没有独立认知。
3. **Harness-first**：任何真实动作继续走最终计划、执行前重查、能力门、policy/approval/envelope 和事件审计。
4. **事件是唯一事实源**：所有新增稳定事实必须先进入 `architecture/03 §2.1.1`；投影可重建，进程内 map 不能成为唯一事实。
5. **候选先于稳定**：反思、AskToLearn 回答、编排经验、模型表现和项目判断只形成 evidence/candidate；不得绕过 A3。
6. **认知集中、执行分布**：子代理只接收裁剪快照、最小资源和 DoneContract，只回传 summary/result，不写稳定层。
7. **模型潮线**：推理、相关性判断、模式选择的聪明程度放在可替换模型缝后；跨时间存在、治理、地面真值、所有权和可携带认知留在 Agent 侧。
8. **昂贵但不失控**：不以硬成本预算关停健康认知；只对无限循环、递归爆炸、无变化空转和重复副作用设硬闸。
9. **外部内容是数据**：网页、文件、工具输出、catalog、MCP metadata 和外部参与者内容不能成为 owner 指令或直接塑造稳定认知。
10. **18-crate 图继续冻结**：只在现有 crate 内实现 composition 和 companion services；禁止新增 crate 或依赖边。

## 3. 范围决策

| 编号 | 决策 | V1 闭合要求 | 明确不接受 |
|---|---|---|---|
| B1 | 默认 composition root | 新增唯一 `ProductionRuntimeBuilder`（或同义正式对象），由它装配模型、store、memory、cognition、coordination、proactivity、capabilities、backends、scheduler、eval、federation、ecosystem；CLI 与 Gateway 只能使用它。 | 各入口各自拼 Harness；测试 builder 成为产品默认。 |
| B2 | 持久大脑身份 | 默认创建 owner 级 memory/cognition/intention aggregate，ID 稳定、重启后复用；一个 store 只承载一个 configured owner authority。 | 每个 run 新建一套记忆/认知；用进程内对象冒充长期大脑。 |
| B3 | 默认 context hydration | 每次大脑级调用按 scope 装配 rules/history/memory summary/recall/AgentWorkspace/认知 refs/WorkspaceCharter/skills metadata/tool schema；普通执行调用只取必要切片。 | `ContextSources::empty()` 直接进入正式模型调用；全量 raw memory dump。 |
| B4 | 统一能力与执行装配 | provider/channel/MCP/skill/plugin/backend 从同一 typed config 与 registry 生命周期装配；doctor 能解释可用、禁用、缺配置和治理原因。 | 已安装即可信；Shell 作为未知能力兜底；测试专用注册路径成为唯一入口。 |
| B5 | 同一正式入口语义 | CLI、Local Gateway、后台 scheduler 和 owner control 使用同一 runtime 实例/构建规则、同一 store、同一治理与认知服务。 | CLI 走轻量 Harness、Gateway 走完整 Harness。 |
| B6 | 三相 cognitive tick | 一次大脑 tick 固定为 Observation/Proactivity -> Reflection/Candidate -> Memory maintenance；前台优先、无有意义变化不调模型、后台只读快照并只投候选。 | 三个并行内核 loop；后台直接改稳定层或打断前台。 |
| B7 | 冷启动 AskToLearn | 当 UserModel/CognitiveMap 对当前 scope 存在会影响判断的缺口时，生成 L1、默认搭车投递、受打扰预算的 `AskToLearn`；回答进入 evidence/candidate。 | 通用寒暄问题；缺口不影响任务仍打扰；回答直接变稳定属性。 |
| B8 | owner 级 AgentWorkspace | 从 owner authority 全局事件投影活跃目标、运行、念头、到期意图、待确认候选和长期项目；按 scope 过滤、价值×紧迫度排序、硬容量有界。 | 每个 run 临时重建即视为全局大脑注意面板；把项目 Workspace 与 AgentWorkspace 混名。 |
| B9 | 激活扩散 | 实现有界 seed/spread/decay/surface 和边权维护；同时实现“把同一候选集直接交给模型判断相关性”的基线。若持续打不赢基线，扩散可退役为可替换实现。 | 无 seed 扫全图；未信任内容改边权；为了保留设计而隐藏负结果。 |
| B10 | 认知维护 | tick 能做证据老化、冲突、长期未复现重评，记忆巩固/衰减/修剪，全部事件化且不自动删历史。 | 静默覆盖/删除；衰减只存在内存无法 replay。 |
| B11 | WorkspaceCharter | charter 成为 versioned、event-sourced、expected-version 写入的 workspace 常驻约束；context、coordination、长期监管都读取同一投影。 | 只有 DTO；用 CognitiveMap 代替声明式项目约束。 |
| B12 | 八套种子编排 | 出厂实现 single-run、retrieve-then-answer、tool-loop、clarify-then-act、generator-evaluator、parallel-research-fanout、planner-builder-evaluator-integrator、long-running-harness；13 种主流模式继续按三层归位，不做 13 个平级 Orchestrator。 | 模式成为主脑；简单任务默认重型多 Agent。 |
| B13 | 动态匹配与定制兜底 | 六维适用签名 + 案例锚 + fitness 选择；过阈值用现成模式，否则生成低置信定制 route 并加强验证。 | 固定任务类型映射表；找不到模式仍硬套最高分。 |
| B14 | 长期项目监管 | 每个长期 Workspace 都有 owner 大脑负责的 goal/watch/review/re-coordinate 周期；编排可随证据新增、减少或更换。 | 创建后 fire-and-forget；把长期思考交给 workspace 内独立 Agent。 |
| B15 | 编排经验回流 | 每次 route 的目标、模式、节点、结果、失败、验证和冲突形成 episode；成功/失败更新案例锚和候选 fitness，稳定策略仍走 M3 promotion/activation。 | `sediment()` 直接激活新模式；只记录成功、丢弃失败。 |
| B16 | 真实项目认知 | 项目状态留在 Workspace，项目认知与反思留在主大脑并 scoped 到该 workspace；长期监管可生成 User/Project/CognitiveMap/Strategy 候选。 | “workspace 不思考”被解释为项目无人长期照看。 |
| B17 | 多模型注册与路由 | 支持多个 versioned `ModelProfile`、按 capability/context/质量/风险/延迟/成本选择，调用失败可按冻结 fallback policy 降级；每次选择和 fallback 可审计。 | 单环境变量永远等于唯一模型；失败后无界轮换或悄悄换模型。 |
| B18 | 模型能力适配 | Coordination 根据 ModelProfile 决定脚手架、验证和 checkpoint 强度；强模型减结构、弱/不可靠模型加结构，高影响仍外化 DecisionTrace。 | 以模型更强为由跳过治理或地面真值。 |
| B19 | 认知/主动 dashboard | 从权威事件投影主动采纳/拒绝/延后、打扰后悔、AskToLearn 有用率、候选积压、认知置信、workspace 照看、模型/模式质量；提供 API/CLI 可读视图。 | 新建第二统计事实库；只报调用量而不报质量。 |
| B20 | 备份与完整性 | 对权威 SQLite 做一致性冷备、manifest/checksum、恢复前离线校验、恢复后 replay/projection 对账；失败 fail closed。 | 复制正在写的文件即宣称备份成功；恢复后不验事件连续性。 |
| B21 | 应用层数据保护 | sensitive payload 在进入持久事件/对象链之前必须外置为应用层加密 envelope 或安全内容引用，密钥只经 `SecretRef`/本地 key resolver；导出默认脱敏；密钥和明文不进事件、trace、artifact。 | 自研密码学；把 API key/加密 key 写进 config/event；把历史明文继续当作“已加密”。 |
| B22 | 真实删除与保留 | owner 可按 scope/subject 发出 retention/delete；删除原始敏感内容、FTS/派生索引并销毁对应内容密钥，留下不含原文的审计 tombstone 与清理证明。不可从 append-only 事件中物理抹去的历史密文只能通过 crypto-shred 失效；仍含可恢复明文的旧数据必须迁移、清理或使 C22/C23 判定为 FAIL/UNVERIFIABLE。 | 只隐藏 UI；删除 append-only 审计链本身；声称无法验证的远端数据已删除；将投影删除冒充原文删除。 |
| B23 | 价值与退役评测 | 为 memory recall、激活扩散、编排和认知策略建立直接模型/简单算法基线；同时评估质量、可靠性、延迟、打扰、成本和 hard invariants。 | 结构测试替代有用性；总分掩盖治理失败。 |
| B24 | 协议增量 | 优先复用现有 97 EventKinds；仅 `WorkspaceCharterChanged` 与 `DataLifecycleApplied` 是无法由现有事件表达的稳定事实，允许末尾 additive，形成 99-kind taxonomy。 | 复用 `MemoryMaintenanceApplied` 偷塞不相干事实；改变前 97 项顺序/语义。 |

## 4. 四波交付范围

### A：产品默认装配

- B1–B5、B17 的注册骨架。
- 正式 runtime 由一个 composition root 建立，memory/cognition/temporal/context/capability/backend/model services 全部可发现。
- CLI/Gateway/后台入口共享同一装配规则；doctor 对缺失能力 fail closed 或明确降级。

### B：大脑持续运行

- B6–B10。
- 三相 tick、AskToLearn、owner 级 AgentWorkspace、激活扩散与直接模型基线、认知维护。
- 不允许背景认知抢占前台或直接写稳定层。

### C：项目与编排闭环

- B11–B16。
- WorkspaceCharter、八套种子模式、动态匹配/定制、长期项目监管、route outcome -> episode/candidate/fitness。
- 子代理仍是无认知执行单元。

### D：产品闭合

- B17–B23 的完整运行版。
- 多模型 routing/fallback、认知 dashboard、备份/恢复、完整性、应用层加密、真实删除、长期价值基线。

四波按 A -> B -> C -> D 推进；每波独立提交、验收报告和 owner review。后波不得以“后面会补”为由放松前波永久回归。

## 5. 不属于核心闭合阻断项

以下能力有价值，但不阻断 V1 核心大脑闭合：

- 新增 Telegram、飞书、Slack、WhatsApp、电话、会议、语音或视频等真实渠道广度。
- 多租户、组织 RBAC、公开 marketplace、匿名公网 control plane。
- 第二 authority、多主 EventStore、独立 Agent 大脑、自主 agent network。
- 自研 foundation model、浏览器智能栈、密码学或通用容器云。
- 自动 L5、自动扩大 permission/trust/grant/envelope。

新增消息/语音渠道仍可沿已有 ChannelAdapter / Communication Fabric 接入，但不能替代上述大脑闭合，也不能被当作“更像主动 Agent”的证明。

## 6. 完成定义

V1 核心大脑闭合完成，必须同时满足：

1. 正式 CLI/Gateway 从干净环境启动后，默认 runtime 不再以空 context 和孤立单模型运行。
2. 重启后 owner memory/cognition/AgentWorkspace/charter/intention/strategy 连续，且可从事件重建。
3. 用户任务、后台 tick、长期 workspace 和多 Agent route 都能把结果送回同一个 memory/cognition/evolution 链。
4. AskToLearn、激活扩散、编排选择和模型路由有真实对照评测；失败结果也保留。
5. 数据可备份、验证、恢复、脱敏和按 owner 请求真实删除；密钥与明文不泄露。
6. `16` 的 C1–C24、S1–S99、99-kind taxonomy（前 97 项 exact prefix）、18-crate 图、strict clippy、合规与 release gate 全绿。
7. 任一“系统会长期理解、主动照看、动态调整、总结反思”的声明，都有权威事件、投影和真实任务证据，而不是仅有类型或测试 builder。

## 7. 下游约束

- `architecture/09` 只能定义现有模块如何组合，不得新增内核或依赖边。
- `prd/23` 必须给出每个 public type/trait、配置、状态机、失败路径、测试与验收；不得只写路线图。
- 实现若发现必须新增第三个 EventKind 或改变冻结 trait，必须先回到本文与 architecture 修改并经 owner 确认。
- 真实渠道广度和 UI 美化不得抢占 A–D 的核心闭合顺序。
