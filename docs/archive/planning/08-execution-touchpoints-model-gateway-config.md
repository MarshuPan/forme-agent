# 执行触点、模型配置与网关入口：Agent 的手脚层

本文档是 forme 的 V7 规划补充。它承接 `07-capability-substrate-mcp-skills-plugins.md`，用于回答用户提出的一个关键问题：

> Agent 除了 MCP、Skills、插件这些能力，还需要终端、文件、浏览器、桌面控制、外部系统、模型配置、网关入口等大量触点。这些东西本质上像 Agent 的手脚，应该如何设计并组合成 forme 自己的主动式 Agent 底座？

本文仍然是规划文档，不定义最终 Rust crate/API，也不是实施型 PRD。本文涉及的主动等级（L0-L5）、AutonomyEnvelope、对象 owner 等口径，以 `../../architecture/canonical-contract.md` 为准。

状态说明：本文中的 Gateway-ready、CLI 先行和 backend MVP 表述是 V7 阶段的执行触点规划。后续 `requirements/03-foundation-scope-decisions.md` 已经固定 M0 必须包含 CLI + Local Gateway/App Server、SQLite/FTS、Shell/File/MCP 第一版 backend 和 Gateway-compatible approval protocol。`12-unified-gateway-and-communication-fabric.md` 又进一步补齐文字/语音/图片/视频、软件/硬件承载、对外沟通、思想代理和 Agent 间协作的统一沟通触手边界。`13-temporal-memory-and-user-model.md` 进一步校准 memory、UserModel、时间尺度和历史导入证据边界。后续正式架构应以后续需求文档为准。

## 核心判断

forme 不能把终端、文件、浏览器、MCP、插件、网关、模型配置看成一堆零散功能。它们应该被统一理解成三层：

| 层 | 核心问题 | 典型对象 |
|---|---|---|
| `Capability Substrate` | Agent 知道自己拥有哪些能力、什么时候该用什么能力。 | ToolRegistry、ToolsetResolver、SkillRegistry、McpRegistry、PluginRegistry、CapabilityReasoner。 |
| `Execution / Embodiment Substrate` | Agent 选定能力后，如何真实、安全、可观测地作用到外部世界。 | Shell、File、PTY、Browser、Computer Use、MCP Tool、App API、Remote Executor、Scheduler、Notification。 |
| `Surface / Gateway / Model Config` | 用户、系统事件和平台从哪里进入，使用哪个模型与权限运行。 | CLI、TUI、API、Gateway、Slack/消息入口、Webhook、Cron、ModelProvider、ModelProfile、GatewayProfile。 |

V7 解决的是“能力是什么、为什么选择它”。V8 解决的是“动作如何发生、入口如何进入、模型如何配置、风险如何治理”。

这三层都必须受 `AgentHarness` 管理，不能绕过 harness。否则主动式 Agent 会出现两个严重问题：

- 认知层认为自己在受控行动，但实际 shell、浏览器或插件已经绕过权限执行。
- 网关、定时任务、MCP、插件、桌面控制各自形成小闭环，导致 session、memory、审计、模型配置和用户授权全部碎片化。

## 设计校准

本轮规划校准了以下通用判断：

- MCP 是连接 AI 应用与外部系统的开放标准，tools、resources、prompts 是不同原语；tools 是模型可调用的外部动作，resources 更接近应用控制的上下文来源。
- 截图驱动的 computer use 是 screenshot/action loop：模型观察屏幕，返回 click、type、scroll 等结构化 UI 动作，harness 执行后回传新截图；第三方页面、邮件、聊天、PDF 和工具输出都必须视为不可信输入。
- model/provider、approval、sandbox、MCP 都应纳入配置层，并明确用户配置、项目配置、profile、系统配置的优先级；沙箱和审批是两个协同但独立的控制。
- hooks、settings scopes、permissions、MCP、skills、subagents 应作为可配置的运行契约；skills 按需加载，hooks 贯穿 session、turn、tool、compact 等生命周期。
- 主动式 Agent 正在从单次对话走向“有身份、有权限、有上下文、有行动队列”的协作系统，需要 channel scoped identity、tools/data access 控制、channel memory、异步工作、token budget 和审计日志。

结论是：主动式 Agent 必须从一开始把行动触点、入口身份、模型路由、权限、审计、长期学习放到同一个 runtime 体系里。

## 与 V7 的边界

`Capability Substrate` 和 `Execution Substrate` 不能混在一起。

能力层负责：

- 有哪些工具、skills、MCP、插件、hooks。
- 当前任务应该暴露哪些能力给模型。
- 哪些能力语义相关、风险高低如何。
- 能力使用历史、失败率、适用边界是什么。
- 是否存在能力缺口，是否需要学习、安装、请求权限或转交。

