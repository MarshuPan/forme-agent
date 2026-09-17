# forme 第二版产品与底座规划设计

本文档是 forme 的第二版规划设计。上一版已经确认 `AgentHarness + AgentLoop` 的通用 Agent 底座方向；第二版在此基础上补入主动交互、自发学习和深度反思沉淀，并把这些能力统一收束到 `CognitiveLoop / Cognitive Plane`。V3 已在 `03-personal-augmented-agent-and-evolving-loop.md` 中把“第二分身”校准为“增强型个人主动智能体”，后续正式概念以 V3 文档为准。本文不是最终需求文档，不是技术方案，也不是实施型 PRD；它的作用是先给 forme 建立一个可评审、可修改、可继续推进的产品与底座基线。

状态说明：本文保留早期 M0/M1/M2 的探索性切分。后续 `requirements/03-foundation-scope-decisions.md` 已经把 M0 修正为“完整核心体系的第一条可运行闭环”，`requirements/04-verification-and-acceptance-strategy.md` 已经固定 M0 的验收方式。因此，本文中关于 M0 只预留 cognitive 边界、CLI 优先或 MVP 的旧表述，只作为历史规划背景，不作为当前最终范围。本文的 `CognitiveLoop / Cognitive Plane` 亦为历史术语——canonical §1 已收敛为三内核（Cognitive Evolution / Coordination / Proactivity），Plane 仅保留为历史与横切表述；同名对象口径以 `../../architecture/canonical-contract.md` 为准。

## 一句话定位

forme 是一个以 `AgentHarness` 为核心、以认知平面和认知 Loop 为长期演化方向的通用 Agent 工具底座。它不是单一 coding CLI，也不是固定场景 workflow，而是一个能承载多入口、长任务、工具执行、权限治理、上下文管理、扩展生态、多 Agent 协作和个人认知沉淀的本地优先 Agent runtime。

当前规划基线仍然应先做“可靠的通用 Agent 底座”，再在这个底座上逐步扩展具体产品形态。早期版本可以以 CLI 为主要入口，但内核不能被 CLI 绑死，必须为 Gateway、Web/TUI、自动化和平台消息入口保留一致的 run/session/event 协议。

中长期 forme 不应只停留在“用户提问、Agent 回答”的助手形态，而应走向“增强型个人主动智能体”的形态：它足够了解用户，能主动观察、主动学习、周期性反思，识别用户短板并补强用户，把长期经验沉淀为可更新的认知结构。这个方向由 `CognitiveLoop` 和横切的 `Cognitive Plane` 承载，不塞进普通 memory、prompt 模板或某个单点工具。

## 背景判断

通用 Agent 工程实践证明了四件事：

- 通用 Agent 平台需要控制面：多入口、session、插件、subagent、事件流、运行生命周期。
- 通用 Agent 产品需要能力层：工具生态、toolset、memory、skills、MCP、context compression、delegation、automation。
- 高可靠 Agent runtime 需要强内核：run/session/turn、事件协议、工具执行、安全审批、沙箱抽象、可恢复状态。
- 成熟 Agent 体验需要明确契约：权限模式、hooks、skills 按需加载、subagent 独立上下文、checkpoint/compact 边界。

harness/loop 研究进一步确认：模型不是 Agent，loop 也不是完整 Agent。真正的 Agent 产品边界是 harness。harness 拥有 session/run/turn、context、tools、policy、event、store、hooks、plugins、subagents、compaction、resume 和 verification。loop 是 harness 内部的执行状态机。

新的增强型主动智能体方向进一步补充了一个判断：forme 不只需要执行层 loop，也需要认知平面。执行层 `AgentLoop` 解决“这次任务怎么跑”；认知平面中的 `CognitiveLoop` 解决“Agent 如何观察、学习、反思、形成判断并更新长期认知”。二者都由 `AgentHarness` 统一调度和约束。

因此，forme 的核心路线应是 Harness-first + Cognitive Plane / Cognitive Loop。

## 目标

forme 第二版规划目标是建立一个可持续扩展的 Agent 底座，并确认长期主动智能体能力如何自然进入底座。

核心目标：

