# Windows 修正版签名升级与安装后核验（2026-11-02）

本记录承接 prd/24 §4.5/§4.10、正式升级预检清单与本轮信任接入回执。当前只完成发布信任接入，未执行候选入库、StageSignedRelease 或 Upgrade；不能据此提升 IP14/IP24。

## 1. 固定输入与保留范围

| 输入 | 绑定 |
|---|---|
| 安装 | `installation:969387966500ba7c649fcc449c073cd5d27042b2b2989f2eda3c7a8d5f376258` |
| Product URL | `http://127.0.0.1:18765/` |
| 原 active release | `forme-windows-x86_64-20261027-7cb5b8b` |
| 当前配置 | product revision 2、config revision 1、ack 空；重新读取后才能绑定具体命令。 |
| 修复源码 | `db96ec2df073352beb1ed1a14ddc081df0796243`；后续只更新验收文档。 |
| 候选 | `C:\code\forme\target\product-bundles\forme-windows-x86_64-20261102-db96ec2-signed-local-dev` |
| release id / digest | `20261102-db96ec2` / `sha256:1acb2f0a400dbb78a51b4d5f84023d5239155ece704e9dcf504a47266454dbbf` |
| 独立 trust | 已安装 `config/release-trust-local-dev-20261102.json`；SHA-256 `6999d15220d8c19d1f0a7bbd4d02935fd33712205dbc8b936c1de46e23d64f17`。 |
| 候选兼容声明 | config 1、data 10、minimum upgrader 1；仍须观测实际数据。 |

保留原安装身份、账号、口令、模型配置与密钥、脑/工作空间、旧 release、管理 journal、配置版本与全部失败证据。不再运行首装安装器，不直接覆盖在用程序，不改防火墙或 Windows 证书存储。

## 2. 入库准备

管理员准备只接受上述候选和已安装独立 trust。副作用前核验原信任接入回执、五份配置与六程序摘要、四服务绑定及运行状态、正式维护快照和具体签名/闭合文件集。只读离线数据检查使用已审查 hostctl；只记录配置 schema、owner/store 摘要、事件水位与完整性状态，不初始化数据库或读取 provider key。现有 `inspect-data` 回执不含 SQLite user_version，因此入库准备明确保持 `database_schema_version=null`、`runtime_compatibility_verified=false`；实际数据 schema 必须由正式 updater 持锁读取后才能判定兼容，入库本身不证明兼容或升级准入。

输入不符、已有待决操作、数据身份不可核验或原目标已存在时停止并留存报告。通过后可将 manifest、64 字节 signature 与签名闭合 content 复制到固定 `host-state/release-inbox/20261102-db96ec2/`，文件逐项 create-new、flush 和摘要复核；签名回执不作为信任来源，也不进入发布 content。复制中断保留原 staging，不删除或重新复制。入库不提交管理命令、不切换程序、不启停服务。

具体脚本为 `target/installed-status-authorization-evidence/prepare-signed-inbox-20261102.ps1`，默认只读，`-PrepareInbox` 才准备固定 inbox。配套文件复制/同父目录发布/不可覆盖证据夹具在 Windows PowerShell 5.1 与 PowerShell 7 各通过 12 项；仅操作临时目录。脚本与配套函数的固定摘要、实际运行结果另随本机回执登记。

- 主脚本 SHA-256：`0b45147f8ed351083dfe97e862fd9b3d00f8a4cff10b501fb957039035b07eac`；两种 PowerShell 语法解析通过。
- 配套函数 SHA-256：`6fce6a3518183153557eda85bfb94aa65bb39e1dfaa6d45503072bbee16d40b2`。
- 仅准备 `C:\ProgramData\ForMe\host-state\release-inbox\20261102-db96ec2`；原目标、attempt marker 或回执已存在时拒绝。当前候选约 457 MB，执行前另核对至少 2 GiB 可用空间。

## 3. 正式暂存与升级