执行触点层负责：

- 动作在哪个后端执行。
- 后端是否支持流式输出、取消、超时、PTY、截图、checkpoint、dry-run。
- 文件、网络、进程、凭据、浏览器 profile、桌面权限如何限制。
- 输出如何回填给模型、UI、事件流和长期证据库。
- 失败、拒绝、超时、沙箱阻断、用户取消如何表达。
- 主动任务是否允许在后台执行，是否必须等待用户确认。

简单说：

```text
Capability Substrate
  -> says: what can be used and why
Execution / Embodiment Substrate
  -> says: how the selected action happens in the world
AgentHarness
  -> enforces: policy, approval, event, state, rollback boundary
Cognitive Evolution Kernel
  -> learns: outcome, capability gap, user preference, environment constraint
```

## 为什么叫 Execution / Embodiment Substrate

“工具层”这个词太窄。终端、文件、浏览器、桌面、MCP、外部 API、消息平台、定时任务和远端 worker 并不只是工具，它们是 Agent 影响世界的触点。

因此建议使用两个并列概念：

- `Execution Substrate`：强调工程执行层，适合 Rust runtime、process、sandbox、backend、event。
- `Embodiment Substrate`：强调 Agent 的“具身触点”，也就是它如何看见、读取、点击、写入、调用、发送、等待和恢复。

在正式技术方案里可以选一个名字。当前规划中用 `Execution / Embodiment Substrate`，避免把这层误解成普通工具注册表。

## 触点设计要点

### 执行内核

- Rust runtime 必须分层，CLI、IDE、app-server 都不是 core。
- M0 必须先有 typed execution backend 和 process manager，不能只用 `std::process::Command` 包一层；shell 执行、审批、沙箱、输出截断、后台进程、deadline、streaming 和错误语义放在同一条链路。
- shell 只是一个 action backend，不是所有能力的兜底。
- approval 和 sandbox 是两个不同控制：批准不等于无沙箱。
- approval request 必须是结构化对象，包含 action、cwd、backend、permission delta、risk、scope、timeout。
- config schema 必须一开始把 model/provider、permission profile、MCP、gateway/surface 预留出来。
- rich client、审批、事件流和会话历史通过稳定 protocol 驱动，而不是 UI 直接调用内部对象。

### 控制面与 Gateway

- Gateway 统一承载 WS/HTTP、auth、device pairing、operator scope、method scope、sessions、channels 和 runtime state；CLI、UI、app、channel 不各自驱动 agent。
- M0 即使只做 CLI，也要 Gateway-ready：Run/Session/Event/Approval/Config 不能绑死 CLI。
- M1 可以引入 local gateway daemon 或 app-server，让桌面、Web、消息入口、cron、MCP、远端节点都走统一协议。
- 平台入口和 provider/model 配置要有 scope：user、workspace、agent、channel、session、organization。
- 插件能贡献触点，但触点必须进入 backend registry 和 policy，不允许插件直接绕过执行层；before/after tool hook 可以观察、改写、阻断，并产生诊断事件。
- agentId 对应 workspace/auth/session/config，支撑多 Agent 隔离；doctor/migration 是配置兼容性和安全边界的一部分。

### 触点生态与配置

- 核心保持窄，触点和生态在边缘扩展；新能力不默认成为 core tool。
- M0 不应堆工具数量，而应先设计“新触点应以什么形态进入系统”的准入规则。
- secrets、runtime config、profile、gateway config、model config 要分层；非密钥设置与 secrets 分开存放。
- profile-aware home/config/log/state 支撑多 profile、多 Agent、多 gateway 隔离。
- terminal 和 browser 这种重触点必须有 setup/doctor/health check，而不是失败时让模型猜。
- 多种 surface 共享同一后端能力，runtime 不能分叉；自动发现工具不等于模型可见，必须显式进入 toolset。
- 定时任务和主动任务应进入同一 job runtime，不要在 cron、gateway、agent loop 各做一套。

### 权限体验与主动协作

- settings scope 分为 managed、user、project、local；deny/ask/allow 规则和 权限模式 让用户理解安全行为，但 权限模式 只是体验层，底层仍要有结构化 permission profile。
- hooks 生命周期覆盖 session 开始、用户输入提交、工具调用前后、权限请求、压缩前后和 session 结束等关键点。
- skills 按需加载；MCP 支持 tool search/schema 延迟加载，避免大量 schema 占满上下文。
- subagent 可限制 tools、model、permissions、hooks、skills。
- rollback 不能泛化承诺，必须按 backend 声明可回滚边界：文件工具可跟踪，shell 命令造成的副作用不一定可回滚。
- 主动式 Agent 的执行层必须支持 background job、schedule、approval delivery、audit log 和 channel-scoped memory；Agent 可以有 channel/workspace scoped identity，在授权 channel 中持续积累上下文，并异步并行处理多个任务。
- computer/browser/desktop 触点必须明确“看见什么、点击什么、记录什么、谁授权什么”。

