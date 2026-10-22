# 受管模型配置与主脑启停增量验收

日期：2026-10-22。分支：`shu/installed-product`。设计说明为 `3577c8b`，受验实现为 `01b77f9`。本记录承接 `prd/25 §11.4–11.5`、`prd/26` 与工程质量规范；不改写 M0–M5/C1–C24 历史回执，不宣布 IP01–IP24 或整个独立部署产品完成。

## 1. 本次闭合的路径

本机安装准备 → 有限证明/账号认领 → 登录 Web 设置 → 保存模型配置和独立凭据文件 → 静态检查 → 针对该操作重新认证 → 受控请求内置 Brain 服务启动/重启 → 分别读取服务操作结果与主脑加载确认。

- 配置独立于脑事件、认证库和 AgentExecution。Web 仅通过有限宿主配置接口读写，不直接调用模型、工具或脑库。冻结的 18-crate 图与 99 个脑 EventKind 未变。
- 配置写者采用 OS 排他锁、版本/CAS 和链式 append journal；保存不可变 revision 与独立版本的密钥文件，持久 intent 后原子发布 core 配置，再提交 receipt。重启恢复原 intent，同 operation 不重复产生 revision，异义 operation 拒绝。
- 已存在的空/截断 journal、丢失 journal 但遗留 revision、摘要正确却不匹配安装模板的待发布 core、owner 不符及旧配置不一致均阻断，不重置为空安装，不覆盖旧文件。
- 全部输入在凭据 staging/intent 之前明确被拒绝时，返回 `422 / ConfigurationInvalid`，允许修改表单；发布或副作用不明时保留原 operation，不自动换 key 重试。旧凭据保持不可变，运行中的旧 revision 继续可读。
- 静态检查显式返回 `model_test_status=NotRun`。保存、检查、OS 服务 receipt、主脑 revision acknowledgement 与当前连接状态分别呈现；任何一项都不冒充真实 provider 可用性。
- 启停绑定 owner、operation、revision、action、期限与摘要，要求操作级口令确认；只能控制内置 Brain 服务。重启必须先取得确认停止的 receipt，才能启动；未知/失败进入恢复状态，再查询同一 operation 不重复 OS 调用。

## 2. 验证结果

环境：Windows `x86_64-pc-windows-gnu`、Rust 1.97.0、Node 24.16.0。以下检查均对本次最终实现源码执行；设计/验收文档提交不改变该源码。

| 检查 | 结果与范围 |
|---|---|
| `pwsh -File tools/verify-code-quality.ps1 -AllowMissingCorpus` | PASS。结构检查器 6/6；433 个源文件，63 处既有规模债务未增长；fmt、workspace check/test、strict Clippy、前端与资源、bundle、合规及 whitespace 均通过。 |
| `cargo test --locked --workspace --all-targets`（上门内执行） | 749 passed / 0 failed / 5 默认 ignored；统计来自 146 个测试 suite 的汇总，不包括后续定向门重复执行。 |
| 前端与 bundle（上门内执行） | strict TypeScript、14/14 前端行为测试、7/7 bundle 回归；70 个锁定依赖，`npm audit --audit-level=moderate` 为 0 漏洞；7 个嵌入资源可重复校验。 |
| `pwsh -File tools/verify-product-configuration.ps1` | PASS。精确执行 16 个配置/激活 Rust 测试，构建四个正式 binary，并执行 1 个实际 host/web/core 进程综合场景。服务管理动作的计数由受控 adapter 验证。 |
| `pwsh -File tools/verify-installed-host.ps1` | PASS，70 个命名场景；包含宿主、worker 控制面与受控停止组件，不是原生安装验收。 |
| `pwsh -File tools/verify-web-identity.ps1` | PASS，18 个命名测试；新增口令/proof 摘要绑定、一次消费、撤销 session 拒绝。 |
| `pwsh -File tools/verify-web-ingress.ps1` | PASS，8 个命名场景及实际 webd 空环境/kill/restart/独立认证库 smoke。 |
| `pwsh -File tools/verify-web-workbench.ps1` | PASS，8 个命名场景，使用计数模型 fixture。 |
| `pwsh -File tools/verify-core-brain-workspace-contract.ps1` | PASS。冻结 crate 图、精确依赖版本、97-kind strict prefix 与仅两个 additive 事件保持。 |
| `bash tools/compliance-doctor.sh` | 按默认参数另跑 PASS，未借 `AllowMissingCorpus` 代替本地完整合规检查。 |

五个默认 ignored 场景涉及另建 CLI、真实 Chrome 和 M4/M5 多进程长期门。本轮没有执行新的完整 `verify-m5.ps1` 或生成发布回执，不以 workspace 回归替代这些证据。

