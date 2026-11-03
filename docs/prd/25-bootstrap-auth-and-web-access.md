# prd/25 初始化、认证、配置与 Web 接入

## 0. 定位与承上

实现 [requirements/17](../requirements/17-installed-product-scope-decisions.md) P03–P08、P10、P15，主验 IP04–IP09、IP13、IP21–IP22，参与 IP12/IP18/IP24。架构以 [architecture/10](../architecture/10-installed-product-and-web-workbench-architecture.md) §3–6、§8 为准，canonical §10/15/24/28/29 不可回退。

**状态：正在实施；独立账号/session、loopback Web、本机证明/有限新脑 claim、产品内部认证 Runtime Ingress、模型/brain/skills 设置与 managed Web 网络事务已有组件实现。** 当前页面可在自建临时安装中完成认领、登录、设置和正式 Run，但尚未取得 IP 整体验收。旧脑/备份候选接管协议已冻结，代码与真实 IP13 仍待实施/验收；OS 口令恢复、其他能力消费者、真实外部 provider、原生安装和完整工作台仍待实施或验收。具体分支及证据以 `../acceptance/installed-product-implementation-progress.md` 为准，不能由固定 bearer 或组件通过推导完整产品就绪。本模块保持现有 Run/Control/Approval、Harness/Store trait 和 99 EventKinds。

## 1. 职责与边界

在受限 `forme-webd` 中实现 ForMe 的统一 Web 控制面：静态页面、认领、登录/session、配置控制、LAN TLS、正式 API/SSE、附件/产物授权。持久认证状态独立于 brain store，只有 Web 写者；Web 与 Brain 的进程分域是安全边界，不改变它们属于同一产品实例的事实。

认领、配置提交和服务启动与 prd/24 的 broker 对接；业务组件由 prd/26 实现。Web 不接 provider SDK、不调模型、不执行工具、不读 memory SQLite，也不安装上游服务端/插件系统。账号只支持单 owner，不能添加第二个拥有不同脑的普通用户。

## 2. 全局位置与集成契约

- gateway 实现 Web transport、认证 middleware、内部 Gateway 认证与安全 DTO adapter；protocol 保存跨进程 DTO；不新增 gateway→store/config/execution 边。
- 与 core 交互通过既有 Harness/Gateway 外观，最终事实仍由 core 写 forme-store。既有 `/v1/*` 保留内部 loopback；外部 `/api/v1/*` 的路由表是显式 allowlist，不做代理通配符。
- 与 broker 交互通过 prd/24 HostManagement；Web 只有有限 owner-maintenance 凭据，没有 AgentExecution 凭据/driver 权限。
- brain 发出的 Run/Action/Approval/Communication/DataLifecycle 等事件不变。登录、认领、配置 revision 等使用 architecture/10 §4 的管理/认证记录，不新造 EventKind。
- Web 用户的可信身份来自登录服务端上下文，不能由消息正文、OwnerId 字段或 forwarded header 构造。消息渠道仍使用既有 adapter/ParticipantProfile/授权，不因 owner 登录 Web 就把外部参与者升级成 owner。

## 3. 数据结构与类型

所有 wire/persistent 对象带 `schema_version: SchemaVersion`；secret input 是仅请求生命周期的敏感类型，禁止 Debug/Serialize 到日志、事件或持久审计。

| 对象 | 字段/约束 |
|---|---|
| `OwnerAccount` | `account_id, owner_id, login_name, password_hash, credential_revision, auth_epoch, created_at`。password hash 仅认证库内部，API 无此字段。OwnerId 在改名/改密码/恢复后不变。 |
| `LoginSession` | `session_digest, account_ref, owner_ref, issued_at, last_seen_at, idle_expires_at, absolute_expires_at, auth_epoch, csrf_digest, reauth_binding?`。随机 cookie token 不落库。 |
| `SetupSession` | `setup_digest, install_ref, claim_operation, expires_at, csrf_digest, verified_data_candidate_ref?`。15 分钟，仅初始化范围，完成认领后消耗。 |
| `ClaimRequest` | `install_id_safe_ref, verified_data_candidate_ref?, login_name, password, operation_id, idempotency_key`，由 setup session 认证。不允许调用者任意指定 OwnerId；已有脑按验证结果绑定。 |
| `ClaimTransaction` | `operation_id, install_ref, claim_digest, stage, reserved_owner_ref, account_ref, expected_revision`。跨 broker/Web 认领事务可恢复，原证明只用其摘要。 |
| `BootstrapCandidateCatalogV2` | `schema_version:2, verification_epoch, candidates[]`；候选仅含 opaque ref/kind/status/digest/owner source/binding digest/issue ref，不含路径、原文、key 或凭据。 |
| `BootstrapCandidateSelectionV2` | `session, candidate_ref, expected_candidate_digest`；Host 重新读取固定候选并核对 digest/epoch/owner/store/schema/锁，不能由浏览器提交路径或 owner。 |
| `BootstrapClaimRequestV2` | `session, candidate_ref, expected_candidate_digest, account_commitment`；candidate kind、OwnerId 和数据 binding 来自 Host 已验证选择，不接受调用者自报。 |
| `BootstrapClaimCommitV2` | `claim receipt, owner binding, active data binding, candidate digest, migration outcome`；`Committed` 与 `Activated` 分开，前者不能表示数据已切换。 |
| `CoreSessionAssertion` | `install_ref, owner_id, session_digest, auth_epoch, audience, request_digest, expires_at, nonce, signature`。服务端签发/验证，不暴露浏览器。 |
| `ProductConfigSnapshot` | `revision, owner_binding_ref, model_profiles, channel_configs, workspace_refs, capability_options, proxy_config, network_config, effective_sources, active_ack`。读取不含密钥原文。 |
| `ConfigPatch` | `operation_id, idempotency_key, expected_revision, typed_changes, secret_mutations`。未声明字段和不允许 hot change 的字段拒绝或要求 restart。 |
| `ValidationReport` | `config_revision, check_results, blocking_issues, restart_required, current_capabilities, model_test_status`。静态配置校验不能自报真实模型可用。 |
| `WebCommandReceipt` | `command_ref, status, run_ref?, operation_ref?, pending_ref?, snapshot_revision?`。状态为 accepted/existing/conflict/blocked；accepted 不是 action success。 |
| `StreamResume` | `stream_scope, opaque_cursor, snapshot_watermark, schema_version`。run events 内部排序仍只认 stream_seq。 |
| `ApiProblem` | `code, safe_message, issue_ref, retryable, retry_after?, required_action?, correlation_ref`。拒绝返回 raw error stack、secret/path。 |

`ProductConfigSnapshot` 的 provider key/channel credential/内部 Runtime Ingress secrets 只以设置状态与可安全显示的 metadata 表示。浏览器不可读的数据也不能藏在 JSON 的未展示字段中。

## 4. 接口与签名

### 4.1 外部 Web API

路径以 `/api/v1` 为根。所有 mutation 带稳定 `Idempotency-Key`，涉及版本对象带 expected revision；服务端用规范化完整语义核对，异义 key 冲突且零推进。请求 body 大小和上传能力按配置限制，不无限缓存。

`PUT /auth/password` 的 key 绑定已认证 owner/session、account ref、credential revision/auth epoch 与目标口令候选。独立认证库只暂存 Argon2id salted hash 和 operation digest，不持久化明文；有效窗口内，同 key、同绑定、同目标沿用同一候选和 digest，同 key 异义或绑定变化返回 Conflict，且不签发 reauth proof、不改口令/epoch。候选 hash 最长保留 24 小时，过期后启动或下一次口令变更会清除；最多允许 64 个未完成候选。成功提交在同一事务中更新 credential、递增 epoch、撤销会话并清除该 owner 的所有候选 hash。操作 key 保留为 tombstone，不能被后续新意图复用；浏览器遇到结果未知时锁定表单并使用原 key/原字段继续核实；收到明确失败后，用户可以发起带新 key 的独立尝试，服务端不改绑旧 key。

