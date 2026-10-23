# 独立部署与 Web 产品化实施记录

本记录承接 requirements/17–18、architecture/10、prd/24–26。它只记录新的产品化施工与证据，不改写 M0–M5、C1–C24 的历史验收；宿主组件测试不等于 IP01–IP24 或安装包验收。

最新施工分支：`shu/installed-product`，当前实现提交 `a53557f`（项目监管与工作空间后续调整）；此前 `95b9d71` 为运行待决 Inbox 投影，`dd76f20` 补齐改密码操作幂等绑定，`56dbed1` 将会话 workspace 绑定到后续正式 run 上下文，`69ee763` 增加 owner 对运行未知结果的持久裁决与恢复。此前 `29a1790` 补齐会话产物历史，`e36b5b9` 补齐恢复候选工作台/发布重新认证；再前为 `5b99ac5`（恢复执行边界与 Host 提交准入）、`b16997c`（原生打包入口/POSIX 权限）、`92459a2`（离线恢复候选）、`20faeac`（签名暂存与维护）。验证日期 2026-10-24。主脑工作台 `477ef52`、权限分区 `29984a4`、配置闭合 `01b77f9` 为历史组件。本分支尚未替代 `main`；当前本机 shell 非管理员、未注册 forme 系统服务；未读取真实 owner/model secret。

## 1. 实施顺序与当前范围

2026-10-24 `a53557f` 完成项目监管与工作空间后续调整组件：[组件验收](installed-product-project-supervision-acceptance.md)。权威状态、监管 CAS、与后台 tick 的暂停串行、旧 ledger replay 兼容、工作空间控制与长期意图入口均已实现；统一源码门 Rust 851 passed / 0 failed / 5 默认 ignored，严格 Clippy/前端/合规门通过，8 个命名工作台集成场景通过。该组件不提升 IP19 或 IP24 状态；当前真实浏览器和原生服务仍未验收。

2026-10-24 基于 `a53557f` 重新编译 [Windows x64 原生候选](installed-product-windows-candidate-20261024.md)：17-file development bundle 与 Inno 安装器编译通过，SHA-256/签名/安装状态均有独立记录。候选明确 unsigned、uninstalled、`release_ready=false`；IP01/IP02/IP03/IP10/IP24 仍未通过，本机当前非管理员且没有已注册的 forme 服务。

2026-10-23 `507cccf` 增加普通主动建议处置的 durable recovery：identity schema v9、owner/target/binding 唯一约束、刷新只读原命令、Unknown 保留及显式原号核实；维护 session fence 同步兼容 v9。当前源码 Rust 844 passed / 0 failed / 5 默认 ignored、strict Clippy、40 项前端测试、完整合规和结构门通过；前端依赖占用后的恢复过程及证据边界见[普通建议恢复组件验收](installed-product-proposal-recovery-acceptance.md)。真实浏览器与 IP20/IP24 仍未关闭，继续整体里程碑。

2026-10-23 的后续提交补齐了对话会话产物目录/历史（`29a1790`）、pending run 的 owner 裁决密文保存/原命令核实/重启恢复（`69ee763`），以及会话 workspace 关联到后续正式 run 的主脑上下文绑定（`56dbed1`）。绑定组件已通过 workspace 回归和 Web→core 正式 runtime 集成场景；仍不等于真实浏览器、多进程、系统服务或 IP01–IP24 联合验收。详见[会话 workspace 运行上下文绑定组件验收](installed-product-session-workspace-binding-acceptance.md)。

2026-10-23 提交改密码 mutation 的 key 绑定（`dd76f20`）：独立认证库 schema v8 绑定 operation key、owner/session、account revision/epoch 与 Argon2id 候选；异义复用在 reauth/状态更新前拒绝，候选到期/成功后清除。前端未知结果锁定字段并保留原 key；静态 bundle 已重新生成。最终源码通过 Gateway all-targets check/strict Clippy、前端 verify 33/33、代码质量与 compliance doctor。完整 workspace Rust test 在 C 盘/Windows pagefile 资源不足时无法链接；重构后的最终源码未获得 runtime test 证据，本组件验收仍待该门重跑。详见[改密码操作幂等绑定组件验收](installed-product-password-change-idempotency-acceptance.md)。