1. 从已登录 Web 的正式维护视图刷新 allowed operations 与 product revision。
2. 选择 `stage_signed_release`，绑定 §1 的 id/digest。owner 在产品页面自行输入密码并确认该操作；自动化不读取或代填口令，不绕过一次性重新认证。只观察原 operation 直到已知终态。
3. 只有暂存 operation 已 Committed 且无 pending stage，才选择 Upgrade，重新读取 revision，绑定同一目标与新的明确备份引用。owner 再次完成该具体操作的产品认证；请求按原 deadline/CAS 和原 key 提交一次。
4. 由正式 Host/updater 排空、离线检查实际 schema/身份、取得一致性备份、切换三个业务服务 image、发布配置 binding，并执行原 Checking/Committed/准入开放。验收脚本不另造管理 actor、proof、journal 记录或 active pointer。
5. 响应丢失、超时或服务暂时不可达，只查原 operation、既有回执、SCM 和只读 probe；不再次 POST、不生成替代 key、不手动重跑 updater，也不自动反向切换。

## 4. 安装后的验收

- 三业务服务实际 image/身份、active release 和签名内容绑定目标；固定 updater 仍保持原按需注册。
- product/config revision、主脑加载 ack、同一 owner/store/事件连续性、fresh readiness 与准入开放一致。Running 或 HTTP 200 不能单独代表 BrainReady。
- 通过正式 Web→Gateway→ProductionRuntime 执行不含私人正文的真实对话，记录实际 provider/profile 与事件，不保存密钥或 cookie。再验证工具审批/隔离、项目/后台、五个 Web 工作面及未知结果恢复。
- 重启/注销、恢复/回滚、卸载和其他 IP 依各自计划与保留范围取证；会影响桌面会话或移除程序的操作另行协调，不包含在一次 Upgrade 中。

升级失败保持原 operation 与备份，按原生验收计划 §3 审查具体恢复。原生包签名、Linux/macOS 不发布边界及完整 IP01–IP24 状态分别记录；本机开发 release 验签不能替代它们。

## 5. 当前入库预检停止与只读定位

owner 确认本次 UAC 后，固定入库准备脚本实际运行一次，PID `59888`、退出码 `1`。`signed-inbox-prepare.json` 为 `stopped_for_review`、`failure_stage=offline-data-identity`、`changes=[]`，SHA-256 为 `3bba423f7ac1084038c169fdcad09b2a07175b8bf110e7c6a4b6740aa716d400`。失败发生在任何复制之前，未创建 inbox attempt marker；维护提交/服务动作均为零。四服务状态与 PID 保持信任接入后的值。此结果不能直接解释成数据损坏，也不能作为升级兼容性证据。

原脚本/报告保留，不重跑 `-PrepareInbox`。后续只读定位使用独立 `inspect-installed-data-state.ps1`，SHA-256 `f78c9863b151bd6e51bb1cb09aa17fcb0880ae675c127817f944b480d091d087`：只观察 core 配置字段名与绑定、文件/sidecar/内容 key 目录存在性、正式维护元数据，以及固定官方 `inspect-data` 的安全结果。它不读取 provider key、不重置数据或 key、不复制发布、不启停服务、不提交维护命令；仅生成开发证据 `installed-data-state-readonly.json`。

### 5.1 已完成的只读定位

该管理员诊断实际运行一次，PID `7464`、观察到退出码 `0`。`installed-data-state-readonly.json` 的 SHA-256 为 `ca1a469961498dbd1e6c165b7f870cf9bb9663c3f209989d4076a00812418582`，结果为 `inspected_read_only`、`failure_stage=null`；产品写入请求、服务动作和维护命令均为零，未调用 provider 或读取 provider key。

配置 schema/revision 均为 1，installation 与维护 owner 绑定核对通过。管理员实际观察到配置指向的 `C:\ProgramData\ForMe\brain\forme.db` 不存在，没有 WAL/SHM/journal sidecar；对应 `forme.content-keys` 目录也不存在。官方 existing-data inspector 返回退出码 1，未形成可验证数据身份。此结果说明 §2 要求的现有 store 前置条件尚未满足；仅描述这次配置所指的路径，不能据此推断其他位置的数据状况、数据损坏或 provider 凭据错误。