- 支持用户提交任务，让 Agent 能在一个可观察、可审计、可恢复的 run 中推进。
- 支持工具调用，并在执行层做权限、审批、沙箱、参数和副作用治理。
- 支持 session 和上下文管理，让长任务和多轮任务不会只依赖模型窗口。
- 支持事件流，让 CLI、未来 UI、Gateway、日志、测试和恢复都基于同一套协议。
- 支持本地优先扩展能力：skills、MCP、plugins、hooks、memory。
- 支持临时 subagent，用于复杂任务拆解、上下文隔离和并行研究。
- 为验证闭环留接口，让 Agent 输出能被 deterministic checks、trace 和后续 eval 检查。
- 为增强型个人主动智能体能力留出认知平面：UserModel、AgentSelfModel、PartnershipModel、CognitiveLoop、Reflection、LearningAgenda、PrincipleStore，以及 proposal 级主动补强策略。
- 支持非对称主动交互：Agent 可以等待更多上下文、主动追问、延迟总结、只记录不打扰，而不是机械一问一答。
- 支持自发性学习的治理边界：学习议题、可信来源、候选知识、验证、沉淀和撤销。
- 支持深度思考与沉淀：把日常事件升级为反思记录、认知更新、原则库和技能库，而不是流水账记忆。

非目标：

- M0 不做完整插件市场；M1 只做插件运行时生命周期，生态分发放到 M2 以后。
- M0 不做全渠道平台；M1 只补 Gateway 和有限平台适配，渠道矩阵放到 M2 以后。
- forme 不以大单体为目标，架构保持 harness-first 分层。
- M0 不承诺完整 OS 级 sandbox；M1 强化 restricted host 和策略抽象，完整 OS sandbox 放到 M2 以后。
- forme 不依赖云端闭源能力作为本地底座核心。
- forme 不宣称 Agent 具备真实意识；工程目标是构建有长期自我模型、用户模型、主动目标和反思机制的个人 Agent。
- forme 不让 Agent 无边界自主上网学习或自主执行危险动作；主动学习和主动行动必须受 policy、source trust 和 approval 约束。

## 初始产品形态

建议 forme 第一阶段采用“CLI + Local Gateway/App Server 同协议双入口”的产品形态，其中 CLI 是第一批高频人机入口，Local Gateway/App Server 是控制协议、未来 UI/API、后台任务和沟通触手的入口。

含义：

- CLI 是第一批用户入口，用于任务提交、事件查看、审批、session 管理和配置诊断。
- Runtime Core 不依赖 CLI，CLI 只提交 `RunRequest` 并消费 `Event`。
- Local Gateway/App Server 进入 M0，和 CLI 提交同一种 run、消费同一种 event、共享 approval protocol。
- Web/TUI 控制台放到 M1，但 M0 的 event store 和 session state 要能支撑它。

这样做的原因是：CLI 可以最快验证 runtime 和工具执行质量，但如果内核按 CLI 大单体写，后续 Gateway、UI、automation、platform adapters 都会重复一套 loop 和状态逻辑。

## 用户与使用场景

forme 早期版本面向需要本地通用 Agent 的高级用户、开发者和自动化工作流构建者。

早期场景应聚焦在“通用任务执行”，而不是先限定某个垂直行业：

- 在本地 workspace 中完成研究、文件处理、代码理解、文档生成、脚本执行。
- 通过 MCP 和插件连接外部工具、知识库和业务系统。
- 通过 skills 固化常用流程和领域操作方法。
- 通过 memory 保存跨 session 的稳定事实和偏好。
- 通过 subagent 拆分研究、分析、验证等低耦合子任务。
- 通过 hooks 和 policy 控制危险工具、审批和质量门禁。

coding 是重要场景，但不是唯一场景。forme 应具备 coding agent 能力，但底座不能被 coding 场景锁死。

从增强型个人主动智能体角度看，forme 还需要覆盖以下长期场景：

- Agent 能理解用户长期目标、偏好、工作风格和风险边界。
- Agent 能从多轮对话和任务结果中判断什么值得沉淀，什么只是临时上下文。
- Agent 能围绕用户目标主动形成学习议题，例如跟踪开源项目、技术趋势、工具生态和业务知识。
- Agent 能周期性输出更高层的反思，而不是只生成每日流水账。
- Agent 能在合适时主动建议、提醒、追问或保持沉默。

## 核心设计原则

### Harness-first

`AgentHarness` 是运行时本体，拥有 run/session/turn 生命周期。所有入口都调用 harness，不能各自实现 loop。

### Protocol-first

外部只依赖 `RunRequest`、`Event`、`RunResult`、`ToolCall`、`ApprovalRequest`、`SessionState` 等协议对象。CLI、Gateway、UI、automation 都消费同一事件协议。

### Policy-at-execution

权限不能只写进 prompt。模型提出 tool call 后，执行层必须重新检查 toolset、参数、路径、网络、命令、sandbox、approval 和副作用。