## ActionBackend 统一抽象

建议把所有“手脚”统一成 `ActionBackend`，但每种 backend 有不同能力和风险。

| Backend | 作用 | M0/M1 建议 | 核心风险 |
|---|---|---|---|
| `ShellBackend` | 执行命令、脚本、构建、测试、系统工具。 | M0 必须。 | 任意副作用、长进程、凭据泄漏、网络访问、不可回滚。 |
| `PtyBackend` | 交互式终端、TUI、REPL、长会话命令。 | M1。 | 状态难复现、输出流复杂、交互阻塞。 |
| `FileBackend` | 读写文件、patch、目录操作、diff、checkpoint。 | M0 必须。 | 数据丢失、越权路径、隐式执行链污染。 |
| `McpBackend` | 调用外部 MCP tools，读取 MCP resources/prompts。 | M0 stdio MVP，M1 dynamic discovery。 | prompt injection、工具冒名、外部系统副作用。 |
| `BrowserBackend` | 通过浏览器自动化协议操作网页、DOM、截图、下载。 | M1。 | SSRF、登录态滥用、网页注入、下载执行风险。 |
| `ComputerUseBackend` | 基于截图和鼠标键盘动作控制桌面/浏览器。 | M2 前以实验模式。 | 视觉误判、误点击、高权限桌面副作用、不可审计 DOM。 |
| `AppApiBackend` | 直接调用 Slack、GitHub、Jira、Notion、邮件、数据库等 API。 | M1/M2。 | 数据外泄、权限过宽、跨租户/跨 channel 泄漏。 |
| `GatewayChannelBackend` | 消息入口和出口，例如 Slack、Discord、Telegram、Email。 | M1。 | 身份混淆、群聊隐私、异步审批、重复投递。 |
| `SchedulerBackend` | cron、reminder、monitor、idle job、long-running task。 | M1。 | 主动性失控、重复执行、过期上下文。 |
| `RemoteExecutorBackend` | 容器、远端 worker、云端 execution。 | M2。 | 供应链、凭据传递、网络边界、状态同步。 |
| `NotificationBackend` | 给用户推送提醒、审批、总结、异常。 | M1。 | 打扰成本、信息泄漏、错误目标。 |

这些 backend 不应各自实现一套权限。统一入口应是：

```text
ActionIntent
  -> CapabilityReasoner
  -> ToolsetScopeRecheck
  -> ExecutionPlanner
  -> PolicyEngine
  -> ApprovalBroker
  -> ActionBackend
  -> ResultNormalizer
  -> EventStore
  -> CapabilityEvidenceStore
  -> Cognitive Evolution Kernel
```

## ActionIntent

`ActionIntent` 是模型、主动任务或子代理想要执行动作时的中间表示。它不等于最终工具调用。

建议字段：

| 字段 | 作用 |
|---|---|
| `intent_id` | 动作意图 ID。 |
| `source` | 来源：user_turn、agent_loop、proactive_job、subagent、hook、schedule、gateway_event。 |
| `goal` | 动作目标，用于审计和结果评估。 |
| `backend_hint` | 可能的后端，例如 shell/file/browser/mcp。 |
| `capability_ref` | 来自 CapabilityRegistry 的能力引用。 |
| `scope` | user/workspace/project/session/agent/channel/job scope。 |
| `risk_hint` | 模型或 reasoner 的初步风险判断。 |
| `requires_background` | 是否长任务或后台任务。 |
| `expected_effect` | 预期外部影响，例如 read-only、write-file、send-message、network-call。 |
| `rollback_expectation` | 是否需要 checkpoint、dry-run 或可回滚说明。 |

`ActionIntent` 的价值是把“模型想做什么”和“系统允许怎么做”分开。主动式 Agent 尤其需要这个中间层，因为它会产生很多候选动作，其中大部分不应该直接执行。

## ExecutionPlanner

`ExecutionPlanner` 负责把 `ActionIntent` 转成具体 `ExecutionPlan`。

它要判断：

- 用哪个 backend。
- 是否需要先读上下文再执行。
- 是否可以 dry-run。
- 是否需要 checkpoint。
- 是否需要用户审批。
- 是否可以并发。
- 是否需要独占锁。
- 输出如何截断。
- 失败是否重试。
- 是否需要转成后台 job。
- 是否应该改用更结构化 backend，而不是 shell。