2026-10-23 新增[运行挂起恢复与裁决工作台组件验收](installed-product-run-recovery-acceptance.md)：pending 状态只读投影、owner 裁决密文 companion 与事件 claim 原子提交、模型调用未知终止、重启后原 command 显式核实，以及 Web 侧待决/未知说明均已实现。当前源码完整 workspace 回归与统一代码质量门通过；该组件不改变 IP01–IP24 状态，也不关闭整个产品化里程碑。

2026-10-23 新增[恢复候选工作台与已暂存发布认证组件](installed-product-restore-workbench-acceptance.md)：恢复的最终 manifest 安全投影、独立切换确认/原操作核实及 Host 重复候选拒绝已实现；发布只读重新认证同时核对 journal、验签和实际文件。统一源码门 Rust 817 passed / 0 failed / 5 默认 ignored，strict Clippy/完整合规/宿主与配置进程门通过，真实浏览器已核对原生能力缺失时禁用切换。完整 Upgrade/Rollback 和原生产品验收仍开放，整体任务持续推进。

2026-10-23 新增[恢复主脑的执行隔离与 Host 提交组件验收](installed-product-restored-brain-acceptance.md)：SQLite 恢复水位、旧 run/intention/route 权限隔离、原 vault/活动数据 CAS、固定离线 worker、完整 CommitRestore 阶段、Web 在线 session fencing 与 Committed 后工作准入已实现。全量 Rust 813 passed / 0 failed / 5 默认 ignored，strict Clippy/完整合规、宿主/配置统一门及实际进程恢复链路通过；服务阶段使用受控 adapter，未冒充原生 SCM。完整 Web 候选激活、签名 Upgrade/Rollback、隔离和其余整体义务继续实施。

2026-10-23 新增[原生打包入口与 POSIX 权限组件](installed-product-native-packaging-acceptance.md)：原生编译器固定参数/SHA pin/独立快照入口、首次安装权限和精确 bundle→signer 接缝已实现。当前 Rust 802 passed / 0 failed / 5 默认 ignored，打包与真实 Rust 验签 28/28，strict Clippy/完整合规/结构门通过。Windows Inno 已真实编译未签名开发候选，未运行安装；其他原生编译、签名公证、实际平台安装仍未执行。固定源码完整 M5 到 S99 因缺依赖清单行失败，补清单后须重新审计；不报告全门通过。恢复切换、升级回滚、隔离和其余产品义务持续实施。

2026-10-23 新增[离线数据维护与恢复候选组件](installed-product-offline-data-acceptance.md)：受管/只读密钥库、authority clone 生命周期 OS fence、本机 inspect/backup/verify/plan/restore，以及新路径正式 core 重启恢复回答已实现。离线存储投影不依赖模型配置可运行；实际临时进程链路已验证，活动数据切换与完整 Host 生命周期继续实施。总目标是全部已批准设计的实现与统一验收，组件完成不作为任务终点。

2026-10-23 新增[签名发布暂存与窄维护入口组件](installed-product-release-maintenance-acceptance.md)：实际 Rust 验签与同卷不可变暂存、owner 重新认证启停/范围计划、原 operation 核实和回执精确绑定已实现。786 项 Rust、前端 19 项、签名 4 项等源码/进程证据通过；独立 npm audit 受 registry TLS 失败影响。完整升级/恢复、强隔离与原生安装验收仍开放，不能据此关闭全产品里程碑。

