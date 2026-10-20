# 独立部署与 Web 产品化实施记录

本记录承接 requirements/17–18、architecture/10、prd/24–26。它只记录新的产品化施工与证据，不改写 M0–M5、C1–C24 的历史验收；宿主组件测试不等于 IP01–IP24 或安装包验收。

## 1. 实施顺序与当前范围

| 顺序 | 交付 | 当前状态 |
|---|---|---|
| 24.1 | 宿主协议、安装清单、管理 journal、签名与兼容性校验、内置 OS 服务管理适配。 | 已实现并提交 `92802fe`；Rust 全量门、冻结 18-crate/99-kind 门、宿主专项门通过；不代表原生安装验收。 |
| 24.2 | 真正的系统 service host、受认证双域 broker、实际账户/ACL/逐 job worker 隔离与会话 helper。 | 待实施；当前没有把既有终端进程注册为新系统服务。 |
| 24.3 | 原生包、受控停服/升级/恢复/卸载、签名供应链与原生平台演练。 | 待实施；没有可发布的签名包或跨平台支持声明。 |
| 25 | 无配置初始化、旧脑验证/认领、账号/session、typed config/secret resolver、内部身份与 Web API/TLS。 | 待实施，继续使用已确认方案，不将现有 bearer 当成账号登录。 |
| 26 | 五个 Web 工作面、正式流/审批/续接、精确前端来源审查与安装包联调。 | 待实施；本批没有导入上游前端或改动页面。 |

24.1 是 prd/24 的第一笔组件交付，不表示 prd/24、prd/25 或 prd/26 整体完成。后续依照同一顺序推进，不能以组件测试提前宣布独立部署/Web 产品已可用。

## 2. 宿主底板的实现边界

- `protocol::installed_product` 是新增版本化产品 DTO；不增加脑 EventKind，不改已有脑协议的序列化结果。
- `execution::installed::ProductJournal` 是与 brain store 分开的管理事实源。OS 文件锁只允许一个管理 writer；`product_seq` 是它自己的顺序。命令幂等、expected revision、阶段 intent/receipt、hash chain 和 replay 共用一个 fold。
- append 先校验，再写入并同步，最后更新内存投影。写入结果不明即阻断后续写；残缺记录、hash/顺序不一致、安装身份不一致均保留原件并拒绝，不截断后继续。
- pending stage 在进程被杀后保留；启动读取只重建状态，不执行服务命令。显式 RecoveryRequired 不能被普通 receipt 覆盖成成功。真正的经认证恢复/裁决入口仍属后续 broker 工作。
- `ReleaseKeyring` 仅接受管理员预置公钥；包不能自行新增信任根。签名绑定规范化 manifest，文件集封闭，验证二进制/静态 UI 内容、大小、平台与兼容范围；切换前还需再次校验。
- 路径检查拒绝相对路径、穿越、Windows device/ADS/大小写别名及已存在的 symlink/reparse-point。它是安装前检查，不独自证明 ACL 或抵御具有目录写权者的所有置换；实际账户/ACL 和执行时身份重查在 24.2/24.3 验证。
- `SystemServiceManager` 在 Windows 使用公开的类型化 SCM API；其他目标使用固定原生服务管理命令、清空环境、有界等待和输出。只接受内置服务，不接收任意程序或 shell。服务启停需观察到预期 OS 终态；进程 Running 不等于主脑 Ready。它不是 SCM dispatcher、LaunchDaemon 或 systemd service host，原生注册/冷启动尚未验收。
- `forme-hostd` 提供仅 loopback 的内部 HTTP 原型，凭据由本机保护的 token 文件或显式开发环境加载；没有浏览器登录、近期 owner reauth 或 AgentExecution 域。全部路由认证，错误返回 400/401，拒绝模糊 framing/浏览器 Origin/非 loopback bind，连接数和请求大小有界。不能把这一原型部署成公开管理服务。
- broker 仅激活 Inspect/StartCore/StopCore/RestartCore；其他 maintenance 类型在 plan 前拒绝，不落假操作。proof、active release、refs、deadline 与 command digest 绑定；动作前写持久 Checking intent。成功 operation 重查不再调用 adapter，中断则只能检查原 intent，不盲重试。真正的近期认证、typed lifecycle workers 和双凭据域属于 24.2/24.3。

## 3. 验证与证据

组件门：`powershell -NoProfile -ExecutionPolicy Bypass -File tools/verify-installed-host.ps1`。每个命名测试必须真实执行一次，测试不存在/被忽略/零执行均失败；末尾明确 IP01–IP24 为 NOT_RUN。

组件覆盖：closed schema、三服务路径/账户清单、保留数据卸载边界、跨平台路径拒绝、并发 CAS、跨重启幂等、真实子进程持锁与被杀、阶段待核实、torn/tampered journal、签名/平台/二进制/UI/extra-file 拒绝与回滚兼容性；另外覆盖 bearer/proof/release 拒绝、过期零调用、并发只一次、真实 broker effect 后进程被杀不重放及真实 HTTP 认证/幂等。fixture 在独立临时目录产生一次计数；真实 HTTP 仅执行只读 Inspect，不操作已安装系统服务。

本批已完成 fmt、workspace check/test、strict clippy、compliance、冻结 18-crate/99-kind 门与原 S1–S99 workspace 回归；宿主专项门输出 `installed-host-foundation: PASS`。macOS/Linux 原生目标和全部 IP 安装场景继续 NOT_RUN，不使用 Windows 组件测试代替。

本批证据边界：组件测试和 loopback HTTP 集成测试通过，不等于 SCM/systemd/LaunchDaemon 已注册，不等于 worker 账户/ACL 隔离已证明，也不等于 Windows/macOS/Linux 安装包可发布。下一批只在此提交之后实现 24.2 的常驻服务 host、双域认证与 worker 隔离。

## 4. 依赖、数据与环境

execution 直接复用已有 `ed25519-dalek =2.2.0`（关闭 default features，仅 std）与 `getrandom =0.4.3`；新增 Windows-only `windows-service =0.8.1` 使用公开 SCM API。没有新增内部 crate/依赖边或内核，锁文件和来源/许可清单同步更新。测试签名密钥是仓库自建的确定性 fixture，不是发布密钥。

本批不会读取或迁移本机 `.forme` 里的真实脑/模型配置，不注册系统服务，不更改系统账号/防火墙/开机项。测试数据只在临时目录创建和清理；密钥、bootstrap proof 和账号口令不进入提交。
