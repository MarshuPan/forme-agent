# 独立 Web HTTP 入口组件验收

## 1. 范围与源码

源码：`shu/installed-product@49674b7`。承接 prd/25 §11.2；认证基础历史报告仍为 [12b3f5d 组件证据](installed-product-25-identity-foundation.md)，不以本报告改写其测试总数。

本次实现 gateway 内的独立 `forme-webd`、受管配置读取、loopback HTTP 认证路由，以及登录挑战与认证库 v1→v2 事务迁移。没有新增内部 crate/依赖边、脑 EventKind 或另一套 ProductionRuntime。

## 2. 已实现行为

- 从绝对路径、封闭 schema、32 KiB 上限的受管配置启动，不读取模型/owner shell 环境，也不打开脑库。地址默认 loopback:8765；配置错误或端口占用不静默换端口。
- 最小公开 bootstrap 只返回 schema/protocol version 与 claimed/setup_needed。未接通的 prove/data/claim 明确返回不可用，不开放匿名注册。
- 登录挑战、登录、会话查询、注销、近期重新认证、全会话撤销和改密码使用真实独立认证库。登录挑战 5 分钟、256-bit OS 随机 cookie，只持久化摘要；同挑战最多创建一个 session。挑战过期、异义提交或丢失响应后的不安全重试明确拒绝。
- 精确 Host/Origin、Fetch Metadata、CSRF、封闭 JSON、cookie 格式与重复值检查在入口执行；默认拒绝 forwarded/rewrite headers。响应 no-store、CSP、no-sniff、frame DENY，错误不回显原始输入。
- 登录/密码工作、mutation、读取独立限流；429 带 Retry-After。最多 64 个接入 socket、120 秒连接期限、30 秒请求期限；读取工作与密码计算分别限并发。请求取消后仍在运行的 blocking 工作继续持有名额。
- 停服关闭工作准入，再按同一 30 秒期限等待连接与认证工作退出。已知认证库版本在 writer 锁内事务升级，保留账号/session；未知版本、错误绑定与损坏文件拒绝接管。

## 3. 执行证据

环境：Windows x86_64、本机 Rust 工具链。测试仅使用仓库自建 fixture 与隔离临时目录，不读取 `.forme` 真实数据或密钥。

| 门 | 结果 |
|---|---|
| `cargo fmt --all -- --check` | PASS |
| `cargo check --workspace --all-targets` | PASS |
| `cargo test --workspace --all-targets` | 688 passed / 0 failed / 5 默认 ignored |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS |
| `bash tools/compliance-doctor.sh` | PASS；未修改 copy detection 或扩大许可白名单 |
| `py -3 tools/code-quality.py` | PASS；337 个源文件，66 处历史规模债务 |
| `tools/verify-web-identity.ps1` | 14 个命名测试真实执行，0 ignored |
| `tools/verify-web-ingress.ps1` | 8 个命名 HTTP/TCP 测试真实执行，0 ignored |
| `tools/verify-web-process.ps1` | 真实 bin、空环境启动、最小状态读取、强制停止后原目录重启均 PASS |
| `git diff --check` | PASS |

认证专项包含：claim 签名/安装/owner/账号承诺绑定、重启、错误密码、CSRF、idle/absolute expiry、epoch/revoke、operation-bound reauth、密码替换、并发名额、登录挑战幂等与已知版本迁移。

HTTP 专项包含：未认领信息最小化；登录/注销/reauth；错误 Host/Origin/forwarded/CSRF；改密码保留 owner 并撤销旧 cookie；独立配额/Retry-After；明文 LAN 与含秘密畸形输入拒绝；取消后密码名额不提前释放；真实 idle TCP 连接上限与释放恢复。

进程 smoke 清空子进程环境，只保留必要系统变量与测试临时目录，实际运行 `forme-webd --config <fixture>`。启动后仅存在独立认证数据，没有构造脑库。它证明进程组件独立，不证明 OS 服务安装、目录 ACL 或浏览器交互。

原永久 M5 长链和对应 closure 门另行记录，不从上述 workspace 数量推定整条发布验收已通过。

## 4. 未完成与不得扩大的结论

| 项目 | 当前状态 |
|---|---|
| broker 本机证明、有限 setup、旧脑验证与双边 claim 恢复 | 待接入；账号创建仍要求已提交的可信 broker 签名 |
| 本机管理员密码恢复 | 待实施；不存在匿名远程重置 |
| typed config、secret resolver、应用 ack 与 core bridge | 待接入 |
| LAN HTTPS、证书信任、原生 peer 身份与账户/ACL | 未由本组件实现或证明；HTTP adapter 拒绝 LAN |
| 五面 React UI、正式脑 API/SSE、附件与产物 | 待实施；本 bin 尚无业务页面 |
| Windows SCM/systemd/LaunchDaemon 实装与安装包 | NOT_RUN / 未完成 |
| IP01–IP24 完整场景 | NOT_RUN；组件正反测试不能代替产品验收 |

本分支尚未合入 main 或推送。本批未注册系统服务、创建系统账户、修改 ACL/防火墙、导入第三方 UI 或迁移个人数据。
