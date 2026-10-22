# prd/24 安装与系统服务生命周期

## 0. 定位与承上

- 实现 [requirements/17](../requirements/17-installed-product-scope-decisions.md) P01–P04、P09–P11、P15，主验 IP01–IP03、IP10–IP16，参与 IP24。
- 架构为 [architecture/10](../architecture/10-installed-product-and-web-workbench-architecture.md) §1–4、§7–9；服从 canonical §7/9/12/24/26/28/29。
- 当前是**分阶段实施 PRD**：宿主底板、worker admission 与受注册入口的真实子进程基础已有代码，但不宣称安装器、原生服务或平台强隔离已完成。它不重开 M0–M5，不增加内核/crate/依赖边。

## 1. 职责与边界

交付完整原生安装包、长期服务、管理 broker、受限 worker、版本/配置/data 路径、升级恢复和卸载。用户无需开发环境，也无需开终端维持服务。

安装器构建为 Inno Setup `.exe`、签名公证 `.pkg`、`.deb/.rpm`；只向已验证的 OS/CPU 组合发布。进程、账户和路径直接使用 architecture/10 §1–2，不另造平行清单。

不实现大脑认知、不替代 Harness，不增加任意 root shell/远程运维平台。bootstrap/login 归 prd/25，业务页面归 prd/26。系统服务可在无人登录时运行；桌面 helper 是否可用是另一能力状态。

## 2. 全局位置与集成契约

| 接入面 | 本模块责任 | 下游约束 |
|---|---|---|
| `execution` | `forme-hostd`、worker、OS service/sandbox 适配与 typed system operations。 | 仅依赖 architecture/03 已允许的 protocol/policy；没有认知/独立 writer。 |
| `gateway` | Web/core 的原生 service host、状态与内置管理 client。 | 通过现有 Harness 外观进入 core，不新增 gateway→execution/store/config 边。 |
| `cli` | hostctl 操作入口经 gateway/protocol 发送维护请求。 | 无直接 DB 写和另一套 ProductionRuntime。 |
| `protocol` | 版本化产品 DTO 与安全 refs。 | 不新增 EventKind；brain 动作继续使用现有 ExecutionPlan/ActionResult。 |
| 安装/构建 | 原生包、签名、静态资源和 release manifest。 | UI 与所有执行文件绑定同一 release；来源/SBOM 审核。 |

消费 core health、drain、backup/restore/integrity 与执行 receipt 的正式接口；真实 Agent 动作仍产生既有 Action/Run/Failure/DataLifecycle 事件。安装/服务管理写独立 ProductManagementRecord，绝不复用 EventKind 假装脑事实。管理 journal 只能持有 operation/run 安全引用。

冻结 Harness/Store trait 保持原签名；本模块新增 transport/host 外观，不在安装脚本里 patch core 或改源码。任何不能通过既有边界落地的变化先更新 architecture，不得靠禁用 gate 通过。

## 3. 数据结构与类型

所有下列 wire/persistent DTO 带 `schema_version: SchemaVersion`；路径、版本、摘要与账户标识为校验后的 newtype，不用任意字符串代替命令。

| 对象 | 必需字段与语义 |
|---|---|
| `InstallationManifest` | `install_id, target_os, target_arch, release_id, release_digest, install_root, config_root, data_root, service_ids, service_identity_refs, created_at`。受保护，路径不进公共 API。 |
| `ReleaseManifest` | `release_id, target, binary_digests, ui_digest, managed_dependencies, config_read_range, schema_read_range, schema_write_range, minimum_upgrader, signature_ref`。签名覆盖完整规范化内容。 |
| `HostStatus` | `install_id_safe_ref, product_state, core_health, active_release, active_config_revision, acknowledged_config_revision, issue_refs, allowed_operations`。ProductState 只用 architecture/10 §3。 |
| `MaintenanceCommand` | `operation_id, idempotency_key, expected_revision, actor_proof, kind, normalized_payload, requested_deadline`。digest 由服务端计算。 |
| `MaintenanceOperation` | `operation_id, command_digest, revision, stage, original_release_ref, target_release_ref, data_candidate_ref?, backup_ref?, receipts, failure_ref?`。stage 使用 architecture/10 §9。 |
| `ProductManagementRecord` | architecture/10 §4 的共同字段；append intent/receipt，不能含 password hash、密钥/token/原始 SecretRef id。 |
| `WorkerJob` | `job_id, execution_plan_ref, plan_digest, authority_epoch, permit_ref, sandbox_profile_ref, workspace_scope, deadline, cancel_ref`。不携带无限制 env 或字符串程序路径。 |
| `WorkerReceipt` | `job_id, plan_digest, nonce_ref, observed_outcome, safe_result_ref, ground_truth_refs, started_at, finished_at?`。worker 自报仍按既有 trust 处理。 |