示例：

| 意图 | 不推荐路径 | 推荐路径 |
|---|---|---|
| 修改文件 | shell `echo > file` | `FileBackend.patch` 或结构化 edit。 |
| 查询数据库 | shell 调 `psql` 并暴露密码 | `AppApiBackend` 或受控 MCP server。 |
| 点网页按钮 | computer-use 盲点坐标 | BrowserBackend DOM/Playwright 优先，必要时再截图。 |
| 发 Slack 消息 | shell 调 webhook | GatewayChannelBackend，带 channel scope 和审计。 |
| 长时间跑测试 | 普通同步 shell | ShellBackend background job + Monitor/Output events。 |
| 主动学习 GitHub 项目 | 无限制网络爬取 | LearningTask + SourcePolicy + rate/budget + evidence pipeline。 |

## PolicyEngine

执行触点层的安全模型应是结构化策略，不是 prompt 约定。

建议至少包含：

| 对象 | 作用 |
|---|---|
| `PermissionProfile` | 当前 run/session/job 的基础权限。 |
| `BackendPolicy` | 每个 backend 的文件、网络、进程、API、channel、browser profile 限制。 |
| `ToolPermissionRule` | 工具级 allow/ask/deny。 |
| `GatewaySurfacePolicy` | 不同入口的默认权限、上下文范围、审批通道。 |
| `ModelPolicy` | 哪些模型可用于哪些任务、是否可外发上下文、是否允许 tool call。 |
| `ApprovalPolicy` | 何时 ask、批准作用域、过期条件、是否允许自动批准。 |
| `SandboxPolicy` | 技术执行边界，例如 filesystem、network、env、process、desktop。 |
| `DataPolicy` | secret、PII、公司数据、channel data、private data 的流转规则。 |
| `ProactivePolicy` | 主动任务的触发频率、干预等级、自动执行范围。 |

策略合并顺序建议：

```text
Managed / organization policy
  -> user policy
  -> workspace/project policy
  -> agent profile policy
  -> surface/gateway policy
  -> session/job policy
  -> skill/tool/plugin requested permissions
  -> dynamic hook decision
  -> final runtime enforcement
```

基本原则：

- deny 优先于 ask，ask 优先于 allow。
- hook 可以收紧或补充策略，不应绕过 managed/user deny。
- 模型可见能力不是执行许可。
- approval grant 必须有作用域和有效期。
- sandbox 是技术边界，approval 是人类授权边界，两者不能互相替代。
- 主动式任务默认不能扩大权限。

## ApprovalBroker

主动式 Agent 会在不同入口运行，所以审批不能只存在于 CLI prompt。

`ApprovalBroker` 应支持：

- CLI/TUI 交互审批。
- Gateway/Web UI 审批。
- Slack/消息 thread 审批。
- API callback 审批。
- 超时后的默认行为。
- 一次性批准、session 级批准、工具参数模式批准。
- 审批结果写入 event log。
- 审批上下文脱敏。

审批请求必须结构化：

| 字段 | 说明 |
|---|---|
| `approval_id` | 审批 ID。 |
| `action_summary` | 用户可读动作摘要。 |
| `backend` | shell/file/mcp/browser/computer/api/channel。 |
| `risk_level` | low/medium/high/critical。 |
| `scope` | 作用域。 |
| `requested_permissions` | 请求新增权限。 |
| `affected_resources` | 可能影响的文件、URL、channel、API 对象。 |
| `rollback_boundary` | 可回滚、部分可回滚、不可回滚。 |
| `expires_at` | 批准有效期。 |
| `choices` | allow_once、allow_for_session、deny、modify_scope、view_details。 |

当 Agent 在消息 channel 中异步工作时，审批和审计必须跟着 channel identity 走，而不是只跟当前本地终端走。

## Observability 与 Event

执行触点层必须事件化。至少需要：

| Event | 说明 |
|---|---|
| `ActionIntentCreated` | 模型、主动任务或子代理提出动作意图。 |
| `ExecutionPlanCreated` | 系统选择 backend 和执行计划。 |
| `PolicyDecisionMade` | allow/ask/deny、原因、策略来源。 |
| `ApprovalRequested` | 需要用户或管理员批准。 |
| `ApprovalResolved` | 批准、拒绝、超时、修改 scope。 |
| `ActionStarted` | 具体 backend 开始执行。 |
| `ActionOutputChunk` | 流式输出、截图、进度、日志。 |
| `ActionCompleted` | 成功结果。 |
| `ActionFailed` | 失败原因。 |
| `ActionDenied` | 策略或用户拒绝。 |
| `ActionCancelled` | 用户或系统取消。 |
| `CheckpointCreated` | 文件或状态 checkpoint。 |
| `RollbackAttempted` | 尝试回滚。 |
| `EvidenceCaptured` | 用于能力评估和认知更新的证据。 |