### Context-as-resource

Context 不是字符串拼接，而是有限运行资源。rules、memory、skills、session history、retrieval、tool schema 和 compression 必须分层治理。

### Event-sourced observability

关键运行边界都应发事件并落盘：run start、turn start、context build、model call、tool proposal、approval、tool start/end、compaction、handoff、verification、run end。

### Local-first extensibility

M0 先支持本地 skills、MCP stdio、本地 plugin manifest 和基础 hooks。分发市场、远端运行、企业策略后置。

### Complete M0, real core loop

M0 可以体验朴素，但核心闭环必须完整。宁可先把 `CompactionManager`、`SubagentRun`、`Verifier`、`CognitiveLoop`、`CoordinationReasoner` 做成窄接口，也不要后续推翻 harness/loop/store/policy/cognition/coordination 协议。

### Cognitive-loop-running

M0 不实现完整增强型个人主动智能体体验，但必须有最小运行中的 CognitiveLoop：它在 post-turn 或 idle 边界生成低置信候选，留下证据和审计事件，并接受 harness、policy、event store、candidate-before-promotion 的治理。否则未来会把主动学习和反思沉淀做成外挂脚本，无法真正影响 Agent 的行为决策。

## 总体分层设计

第二版采用“运行时六层 + 认知平面”的理解。运行时六层解决 Agent 如何被调用、如何执行、如何持久化；认知平面横切这些层，解决 Agent 如何长期观察、判断、学习、反思和沉淀。

| 层 | 责任 | M0 判断 |
|---|---|---|
| Surface Layer | CLI、未来 TUI/Web、API、cron、平台入口。 | M0 做 CLI，保留其他入口协议。 |
| Control Plane | run submission、session stream、状态查询、插件生命周期、未来 Gateway。 | M0 做 core API 和事件订阅边界，Gateway daemon 可后置。 |
| Runtime Core | AgentHarness、LoopEngine、Run/Session/Turn、ModelAdapter、Approval、ResumeState。 | M0 必须做。 |
| Capability Layer | tools、toolsets、skills、MCP、plugins、hooks、memory、subagents。 | M0 做最小闭环。 |
| Execution Layer | shell/PTY、文件、进程、sandbox adapter、MCP tool execution。 | M0 做最小 unified exec 和执行抽象。 |
| State Layer | event log、session state、transcript、config、secrets、lineage、FTS。 | M0 做 append-only event log 和结构化 session state。 |

`Cognitive Plane` 不放进上表作为普通层，因为它不是一个单独功能模块，也不是 memory 的别名。它贯穿 Surface、Control、Runtime、Capability、Execution 和 State：

- 从 `EventStore`、session state、task result、tool result、user feedback 中订阅观察信号。
- 通过 `ActionProposal` 向 `AgentHarness` 提交行动建议，再由 harness 转换成受 policy 约束的 `RunRequest`。
- 通过 `CognitiveUpdateProposal` 提交 memory、skill、principle、UserModel、AgentSelfModel、PartnershipModel 或 CognitiveMap 的更新候选；主动补强只作为 proposal 中的 `augmentation_strategy` 决策输出。
- 通过 `LearningTask` 提交学习议题，但学习执行仍然走 `AgentLoop`、tool runtime、network policy 和 source policy。
- 不能直接调用工具，不能直接写长期记忆、原则或用户画像，不能绕过 approval、policy、event store 和审计链路。

这样建模后，长期主动智能体能力不是后期外挂脚本，而是从 M0 的 event、state、policy、context 边界开始就被预留；同时它也不会反过来污染执行层，让早期底座变成不可控的自治系统。

## 核心对象模型

当前规划基线应围绕以下对象组织：