`MaintenanceKind` 只允许：Inspect、StartCore、StopCore、RestartCore、ValidateConfig、ApplyValidatedConfig、ConfigureLan、StageSignedRelease、Upgrade、RollbackCompatibleRelease、PrepareRestore、CommitRestore、UninstallPlan。备份创建委托 core/既有离线工具；不提供 ExecuteShell、WriteArbitraryFile、RunSql、SetOwner 或忽略 key 的开关。

安装目录与 owner workspace 的删除范围分别编码；`UninstallPlan` 默认 `preserve_owner_data=true`。任何额外删除都是显式 scope，不允许用路径为空或根目录表示“全部”。

## 4. 接口与签名

以下是新增宿主接口，不替换已冻结 core trait。`AuthenticatedMaintenance` 由 OS/服务端认证构造，客户端不能反序列化出可信身份。

```rust
trait HostManagement {
    fn inspect(&self, auth: &AuthenticatedMaintenance) -> Result<HostStatus>;
    fn plan(&self, auth: &AuthenticatedMaintenance,
            command: MaintenanceCommand) -> Result<MaintenancePlan>;
    fn execute(&self, auth: &AuthenticatedMaintenance,
               plan: BoundMaintenancePlan) -> Result<OperationRef>;
    fn operation(&self, auth: &AuthenticatedMaintenance,
                 id: OperationRef, after: ProductCursor) -> Result<OperationPage>;
}
trait NativeServiceManager {
    fn inspect(&self, id: BuiltinServiceId) -> Result<NativeServiceStatus>;
    fn apply(&self, change: ValidatedServiceChange) -> Result<NativeReceipt>;
}
trait IsolatedExecutorHost {
    fn dispatch(&self, auth: &AuthenticatedCore, job: WorkerJob) -> Result<JobRef>;
    fn probe(&self, auth: &AuthenticatedCore, job: JobRef) -> Result<WorkerObservation>;
    fn cancel(&self, auth: &AuthenticatedCore, job: JobRef) -> Result<CancelReceipt>;
}
```

同 key 同 digest 返回原 operation；同 key 不同语义返回 Conflict，零状态推进。expected_revision 不符拒绝；计划形成后目标 release、路径、service ids、data refs 不得被替换。execute 必须验证近期 owner/本机管理员凭据、digest、expiry、nonce 与当前状态。

查询不授予操作权。dispatch 仅接受已认证 core + exact permit，Web credential 不能调用；probe 只读，不能伪装成 retry。OS service adapter 只处理 manifest 中的内置服务，不接收任意 service name/path。

未认领时经本机证明产生的有限 setup 身份只可 Inspect 已识别数据/PrepareRestore 候选与完成 claim，不具有普通 owner-maintenance 权限；不能启动任务、替换 release 或改任意服务。此限制由 broker 检查，不能只在向导隐藏按钮。

### 4.1 Worker 控制面与受管配置（24.2 增量）

内部原型使用两个独立 loopback listener：`FORME_HOST_BIND`（默认 `127.0.0.1:8766`）仅提供 OwnerMaintenance；`FORME_AGENT_EXECUTION_BIND`（默认 `127.0.0.1:8767`）仅提供 AgentExecution。即使持有正确 token，调用另一个 listener 上的路由也被拒绝。两个域分别限制连接；该原型仍不是原生 IPC peer identity 或完整安装验收。

AgentExecution 路由为 `POST /v1/agent/workers/dispatch`、`GET /v1/agent/workers/{job_id}`、`POST /v1/agent/workers/{job_id}/cancel`。dispatch 使用版本化 closed `WorkerDispatchRequest { schema_version, job, permit }`，验证 exact job/permit binding；查询和取消只引用原 job，不创建替代任务。

`FORME_WORKER_CONFIG_PATH` 指向受保护的绝对 JSON 文件。`WorkerRuntimeConfig` 含 `schema_version, authority_epoch, journal_path, runtime_root, allowed_scopes, workspace_roots, launch_profiles`；每个 profile 含 `schema_version, sandbox_profile_ref, program, args`。读取上限 4 MiB；拒绝未知字段、重复 scope/profile、缺失映射、链接、路径别名或交叠。配置、worker journal、runtime、workspace 与注册程序不得互相成为可写覆盖目标；broker 另拒绝 worker journal 与管理 journal/凭据文件混用。结构检查不替代实际 ACL 和执行时身份验证。