这些事件要同时服务：

- 用户 UI。
- Gateway event stream。
- AgentLoop tool result 回填。
- Session history。
- CapabilityEvidenceStore。
- Cognitive Evolution Kernel。
- 后续 eval/replay。

## Shell / Terminal / PTY

Shell 是 M0 必须有的手脚，但它不应该成为所有动作的默认通道。

M0 ShellBackend 要求：

- cwd 明确。
- env 明确，secret 最小化注入。
- timeout/deadline。
- stdout/stderr 分离。
- output budget 和截断提示。
- exit code。
- cancellation。
- background job 基础模型。
- approval/sandbox 接入。
- command risk classification。
- shell type 和 login shell 控制。
- Windows/PowerShell 与 Unix shell 差异进入 backend metadata。

M1 PTY 要求：

- 交互式会话 ID。
- screen/output buffer。
- input event。
- resize。
- idle detection。
- attach/detach。
- monitor hooks。
- long-running job 状态。

设计原则：

- 能用结构化 FileBackend 的文件修改，不走 shell。
- 能用 AppApiBackend 的 API 调用，不走 shell。
- 能用 BrowserBackend 的 DOM 操作，不走纯截图坐标。
- shell 输出进入模型前要经过截断、脱敏和结构化摘要。
- shell 写入造成的副作用默认不可承诺完全回滚。

## FileBackend

文件触点是 M0 必须能力，但要拆成多种操作：

- read。
- write。
- patch。
- mkdir/remove/move/copy。
- list/glob/search。
- diff。
- checkpoint。
- rollback。

关键点：

- 路径必须按 workspace/project/sandbox scope 解析。
- 写操作必须能生成 diff 或 patch summary。
- 删除和递归移动必须高风险。
- checkpoint 只覆盖 FileBackend 可管理的改动。
- shell、外部 API、浏览器下载造成的文件变化，需要 FileWatcher 或 post-scan 才能发现，不能天然算入 checkpoint。
- 文件内容作为 evidence 进入认知层前要考虑 privacy/data policy。

FileBackend 是主动学习和长期记忆的重要入口。Agent 主动读取用户文档、项目文件、历史记录时，必须由 ObservationScope 和 DataPolicy 控制，不能因为“读文件是低风险”就无限读取。

## BrowserBackend

BrowserBackend 适合网页、SaaS、调试、本地应用 UI 等触点。它应优先使用结构化浏览器自动化，而不是纯视觉坐标。

能力方向：

- navigate。
- screenshot。
- DOM snapshot。
- click/type/select。
- wait。
- download/upload。
- console/network logs。
- cookie/profile scope。
- local/remote CDP。
- browser session attach/detach。

安全重点：

- SSRF 和本地网络访问。
- 登录态隔离。
- 下载文件处理。
- 第三方页面 prompt injection。
- 表单提交和消息发送默认高风险。
- 自动化点击前后要有截图/DOM 证据。
- 浏览器 profile 不能默认共享用户主浏览器。

M1 可以先做受控 browser backend。ComputerUseBackend 应晚于 BrowserBackend，因为 DOM/协议级自动化更可审计、更可回放。

## ComputerUseBackend

Computer Use 是最像“手脚”的能力，但也是风险最高的触点之一。

它的本质是：

```text
Task
  -> screenshot
  -> model returns structured UI actions
  -> harness executes click/type/scroll/key/wait
  -> new screenshot
  -> repeat
```

forme 对它的定位：

- 不是 M0 核心。
- M1 可以预留接口和实验 flag。
- M2 以隔离环境、测试环境或专用 browser/VM 为主。
- 默认不直接控制用户真实桌面。

必须具备的治理：

- 每次动作前后的截图证据。
- 坐标和目标解释。
- 高风险 UI 操作前审批。
- 禁止默认访问密码管理器、支付、系统设置、私密聊天。
- session 级 isolation。
- action rate limit。
- 失败和重复动作检测。
- 可人工接管。
- 第三方内容不可信原则。

Computer Use 不应绕过 ToolRegistry。它可以是一个 backend，也可以由模型原生 computer tool 驱动，但在 forme 内部仍应走 ActionIntent、PolicyEngine、ApprovalBroker 和 EventStore。

## MCP Backend

V7 已经定义 MCP 属于 Capability Substrate。V8 补充执行层要求。

MCPBackend 要区分：

- `mcp_tool_call`：模型可触发的外部动作。
- `mcp_resource_read`：应用控制的上下文读取。
- `mcp_prompt_load`：工作流提示模板。
- `mcp_elicitation`：MCP server 请求用户输入。