| 路由 | 请求与结果 | 身份/语义 |
|---|---|---|
| `GET /bootstrap/status` | 仅 claimed/setup_needed/protocol_version。 | 公开最小信息；不返回 owner/path/model。 |
| `POST /bootstrap/prove` | 本机一次性证明 → 有限 setup cookie。 | 原子预留 claim；并发/重放不能创建第二个初始化者。 |
| `GET/POST /bootstrap/data` | schema 1 新脑视图；schema 2 返回固定路径发现的三类候选并准备指定候选。 | 只接受有效 setup cookie + CSRF；Host 验证 owner/key/完整性/锁/保护状态，不允许匿名读取、任意文件路径或调用者自报验证结果。 |
| `POST /bootstrap/claim` | ClaimRequest → claim receipt/owner session。 | 有限 setup session + 已验证数据 ref + CAS；消耗证明/初始化会话。 |
| `POST /auth/login` | login/password → cookie + safe session view。 | 验证密码；登录也校验 Host/Origin 和 login CSRF。 |
| `GET /auth/login-challenge` | 有效 5 分钟的 HttpOnly login cookie、login CSRF 和 attempt ref。 | 只为同 origin 的登录表单建立有限挑战；不授予 owner/setup 权限。 |
| `GET /auth/session` | safe owner/account、expiry、csrf nonce。 | 有效 cookie；不返回内部 assertion。 |
| `POST /auth/logout` | 撤销当前会话。 | CSRF；不会取消已授权后台任务。 |
| `DELETE /auth/sessions` | 撤销全部会话并升 auth epoch。 | 近期重新认证；不更换 OwnerId/keys。 |
| `POST /auth/reauth` | password + operation digest → 绑定的短时 proof。 | 仅 5 分钟且单 operation 使用。 |
| `PUT /auth/password` | old/reauth + new password。 | 原子 hash/epoch 更新，旧会话撤销。 |
| `GET /product/status` | HostStatus + safe health。 | 登录；core 离线也可用。 |
| `GET/PATCH /product/config` | safe snapshot / ConfigPatch → validation/operation。 | CAS，secret 只写；apply/ack 独立。 |
| `POST /product/config/validate` | 指定 revision → ValidationReport。 | 不把格式检查等同于模型实测。 |
| `POST /product/commands` | typed MaintenanceCommand → OperationRef。 | 必需的 reauth + broker allowlist；不是 shell。 |
| `GET /product/operations/{id}` | 安全进度/receipt。 | 按 product cursor 重连，不读取 brain journal。 |
| `GET /brain/runtime/doctor` | 原 `/v1/runtime/doctor` 的版本化安全投影（checks、blocking、digest）。 | owner session 经内部认证 Runtime Ingress 只读同一 ProductionRuntime；无参数、无模型/工具调用，不推进脑事件。仅此 GET 映射，禁止 tick、reload 或任意 runtime 代理。 |
| `GET/POST /brain/runs`、`GET /brain/runs/{id}` | 正式 run list/submit/read。 | owner assertion；同一 ProductionRuntime。 |
| `GET /brain/runs/{id}/events` | 正式事件 SSE。 | run scope + cursor，heartbeat/重连；无隐藏 CoT。 |
| `POST /brain/runs/{id}/control` | 原有 typed cancel/resume/approval union。 | exact pending binding；不允许通用 retry unknown。 |
| `/brain/{sessions,workspaces,inbox,candidates,memory,settings}` | 逐项映射已有 read/command 外观。 | 缺少 core 外观时先做正式 adapter；不能临时直读 DB。 |
| `POST /brain/uploads` | 有限附件流 → scoped content ref。 | core content store/AEAD 路径；未确认附件不成为指令。 |
| `GET /artifacts/{opaque_ref}` | 授权内容/下载。 | owner/run/workspace scope；拒绝任意路径。 |

`/brain/*` 不是字符串转发；每一路由明确 input schema、command type、auth、版本和响应 projection。只有 static assets 与最小 bootstrap/login 路由允许未登录；public health 不暴露内部 issue 细节。API 客户端版本不兼容返回明确 UnsupportedVersion，而非丢弃未知安全字段继续。

### 4.2 内部接口

```rust
trait WebIdentityService {
    fn login(&self, request: SensitiveLoginRequest) -> Result<SessionCookie>;
    fn authenticate(&self, cookie: &SensitiveCookie,
                    request: WebRequestBinding) -> Result<AuthenticatedWebOwner>;
    fn revoke(&self, owner: &AuthenticatedWebOwner,
              command: RevokeSessions) -> Result<AuthRevision>;
}
// CoreWebBridge is the existing internal transport seam, not a second product boundary.
trait CoreWebBridge {
    fn submit(&self, owner: &AuthenticatedWebOwner,
              command: BoundBrainCommand) -> Result<WebCommandReceipt>;
    fn snapshot(&self, owner: &AuthenticatedWebOwner,
                query: ScopedBrainQuery) -> Result<SafeProjection>;
    fn stream(&self, owner: &AuthenticatedWebOwner,
              query: StreamResume) -> Result<EventSubscription>;
}
```

`AuthenticatedWebOwner` 只能由认证 middleware 创建。内部 Runtime Ingress 为每次 mutation 生成 audience/request/session-bound 短时 assertion，内部 Gateway 重新验证服务身份与当前 session/epoch；必要的 introspection 走私有认证 IPC。注销后不能只等旧 token 到期才停止接受新 mutation，core 无法核实有效会话时拒绝。读长连接也周期确认会话，失效后结束连接，但已授权 run 不随之取消。

### 4.3 Loopback Runtime Ingress 实施约束

- Web 与 CLI 使用不同内部凭据；安装身份、验证公钥和独立认证 fence 路径必须一并配置，缺项阻断启动。assertion 绑定实际 method、path/query、完整 body 摘要、owner、session 摘要、epoch 和 nonce，有效期最长 30 秒；内部凭据/签名材料不进入浏览器。
- core 在签名、请求摘要和安装/owner 校验后，原子检查 epoch 并消费 nonce，再进入原 Gateway。nonce 去重与会话/全局撤销下界保存在独立的、OS 单写者锁定的认证 SQLite 中，使用 FULL 同步事务；不写脑库/脑事件。重启不得清除仍有效的 nonce 或撤销下界；格式/身份/完整性不明拒绝恢复，不自动重建。
- 注销/全会话撤销/改密码先验证当前身份与 exact operation proof，再在**同一认证 SQLite 事务**中提交本地撤销与 core fence outbox；无效 reauth 不改变本地 epoch、口令或 core fence。core 离线不阻止本地撤销。仅当 outbox 的全部相关 fence 得到 core 持久确认后报告双方已同步；尚未确认时返回 202，明确本地已生效、core 同步待完成，并清除浏览器 cookie。
- 每次代理工作保留 admission/capacity guard 至真实 IO 结束；HTTP 超时或客户端断连不释放仍在工作的配额。只开放逐项允许的 brain 路由，不接受 URL proxy、浏览器指定 principal 或任意维护操作。

#### 4.3.1 离线撤销与恢复

- outbox 属于独立认证库，使用已验证旧格式的原子 `user_version=3 -> 4` 迁移。记录仅含版本、安装/owner 绑定、源会话摘要/epoch、目标会话范围和单调下界；无 cookie、口令、hash、provider key 或可执行脑命令。认证库损坏、未知版本或绑定不符仍拒绝，不重建。
- 本地撤销与签发/提交内部请求共用有界 transport 配额下的串行控制门。已提前完成准入的请求可以完成；撤销提交之后，任何持有旧 `AuthenticatedWebOwner` 的排队请求都要重查真实会话，并在签名/IO 前失败。门不能仅保护 HTTP handler 的短暂生命周期。
- Web 启动、周期维护和每次正式脑请求之前，先按原记录重送**仅减权**的 fence。ack 丢失时原记录保留；重启重发使用新 nonce，core 仅在 fence 专用路由允许被撤销的源会话提交相同或更严格的下界，不能用该恢复证明执行脑命令。收到 core 持久确认才原子移除原记录。
- 全局与单会话 fence 均只增不减，乱序或重复递交不复活旧授权。待同步记录未清空时不向 core 发送新的脑请求；登录、状态和受认证的独立宿主维护继续可用。不存在 core 配置时保留记录，不伪造远端确认；后续接通仍先同步。
- 原子性验收覆盖：core 离线注销/改密码、提交后重启、ack 丢失、同记录重复/乱序同步、无效 proof 零变化、旧内存身份/已签旧 assertion 拒绝、新 session 先同步后提交，以及 logout 不取消已授权任务。此组件不替代原生安装或 IP06/IP24 联合验收。

### 4.4 窄维护外观与原操作核实

`GET/POST /api/v1/product/maintenance` 与 `GET /api/v1/product/maintenance/operations/{id}` 使用已认证 session；写请求还需 Origin/CSRF、稳定 operation id 与针对服务端重建 digest 的单次密码重认证。浏览器仅使用既有 closed `ProductMaintenanceAction`：StartCore、StopCore、RestartCore、StageSignedRelease、Upgrade、RollbackCompatibleRelease、CommitRestore 和保留数据的 UninstallPlan，且只展示 Host 当前 `allowed_operations` 所声明的动作；没有 arbitrary MaintenanceKind、路径、程序、SQL 或恢复裁决参数。owner 取自已验证会话，Host 再与产品配置绑定核对；读取不授予操作权。

该外观经 Configuration 域的固定路由转发，不给 Web 发 maintenance bearer，也不让 Configuration token 访问通用管理/AgentExecution 路由。Host 在内部构造原 MaintenanceCommand，复用同一 ProductJournal/CAS/intent/receipt。快照带独立 `product_revision`、实际 HostStatus、owner 绑定与最多 25 条最近 operation；读取时版本变化返回 conflict，不能把配置 revision 当产品 revision。

