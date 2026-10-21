# 独立部署与 Web 产品化实施记录

本记录承接 requirements/17–18、architecture/10、prd/24–26。它只记录新的产品化施工与证据，不改写 M0–M5、C1–C24 的历史验收；宿主组件测试不等于 IP01–IP24 或安装包验收。

## 1. 实施顺序与当前范围

| 顺序 | 交付 | 当前状态 |
|---|---|---|
| 24.1 | 宿主协议、安装清单、管理 journal、签名与兼容性校验、内置 OS 服务管理适配。 | 已实现并提交 `92802fe`；Rust 全量门、冻结 18-crate/99-kind 门、宿主专项门通过；不代表原生安装验收。 |
| 24.2 | 真正的系统 service host、受认证双域 broker、实际账户/ACL/逐 job worker 隔离与会话 helper。 | **基础层 `7eec019`、进程边界 `0021902`、worker 控制面 `f1122ec` 已提交**；hostd 的 worker 配置/dispatch/probe/cancel、双域 listener、并发控制与取消终态重放已有组件证据。Windows SCM dispatcher 已实现但未注册安装验收；默认阻断普通进程，只有显式前台开发开关可运行 fixture。平台账户/ACL、Job Object/cgroup/LaunchDaemon sandbox、网络隔离及会话 helper 仍待实施/原生验收，不能把控制面通过当作强隔离完成。 |
| 24.3 | 原生包、受控停服/升级/恢复/卸载、签名供应链与原生平台演练。 | 待实施；没有可发布的签名包或跨平台支持声明。 |
| 25 | 无配置初始化、旧脑验证/认领、账号/session、typed config/secret resolver、内部身份与 Web API/TLS。 | **独立认证基础已在 `shu/installed-product@12b3f5d` 提交，待合入/产品验收**；12 项专项、678 项全量测试、strict Clippy、宿主与质量门通过。broker setup/旧脑验证/签发、口令恢复、HTTP/TLS、配置和 core bridge 仍待接入，不将组件或现有 bearer 当成完整网页登录。 |
| 26 | 五个 Web 工作面、正式流/审批/续接、精确前端来源审查与安装包联调。 | 待实施；本批没有导入上游前端或改动页面。 |

24.1 是 prd/24 的第一笔组件交付；25 的账号/session 是另一笔组件交付，均不表示 prd/24、prd/25 或 prd/26 整体完成。后续按依赖推进，不能以组件测试提前宣布独立部署/Web 产品已可用。

### 本轮工程维护与认证基础

- 维护规范：`docs/engineering/code-quality.md`，结构检查器/CI 为 `tools/code-quality.py` 与 `.github/workflows/code-quality.yml`。当前 66 个历史超限文件是待还债基线，不是“质量全部通过”的认证；新增超限或旧文件增长拒绝。
- Gateway 拆分：`b373aec` 保持 public API、事件、生产装配与测试名字不变，按控制域和 HTTP 处理职责拆开。`lib.rs` 从 3641 行缩至约 230 行，`server.rs` 从 2256 行缩至 599 行。其余历史大模块继续按风险逐批维护，不全库机械切片。
- 认证：`12b3f5d` 只写独立认证库，不打开脑库。签名账号认领接缝、Argon2id、session/CSRF/epoch、operation-bound reauth、口令替换与重启持久性详见 [组件报告](installed-product-25-identity-foundation.md)。当前没有安装服务或可登录的新网页地址。
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

此前底板已完成 fmt、workspace check/test、strict clippy、compliance、冻结 18-crate/99-kind 门与原 S1–S99 workspace 回归。worker 控制面 `f1122ec` 的宿主专项 51 项全部通过；全量测试为 657 passed / 0 failed / 5 默认 ignored，strict clippy、合规与冻结图检查通过。2026-10-21 正在补收最终源码的 `tools/verify-m5.ps1` 长链结果，完整状态见 [worker 控制面报告](installed-product-24-2-worker-control-plane.md)。macOS/Linux 原生目标和全部 IP 安装场景继续 NOT_RUN，不使用 Windows 组件测试代替。

本批证据边界：组件测试、loopback HTTP 集成测试和受注册入口的真实子进程测试通过，worker admission、进程启动和 receipt 恢复基础已闭合；这仍不等于 SCM/systemd/LaunchDaemon 已注册，不等于 worker 账户/ACL/Job Object/cgroup/网络隔离已证明，也不等于 Windows/macOS/Linux 安装包可发布。下一批继续实现平台级隔离、常驻 service host、会话 helper 与 24.3 安装事务。

## 4. 依赖、数据与环境

execution 直接复用已有 `ed25519-dalek =2.2.0`（关闭 default features，仅 std）与 `getrandom =0.4.3`；新增 Windows-only `windows-service =0.8.1` 使用公开 SCM API。没有新增内部 crate/依赖边或内核，锁文件和来源/许可清单同步更新。测试签名密钥是仓库自建的确定性 fixture，不是发布密钥。

本批不会读取或迁移本机 `.forme` 里的真实脑/模型配置，不注册系统服务，不更改系统账号/防火墙/开机项。测试数据只在临时目录创建和清理；密钥、bootstrap proof 和账号口令不进入提交。