| 顺序 | 交付 | 当前状态 |
|---|---|---|
| 24.1 | 宿主协议、安装清单、管理 journal、签名与兼容性校验、内置 OS 服务管理适配。 | 已实现并提交 `92802fe`；Rust 全量门、冻结 18-crate/99-kind 门、宿主专项门通过；不代表原生安装验收。 |
| 24.2 | 真正的系统 service host、受认证双域 broker、实际账户/ACL/逐 job worker 隔离与会话 helper。 | 已有受管 Host、三个 daemon 的 SCM host 与 typed 注册/移除；`29984a4` 增加新安装路径分权、固定 Windows ACL helper 和实际临时 NTFS 组件验证。真实服务账户 token/有效拒绝、job 强隔离/网络与会话 helper 仍未验收或完成，普通 worker 在生产继续 blocked。 |
| 24.3 | 原生包、受控停服/升级/恢复/卸载、签名供应链与原生平台演练。 | 已有可重复 bundle、精确签名/验签和三平台原生编译入口；POSIX 权限、新配置发布、首次安装/卸载失败边界已实现。2026-10-24 基于 `a53557f` 的 Windows Inno 候选已编译但未签名、未安装；其他原生编译器/管理员安装、完整生命周期事务、签名可发布原生包与跨平台验收未完成，详见[本轮候选报告](installed-product-windows-candidate-20261024.md)。 |
| 25 | 无配置初始化、旧脑验证/认领、账号/session、typed config/secret resolver、内部身份与 Web API/TLS。 | 认证/认领、受管 core 文件配置、私有模型凭据 resolver、签名会话桥与持久 nonce/fence 之后，`01b77f9` 完成模型配置 journal/staging/CAS、不可变凭据、重启恢复、静态校验、口令绑定启停与 core acknowledgement。实际 host/web/core 进程验证认领、配置持久化、幂等和正式 Run；已认证浏览器验证设置页。旧脑/备份接管、OS 恢复、LAN TLS、完整能力/渠道配置和整机初始化仍待实施。 |
| 26 | 五个 Web 工作面、正式流/审批/续接、精确前端来源审查与安装包联调。 | 已有 React/TypeScript/Vite 静态前端、初始化/登录、会话创建/列表/重命名/归档、对话/事件续接/审批/取消、附件加密上传与授权产物历史、pending/Unknown 原命令核实、会话 workspace 到后续 `SessionBound` 正式 run 上下文绑定、项目登记/路线规划/执行/取消、Charter 读写、权威项目监管快照与暂停/恢复、后续项目分段调整及长期意图取消、认知投影与处置、typed 活动运行 pending Inbox 投影、模型设置和受限维护。仍未闭合：剩余设置/维护/API、真实浏览器完整流程及 IP17–23/IP24 联合产品验收。普通建议 Unknown 刷新恢复已由 [普通建议恢复组件验收](installed-product-proposal-recovery-acceptance.md) 记录；组件门不替代原生服务和正式安装包证据。 |

24.1 是 prd/24 的第一笔组件交付；25 的账号/session 是另一笔组件交付，均不表示 prd/24、prd/25 或 prd/26 整体完成。后续按依赖推进，不能以组件测试提前宣布独立部署/Web 产品已可用。

最新增量和证据边界见 [主脑投影与认知处置组件验收](installed-product-brain-views-acceptance.md)；前批见 [安装权限分区组件验收](installed-product-service-access-acceptance.md)、[模型配置与主脑启停验收](installed-product-configuration-acceptance.md)，2026-10-21 历史见 [受管服务与工作台组件记录](installed-product-managed-workbench.md)。当前在 `shu/installed-product`，不是 main/发布版本；本轮没有读取真实 owner 密钥、迁移个人数据或注册系统服务。

### 最新主脑工作台（2026-10-22，`477ef52`）

工作台新增正式记忆/认知/收件箱投影、证据与血缘、受 governor 约束的候选处置及持久命令回执，AskToLearn 回答/拒绝/延后接回原学习路径。Unknown 不重复执行，同一候选刷新后换号也阻断；浏览器不能自填 evidence/actor/派生列表。详细见[主脑投影与认知处置组件验收](installed-product-brain-views-acceptance.md)。完整 IP20、实际浏览器/原生产品验收尚未完成；建议 Unknown 刷新恢复和真正的运行待决收件箱此前未闭合。

### 最新运行待决收件箱投影（2026-10-23）

收件箱现在从现有 `RunPendingView` 投影活动运行的 ApprovalWait、ToolInterrupt、Handoff、ResolutionUnknown 检查点，以固定安全摘要显示并导航回原 run；终态 Failed/Limited 不再被当作待处理。没有新增 `EventKind`、protocol 对象、状态机或持久事实源。Rust、前端、全 workspace 和 Web 工作台命名门通过；完整 IP20、真实浏览器刷新/断线、多 Web 进程及安装产品联合验收仍开放。详见[运行待决收件箱组件验收](installed-product-inbox-pending-acceptance.md)。

### 最新权限分区（2026-10-22，`29984a4`）

新安装使用独立 core 发布目录与 brain runtime，保持认领前新脑为空；重复准备读取原 template，旧配置/密钥/ack/fence 原地保留。Windows 首次安装 helper 只处理固定默认根与服务 SID，先保护后写入，设置并回读 ACL 后才允许启动 Host/Web；失败阻止后续步骤，卸载失败也阻止移除程序。Linux/macOS 服务启动路径同步，权限与原生目标验收并未因此完成。