Start/Restart 先验证当前绑定配置，StageSignedRelease 只有独立 release worker 已配置且预检通过才可执行；Upgrade/RollbackCompatibleRelease 另要求对应 updater/data worker 就绪，并绑定原 release id/digest 和 opaque backup ref，按 PRD24 §4.10 的排空、备份、切换、健康核验及兼容边界执行；CommitRestore 沿已核验候选和原配置 revision。卸载按钮只生成范围计划，不删除数据。丢响应时页面保留原 operation id，只查询该操作，不能自动再次派发；pending/RecoveryRequired 继续显示原阶段。主脑离线时认证和这些受限维护仍可用。缺少完整 worker 时不显示可执行入口，不在此借通用代理放开。

### 4.4.1 升级中断的有限恢复入口（owner 已批准）

独立新增 `GET /api/v1/product/release-recovery/candidates/{original_id}`、`POST /api/v1/product/release-recovery`、`GET /api/v1/product/release-recovery/operations/{recovery_id}`、`POST /api/v1/product/release-recovery/verify`。对应内部 Configuration 域将 `/api/v1/product` 替换为 `/v1/host/configuration`，不加入 OwnerMaintenance/AgentExecution 域。DTO 使用 PRD24 §4.10.5；写 body 为相应 request 的无 owner 字段外形加 `password`，owner 由服务器认证会话重建。写请求须 Origin/CSRF/idempotency-key=operation_id、操作摘要绑定的单次密码重认证，再由 Host 验证实际 owner；verify 具有独立 domain digest，不能复用初始恢复授权。

另提供认证只读 `GET /api/v1/product/release-recovery/operations`（内部 Configuration 域同前），返回 `ProductReleaseRecoveryList`，从同一 journal 快照取得当前 revision 和最近 25 项原恢复记录。它不读取数据文件、启动 worker 或执行恢复；用于页面刷新/重新登录后找回原 intent，不能因为页面内存消失生成新恢复号。

首次恢复已落 intent 后断线/重启，重复原 POST 只查；查询不提交或结束恢复。仅用户显式 verify 原号/原 request_digest/当前 revision 才能继续只读核实并记 receipt，失败保留 intent。不存在/错 owner/未知 schema/不同 digest/CAS 均拒绝；配置 token 不得接触通用管理/worker 路由。普通维护 body 仍不接受恢复参数，UI 不用新增入口执行任意 stage/retry/shell。

## 5. 行为、状态与并发

### 5.1 认领与口令恢复

认领创建的唯一网页登录账户固定为产品 `SuperAdmin/Owner`。当前不支持普通 Web 用户、角色切换或多租户；该账户可以管理本产品控制面的全部设置、项目、任务、主脑生命周期、维护、备份恢复和数据。高风险操作要求的近期密码重新认证是 operation 绑定，不是第二个角色，也不能提升 Agent 的 trust、permission、envelope 或 OS 身份。

未认领服务只有最小公开状态和证明交换；proof 的生成、期限和交付按 architecture/10 §3。prove 先 broker CAS 预留 installation，换取有限 setup session；在其 scope 内验证新脑/原脑/恢复候选后，claim 才绑定正确 OwnerId、以 Web transaction 创建账号 hash 并 broker commit。中断恢复根据双方 operation refs 核对；pending claim 只能由原 setup session/本机管理身份恢复或显式撤销，不能让另一个访问者补填密码夺取账户。

已有脑/备份的 owner 来自解密与完整性验证，不能先随机创建 owner 再修改它适配旧数据。认领完成或本机撤销/轮换证明使 setup session 无效。已认领账号恢复不能导入另一个 owner 的脑；owner 不匹配应拒绝，而不是改写账户/数据身份。

首次认领候选固定沿 [首次认领旧脑与备份候选协议](../architecture/proposals/installed-bootstrap-legacy-brain-claim.md)：`NewBrainTarget` 继续使用 schema 1 兼容路径；`ExistingBrain` 与 `VerifiedBackup` 必须使用 schema 2 catalog/selection/claim。候选发现只能来自安装器 manifest、Host 配置和固定受保护布局，浏览器不能提交路径、URL、owner、key 或“已验证”布尔值。历史明文、错误 key/owner、未知 schema、损坏、链接/别名或独占锁不明的候选只能显示为 rejected/unverifiable，不能为了完成初始化而回退为空新脑。

候选选择先记录 candidate ref/kind/digest、verification epoch 和 owner source；数据接管再在新的空目标上执行有界复制/恢复，保留原件。产品 claim journal v2 依次区分 `CandidateInventoryObserved`、`CandidateSelectedV2`、`ClaimDataPrepared`、`ClaimCommittedV2`、`ClaimActivatedV2` 和 `ClaimAbortedV2`；旧 v1 journal 字节/hash 保留，旧 reader/writer 在首个 v2 fact 后必须拒绝。`ClaimCommittedV2` 不等于 active data 已切换，只有 Host 完成 active data/config CAS、旧 authority fence 和 ack 核对后才可 `Activated`。准备阶段之后的未知副作用、进程重启和响应丢失只允许核实原 operation/receipt，不能换候选、换 owner、重派或启动 Brain。

密码规则、Argon2id 和 TTL 统一引用 architecture/10 §5.1。改密码/本地恢复先写新 credential revision 与 revoke epoch，再提交最终状态；崩溃后不得重新激活旧口令。忘记口令通过本机管理员 `hostctl` 发起有限 reset 操作，记录事实、撤销会话，不建立匿名远程找回或重置 OwnerId。

本机恢复在读取配置、stdin 口令或认证库前核验当前进程的操作系统权限：Windows 读取真实进程提升 token，Unix 读取 effective UID。不得运行 PATH 中的 whoami/id、解析本地化组文案，或因 deny-only 管理组 SID 而认定已提权。检查失败即拒绝；此权限只允许有限恢复，不提升 Agent 的行动授权。

改密码的 Idempotency-Key 按 §4.1 绑定目标和当前认证版本。未知结果只允许原操作重试；提交后旧 session 必须失效，用户以新口令重新登录确认，不通过旧 cookie 查询或重复设置第二个目标口令。候选哈希在事务提交时清除；过期候选只能得到 Conflict，不能以相同 key 创建新的目标。

### 5.2 配置与启动

有效配置保存是启动主脑的自动触发点：Host 在同一已认证配置域完成 CAS/journal 提交后，自动选择 `Start` 或 `Restart` 并执行既有受控服务事务。浏览器只等待/查询真实产品回执，不再为正常配置启用再次提交密码；配置保存与 core acknowledgement 仍分开，只有 core 返回对应 revision 的正式 acknowledgement 才显示“已生效”。Host 启动和系统重启重复执行同一确定性 reconciliation；无配置、未安装、RecoveryRequired、Unknown 或状态不明时不盲重试，返回可诊断状态，手动启动只作维护入口。

初始化顺序：本机证明/有限 setup → 验证/选择脑数据 → 完成账号认领 → 配置模型/key/proxy → workspace/能力 → 静态 doctor → 受控启动 → runtime readiness → 可选正式模型连接测试。doctor 分清格式、密钥存在、网络可达、真实模型工具兼容；未执行的测试显示未验证。

保存通过 broker 版本事务：校验 patch/身份 → staging+secret resolver 写入 → validate → expected revision CAS → commit config → 通知 core 应用或排空重启 → 对照 actual composition ack。保存成功不是应用成功；失败保留 previous known-good，显示待修复字段且不把密钥回填页面。

服务配置优先规则按 architecture/10 §5.2。模型测试若需真实调用，只由正式 core/runtime 发起并记实际 profile/结果；Web 与 broker 不另建一个可直调 provider 的测试通道。错误 key 不进入无限重启；core 可进入 Degraded/ConfigurationRequired，Web 继续可达。

### 5.3 身份与审批

请求到达先 Host/Origin/CSRF/session/epoch 检查，再路由权限，再生成内部可信 AuthContext。浏览器 body 不接受 principal/approver 的自我声明；批准人由认证 owner 绑定。

批准操作只提交 pending approval ref、选择与预期 binding，服务端读取最终 plan 和规范化摘要；显示与执行的 affected_resources、schema/policy、nonce/expiry 必须相同。页面过期、plan 改变、能力撤销、切换 owner/epoch、已消费 nonce 均返回明确 conflict/expired，不能自动重新批准。L5 沿用 one-shot，不提供“此网页内总是允许”。

Unknown 呈现原 job/receipt/probe/裁决路径，不包装成普通重试按钮；取消请求只有在 backend 确认后才显示取消成功。control 与 resume 接收现有 PendingKind 对应 typed input。

### 5.4 SSE、断线与请求幂等

run SSE 依 `stream_seq` 恢复；收件箱/多 run 列表使用服务端 opaque projection cursor。snapshot 与 subscription 需共享 watermark，gap/retention 使客户端重取，不以时间过滤跳过事件。慢消费者有界队列与可恢复断开，不阻塞 core writer。

提交超时保留幂等 key/command ref；重连先查询已有 receipt，再同 key 重试，不能创建第二任务。只有未提交草稿可以本地持久化，已经接受的任务不在浏览器离线继续执行。

