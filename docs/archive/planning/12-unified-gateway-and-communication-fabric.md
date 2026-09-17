# 统一网关与沟通触手：Interaction / Communication Fabric

本文档是 forme 的 V11 规划补强。它承接 `08-execution-touchpoints-model-gateway-config.md`、`10-coordination-kernel-and-resource-orchestration.md`、`11-cognitive-map-trust-and-failure-evidence.md`，专门回答用户提出的“触手”问题：

> forme 不应只通过文字 CLI 和用户交互。它需要支持文字、语音、图片、视频等沟通形式，也需要通过软件渠道和硬件设备承载这些沟通，并能代表用户在受控边界内对外沟通，甚至与其他 Agent 协作。

本文仍然是规划文档，不定义最终 Rust crate/API、数据库 schema 或实现步骤。本文涉及的对外膜、RepresentationPolicy（已折叠进 DisclosurePolicy）、身份模型等口径，以 `../../architecture/canonical-contract.md`（§5/§10）为准。

## 核心结论

forme 的沟通触手不应该直接长在 `AgentHarness` 上，而应该统一长在 `Unified Gateway / Interaction & Communication Fabric` 上。

也就是说：

```text
沟通形式
  -> text / voice / image / video

承载层
  -> software carrier: CLI / Web / mainstream messaging / collaboration platform / email / phone system / meeting app / API
  -> hardware carrier: microphone / speaker / camera / screen / phone / PC / wearable / future device

Channel Adapter
  -> 把平台、设备、协议、媒体格式转成标准事件

Unified Gateway / Communication Fabric
  -> identity / session / scope / auth / permission / routing / delivery / pairing / allowlist / rate limit / audit

CommunicationSession
  -> participant / modality / channel / purpose / disclosure policy / deadline / budget / termination policy

AgentHarness
  -> run / policy / coordination / cognition / memory / proactive / failure evidence
```

统一网关不是 CLI 之外的另一个入口，而是所有外部沟通触点的归一控制面。CLI、Web、消息平台、协作平台、电话、麦克风、摄像头、外部链接、Agent-to-Agent 会话，都应通过统一的身份、会话、权限、事件和投递模型进入 forme。

## 为什么不能直接接到 Harness

如果每个沟通触手都直接调用 `AgentHarness`，早期会很快，长期会形成严重分叉：

- CLI 有一套 session。
- 某个消息或协作平台有另一套 session。
- 本地麦克风和摄像头又有一套临时事件。
- 外部链接对话无法和用户授权、披露范围、截止时间统一。
- Agent 间沟通容易变成无限 token 消耗。
- 主动式观察、通知、审批、对外沟通会绕过相同的 policy 和 audit。

因此，沟通触手必须先进入统一网关，再由网关转成标准化 `IngressEvent`、`CommunicationEvent`、`RunRequest`、`ApprovalResponse` 或 `DeliveryRequest`，最后交给 harness 治理。

`AgentHarness` 仍然是运行时最高边界，但它不应该亲自理解每个平台的 webhook、音频流、图片格式、视频帧、电话状态或群聊线程。Harness 应该接收已经规范化、带有身份和权限边界的事件。

## 统一网关的设计原则

统一网关是通用 Agent 的重要工程方向。forme 从中抽象出三点：

- 平台入口必须标准化成统一事件，而不是每个平台一个 Agent。
- Gateway 必须管理身份、session、allowlist、pairing、投递和审计。
- Channel 和设备能力必须通过 adapter/registry 接入，而不能绕过 harness、policy、approval 和 event store。

## 沟通形式层

沟通形式先按人类交互形态划分，而不是先按平台划分。