默认配置加载**不启用普通进程 adapter**。没有已验证的平台隔离实现时，服务可提供维护与状态查询，但 dispatch 被阻断。开发者必须在前台显式传 `--development-process-workers` 才能使用受注册的普通子进程；SCM 服务模式永远不接受此降级。该开关不能由 JSON、Agent 请求或 profile 名开启，开发证据不得登记为安全沙箱或 IP10 PASS。

### 4.2 Core 停服接缝（24.2 增量）

`gateway` 增加 `GatewayLifecycleConfig`（版本、`drain_timeout`）与可注入可信停止信号的 `serve_environment_until` 外观；原 `serve_environment` 保持签名并委托同一实现。它仍创建唯一 ProductionRuntime，不是另一套运行时，也不新增 Web/Agent 停服路由。默认停止信号为前台 Ctrl+C；Unix 同时响应 SIGTERM。SCM/其他原生 service host 后续通过同一接缝提供 OS 控制信号，不能另设重启监管者。

收到停止信号或 listener 失败后，按同一单调截止时间：关闭 Gateway 工作准入与 scheduler pass 准入 → 停止 listener → 等待已经获准的阻塞操作和当前 scheduler pass → 等待 Accepted/Running run 与 session 执行队列排空 → 退出。准入与关闭在同一锁内线性化；HTTP timeout/请求取消不能提前释放仍在执行的阻塞操作名额。已经落盘的 Waiting/Unknown 保留，停服不把它们批量取消或改成 Complete。

开发环境通过 `FORME_SERVICE_DRAIN_TIMEOUT_MS` 配置排空时限，默认 30 秒，有效范围 1 毫秒至 10 分钟。超时或无法核对状态时返回明确错误；daemon 以非零状态退出，保留原事件供既有启动恢复扫描处理，不重做动作、不生成虚假停服成功。该接缝只证明核心进程的受控停服，不替代升级前备份/checkpoint、服务注册、独立身份或原生安装验收。

### 4.3 服务注册与受管 Host 启动（24.2/24.3 增量）

三个 Windows daemon 的管理命令仅接受 `--install-service --config <absolute-path>` 或 `--uninstall-service --config <absolute-path>`。注册/移除共用 execution 的内置服务适配；gateway 仅经既有 harness 依赖重导出调用，不增加 crate 边或建立脑实例。程序取当前 executable，账户取 BuiltinServiceId 的固定账户矩阵；调用者不能指定账户、服务名或替代程序。重复注册必须核对程序/完整参数、配置路径、账户、依赖、启动类型与有限故障恢复策略；同名但不同配置拒绝，不静默接管。移除前同样核对归属，再等待观察到 Stopped 才删除注册；查询失败或排空超时保留注册与数据。

host/Web 自动启动；core 在认领与配置通过前按需启动，注册时允许其配置尚未创建，实际启动仍须完整校验。SCM 故障恢复为 5/30/120 秒延迟后停止自动重启，24 小时重置计数；不运行任意恢复命令。原生包负责服务账户/ACL 与配置预置，这些命令不独自证明安装可用或隔离合格。Linux 单元使用真实支持的 Type=exec，不伪报未实现的 notify 就绪；进程状态与业务 readiness 分开探测。

`forme-hostd --config <absolute-path>` 加载有界、closed schema 的 HostServiceConfig：安装/release/本机 target、独立 loopback 双域地址、管理 journal、两个私有 credential 文件及可选 worker/bootstrap 配置。路径无链接/别名/交叠，凭据按私有文件读取；服务模式不能读取 shell 环境作为回退，不能开启开发进程 worker。前台无参数环境模式仅保留开发兼容。此配置不是脑事件；坏配置不能创建 journal 或替代脑。

### 4.4 Windows 首次安装的权限分区与失败边界

首次生成配置时，发布文件为 `config/core/core.json`，主脑认证 fence 与配置加载确认分别为 `brain/runtime/core-auth.sqlite`、`brain/runtime/core-config-ack.json`。主脑对前者只读、对后两者可写；不能为运行态写入而放开 `config` 或 `host-state`。core 配置的原子替换与模型凭据的不可变 revision 通过各自目录继承主脑读取权限，不能只为当前文件临时加 ACE。配置 journal/revision 仍由 Host 独占写入；与发布文件同目录的非 secret 管理记录可由 Brain 只读，不构成另一个写者。