执行层要求：

- server identity。
- tool namespace。
- schema snapshot。
- transport。
- timeout。
- output limit。
- trust level。
- permission scope。
- refresh/version event。
- tool result normalization。
- MCP server stderr/log 隔离。
- elicitation approval。

MCP 动态发现不能直接扩大模型上下文。正确链路是：

```text
MCP server discovered
  -> McpRegistry
  -> CapabilityRegistry
  -> ToolsetResolver
  -> schema 延迟加载 / tool search
  -> ToolsetScopeRecheck
  -> McpBackend execution
```

## AppApi 与 Connector

很多真实主动式能力不应该通过 shell 或浏览器完成，而应该走 API connector：

- Slack/Teams/Discord。
- GitHub/GitLab。
- Jira/Linear。
- Notion/Confluence/Google Drive。
- Gmail/Outlook。
- Calendar。
- Database。
- Monitoring/Sentry/Datadog。
- CRM/ERP。

AppApiBackend 的核心是身份和权限：

- 使用 Agent 自己的 identity，还是用户 delegated identity。
- token scope。
- channel/workspace/team scope。
- rate limit。
- audit log。
- read/write 分离。
- dry-run。
- idempotency key。
- duplicate delivery protection。

channel scoped identity 对 forme 很重要：同一个 Agent 不应该在 sales channel 学到的东西默认用于 engineering channel，也不应把私信数据带到群聊。AppApiBackend 必须天然支持 identity/scope 分离。

## Gateway 与 Surface

forme 不应该把 Gateway 理解成“以后做 WebSocket 服务”。Gateway 是所有入口的控制面抽象。

入口包括：

- CLI。
- TUI。
- Desktop UI。
- Web UI。
- HTTP API。
- app-server。
- Slack/消息平台。
- webhook。
- cron/scheduler。
- MCP inbound。
- file/system monitor。
- future mobile。

建议定义 `SurfaceRegistry` 与 `GatewayProfile`：

| 对象 | 作用 |
|---|---|
| `Surface` | 一个入口类型，例如 CLI、Slack channel、API token、cron job。 |
| `SurfaceIdentity` | 谁在发起请求，用户、channel、bot、service account。 |
| `GatewayProfile` | 入口默认模型、权限、toolset、memory scope、approval channel、token budget。 |
| `IngressEvent` | 从入口进入的消息、文件、webhook、schedule、monitor event。 |
| `EgressEvent` | Agent 输出、通知、审批、结果投递。 |
| `SessionRouter` | 把入口事件路由到 session/agent/job。 |
| `ApprovalChannel` | 在该入口中如何完成审批。 |

Gateway 的核心职责：

- 统一 run submission。
- 统一 session event stream。
- 统一身份和 scope。
- 统一 approval delivery。
- 统一 background job 管理。
- 统一 token/budget/audit。
- 统一 channel memory boundary。

M0 可以不实现 daemon，但必须定义 Gateway-ready protocol：

```text
submit_run
stream_events
respond_approval
cancel_run
resume_run
list_sessions
get_run_status
create_background_job
deliver_notification
```

M1 再实现 local daemon/app-server。

## 模型配置

模型配置不能只是一个 `model = xxx`。主动式 Agent 需要 model profile 和 routing policy。

建议对象：

| 对象 | 作用 |
|---|---|
| `ModelProvider` | 云端模型服务、Chat Completions 兼容接口、本地模型、企业网关等 provider。 |
| `ModelProfile` | 某个模型的上下文、tool calling、vision、computer use、reasoning、成本、延迟、可靠性能力。 |
| `ModelCredentialRef` | 凭据引用，不把密钥混入普通 config。 |
| `ModelRoutingPolicy` | 根据任务类型、风险、成本、延迟、上下文长度选择模型。 |
| `FallbackPolicy` | provider 失败、rate limit、模型不支持工具时如何降级。 |
| `ContextBudgetPolicy` | 不同模型的上下文预算、压缩触发、schema budget。 |
| `ToolCompatibility` | 模型是否支持函数调用、并行工具、结构化输出、vision、computer。 |
| `ReviewModelPolicy` | 需要 verifier、critic、safety review、reflection 时用哪个模型。 |

模型配置的核心原则：

- provider、model、auth、base_url、headers、rate limit、cost profile 分开。
- user/project/surface/agent/job 都可指定 model profile，但受 managed policy 限制。
- 模型是否能调用某工具不是 prompt 决定，而是 ModelProfile + ToolCompatibility 决定。
- 主动任务可用便宜模型做观察和摘要，但高风险行动前应切换强模型或 verifier。
- 认知进化相关任务需要记录使用了哪个模型，便于评估和回滚认知更新。

