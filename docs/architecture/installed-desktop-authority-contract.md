# 安装版桌面授权实施契约

2026-11-09：承接 owner 已批准的[桌面授权方案](proposals/installed-desktop-authority-closure.md)。本文将该方案的对象、持久化和恢复规则落实到 PRD24–26；不表示代码或原生验收已经完成，不再等待同范围批准。

## 1. 对象与摘要

方案 §2 中九个对象的字段保持不变。独立 `DESKTOP_AUTHORITY_SCHEMA_VERSION = 1`；旧 desktop session、worker、ApprovalGrant 和 ResumeInput 协议不变。所有新增对象拒绝未知字段，标识符复用 `validate_product_id`（最多 128 字节），摘要复用 `validate_product_digest`。每个控制请求编码最多 16 KiB。目录最多 64 条，label 最多 128 字节且禁止控制字符；目录 TTL 最多 30 秒，consent/grant TTL 最多 15 分钟。

字段类型：`owner_ref: VerifiedPrincipal`、`authority_epoch: AuthorityEpoch`、`run_ref: RunId`、`approval_ref: ApprovalId`、`action_ref: ActionId`、`plan_digest: PlanDigest`、`normalized_action_digest/catalog_digest: SchemaDigest`、`affected_resources: Vec<ResourceRef>`、`capability: DesktopCapability`、`policy_version/tool_schema_version: Version`、时间为 `Timestamp`、revision 为 `u64`、其余 ref/operation 为受验证 String。受影响资源遵守 WorkerJob 的校验规则且不得重复。pending 仅支持已有 Computer 操作，不能以该授权启用交互 PTY 或引用输入。

`DesktopConsentStatus` 为 `Prepared/Committed/Revoked/Expired/Unknown`；`DesktopGrantStatus` 为 `Issued/Consumed/Revoked/Expired/Unknown`。两者 snake_case wire 编码。`DesktopGrantView.binding` 是原 `DesktopSessionBinding`，只向认证 Core 返回；其余状态不表示动作成功。

四个新摘要域沿用已批方案：`forme.product.desktop.catalog.v1`、`forme.product.desktop.pending.v1`、`forme.product.desktop.consent.v1`、`forme.product.desktop.grant.v1`。用现有 canonical serializer 对 `(domain, DTO)` 摘要；catalog 的自身 digest 字段在计算时不参与。pending_ref 是原待审批动作的稳定标识，重注册必须逐字段相同，不能靠重用 ref 改 plan。所有预校验先于持久化或 helper 调用。

## 2. Host 独立事实源

`DesktopAuthority` 位于 execution 的 installed 域，拥有独立 OS 文件锁及有界、带 schema/sequence/previous digest 的 append-only journal。单 record 最多 128 KiB，总量最多 64 MiB；超过上限明确不可用，不截断历史。首次绑定 installation/owner/epoch，重开须逐项核对。追加先完整校验下一投影，写入并 sync 成功才发布；任何写入结果不明使 writer poisoned，禁止后续写入与执行。残行、错序、错 hash、错 schema、重复/不合法状态迁移均拒绝重开。

目录由只读 OS adapter 观察，不调用 DesktopHelperAdapter::observe（它可能启动 helper）。Host 在内存保留每份未过期目录及对应 OS 原件；session_ref 绑定该目录、真实 user/session/generation 和固定 desktop。`catalog_revision` 等于读取时的 authority revision（首条身份绑定为 revision 1），用于 consent 的 expected_revision；新读取可使旧目录失效，但不改变已提交 consent。Windows 的桌面引用固定为 `desktop:windows-default`，原生 launch 仍选择 `winsta0\default`；引用不是路径，不在产品 ID 中嵌入反斜杠。重启丢失目录后旧目录不能新建 consent；已提交 consent 的 OS 原件在受保护 journal 中保留，新的 grant 和 admission 仍须重新观察核对。

事实分为 pending registered、consent committed、consent revoked、grant issued、grant consumption intent。认证/CSRF/近期重认证由原入口与 Host 分域验证；Web 不传可信 principal/OS 身份。consent 和 revoke 共用 authority revision CAS；同 operation 同义返回原结果，异义拒绝。查询不写事实、不启动 helper、不改变原结果。过期状态是原记录与当前时间的投影。

一个 pending 至多一个有效 consent，一个 consent 至多签发一个 exact job grant。grant request 的 execution.desktop_session 必须为空；比对 pending 的 owner/epoch/action/plan/normalized digest/resources/capability 与原 job/permit/execution，禁止先提供自签 binding。Host 生成 grant_ref，绑定原 worker nonce，最晚截止为 consent、job、permit 与 now+15min 的最小值。不得为了延长超时而改 job 或另签 grant。

