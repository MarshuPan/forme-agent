# prd/26 Web 工作台

## 0. 定位与承上

实现 [requirements/17](../requirements/17-installed-product-scope-decisions.md) P12–P16，主验 IP17–IP23，参与 IP04/IP06/IP08/IP18/IP24。依据 [architecture/10](../architecture/10-installed-product-and-web-workbench-architecture.md) §5–6/§10，以及 canonical §13–18/§28–29。

**状态：分阶段实施中，不代表 PRD26 或 IP17–IP24 已完成。** 当前分支已有 React + TypeScript + Vite 静态构建、初始化/登录、对话与事件续接、审批/取消及运行未知结果裁决、会话 workspace 到后续正式 run 的上下文绑定、项目/路线/Charter、加密附件与授权产物历史、记忆/认知投影与候选处置、AskToLearn 和普通主动建议处置、活动运行待决 Inbox 投影、普通建议跨刷新原命令恢复，以及部分模型配置/受限维护页面。上述均有不同范围的组件证据；完整工作台、授权原始内容与真实遗忘、完整设置/维护、真实浏览器全链路及原生产品联合验收仍未闭合。恢复组件见 `installed-product-proposal-recovery-acceptance.md`，真实浏览器证据仍单列。状态以产品化进度记录和各组件报告为准。DeepSeek 的官方展示截图只是结构参考；无桌面壳、无独立 Node 后端。

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

### 4.3 恢复候选的独立激活

成功执行批准的恢复计划后，DataBackupView 增加可选、版本化的 `restoration` 安全投影：candidate_ref、最终 manifest 的 candidate_digest、owner_digest、store_identity、execution_fence_digest。它只能由正式 RestoreReport 的已验证结果构造；普通备份/历史缺字段回执不提供激活按钮。不得把 database_digest 当 candidate_digest，不向浏览器输出数据库/vault 路径，也不让浏览器自行填写校验结果。

候选完成与活动主脑切换仍是两次独立批准。工作台读取当前 Host 维护快照及配置 revision，展示候选身份、配置版本、会话将撤销、旧任务不会恢复执行和原数据保留的确认，再通过现有 operation-bound 近期口令与 `ProductMaintenanceAction::CommitRestore` 提交确切引用/摘要/owner/config revision。只在 Host 声明允许此操作时显示可用入口；缺 helper/未安装服务不得以 UI 开关伪造能力。

提交后保留原 operation；结果不明只核实原回执，不换号重试、不重新发送口令。恢复撤销当前登录时重新登录后从维护记录核实，不能把 401 当恢复失败或自动重做。Host 在副作用前拒绝当前已经激活的同一候选；实际候选仍须由固定 worker 重新验证，浏览器投影不是授权依据。页面的已提交、待核实和恢复受阻状态与原生验收分开记录。

### 4.4 会话提交与附件历史的实施约束

新会话草稿独立于服务器列表刷新：没有服务端记录的草稿不能自动切回第一条旧会话。创建、上传和发送在第一次 await 前取得同一前端排他提交权；创建会话和提交 run 各自固定命令号与原始输入。丢失创建或运行响应后仅允许原命令的幂等核实，不能因双击、重渲染或换 tab 新建另一会话/运行。未决提交期间不能换会话或悄悄替换草稿。

附件历史由正式 core 提供，Web 不用浏览器持久存储充当目录。`POST /v1/artifacts/query` 接受 closed `ArtifactCatalogQuery{schema_version,session,page:DataPageQuery}`，返回版本化 `ArtifactCatalogView{session,digest,total,next_offset,artifacts}`；条目含 content_ref、content_digest、scope、bytes 和可选 file_name/media_type。分页仍用实际快照摘要，无模型调用、无写入；未授权 owner、错误 session、陈旧快照和数据 fence 均拒绝。

上传保留原始字节的现有加密记录，另存有界加密元数据 companion（同 session scope/subject，绑定原 content_ref、session/workspace、原始摘要/长度/名称/类型）。同号不同内容冲突；重复原上传可以补齐尚未写完的元数据，不重复创建原始对象。元数据对象不进入模型上下文、不作为下载对象；生命周期清理同 scope/subject 时一同清理。历史无 companion 的原始附件仍可读，列表明确名称/类型未知，不伪造原名，也不在查询时迁移。