Windows 包使用随包、固定入口的 PowerShell/.NET ACL helper；它不是服务端 shell API，不接受任意账户、服务名或路径。当前首次安装仅允许 Program Files/ForMe 与 ProgramData/ForMe 的精确默认根。SYSTEM/Administrators 为管理写者；Brain/Web 使用注册后的固定虚拟账户 SID。程序目录两者只读执行；Web 只可写 `web-auth` 和自有日志；Brain 只可写 `brain`、`workspaces` 和自有日志；host-state、backups、管理凭据不授予两者。`core-owner.token` 与 `agent-execution.token` 只额外允许 Brain 读取，`core-web.token` 允许 Brain/Web 读取；`model-credentials` 只额外允许 Brain 读取。父目录只给必要的遍历/属性查询，不将权限继承到其他秘密。worker 不从这套规则取得任何权限。

安装顺序为：只读检查目标不存在/为空且无链接、已有服务与安装 → 原子创建受保护根目录（先于任何秘密写入）→ 解包 → hostctl prepare → 顺序注册三个服务但不启动 → 固定 SID 解析与完整 ACL 设置/回读 → 最后才允许启动 Host/Web，Brain 仍等认领/config。每一步检查退出状态；失败停止后续步骤，保留受保护现场，不自动重做未知副作用或宣称安装成功。helper 拒绝 symlink/reparse、非规范路径、意外文件和非预期 ACL；安装器不静默修复或迁移旧安装，也不替代后续升级/恢复事务。

既有 `prepare-installation` 重开必须读取并校验原 `product-template.json`，沿用其中的 core/ack/fence 路径；旧布局不自动搬移。旧布局转为原生服务仍需显式、停服的数据/权限迁移。ACL descriptor、真实临时 NTFS 继承/原子替换、安装控制流与实际服务账户访问拒绝分别取证；前几项通过不能标记 IP01/IP10/IP24 或管理员 SCM 演练通过。

### 4.4.1 POSIX 原生权限与后续发布

Linux/macOS 使用同一有限权限矩阵：管理域与 backups 仅 root；brain/workspaces 仅 Brain；web-auth 仅 Web；config/keys 父目录只允许遍历。`core-owner.token`、`agent-execution.token` 仅额外授予 Brain 组读取；`core-web.token` 由专用 bridge 组供 Brain/Web 读取，该组不拥有其他管理凭据。Web 不能读取 Brain/owner/model/management secrets，也不能写 core 配置；Brain 不能写管理配置或全局 keys。Brain 自身内容 vault 位于 brain 数据域，不需要全局 keys 写权限。

`config/core` 与 `keys/model-credentials` 由 root 拥有，Brain 组只读并设置 setgid；后续 Host 创建的共享 revision 目录继承同组与 `2750`，共享文件为 `0640`。配置日志及内部 state revision 继续 root 私有。发布前核对实际继承 gid/父目录模式，再给新 inode 最小读取权限，原子替换不能让下一个 core 配置退回 `0600`。没有安装器明确设置的 setgid 共享父目录时，开发路径仍保持 `0700/0600`，不全局放宽文件权限。

Linux Host 的 mount sandbox 明确允许受保护 config、model credential、Host journal、release staging、backups 与自身日志写入；Web/Brain 只写各自数据与日志。Brain service 预先 enable，但首次安装不 start，`ConditionPathExists` 在认领配置前阻止启动。卸载必须先停止 Brain，再停止 Web/Host，检查实际 inactive 后才移除程序；失败保留现场，不能以 `|| true` 忽略。macOS 通过原生 bootstrap/bootout 管理 Brain，首次安装不 bootstrap Brain；不能把“plist 已安装”写成进程已运行。账户、ACL 与实际拒绝仍按原生 IP 场景取证。

### 4.5 签名发布落盘与卸载范围的执行细化

`HostManagement` 与 `NativeServiceManager` 保持既有实现签名。受管 Host 可显式加载 closed、版本化的 `ReleaseStagingConfig`，只含 installation 引用与管理员预置发布公钥；缺失配置时不开放发布操作。它不从包内导入信任根，不接受来自 Web/Agent 的文件路径。

发布源固定为本 installation 的 `host-state/release-inbox/<release_id>/`，其中 `manifest.json`、原始 64 字节 `signature.ed25519` 与 `content/` 分离。`StageSignedRelease` 在 plan 前和 execute 前各验证身份、目标、签名、完整闭合文件集及 command 的 release digest。执行必须先持久化 Checking intent，再在 releases 同卷创建 operation 专属新目录，逐文件有界复制、sync、复核、原子更名，最后保存独立不可变 receipt；既有 release 绝不覆盖。中断现场保留，只查询原 operation，不重新复制或删除重试。staging 成功仅证明发布文件就绪，不能冒充 Upgrade、服务 image 切换或 IP 原生验收。

`UninstallPlan` 仅输出绑定 installation manifest 的确切服务/程序移除范围与全部保留路径，结果存于 host-state 的不可变管理记录，journal 仅记摘要。它不执行卸载、不删除数据，也不将 preserve=false 转换成数据清除权限。阶段/终态查询和同义幂等重查不重新创建文件。