## 3. 消费、互斥与 crash cuts

Host 共享一个 authority handle 给 owner broker 和 WorkerExecutorHost。consent/issue/revoke/admit 都在 authority 写锁下线性化；worker admission 还持有原 worker operations 锁，固定先 worker lock 后 authority lock，owner 操作不反向获取 worker lock。新桌面 dispatch 必须找到与完整 job/permit/execution 对应的原 issued record。先持久化 consumption intent，再进行原 worker admission；授权事务本身不启动 helper、不产生 ActionStarted。

| 崩溃切点 | 重启或原号查询行为 |
| --- | --- |
| pending/consent/grant 完整追加前 | 完整旧记录仍有效；残行或 sync 不明阻断 writer，不推断成功。 |
| grant 已签发、consumption intent 前 | 可查原 grant；只有原 job/permit/execution 可消费，不能换号重领。 |
| consumption intent 后、worker admission 前 | 原 grant 标为 Consumed；无原 worker 时状态 Unknown，禁止重新准入或创建另一个 grant。 |
| worker admission 后、Started 前 | 原 worker journal 恢复 Unknown；不启动 helper、不补做动作。 |
| Started 后、receipt 前 | 查询原 worker；无可信终态继续 Unknown，禁止第二次 dispatch。 |
| 原 worker receipt 后 | 先按现有 duplicate dispatch 规则返回原观察，不为重放启动 helper或重新消费。 |

消费意图是不可逆的零重派 fence，不将“尚无 worker”解释为未执行。撤销在新消费前胜出则阻止准入；消费后只报告已消费，原 worker 终态独立核实。锁屏/注销/generation 变化在签发与执行前均拒绝；已 Started 的不明不能改成未执行。认证失效、跨 owner、跨 epoch、错参数/nonce 和未知 capability 在 helper 前拒绝。

## 4. 接口、主脑与 Web

HostManagement 和 AgentExecution 采用已批方案 §3 的方法与四类 Web 路由，新增 trait 方法保留拒绝型默认实现。owner broker 验证其现有 Web configuration/OwnerMaintenance domain；Core 入口验证既有 AgentExecution credential 及 native peer。两域不能互换，新增路由仍使用既有有限 allowlist。

主脑使用方案 §4 的原 ApprovalWait/grant-once 流程：最终 policy/plugin recheck 后注册 pending，无桌面 consent 时不得发 ActionStarted；owner 提交 exact consent 后才批准原动作。恢复时再核对 final plan，WorkerActionBackend 保留唯一 attempt，再取原 grant 并通过既有 `with_desktop_session` 附加。获取结果不明只查原 operation，不自动重领。Web 只显示安全目录及 consent 状态，不显示 binding/OS token/nonce/helper endpoint。

Computer 设置与独立 profile 仍按[设置 v3 契约](installed-managed-settings-contract.md)接入。只有上述消费者、固定活动 release helper 和实际 placement 全部存在时才允许启用；配置保存不生成动作批准。Screenshot 输出另按已批结果方案实施，不借授权阶段伪报结果支持。

### 4.1 接线与只读核对细化

主脑与执行注册表共享同一个 `Arc<WorkerActionBackend>`，通过内部装配字段接入，不改变冻结 `ActionBackend` trait。受管 Computer 始终先进入原 ApprovalWait。恢复时先按 pending_ref 只读核对 consent；缺失、过期、撤销或查询不明均保留等待，不 resolve/authorize 原审批，不保留新 worker attempt。通过后执行既有 final plan/policy/capability/competence 重查，再以同一个 backend 保留唯一 job/permit 并申请 grant；原审批的 resolve、authorize 和持久消费均在此之后。父子 Harness 共享相同 backend。

为支持上述零副作用前置核对，AgentExecution 的既有桌面控制面细化一个只读方法：`desktop_pending_consent(&self, pending_ref: &str) -> Result<DesktopConsentView>`；Host 对应方法增加既有 auth/now 参数。它只查询本 installation/owner/epoch 已登记 pending 的原 consent，返回既有安全 view，不返回 OS 标识、grant 或新增执行权限。缺省实现拒绝。此为已批准桌面授权闭合范围内的内部读取接缝，不增加 Web 路由或改变原审批对象。

pending_ref 由 `(forme.product.desktop.pending.v1, reference, approval_ref, plan_digest)` 的 canonical digest 去除 `sha256:` 前缀后加 `desktop-pending:`；consent_ref 由 `(forme.product.desktop.consent.v1, reference, pending_ref)` 同法加 `desktop-consent:`。这些引用仅用于定位原件，绝不证明已授权。pending 与 consent 请求摘要仍覆盖全部冻结字段；不同 owner/epoch/plan 不可借引用绕过原件比较。一个 pending 不可被第二个 consent 替换。