附件原始字节上限保持 4 MiB；hex JSON 的专用请求上限为 8 MiB + 8 KiB，只开放精确的 `POST /api/v1/brain/artifacts/upload` 和对应 core 路径。普通控制请求沿用原上限，认证、CSRF、签名请求摘要和数据 fence 不变；不得全局放大普通请求限额。目录每 session 最多检查 20,000 个加密对象（含 companion），历史无元数据的单次解密扫描上限为 32 MiB；超限显式拒绝，不截断后冒充完整目录。分页参数不绕过目录上限，迁移须另走明确维护流程。

前端切换/刷新会话即从 core 重读目录，按 content_ref 去重，下载绑定当前 session。上传未确认保留原文件和原命令号在内存，仅显式核实相同上传；不自动重传或用新 key 重试。用户注销/会话失效清理这些内存状态。这里补齐 prd/26 §5.2 已批准的对话与附件义务，不新增脑事件、私有记忆或依赖边。

### 4.5 会话 workspace 与正式运行上下文绑定

会话记录的 `workspace` 关联必须影响此会话后续提交的每个用户 run，而非仅用于 Web 分类。Web 在 owner 身份认证后，从独立 identity DB 的该 owner 会话记录解析 workspace；浏览器不得在 `RunRequest` 或其他请求字段中指定/覆盖 workspace。会话不存在、属于其他 owner 或已归档时拒绝创建新 run；未关联 workspace 使用当前正式 runtime 的默认 workspace。

Web 在同一 identity DB 单写者事务中读取会话当前关联，并为首次提交的 `(owner, session, idempotency_key)` 持久固定 workspace、规范化请求 digest 和操作状态。操作 key 重用但 digest 不同必须冲突；会话关联变更与 run 首次绑定由该事务线性化，后续改关联只影响之后的新 command。创建请求的响应丢失、Web/core 任一方重启或 core 接纳结果未知时，保留原 binding，重发/核实只能使用同 key、同 digest、同 workspace；不得按新会话关联换 workspace，也不得自动换 key。core receipt 确认已有 run 后保存 run ref；unknown 保持可核实，不盲目重提副作用。

产品化 Web adapter 将服务端解析并固定的上下文经现有签名 Web→core bridge 发至专用受认证 run ingress。该 ingress 校验 `RunRequest.session` 与 envelope session 一致、owner 来自当前 AuthContext，并调用现有 `ProductionRuntime::submit_workspace_run`；core 再校验 workspace 已注册及 scope/charter 边界，使用既有 `SessionBound` 事件固定实际上下文。该适配不得扩充脑 `EventKind`、改冻结 `RunRequest`/Harness trait、直接写 brain DB 或绕开既有 gateway/harness、policy、context、competence 和 execution 治理。内部 envelope 必须版本化且被 bridge request digest 完整绑定。

会话 workspace 关联只决定默认项目上下文，不授予该 workspace 的任意工具、文件系统或外部动作权限；Workspace 注册/charter 也不替代 Permission/Trust/AutonomyEnvelope。run 已接纳后不热切换；解除关联影响之后的 run，不改写历史 `SessionBound` 或历史 run workspace。

### 4.6 普通建议处置的刷新恢复

Web identity DB 是该恢复记录的唯一 owner；不把普通建议处置命令写入前端持久存储、brain DB 或新增 EventKind。提交 `/api/v1/brain/workbench/proposal` 前，Web 先在 identity DB 单写者事务中插入恢复记录，字段为 `schema_version, owner_digest, command, binding_digest, run, proposal, payload, created_at`；`payload` 是已校验的封闭 `WorkbenchProposalCommand` JSON，不含自由文本、秘密或浏览器提供的 actor。owner 摘要与命令规范化绑定必须可校验，数据库 ACL 保持 Web 服务专属。identity DB 最多保留 256 条 owner 未决记录，超限时在转发主脑前 fail closed。`command` 全局唯一；`(owner, run, proposal)` 同时唯一，阻止同一目标并发生成第二个恢复命令。

同 owner、同 command、同 binding 的预登记是幂等读取；异 owner 或异 binding 重用 command 一律冲突。记录成功后才经现有签名 Web→core bridge 转发原命令。只有解析到版本/command/binding 一致且状态为 `Confirmed` 或 `Rejected` 的正式 `WorkbenchCommandReceipt`，或收到 core 对该原命令的 HTTP 409（该状态只由 command 开始前的确定性冲突返回），才按 owner+command+binding 条件删除记录。Unknown、其他 HTTP/transport 错误、超时、进程中断或删除回执失败均保留记录；不得因刷新、重登、重启或换浏览器生成新 key。

