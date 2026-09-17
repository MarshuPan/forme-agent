# 开源定位、主动式内核沉淀与原创边界

本文档是 forme 的 V8 规划收口文档。它用于回答两个问题：

1. 到目前为止，forme 的主动式 Agent 内核是否已经沉淀成一个可作为初版底座的架构。
2. 项目计划开源后，设计与实现必须遵守哪些原创与合规边界。

本文不是法律意见，也不是最终开源合规审计。它是架构和工程层面的约束文件，后续进入 PRD 和实现前仍需要正式 license、notice、dependency 和代码来源审查。开源原创与合规的 canonical 口径以 `../../architecture/canonical-contract.md` §12 为准。

## 当前判断

当前文档已经基本完成了初版 Agent 内核的定位沉淀，但还没有进入可编码 PRD。

已经明确的部分：

- forme 不是单纯通用 Agent，也不是 coding CLI。
- forme 的方向是主动式、可进化、可协调、受治理的个人/工作空间 Agent 内核。
- `AgentHarness` 是最高运行治理边界。
- `AgentLoop` 只是一次 run/turn 内部的模型与工具执行状态机。
- `Cognitive Evolution Kernel` 是主动式与进化式能力的内核主线。
- V9 后续补充确认 `Coordination Kernel` 是第三个核心内核，负责资源判断、行动路线、done contract、decision trace 和协调策略进化。
- V10 后续补充确认 `CognitiveMap`、`Trust / Delegation` 和 `FailureEvidence` 是三内核补强，不新增第四内核。
- `Capability Substrate` 负责能力发现、选择、暴露、语义判断和能力证据。
- `Execution / Embodiment Substrate` 负责 shell、file、browser、computer use、MCP、API connector 等真实行动触点。
- `Surface / Gateway / Model Config` 负责入口、身份、权限、审批、模型路由和配置治理。
- 所有主动行动、学习、长期写入、策略变更都必须回到 `AgentHarness`、policy、event store 和 promotion/rollback 链路。

后续文档已经补齐的部分：

- `requirements/01` 已经把用户、目标、边界和优先级正式写成 requirements。
- `requirements/02` 已经把能力域、阶段边界和验收方向固定下来。
- `requirements/03` 已经把 M0 固定为完整核心体系的第一条可运行闭环。
- `requirements/04` 已经把 M0 的验证矩阵和验收证据要求固定下来。

尚未完成的部分：

- 技术方案还没有把 Rust crate、接口、存储、事件 schema、配置 schema 定下来。
- PRD 还没有把功能、实现步骤、测试、验收标准拆成可执行任务。
- 主动式能力仍属于研究性内核，必须用 M0/M1/M2/M3 分阶段验证，不能一次性承诺完整自治。

因此，当前成熟度可以定义为：

> 架构方向已经足够作为初版底座主线；M0 范围和验收策略已经进入 requirements；实现细节仍需要在 architecture、PRD 阶段继续收敛和实验验证。

## 当前主动式内核沉淀

当前主动式内核不是“通知系统”，也不是“常驻看屏幕”，而是一套受治理的认知行动系统。

初版内核可概括为：

```text
AgentHarness
  -> governs runs, sessions, tools, policy, approval, events
Cognitive Evolution Kernel
  -> manages stable kernel, cognitive substrate, memory, learning, evolution
Coordination Kernel
  -> selects resources, routes work, defines done contracts and records decision traces
Capability Substrate
  -> knows what the Agent can use and why
Execution / Embodiment Substrate
  -> executes real-world actions through controlled backends
Surface / Gateway / Model Config
  -> controls where work enters, who it belongs to, and which model/policy applies
```

主动式的最小闭环是：

```text
Authorized Signal
  -> Observation
  -> Opportunity
  -> ValueGate
  -> InterventionLevel
  -> ActionIntent / LearningTask / ReflectionTask
  -> AgentHarness governance
  -> Approval / Execute / Wait / Record
  -> Evidence
  -> Reflection
  -> CandidateUpdate
  -> Promotion / Rollback
```

