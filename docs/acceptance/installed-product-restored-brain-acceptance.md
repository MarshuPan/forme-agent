# 恢复主脑的执行隔离与 Host 提交组件验收

2026-10-23，`shu/installed-product@5b99ac5`。承接 prd/24 §4.8–4.9；本记录区分真实数据库、实际子进程和受控服务 adapter，不据此宣布管理员安装、原生恢复或 IP01–IP24 已通过。

## 已实现

- SQLite 格式 9→10 增加恢复边界 companion metadata，绑定 owner/store、源与批准计划、代次/前一摘要及每个 run 的 stream_seq。历史事件与密钥不重写，不增加脑 EventKind；恢复后的旧 run、审批、intention 和 route 不恢复执行权限，原回答与事实保留可读，新 owner 命令仍走正式主脑。
- 在线批准恢复与本机离线恢复都在新目标封存边界后重新计算最终 manifest。候选被再次运行或篡改后不能作为原批准候选提交；重复恢复延长边界链。
- ProductConfigState v3 的 active_data 将实际数据库、原 vault、owner/store、候选 manifest 与边界绑定到现有 revision/CAS 配置事务。后续模型保存不丢路径、原 key 或隔离边界；实际 ProductionRuntime acknowledgement 必须报告匹配的数据身份与 composition。
- Host 的 CommitRestore 完成全部阶段：持久 intent→停脑→固定 hostctl worker 持两库独占 fence→验证同一 owner/store 和候选→独立可验备份→撤销旧 Web/reauth/session epoch→配置发布→释放 fence→重启并核对 ack/Running→Committed→开放准入。
- Web 可保持运行。有限本机认证维护外观只对既有 identity DB 执行原子 session fencing，不争夺第二个 daemon singleton lock，不改口令/OwnerId/内容 key；core fence 原文件保留其服务身份访问权。
- Host 所有、core 只读的 work-admission 记录独立于就绪回执。每次 HTTP 工作和 scheduler claim 都核对它；缺失/损坏/不匹配或未释放时拒绝。Checking 失败即使进程 Running 也不接任务。Committed 与准入发布之间中断，只允许核实原提交后补发准入，不重做停服、备份或切换。
- 操作绑定配置 revision 和统一截止时刻；错 owner、过期、陈旧配置零停服。失败保留 RecoveryRequired 和原 intent，换号不能逃避未决事务。受管数据操作根保持固定，不随恢复库路径重复嵌套。

## 本轮证据

| 层级 | 结果与边界 |
|---|---|
| 完整 Rust | `cargo test --locked --workspace --all-targets --quiet` 退出 0；155 个 suite，813 passed / 0 failed / 5 默认 ignored。忽略项不计作执行通过。 |
| 静态/结构 | fmt、workspace all-target check、strict Clippy、完整默认 compliance 和 diff whitespace 通过。结构门 527 个文件、62 处既有规模债务，没有刷新基线。 |
| Host 状态机 | execution lib 44/44；恢复新增三个命名场景，其中故障矩阵覆盖 stop/未知 stop、候选验证、异主候选、备份、fence、释放、启动和错 ack。每次 adapter 调用前核对真实 journal pending_stage；成功重查不增加调用。服务 adapter 是受控 fixture，未注册 SCM。 |
| 真实数据库/认证 | Gateway 恢复专项 3/3，store offline recovery 9/9。实际加密 SQLite/原 vault/两库锁/可验备份，WebIdentityStore 同时保持打开；旧 session 失效、口令与 owner 保持。错误 revision/owner/digest/路径/跨 operation 请求均拒绝。 |
| 实际进程 | `node --test tools/tests/managed_configuration.test.mjs` 1/1；实际 hostd/webd/gatewayd/hostctl、私有有界管道依次 validated→backup→fence→release。旧登录失效，同账号口令重新登录成功；恢复 core 未获准入时 503，释放后原回答可读，model 计数不增加。模型为本地计数 fixture，非真实 provider。 |
| 统一组件入口 | `verify-installed-host.ps1` 的既有门和新增十个精确命名场景全部通过；`verify-product-configuration.ps1` 的 22 个 Rust 场景、实际 binary 构建和完整进程链路通过。入口明确保留原生 IP 未运行。 |
| 前端/供应链回归 | 前端 strict TypeScript/行为与资源门 19/19；bundle/native/POSIX 23/23；离线签名及实际 Rust verifier 5/5。这些不是原生安装/签名发布证明。 |

进程 fixture 的 stop 是 Windows kill，不能冒充优雅 SCM stop：为避免改写或 checkpoint 被杀原库的 WAL，恢复 worker 专项明确使用先前由在线 ProductionRuntime 生成的 cold snapshot，并在 finally 恢复临时测试配置。准入文件由测试显式代入 Host 发布结果；完整 Host 阶段顺序另由上述状态机测试验证。二者不合并宣称真实安装事务已通过。

首次进程复验曾在重新登录处得到 403，原因是测试未用服务器 challenge.attempt_ref，已改用原挑战绑定后完整通过；没有放宽认证。此前被杀原库的 WAL 检查拒绝保留为正确边界，没有删除 sidecar 或放松验证。

## 继续中的整体义务

本批不关闭总里程碑：恢复候选的完整 Web 选择/提交和原生 SCM 联调、旧脑/备份接管、签名 Upgrade/Rollback/self-update、强隔离 worker、桌面 helper、LAN TLS/完整设置及 IP01–IP24/核心永久门的最终当前源码验收继续实施。未读 `.forme` 私有数据或真实模型密钥，未注册本机服务，未合 main/推送/发布；原生环境不足保留为未验证。