认证后 `GET /api/v1/workbench/proposals/pending` 返回 `{schema_version: 1, commands: WorkbenchProposalCommand[]}`，最多 256 条且只含当前 owner 未决记录。读回时逐条验证 owner、schema 与 binding digest；坏记录导致恢复错误，不跳过或部分显示。前端仅以内存将列表按 `(run, proposal)` 关联 Inbox；匹配不到当前 Suggestion 时仍展示安全的“原处置待核实”条目。恢复动作必须由 owner 显式触发，并原样重发列表中的命令。页面加载/刷新只读不发命令；终态后刷新正式 brain projection。此记录只用于找回命令，不替代主脑回执或主脑事件作为事实源。

### 4.7 项目监管与工作空间权威视图

`GET /api/v1/brain/projects/status` 经既有签名桥调用同一路径的 core 视图，返回 versioned `WorkspaceControlSnapshot { observed_at, revision, workspaces, projects, stream_heads }`。每个 `WorkspaceProjectControlView` 包含 project ref、core 计算的 project digest、`ProjectSupervisionState { schema_version, version, enabled }`、checkpoint、活跃 route refs、下次 cadence、监管判定与理由/证据引用。监管判定为 Paused/ActiveRoute/Due/NotDue/Unverifiable；达到既有一次最多 16 项 review 的上限时，不把未评估项谎报为 NotDue。视图只读，数据 fence 与 scope/owner 验证继续有效；快照中途变化返回 409。单次上限 256 个项目/工作空间，超限拒绝，不默默丢项。

`POST /api/v1/brain/projects/supervision` 接受封闭 `ProjectSupervisionCommand { schema_version, command, project, expected_project_digest, expected_version, enabled }`；Idempotency-Key 必须一致。owner 暂停/恢复后续监管通过既有 `DecisionTraceRecorded.orchestration` 的 additive `ProjectSupervisionChanged` 事实提交；不新增 EventKind、crate 或依赖边。事件只含 project、expected digest/version 与 enabled。`OrchestrationLedger.supervision` 是可重放投影，历史缺失表示 version=0/enabled=true；空 map 在序列化中省略，保留旧 ledger digest。

mutation 在单写者/CAS 内核对当前 project digest 与 supervision version；并发同版本最多一条成功。相同 command/相同语义返回原事件，异义复用拒绝；回执复用带 owner/command binding 的 `WorkbenchCommandReceipt`。暂停不取消在途 route、子代理或未知动作，也不影响 owner 显式规划；恢复仅重新允许下一个受治理背景 tick 规划，不立即执行。背景 snapshot 与最后 route append 都检查 enabled，晚到暂停必须在 route 提交前生效。暂停/恢复不能授予工具或扩大权限。

工作空间页可在默认 workspace 与已登记项目/charter 的 workspace 之间切换；既有 charter API 继续服务各自 workspace。项目编辑使用视图的 authoritative project digest 和既有 ProjectRevised，保留固定 identity/workspace/goal/done；summary、cadence、signature、model/budget 等改变只影响下一 segment。表单未确认时保留原 body/key，409 明确要求重新读取，不能在 catch 后生成新 key 冒充成功。长期意图使用既有 jobs API，按 binding/envelope scope 归属 workspace；取消与暂停监管分开显示，不将 CancelRequested 等同于动作停止。

## 5. 页面、行为与状态

### 5.1 全局外壳与初始化

采用左侧导航/会话/工作空间列表、中间工作区、右侧可折叠详情/产物。五个主入口只有一套 router、会话状态与命令 client。窄屏左/右栏抽屉化，主对话保持可用；无需模仿 NAS 桌面窗口系统。

路由门先读 session/ProductState：Unclaimed 进认领；ConfigurationRequired 进配置；Starting 显示真实启动进度；Ready 正常进入；Degraded 明确受影响能力；Maintenance/RecoveryRequired/Stopped 进入相应维护视图。core 离线时 shell/设置/诊断可用，对话 mutation 显示不可用及原因，不能无限 spinner。

初始化向导按“本机证明 → 选择并验证脑数据 → 创建账号并完成认领 → 模型与密钥 → workspace/能力 → 检查与启动”执行。高级项可折叠，不能把所有安全字段甩给用户填写；不要求用户理解 env、crate 或内部 bearer。证书未被客户端信任、模型未实测等保持准确状态。

设置中的默认工作空间和模型角色编辑与模型列表共用一份配置 revision、保存操作号及未决状态。角色选项只来自该配置内的 profile；编辑未决时锁定原完整 body 并核实原操作，不生成新号。默认 workspace 与某个会话的绑定分开显示，说明变更仅作用于后续未显式绑定的运行；已保存、待重启和 core 已确认分别呈现。不会因改变角色或读取设置而触发模型调用。