离线构建使用 `tools/sign-product-release.mjs` 将已审查 bundle 转为 `manifest.json + signature.ed25519 + content/`；明确输入兼容范围与独立私有 Ed25519 key 文件，输出/来源不得包含私钥。`share/forme/web` 映射为签名内容中的 `web`，文件摘要与序列化次序遵循 protocol 的 Rust `ReleaseManifest::digest`，不另造规范化算法。受管 Host 的独立只读入口 `--verify-release <absolute-directory> --trust-config <absolute-file> --target <wire-target>` 复用同一个 verifier，既不创建 journal，也不进入 SCM/ProductionRuntime。签名回执记录原生包签名/安装未验证；发布包内自报公钥不能成为安装信任根。原生 Authenticode/Developer ID/仓库签名与真正安装验收独立完成。

### 4.6 在线与离线 store 互斥、密钥库访问

ProductionRuntime 在打开持久 store 前取得同目录 `<store-stem>.access-lock` 的 OS shared lock，锁绑定实际数据库并由 authority guard 及其所有 clone 持有，直到最后的 Harness、BrainServices、heartbeat 或独立 authority guard 释放后才结束；不能随 builder、HTTP 请求或 runtime facade 结束提前释放。多个在线候选仍由既有 authority lease 决定唯一写者；shared lock 不替代 epoch/nonce/恢复扫描。离线维护工具必须先取得同一文件的 exclusive lock，随后才可检查、备份、恢复或迁移；冲突时零 brain/keys 写入，不靠“服务已停止”推断独占。锁文件不删除、不重命名，进程退出由 OS 释放；换路径维护同时绑定源与目标，拒绝链接/别名。没有这条锁的历史进程仍须停服和 SQLite 占用检查，不能宣称新锁能约束旧二进制。

开发密钥库的显式本机权限准备保持兼容；受管构造沿用安装器设置的目录 ACL，不读取交互 USERNAME，也不执行 icacls 改写固定服务 SID。只读 existing 构造不得创建目录/密钥、调整 ACL 或销毁密钥；缺 key、破损 key、链接、撤销标记均拒绝，不自动生成替代材料。离线检查先用只读 resolver 验证 owner/解密/replay，实际迁移或新路径恢复另有绑定的写入操作。库接口变化为 additive，不改 ContentKeyResolver 与既有 store trait。

### 4.7 本机离线数据外观与新路径恢复

旧明文迁移的 plan/拒绝路径继续保持零文件创建：已有 access fence 时取得原锁，没有 fence 时不补建；Windows 以原库拒绝写/删除共享 handle 防止新写者，Unix 的新运行时同时取得原库 advisory shared lock，旧源检查持 exclusive lock，避免首次建立 fence 的竞态。非参与旧程序的停服要求仍适用，不修改旧迁移的计划/审批协议。

本机命令沿 `forme-hostctl → Gateway OfflineCoreMaintenance → Harness OfflineBrainMaintenance → Store OfflineBrainSource`；不为 execution 增加脑库依赖，也不构造 ProductionRuntime。调用者必须已有受保护 config、数据库和原 vault 的 OS 访问权；该命令不是提权代理或匿名 HTTP 接口，不替代 Host 的 owner 重新认证和生命周期事务。

离线配置使用受保护 CoreServiceConfig 的封闭存储投影：校验安装/schema/revision、owner/workspace、store/artifact/key 路径与隔离，不加载模型凭据，不要求模型、scheduler 或 transport 配置可运行。未知顶层/brain 字段和错误存储身份仍拒绝；坏模型端点或凭据不能挡住灾难恢复，也不能借维护投影启动 core。

`inspect-data --config <absolute-core-config>` 只读检查；`backup-data --config … --target <new-database>` 创建新备份；`verify-data --config … --source <backup>` 验证；`plan-restore-data --config … --source <backup> --target <new-database>` 保存计划；`restore-data --config … --source <backup> --target <new-database> --approve <prepared-digest>` 执行一次。路径只来自本机操作员，不接受网页传入的任意路径。

已有源先拒绝 SQLite WAL/SHM/journal 与旧写者，再用 immutable read-only 连接检查受支持 schema、owner/store identity、integrity、每 run 连续性、全部已知内容列的保护与行绑定、key history、实际解密和投影 replay；检查不得恢复或初始化原库。备份沿用既有 BackupManifest，流式计算摘要，原 key 不打包。目标必须全新、同样持 exclusive fence，部分失败保留现场；不覆盖、不删除后重试。