## 配置体系

建议 forme 从一开始定义 typed config schema，不要等实现后再补。

配置层级建议：

```text
Managed / organization requirements
  -> system config
  -> user config
  -> profile config
  -> workspace/project config
  -> agent profile config
  -> surface/gateway config
  -> session/job override
  -> CLI/API one-off override
```

配置分类：

| 分类 | 内容 |
|---|---|
| `model` | 默认模型、provider、routing、fallback、reasoning。 |
| `providers` | provider catalog、base_url、capabilities、rate limit。 |
| `permissions` | permission profiles、approval、sandbox、network、filesystem。 |
| `tools` | enabled toolsets、disabled tools、risk overrides。 |
| `mcp` | servers、transports、env、allowlist、refresh、output limits。 |
| `skills` | skill dirs、enable/disable、auto trigger、preapproved tools。 |
| `plugins` | plugin dirs、trust、marketplace、managed allow/deny。 |
| `hooks` | lifecycle hooks、matcher、timeout、fail policy。 |
| `gateway` | surfaces、auth、ports、channels、approval delivery、event stream。 |
| `execution` | shell、pty、file、browser、computer、remote executor settings。 |
| `memory` | memory scopes、retention、privacy、channel separation。 |
| `proactive` | proactive levels、idle work、schedule、notification policy。 |
| `budget` | tokens、cost、time、rate limits。 |
| `observability` | logs、trace、audit、metrics、debug export。 |

Secrets 不应直接混入普通 config。普通配置引用 `CredentialRef`，由 secret store、env、OS keychain 或用户手动注入提供。

## Config Doctor

模型、网关、工具、MCP、浏览器、终端、插件都依赖环境。没有 doctor，用户体验会变成模型反复猜错。

`ConfigDoctor` 应检查：

- config schema。
- provider credentials。
- model availability。
- model/tool compatibility。
- MCP server start/connect/list tools。
- shell availability。
- PowerShell/Bash/Git/Node/Python/Rust 等依赖。
- browser install/CDP/Playwright。
- sandbox backend availability。
- filesystem permissions。
- network policy。
- plugin manifest validity。
- gateway port/auth/channel config。
- profile state isolation。

诊断结果应是结构化对象：

```text
ok / warning / error / disabled
  -> area
  -> reason
  -> affected capability
  -> fix suggestion
  -> auto_fix_available
```

setup/config/doctor 是长期维护成本的大头。forme 如果目标是主动式 Agent，doctor 更重要，因为主动任务经常在用户不盯着的时候运行，失败必须可解释、可恢复。

## 与主动式和进化式学习的关系

执行触点层不是 Cognitive Evolution Kernel 的附属工具箱，而是认知进化的证据来源。

每次动作都应反馈：

- 是否成功。
- 为什么失败。
- 用户是否批准。
- 用户是否撤回或否定。
- 哪个工具更适合。
- 哪个模型判断更准确。
- 哪个 backend 更稳定。
- 哪种风险需要更严格策略。
- 哪个场景需要新 skill、MCP 或 connector。

这些进入：

```text
ActionOutcome
  -> CapabilityEvidenceStore
  -> AgentSelfModel capability confidence
  -> LearningPipeline
  -> SkillUpdateProposal / ToolUpdateProposal
  -> EvolutionGovernor
```

主动式 Agent 的执行边界：

| 主动等级 | 可做动作 |
|---|---|
| L0 Observe | 只观察授权事件和只读上下文，不主动打扰。 |
| L1 Suggest | 生成建议、提醒、学习任务，不执行外部副作用。 |
| L2 Prepare | 起草、整理、分析、生成待批准计划或草稿。 |
| L3 Act With Approval | 在明确 scope 内请求批准后执行写操作或外部动作。 |
| L4 Act Autonomously | 仅限低风险、可回滚、预授权、预算内动作。 |
| L5 High Impact | 高影响、不可回滚、敏感披露、关键修改或外部承诺动作；不能默认自治，必须显式审批。 |

Execution Substrate 必须能表达这些等级。否则“主动式”会变成无限制自动化。

## 初始能力准入规则

按照“核心保持窄、能力在边缘扩展”的原则，forme 应为新触点设准入顺序。

当需要新增能力时，按以下顺序判断：

1. 是否能通过现有 shell/file/browser/API 组合完成，并用 skill 描述流程。
2. 是否适合作为 CLI/API command，由 Agent 调用但不进入 model tool schema。
3. 是否适合作为 service-gated tool，只在配置完成时进入 toolset。
4. 是否适合作为 plugin，打包工具、skills、hooks、MCP config。
5. 是否适合作为 MCP server，供 forme 和其他 MCP host 复用。
6. 是否足够通用、不可替代、几乎每个用户都需要，才进入 core backend/tool。

