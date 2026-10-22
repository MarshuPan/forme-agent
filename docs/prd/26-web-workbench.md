# prd/26 Web 工作台

## 0. 定位与承上

实现 [requirements/17](../requirements/17-installed-product-scope-decisions.md) P12–P16，主验 IP17–IP23，参与 IP04/IP06/IP08/IP18/IP24。依据 [architecture/10](../architecture/10-installed-product-and-web-workbench-architecture.md) §5–6/§10，以及 canonical §13–18/§28–29。

**状态：基础工作台和工作空间命令层已实现，完整产品面仍在实施。** 当前已接入 React + TypeScript + Vite 静态构建、初始化/登录、对话、事件续接、审批/取消、项目登记/路线规划/执行/取消和 Charter 读写；由 forme-webd 提供。附件/产物、完整收件箱处置、记忆/认知操作面、完整设置/维护页、已认证浏览器全链路和 IP17–IP24 原生联合验收仍未完成。DeepSeek 的官方展示截图只是结构参考；无桌面壳、无独立 Node 后端。

## 1. 职责与边界

为唯一主大脑提供完整的浏览器交互：初始化/登录外壳、对话、工作空间、收件箱、记忆与认知、设置与维护。它展示真实任务/认知事实、提交受治理命令，不重新实现推理、记忆、插件、调度或编排。

允许使用 DeepSeek 页面结构和经审查可拆分的前端控件/对话交互；不引入其宿主容器、RPC 框架、插件发现或业务服务端。组件无法分离就使用 forme 自有实现，不能以“前端需要”为由把其核心搬入。合规例外精确见 [frontend-reuse-policy](../compliance/frontend-reuse-policy.md)。

## 2. 全局位置与集成契约

| 本模块层次 | 唯一接口 | 事实来源 |
|---|---|---|
| Web shell/router/session gate | prd/25 auth/status API | 服务端 session 和 ProductState。 |
| typed API client | prd/25 的 allowlisted API 与既有 core DTO | command receipts、safe snapshots、授权 artifacts。 |
| event reducer/query cache | scoped cursor + snapshot watermark | 原 EventKind/stream_seq 与 server projections。 |
| 业务页面/控件 | 上述统一 client/reducer | 无私有 model/tool/memory/SQL 调用。 |
| build/reuse inventory | 产品 release manifest、组件来源清单 | 固定本地静态资源/lockfile/SBOM。 |

主大脑只能有一个 UI 身份。workspace 不显示为另一个具有人格/记忆的 Agent，subagent 只在任务详情中表现为执行单元；用户向主大脑沟通，不被迫向多个角色重复提交。

前端无稳定事实写者。charter、候选、审批、权限、模型配置等 mutation 只发正式 command；显示 pending 到服务器确认，不能直接改 query cache 后当成成功。产品管理记录与脑事件分开消费，不自造 EventKind。

## 3. 数据结构与类型

wire DTO 从 protocol/schema 生成或进行兼容性校验，均带 `schema_version`；前端 view model 只保存最少字段，不复制 Rust 内部所有对象。

| 对象 | 字段/约束 |
|---|---|
| `WorkbenchSessionView` | `authenticated_owner_label, auth_expiry, product_state, active_workspace_ref?, allowed_routes`；没有 provider key/internal bearer。 |
| `ConversationView` | `session_ref, workspace_ref?, title, run_refs, messages, pending_actions, cursor`；消息带来源/公开输出类型，不把工具结果提升成指令。 |
| `CommandDraft` | `draft_id, workspace_ref?, body, attachment_refs, options`；提交前无 run ref，提交时产生稳定 key。 |
| `SubmittedCommand` | `client_command_ref, idempotency_key, receipt, pending_status`；超时保留 key，重连取原 receipt。 |
| `RunView` | `run_ref, actual_status, public_steps, output_refs, pending_kind?, evidence_refs, event_watermark`。Waiting/Unknown 不能折叠为 Complete。 |
| `ApprovalView` | `approval_ref, final_action_summary, normalized_parameters, affected_resources, risk, rollback_boundary, expiry, binding_ref, current_validity`；高级详情可看 digest/schema/policy。 |
| `WorkspaceView` | `workspace_ref, charter_revision, goals, constraints, history_refs, active_runs, intentions, supervision, route_refs`；没有独立认知 owner。 |
| `InboxItemView` | `item_ref, kind, origin_ref, scope, requested_response, current_status, expiry?, allowed_commands`；已读与已处置分开。 |
| `CognitionView` | `object_ref, type, scope, stability, confidence, evidence_refs, lineage, candidate_or_stable, allowed_reviews`；无隐藏 CoT。 |
| `SettingView` | `safe_value, configured, effective_source, revision, active_ack, restart_required, validation`；不含服务器 secret 原文。 |