Windows 额外保持原库只读、拒绝写/删除共享的文件 handle；POSIX 对非 WAL 源保留 SQLite writer reservation。POSIX 冷 WAL 源不执行会创建 sidecar 的 BEGIN IMMEDIATE，使用 OS access fence、原文件摘要与冷源检查；不遵守 access fence 的历史二进制必须先经本机服务管理停服，不能声称新锁约束任意旧进程。新运行时和离线流程间的互斥由共享同一 OS 锁保障，原生平台另行验证。

恢复计划绑定源文件、manifest、owner/store、确切目标和 15 分钟有效期，执行在校验前和目标创建前都检查有效期。计划与回执以 create-new、sync 写入 `<target>.restore-plan.json` 和 `<target>.restore-receipt.json`，只包含摘要与结果，不包含 key 或路径。成功回执的 `authority_switched=false` 是明确边界：候选数据已验证，不表示 Host 已完成 active data/config 切换、旧 authority/审批 nonce fencing 或原生恢复验收。

### 4.8 恢复后的执行授权隔离与活动数据绑定

恢复是历史事实回到本机，不是历史行动权限重新生效。备份可能早于已经发生的外部副作用；不得把备份里的 Waiting 当作“尚未执行”，也不得批量改为 Aborted/Complete 来清理现场。

离线恢复在新路径完成事件/解密/replay 验证后，必须封存恢复边界，再生成最终 manifest/receipt。边界是 store 持有的 authority companion metadata，含 schema、代次、原 owner/store、源数据库摘要、批准的恢复计划摘要、前一边界摘要和每个 run 的精确 stream_seq 水位。它不记录安装操作阶段、口令或模型秘密，不增加 EventKind、不改变 EventStore/Harness trait，也不是另一个脑事实源。SQLite 存储格式由 9 升到 10；显式支持旧 9 的只读检查，初始化时原子添加元数据表，未知或额外 schema 仍拒绝。现有历史事件和内容 key 不重写，authority lease 不通过删行或伪造时间失效。

运行时与 store 执行边界共同 enforce：

- 边界之前存在的 run 不再取得 model/tool/approval resume 权限；旧 idempotency key 可以查询原 receipt，不能重新 drive。最终回答与证据保持只读可见。
- 旧 prospective/scheduled intention 不再 claim/履约；仍可查询和取消。新 owner 命令需新 intention/plan，不能仅延长旧 TTL 复活授权。列表标记需重新确认，不能伪报已完成。
- 历史 route 不恢复 child、不 retry 或续段；项目 charter/资料保留，owner 重新规划后形成新 route。后台可反思历史形成候选/建议，但不能把旧授权转换成新执行。
- 原 Unknown、ActionStarted 无完成和未决 remote/worker receipt 保留为待核实。只读 probe/证据核对不产生第二次执行；历史 nonce/lease 不能批准新动作。新动作继续走同一 Harness、最终 plan recheck 与 one-shot permission。
- 元数据摘要、owner/store、水位/代次或期望边界不匹配则拒绝启动/写入；不忽略坏边界回到无隔离状态。restore 之后再次备份/恢复保留并延长边界链。

活动数据是 Host 配置事务的一部分：持久 state 包含可选的已验证 data binding（数据库、原 vault、owner/store、候选/恢复边界摘要），与模型等配置共用 revision/CAS、immutable staging、intent/commit 和 acknowledged revision。旧 state 只显式兼容读，下一次事务写新版本；保存模型或重启不能丢失 data binding。发布的 core config 明确携带 content_key_directory 和期望恢复边界；不能根据新文件名创建新 vault。普通浏览器 patch 不接受路径或自报验证材料。

完整 CommitRestore 仍必须经过 Planned→Validated→Quiescing→BackedUp→Switching→Checking→Committed：停止准入/排空与独占 fence、验证当前数据并留备份、重新验证候选与 keys、提交活动指针、核对同一 owner/store/边界与 config ack，最后才开放新任务。切换/回执不明保持 RecoveryRequired，查询原 operation；不能换号重做。Web 会话撤销和旧执行隔离都完成前，不报告恢复已可继续工作。离线复制或本节单独通过不等于 Host 生命周期/IP14 已通过。

验收同时覆盖：旧审批零模型/driver 调用，旧 intention/route 零履约/child，Unknown 未洗掉，原回答/记忆可读，新 owner 请求可按新计划执行，重复恢复链连续，篡改/缺失期望边界拒绝，保存模型后活动路径/vault/边界不变，以及切换中断不得退回原路径或静默执行旧任务。

## 5. 行为、状态与并发

### 5.1 安装与启动