| 对象 | 含义 |
|---|---|
| `AgentHarness` | Agent 运行时主对象，负责编排 run/session/turn、context、tools、policy、event、store、extensions。 |
| `Run` | 一次用户或系统触发的 Agent 运行，有 run id、状态、预算、结果和 stop reason。 |
| `Session` | 长期会话和上下文容器，绑定 workspace、agent profile、history、memory scope。 |
| `Turn` | run 内的一次模型/工具交替执行边界。 |
| `Event` | runtime 对外的事实记录和流式输出单位。 |
| `ToolCall` | 模型提出并由 runtime 治理的工具调用 proposal。 |
| `ApprovalRequest` | 危险动作或策略要求的人类审批请求。 |
| `ResumeState` | approval、中断、工具失败、handoff 等可恢复状态。 |
| `AgentProfile` | 长期 agent/persona/profile，绑定默认模型、工具、skills、memory scope、policy。 |
| `SubagentRun` | 临时 child run，独立上下文、限制 toolset、summary/result 回传。 |
| `Verifier` | 对中间产物、最终产物或 trace 做确定性检查和后续 eval 的入口。 |
| `CognitiveLoop` | 认知平面中的运行机制，按 `LoopSpec` 推进观察、定位、决策、验证、反思和候选沉淀。 |
| `LoopSpec` | 可版本化的认知 loop 模板，定义阶段、输入、输出、适用场景、退出条件和风险边界。 |
| `UserModel` | 对用户偏好、目标、工作方式、风险偏好、表达习惯的结构化理解。 |
| `AgentSelfModel` | Agent 自身能力、限制、置信度、工具边界、能力短板和行为约束。 |
| `PartnershipModel` | 描述用户与 Agent 的互补关系、协作阶段、放权边界、纠偏方式和长期共同成长状态。 |
| `augmentation_strategy` | `ActionProposal`、`CommunicationProposal` 或 `LearningTask` 上的决策字段，判断这一次如何补强用户：顺从、提醒、追问、反证、学习、建议或请求确认。 |
| `LearningAgenda` | 主动学习议题队列，决定 Agent 应该关注哪些知识和技能。 |
| `ReflectionRecord` | 从事件和任务结果中提炼出的经验、判断、矛盾、原则和认知更新。 |
| `PrincipleStore` | 长期稳定的判断原则和行为准则，不等同于流水账 memory。 |

## Harness 与 Loop 设计

M0 的 `AgentHarness` 至少负责：

1. 接收 `RunRequest`。
2. 绑定 session、workspace、agent profile、policy profile。
3. 生成 run id、turn id、tool call id。
4. 调用 `ContextEngine` 构建每轮上下文。
5. 调用 `LoopEngine` 推进 model/tool/handoff/final output。
6. 在工具执行前调用 `PolicyEngine`。
7. 把所有关键边界写入 `EventStore`。
8. 支持 cancel、wait、stream、resume。
9. 在 run 结束时生成 `RunResult`。

M0 的 `LoopEngine` 不作为外部入口，只作为 harness 内部状态机。它至少支持：

- model call。
- tool call proposal。
- tool policy evaluation。
- tool execution。
- tool result append。
- final output。
- stop reason。
- max turn / budget / cancellation。
- approval wait/resume。
- error classification。

## 工具与执行设计

工具系统分为四层：

1. `ToolRegistry`：注册所有可用工具和 schema。
2. `Toolset`：当前 session/agent/subagent 可见工具集合。
3. `ToolRuntime`：执行工具、结构化结果、流式输出、错误分类。
4. `ExecutionBackend`：shell、file ops、MCP、browser、remote/sandbox 的具体执行器。

M0 推荐工具能力：

- shell/exec 最小实现：cwd、env、timeout、cancel、stdout/stderr streaming、output budget。
- file read/write 基础能力。
- MCP stdio：server 配置、tools/list、call_tool、allowlist、timeout。
- tool execution re-check：执行前确认工具在 toolset 内，并通过 policy。
- structured tool result：成功、失败、截断、artifact、sensitive flag。

不建议 M0 直接做完整 browser automation、远端 executor、完整 OS sandbox、插件市场工具分发。

## 权限与安全设计

M0 至少需要四类安全对象：

| 对象 | 责任 |
|---|---|
| `PermissionProfile` | 描述文件、shell、网络、MCP、plugin、skill 等能力边界。 |
| `ApprovalPolicy` | 决定 allow / ask / deny，以及审批缓存、过期、拒绝后的行为。 |
| `PermissionMode` | 用户当前工作模式，如 plan、ask、auto、trusted。 |
| `SandboxPolicy` | 描述 host/restricted/sandbox backend 的执行策略。 |

M0 的安全底线：

- 所有危险工具都必须经过执行层 re-check。
- shell、文件写入、网络、MCP、插件工具应有不同风险等级。
- approval request 必须可落盘，审批后可 resume。
- hook 可以观察或阻断工具调用，但不能绕过核心 policy。
- 配置必须区分 workspace scope 和 user/global scope。

## 上下文、记忆与 Skills

M0 上下文分层：

1. System base。
2. Agent profile。
3. Workspace/project rules。
4. Session history。
5. Memory summary。
6. Skills metadata。
7. Retrieved context。
8. Tool schemas。
9. Volatile runtime context。

M0 建议能力：