### 5.5 本地/LAN 与维护

默认 loopback:18765；LAN 变更先预检绑定地址、Host allowlist、证书私钥、HTTPS、firewall 范围，再写操作 intent，切换并自检。原连接失效时提供已验证新地址和恢复说明；失败回到可达的 loopback 修复状态，不能全接口裸 HTTP 兜底。

Web 传输配置使用可缺省的受管 `tls` 对象（`schema_version, material_ref, material_configured`）。公共设置 v2 只允许浏览器提交固定的 `material_ref=managed:web`，只读取 `material_configured` 状态，绝不上传、读取或指定证书/私钥路径；Host/安装管理域负责解析受保护材料并拒绝链接、超限文件、无效 PEM 和不匹配的密钥。未配置时只接受 loopback HTTP；配置后整个 listener 使用 TLS 1.2/1.3，不在同一端口嗅探/降级明文。Host 白名单为有限、去重、含实际端口的精确 authority，拒绝通配、userinfo、模糊别名和代理头；mutation Origin 必须与实际 Host 的 HTTPS origin 一致。TLS 下所有认证 cookie 加 Secure，仍保持 HttpOnly、SameSite=Strict、host-only；不因 TLS 取消 CSRF 检查。握手并发、握手时间、已接受连接总数和连接存活时间均有界；慢握手不阻塞其他连接。成功加载证书不等于客户端信任、网络可达或防火墙配置成功；配置事务和 IP12 仍须分别验收。

管理 RPC 必须验证 OS peer/服务身份。core 不可达时，Web 可以显示状态、改配置、检查指定备份并请求受控恢复；不能在 Web 进程自己打开脑数据库补读。离线完整性/恢复委托获得 exclusive lock 的既有维护路径。

## 6. 错误、安全、权限与审批

API 错误包括 `Unauthenticated / Forbidden / CsrfRejected / ClaimUnavailable / Conflict / PendingRestart / CoreUnavailable / Maintenance / CertificateUntrusted / RateLimited / UnsupportedVersion / RecoveryRequired`。HTTP 202 表示 accepted，409 表示陈旧版本/异义 key，429 带 Retry-After；不能把 200/202 作为动作完成证据。

登录/claim/reauth 均严格 body/速率限制，Argon2 工作队列有界。登录响应不暴露账号是否存在；不把登录失败原因写成含原口令的 stack。CSRF nonce 不放 URL；SameSite 不能替代 Origin/Host 检查。对 loopback 也防 DNS rebinding，拒绝未知 Host 与任意 CORS。

附件/Markdown/工具输出都为不可信内容；CSP、HTML 清洗、链接协议与 sandbox 由 prd/26 执行。下载按安全 ref/实际资源授权，拒绝路径穿越、符号链接替换、错误 MIME/script 执行；上传有限流/限量/临时文件保留与 crypto lifecycle。

所有错误区分宿主故障与 Agent FailureTaxonomy；只有经过 core 的失败才写其 FailureEvidence。provider keys、密码/session、helper credential、数据 key 不进入公共 DTO、localStorage、事件、日志、trace、诊断或分析 telemetry。

## 7. 配置与可观测

- `web.{bind,port,allowed_hosts,trusted_proxies,tls}`；初始端口 18765，LAN 必需 TLS。
- `auth.{idle_ttl,absolute_ttl,reauth_ttl,password_hash_parameters,login_rate,argon_concurrency}`；安全默认取 architecture/10。
- `transport.{read_rate,command_rate,event_connections,event_queue_limit,reconnect_policy,upload_limits}`；读/命令/登录独立 bucket。
- `service_config.{source_mode,revision,active_ack}`；保存、有效来源和需要重启均可见。

记录安全 request/operation refs、认证结果码、速率 bucket、断线/背压/gap 次数、core 状态和 config ack；不记录 cookie/authorization/body 秘密。日志 logout/reset 可核对，但不暴露原始 session 摘要作为可用 credential。

## 8. 实施步骤与顺序

1. 固定 DTO/route allowlist/错误码；与 prd/24 对齐 claim/config/operation 事务。
2. Web 独立启动与安全静态资源；无模型配置也能看到 bootstrap，公开面最小化。
3. 认领、Argon2id/session/CSRF/epoch/本机恢复；先做抢占、伪造与并发否定测试。先实现 schema 2 候选 catalog/selection、固定路径只读验证、claim journal v2 与旧 v1 兼容，再接入数据准备/active binding；不复用普通 `CommitRestore` 冒充首次接管。
4. 持久 typed config + secret resolver + effective source + doctor/start/ack，不依赖用户手工 env。
5. 内部认证 Runtime Ingress 与正式 Gateway 协议；验证 owner/approver 不可被 payload 替换。
6. LAN HTTPS/firewall/Host/证书信任；SSE/backpressure/idempotency 和附件授权。
7. 与 prd/26 五个工作面联调，完成主验 IP 和 IP24；前端 stub 不代替本模块后端完成。

## 9. 测试

单元：密码参数/上限、session expiry/epoch、CSRF/Host、claim CAS、config transaction/recovery、assertion audience/digest/nonce、异义 idempotency、stream watermark。fixture 自建，不引用真实账号/key。

集成：两浏览器并发 claim、断电阶段恢复、logout 后 mutation、伪造 owner/审批、core 停止/错误配置/key、环境覆盖、LAN 未信任证书、错 Host、重复提交、SSE gap、读流限流与命令不互相饿死。候选专项还必须覆盖三类候选、错误 owner/key、历史明文、损坏/未知 schema、数据库占用、candidate digest TOCTOU、旧 reader 对 v2 拒绝、数据准备各阶段中断、响应丢失原号恢复、已认领拒绝以及“零空脑替代”。批准反向场景验证 driver=0，而不只看 HTTP 拒绝。

真实浏览器验证 cookie/CSRF/续接；原生平台验证账户/key 权限和网络规则；至少一次真实 model profile/工具调用通过既有 core 全链路。使用 IP04–09/IP13/IP21–22 的完整正反断言；跨平台未跑不标 PASS。

## 10. 验收

清洁安装无 env 即可认领、设账号和模型并启动原有大脑；重启/改密码保留脑/OwnerId，core 离线仍可受认证修复。首次认领能够在不接收浏览器路径/owner 的前提下发现、验证并显式选择新脑、已有脑或备份候选；错误/明文/占用/损坏候选不创建替代空脑，数据接管只有在 `Activated` 事实后才显示完成。LAN 无匿名/裸密钥，内部 Gateway 不被公网化，所有 control 绑定最终 plan 且幂等。

网页保存配置后能证明 actual active revision/provider；读流不会压垮命令配额。主验 IP 有真实证据，历史 governance/regression 不退化；本模块没有独立 model/backend/memory writer。

## 11. 原创性与合规

认证、密码哈希、TLS、cookie、CSRF 和 IPC 使用成熟公开协议/安全库，不自行设计密码学。新增依赖锁版本、来源、实际许可、transitive notice/漏洞/SBOM，实施时更新清单；本次文档不表示已引入。

不从 DeepSeek/OpenClaw/Hermes 复制后端认证、消息、RPC 或配置系统。前端 UI 的受限复用只按 [frontend-reuse-policy](../compliance/frontend-reuse-policy.md)，不能扩大到本模块 core/安全实现；合规门继续 fail closed。

### 11.1 实施细化：认证持久化与认领接缝

认证实现放在 gateway 的 `web::identity`，仅写独立 `web-auth/identity.sqlite`，使用独立 OS writer 锁、SQLite FULL 事务与 application/schema/installation binding。未知、旧版本、其他用途或损坏的文件拒绝接管，不初始化覆盖。此处复用 rusqlite 的公开事务 API，不新增 gateway→store 边，也不打开 brain SQLite。目录 ACL 由安装器负责，模块内的路径检查不自报已验证 OS 权限。

单 owner 创建分为 Web 准备账号承诺（随机 salt 的 Argon2id 记录）和消费 broker 的已提交认领回执。新增 `WebClaimCommit / SignedWebClaimCommit` 产品 DTO；签名绑定 install、operation、已验证 candidate、真实 owner、账号承诺和期限。验签只使用受管预置公钥，不接受浏览器带来的公钥；未接通 broker 的有限 setup/数据验证/commit 前，不开放匿名注册或把此组件当成 IP05 完成。断线可用原 operation 和原承诺幂等收尾，不生成另一 owner。

密码 KDF 固定使用 Argon2id v19、64 MiB、3 iterations、parallelism 1、16-byte OS salt 和 32-byte 输出，持久记录显式保存 schema/算法/参数；不导入任意参数执行。密码工作最多并行两个，正在运行的计算持有名额到真正结束。密码/原始 cookie 类型不派生 Debug/Serialize 并在释放时清零；会话和 reauth 原文只交 transport，数据库仅保存摘要。