内部有限路由：Configuration 为 GET `.../configuration/desktop/sessions`、POST `.../configuration/desktop/consents`、GET `.../configuration/desktop/consents/{operation_id}`、POST `.../configuration/desktop/revocations`，统一前缀 `/v1/host`；AgentExecution 为 POST `/v1/agent/workers/desktop/pending`、GET `/v1/agent/workers/desktop/consents/{pending_ref}`、POST `/v1/agent/workers/desktop/grants`、GET `/v1/agent/workers/desktop/grants/{operation_id}`。各自只接受对应 credential domain，不接受路径或查询通配。

Web consent/revoke 的外层请求为 closed `{schema_version:1, request:<原 DTO>, password:<write-only>}`，请求体总计不超过 16 KiB；`Idempotency-Key` 必须等于 request.operation_id。口令只用于 Web 的 exact request digest 重新认证，不传给 Host。浏览器先核实 consent Committed，再发送原 grant-once；原 Core 入口也独立核对，不信任浏览器流程。

backend 预留 attempt 与实际 dispatch 尝试是两个状态。申请 grant 不算 dispatch；只在首次实际执行前 CAS 标记 dispatch 已尝试，重复调用只查询原 worker。grant 获取不明后只能查询原 operation；不换号、不再次申请。取得 binding 后必须校验 owner/epoch/action/plan/nonce/capability/有效期与原 request，才能附加到 plan。

## 5. 验证

协议测试覆盖 closed schema/尺寸/期限、错误摘要与身份。真实 journal 测试覆盖 CAS、同号幂等、异义冲突、重启/双 writer、残行/篡改、撤销竞争和逐 crash cut 的零第二次准入。生产接线测试必须经正式 pending/审批与 worker backend，不能仅手工构造 binding。Windows 原生验收另外证明 SCM Host、真实登录用户会话、锁屏/注销、固定 helper/peer 和同 job receipt；组件通过不替代 IP11。

## 6. 独立受管 placement 与配置投影

Windows 安装侧新增内部 closed schema 1 `InstalledDesktopProvisioning`，包含原 `ManagedWorkerBinding`、scope 和有限 surface refs；它由已校验的 installation/worker/credential 布局派生，不能从 Web、模型或环境变量输入。endpoint、credential 和 epoch 与安装的 AgentExecution 域一致；独立 profile 为 `sandbox:installed-desktop-v1`，backend_kinds 仅 Computer。它不进入 File 的 AppContainer 程序注册表，桌面调用仍只能经过固定活动 release helper。原 InstalledFileProvisioning 的 profile、语义与序列化保持。

首个 placement 只承认 `workspace:default` 下的 `surface:windows-default`，表示 owner 随后选择的 Windows 交互默认桌面；这不是 SID、会话号或物理显示器 ID。输入坐标许可范围为每轴 [-32768,32768)，用于限制动作参数，不声称是实际屏幕尺寸。精确动作可使用范围内更小的 bounds；helper 执行时仍核对 OS 会话与真实可用显示区域。截图只接受能够映射到单个真实显示器的原 bounds，并受独立输出契约的像素预算限制；不默默裁剪或改变请求尺寸。

Host 的私有 ConfigState 11 增加 optional desktop placement；旧 2–10 的发布字节与快照不重写。仅新的 owner 保存可将安装 placement 纳入新 revision，旧客户端保存保留已存在字段。state 11 必须含 desktop placement，既有 managed 字段允许在 10 或 11；v3 公共设置形状不变。Host 在发布前将 enabled 的 scope/surface_refs 与这个原件逐项比较。没有安装 placement、固定 helper 或消费者时拒绝启用。

`ManagedBrainConfig` 增加 optional `desktop_placement`，由 Host 受保护配置发布；ProductionRuntimeBuilder 验证其 schema/profile/epoch/endpoint、与 managed Computer 设置的对应关系，并将专用 desktop binding 单独传至 capability assembly。能力只有配置显式启用且该 binding 可用才注册，不通过 File binding 的 backend_kinds 推断支持。Host 在注册 pending、签发及消费前再次读取当前启用配置并比较 profile、scope、surface、driver 与参数范围；禁用立即阻断新准入，原已开始 job 仍只核对原 receipt。

Web 在原审批卡内显示会话目录、重新认证口令及同一次 consent 的状态。pending_ref 使用 §4.1 的 canonical tuple 定位；目录变化要求重新读取，已提交或响应不明的操作只能按原 operation 查询。口令和目录不写浏览器持久存储。只有 Host 已核实 Committed 才解锁原“仅批准这一次”；拒绝原动作始终可用。原请求数据和批准 nonce 不因刷新查询而重新生成。