### 5.2 对话

- 会话列表支持创建、查看历史、重命名/归档和 workspace 关联；这些是同一脑的对话，不是新增 Agent。
- 关联 workspace 后，该会话后续 run 必须进入对应 workspace 的正式主脑上下文；UI 标签/本地状态不是绑定证据，详情与历史按 run 的 `SessionBound` 显示实际 workspace。
- composer 支持文本、授权附件、停止生成/任务控制；模型选择只提交被允许的 profile hint，实际 profile 由 core 决定并显示。
- 提交即锁定 idempotency key，返回 receipt 后绑定 run；双击/刷新/网络超时都复用或查询原命令。
- 流式显示公开回答、动作名称/资源/状态、等待审批、产物与失败。展示模型明确公开的 reasoning 摘要须符合内容/保留策略；不制造“脑内独白”。
- 重启/恢复后读取原 run 时，Harness 从其受保护事件流只读重建已提交最终回答：匹配同一 model call/profile 的 delta 与 finished、Final classification，且 RunComplete 的停止原因必须为 final_output；未完成、工具/handoff、失败/中止/限额运行和后续内部思考不成为回答。读取不调模型、不创建 run、不写新事件；有界文本或存储错误明确失败，不静默截断。Web 不另存回答作为第二事实源。
- 若 run 因重启时缺少 `ModelCallFinished` 而以 `model_call_outcome_unknown_after_restart` 失败，界面明确说明回复和费用未知、没有自动重试，并提示 owner 只能核对原事件或另起新 run；不得显示为处理中、成功或“可恢复续跑”。
- 子代理/DAG 是任务详情，可查看节点、依赖、当前阻断和验证结果，不能向 child 私发越权命令。
- RunWaiting/PendingKind 显示用户需要做什么；Unknown 显示“结果待核实”及 probe/裁决入口；没有“重新执行以试试看”的默认按钮。
- 工具终态、验证与最终回答分开：模型说完成但 verifier 未通过时显示验证失败/未验证，不使用绿色成功覆盖。
- 附件在 server 接受后才进入 command；取消草稿不自动删除原始项目文件。产物按授权 ref 预览/下载，不暴露实际磁盘路径。

### 5.3 审批与控制

审批卡片给用户可理解的“要做什么、影响哪些对象、会产生什么结果、能否撤销、有效期”，参数和高级摘要可展开。当前 exact binding 来自服务器，点击批准后等待 one-shot 消耗与 action receipt，不乐观显示已执行。

拒绝、过期、参数变化/陈旧 revision 显示明确结果并重新获取 pending；不能代用户重新审批。L5 是这一次操作，不提供默许全部。取消的 requested/acknowledged/completed 各有实际状态；取消命令不是 backend 已停止的证据。

需对 unknown 裁决时首先展示原 receipt/只读探测和风险；owner 新动作仍须新 intent/plan/审批与既有恢复规则。维护重启与取消任务也分开，不用刷新页面触发服务重启。

待决状态由服务端事件的强类型投影给出（ApprovalWait、ToolInterrupt、Handoff、ResolutionUnknown），不改变冻结的 RunSummary。页面从原检查点和公开事件读取 target/原结果引用，裁决表单只提交既有 closed RunControl。待确认时锁定原 command 和表单参数；重连查询事件，必要时以同 command 核实原裁决，不生成新 command。若 claim 已落盘而浏览器刷新，认证后的 pending 投影可附带经密文记录核验的原 `RunControl`；页面只恢复原表单并等待 owner 显式核实，不自动提交，也不把新表单覆盖旧命令。密文缺失/无效时只展示 Unknown 和安全处置说明，不允许换号重裁。未知 action 明确区分 owner 确认完成/失败/取消与真正 backend 探测；不能把人工确认称为独立验证。已解决 handoff 但后续中断只可核实或取消，不显示再次交接按钮。

### 5.4 工作空间

展示 charter/目标、约束、资料范围、进度/历史、活跃任务、长期 intentions/监管、决策与 route。owner 编辑 charter 走 expected_revision/CAS；影响进行中任务的变更显示将用于后续协调，不 mid-run 热换。

可以暂停/继续长期任务、请求检查/复盘/重新协调，具体入口仍调用主脑正式 command；页面不自行定时触发任务或跑编排。未使用编排、单编排、多编排都是合法状态，不以 workspace 存在强制创建 subagent。

“脑对项目的判断”与“项目操作记录/资料”分栏展示 scope/provenance；思考和进化由主脑承担，workspace 保留常规项目状态。恢复旧项目时使用 core recall/charter/历史结果，不新造前端项目 memory。