这意味着 forme 的主动式能力有几个硬边界：

- 主动不是无限制执行。
- 主动不是无限读取用户数据。
- 主动不是自动上网学习所有东西。
- 主动不是模仿用户成为“用户复制品”。
- 主动行动必须带 scope、理由、风险、审批策略和事件记录。
- 高影响动作必须经过审批。
- 长期认知更新必须先进入候选层，再验证、提升和可回滚。

## 主动式内核的来源

forme 的主动式内核必须来自：

- proactive agent、mixed-initiative、memory、continual learning 与 self-improvement 等公开研究方向的抽象问题。
- 用户对“增强型主动助手”的长期设想。
- 工程上可治理的 harness、policy、event、evidence、promotion/rollback 设计。
- 真实任务迭代中的实验和评估。

主动式能力是 forme 的核心主线，不是在通用任务 runtime 上附加的通知或定时功能。

## 开源原创原则

forme 计划开源，设计与实现必须遵守以下原则。

### 1. 独立设计，不复制表达

不能复制：

- 第三方源码。
- 第三方文件组织结构的一比一复刻。
- 类名、函数名、配置名、命令名的大规模照搬。
- 文档段落、注释、错误信息、提示词、系统 prompt 的近似复制。
- 测试用例结构、fixture、样例数据的直接迁移。
- 来源不明或未获授权材料中的任何内容。

### 2. 从问题出发重新设计

每个能力都必须经过 forme 自己的设计转换：

```text
problem abstraction
  -> forme requirement
  -> forme architecture decision
  -> original Rust design
  -> original code and docs
```

禁止路径：

```text
third-party code / docs
  -> rename variables
  -> adjust language
  -> commit into forme
```

即使第三方材料是 MIT、Apache、BSD 等宽松许可证，也不应把项目变成拼接式复刻。许可证允许复用不等于架构上应该复用，更不等于可以忽略 attribution、notice 和维护边界。

### 3. 建立原创性与合规边界检查机制

进入 PRD 和实现后，需要建立原创性与合规边界检查机制。只有当实现引入依赖、代码复用或协议兼容时，才需要记录具体来源：

| 字段 | 说明 |
|---|---|
| `reference` | 依赖、标准或规范来源。 |
| `source_type` | public_source、official_docs、paper、standard。 |
| `borrowed_idea` | 采用的抽象思想或兼容目标。 |
| `forme_design` | forme 中重新设计后的对象或机制。 |
| `copy_risk` | none、low、medium、high。 |
| `license_action` | none、notice、dependency_review、legal_review。 |
| `implementation_rule` | 必须原创实现、可用第三方 crate、可做接口兼容、不允许复用。 |

这个记录应成为开源前的检查材料，而不是实现完再补。

### 4. 文档也要原创

开源风险不只来自代码，也来自文档、配置名、命令、UI 文案和示例。

forme 的文档应：

- 用自己的术语体系。
- 用自己的架构图。
- 用自己的 examples。
- 不改写第三方文档段落当作本项目说明。
- 不用“和某项目一样”的表达作为正式设计。

## 允许的方式

### 标准和协议

MCP、JSON-RPC、HTTP、WebSocket、SQLite、OpenTelemetry 等开放标准可以按标准实现。实现标准协议不是复制项目。

但需要注意：

- 协议字段按标准来。
- 私有扩展字段不能直接照搬。
- 兼容某生态时要明确 compatibility target。

### 第三方依赖

后续可以使用开源 crate/npm/package，但要经过：

- license 检查。
- transitive dependency 检查。
- security / maintenance 检查。
- attribution / notice 检查。
- 是否适合 Rust 底座和长期维护。

## 禁止的方式

以下行为应明确禁止：