最终源码门 PASS：146 个 Rust suite 汇总 752 passed / 0 failed / 5 默认 ignored；前端 14/14、bundle 9/9、Windows PowerShell 5.1/7 的 NTFS 组件各 7/7、strict Clippy、完整合规和冻结图门通过。结构门扩大到安装脚本，438 个文件、63 处历史规模债务未增长。宿主/identity/ingress/workbench/配置进程门最终串行复验通过；中间传输失败和证据限制保留于上述报告。没有管理员 SCM、Inno 编译/签名、真实 provider 或 IP01–IP24 新验收。

### 最新配置闭合（2026-10-22，`01b77f9`）

统一源码门 PASS：Rust 749 passed / 0 failed / 5 默认 ignored，strict Clippy、前端 14/14、bundle 7/7、依赖漏洞审计和资源可重复校验通过；结构门扫描 433 个文件、63 处历史规模债务未增长。默认参数完整合规门另跑 PASS。宿主 70 个命名场景、identity 18/18、ingress 8/8、workbench 8/8 与冻结 workspace 门通过。

配置专项 16 个 Rust 场景和 1 个正式 binary 综合进程场景通过，真实浏览器完成临时账号登录/配置检查/错误编辑恢复；真实 Run 经 ProductionRuntime 调用本地计数模型。原生 SCM 启停仅有 adapter 契约证据，本轮没有原生安装、真实 provider、完整 M5 长链重验或新的发布回执。详细命令、截图与未闭合内容见上述报告。

### 前批工程维护与认证基础（截至 `1140101`，历史记录）

- 维护规范：`docs/engineering/code-quality.md`，结构检查器/CI 为 `tools/code-quality.py` 与 `.github/workflows/code-quality.yml`。初始 66 个历史超限文件是待还债基线，不是“质量全部通过”的认证；新增超限或旧文件增长拒绝。当前数值以最新组件记录为准。
- Gateway 拆分：`b373aec` 保持 public API、事件、生产装配与测试名字不变，按控制域和 HTTP 处理职责拆开。`lib.rs` 从 3641 行缩至约 230 行，`server.rs` 从 2256 行缩至 599 行。其余历史大模块继续按风险逐批维护，不全库机械切片。
- 宿主拆分：`26ce4b4` 将 783 行的 `forme-hostd.rs` 按配置/凭据、HTTP、进程生命周期与 SCM 分开，入口剩 40 行并移除旧规模豁免。最终 workspace 688 passed / 0 failed / 5 默认 ignored，宿主专项 60 个命名场景通过；当前扫描 341 个文件、65 处历史规模债务，详见 [可维护性记录](installed-product-maintainability-progress.md)。`57e34b8` 同步修复 M4/M5 审计的旧文件定位，保留缺失实现/案例时的阻断。
- 认证：`12b3f5d` 只写独立认证库，不打开脑库。签名账号认领接缝、Argon2id、session/CSRF/epoch、operation-bound reauth、口令替换与重启持久性详见 [组件报告](installed-product-25-identity-foundation.md)。`52808df` 增加了本机证明、有限 setup scope、候选绑定、签名 claim、journal hash chain 和丢响应恢复，详见 [claim 组件报告](installed-product-25-bootstrap-claim.md)。当前没有安装服务或可登录的新网页地址。
- Web 入口：`49674b7` 增加 `forme-webd`、精确 loopback Host/Origin/CSRF、独立限流、有界连接/密码工作、登录挑战和已知认证 schema 升级；`52808df` 将 `/api/v1/bootstrap/prove|data|claim` 接到有限 setup client。原有 HTTP/进程门与新增 setup 单测通过；联合 hostd+webd 初始化链路和真实浏览器仍待验收，尚无业务页面或 LAN TLS。
- 上述实现仍在 `shu/installed-product`，尚未推送/合入 main；本轮未读取真实模型密钥或迁移个人数据。原生安装、五面 UI 和 IP01–IP24 仍未由本组件门验收。

## 2. 宿主底板的实现边界