### 5.5 收件箱

统一显示主动建议、AskToLearn、审批、需要 owner 的失败/Unknown 和项目提醒，并标明来源、相关 workspace/run、为何此时需要响应及当前处置。

运行待决项复用既有 `RunPendingView`，不在 Web/Inbox 新造状态机：认证查询投影当前非终态且 `pending != null` 的运行，使用既有 `WorkbenchItemKind::Waiting` 表示，`event/stream_seq` 指向该运行当前检查点，`state` 区分 `ApprovalWait`、`ToolInterrupt`、`Handoff`、`ResolutionUnknown`。摘要使用安全的固定说明，不把原始参数、外部内容或未授权文本复制进收件箱。打开条目只导航到原 run；详情再读取权威 pending/approval 并使用现有 typed control。收件箱本身不批准、不探测、不裁决，也不创建新 command。该组件已实现；真实浏览器刷新/断线恢复、多 Web 进程以及 IP20 全流程仍按 §9/§10 单独验收。

终态 `Failed`/`Limited` 不等同于“待处理”；有 `FailureEvidenceRecorded` 时作为失败证据显示并链接原 run。模型调用在重启后结果未知的终态失败，仍须清楚标成回复/费用未知，不能生成自动重试入口。运行 pending 投影以 terminal event 和当前 `RunPendingView` 为准，不把历史 `RunWaiting` 当成仍待处理。

按类型提供采纳、拒绝、延后、回答、探测/裁决等允许操作；点击/已读不等于采纳。采纳建议若涉及真实动作，会进入既有新计划/审批，不直接调用工具；拒绝抑制/延后 intention 由后端事件确认。过期项保留历史，不用前端删除隐藏失败。

普通主动建议通过 closed `WorkbenchProposalCommand`（`schema_version, command, run, proposal, outcome, defer_until`）处置；key 与 command 必须一致，run/proposal 必须指向已发出的普通建议，不能用该入口回答 AskToLearn。延后必须指定未来时间并生成既有 DeferredProposal intention；采纳/拒绝不得夹带时间或替换 actor。回执复用 `MemoryMaintenanceApplied` 的 `workbench-proposal-v1|key_digest|binding_digest|started/confirmed/rejected|target_digest`，target 锁覆盖不同 key 的并发。started 无终态返回 Unknown，原命令可查询但换 key 不得绕过未决 target；已有同义终态先返回，不能因延后时间已过去而破坏历史核实。采纳只确认 owner outcome，不创建 Run、审批或动作，后续执行仍走主脑正式计划与权限路径。

普通建议处置在 Web 转发前先写 owner-scoped identity 恢复记录，终态主脑回执确认后才删除；认证后的未决命令查询供刷新/重启后人工找回原 key 与原 payload。Unknown/断线只显示并核实同一命令，不自动提交、不生成新 key。若原建议已从活动 Inbox 消失，恢复记录仍以安全待核实项呈现，直到取得明确终态；身份库中的恢复元数据不是脑事实，主脑事件/回执仍为最终依据。

### 5.6 记忆与认知

区分原始记录（会话/每日/episode/反思等视图）与认知（UserModel、AgentSelfModel、CognitiveMap、strategy 等）。长期/每日/会话是读取组织方式，不建立另一套 markdown 文件事实源。

每个对象可查看 scope、稳定性、置信度、时间、来源证据和血缘；候选/稳定/撤回/待重评清楚区分。owner 可纠正、确认/拒绝候选、撤销或请求遗忘，提交到正式 governor/data-lifecycle。用户文本修改不是前端直写稳定对象。

删除/遗忘展示精确范围与不可逆性、本地完成和远端 Requested/Unknown。隐藏/归档聊天与真正删除分开；不能以删除页面卡片冒充原文/FTS/keys 已清理。证据受权限/保留限制时显示原因，不扩大披露。

#### 5.6.1 只读材料正文

新增 additive 产品适配 DTO `WorkbenchMaterialQuery { schema_version: 1, node, run, event, scope?, snapshot }` 与 `WorkbenchMaterialView { schema_version: 1, node, run, event, scope, content_ref, state, bytes?, content_digest?, text? }`。`snapshot` 是同一 Memory 列表及 scope 的权威摘要；node/run/event 必须共同指向当前投影中的一项，不能把任意 ContentRef、路径或 URL 作为读取入口。既有 WorkbenchQuery/Item/Snapshot、脑事件与冻结 trait 均不变。