| 形式 | 说明 | 初始处理方式 | 风险 |
|---|---|---|---|
| Text | 文本消息、命令、富文本、文档片段、表单字段。 | 标准 `TextMessageEvent`。 | 上下文注入、身份混淆、群聊隐私。 |
| Voice / Audio | 实时语音、语音消息、通话、TTS 输出、会议音频。 | STT/TTS/Reatime adapter 统一成音频事件和 transcript。 | 误识别、旁人声音、录音授权、持续监听。 |
| Image | 用户上传图片、截图、摄像头单帧、文档图片。 | `ImageObservationEvent`，带来源、scope、用途和保留策略。 | 隐私、敏感信息、视觉误判、截图过度采集。 |
| Video | 视频流、会议画面、摄像头、屏幕录制、短视频附件。 | M0 只定义边界，后续转成帧、片段、转录和关键事件。 | 信息量巨大、持续观察风险、误触发、存储成本。 |

这个分层避免把“某个消息平台”“麦克风”“摄像头”混在同一层讨论。消息平台是软件承载，麦克风和摄像头是硬件承载，文字/语音/图片/视频才是交互形式。

## 承载层

承载层分为软件承载和硬件承载。

### 软件承载

软件承载是用户、外部人、外部系统或其他 Agent 通过某个软件通道与 forme 交互。

典型类型：

- CLI / TUI。
- Web / Desktop。
- HTTP API / Webhook。
- 主流 IM、团队协作、邮件、短信等消息平台。
- 会议软件、电话系统、客服系统。
- 外部分享链接。
- Agent-to-Agent protocol 或 future agent network。

软件承载的关键不是“接多少平台”，而是所有平台都要归一成：

```text
PlatformEvent
  -> ChannelAdapter
  -> IngressEvent
  -> Gateway routing / auth / policy
  -> CommunicationSession or RunRequest
```

### 硬件承载

硬件承载是设备本身提供的输入输出能力。

典型类型：

- 麦克风。
- 喇叭。
- 摄像头。
- 屏幕。
- 键盘/鼠标。
- 手机。
- PC。
- 未来可穿戴设备、车载设备、家庭设备。

硬件承载不应绕过 Gateway。即使是本机麦克风、摄像头，也应该作为 `LocalDeviceChannelAdapter` 或 Gateway-adjacent bridge 进入统一事件链路。

原因是硬件输入往往更敏感：

- 麦克风可能听到非目标用户。
- 摄像头可能看到房间和隐私内容。
- 屏幕可能包含密钥、聊天、文档、支付、客户资料。
- 喇叭/TTS 可能在不合适环境中外放敏感信息。

因此，本地设备也必须有 `DeviceGrant`、`ObservationScope`、`RetentionPolicy`、`DisclosurePolicy` 和 `AuditEvent`。

## Channel Adapter

`ChannelAdapter` 是平台和设备的隔离层。它负责处理协议细节，但不能拥有 Agent 行为。

职责：

- 接收原始平台事件、webhook、音频帧、图片附件、视频帧或设备事件。
- 解析平台身份、空间、thread、message id、reply target。
- 下载或引用附件，生成可审计 media ref。
- 做基础去重、重试、rate limit 和幂等保护。
- 转成标准 `CommunicationEvent`。
- 标注 modality、carrier、participants、trust level、permission scope、data sensitivity。
- 负责出站消息的格式适配和投递结果回传。

不做：

- 不直接调用模型。
- 不直接写长期 memory。
- 不直接执行工具。
- 不绕过 Gateway 做审批。
- 不把平台特有概念泄露到 harness 内核。

## Unified Gateway / Communication Fabric

`Unified Gateway` 是沟通触手的控制面。它不是一个单纯的 WebSocket server，也不是 platform adapter 的集合。

它至少负责：

- `Identity`：用户、外部人、外部 Agent、设备、bot、service account 的身份绑定。
- `Participant`：一次沟通中的参与者、角色、权限、是否代表用户。
- `Session Routing`：把 channel/thread/link/device event 路由到 session、agent、job 或 communication session。
- `Auth / Pairing / Allowlist`：谁可以联系、通过什么方式、是否需要用户授权。
- `Scope`：本次沟通可使用哪些上下文、记忆、工具、模型、对外披露范围。
- `Delivery`：出站消息、审批请求、通知、结果投递、失败重试。
- `Conversation State`：外部沟通的开始、进行、暂停、结束、过期和归档。
- `DisclosurePolicy`：哪些信息能说、不能说、需要模糊化、需要先问用户。
- `TerminationPolicy`：什么时候结束沟通，避免无限对话和无限 token 消耗。
- `Audit`：所有外部收发、授权、披露、拒绝、过期、异常都要进入事件。

