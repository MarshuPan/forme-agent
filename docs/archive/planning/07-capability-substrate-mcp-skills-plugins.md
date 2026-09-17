# 能力基质：MCP、Skills、Plugins 与工具语义体系

本文档是 forme 的 V6 规划补充。它承接 `06-cognitive-evolution-kernel-assessment.md`，用于回答一个具体问题：

> 在主动式、可进化的 Agent 内核下，MCP、Skills、插件、hooks、tool registry、toolsets 这些常规能力到底应该如何组合，才能服务 forme 的认知进化内核？

本文不是技术方案，不定义最终 Rust crate/API，也不是实施型 PRD。本文涉及的同名对象、主动等级、能力与执行边界等口径，以 `../../architecture/canonical-contract.md` 为准。

## 核心判断

MCP、Skills、Plugins 不应该被理解成三个并列功能开关。它们分别代表不同的能力形态：

| 能力形态 | 本质 | 主要回答 |
|---|---|---|
| Tool | 可执行动作。 | Agent 能做什么。 |
| MCP | 外部工具/资源协议。 | Agent 如何接入外部系统能力。 |
| Skill | 流程知识和语义工作法。 | Agent 应该如何做。 |
| Plugin | 能力打包、分发和信任单元。 | 能力从哪里来，如何被安装和治理。 |
| Hook | 生命周期拦截和治理点。 | 行动前后如何检查、改写、阻断和审计。 |
| Toolset | 当前上下文可见能力集合。 | 这次任务能用哪些能力。 |

这些能力都不能绕过 `AgentHarness`。它们应进入同一个 `Capability Substrate`，被统一发现、注册、选择、暴露、执行、审计、学习和治理。

## 为什么叫 Capability Substrate

`Capability Substrate` 不是普通 capability layer。它是 Cognitive Evolution Kernel 的“能力基质”：

- `Cognitive Evolution Kernel` 需要它来理解自己能做什么。
- `AgentSelfModel` 需要它来维护能力边界和置信度。
- `LearningPipeline` 需要它来判断缺哪些工具、技能和知识。
- proposal 级主动补强策略需要它来决定能否主动补强用户。
- `ProactiveActionPolicy` 需要它来判断行动能否执行、是否需要确认。
- `EvolutionGovernor` 需要它来评估新能力能否进入稳定层。

因此，能力不是静态列表，而是认知系统的一部分。

## 能力设计要点

### Hook 与插件

- 工具定义、工具执行和 runtime metadata 分离。
- pre-tool hook 可观察、改写、阻断工具调用；主动否决与异常失败语义不同；hook failure 应 fail closed。
- 插件可以贡献 provider、channel、tool、hook、diagnostics 等能力，但贡献能力必须进入对应 registry，不能直接进入执行链。
- session extension 可以订阅 agent event。
- M0 应定义 pre/post tool hook 契约；高风险工具需要专门 policy metadata。

### Registry 与 Toolset

- `ToolRegistry` 和 `Toolset` 分离：registry 回答系统有哪些工具，toolset 回答当前 session 可见哪些工具，只暴露当前可用工具 schema。
- 工具可见性不是安全边界，执行前必须重查 toolset scope。
- MCP 动态发现进入 registry/toolset，而不是旁路；MCP 是工具来源，不是安全豁免。
- 工具需要表达并发、顺序、读写、状态修改等语义。
- registry/toolset 分离应进入 M0。

### Runtime-first 扩展

- MCP tools/resources 转成本地 runtime 能力后再治理，MCP resources 和 tools 必须分开。
- Skills 是上下文/工作流能力，不是 tool；Plugin 是分发单元，不是能力类型本身。
- 扩展最终都穿过 protocol、runtime、tools、permissions；`Capability Substrate` 必须 runtime-first，不是 CLI-first。
- plugin 贡献的 tool/skill/hook/MCP 要分别进入 registry。

### 渐进暴露与治理