最终统一源码日志、产品组件串行门日志和完整合规日志保留在本机临时目录，属于本轮辅助核对材料，不是签名发布回执；其内容摘要分别为：

| 本机日志名 | SHA-256 |
|---|---|
| `forme-config-final-quality-1fb03973caa14237a2bae8336d53a8c8.log` | `624D4C1DE7C26DF02EF537482E7970D4E138B4EB7F151C3FB3A328162DE619B8` |
| `forme-config-product-gates-9e85b577759042fcab5d0e8dbda63986.log` | `83A1516E54E160D38091E75384709A1603B2B2FDD4679A0EEA9514CE2740C838` |
| `forme-config-compliance-eacb6fee46234994b4bd055281b60da7.log` | `369AA4CA23BBB16CCE58BB22BE8BE49E290675D13943609A496FA0CD0B6BDC92` |

## 3. 实际进程与浏览器证据

`tools/tests/managed_configuration.test.mjs` 在独立临时安装目录、随机 loopback 端口与空继承环境运行正式 `forme-hostctl`、`forme-hostd`、`forme-webd`、`forme-gatewayd`：

1. `prepare-installation` 后使用本机证明完成账号认领；未登录与错误 CSRF 被拒绝。无效模型端点返回 422，配置仍为 revision 0。
2. 从 Web 保存 revision 1，响应不含 key/credential path，保存与静态检查均不调用模型。错误启停口令不推进宿主管理 journal。
3. 杀掉并重启 host/web 后，账号、session、配置和幂等结果保留；原保存重放不再应用，陈旧 revision 返回 409。
4. 作为测试进程启动真实 ProductionRuntime core，读取到配置 acknowledgement 1。通过正式 Web Run 路径调用本地计数模型，任务完成；检查 `RunAccepted → SessionBound → ContextBuildFinished → ModelCallStarted → ModelCallFinished → RunComplete` 事件。
5. 保存 revision 2 后 acknowledgement 仍为 1，旧 core 引用的凭据文件保持原值；终止 core、等待其原 writer lease 失效后重启，确认 acknowledgement 2。加载新 key 不代表新 key 的真实 provider 验证通过。

上述 core 由测试 supervisor 启动；**没有通过本机 SCM 执行 Start/Restart**。原生服务动作由另外 4 个 adapter 场景验证并发/重启重放一次、越权/陈旧/过期零调用、未知 stop 不 start、确认 stop 后 start 的顺序。

另用真实 IAB 浏览器登录同一类临时账号，打开设置页，检查模型已保存、key 不回显、静态检查与主脑确认独立显示，以及错误端点被拒绝后仍可编辑。没有点击原生启停或修改真实账号/配置。浏览器页签、临时进程和该测试目录已清理。

![临时安装设置页的静态检查与主脑确认](product-configuration-artifacts/configuration-browser.png)

截图 SHA-256：`586E0BF9ABB6C2E370BCA726D3E3A9813FFEBE8A49270830B622EAF50947C28E`。截图只包含测试界面，不含真实模型密钥。

## 4. 可维护性与依赖

配置存储拆为入口/状态协调（389 行）、journal（304 行）、materialize（348 行）、文件 IO/原子发布（103 行）及独立测试；Web 配置 HTTP 为 188 行，前端启停组件为 73 行。新增生产/测试文件均符合 600/1200 行约定，未刷新或扩大质量基线。

execution/gateway 直接精确声明已有锁定依赖 `tempfile = "=3.27.0"`，仅使用公开原子文件 API；没有新增 registry 包或内部依赖边。来源/许可审查与 NOTICE 同步更新。没有导入第三方内核、插件、prompt 或 fixture。

## 5. 仍需实施与验收

- 原生服务账户/ACL、逐 job 强隔离、会话 helper 与 core 到隔离 worker 的正式工具接入；缺少隔离时生产执行继续 blocked。
- 将本机安装准备接入完整安装、升级、恢复、卸载事务；跨平台原生包、签名、冷启动与保留数据演练。
- LAN TLS/证书/网卡/防火墙、旧脑/备份接管与 OS 管理员恢复；能力与消息渠道的完整受管设置。
- 附件/产物、完整收件箱和候选处置、其余工作台与维护功能。
- 真实 provider/工具和各原生平台 IP01–IP24，特别是一次完整新机安装到长期运行的 IP24 联合验收。

本轮仅使用仓库自建测试账号/密钥/模型 fixture，不读取、迁移或上传真实 `.forme` 数据与模型配置。提交在产品化分支，未合入 main，未推送。当前进度以 [总实施记录](installed-product-implementation-progress.md) 为准。