对 forme 来说，Gateway 有三种入口职责：

1. 把沟通事件转成 run。
2. 把 run 中需要人类确认的 approval 投递给合适的沟通渠道。
3. 把 Agent 的结果、问题、通知或对外表达投递回合适目标。

## CommunicationSession

对外沟通和 Agent 间协作不能只用普通 chat session 表达。它需要独立的 `CommunicationSession`。

建议边界：

| 对象 | 作用 |
|---|---|
| `CommunicationSession` | 一次有目的、有边界、有参与者、有终止条件的沟通会话。 |
| `ParticipantProfile` | 参与者身份：用户本人、外部人、外部组织、外部 Agent、设备、bot。 |
| `CommunicationPurpose` | 目的：介绍用户、收集信息、协商事项、教学、答疑、Agent-to-Agent 任务对接。 |
| `ModalitySet` | 本次允许使用 text、voice、image、video 中哪些形式。 |
| `CarrierRef` | 通过哪个软件或硬件承载。 |
| `DisclosurePolicy` | 可以披露哪些用户信息、项目事实、联系方式、能力、案例、限制和不可披露项。 |
| `AutonomyEnvelope` | Agent 在本次沟通中可自主表达、追问、总结、转交或结束到什么程度。 |
| `Deadline / TTL` | 会话截止时间或最大轮次。 |
| `Budget` | token、时间、消息条数、外部 API 成本。 |
| `TerminationPolicy` | 目标达成、预算耗尽、超时、风险升高、用户撤销、对方无关时如何结束。 |
| `TranscriptPolicy` | 是否保存 transcript、保存多久、是否允许进入候选记忆。 |
| `HandoffPolicy` | 什么时候必须回到用户本人、什么时候可以继续代理表达。 |

`CommunicationSession` 的存在，是为了避免“外部人打开链接就能无限问 Agent 关于用户的一切”。每次对外沟通都必须有目的、范围、期限、披露规则和停止条件。

## 对外沟通链接

用户提出的“让合作伙伴通过链接了解我”是一个典型能力。

推荐抽象：

```text
User creates ExternalCommunicationGrant
  -> Gateway creates scoped link
  -> external participant enters CommunicationSession
  -> Agent answers within DisclosurePolicy
  -> risky / unclear question triggers user approval or refusal
  -> session ends by goal / TTL / budget / user revoke
  -> transcript becomes event evidence, not automatic stable memory
```

关键设计：

- 链接不是公开永久 Agent。
- 链接必须绑定目的，例如“让对方了解我的背景和合作方式”。
- 链接必须绑定披露范围，例如“可以讲职业经历和公开项目，不讲私人信息、客户数据、财务、内部计划”。
- 链接必须绑定时效和预算。
- 对方身份不明时，默认低信任。
- Agent 可以礼貌拒答、转人工、请求用户确认或结束会话。
- 会话结束后生成 digest，但不自动把外部人的信息写入稳定记忆。

这个能力本质上是主动式 Agent 的外化：Agent 不只是陪用户对话，也能在用户授权下代表用户完成有限目的的外部沟通。

## 思想代理与代表表达

“Agent 代表我讲一堂课、讲述一个事情、进行有限对接”不应被理解为简单复读用户资料。

它需要三层约束：

1. `UserModel / PartnershipModel / AgentSelfModel`：了解用户的风格、能力、目标、边界和短板，也了解 Agent 自身能否可靠代表表达。
2. `DisclosurePolicy.representation`：知道哪些观点可以代表用户说，哪些只能说“可能倾向于”，哪些必须回到用户确认。
3. `CommunicationSession / TerminationPolicy`：知道这次代表表达的场景、受众、目标、时长、截止条件和风险升级路径。