- `protocol::installed_product` 是新增版本化产品 DTO；不增加脑 EventKind，不改已有脑协议的序列化结果。
- `execution::installed::ProductJournal` 是与 brain store 分开的管理事实源。OS 文件锁只允许一个管理 writer；`product_seq` 是它自己的顺序。命令幂等、expected revision、阶段 intent/receipt、hash chain 和 replay 共用一个 fold。
- append 先校验，再写入并同步，最后更新内存投影。写入结果不明即阻断后续写；残缺记录、hash/顺序不一致、安装身份不一致均保留原件并拒绝，不截断后继续。
- pending stage 在进程被杀后保留；启动读取只重建状态，不执行服务命令。显式 RecoveryRequired 不能被普通 receipt 覆盖成成功。真正的经认证恢复/裁决入口仍属后续 broker 工作。
- `ReleaseKeyring` 仅接受管理员预置公钥；包不能自行新增信任根。签名绑定规范化 manifest，文件集封闭，验证二进制/静态 UI 内容、大小、平台与兼容范围；切换前还需再次校验。
- 路径检查拒绝相对路径、穿越、Windows device/ADS/大小写别名及已存在的 symlink/reparse-point。它是安装前检查，不独自证明 ACL 或抵御具有目录写权者的所有置换；实际账户/ACL 和执行时身份重查在 24.2/24.3 验证。
- `SystemServiceManager` 在 Windows 使用公开的类型化 SCM API；其他目标使用固定原生服务管理命令、清空环境、有界等待和输出。只接受内置服务，不接收任意程序或 shell。服务启停需观察到预期 OS 终态；进程 Running 不等于主脑 Ready。它不是 SCM dispatcher、LaunchDaemon 或 systemd service host，原生注册/冷启动尚未验收。
- `forme-hostd` 提供仅 loopback 的内部 HTTP 原型，维护与 AgentExecution 使用独立 listener/路由/凭据和连接配额；Windows SCM 路径加载受管凭据文件。全部路由认证，拒绝跨域 token/路由、错误 Host、模糊 framing、浏览器 Origin 和非 loopback bind，连接数与请求大小有界。尚无浏览器登录、近期 owner reauth 或原生 peer identity；不能把这一原型部署成公开管理服务。
- broker 仅激活 Inspect/StartCore/StopCore/RestartCore；其他 maintenance 类型在 plan 前拒绝，不落假操作。proof、active release、refs、deadline 与 command digest 绑定；动作前写持久 Checking intent。成功 operation 重查不再调用 adapter，中断则只能检查原 intent，不盲重试。真正的近期认证、typed lifecycle workers 和双凭据域属于 24.2/24.3。

### 2.1 Worker admission foundation (24.2)

- `WorkerJob`、`WorkerPermit`、`WorkerObservation`、`WorkerReceipt` 和 `WorkerCancelReceipt` 是版本化产品 DTO，不进入 brain EventKind；Job/Permit 同时绑定 owner、action、affected resources、exact plan digest、workspace scope、sandbox profile、authority epoch、deadline、nonce 和 cancel ref。
- `WorkerExecutorHost` 只接受 `HostCredentialDomain::AgentExecution`；OwnerMaintenance 凭据、错误 principal、陈旧 authority epoch、越界 workspace、替换后的 plan/resource、过期 permit 和无 scope allowlist 均在底层隔离器之前拒绝。
- `WorkerJournal` 使用独立 OS 锁、单 writer、顺序与 hash chain。admission、start、cancel、receipt、unknown 都先持久化再更新投影；进程重启发现未终态 job 时只把**原 job**置为 `Unknown`，不创建新 job、不自动重做副作用。
- 默认 `UnavailableWorkerIsolation` 永远拒绝执行，加载配置不会自动解除阻断。仅前台显式 `--development-process-workers` 可用于普通进程开发测试；SCM 服务路径拒绝该开关，生产需后续平台 adapter。这使“还没有真实 sandbox”表现为不可用，而不是偷偷以 brain/高权限宿主身份执行。
- `WorkerLaunchRegistry` 只接受安装配置预注册的绝对程序和固定参数；`ProcessWorkerIsolation` 通过 profile 选择入口、清空继承环境、写入受保护的 job/permit、切换到精确 workspace 并以子进程退出码生成 receipt。它是进程边界，不宣称已具备平台级强隔离；生产 strict profile 仍必须注入对应 OS adapter。
- 专项正反测试为 5/5：隔离能力缺失阻断、双凭据域边界、plan/scope/epoch/deadline 绑定、重复 admission 不重复副作用、重启 Unknown 不自动重做、terminal cancel 不重复调用隔离器。测试使用仓库自建 fixture，不代表 Windows SCM、Linux systemd 或 macOS LaunchDaemon 已通过。
- 进程专项测试为 3/3：真实子进程成功 receipt、非零退出的失败 receipt、取消终止且二次取消不重复副作用。

### 2.2 Worker 控制面（24.2）