认证后的 `POST /api/v1/brain/workbench/material` 经原签名桥到 core `/v1/brain/workbench/material`。Harness 在数据读取 fence 和 owner 校验内重建当前 Memory 投影、核对 snapshot/节点/源事件，再读取服务端选出的内容；结束前复核 stream heads。陈旧列表返回 409，跨 owner/不存在的节点拒绝。查询不调模型、工具或后台 tick，不创建 run、脑事件、内容记录或新的解密 key。

只有实际产物 writer 登记的正文可读。执行产物与 owner 上传正文在创建时附加加密、版本化的 `runtime-artifact-material-v1:{content_ref}` companion，绑定确切 ref、scope、subject、正文摘要及字节数；companion 自身的地址由该命名空间计算，不能由正文 JSON 自报身份。companion 和正文受同一数据生命周期主体控制，正文读取通过当前 owner 的 catalog 校验及原加密 binding；不开放通用 protected-content 解密接口，不把控制裁决、data-operation、strategy 或其他内部 companion 当作可下载原文。

结果状态为 `Available / ReferenceOnly / MissingOrErased / TooLarge / NonText`。最多返回 65,536 字节的完整 UTF-8 纯文本；不截断、不执行 Markdown/HTML/SVG，不将原文字段解释成 prompt。缺少可信 companion 的历史/外部/内联引用保持 ReferenceOnly，不猜测其原文；正文缺失或被生命周期阻断、二进制和超限分别明确显示。既有只读引用与来源追踪继续可用。

页面由 owner 显式点击后查询，正文仅存在当前条目的内存状态；关闭/切换视图或登录失效时清除。请求完成后必须核对 node/run/event/scope/ref、schema、状态与 text/bytes/digest 一致性；刷新列表不会自动读取所有正文。测试覆盖已登记正文、刷新/重启后的同一内容、陈旧快照/跨 scope/跨 owner/伪造节点、内部 companion 冒充、真实生命周期删除、超限/非 UTF-8、安全文本渲染和零模型/零事件/零 key 创建。

#### 5.6.2 从具体记忆进入真正的数据生命周期

记忆条目的“准备遗忘”先做只读范围解析，不删除卡片或原文。新增产品只读适配 `/api/v1/brain/workbench/material-lifecycle` → 原签名桥 → core `/v1/brain/workbench/material-lifecycle`，请求继续使用 §5.6.1 的 `WorkbenchMaterialQuery`。响应为新的 `WorkbenchMaterialLifecycleView { schema_version: 1, node, run, event, scope, content_ref, subject, lifecycle_version }`；不改既有对象、trait、DataWorkbenchAction 或脑事件。subject 由同一当前记忆行、真实 writer 登记的材料 companion 和 protected-content 记录共同解析，不能从 node/ref 字符串猜测，更不能把内部 companion 当作用户材料。来源缺失、被删除、scope 不符或陈旧列表均拒绝；不要求解密正文，也不创建事件、模型调用或 key。

页面核对响应与原条目身份、scope/ref、摘要格式和版本后，仅以内存把范围带入现有设置的数据管理区；不放入 URL 或浏览器持久存储，注销后清除。跳转不自动发送生命周期命令。该区仍使用既有 `DataWorkbenchAction::PrepareLifecycle`，绑定返回的 scope/subject/lifecycle_version 并要求口令；版本已变化时阻止沿旧选择准备，owner 需重新从当前条目解析或明确选择数据目录中的其他范围。

准备后显示正式 core 的对象/派生项总数、范围、有效期与不可逆性：一个主体可能包含多条来源或派生记忆，不能宣称只删除当前卡片。实际 Delete/CryptoShred 继续用独立、显式确认、口令和 plan digest 绑定的既有 Apply 命令；Unknown 只查原回执，不能通过换号或刷新重试删除。确认后显示本地清理/key 销毁和真实远端状态，旧正文、下载及主脑上下文均遵守原 data fence。认知撤回仍只进入 governor/重评，不因这一入口被等同为永久遗忘。

验收须有 source→subject 的正反绑定、只读零写入、陈旧/跨 owner/伪造/内部引用拒绝，以及正式 Web→core 准备/批准/真实清理/原号核实的临时数据证据；浏览器跨页与整包 IP20/IP24 继续独立取证。

#### 5.6.3 文字纠正与原认知失效

文字纠正与既有确认/拒绝/撤回分开。新增 closed 产品命令 `WorkbenchCorrectionCommand { schema_version: 1, command, run, kind, id, event, snapshot, statement }`，其中 kind 只接受 Candidate/Stable，statement 是 1–8,192 UTF-8 字节的 owner 反馈。命令不接收 actor、scope、原对象、证据、派生列表、置信度、稳定性、权限或新属性类型；认证 owner、当前 Cognition 列表及原事件共同决定这些字段。陈旧 snapshot、来源/状态不匹配、无可靠 scope、已撤回/拒绝/失效及 fixed/constitutional 对象在写入前拒绝。

