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