本地 UI state 仅有当前 tab、折叠面板、无秘密草稿和未确认 command key。禁止把 credentials、全文 memory、trace、上传内容或可继续执行的离线任务队列持久化到 localStorage/IndexedDB。注销/owner session 失效清内存 query cache，返回页面前重新认证，避免共享设备残留展示。

## 4. 接口与签名

```typescript
interface WorkbenchClient {
  session(): Promise<WorkbenchSessionView>;
  hostStatus(): Promise<HostStatusView>;
  query<T>(query: ScopedQuery): Promise<VersionedSnapshot<T>>;
  submit(command: TypedWorkbenchCommand,
         binding: CommandBinding): Promise<WebCommandReceipt>;
  subscribe(scope: StreamScope, cursor: StreamResume,
            onEvent: (event: SafeStreamEnvelope) => void): StreamHandle;
  artifact(ref: ScopedArtifactRef): Promise<AuthorizedArtifact>;
}
```

API 不接受任意 URL/SQL/方法名；TypedWorkbenchCommand 是已存在的 run/control/review/charter/config/maintenance 命令的明确联合。CommandBinding 包含 idempotency key、expected revision 和对应 pending binding，不含客户端伪造 principal。

reducer 为纯函数：`reduce(snapshot, ordered_event) -> view`，重复 event 不改变结果，gap 返回需要重新同步。多 run 列表使用 server cursor，不拼出全局 stream_seq。stream.close 仅关闭连接，不取消任务。

### 4.1 工作台投影与认知处置适配

`POST /v1/brain/workbench/query` 接受版本化 `WorkbenchQuery`（section、可选 scope、offset、limit、可选 snapshot digest）。这是无模型调用、无写入的认证查询；每页最多 100 项。分页绑定实际投影摘要，快照变化返回冲突，调用方重新读取第一页；不把跨 run 的事件序号拼成全局游标。对象包含原 run/event/stream_seq、证据、血缘、状态与允许的处置。原始材料只返回授权 content ref，未解密的引用不冒充正文。

`POST /v1/brain/workbench/review` 的 `WorkbenchReviewCommand` 只接受 command key、run、candidate、expected_state、decision。owner 来自认证，证据、稳定目标与传递派生关系由正式 core 重新解析，再调用既有 CandidateReviewCommand/governor。浏览器不能指定 actor、证据或失效列表；确认仍可能因证据不足被拒绝。

适配层不新增脑 EventKind。认知 review 的命令回执使用既有 `MemoryMaintenanceApplied.deltas_ref` 的保留编码 `workbench-review-v1|key_digest|binding_digest|started/confirmed/rejected|target_digest`，只记录命令绑定、目标摘要与处置状态，不携带用户正文或秘密。相同 key/相同绑定返回原回执；相同 key/不同绑定拒绝。同一候选的控制串行，存在未确认处置时，新 key 同样不能重新执行。已记录 started 但没有终态时返回 Unknown，不盲重放中间提升/撤回；UI 查询真实对象状态和原回执。此记录是认知维护审计，不充当安装、认证或配置事实。

### 4.2 数据工作台的安全适配

Web 数据工作台使用正式 core 的封闭查询/命令：`POST /v1/data/workbench/query`（目录、原操作状态、备份验证）与 `POST /v1/data/workbench/command`（备份、准备恢复、准备生命周期、批准原计划）。浏览器入口为 `/api/v1/data/query|command`；不经过通配 brain proxy，mutation 先验证 owner、CSRF、命令号与本次命令摘要绑定的口令，再由签名 bridge 发往 core。口令不进入 core、journal 或日志。

目录仅展示 scope、subject digest、对象/投影数量、生命周期版本与 pending fence，不返回表名、row key、加密 envelope 或 key digest。分页使用实际目录 digest，快照改变返回冲突。生命周期范围预览展示直接与派生对象总数及不可逆性；计划内部明细留在 core。恢复只创建新的 opaque 本地对象，不切换 active brain，不接受任意路径；完成不能宣称已经恢复外部世界或旧 key。