- skill 不应全量进 context，而应先暴露索引和触发条件，再按需加载正文。
- MCP schema 也不应全量常驻，应支持 deferred loading。
- 插件需要 managed policy 和 trust boundary；权限模式 与 MCP/tools/hook 结合。
- session resume/fork 和 checkpoint 需要明确边界说明。

## Capability Substrate 架构

建议抽象为八个核心组件：

| 组件 | 责任 |
|---|---|
| `CapabilityRegistry` | 所有能力的统一索引，记录来源、类型、scope、风险、版本和状态。 |
| `ToolRegistry` | 可执行工具定义、schema、handler、metadata。 |
| `ToolsetResolver` | 根据 user/workspace/project/session/agent/task/policy 计算当前可见工具。 |
| `SkillRegistry` | skills 元数据、触发条件、作用域、内容加载策略。 |
| `McpRegistry` | MCP server、tools、resources、transport、allowlist、refresh 状态。 |
| `PluginRegistry` | 插件 manifest、贡献项、信任状态、生命周期、来源策略。 |
| `HookRegistry` | BeforeExecution、BeforePolicy、AfterExecution、SessionStarted、BeforeCompaction 等 hook。 |
| `CapabilityEvidenceStore` | 记录能力使用、失败、验证、用户反馈、适用边界和能力更新证据。 |

`CapabilityRegistry` 是总索引，不代替各专门 registry。它让认知内核能统一查询“我现在有哪些能力、从哪里来、能不能用、风险是什么、过去表现如何”。

## 能力进入模型前的链路

工具、skills、MCP 和插件不能直接进入模型上下文。建议统一链路：

```text
Installed / Built-in / Discovered capability
  -> CapabilityRegistry
  -> ScopePolicy
  -> TrustPolicy
  -> ToolsetResolver / SkillSelector
  -> ContextBudget / SchemaBudget
  -> ModelVisibleCapabilitySet
  -> AgentLoop
```

关键点：

- 模型看到的是当前任务可见能力，不是全量能力。
- schema/skill 描述必须与当前可见能力一致。
- 模型可见性不等于执行许可。
- 进入模型前要考虑上下文预算、风险等级和语义相关性。

## 能力执行链路

工具执行必须重新治理：

```text
Model ToolCall / ActionProposal
  -> ToolCallNormalizer
  -> ToolsetScopeRecheck
  -> PermissionPolicy
  -> HookChain / BeforeExecution
  -> ApprovalPolicy
  -> ToolRuntime / MCPClient / Exec
  -> ResultValidator
  -> HookChain / AfterExecution
  -> EventStore
  -> CapabilityEvidenceStore
  -> AgentSelfModel update candidate
```

这条链路解决三个问题：

- 防止模型调用不可见或越权工具。
- 防止 hook/plugin/MCP 绕过权限。
- 把能力表现反馈给认知系统。

## MCP 设计原则

MCP 在 forme 中应被定义为外部能力协议，不是内核能力本身。

### MCP tools

MCP tool 进入 forme 后应转为本地 `ToolDefinition`：

- 需要 tool name namespace。
- 需要 server identity。
- 需要 schema snapshot。
- 需要 risk metadata。
- 需要 source/trust 信息。
- 需要 timeout 和 budget。
- 需要 execution re-check。
- 需要 event trace。

### MCP resources

MCP resources 不是 tools。它们属于 context/evidence/source：

- 可以进入 `ObservationFabric`。
- 可以进入 `EvidencePipeline`。
- 可以作为 `CandidateKnowledge` 来源。
- 不应自动获得执行权限。

### MCP refresh

MCP 动态发现需要可审计：

- server added / removed。
- tool added / removed / changed。
- schema version changed。
- permission changed。
- stale capability disabled。

### MCP 与主动式

主动式 Agent 使用 MCP 时要更谨慎：