- rules：读取项目/用户规则，作为确定性上下文。
- skills：支持本地 `SKILL.md` metadata，正文按需加载。
- memory：先做简单 topic memory / summary interface，不急于做复杂自动写入。
- compaction：先定义接口、事件和手动/阈值触发边界；自动压缩策略可以 M1 增强。
- reflection：M0 可先保留记录类型和事件来源，M1 再做周期性反思。
- principle：M0 可先保留 store 边界，M1 再由 CognitiveLoop 维护。

关键边界：

- memory 不是权限系统。
- skills 是流程知识，不是工具权限。
- rules、memory、skills、retrieval 不能混成不可审计的大段 prompt。
- reflection 不是每日流水账，应记录判断变化、矛盾、原则和可复用经验。
- principle 不是 prompt 口号，必须有来源、更新时间、适用范围和撤销机制。

## 认知 Loop 与 LoopSpec

认知 Loop 在 forme 中不应被建模为静态角色列表或简单 prompt，而应被定义为认知平面的运行机制。`CognitiveLoop` 负责执行认知循环；`LoopSpec` 负责描述某一类认知循环的阶段、输入、输出、适用场景、退出条件和风险边界。九阶段 loop 只是 seed `LoopSpec`，不是永久固定流程。

`CognitiveLoop` 与 `AgentLoop` 的关系：

- `AgentLoop` 是执行层 loop，负责一次 run 内的模型调用、工具调用、结果回填和结束条件。
- `CognitiveLoop` 是认知平面的 loop runner，负责按选定 `LoopSpec` 推进长期观察、主动交互、自发学习、深度思考、认知更新和行为风格沉淀。
- `LoopSpec` 是可版本化的认知策略定义，决定一次认知任务采用哪些阶段、角色、条件和退出规则。
- `AgentHarness` 是调度容器，负责约束这两个 loop 的权限、状态、事件和生命周期。

seed `LoopSpec` 的默认九阶段模板：

| 阶段 | 作用 |
|---|---|
| 观察 | 收集用户输入、环境事件、项目变化、外部资料、任务结果。 |
| 定位 | 判断事件与用户长期目标、当前项目、风险边界的关系。 |
| 分解 | 通过多角色、多条件视角拆解问题。 |
| 推演 | 形成假设、路径、收益、风险、反证和依赖。 |
| 决策 | 选择立即回应、等待、追问、学习、执行、记录或沉淀。 |
| 行动 | 产出 `ActionProposal`、`LearningTask` 或 `ReflectionTask`，由 `AgentHarness` 决定是否启动 `AgentLoop` 或其他受控流程。 |
| 验证 | 对结果、证据、来源、工具输出和推理结论做检查。 |
| 反思 | 从成功、失败、冲突和反馈中提炼经验与认知变化。 |
| 沉淀 | 提交 UserModel、AgentSelfModel、PartnershipModel、CognitiveMap、Memory、Skill、Principle 和 LearningAgenda 的候选更新。 |

这九个阶段不是每次都必须完整跑完的固定链路，而是 seed `LoopSpec` 的默认模板。`CognitiveLoop` 应按事件类型、风险等级和复杂度选择或裁剪 `LoopSpec`：

- 低影响消息：观察 -> 定位 -> 决策。
- 普通用户反馈：观察 -> 定位 -> 反思 -> 候选沉淀。
- 学习任务：观察 -> 定位 -> 分解 -> 推演 -> 行动 -> 验证 -> 反思 -> 候选沉淀。
- 原则或用户画像更新：尽量走完整路径，并默认需要用户确认或可回滚审批。

裁剪规则的目的不是弱化深度思考，而是避免把所有小事件都变成重流程。真正需要深度的是高影响、高不确定性、会改变长期认知或会触发外部副作用的事件。

M1 的最小 `CognitiveLoop` 可以先不做完全自治，只做四件事：

- 非对称交互策略：判断何时回应、何时等待、何时追问、何时只记录。
- 周期性反思：从 event log 和 session state 中生成 reflection records。
- 认知更新候选：把反思结果提交为候选 memory/principle/skill/UserModel/AgentSelfModel/PartnershipModel/CognitiveMap 更新，等待确认或规则化提升。
- seed `LoopSpec`：把默认九阶段模板作为第一版可审计、可裁剪、可版本化的认知 loop 定义。

M2 再增强主动学习：