会话默认 idle 30 分钟、absolute 12 小时；CSRF nonce 从随机 HttpOnly session credential 以独立域摘要派生，可在刷新后返回，不存 plaintext cookie。任何 mutation 仍需 transport 的精确 Host/Origin/CSRF 和当前 epoch 重查。5 分钟 reauth 必须绑定具体 operation；改密码的 operation 绑定预生成新哈希承诺，全会话撤销使用不同的操作域，不能互换或替换新口令。账号/epoch/一次性 proof 消耗与会话撤销在同一事务完成。

这些是原有 §3–5 的实现细节，不更改既有 Harness/Store/WebIdentityService 语义，不增脑事件。组件证据与 Web HTTP/浏览器、broker 双边恢复和原生安装 IP 证据分别记录。

### 11.2 实施细化：独立 HTTP 入口与登录挑战

`forme-webd` 是 gateway 内的独立 Rust bin，不构建 ProductionRuntime，不依赖模型/owner 的 shell env；从受保护的绝对路径 Web service config 读取 installation、认证目录、broker 认领公钥及监听参数。未认领只返回最小 bootstrap 状态；尚未接通的 setup 或 core 能力明确不可用，不开放临时注册或裸 bearer 页面。

初始 HTTP adapter 只接受精确 loopback 地址和配置 origin；未实现 TLS 的版本拒绝 LAN 配置，不以 HTTP 兜底。所有请求先核对唯一 Host、Origin/Fetch Metadata 和默认拒绝的 forwarded headers；mutation 强制 exact Origin 与 CSRF。static/API 均有 CSP/no-sniff/no-store/拒绝 framing 等响应头，不设置宽泛 CORS。登录、已认证 mutation、读取各用独立有界 bucket；昂贵认证在有界 blocking 工作中运行，超时/取消后仍占用原名额直至真实结束。

登录挑战用 256-bit OS 随机 HttpOnly cookie，服务端只存摘要、期限和消费结果。返回的 attempt ref 作为该表单的 Idempotency-Key，CSRF 以独立域绑定。登录校验挑战、Origin/CSRF、密码和当前 credential revision 后，在同一事务创建 session 并绑定原 attempt；重复请求只能重取原会话（须已持其 cookie），或返回明确 Conflict/Expired，不再创建 session。丢失响应及 cookie 时用户重新发起一个新挑战；不在数据库保存可还原 session 原文来换取透明重试。过期挑战可有界清理，旧证明仍不可重放。

认证 schema 的已知版本升级必须在 writer 锁内事务迁移，未知/损坏版本仍拒绝，不能通过重建认证库解决。该 HTTP 组件不自报原生 ACL、broker 认领、LAN 信任、core assertion 或 IP 完整验收；接入范围与缺项逐批记在实施记录。

### 11.3 实施细化：本机证明与 broker 认领权威

broker 的认领权威使用独立、OS 排他锁保护的 append journal；记录绑定 installation 和受管签名公钥，只有摘要、期限、操作与已验证数据引用，不含原始 proof/setup cookie/密码/私钥。记录有单调局部序号、前序摘要和 schema，先 sync 再推进投影；损坏、截断、不匹配或写入结果不明均阻断，不截断重建。它是产品管理事实，不是新大脑或脑 EventKind。

本机管理员可轮换 30 分钟的一次性证明；原文仅在该受认证管理请求的响应中交给本机调用者。Web 使用另一个限定 bootstrap 的服务凭据，仅能交换证明、查询/选择注册的数据候选和提交原认领；不能用它调普通维护或 AgentExecution。初始适配器仍为私有 loopback HTTP，原生 peer/ACL 验证与平台安装验收分开记录，不把 bearer 本身视为 OS 管理权证明。

交换证明必须把同一 installation 原子预留给一个 claim operation 和 Web 预生成的 setup 摘要，固定 15 分钟期限；相同请求只返回原 reservation，不延长 TTL。第二个 operation、另一 setup 摘要或证明重放均不能占用。过期 reservation 不自动交给另一个浏览器，需本机管理身份轮换；认领已提交后不能再轮换或换 owner。

数据候选只来自受保护的注册目录/离线验证器，不接受浏览器路径或 OwnerId。新脑候选必须确认精确目标为空、没有链接/间接跳转，选择时分配一次 owner，提交前重查。目标出现旧数据、无法读取、缺 key 或没有旧脑验证适配器时显式阻断，不把它伪装为新脑。旧脑接管、备份验证与实际 core 数据初始化另行接通；候选选择本身不创建 brain store。

提交严格绑定原 operation/setup/candidate/owner/账号承诺；先持久化 committed 事实，后签发现有 SignedWebClaimCommit。响应丢失或进程重启，只允许原绑定重取回执；已提交绑定不能被新账号/新承诺替换。即使 setup 已过期，也只可重取同一 committed 结果并收尾，不恢复一般 setup 权限；尚未提交的过期操作仍拒绝。签名私钥和 Web 服务凭据从受管文件读取，缺失不自动重生；公开 Web 仅持 pinned 公钥。

对应组件测试至少包括并发抢占、跨域凭据、过期、轮换、响应丢失、重启幂等、异义承诺、数据目录变化、journal 截断/篡改/异安装与排他 writer；这些仍不是 IP05/IP13 原生全链路 PASS。

### 11.4 实施细化：受管核心启动

`forme-gatewayd --config <absolute-path>` 从版本化 `CoreServiceConfig` 加载安装身份、配置 revision、loopback transport、受管 brain 配置及 drain 时限。`ManagedBrainConfig` 明确 owner/store/workspace/profile、模型声明与各 profile 的凭据文件、scheduler、cognition 和 artifact 路径；模型声明中的历史 `credential_env` 字段仅为兼容旧模型 schema 的槽位名，此模式不读取该环境变量。模型值在 provider 绑定时从私有受限文件解析，不写 process env，也不复制进配置、manifest、事件或诊断。

`ManagedBrainConfig.content_key_directory` 是可选的绝对、无链接 vault 路径；缺省沿用 store 的 `.content-keys` 同级目录。新路径恢复可显式沿用原 vault，不能由登录口令派生或从备份中臆造 key。已存在 authority/events 的受管脑先用 existing 只读 resolver 核对原材料；vault 缺失即拒绝，不生成替代目录后假装重启成功。artifact、store 和 vault 范围不得重叠；新路径选择不是自动切换授权。

服务模式必须显式传 `--config`；前台开发保留无参数 environment 模式。Windows SCM 同样使用进程命令行中的受管路径，不从交互 shell 推断配置。路径必须绝对、无链接/reparse-point，读取有界、closed schema/version；检查失败不退回环境或创建替代脑。Web/core 仍使用同一 ProductionRuntimeBuilder 和受控停止接缝。文件结构检查不代表原生 ACL 已验证。

本节定义持久启动来源与模型 file resolver；配置 staging/CAS、secret 写入和 core ack 进一步按 §11.5 实施并独立取证。尚未接到隔离 worker 的可执行能力在受管模式中保持不可用，不得借环境启用原地 shell/MCP/browser。capability worker 接入和原生安装仍须单独实施，不能把受管启动配置称为 IP08 或完整设置页验收。

### 11.5 实施细化：配置事务与受控启动

配置写者使用独立 OS 文件锁与有界、校验链式产品 journal。每次变更先完整校验模型、版本、owner 和凭据，再写不可变 revision 文件及独立版本的凭据；不得覆盖运行中 revision 引用的密钥。journal 只保存操作/摘要/安全 revision ref，配置值与凭据路径只在受保护 revision 文件。持久 intent 后原子替换受管 core 配置，再提交 receipt；重启按原 intent 核对文件并完成发布，不创建新 revision。损坏/截断/文件不匹配阻断，不重建空配置。所有已提交 operation 的语义摘要跨重启保留，异义 key 与陈旧 revision 零推进。

`POST /product/config/validate` 只检查指定已保存 revision 的静态配置、文件与凭据存在性，明确 `model_test_status=NotRun`。`POST /product/config/activate` 是有限的 Start/RestartCore 维护适配：绑定登录 owner、operation、配置 revision 和 action，要求该操作的近期口令重新认证；只能控制内置 Brain 服务，无任意服务名、程序、路径或 AgentExecution。服务动作沿既有产品维护 journal；中断后只查询原 operation，pending/unknown 不重新启停。配置与服务操作使用同一配置锁，防止检查之后换 revision。

网页区分“保存”“静态检查”“请求启动/重启”“核心确认”，启动 OS receipt 不等于模型调用成功；已确认 revision 也不单独证明当前进程健康。模型真实测试仍只经正式 core 的 Run 路径。旧版已知配置只在 owner/模板/完整文件一致时迁移，无法核对则要求本机恢复，不能丢弃原配置。模型输入在凭据 staging/该命令 intent 之前被拒绝时，返回明确的 422/ConfigurationInvalid，允许编辑；任何发布或服务副作用不明不能映射成该错误，也不能换新 operation 自动重试。

### 11.5.1 实施细化：默认工作空间与模型角色