- 低风险只读 MCP resource 可用于候选观察。
- 写操作 MCP tool 默认不能自动执行。
- 主动任务调用 MCP 必须有 `ProactiveJob` scope。
- MCP 结果进入认知层前必须经过 evidence/freshness/conflict check。

## Skills 设计原则

Skill 是流程知识，不是工具，也不是 memory。

### Skill 的作用

Skill 应回答：

- 这类任务应该怎么做。
- 需要哪些步骤。
- 常见风险是什么。
- 需要什么验证。
- 哪些工具适用。
- 什么时候需要追问或拒绝。

### Skill 的加载

采用渐进暴露：

```text
Skill metadata visible
  -> trigger / match
  -> load selected skill body
  -> apply context budget
  -> run with task
  -> record skill usage outcome
```

M0 不应全量加载所有 skill。至少要有：

- name。
- description。
- scope。
- trigger hints。
- required tools。
- risk level。
- verification expectations。
- content hash/version。

### Skill 与认知进化

Skill 应参与学习和进化：

- 任务失败可产生 `SkillUpdateProposal`。
- 多次成功可提升 skill confidence。
- skill 过期或环境变化时应降级。
- skill 可以依赖 CapabilityRegistry 中的工具能力。
- skill 本身不扩大权限。

## Plugins 设计原则

Plugin 是能力打包和信任治理单元，不是能力类型。

一个 plugin 可以贡献：

- tools。
- skills。
- MCP server config。
- hooks。
- commands。
- agent definitions。
- context sources。
- UI/surface extension。

但每个贡献项都必须进入对应 registry：

```text
Plugin manifest
  -> PluginRegistry
  -> contribution validation
  -> ToolRegistry / SkillRegistry / McpRegistry / HookRegistry
  -> ScopePolicy
  -> TrustPolicy
  -> runtime use
```

### Plugin trust

插件需要明确：

- 来源：built-in、local、workspace、user、remote、marketplace。
- 信任等级：trusted、reviewed、untrusted、disabled。
- 作用域：user、workspace、project、agent、session。
- 可贡献能力类型。
- 是否允许 hooks。
- 是否允许写操作工具。
- 是否允许 MCP server。
- 是否允许外部网络。

M0 可只做 local manifest，不做 marketplace。但 manifest 结构要预留治理字段。

## Hooks 设计原则

Hooks 是 runtime lifecycle 的治理点，不是自由脚本入口。

建议 M0/M1 预留：

| Hook | 作用 |
|---|---|
| `SessionStarted` | 初始化 context、policy、observability。 |
| `PreModelCall` | 最后检查上下文和模型输入。 |
| `BeforeExecution` | 工具执行前拦截、改写、阻断。 |
| `BeforePolicy` | 进入用户确认前补充解释和风险。 |
| `AfterExecution` | 结果校验、脱敏、审计、证据记录。 |
| `AfterExecutionBatch` | 多工具调用后的聚合检查。 |
| `BeforeCompaction` / `AfterCompaction` | 上下文压缩前后记录 lineage。 |
| `SessionEnded` | 收尾、反思候选、指标落盘。 |

Hook 边界：

- hook 失败默认 fail closed，除非明确配置为 best-effort。
- hook 是否能改参数必须显式声明。
- hook 可见数据需要最小化。
- hook 输出必须进入事件。
- hook 来源必须受 plugin trust 管理。

## 语义判断能力

用户提到四个工具的“语义判断能力很强”。这不应只理解成模型能力，而是工程层的语义选择机制。

forme 需要以下语义判断层：

| 判断层 | 作用 |
|---|---|
| Tool Relevance | 当前任务需要哪些工具。 |
| Tool Risk | 工具调用风险、影响范围、是否可回滚。 |
| Skill Match | 当前任务适合哪些 skills。 |
| Capability Gap | 当前能力是否不足，需要学习、安装、请求权限或转交。 |
| Source Trust | MCP/resource/plugin 来源是否可信。 |
| Execution Mode | 并发、顺序、独占、dry-run、approval-required。 |
| Semantic Defer | 是否先不暴露全 schema，而用 tool search/schema 延迟加载。 |