- 复制第三方源码后改名。
- 把第三方目录结构搬进 forme。
- 复制第三方 prompt、文档、错误信息、命令输出或完整命令体系。
- 在没有公开证据的情况下把第三方内部行为写成 forme 的事实。
- 在没有 license/notice 处理的情况下复用代码片段。
- 在开源仓库中保留第三方源码快照、压缩包或来源不明材料。

## 代码仓库结构约束

- 开源仓库不发布任何第三方源码快照。
- 本地参考材料只能放在被忽略的本地目录，主工程源码不得 import、include、copy 或 build 这些材料。
- 若保留第三方源码用于测试或 fixture，必须有 license、notice 和明确用途。

开源发布前应执行检查：

```text
No third-party source snapshots in release tree
No copied license-incompatible assets
No accidental build dependency on local reference material
No copied docs/prompts/error strings
Originality and compliance records reviewed where concrete reuse or compatibility exists
LICENSE / NOTICE completed
```

## 初版内核可以怎么落地

在开源约束下，M0 应采用“原创完整核心闭环”策略：

| M0 领域 | 初版做法 |
|---|---|
| Runtime | 原创 `AgentHarness`、Run/Session/Turn、Event 模型。 |
| Loop | 原创 `AgentLoop` 状态机，只保留通用 reason/action/observation 与 tool-call 思想。 |
| Capability | 原创 `CapabilityRegistry`、ToolRegistry、ToolsetResolver、CapabilityReasoner。 |
| Execution | 原创 ActionIntent、ExecutionPlan、Shell/File/MCP backend。 |
| Policy | 原创 PermissionProfile、PolicyEngine、ApprovalBroker。 |
| Cognition | 原创 Cognitive Evolution Kernel 数据模型和候选提升机制。 |
| Config | 原创 typed config schema、profiles、secrets reference、ConfigDoctor。 |
| Docs | 原创文档体系。 |

这会降低短期速度，但能提高开源后的稳定性、可信度和长期可维护性。

## 当前文档是否已经完善到这个程度

结论：主动式内核的架构方向在 V8 阶段已经基本完善；开源原创约束在本文档前还不够集中，现在需要把本文作为后续阶段的硬边界。后续讨论确认还缺少一个把认知、能力、工具、记忆、模型和用户注意力组合成行动路线的协调内核，因此由 `10-coordination-kernel-and-resource-orchestration.md` 作为必要补强。

具体判断：

- `05-proactive-specialized-agent-kernel.md` 已经把主动式和进化式学习上升为内核特征。
- `06-cognitive-evolution-kernel-assessment.md` 已经把主动式背后的认知进化内核讲清楚。
- `07-capability-substrate-mcp-skills-plugins.md` 已经把 MCP、Skills、Plugins、Hooks、Toolset 等能力体系化。
- `08-execution-touchpoints-model-gateway-config.md` 已经把 Agent 的手脚层、模型配置和网关入口体系化。
- 本文档补齐了开源定位、原创边界、禁止事项和后续原创性与合规边界检查要求。
- `10-coordination-kernel-and-resource-orchestration.md` 补齐了 Coordination Kernel、资源选择、done contract、decision trace 和协调策略进化。
- `11-cognitive-map-trust-and-failure-evidence.md` 补齐了场景认知地图、渐进放权和失败证据这三个支撑主动式内核长期成立的关键边界。

下一步不建议继续无限扩展普通规划概念。协调内核、认知地图、信任放权和失败证据这些关键缺口已经补齐后，更合适的推进方式是：

1. 评审 `docs/requirements/03-foundation-scope-decisions.md`，确认 M0 完整核心闭环。
2. 评审 `docs/requirements/04-verification-and-acceptance-strategy.md`，确认 M0 验收矩阵。
3. 再写 Rust 技术方案和实施型 PRD。

主动式、进化式和协调式部分仍需要在实现中慢慢研究和校准，但当前已经足够成为初版底座的三内核方向。