这个准入规则能避免 M0 变成工具大杂烩，也能避免主动式 Agent 在早期就背上过大的安全和维护负担。

## M0/M1/M2/M3 切分建议

### M0

M0 目标：本地可靠执行底座和配置骨架。

必须包含：

- `Execution Substrate` 概念边界。
- `ActionIntent` / `ExecutionPlan` / `ActionBackend` 数据边界。
- `ShellBackend` MVP。
- `FileBackend` MVP。
- `McpBackend` stdio MVP。
- `ExecutionBackendRegistry`。
- `PolicyEngine` MVP。
- `ApprovalBroker` MVP。
- `ActionEvent` 基础事件。
- output budget、timeout、cancellation。
- `ModelProvider` / `ModelProfile` / `ModelRoutingPolicy` 配置骨架。
- config schema 和 config precedence。
- secrets 与 config 分离。
- Gateway-ready run/session/event/approval protocol。
- ConfigDoctor MVP。

M0 不做：

- 完整 Gateway daemon。
- 完整 browser/desktop control。
- 远端 executor。
- 插件市场。
- 全自动 model routing。
- L4 自主行动。

### M1

M1 目标：多入口、后台任务、浏览器和动态能力。

增强：

- local gateway daemon 或 app-server。
- SurfaceRegistry / GatewayProfile。
- background job / scheduler MVP。
- ApprovalChannel 多入口。
- BrowserBackend。
- PTY/terminal session。
- MCP dynamic discovery / schema 延迟加载。
- tool search。
- plugin runtime lifecycle。
- model routing MVP。
- config hot reload 和 ConfigChange event。
- channel-scoped memory 基础边界。
- proactive L1/L2。

### M2

M2 目标：主动式协作和跨系统触点。

增强：

- AppApiBackend connector framework。
- Slack/消息平台 gateway。
- ComputerUseBackend experimental。
- RemoteExecutorBackend。
- richer sandbox backends。
- managed policy。
- cost/token budget per surface/channel/job。
- connector identity / delegated identity。
- proactive L3。
- capability performance scoring。
- tool/backend recommendation。

### M3

M3 目标：执行策略自进化。

增强：

- backend selection policy evolution。
- execution simulation/replay。
- action risk model evolution。
- skill/tool/backend promotion/rollback。
- proactive L4 受控开放。
- long-horizon autonomous project work。
- multi-agent parallel execution governance。
- organization-grade audit and compliance。

## 风险与取舍

### 不要让 shell 成为万能工具

shell 很强，但它的副作用最大，且回滚边界最弱。文件编辑、API 调用、浏览器操作、消息发送都应该优先使用结构化 backend。

### 不要让 Computer Use 过早成为核心

Computer Use 很符合“手脚”的直觉，但它依赖视觉和坐标，风险比 DOM/API 级自动化高。M0 应先把 backend/policy/event 做好，M2 再引入隔离环境下的 ComputerUseBackend。

### 不要把 Gateway 推迟到实现后期

即使 M0 不做 daemon，也要让 core API Gateway-ready。否则 CLI 先行会把 session、approval、event、config 写死，后续主动式和多入口会重构得很痛。

### 不要把模型配置当成 UI 选项

模型配置决定 Agent 的推理能力、工具兼容、成本、上下文、延迟和安全边界。它必须进入 runtime policy 和 event trace。

### 不要把插件、MCP、hook 变成安全旁路

所有扩展都必须进入 registry、policy、approval、event、evidence。越强的扩展能力，越需要更强的来源治理。

## 当前结论

forme 的底座应该在 V7 能力基质之上，补上 V8 执行触点与配置体系：

- `Capability Substrate` 负责知道和选择能力。
- `Execution / Embodiment Substrate` 负责把能力变成真实动作。
- `Surface / Gateway` 负责所有入口、身份、审批和事件流。
- `Model Config` 负责模型、provider、routing、预算和工具兼容。
- `PolicyEngine` 和 `ApprovalBroker` 负责安全和人类授权。
- `EventStore` 和 `CapabilityEvidenceStore` 把动作结果反馈给认知进化内核。

这样设计后，forme 的“手脚”不是一堆工具，而是一套可治理的行动系统。它能支撑当前的通用 Agent 能力，也能支撑未来主动式、长期协作、自我学习和自我进化。

## 参考资料

- [Model Context Protocol introduction](https://modelcontextprotocol.io/docs/getting-started/intro)
- [Model Context Protocol resources specification](https://modelcontextprotocol.io/specification/2025-06-18/server/resources)