这些判断应进入 `CapabilityReasoner`：

```text
TaskContext
  -> CapabilityReasoner
  -> relevant tools / skills / MCP / plugins
  -> ToolsetResolver / SkillSelector
  -> model visible set
  -> execution governance
```

`CapabilityReasoner` 不是一个随便问模型的步骤。它应结合：

- task type。
- user/workspace/project scope。
- tool metadata。
- skill metadata。
- policy。
- historical success/failure。
- current context budget。
- AgentSelfModel capability confidence。

## 与 Cognitive Evolution Kernel 的关系

Capability Substrate 是 Cognitive Evolution Kernel 的下层支撑。

```text
Cognitive Evolution Kernel
  -> asks: what can I do / learn / observe?
Capability Substrate
  -> answers: tools, skills, MCP, plugins, hooks, permissions, evidence
AgentHarness
  -> enforces: policy, approval, execution, event, rollback
```

具体关系：

- `AgentSelfModel` 从 CapabilityEvidenceStore 中更新能力置信度。
- `LearningPipeline` 从 CapabilityGap 中生成学习议题。
- 主动补强策略使用 CapabilityReasoner 判断能否主动补强用户。
- `ProactiveActionPolicy` 使用 ToolRisk 和 InterventionLevel 决定是否执行。
- `EvolutionGovernor` 使用 skill/tool/plugin 的表现证据决定是否提升或降级。

## M0/M1/M2/M3 切分建议

### M0

M0 必须包含：

- `CapabilityRegistry` 概念边界。
- `ToolRegistry`。
- `ToolsetResolver`。
- execution re-check。
- basic `SkillRegistry`。
- local skills metadata + 按需加载。
- MCP stdio client MVP。
- MCP tools/resources 区分。
- local plugin manifest MVP。
- basic hook contract：BeforeExecution、AfterExecution、SessionEnded。
- tool metadata：risk、read/write、stateful、requires_approval、requires_sequential。
- CapabilityEvidence event。

M0 不做：

- 插件市场。
- 复杂远端插件安装。
- 全量 MCP dynamic marketplace。
- 全自动 tool search。
- 复杂 skill auto-generation。

### M1

M1 增强：

- MCP dynamic discovery / refresh。
- deferred tool schema。
- skill 按需加载。
- plugin runtime lifecycle。
- more hooks：BeforePolicy、BeforeCompaction/AfterCompaction、AfterExecutionBatch。
- CapabilityReasoner MVP。
- tool search。
- capability usage dashboard / logs。

### M2

M2 增强：

- remote/marketplace plugin。
- managed policy。
- multi-source MCP governance。
- skill update proposal。
- capability gap detection。
- tool/skill performance scoring。
- active learning links to skills/tools.

### M3

M3 做进化治理：

- automatic capability recommendation。
- skill evolution with replay/eval。
- tool selection policy evolution。
- plugin trust scoring。
- capability promotion/rollback。
- cross-agent capability sharing。

## 当前结论

MCP、Skills、Plugins 不能只是“常规能力”清单。对 forme 来说，它们应共同构成 `Capability Substrate`：

- MCP 提供外部系统接入。
- Skills 提供流程和语义工作法。
- Plugins 提供打包、分发和信任治理。
- Hooks 提供生命周期拦截。
- ToolRegistry/Toolset 提供执行能力和作用域。
- CapabilityReasoner 提供语义选择。
- CapabilityEvidenceStore 把能力表现反馈给认知进化内核。

这样设计后，Agent 不只是“会调用工具”，而是能理解自己的能力边界、选择合适能力、发现能力缺口、学习新能力、评估能力表现，并在 Harness 治理下主动使用这些能力补强用户。