数据命令在现有 protected-content 存储中保存 owner/命令绑定的加密操作伴随记录（started、confirmed 不可变对象）；这不是新的认知事实源，批准/生命周期/完成仍使用既有权威事件和消费账本。完整计划仅在 core 解密，浏览器收到安全 preview、原命令号与 plan digest。相同号/不同参数冲突，同号重复查询原回执；started 缺少 confirmed 返回 Unknown，不再次运行备份、恢复或删除。准备/批准分成不同命令；批准重新加载原加密计划并调用已有 plan-bound one-shot 路径。删除执行期间及失败后的 data fence 保留，不能为工作台读取绕过它。

操作列表/原回执在刷新、重启后仍可核实；Unknown 没有默认重试按钮。备份验证是只读文件/完整性/replay检查，不是批准、导入或切换。数据删除回执区分本地结果与远端 Unknown，不把密钥计数或组件测试当成远端擦除证明。内部操作记录不作为任意产物下载源。

## 5. 页面、行为与状态

### 5.1 全局外壳与初始化

采用左侧导航/会话/工作空间列表、中间工作区、右侧可折叠详情/产物。五个主入口只有一套 router、会话状态与命令 client。窄屏左/右栏抽屉化，主对话保持可用；无需模仿 NAS 桌面窗口系统。

路由门先读 session/ProductState：Unclaimed 进认领；ConfigurationRequired 进配置；Starting 显示真实启动进度；Ready 正常进入；Degraded 明确受影响能力；Maintenance/RecoveryRequired/Stopped 进入相应维护视图。core 离线时 shell/设置/诊断可用，对话 mutation 显示不可用及原因，不能无限 spinner。

初始化向导按“本机证明 → 选择并验证脑数据 → 创建账号并完成认领 → 模型与密钥 → workspace/能力 → 检查与启动”执行。高级项可折叠，不能把所有安全字段甩给用户填写；不要求用户理解 env、crate 或内部 bearer。证书未被客户端信任、模型未实测等保持准确状态。

### 5.2 对话

- 会话列表支持创建、查看历史、重命名/归档和 workspace 关联；这些是同一脑的对话，不是新增 Agent。
- composer 支持文本、授权附件、停止生成/任务控制；模型选择只提交被允许的 profile hint，实际 profile 由 core 决定并显示。
- 提交即锁定 idempotency key，返回 receipt 后绑定 run；双击/刷新/网络超时都复用或查询原命令。
- 流式显示公开回答、动作名称/资源/状态、等待审批、产物与失败。展示模型明确公开的 reasoning 摘要须符合内容/保留策略；不制造“脑内独白”。
- 重启/恢复后读取原 run 时，Harness 从其受保护事件流只读重建已提交最终回答：匹配同一 model call/profile 的 delta 与 finished、Final classification，且 RunComplete 的停止原因必须为 final_output；未完成、工具/handoff、失败/中止/限额运行和后续内部思考不成为回答。读取不调模型、不创建 run、不写新事件；有界文本或存储错误明确失败，不静默截断。Web 不另存回答作为第二事实源。
- 子代理/DAG 是任务详情，可查看节点、依赖、当前阻断和验证结果，不能向 child 私发越权命令。
- RunWaiting/PendingKind 显示用户需要做什么；Unknown 显示“结果待核实”及 probe/裁决入口；没有“重新执行以试试看”的默认按钮。
- 工具终态、验证与最终回答分开：模型说完成但 verifier 未通过时显示验证失败/未验证，不使用绿色成功覆盖。
- 附件在 server 接受后才进入 command；取消草稿不自动删除原始项目文件。产物按授权 ref 预览/下载，不暴露实际磁盘路径。

### 5.3 审批与控制

审批卡片给用户可理解的“要做什么、影响哪些对象、会产生什么结果、能否撤销、有效期”，参数和高级摘要可展开。当前 exact binding 来自服务器，点击批准后等待 one-shot 消耗与 action receipt，不乐观显示已执行。

拒绝、过期、参数变化/陈旧 revision 显示明确结果并重新获取 pending；不能代用户重新审批。L5 是这一次操作，不提供默许全部。取消的 requested/acknowledged/completed 各有实际状态；取消命令不是 backend 已停止的证据。

需对 unknown 裁决时首先展示原 receipt/只读探测和风险；owner 新动作仍须新 intent/plan/审批与既有恢复规则。维护重启与取消任务也分开，不用刷新页面触发服务重启。

### 5.4 工作空间

展示 charter/目标、约束、资料范围、进度/历史、活跃任务、长期 intentions/监管、决策与 route。owner 编辑 charter 走 expected_revision/CAS；影响进行中任务的变更显示将用于后续协调，不 mid-run 热换。