`POST /api/v1/brain/workbench/correction` 经既有签名桥进入 core 同名正式适配，交给唯一 ProductionRuntime。Harness 从真实原候选/稳定对象构造 owner 纠偏：原稳定认知沿既有 RetractionEvent/传递 ReevaluationTaskCreated 语义停用；未提升的原候选沿既有 CandidateDowngraded 语义处理。文字反馈形成单独、应用层加密的来源记录与 owner evidence，并生成低置信、working 层的纠正判断候选，关联原对象和来源。该候选使用现有 CognitiveMapUpdateProposed/Frame 与 CandidateCreated 路径，不直接修改任何稳定对象；文字不是 UserModel、AgentSelfModel、TrustProfile、PartnershipModel 的结构化字段补丁，也不能改变权限或提高能力自评。

同一纠偏涉及的来源、原对象失效、派生重评和新候选须在 authority store 的一次本地事务内提交，使用所有相关 stream 的 expected version/CAS；事件仍经原 prepare/append/projection/FTS 路径分配 stream_seq。可新增 store 的受限本地原子写入实现方法，不改冻结 trait、同步/复制协议、事件类型或内部依赖图。来源密文与其登记绑定同事务提交；失败回滚全部脑记录，不能先撤回再因候选写入失败留下半次纠偏。规范事件顺序和 ID 由服务端确定，文字不得出现在 journal、命令 ID、摘要标签、日志或错误文本中。

只读核实使用 `POST /api/v1/brain/workbench/corrections/query` 与 closed `WorkbenchCorrectionQuery { schema_version: 1, command? }`，仍经同一签名桥。指定原 command 时返回该命令的真实回执；未指定时只返回同一认知 aggregate 内最近最多 100 条确认回执及 `truncated` 标志，不将跨 run 时间排序当权威。回执登记为既有 MemoryMaintenanceApplied 下的版本化 `workbench-correction-v1` 元数据，仅含 command/binding/target/candidate 等安全引用，不含反馈正文。来源正文和新候选受同一真实清理血缘约束；删除反馈正文不撤销既有失效事实，也不得使原错误认知复活。

相同 owner/command/规范化 body 返回原回执，异义复用拒绝。审批式确认与普通 review 共享原对象的并发边界，并在事务内重查 source stream 与大脑 aggregate 版本，后台变化不能被旧页面覆盖。断线/写入结果不明时只查询同一命令的正式回执；原子事务使核实结果只能是完整提交、没有接纳记录或明确故障，禁止用新 key 自动再纠正。回执包含 `schema_version, command, binding, status, event, candidate`；确认的含义是“反馈、失效与新候选已记录”，不是新说法已提升、认知质量已验证或授权已改变。

原子写入对这批封闭的失效/重评/无正文回执事实，须解除自动聚合前驱产生的正文派生依赖；显式因果与证据依赖保留，反馈观察/记忆/新候选/冲突事实仍依赖可删除正文。该例外只在受限本地纠正事务内按已验证事件类型应用，不是通用的血缘删除 API，不能给任意事件或文本贴“审计”标签以保留内容。

页面先让 owner 审阅原对象、影响范围和纠正文本，再显式提交。处理中/未确认时固定原 key/body，注销清草稿；刷新后从认证的主脑只读回执恢复原操作，不能从浏览器持久层重建事实。原文/纠正内容只按已有授权正文规则读取。新候选仍需正式 governor 的证据、冲突和谨慎不对称检查；确认 owner 反馈不伪造多时点的结果证据。纠正不删除历史，永久遗忘继续走 §5.6.2。

验收覆盖候选与稳定对象、原派生链立即失效、纠正后候选未提升、文本注入仅为数据、跨 owner/伪造来源/陈旧页面/固定层拒绝、CAS 与事务中故障零半提交、同号重启核实/异义冲突、加密落盘/真实生命周期清理，以及 Web→bridge→core 和浏览器交互；未取到对应层级证据不得声称完整 IP20/IP24 通过。

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

