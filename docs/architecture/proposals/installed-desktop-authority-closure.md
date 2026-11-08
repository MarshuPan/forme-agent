# 安装版桌面授权签发与主脑执行接线草案

状态：待 owner 确认。承接已批准的 architecture/10 §7.2、PRD24 §4.11 和 IP11，不增加新能力阶段。本提案尚未修改冻结协议、产品源码或安装现场；原签名候选及其源码工作树保持原件。冻结契约变更按 AGENTS.md 硬规则 3，在确认后先回填正式文档，再实施代码。

## 1. 已核实的接线缺口

| 现有契约 | 当前实现与证据 | 必须补足的行为 |
| --- | --- | --- |
| Host 独立签发桌面授权 | `DesktopSessionBinding` 已有 owner/action/plan/epoch/nonce/TTL，但 `WorkerExecutorHost::prepare_desktop_action` 只校验传入 binding 并观察 helper，没有独立签发记录查询。 | Host 保存 owner 选择及实际签发记录，dispatch 不以调用者提交的 binding 自证授权。这个缺口本身不证明默认安装已可被利用；默认 Computer 未启用。 |
| 选择真实 OS 会话 | `WindowsDesktopSessionHost::inspect` 根据已有 binding 的 session id 核对 token/SID/AuthenticationId/锁屏；没有 owner 可选的 Host 会话目录。 | Host 从 OS 枚举并独立观察，Web 只选择 opaque catalog ref；不能让 Web 或模型拼 SID、session id、generation、desktop 或 pipe。 |
| 正式主脑附加 binding | `ExecutionPlan::with_desktop_session` 唯一调用位于 `installed_worker_backend_contract.rs`，普通 planner 初始化为 None。 | 在最终 plan recheck 后、原 worker dispatch 前取得 Host 的 exact grant 并附加。`worker_backend::execution_spec` 已正确转发该字段，不是丢字段问题。 |
| 可用性与执行保持一致 | 新安装 `file_provisioning` 的 binding 仅支持 File，managed config 仅开启 File。生产 factory 虽有 Computer 的受管分支，但没有上述授权消费者。 | Computer 只有在固定 helper、可用平台 binding、有效授权流程和受管配置均接通后才可启用；不通过打开通用 backend flag 宣告可用。 |

既有真实 helper/peer 原生代码存在。当前 safe helper 只执行有限 Computer 操作，拒绝 Screenshot、SecretRef/ContentRef 注入和未实现的交互 PTY。补齐授权不能把这些子能力标记为已支持。

## 2. 需要批准的最小协议增量

保留原 `DesktopSessionBinding`、`WorkerJob/Permit/ExecutionSpec`、`ExecutionPlan` 摘要、`ApprovalGrant` 和 `ResumeInput` 形状。产品侧新增独立 schema 1 的 closed DTO，不升级全局设置 schema 或复写历史产品记录。新公有 trait 方法及路由必须先回填 PRD24/25/26 与 architecture/10。

所有操作绑定同一 installation/owner/authority epoch。ID 沿用现有 `validate_product_id`；digest 使用现有验证器；新请求的 schema/未知字段/长度/身份校验须在 journal 写入、helper 启动或授予权限前完成。单请求上限 16 KiB；目录最多 64 个会话，超限显式 unavailable，不截断后暗选；每个展示 label 至多 128 字节。真实 OS 标识仅进入受保护 Host state，Web 返回 opaque ref 和安全展示状态。