- `LearningAgenda`：围绕用户长期目标和当前项目生成学习议题。
- `SourcePolicy`：约束 GitHub、论文、官方文档、技术博客等来源优先级。
- `CandidateKnowledge`：学习结果先进入候选区，不能直接污染长期认知。
- `KnowledgePromotion`：经过证据、置信度、适用范围、冲突检查后再进入 memory/skill/principle。

## 扩展设计

M0 扩展体系先做本地优先：

- `MCP`：stdio MVP。
- `Skills`：本地目录和 metadata。
- `Plugins`：本地 manifest，声明 tools、skills、MCP servers、hooks contribution。
- `Hooks`：BeforeExecution、AfterExecution、SessionEnded、BeforeCompaction/AfterCompaction 的基础契约。

M1 再增强：

- plugin runtime lifecycle。
- enable/disable、trust、hot reload。
- more MCP transports。
- tool search / schema 延迟加载。
- agent definition files。

## Subagent 与多 Agent

M0 只做临时 `SubagentRun`，不急于做完整长期多 Agent 平台。

M0 subagent contract：

- 输入：goal、context brief、allowed tools、model/policy、timeout。
- 运行：独立上下文，不默认继承父 agent 全量历史。
- 限制：toolset 更窄，危险工具默认不继承。
- 输出：summary、artifacts、events、status。
- 回传：父 agent 只接收 summary/result，不接收无限 transcript。

长期 `AgentProfile` 可放到 M1；agent team、handoff policy、speaker selection 放到 M2/M3。

## 状态与持久化

M0 推荐采用 append-only event log + structured session state 双层思路：

- event log 记录不可变事实：用户输入、模型输出、工具调用、审批、错误、压缩、结果。
- session state 支撑查询：session 列表、title、status、message index、toolcall index、lineage。
- transcript 由事件派生或同步写入，但不应成为唯一事实来源。

M0 至少要支持：

- run/session/turn 查询。
- run result 和 stop reason。
- approval pending/resolved 查询。
- tool call 审计。
- resume state。
- event stream replay。

SQLite + FTS 是合理默认，但最终技术方案再确定具体 schema。

## 验证与质量闭环

M0 不需要完整 eval 平台，但必须保留 verifier 边界。

M0 verifier 可支持：

- command verifier：运行确定性命令检查结果。
- file verifier：检查文件存在、内容规则、格式。
- schema verifier：检查结构化输出。
- trace export：导出 event trace 供人工或后续 eval 使用。

M1 再做：

- LLM judge。
- rubric retry。

M2 再做：

- tool eval dataset。
- trace clustering。
- prompt/tool/policy 改进闭环。

## M0 范围建议

本节原本是早期探索性 M0 清单，当前已被 `../../requirements/03-foundation-scope-decisions.md` 修正。以下口径按“完整核心体系的第一条可运行闭环”对齐，不再采用“只预留 cognitive 边界 / CLI-only MVP”的旧切分。

建议 M0 包含：

- Rust workspace 分层。
- `AgentHarness` 最小实现。
- `LoopEngine` 最小状态机。
- `Run/Session/Turn/Event` 协议。
- CLI + Local Gateway/App Server 双入口，提交同一种 `RunRequest`，消费同一种 event。
- SQLite/FTS 方向的 `EventStore`、append-only event log 和结构化 session state。
- `ContextEngine` 最小实现。
- `ToolRegistry` / `Toolset` / `ToolRuntime`。
- unified exec 最小实现。
- MCP stdio MVP，包含 tools/resources discovery、allowlist、policy check、call event、timeout 和禁用边界。
- local skills MVP，包含 metadata、scope、version、trust boundary、按需加载和事件记录。
- local plugin manifest MVP，插件贡献 tools、skills、MCP servers、hooks、config contribution 时必须进入 registry、trust、policy 和来源追踪。
- `PolicyEngine`、权限模式、approval request、Gateway-compatible approval protocol。
- `DelegationGrant` 与 `AutonomyEnvelope` 的运行时 enforcement，trust 不能突破 permission。
- hooks 基础点：`BeforeExecution`、`AfterExecution`、`SessionEnded`。
- `SubagentRun` MVP。
- `Verifier` MVP。
- config schema 和 doctor 命令。
- 最小主动闭环：授权 observation -> opportunity -> value gate -> proposal -> feedback -> evidence。
- 最小 `CoordinationReasoner`：从 GoalFrame、SituationModel、ResourceInventory、CognitiveMapRef、TrustProfileRef、FailureEvidenceRef 生成 `ResourcePlan`、`DoneContract`、`AutonomyEnvelope` 和 `DecisionTrace`。
- CognitiveLoop 的最小运行链路：post-turn 或 idle 触发，生成低置信候选，但不自动提升稳定认知。
- Temporal Memory / UserModel 候选边界：raw storage、memory substrate、session history、memory summary、candidate memory、stable memory、`UserAttributeCandidate`、`UserModelAttribute`、`ImportedHistoricalEvidence`、稳定性、置信度、时间戳和过程证据优先级。
- AgentSelfModel / PartnershipModel 候选边界：`AgentSelfUpdateCandidate`、`CapabilityEvidence`、`CapabilityGap`、`LearningAgendaItem`、`PartnershipStateCandidate`。
- CognitiveMap 候选边界：低置信 `CognitiveMapUpdateProposal`，记录场景判断框架、质量标准、盲区和资源关系。
- `FailureEvidence` 分类与 failure digest，并关联 trace、资源、验证、用户反馈和后续修正建议。
- Communication Fabric 数据边界：`CommunicationEvent`、`CommunicationSession`、`ParticipantProfile`、`DisclosurePolicy`、`TerminationPolicy`、`ExternalCommunicationGrant`、`CommunicationProposal`；M0 只跑通 text + CLI/Local Gateway 的标准事件路径。
- 原创性与合规边界检查机制：本地参考材料不进入 build/import/include 路径。