- 新增 closed `WorkerDispatchRequest` 与 4 MiB 有界 `WorkerRuntimeConfig`，拒绝未知字段/版本、重复 scope/profile、缺失映射、路径交叠及管理文件混用；注册入口不能由 Agent 请求替换。
- 执行请求只在 AgentExecution listener 认证；维护凭据不能 dispatch，Agent 凭据也不能在维护端口调用执行路由。两个端点各自限制连接，不以路径判断代替端点隔离。
- host 将 admission/start、probe/receipt、cancel/receipt 串行化；取消回执参与 journal fold，查询和重启保留 exact outcome/receipt。并发取消不会抢在 admission 完成前进入 adapter。
- 真实 HTTP 并发重复派发由 fixture 的追加计数证明只执行一次；错误请求的 journal 长度不变且无 marker。Windows 使用长度观察而非读取已被独占锁定的 journal，不解除锁来做测试。
- 证据详见 [worker 控制面报告](installed-product-24-2-worker-control-plane.md)。此批仍未注册服务、创建系统账户、操作真实 owner 数据或导入前端。

## 3. 验证与证据

组件门：`powershell -NoProfile -ExecutionPolicy Bypass -File tools/verify-installed-host.ps1`。每个命名测试必须真实执行一次，测试不存在/被忽略/零执行均失败；末尾明确 IP01–IP24 为 NOT_RUN。

组件覆盖：closed schema、三服务路径/账户清单、保留数据卸载边界、跨平台路径拒绝、并发 CAS、跨重启幂等、真实子进程持锁与被杀、阶段待核实、torn/tampered journal、签名/平台/二进制/UI/extra-file 拒绝与回滚兼容性；另外覆盖 bearer/proof/release 拒绝、过期零调用、并发只一次、真实 broker effect 后进程被杀不重放及真实 HTTP 认证/幂等。fixture 在独立临时目录产生一次计数；真实 HTTP 仅执行只读 Inspect，不操作已安装系统服务。

此前底板已完成 fmt、workspace check/test、strict clippy、compliance、冻结 18-crate/99-kind 门与原 S1–S99 workspace 回归。worker 控制面 `f1122ec` 当时的宿主专项 51 项、657 passed / 0 failed / 5 默认 ignored 是历史组件证据，保留于 [worker 控制面报告](installed-product-24-2-worker-control-plane.md)。最新源码 `26ce4b4` 的全量/宿主/质量证据见上文；当前 `tools/verify-m5.ps1` 与发布回执重验统一见 [M5 报告 §10](m5-acceptance-report.md#10-产品化分支的永久回归重验)。macOS/Linux 原生目标和全部 IP 安装场景继续 NOT_RUN，不使用 Windows 组件测试代替。

此前 `785027b` 后复跑结果：统一代码质量门 PASS（结构回归 6/6、421 个源文件、63 个历史规模债务未增长；Rust fmt/check/test/strict Clippy、前端依赖审计/TypeScript/行为测试、Web 资源、bundle、原创性与许可证均 PASS）；产品组件门 PASS（Host/worker 专项、Web identity 16/16、Web ingress 8/8、Web workbench 8/8，以及 webd 空环境启动、kill/restart 和独立认证存储 smoke）。这是历史组件证据；最新结果见上方 `01b77f9` 记录，两者均不提升为 IP01–IP24。

本批证据边界：组件测试、loopback HTTP 集成测试和受注册入口的真实子进程测试通过，worker admission、进程启动和 receipt 恢复基础已闭合；这仍不等于 SCM/systemd/LaunchDaemon 已注册，不等于 worker 账户/ACL/Job Object/cgroup/网络隔离已证明，也不等于 Windows/macOS/Linux 安装包可发布。下一批继续实现平台级隔离、常驻 service host、会话 helper 与 24.3 安装事务。

## 4. 依赖、数据与环境

execution 直接复用已有 `ed25519-dalek =2.2.0`（关闭 default features，仅 std）与 `getrandom =0.4.3`；新增 Windows-only `windows-service =0.8.1` 使用公开 SCM API。没有新增内部 crate/依赖边或内核，锁文件和来源/许可清单同步更新。测试签名密钥是仓库自建的确定性 fixture，不是发布密钥。

本批不会读取或迁移本机 `.forme` 里的真实脑/模型配置，不注册系统服务，不更改系统账号/防火墙/开机项。测试数据只在临时目录创建和清理；密钥、bootstrap proof 和账号口令不进入提交。