可以暂停/继续长期任务、请求检查/复盘/重新协调，具体入口仍调用主脑正式 command；页面不自行定时触发任务或跑编排。未使用编排、单编排、多编排都是合法状态，不以 workspace 存在强制创建 subagent。

“脑对项目的判断”与“项目操作记录/资料”分栏展示 scope/provenance；思考和进化由主脑承担，workspace 保留常规项目状态。恢复旧项目时使用 core recall/charter/历史结果，不新造前端项目 memory。

### 5.5 收件箱

统一显示主动建议、AskToLearn、审批、需要 owner 的失败/Unknown 和项目提醒，并标明来源、相关 workspace/run、为何此时需要响应及当前处置。

按类型提供采纳、拒绝、延后、回答、探测/裁决等允许操作；点击/已读不等于采纳。采纳建议若涉及真实动作，会进入既有新计划/审批，不直接调用工具；拒绝抑制/延后 intention 由后端事件确认。过期项保留历史，不用前端删除隐藏失败。

普通主动建议通过 closed `WorkbenchProposalCommand`（`schema_version, command, run, proposal, outcome, defer_until`）处置；key 与 command 必须一致，run/proposal 必须指向已发出的普通建议，不能用该入口回答 AskToLearn。延后必须指定未来时间并生成既有 DeferredProposal intention；采纳/拒绝不得夹带时间或替换 actor。回执复用 `MemoryMaintenanceApplied` 的 `workbench-proposal-v1|key_digest|binding_digest|started/confirmed/rejected|target_digest`，target 锁覆盖不同 key 的并发。started 无终态返回 Unknown，原命令可查询但换 key 不得绕过未决 target；已有同义终态先返回，不能因延后时间已过去而破坏历史核实。采纳只确认 owner outcome，不创建 Run、审批或动作，后续执行仍走主脑正式计划与权限路径。

### 5.6 记忆与认知

区分原始记录（会话/每日/episode/反思等视图）与认知（UserModel、AgentSelfModel、CognitiveMap、strategy 等）。长期/每日/会话是读取组织方式，不建立另一套 markdown 文件事实源。

每个对象可查看 scope、稳定性、置信度、时间、来源证据和血缘；候选/稳定/撤回/待重评清楚区分。owner 可纠正、确认/拒绝候选、撤销或请求遗忘，提交到正式 governor/data-lifecycle。用户文本修改不是前端直写稳定对象。

删除/遗忘展示精确范围与不可逆性、本地完成和远端 Requested/Unknown。隐藏/归档聊天与真正删除分开；不能以删除页面卡片冒充原文/FTS/keys 已清理。证据受权限/保留限制时显示原因，不扩大披露。

### 5.7 设置与维护

| 设置分区 | 必须展示/允许的动作 |
|---|---|
| 模型渠道 | profiles/角色路由、实际有效来源、endpoint、密钥已配置状态、doctor/实测、变更待重启。key 输入只写。 |
| 消息渠道 | 已有 ChannelAdapter 的配置、连接/错误、参与者与 owner binding、授权/撤销。没有实现的 adapter 不显示假开关。 |
| 能力 | skills/plugins/MCP/backends/packages 的发现、安装、启用、trust、permission 各自状态；连接现有 M5/MCP 治理。 |
| 权限与审批 | 当前 policy/envelope/grants、scope/expiry/revoke；不以全局 trusted mode 替代最终检查。 |
| 网络与账号 | 本地/LAN、HTTPS/证书信任、防火墙、登录会话、口令修改/撤销、恢复指引。 |
| 数据 | backup manifests、验证、恢复新路径候选、retention/delete 结果和恢复材料状态。 |
| 系统 | service health、core composition、版本、config ack、诊断、受控重启/升级/回滚与卸载说明。 |

看板引用现有权威统计：numerator/denominator/unverifiable 同时呈现，不将单一通过率作为成长承诺。设置通过后台版本事务落地；“已保存”“等待重启”“已生效”“失败待修复”区分。core-offline 模式不伪造脑数据，只显示可证实 host facts 与修复操作。

### 5.8 断线、长任务与错误

连接丢失显示时间和最后 watermark，指数退避+抖动；401 重新登录、429 尊重 Retry-After、409 更新 snapshot/提示冲突、503 显示 core/维护状态。不要所有错误都自动重发 mutation。

恢复读取 snapshot + cursor，再接 stream；已有消息按 event identity 去重，不以最后一段文字去重。跨 tab 共享只读会话状态/失效通知；命令各自使用服务端幂等，不能依赖单 tab disabled button 防重复。