`ProductConfigPatch` 增加可选 typed `brain: ProductBrainPreferences`；省略表示保留原值，不能重置成安装默认值。该对象含 `schema_version=1`、`default_workspace: WorkspaceRef`、`model_routing: ProductModelRouting`。路由含 `schema_version=1` 及可空 `foreground`、`cognition` profile ref：前者为正式对话/任务模型选择的优先项，后者为后台认知相关性判断及其对照评估使用的模型。空值分别沿用既有确定性兼容选择与默认 profile，不展示尚无消费者的角色开关。现有无需模型的候选提取与维护不会因配置角色而新增模型调用。

默认 workspace 只决定新的未显式绑定请求的上下文；既有会话的 workspace 绑定、项目和历史 `SessionBound` 不被批量迁移，也不因选择而创建项目私有脑。workspace 是非空有界不含控制字符的逻辑 ref，不接受文件路径。路由必须引用同一完整 patch 内的模型；删除仍被引用的模型拒绝整次事务。foreground 优先仅在上下文、工具 schema、能力强度等既有约束满足时生效；不满足时仍由原兼容排序选择，保留已有 fallback 组和 high-risk 限制。cognition 显式绑定的模型不可用时启动失败，不能静默调用默认模型并自报绑定项。

Host 在同一 CAS/journal 中保存偏好、模型与凭据，materialize 到同一 `CoreServiceConfig.brain`，只有实际 core ack 后才算加载。snapshot 返回有效偏好与来源（安装模板或受管配置），不返回密钥。已发布旧 revision 的 JSON 与旧 operation digest 保持可核验；旧请求省略该字段时继续使用旧摘要算法，有新偏好时使用显式版本域摘要。配置恢复、模型保存和恢复脑接管均保留偏好；非法/陈旧/异义保存零发布，不调模型。受管启动不读取 shell env 覆盖路由。

只有角色分工改变也必须改变正式 composition digest，不能仅依赖 profile 清单发现差异。按 architecture/09 §2.3 在既有 snapshot 摘要域绑定已消费的角色配置，不增加 manifest/EventKind 字段。未显式设置角色时保持原摘要；相同角色配置重启得到同一摘要。历史 SessionBound/审批不改写，默认恢复入口不能把旧角色上下文视作当前配置；只读 pending 与历史查询继续可用。至少验证前台单独变化、后台单独变化、同角色重启、缺省摘要兼容，以及旧 run 的原绑定不被新角色改写；读取/拒绝均零额外模型和工具调用。

本增量只闭合上述有实际消费者的设置；能力、消息渠道、代理和网络分别按其 typed 生命周期实施，不能因本节通过而宣称 IP08/IP21 完成。

### 11.7 受管 skills 配置接入

能力设置按实际消费者逐项接入，首先接通现有 SkillRegistry，不开放任意能力 JSON、可执行路径或配置中自报权限。`ProductConfigPatch` additive 字段 `capabilities?: ProductCapabilitySettings` 与模型/brain 共用原 operation、expected revision、Host journal 和 staging/CAS；对象为 closed/versioned `{ schema_version: 1, skills }`。每项为 `{ schema_version: 1, id, summary, scope, version, body, enabled, trust }`，id/scope 是产品标识而非磁盘路径；version 为正整数。最多 128 项，summary 最多 2,048、body 最多 16,384 UTF-8 字节，整个 settings JSON 最多 65,536 字节；空正文、重复 id、未知字段/版本和非法控制字符拒绝。秘密仍只走现有专用 resolver，skill 正文不能作为密钥输入。

`enabled` 与 `trust` 独立；缺省为禁用、`untrusted`。信任选择仅为 `untrusted / owner_input`，后者必须由已认证 owner 明确标记，不由启用、导入或保存自动生成。它是现有 TrustTier 的输入来源标记，不是 capability permission、AutonomyEnvelope 或 VerifiedProcess。外部技能保留不可信状态，直至 owner 单独审查并确认；不能从正文自报 trust、hook、工具或执行权。

`ProductConfigView` 返回已认证 owner 的当前 skills 和来源。缺省 patch 字段保留既有设置，显式空列表才清空；读回不调模型、不触发安装/发现/执行。配置保存和 core acknowledgement 分开，旧 run 不热换 skill/context。Host state 使用 schema 5 承载显式 capability settings，保留 schema 2/3/4、无 capabilities 的旧 patch digest、已保存 core bytes、brain preferences 和恢复数据绑定；在任何凭据/快照文件写入前验证完整新 state 与 core 的体积上限。恢复/后续旧客户端保存不能丢掉已保存 skills。

受管 core 文件只在显式配置时增加同一 versioned skills settings。ManagedBrainConfig 进行同一语义校验并转为现有 SkillDefinition：禁用项不可选，不可信项不可见/不可加载，其他项仍经既有作用域过滤、按需正文加载、provenance、context 及 Harness 治理；不旁路注册表、不新建 memory 或后台模型 client。不接受 stdio MCP、shell、browser、plugin executable 或任意 environment 配置；这些能力仍按 prd/24 的隔离 worker 接入，尚未接通时保持 blocked。

验收覆盖旧 digest/旧 materialization、保存/幂等/异义复用/CAS、中断后原 intent 恢复、旧客户端保留/显式清空、恢复数据切换保留；正式 managed runtime 及 Web→Host→core 重启证明 skill metadata/正文进入真实 context，未选/不可信/禁用/越 scope 不进入。模型用量、真实 provider、原生安装和完整 IP08/IP21/IP24 分别取证。

### 11.6 实施细化：配置发布与运行态的访问隔离

新安装的路径/Windows ACL 采用 prd/24 §4.4。`brain/runtime` 在认领后由 core 首次启动按需创建；准备安装和赋权不往新脑目录预写占位文件/子目录，保持原“新脑候选必须为空”的检查。Host 创建的 `config/core` 和 `keys/model-credentials` 则可预建，未来发布/凭据 revision 继承只读授权。

重复准备只核对原安装与其有界、版本化 template，报告 template 指定的 core 文件，不改写 template、密钥或旧 runtime。模板身份、路径归属、版本或文件类型不匹配即拒绝；不得把找不到新路径误当成旧安装没有配置，也不得通过放宽父目录写权限兼容旧布局。

### 11.8 受管 Web 网络配置与维护绑定

`ProductConfigPatch.network?: ProductNetworkConfig` 与模型、brain、skills 共用原 CAS/revision；省略保留，不能在普通保存、核心启动或恢复脑发布时激活网络。对象为 closed/versioned `{schema_version:1, bind, allowed_hosts, tls, firewall}`：bind 是带非零端口的明确 IP，禁止 unspecified/multicast；Host 是最多 16 项、无重复、小写且带实际端口的精确 authority，IPv6 必须带方括号。LAN 必须同时具备 HTTPS、Host 与 firewall binding。loopback 不保留 firewall rule。

公共设置 v2 的 `tls` 为 `{schema_version:1, material_ref:"managed:web"}`；响应只返回同一材料引用和 `material_configured`，不返回证书/私钥路径。Host 在受保护的 `web_material_directory` 内解析材料，拒绝路径逃逸、链接、超限文件、无效 PEM 和不匹配的密钥；证书和私钥各至多 64 KiB。静态检查解析 PEM、验证证书与密钥匹配，不声称客户端信任或网络可达。`firewall` 为 `{schema_version:1, rule_ref}`，引用不能代替原生规则生效证明。

Host state schema 6 保留旧 schema 2–5、旧请求摘要与 core bytes。新安装固定 `web_network_config_path`（`config/web-network/web-network.json`）及独立材料目录 `config/web-material`，浏览器不能改变投影路径。两个目录只允许管理域写入、Web 读取，Brain 不可读；网络发布目录在 Windows 继承只读服务 ACL，在 POSIX 使用 Web 专属 setgid group，使原子替换后的文件仍保持服务只读，不扩大整个 config 的权限。旧模板路径不静默迁移。不可变 revision 同时保留待应用配置和当前已发布网络投影；journal 的可选 `web_network_digest` 绑定后者。`network_active` 只表示两者一致，不等于运行中的 listener 已加载、客户端信任或跨设备可用；core acknowledgement 也不能代替网络维护回执。`pending_restart`/校验报告须同时覆盖 core 未确认与网络投影未应用。

没有上述受管投影路径的旧安装模板继续支持原模型/brain/skills 保存；客户端回传未改变的默认网络值可兼容读写，但不同的网络目标须在任何 staging 前拒绝。不能通过设置页静默改写旧安装模板，也不能保存实际上没有输出路径的新网络配置。此类安装不宣告 ConfigureLan 可用，待显式安装升级处理。