M0 不包含：

- Web/TUI 控制台。
- 插件市场。
- 完整自动化/cron。
- 完整 checkpoint/worktree。
- 完整 OS sandbox。
- 多平台消息矩阵。
- 企业策略和云端执行。
- LoopSpec 自动进化。
- 无监督主动上网学习。
- 自动修改长期原则和用户画像。
- 自动提升稳定认知、稳定用户属性、稳定 Agent 自我属性或伙伴关系状态。
- 默认开启 L4/L5 高自治。
- 真实多平台 adapter、真实麦克风/摄像头/电话系统、公网外部链接、实时语音/视频和 Agent-to-Agent 协议实现。

## M1/M2/M3 方向

M1 适合增强产品能力：

- Gateway daemon。
- Web/TUI 控制台。
- context compression 自动化。
- memory manager。
- checkpoint/worktree。
- tool search / schema 延迟加载。
- plugin runtime。
- automation / cron。
- platform adapters。
- agent definition files。
- per-agent profile。
- CognitiveLoop MVP：非对称交互、周期性反思、候选认知更新。
- seed LoopSpec：默认九阶段模板的可审计定义、事件裁剪和版本号。
- UserModel / AgentSelfModel / PartnershipModel 基础管理，以及主动补强策略的 proposal 级治理。
- ReflectionRecord / PrincipleStore。
- LearningAgenda 的手动或半自动模式。

M2 适合主动学习、能力成长和生态增强：

- 插件市场。
- 多端原生 app。
- 多 messaging 平台矩阵。
- 完整 OS sandbox。
- hosted execution。
- enterprise managed policy。
- agent marketplace / skill hub。
- distributed workers。
- 自主学习系统：SourcePolicy、CandidateKnowledge、KnowledgePromotion、SkillLibrary 自动更新。
- CapabilityGap、CapabilityUpdateProposal 和 AgentSelfModel 能力更新候选。
- 主动学习与能力成长体验：主动建议、定期汇报、长期目标跟踪的基础能力。

M3 适合自进化和高阶治理：

- LoopRegistry、LoopSelectionPolicy、LoopFitness。
- LoopEvolutionProposal、LoopPromotionPolicy、loop rollback。
- replay / simulation / A-B 对比，用于验证 loop 改动。
- 多角色认知协作网络：不同认知角色围绕同一问题循环审议和迭代。
- 认知冲突图谱和长期目标跟踪的治理化。
- 更完整的增强型个人主动智能体体验：主动建议、定期汇报、长期目标跟踪、认知冲突治理。

## 关键设计选择建议