只读维护投影仍为 product revision 2、config revision 1、operations 空；独立 SCM 复核 Host/Web Running（PID `37740`/`21024`）、Brain/updater Stopped。active release 仍为原版本，未复制候选、暂存或升级。

## 6. 当前后续顺序与管理员确认

Codex 工具自动批准与 Windows UAC 分属两层。本轮核对命令执行进程及 Windows MCP 执行进程均没有管理员令牌，Windows UAC 仍启用；Codex 的自动批准不能替代该系统交互。后续优先使用已经以管理身份运行的 Host 及其固定 typed 维护接口。确需额外管理员准备时，将同一授权范围内的有界步骤合并；不修改 UAC、ACL 或增设通用特权执行入口来消除弹窗。

当前实例尚在缺少自动主脑激活的旧版本。下一项为 owner 经现有 Product URL 的“设置 → 设备维护 → 启动主脑”提交一次正式 `StartCore`，由 Host 校验现有 revision 与 owner 认证后启动原 Brain；不直接使用 SCM 绕过产品维护，不由验收脚本初始化空 store、分配 keys 或替换配置。这次产品认证需要 owner 自行输入口令，Host 服务动作本身不需要新的 Windows UAC。

提交后只查询原 operation、SCM、实际配置 acknowledgement 和 readiness；发生 Unknown 或失败时保留原号及现场。成功启动不直接证明真实 provider 或升级通过。后续为离线身份检查而停脑，仍须通过明确的产品维护事务完成；取得独占可验证数据后才另行准备新的入库尝试，原失败脚本/报告不覆盖、不重放，实际 schema 仍由正式 updater 持锁核验。

## 7. 网页认证状态与已安装匿名边界观察

owner 授权直接操作后，实际切到唯一的“forme · 个人工作台”独立 Chrome 窗口重新观察。页面当前显示账号/密码及登录按钮，尚无可使用的已认证维护界面。这里只证明该窗口的当前状态，不能用先前登录记录替代；未操作密码管理器、填口令或提交认证。已将该窗口交由 owner 登录，登录后继续正式维护，不使用服务凭据冒充 owner 的重新认证。

等待认证期间，用无 cookie、无重定向、无代理的固定 GET 观察脚本检查实际已安装的 `http://127.0.0.1:18765/`，未提交登录、配置、维护或脑命令。实测结果如下：

| 观察 | 实际结果 |
|---|---|
| HTML 与公开 bootstrap | 均为 200；HTML 存在，bootstrap 仅有 `schema_version/claimed/setup_needed/protocol_version`，`claimed=true`、`setup_needed=false`。 |
| 匿名维护、配置、产品状态 | 三个正式接口均为 401。 |
| 错误 Host、错误 Origin、伪造 X-Forwarded-Host | 三种请求均为 403。 |
| 安全响应头 | 八个响应均核对 CSP、nosniff、no-referrer、no-store、COOP same-origin 与 CORP same-origin。 |
| 服务观察 | 前后四服务状态与 PID 一致；Host/Web Running，Brain/updater Stopped。 |

第一份观察脚本误将公开字段集合限制为三个，漏掉所有 DTO 必需的 `schema_version`，因此原 `installed-web-boundaries-20261102.json` 为 `review_required`。原件保留，SHA-256 为 `eb80712e4b79b0c0f4a5c63e8183b27f11789d1a8a5becc2ea69e726b202ca3f`。对照正式 bootstrap DTO 后，独立补充脚本先核对原报告摘要、复核八项 HTTP 状态及八组安全头，再只读重新取得公开 bootstrap 四字段；`installed-web-boundaries-review-20261102.json` 为 `observed_boundary_checks_passed`，SHA-256 为 `009be06416abc16da5261cb7d40583aebb1ce5ea6ffc52b3cd881939e7ebc5cb`。

这些是当前安装 Web 的局部真实观察，不证明 BrainReady、完整 IP07/IP22、外部 provider、发布切换或完整 IP24；旧程序仍在运行。脚本及两份不可覆盖报告均保存在本机 ignored `target/installed-status-authorization-evidence/`。