## 6. 错误、安全、权限与审批

- 所有 HTTP 错误按 prd/25 ApiProblem 显示具体问题、关联 ref 与下一步；不把 transport success 当 task success。
- Markdown 禁止执行 raw HTML/script；URL 协议白名单、外部链接隔离 opener、图片来源/隐私策略明确。内容中的“系统消息/审批按钮”只作为文本，不能变成真实 UI 控制。
- HTML/SVG/文档预览采用隔离 iframe/下载，CSP 禁止任意 script/connect；不把不可信 artifact 放在共享认证 origin 执行。安全隔离做不到则只允许下载。
- secret/password 输入不可自动回填实际值；无分析 SDK 采集聊天/配置内容；认证响应/脑内容 no-store，service worker 不缓存敏感 API。
- 重连、logout/BFCache、共享设备会话均先检查身份再展示私有缓存；不在浏览器持久化可利用 credentials。
- 真实 action 由 core enforcement，UI 隐藏按钮只是体验而非授权机制；直接构造 API 仍必须被后端拒绝。

## 7. 配置与可观测

前端 build 固定 asset base、CSP 配合、版本/schema compatibility 和许可清单；不接受运行时注入任意 upstream module/远程插件。release_info 显示 UI/backend 是否匹配，静态资源带 content hash，缓存更新不能混用旧新协议。

交互指标仅包含去身份化延迟/断线/重连/gap/command result code，不默认上传内容。可由 owner 复制的诊断摘要只含安全 refs；敏感输入不得出现在 console、错误 boundary、URL 或 screenshot 自动采集。

## 8. 实施步骤与顺序

1. 对 DeepSeek 固定 source revision 做组件/依赖审查，提交 provenance/NOTICE/SBOM；不可拆组件采用原创实现，不先批量搬运。
2. 定义 UI DTO/client、认证/router gate、event reducer 与错误模型；与 prd/25 完成真实 login/bootstrap/status 联调。
3. 实现对话/composer、附件/产物、SSE 续接与 exact approval/control；先验证幂等/Unknown/安全内容渲染。
4. 实现 workspace、收件箱、记忆/认知三面；所有按钮接正式事件/command，删除假 mock 成功路径。
5. 完成 settings/维护、core-offline、LAN 信任/数据恢复流程；加入窄屏/键盘/屏幕阅读器支持。
6. 以安装包静态 UI 验 IP17–23/IP24，并复跑相应 core 永久门；Vite dev server 不等于发布成品。

## 9. 测试

单元重点 reducer 重复/gap、snapshot merge、command key、审批过期/冲突与错误状态，不为静态布局镜像编写无意义测试。交互集成用自建 fixtures，覆盖表单、权限、附件、候选、长任务续接与真实服务错误。

浏览器端验证 IP17–23：两 tab 重复提交只一个 run；流中刷新后完整；logout 后无私有缓存；审批参数变化 driver=0；恶意 Markdown/HTML/SVG/路径不能执行/越权；core crash 时设置仍可用；读流与命令独立限流。验证 Windows/macOS/Linux 支持浏览器与窄屏，键盘完成登录/对话/审批。

至少用真实安装包/真实 model/backend 完成一次从配置到对话/工具审批/项目监管/候选回流/重启续接的联合路径；测试模型只证明交互状态，不能证明 Agent 有用。生成新的产品 evidence，不重写历史报告。

## 10. 验收

五个工作面完整接入同一个大脑，页面事实均可追溯到 server projection/事件；无私有 core、假按钮、假进度或模拟成功。DeepSeek 复用边界、artifact 隔离、密钥保护、断线/重复/未知结果均有正反证据。

Ready/Degraded/Maintenance/RecoveryRequired 路径均可实际操作；IP17–23 及 IP24 逐项有结果，不把截图/结构存在升级成可用性 PASS。核心历史验收与新产品平台验收分别保留。

## 11. 原创性与合规

唯一批准的源码复用范围为 [frontend-reuse-policy](../compliance/frontend-reuse-policy.md) 的逐文件前端白名单，保留 MIT/第三方 notices；本模块的 API/reducer/业务/安全 fixture 原创。当前没有导入任何 upstream 源码，不在实际依赖清单虚报已使用。

React/TypeScript/Vite 及 renderer/component/icon 等实际依赖实施时锁版本并审计，外部图片/字体/品牌资源也需授权。禁止把 research/upstreams 加为 alias/import，禁止大范围排除 copy detector；合规与 UI bundle dependency graph 均为发布阻断门。