| 问题 | 推荐初始选择 | 原因 |
|---|---|---|
| 第一入口 | CLI + Local Gateway/App Server 同协议双入口 | CLI 最快验证内核，Local Gateway/App Server 防止产品形态被 CLI 锁死。 |
| Gateway | M0 做 Local Gateway/App Server 最小入口，M1 增强控制台和后台任务体验 | M0 必须统一 run/event/approval 协议，M1 再补产品广度。 |
| Session store | event log + structured state，优先考虑 SQLite | 同时满足审计、恢复和查询。 |
| Skills | M0 纳入 | skills 是通用 Agent 的流程能力，不宜后置太久。 |
| MCP | M0 支持 stdio | MCP 是通用工具生态基础。 |
| Subagent | M0 做临时 child run | 复杂任务和上下文隔离很早就会需要。 |
| Checkpoint/worktree | M1 | coding 场景重要，但不是通用底座 M0 的必要前提。 |
| 权限模式 | M0 纳入 plan/ask/auto/trusted | 这是用户理解风险和控制权的核心体验。 |
| Plugin | M0 本地 manifest，M1 runtime lifecycle | 先定贡献边界，再做生态治理。 |
| Sandbox | M0 做策略抽象，M1 强化 restricted host，M2 再做完整 OS sandbox | 完整 OS sandbox 成本高，但接口必须先有。 |
| 增强型个人主动智能体能力 | M0 运行最小 CognitiveLoop 候选生成，并定义 UserModel、AgentSelfModel、PartnershipModel、CognitiveMap 与主动补强策略边界；M1 增强体验 | 认知平面需要事件、状态和对象基础，不能在 M0 只做空预留。 |
| 主动学习 | M1 手动/半自动议题，M2 受控主动学习闭环 | 避免早期引入噪声、投毒和不可控行为。 |
| 深度反思 | M1 做 reflection/principle 候选，确认后沉淀 | 反思要可审计、可撤销，不能直接覆盖长期认知。 |
| Loop 自进化 | M1 固定 seed LoopSpec，M2 评估候选，M3 promotion/rollback | 避免早期 loop 自改导致认知漂移。 |

## 后续文档拆分

本规划确认后，建议按下面顺序推进：

1. `docs/archive/planning/02-cognitive-loop-design.md`：专门定义认知 Loop、主动交互、自发学习和反思沉淀。
2. `docs/archive/planning/03-personal-augmented-agent-and-evolving-loop.md`：校准增强型个人主动智能体、PartnershipModel、主动补强策略和自进化 Loop。
3. `docs/requirements/01-vision-and-scope.md`：只讲背景、目标、用户、边界、成功标准，不写技术实现。
4. `docs/requirements/02-capability-requirements.md`：按 harness、agent loop、cognitive loop、tools、policy、context、state、extensions、subagent、eval 拆功能需求和优先级。
5. `docs/architecture/03-foundation-architecture.md`：进入 Rust workspace、crate/module、协议、数据流、存储和接口设计。
6. `docs/prd/01-foundation-implementation-prd.md`：实施型 PRD，包含功能描述、实现步骤、测试和验收标准。

## 需要用户后续确认的分歧点

以下问题不阻塞写下一版需求文档，但会影响 M0 范围：

1. Gateway daemon 的产品体验何时进入，但 M0 已确定包含 CLI + Local Gateway/App Server 的同协议双入口。
2. SQLite/FTS 的最终 schema 仍待技术方案细化，但 M0 已确定采用 append-only event log + SQLite/FTS 方向。
3. skills / MCP / plugins 的完整生态体验仍待细化，但 M0 已确定三者都是一等能力，MCP 先以 stdio 为基础。
4. subagent 并发策略、checkpoint/worktree、跨平台 host executor 的细节仍需在技术方案中确定。
5. seed `LoopSpec` 默认九阶段模板的裁剪规则仍需细化，例如哪些事件必须走完整路径。
6. 主动学习默认执行边界仍需细化：哪些只生成学习议题，哪些允许低风险资料收集，哪些必须确认。
7. Reflection、Principle、UserModel、AgentSelfModel、PartnershipModel 的稳定提升规则需要在实施型 PRD 中具体化。
8. 增强型个人主动智能体能力在 M1 是否需要 UI/TUI 展示认知变化、学习议题、伙伴关系和主动补强策略。
9. `AgentSelfModel` 的能力等级如何确认、降级和与 `CompetenceGate` 联动。

## 当前结论

forme 第二版规划应以 Harness-first 的通用 Agent runtime 为核心，并以 Cognitive Plane / Cognitive Loop 作为增强型个人主动智能体能力的演化主线。M0 的成败不在于工具数量多，而在于边界是否正确：所有入口都提交 run，所有执行都经过 policy，所有状态都能事件化，所有上下文都能分层治理，所有扩展都进入 harness 生命周期，并且认知平面需要的 observation、action proposal、reflection、learning、cognitive update proposal、principle 边界不会被后续外挂补丁破坏。

只要这个底座立住，后续无论是 coding agent、通用桌面 agent、自动化 agent、平台连接 agent、多 agent workspace，还是增强型个人主动智能体形态，都可以在同一套 runtime 和认知 loop 上扩展，而不是重新写一套 loop。