| 新对象 | 冻结字段及语义 |
| --- | --- |
| `DesktopSessionCatalog` | `schema_version, installation_ref, owner_ref, authority_epoch, catalog_revision, catalog_digest, entries, observed_at, expires_at`。有效期最多 30 秒；目录读取不启动 helper、不创建 grant、不写脑事件。 |
| `DesktopSessionChoice` | `schema_version, session_ref, platform, label, available, supported_capabilities`。session_ref 随 Host catalog/generation 绑定；无可验证用户 token、锁屏、断线或 peer 条件不满足时不可选择。 |
| `DesktopPendingAction` | `schema_version, pending_ref, owner_ref, authority_epoch, run_ref, approval_ref, action_ref, plan_digest, normalized_action_digest, affected_resources, capability, policy_version, tool_schema_version, expires_at`。内容由认证 Core 依据冻结的最终待审批 plan 注册；Web 不能提交或改写这些事实。仅注册，零执行。 |
| `DesktopConsentRequest` | `schema_version, operation_id, expected_revision, pending_ref, catalog_revision, catalog_digest, session_ref, expires_at`。Web 从认证会话绑定 owner 并重新认证；不得携带可信 SID/session/generation、任意 capability 集、参数、程序或浏览器自报 principal。期限不得超过 pending 与 15 分钟上限。 |
| `DesktopConsentView` | `schema_version, operation_id, revision, pending_ref, consent_ref, status, expires_at, reason_ref`。status 为 `Prepared / Committed / Revoked / Expired / Unknown`。只证明桌面选择授权，不能等同原动作审批或执行成功。 |
| `DesktopGrantRequest` | `schema_version, operation_id, consent_ref, job, permit, execution`。只允许认证 AgentExecution caller；Host 比对已登记 pending、已提交 consent、原 plan/action/owner/epoch、规范化摘要、资源、OS 当前 generation 与实际支持的 capability，随后签发原 `DesktopSessionBinding`。输入 execution 的 desktop_session 必须为空，防止调用者自行签发。 |
| `DesktopGrantView` | `schema_version, operation_id, revision, binding, status, reason_ref`。仅 Core 可读取 binding；普通 Web 只读 consent/operation 状态，不获取 token、nonce、helper endpoint 或完整 OS 标识。 |
| `DesktopRevokeRequest` | `schema_version, operation_id, expected_revision, consent_ref`。认证 owner 重新认证后撤销；停止新准入，已 Started 的 job 只核对原 receipt，不把撤销当作动作未发生。 |
| `ProductComputerPlacementSettings` | `schema_version, enabled, scope, surface_refs`，作为待批设置 v3 的 `managed_capabilities.computer` 独立字段。surface_refs 只引用 Host 已验证的桌面 placement，最多 64 项；默认 disabled，启用不创建 consent/grant。Host 校验并物化专用 desktop profile 与 Computer binding，不复用 File profile 冒充桌面支持；缺真实 helper/consumer 时拒绝启用。旧 v1/v2 保存保留本字段，不隐式启用或清空。 |

新 digest 使用现有 canonical serializer，域分别为 `forme.product.desktop.catalog.v1`、`forme.product.desktop.pending.v1`、`forme.product.desktop.consent.v1`、`forme.product.desktop.grant.v1`。域与对应 DTO 的全部冻结字段一起摘要，不对旧 plan/digest 做算法变化。展示 label 不参与权限判断。

## 3. 受认证的接口与分域

HostManagement 的 owner 控制面新增 typed 方法：

```rust
fn desktop_sessions(&self, auth: &HostAuth, now: Timestamp) -> Result<DesktopSessionCatalog>;
fn consent_desktop(&self, auth: &HostAuth, request: DesktopConsentRequest, now: Timestamp) -> Result<DesktopConsentView>;
fn desktop_consent(&self, auth: &HostAuth, operation_id: &str, now: Timestamp) -> Result<DesktopConsentView>;
fn revoke_desktop(&self, auth: &HostAuth, request: DesktopRevokeRequest, now: Timestamp) -> Result<DesktopConsentView>;
```

AgentExecution 的 Core 控制面新增 typed 方法：

```rust
fn register_desktop_pending(&self, request: DesktopPendingAction) -> Result<DesktopPendingAction>;
fn acquire_desktop_grant(&self, request: DesktopGrantRequest) -> Result<DesktopGrantView>;
fn desktop_grant(&self, operation_id: &str) -> Result<DesktopGrantView>;
```

上述为需批准的签名范围；实现回填时放入现有 Host/worker transport 对应 trait，保持拒绝型默认实现，外部旧实现不能因缺方法而获得能力。Web 的 allowlist 只增加 `/api/v1/desktop/sessions`、`/api/v1/desktop/consents`、`/api/v1/desktop/consents/{id}`、`/api/v1/desktop/revocations` 四类有限路由。不得新增通配 broker、任意 IPC、原生桌面直调或 Web→模型路径。

Host 继续复用既有 Brain Service SID/活动 release image/专用 bearer 的 AgentExecution 双向认证；Web 使用自己的 HostManagement 凭据，不能调用 grant 签发面。session/helper journal 是 Host 产品事实；脑审批、ActionPlanned/Started/终态仍由原 EventStore 单写者产生。18 crates、99 EventKinds、五处审计 unsafe 边界不变。

## 4. 一次动作的顺序与竞争边界