Agent 可以代表用户表达思想，但不能声称自己就是用户，也不能把候选认知、低置信判断、私密偏好或未授权内容对外表达为确定事实。

## Agent-to-Agent 沟通

Agent 间沟通是未来方向，但必须从第一天定义风险边界。

一个用户的 Agent 和另一个用户的 Agent 沟通时，双方都应携带：

- 任务目的。
- 参与者身份。
- 允许披露范围。
- 允许请求的信息范围。
- 最大轮次、时间和 token 预算。
- 终止条件。
- 是否允许把对方信息写入候选记忆。
- 是否允许后续再联系。

推荐模式：

```text
AgentA creates A2ACommunicationSession
  -> Gateway validates identity / scope / budget
  -> AgentA sends bounded task brief
  -> AgentB responds within its disclosure policy
  -> both sides negotiate only around declared purpose
  -> session terminates on goal / budget / timeout / policy risk
  -> both sides store digest as candidate evidence only
```

Agent-to-Agent 的默认姿态应该是“短会话、强目的、强终止、弱记忆写入”。不能让两个 Agent 因为礼貌追问或低价值协商无限循环。

## 与三内核的关系

沟通触手不是第四个内核。它是三内核和 harness 对外发生作用的 surface/control layer。

### 对 Cognitive Evolution Kernel

沟通事件是认知输入，但不是稳定认知。

语音、图片、视频、外部链接对话、Agent-to-Agent 会话都只能先进入 event/evidence/candidate 层。是否进入 `UserModel`、`CognitiveMap`、`TrustProfile` 或 `StrategyMemory`，必须经过候选、证据、冲突检查和用户确认。

### 对 Coordination Kernel

协调内核要决定：

- 这次目标应该用文字、语音、图片还是视频。
- 应该通过 CLI、Web、消息平台、电话还是外部链接。
- 是否需要先收集资料再沟通。
- 是否需要先向用户确认披露范围。
- 是否应该由 Agent 直接答、生成草稿、请用户审批、或拒绝。
- 什么时候结束沟通。

因此 `ResourceInventory` 必须扩展出 communication resources：

- 可用沟通形式。
- 可用渠道。
- 可用设备。
- 可用身份。
- 可用审批通道。
- 可用披露策略。
- 当前打扰成本和外部风险。

### 对 Proactivity Kernel

主动式能力需要沟通触手才能真正发生：

- 主动提醒用户。
- 主动追问缺失信息。
- 主动准备对外回复草稿。
- 主动识别外部沟通机会。
- 主动把审批请求投递到最合适渠道。
- 主动在 deadline 前跟进，但不越权执行。

Proactivity Kernel 可以提出 `CommunicationProposal`，但真实投递必须经过 Gateway、policy、approval 和 communication session。

## 与能力层和执行层的关系

沟通触手不是普通工具，但会调用工具。

例如：

- 对外链接会话可能需要读取用户已批准的公开简介。
- 协作平台消息可能需要读取某个文档。
- 电话沟通可能需要 STT/TTS。
- Agent-to-Agent 会话可能需要检索项目摘要。
- 外部人询问某个项目细节时，Agent 可能需要生成 `ApprovalRequest` 给用户。

因此沟通层要和以下能力协作：

- `CapabilityRegistry`：知道有哪些平台、设备、STT/TTS、media parser、delivery backend。
- `ExecutionBackendRegistry`：执行消息发送、语音播放、截图读取、API 调用。
- `PolicyEngine`：判断是否允许读取、表达、发送、保存。
- `ApprovalBroker`：把审批投递到当前最合适的渠道。
- `EventStore`：记录所有收发和披露。
- `FailureEvidence`：记录误投递、误披露、误识别、对话失控、超预算、未按时终止等失败。

## M0/M1/M2/M3 切分

### M0

M0 必须把统一沟通触手纳入完整核心闭环，但不要求接入大量平台或真实硬件能力。

M0 要求：