1. 校验 OS/CPU、包签名、磁盘、目标路径/ACL、端口；发现旧 installation/store 时进入接管路径，不覆盖。
2. 创建 release staging，逐文件校验；建立服务账户与私有 IPC；设目录权限后原子提交安装 manifest。
3. 先注册 broker/Web/core；启动 broker 与 Web。未认领/配置不全时 core 保持等待状态，不循环报错重启。
4. 由 prd/25 完成认领、配置与 doctor 后启动 core；core 取得唯一 store 锁，恢复扫描，核对 composition/owner/config revision 后报告 ready。
5. 在安装器的交互用户会话打开浏览器；headless 给本地访问指引，不从 root/SYSTEM 服务启动用户浏览器。

同一 InstallId 的安装/升级/恢复/卸载共用一个 exclusive operation lock；core 数据另有独占 writer lock。新进程争锁失败是明确 AlreadyRunning，不自动换 store。OS 重启根据 journal/active pointer/release checksums 恢复操作，不根据最后一条 UI 文案猜状态。

### 5.2 常驻与停服

OS 监管配置必须有延迟/退避/崩溃次数阈值，超过阈值进入需诊断状态，不无限高频重启。core crash 后先恢复 Unknown，再按既有 tick/route lease 规则恢复；不重放未确认工具副作用。

正常 stop/upgrade：关闭新 admission 与后台 claim → cancel/drain 到已知结果或 Unknown → 一致性 checkpoint/backup → 释放 writer → OS 停止。Web 关闭、退出和连接超时都不触发此流程。OS shutdown 截止不足时保留可恢复 journal/未知态，不能写假 Complete。

### 5.3 工具与会话隔离

worker 使用独立低权限身份启动；最小 env/handles，仅装载本 plan 的 workspace、临时目录、网络/credential scope。同一执行账户的不同 job 也须互相隔离，不能读写兄弟 workspace/credential。Windows Job Object/账户 ACL、Linux systemd sandbox/cgroup/账户、macOS 可用的账户/文件与进程限制必须逐平台验证；进程组本身不证明文件/网络隔离。不可满足强制限制则该 backend blocked，不能回退到 brain 用户执行。

会话 helper 仅在 owner 指定的 OS 会话启动，受 grant/TTL/会话 liveness 约束。OS 注销后撤销桌面执行能力，broker/core 仍常驻。会话 helper 不能获得私有 keys、brain DB 或安装管理 API。

特权动作经 Harness 最终 recheck、ActionStarted 与 one-shot 授权进入 broker；broker 还检查本地 typed-operation allowlist/permit binding。接收后失联只查原 job receipt；核心死亡、超时或 worker 无终态均保留 Unknown，不生成第二 job 盲重试。

同一 worker host 串行化 admission/start、probe/receipt 与 cancel/receipt 的状态迁移，防止并发请求在写日志与启动/终止进程之间交错。取消回执必须保留在 journal 投影中，后续查询和重启重放均返回一致的 `Cancelled/Cancelled` 或 `Unknown/Unknown`；不得返回“已取消但 outcome 为空”。重复查询/取消/dispatch 不能重复启动或终止进程，终态不得被晚到的 receipt 覆盖。

### 5.4 升级与恢复

签名/兼容性通过后进入 architecture/10 §9 的事务阶段。写 intent 在前，有限动作与 receipt 在后；分阶段 fsync/atomic replace，不能依赖仅内存标记。切换前一致性备份必须已有可验证 manifest，切换后 smoke 还须证明同一 owner/store/event continuity。

Web/broker 自更新也由同一安装事务和 OS 管理；升级器先保留可运行的前一版，Windows 文件占用标记 PendingReboot 或受控重启。回滚需要 schema read/write compatibility；不兼容时只允许恢复新路径候选。每次 authority 切换失效旧 execution leases/nonces，恢复扫描不重发已发生动作。

恢复必须验证 backup checksum、schema、事件连续性、key availability 和 projection replay；`CommitRestore` 绑定已验证候选 digest/owner/store/current revision。维护期间核心 writer 关闭，restore 不覆盖原库。跨机恢复需要独立 key recovery 材料，缺 key 显示不可解密。

### 5.5 卸载

先生成明确范围清单，默认保留数据/备份/workspaces。排空、停止服务、移除注册项/firewall/session helpers，再移除本产品 program releases。用已解析 manifest 的精确路径，不跟随外部 symlink 递归删除。保留可读恢复说明，告知哪些凭据/恢复材料须保管；数据清除使用独立授权与 DataLifecycle 流程。

## 6. 错误、安全、权限与审批