`ProductConfigValidationReport.network_validation_digest` 绑定待应用的 typed 配置和已解析证书指纹；私钥匹配复核但不回传私钥/其路径衍生秘密。`ConfigureLan{config_revision,validation_digest}` 仍经 `/product/maintenance`、owner、近期口令与 product revision。已知缺少 firewall adapter 或管理员规则尚未满足时在 plan 前拒绝，不写假维护事务，Host 不宣告该切换可用。原生 firewall 接口 `reconcile_firewall(previous, desired)` 必须核实完整目标绑定，以及管理员已撤销旧规则或同规则中的旧监听范围；它不自动修改防火墙。receipt 绑定 `bind,rule_ref?,network_digest,enabled,removed_rule_refs`；LAN→loopback 也须证明旧规则已移除。adapter 已尝试后的错误绑定或不明结果 fail closed，原 operation 进入待恢复，不换 key 重试。

网络切换采用先回执、后切换：Host 在配置锁内校验并持久化原 `Planned`，只有首次接纳者得到不可复制的进程内 dispatch ticket；HTTP 完整返回该操作并关闭内部响应流，让 Web 配置请求能先退出，再由同一受监督、计入 drain 的 Host connection worker 消费 ticket。不得在请求仍等待 host 时同步停止其所在 Web，也不得启动无监管后台线程。响应写入/关闭失败、重复 POST、原号查询或进程重启均不重新生成 ticket，不自动执行。执行时重新认证原 actor/owner、校验原 command digest、期限、当前 release、revision、已发布投影和证书材料，之后先落 durable stage intent 再做原生动作。首次回执仅是接纳，不等于切换完成；Web 保留原操作号和候选地址，断线后在新入口重新登录只核实原号。

网络切换沿用产品 journal 已有的全局维护互斥：`Planned` 在单写者锁内检查所有未 `Committed` 的原操作（包括未决 intent 和 `RecoveryRequired`），拒绝新网络切换、升级、回滚、恢复提交、配置应用或卸载事务，反向同样成立。不同操作号、刷新页面、重新认证和 Host 重启不解除此边界。相同原命令的幂等核实、状态投影和原回执查询保持可用，但只读查询不创建另一个 Inspect 事务；不能通过删除记录或把 Unknown 当成失败解除互斥。沿用原 schema/fold，不另造网络锁、重写历史事实或改动既有阶段迁移语义。

只有已核实 firewall 后才发布固定 overlay 并对内置 Web 服务执行有界切换；副作用/启动结果未知须保留原操作和诊断，不视为已完成。原生 adapter、监听健康检查、失败回到可达 loopback、客户端证书信任与跨设备登录属于完整 IP09/IP24 的独立义务；仅有 typed 配置、TLS 组件或模拟 receipt 时不得宣称这些义务通过。Web 关闭不停止主脑，内部 Gateway 地址不随此配置变为 LAN。

### 11.9 Windows 原生防火墙核验

此适配实现 §11.8 已批准的原生网络边界，不新增维护动作或脑事件。遵守 owner 本轮确认的“不自动修改防火墙”：安装、配置保存、检查与网络应用均不创建、修改或删除 OS firewall rule。管理员在系统中预先配置规则；产品只读核实目标规则以及旧绑定已撤销。`SystemServiceManager` 只在实际 Windows Host daemon、匹配的 Host/Web SCM 注册与恢复策略、受保护的固定配置/二进制以及管理员 token 均可核实时宣告支持；开发测试程序和未注册安装不因此获得能力。执行前重新核对原安装与实际权限；其他平台继续明确不可用，直至完成对应适配。

原生命令使用产品内置的固定、只读 PowerShell/NetSecurity 程序，仅通过有界 stdin 接受 closed/versioned 参数；不拼接用户脚本、命令、服务名或任意规则名。当前适配固定受保护的 `C:\Windows\System32` 程序与模块，其他 Windows 目录布局显式不可用；不搜索 PATH 或从请求/env 选择可执行文件。清除继承环境、禁用 profile，并限制输入、输出、运行期限和 child 回收。预检须在 plan 前拒绝缺失/不匹配规则；执行前再查。执行中的进程失败、超时、截断/不匹配回执保留原操作的不明结果，不自动重新派发。

规则名为 `forme.web.lan.` 加 `SHA256(rule_ref UTF-8)` 的小写 hex，产品 group 为 `forme.web.lan.v1`，description 绑定 `canonical_digest(("forme-native-web-rule-v1",rule_ref,bind,SCM Web executable))`。管理员规则限定到 SCM 核实的 Web 程序及 `forme-web` 服务。LAN allow 仅覆盖明确本机 IP/端口、入站 TCP、`LocalSubnet`、Domain/Private profile，禁用 edge traversal；不开放 Public 或路由映射，不操作其他规则。旧规则名不同时须在 PersistentStore/ActiveStore 均已不存在；同名换范围只接受唯一精确的新绑定。目标必须同时在持久规则和实际生效规则中核实地址、端口、应用、服务、方向、动作和 profile，命令退出成功不能替代该读回。`validate_firewall_reconciliation` 是只读 plan 前预检，默认 adapter 保持兼容；`reconcile_firewall` 在正式 Windows 适配中也只再次核验并生成绑定回执，不隐含 OS 写权限。

测试将固定脚本的参数/读回算法与实际管理员防火墙验收分开。自建 cmdlet fixture、只读权限预检或编译成功不计原生防火墙 PASS；证书客户端信任、Web listener 健康、失败恢复和跨设备路径仍须单独完成，不能由该 adapter 的成功回执替代 IP09/IP24。

### 11.10 设置协议 v2 与兼容读取

owner 已确认扩展设置协议及 LAN/TLS 事务。公共 `ProductConfigView/Patch/Response` 的设置版本与 template、模型条目、owner binding、验证请求和 Host 私有 state schema 分开，不能把共用的 v1 常量整体改为 2。新设置协议使用 schema 2；已发布的 schema 1 请求仍校验原字段和原摘要，省略 brain/capabilities/network 继续保留已有值。未知版本、字段和不支持的写入零 staging、零密钥写入、零原生动作。

`GET /api/v1/product/config?schema_version=2` 明确请求完整设置快照；未指定或显式 1 保留已发布的 v1 形状。v2 增加 typed 能力可用性投影，逐项标明模型、skills、MCP、plugin/package、消息渠道、代理和 LAN 管理是否有实际消费者及其阻断原因；不支持的设置只读展示，不能伪造已启用状态。模型与内层 DTO 沿用其各自 schema 1。写入响应跟随请求的设置 schema；同 operation 从 v1 改成 v2 是不同命令，不能静默复用授权。版本协商不降低 owner/CSRF、CAS、trust/permission 或执行隔离要求。

可用性字段为 `availability: ProductConfigAvailability`，内层 schema 1，包含 `features: Vec<ProductSettingAvailability>`，每个固定 feature 恰好一项。feature 为 `models/skills/mcp/plugins/packages/channels/proxy/network`；state 为 `configurable/unavailable`，reason 为可选 `managed_consumer_missing/network_paths_missing`。本投影仅标明受管配置消费者是否已接通；是否已安装、授权、通过原生检查和允许本次维护仍取各自正式投影，不能把 configurable 解释为已获执行权。v1 响应完全省略 availability，不能以 null 新字段改变其已发布形状。

v1/v2 共用同一 Host store 和恢复日志，不建第二套配置。新前端显式读取/提交 v2，并拒绝不能理解的响应版本；旧 patch、原日志字节、core 配置和凭据引用不因读取或升级而改写。设置可用性仅是配置/适配能力，不证明 provider、listener、客户端信任或任务质量。

### 11.11 受管模型代理配置

`proxy_config` 是 `ProductConfigSnapshot` 中已经确认的设置项，本批把它接通到模型 HTTP provider；它不是 Web 的 URL 代理，也不能把浏览器请求转发到任意地址。公共设置 v2 增加 `proxy` 与 `proxy_source`，v1 形状保持不变。`proxy` 是 closed/versioned `{schema_version:1, mode:"direct"|"http", endpoint?}`：`direct` 不带 endpoint；`http` 只接受明确的 `http://` 或 `https://` authority，拒绝 userinfo、路径、query、fragment、控制字符、空主机和无效端口。当前不支持 SOCKS、PAC、自动发现、代理认证或从聊天正文指定代理。

代理配置与模型、brain、skills、network 共用同一 `operation_id`、expected revision、Host journal、CAS、staging 和 core acknowledgement。v2 省略 `proxy` 保留当前值；显式 `direct` 清除代理。旧 v1 保存不得清除代理。代理变更进入新的 composition/config digest，进行中的 run 固定原配置，应用前需要受控 core 重启或重新装配；不能通过修改进程环境热更新。

Host 只把已验证的代理 authority 写入受管 core 配置。模型 provider 在每次真实 HTTP call 前使用已绑定的代理快照；代理连接失败、返回不明或配置过期按现有模型 transport 的 retryable/unknown 分类记录，不能自动切换到直连。代理不会改变 model profile、trust、permission、AutonomyEnvelope、审批或 fallback 真实性，provider 事件仍记录实际 profile。代理 endpoint 和任何未来 secret ref 均不回显到浏览器、事件、trace、诊断包或错误正文；本批不实现代理认证，因此含凭据的 endpoint 必须拒绝。