- 定义 `Interaction / Communication Fabric` 作为 Gateway 的一等子域。
- CLI + Local Gateway/App Server 共享 `RunRequest`、event stream 和 approval protocol。
- 定义 `CommunicationEvent`、`CommunicationSession`、`ParticipantProfile`、`DisclosurePolicy`、`TerminationPolicy`、`ExternalCommunicationGrant` 的需求边界。
- 支持 text modality 的最小路径：CLI / Local Gateway 进入标准事件。
- 支持 Gateway-compatible approval delivery，不把审批绑死 CLI。
- 支持外部沟通 session 的数据边界：scope、TTL、budget、disclosure、transcript policy。
- 支持 `CommunicationProposal`，由 Proactive/Coordination 提出，由 Gateway/Harness 治理。
- 将沟通失败纳入 `FailureEvidence`。

M0 不做：

- 真正接入多种消息或协作平台。
- 真正接入麦克风、摄像头、电话系统。
- 真正对外生成公网链接。
- Agent-to-Agent 协议实现。
- 实时语音/视频。
- 默认常驻监听或看屏幕。

### M1

M1 目标是本地和低风险沟通体验增强：

- Local Gateway/App Server 可承载 Web/本地控制台。
- 外部 link session 的本地或受限版本。
- Notification / ApprovalChannel 第一版。
- LocalDeviceAdapter 设计落地：麦克风、喇叭、摄像头只在显式授权下进入实验路径。
- text + image 输入支持增强。
- STT/TTS 可以作为可选 backend，不默认常驻。
- CommunicationSession trace viewer。

### M2

M2 目标是跨平台主动协作：

- 首批主流消息和协作平台 adapter。
- channel-scoped memory。
- per-channel identity、budget 和 approval delivery。
- 外部链接对话正式可用。
- voice call / meeting / phone system 的实验适配。
- Agent-to-Agent bounded session 实验。
- communication resource scoring 进入 CoordinationReasoner。

### M3

M3 目标是沟通策略进化和高阶代表能力：

- 沟通策略 replay/eval/promotion/rollback。
- disclosure policy 辅助生成和风险模拟。
- 长期 relationship model 候选。
- 多 Agent 协作协议治理。
- 代表表达质量评估。
- proactive communication policy 受控进化。

## 风险边界

### 不允许无限沟通

外部链接和 Agent-to-Agent 必须有 TTL、最大轮次、最大 token、最大 wall time 和终止策略。

### 不允许默认披露

Agent 了解用户，不代表可以对外讲用户的一切。对外沟通必须默认最小披露。

### 不允许硬件默认监听

麦克风、摄像头、屏幕只能在明确授权、明确 scope、明确用途、明确保留策略下启用。

### 不允许把外部对话直接变成长记忆

外部人说的话、其他 Agent 说的话、平台消息里的内容都只能先进入 evidence/candidate，不自动写入 stable memory。

### 不允许把“代表用户”说成“就是用户”

Agent 代表用户表达时必须保持身份边界。它可以说“我代表用户在本次授权范围内说明”，不能伪装成用户本人。

### 适配器必须原创实现

forme 不能复制第三方的 platform adapter 目录、实现、文案、错误信息、测试 fixture 或协议封装结构。

## 当前结论

forme 的“触手”应被设计为统一的 Interaction / Communication Fabric：

- 沟通形式层定义 text、voice、image、video。
- 承载层区分 software carrier 和 hardware carrier。
- Channel Adapter 抹平平台、设备、协议和媒体格式。
- Unified Gateway 统一身份、session、权限、scope、投递、审批和审计。
- CommunicationSession 定义对外沟通和 Agent 间协作的目的、范围、期限、预算、披露和终止。
- AgentHarness 仍然是最终运行和治理边界。

这样设计后，forme 才能从 CLI Agent 演进成真正主动式、可进化、可协调、能代表用户进行有限外部协作的个人/工作空间 Agent，而不会因为平台和设备触点增多导致权限、记忆、审批、披露和审计碎片化。