- 安装/OS 故障以 `PlatformUnsupported / SignatureInvalid / PathUnsafe / PortInUse / StorageUnavailable / ServiceUnavailable / RecoveryRequired / SchemaIncompatible / KeyUnavailable` 作为宿主错误码，给 issue_ref、可重试性与可执行修复，不包含原始秘密。
- Agent 相关失败映射 canonical FailureTaxonomy，并由 core 写 FailureEvidence；安装前的 OS 故障只进产品 journal，不为它伪造 Run/认知事实。
- maintenance credential 只能触发绑定操作；AgentExecution credential 不可安装/改配置。两者的进程 peer identity、IPC endpoint、audience 和 nonce store 分开。
- 防 symlink/reparse-point、文件置换、staging path traversal、签名后替换、低权限账户修改 service image 等 TOCTOU；打开/校验后的对象身份在执行前重查。
- 不提供关闭 policy、解锁任意 root shell、清空 store 继续启动、忽略 unknown 或绕过 OS 隐私权限的恢复按钮。

## 7. 配置与可观测

宿主配置：`installation.{target,paths,services}`、`service.{startup,restart_backoff,crash_threshold,drain_timeout}`、`update.{source,channel,maintenance_window}`、`executor.{sandbox_profiles,session_bridge}`；有效值带来源/revision。端口/账号/model 配置归 prd/25。

状态页报告各 service 的 OS 状态、ready/probe、版本、config ack、能力可用性和安全 issue；日志轮换、磁盘低水位、journal/store 损坏必须可见。诊断导出默认仅版本/摘要/错误码/安全 refs，敏感配置/原文/keys 不打包。

管理进度使用 `product_seq`，脑任务使用原 `stream_seq`，不能把两者混排重放。所有操作返回稳定 operation_id，以便页面断线重查。

## 8. 实施步骤与顺序

1. 固定安装/release/host DTO、路径与账户矩阵；建立独立 product journal 和幂等/CAS。
2. 完成 service host/OS manager adapter，先证明无配置也可启动 Web/broker 与 sole core lock。
3. 完成原生包、签名验证、最小权限和 headless 路径；与 prd/25 对接认领/config ready。
4. 实现工具 worker/会话桥与双域 broker；逐平台验证文件/keys/服务保护边界。
5. 实现排空、备份、新路径恢复、升级回滚和默认保留数据卸载；加入断电/进程杀死/磁盘失败点。
6. 按实际原生目标收集 IP01–03、IP10–16；未测试平台不入支持清单；最后与 prd/25–26 执行 IP24。

不得把服务注册/worker 隔离留在最终安装阶段才发现不兼容，也不能先发 root 全权限版本再补治理。

## 9. 测试

单元覆盖 manifest 校验、path identity、release compatibility、journal fold、命令异义幂等冲突、nonce replay、崩溃恢复阶段。集成用自建有限副作用 fixture，独立文件/计数器证明一次且仅一次，并检查原 Action 事件与 Unknown 状态。

IP01–03、IP10–16 的 fixture/正反断言以 requirements/18 为准。每个原生目标至少验证：未登录冷启动、杀 core/杀 Web、双启动、升级中断、损坏备份/key 缺失、默认卸载保留数据。检查服务和 worker 的实际账户与文件访问拒绝；仅在配置文件写低权限名称不算隔离通过。

不以 mock service manager 代替 SCM/LaunchDaemon/systemd；mock 只验证算法。Windows/macOS/Linux 与每个 CPU 的 evidence 分开登记。变化涉及 backend/store/runtime 时复跑 S1–S99、相关 C 门、strict clippy 与 release/compliance gates。

## 10. 验收

- 原生签名包从无开发工具系统完整安装，地址/服务/数据路径稳定，后台无需浏览器或桌面用户登录。
- 唯一 ProductionRuntime/writer、core-offline maintenance、真实隔离与特权审批有正反证据。
- 升级/断电/恢复/卸载遵守幂等、未知副作用和数据保留契约；无 key/不可兼容时诚实阻断。
- 主验 IP 全部有可核对 evidence；未完成项明确标记。通过本模块不等于 prd/25–26 或全产品已完成。

## 11. 原创性与合规

OS 服务机制按公开 SCM/launchd/systemd API 原创适配，不移植 OpenClaw/Hermes 的脚本、提示、错误或目录。安装构建器、签名工具、平台安全 wrapper 与 managed dependencies 均固定版本/source/license/copy_risk/notice，审计包含随包分发的 runtime/安装片段；不预设生成包免除第三方义务。

`research/upstreams/**` 不进 build。新增直接依赖、原生包依赖和签名流程在实施时更新依赖清单/NOTICE/SBOM；本次仅设计，未引入任何新依赖。每个提交前合规门仍必须通过。