验收至少覆盖：v2 读写与旧 v1 保留、CAS/幂等/异义冲突、direct 清除、state/revision 恢复、core materialization、真实模型 HTTP 请求确实经过固定代理、代理失败不直连、运行中配置不热换、endpoint 中的 userinfo/path/query/fragment/控制字符全部在 staging 前拒绝。MCP、plugin/package、消息渠道仍因没有对应的受管产品消费者保持 `unavailable`，不得借代理配置通过提升它们的状态。

### 11.12 网络监听检查与有界失败回退

服务管理器的 Running 回执不足以提交 `ConfigureLan`。Host 还必须连接批准的精确地址，检查实际 listener 的安装身份及已加载配置；不跟随 redirect、不使用环境代理、不解析任意外部 URL。检查是只读且有界的，允许在固定窗口内等待启动；不重发维护命令、不调用 core、模型或工具。

Web 提供最小 `GET /api/v1/product/network/health?nonce=<64 hex>`，只接受有界随机 challenge，沿用精确 Host/Origin、默认拒绝 forwarded header、独立有界读配额及 no-store。回应仅含 schema、installation digest、listener 配置 digest、原 nonce 与用途隔离的 Ed25519 签名；不含 owner、路径、模型、账号、cookie 或诊断正文。签名复用安装预置的 Web assertion key，签名域与认领/登录/core assertion 完全不同。缺少受管 key 时不宣告该检查可用。Host 使用 template 中 pinned Web 公钥验证新 challenge，不能信任对端附带的公钥或通用 200 页面。

listener digest 绑定实际加载的 bind、精确 Host 和 TLS 开关/配置；防火墙仍须独立只读核验。HTTPS 连接验证证书、名称和批准材料的指纹，不以跳过证书验证取得 PASS。Host 对安装材料的定点信任不代表另一台设备或浏览器已信任证书。不同安装、旧端口/旧配置、错 nonce、错误签名、过大响应、重定向或错误证书均不能提交成功。

只有切换的 Stop/Start 均取得确切已知回执、但监听检查未通过时，才可执行同一已批准命令内的有限回退：先持久化回退 intent，再确切停止刚启动的 Web，发布安装模板的 loopback 配置，启动并验证该 loopback listener。不得回到旧 LAN、扩大监听范围、改防火墙或修改 desired 设置。用户授权的网络目标仍保留为待应用；回退只改变 published overlay。

网络检查/回退结果使用产品 journal 的 additive 记录，绑定原 operation、command、目标/实际配置摘要和检查回执。只有目标检查通过，或 loopback 回退已完整验证，才可结束该事务；后者必须以明确 `RolledBack` 结果展示，不能显示目标应用成功。任何原生副作用不明、发布不明、签名/配置冲突或回退中断进入 `RecoveryRequired`，保留全局维护 fence，不能通过重启自动重放动作。journal 恢复只重建原事实；新记录不改变历史 envelope/hash 或现有生命周期分类。

具体 wire 为 `ProductNetworkHealthProof{schema_version:1,installation_digest,listener_digest,nonce,signature}`；listener 摘要域绑定实际 bind、按序精确 Host 列表与 TLS 开关，证书身份另外由 TLS peer 证书与批准材料逐字节核实。每次尝试生成新的 32-byte OS challenge；签名摘要为用途域加 schema/安装/listener/nonce，不包含 signature 自身。健康窗口最多 8 秒，单次连接/读取受剩余总时限约束，响应最多 8 KiB；仅重新探测，不重放 mutation。

`MaintenanceOperation.network_result?` 是 schema 1 的 additive 安全结果：`outcome(activated/rolled_back),active_bind,requested_listener_digest,active_listener_digest,health_receipt_digest`。历史对象省略该字段；历史 Committed 没有该字段时只能表示原维护已结束，不能补造健康证明。journal 增加 `NetworkProgress`，子阶段顺序只有 `TargetIntent -> TargetVerified` 或 `TargetIntent -> FallbackIntent -> FallbackVerified`；TargetIntent 固定目标和安装 loopback 摘要，FallbackIntent 绑定已知启停回执及健康失败摘要，Verified 绑定实际 proof。子阶段写入只在原 Checking intent 内、同一 ConfigureLan 命令下接受；结束后、跨命令或次序不对零追加。回退不新建 MaintenanceCommand，不改 Config revision 或 desired，只通过原配置写者发布明确 loopback snapshot。每一原生动作失败都停在该原操作，恢复查询不重建一次性执行权。

组件验收至少覆盖：真实独立 Web 进程的 HTTP/TLS challenge、错安装/nonce/签名/旧配置、没有 core 时的检查；成功切换只执行一次；健康失败后仅回退确切 loopback；Stop/Start Unknown 零后续发布/启动；回退的每个中断点及重启原号核实；旧日志混合回放及 v1/v2 配置兼容。原生 SCM、客户端证书信任与跨设备 IP09/IP24 另取证。

健康 transport 放在现有执行宿主，不新增 crate 或依赖边。`NativeServiceManager` 的 additive 默认方法 `probe_web_listener(network, nonce, timeout)` 使用真实有界 TCP/TLS；可替换的测试 adapter 只能提供传输回执，不能跳过 broker 的 pinned-key、安装、配置和 challenge 验证。错误明确分为普通不可达与不可信响应；证书、HTTP framing、签名及身份冲突归后者。TLS 以受管证书链作为定点信任材料，执行名称/有效期/链验证并核实实际 peer 链指纹；不使用系统代理、DNS 或宽松验证器。

既有 ConfigureLan 生命周期分类不变，也不增加第二把全局维护锁。执行中每次发布或原生动作前按原 owner/revision/validation digest 重查，探测后再次核对 published/desired 与授权；并发设置保存使原命令过期时进入恢复，不覆盖新设置。8 秒探测窗口还受原命令剩余期限约束；授权到期后不能启动新的原生动作。网络子阶段另绑定目标与固定 loopback 的 bind，Verified 结果必须匹配对应 bind/digest。Checking 未持久化 Verified 时不能结束新网络子事务；历史没有子阶段的 Committed 保持原样，不补造证明。

为区分历史事务与必须验证的新事务，新 ConfigureLan 的 Planned fact 带可选 `network_contract:1`，非网络事务及旧 wire 完全省略该字段。缺省不是当前代码创建新网络事务的选项；新 writer 固定写入该标记。回放旧 fact 时空值仍省略，保持原 hash。带标记的网络事务若缺少相应 Verified/结果，不能写 Checking receipt 或 Committed；无标记的历史终态可读，但永远没有健康结果。

### 11.13 主脑离线的只读备份核验

§5.5 的离线备份核验使用独立的闭合 `POST /api/v1/product/data/verify`，不把主脑数据查询改成自动故障转移。浏览器只提交 `schema_version:1, source, config_revision`；`source` 是既有数据工作台的 opaque 对象名，不是路径、URL 或 key。请求继续验证 session、Origin/CSRF 和有界读配额，owner 从已认证会话赋值。Web 通过 Configuration 域固定路由 `/v1/host/configuration/data/verify` 发送 `ProductBackupVerificationRequest{schema_version:1,install_ref,owner,expected_config_revision,source}`；该 route 不是通配维护/AgentExecution 入口。

Host 核对已认领 owner、当前配置 revision 和安装身份后，使用当前安装固定的 `forme-hostctl maintenance-verify-backup --installation-manifest <安装派生路径>`，经有界 stdin/stdout pipe 传递该请求；程序、参数、配置和数据根不能由浏览器选择。worker 从受保护的安装清单、template 与 storage-only 配置派生唯一 vault 和 `brain/data-operations/<source>`。它通过现有 `OfflineCoreMaintenance`/`OfflineBrainMaintenance` 获得备份的 exclusive access fence，验证 owner、原 key、完整性、事件连续性和 replay；不建立 ProductionRuntime、不加载模型/worker/代理、不启动 core、不写脑事件、不恢复或切换数据。已知 runtime-only 配置只能被明确忽略，未知字段仍拒绝。锁冲突、错误 key、链接、保护不可证、坏备份或过期 revision 均拒绝，不新建空脑。

返回 `ProductBackupVerificationResponse{schema_version:1,request_digest,config_revision,verification}`，`verification` 沿用不含路径、key 或正文的 `DataBackupView`，不携带 restoration。Host/Web 同时核对请求摘要、对象与 revision，失败不转为 verified；worker 的完整交换、成功退出及回收共用最多 8 秒期限，frame 上限沿现有维护 pipe，外层 Configuration transport 保留 10 秒期限。Host 在核对与核验期间持有既有配置锁；未决生命周期事务阻断查询，不另建事实源。页面在线核验仍走正式 core；离线时显式选择本路径，仅显示核验结果和边界，不解锁备份创建、遗忘、导入、恢复或切换。此只读查询可重新检查，但不被包装成任何 mutation 的重试或批准。它不改变现有 brain/maintenance trait、EventKind 或私有配置历史字节。