装配诊断使用 prd/25 的只读 doctor 映射，展示原始状态类别及安全原因；ready 表示当前配置已装配，不表示 provider、MCP、消息渠道或 OS 沙箱已通过实时连接/执行验证。未配置、平台不支持和阻断分开显示；无实现的渠道不合成 ready/开关。读取失败、未知 schema/状态或 core 离线时不得把旧快照显示为当前正常；刷新只查询，不触发 tick、模型实测、安装或重启。设置页使用 ProductConfigView/Patch/Response 的 schema 2 外形，脑内层对象仍保持各自 schema 1；不以只读诊断代替后续渠道/能力/网络配置事务。

首笔受管能力编辑按 prd/25 §11.7 接入 skills：与模型/工作空间/角色共用一个保存表单、配置版本和原操作号；启用和 owner 明确信任分别选择，默认不可信/禁用。列表正文只存在受认证页面内存，不进入 localStorage、URL、console 或浏览器恢复队列。编辑与移除只改变待保存草稿；保存结果不明时锁定整份原 body，显式核实同一操作，不能只重发技能子集。刷新以 Host 当前 revision 为准，已保存/待重启/核心已确认分开；原 scope 与版本可见。未接入的 MCP/plugin/package、消息渠道或隔离 backend 不提供假启用开关，本笔不关闭这些剩余设计。

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

设置页显式协商 prd/25 §11.10 的 schema 2；模型/skills 与 LAN 保存、应用及确认仍按各自事务区分。渠道、MCP、plugin/package 和代理没有真实受管消费者时显示服务端给出的不可用状态及原因，不提供假保存/启用按钮。旧 v1 客户端与新页面共用同一配置写者，版本切换不能清空已有值。

网络应用按 prd/25 §11.11 展示原号检查结果。`Activated` 只表示 Host 已核实目标 listener；`RolledBack` 明确显示目标未生效、已回到经验证的本机入口，并保留待应用设置。`RecoveryRequired`/断线只查询原 operation，不自动重新提交。浏览器证书信任及 LAN 可达仍单独展示，不因 listener 签名或服务 Running 自报通过。

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

普通建议恢复至少覆盖：identity DB 写入先于 core dispatch；同 owner/同 binding 幂等读取；异 owner/异 binding 冲突零转发；Web 重启后读取到字节/字段等价的原命令；Unknown/断线/超时保留；明确终态回执后才删除；终态已在 core 但 Web 删除/响应中断时可安全核实；没有当前 inbox item 时仍可找到原操作；页面加载不触发 mutation；整个流程 driver/event 计数证明无新 command key、无重复建议副作用。

项目监管覆盖：旧 ledger digest/重放兼容；非 owner/陈旧 project digest/version 零事件；同号重放与异义拒绝；双请求 CAS 仅一条改变；暂停重启后仍阻止背景规划；恢复不调用模型/driver；当前 route 不被暂停伪装成取消；背景 route 提交与晚到暂停串行。只读状态及浏览器切换 workspace 不调用模型、不生成 route；项目修订不热换原路线，未确认的规划/编辑保持原 key/body。

浏览器端验证 IP17–23：两 tab 重复提交只一个 run；流中刷新后完整；logout 后无私有缓存；审批参数变化 driver=0；恶意 Markdown/HTML/SVG/路径不能执行/越权；core crash 时设置仍可用；读流与命令独立限流。验证 Windows/macOS/Linux 支持浏览器与窄屏，键盘完成登录/对话/审批。

至少用真实安装包/真实 model/backend 完成一次从配置到对话/工具审批/项目监管/候选回流/重启续接的联合路径；测试模型只证明交互状态，不能证明 Agent 有用。生成新的产品 evidence，不重写历史报告。

## 10. 验收

五个工作面完整接入同一个大脑，页面事实均可追溯到 server projection/事件；无私有 core、假按钮、假进度或模拟成功。DeepSeek 复用边界、artifact 隔离、密钥保护、断线/重复/未知结果均有正反证据。

Ready/Degraded/Maintenance/RecoveryRequired 路径均可实际操作；IP17–23 及 IP24 逐项有结果，不把截图/结构存在升级成可用性 PASS。核心历史验收与新产品平台验收分别保留。

## 11. 原创性与合规

唯一批准的源码复用范围为 [frontend-reuse-policy](../compliance/frontend-reuse-policy.md) 的逐文件前端白名单，保留 MIT/第三方 notices；本模块的 API/reducer/业务/安全 fixture 原创。当前没有导入任何 upstream 源码，不在实际依赖清单虚报已使用。

React/TypeScript/Vite 及 renderer/component/icon 等实际依赖实施时锁版本并审计，外部图片/字体/品牌资源也需授权。禁止把 research/upstreams 加为 alias/import，禁止大范围排除 copy detector；合规与 UI bundle dependency graph 均为发布阻断门。