1. Harness 为受管 Computer intent 形成最终 plan，经既有 plugin transform、policy、permission、AutonomyEnvelope 与 CompetenceGate。注册该 exact pending descriptor；没有有效桌面 consent 的动作进入既有 `ApprovalWait`，不启动 Worker/helper、不消耗审批 nonce、不发 ActionStarted。不得发明新 ResumeTrigger。
2. Web 读取原 pending 与 Host catalog，展示当前动作、影响资源和会话选择。owner 对该 pending/session 重新认证提交 consent。刷新或响应丢失只查询原 operation；Web 本地勾选、账户 SuperAdmin 或已登录不代表已授权 OS 会话。
3. consent 的 Host 原号结果明确 Committed 后，才能沿原 pending 的既有 `grant-once` 审批/ResumeInput 提交原动作批准。未提交/Unknown 的 consent 不准放行原审批。直接 Core 审批同样必须在执行出口检查 consent，防止客户端绕过 Web 顺序。
4. Harness 恢复后重新形成并检查最终 plan。任何 action/参数/资源/policy/tool schema/epoch 变化，都不能继承旧 pending consent 或 ApprovalGrant。原审批 nonce 与 worker nonce 是不同域，禁止混用。
5. WorkerActionBackend 为该 plan 保留唯一 attempt，并用原 job/permit 请求 exact grant。Host 此时才绑定 worker nonce 与 deadline；grant 到期取 consent、job、permit 和签发时刻加 15 分钟的最小值。Host 持久化签发后才返回，backend 用既有 `with_desktop_session` 附加后 dispatch。
6. grant 获取响应不明只能查原 operation，零 dispatch；不得换 operation/nonce/job 重领。grant 已签发但 dispatch 未发出，与动作已 Started 是不同状态。dispatch admission 必须查到 Host 原签发记录并 CAS 消费 exact grant，再沿既有 helper observe/Started/receipt 链路执行。
7. 相同 grant/job/permit 的重复 dispatch 先返回原 job 观察；其他 job、参数、nonce 或 generation 复用一律拒绝。Started 后的丢连接、过期或重启仍为 Unknown，只查原 receipt。撤销、锁屏或注销后不再准入；重登录不能复活原 grant。

Host 操作锁/单写者覆盖 consent/issue/revoke/admit 的比较与写入；原 worker journal 与 grant journal 的原子性采用 write-ahead intent 与同号恢复：先有持久 grant 消费意图再准入 worker，崩溃时独立核对原 worker 原件，未能确定关系则 Unknown 且不得执行。不得用两次非原子写入后“启动恢复猜测”为已消费/未发生。后续回填 PRD 必须列明每个 crash cut 的原件、恢复判断与零重复副作用断言。

## 5. 实现与原生边界

安全 Rust 实现 Host catalog/consent/grant journal、transport、final-plan attachment、Web typed adapter 与会话选择。现有 Windows session adapter 只在同一已审计文件内增加 OS 目录查询，并返回 safe typed observation；peer adapter 复用，不增加 unsafe 文件、任意程序、环境或原始 token 外传。固定活动 release helper 校验及 kill-on-close Job 保持。

受管 Computer 配置依赖前份[受管消费者提案](installed-managed-consumers-closure.md) 的显式配置与消费者接线；该草案原来没有 Computer 字段，本节明确把上表的兼容 v3 增量一并列入本草案，不能假称之前已批准。授权协议仍是独立变更范围，批准其中一份不隐含批准另一份。Screenshot/交互 PTY/ContentRef、macOS TCC 与 Linux portal 的后续实现义务保持开放；本草案不以 disabled 或 unavailable 将其关闭。

## 6. 可执行验收与交付

- 契约与事务：新/错 schema、未知字段、超限、过期目录、错误 owner/epoch、未认证/CSRF、伪造 OS 标识、CAS、异义幂等、grant 未签发/被篡改/跨动作复用均在 helper/driver 前拒绝。逐个 crash cut 重启核对同号原件，不双签发或双执行。
- 正式主脑：由 ProductionRuntime 创建 exact pending，Web 选真实目录后 consent 与原审批恢复，final recheck 后 backend 附加 Host binding，Host 查原签发记录并执行。只运行测试里的 `with_desktop_session` 不能计通过。
- 撤销与竞态：consent 保存后改 plan、审批后锁屏/注销/换 generation、grant 签发与 dispatch 之间撤销、同 job 重放、Host/helper 丢失、Started 后 receipt 丢失；验证原 job Unknown 与零重派。
- Windows 原生 IP11：SCM LocalSystem Host，真实登录用户 token/session/generation、活动 release helper 与双向 peer、一次有限桌面动作和原 job receipt；锁屏、注销与会话切换另外保留对应实际结果。组件 fixture 不替代原生正反证据。
- 回归：涉及模块门、S1–S99/C1–C24 相应永久门、工程/合规/结构门；新开发签名安装包另做 IP10/IP11/IP18/IP21/IP24 联合验收。未执行或环境不足明确 NOT_RUN/UNVERIFIABLE。

确认后先回填正式文档并单独提交，再实现与验证。2026-11-09 的当前安装已是 `cf3fbc9`，Host/Web/Brain 真实运行；新 `7b13529` 签名升级及文件/备份联合批次仍等待原 UAC，尚无升级回执。本草案不修改安装现场、不触发 UAC、不宣称已关闭 IP11 或整体目标。本次仅校准安装状态，§2–6 的待批协议与实施范围不变。
