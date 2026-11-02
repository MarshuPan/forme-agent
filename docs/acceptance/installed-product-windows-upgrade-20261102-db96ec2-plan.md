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

## 8. Owner 测试凭据与正式 HTTP 操作授权

owner 随后完成登录、恢复原目标，并明确授权代理直接使用其提供的本地测试凭据执行正式认证与维护。凭据仅置于 Git 忽略的 `.forme/acceptance-test-login.json`，不进入源码、报告、命令参数或 Git。本文 §3/§6 中要求 owner 亲自输入产品口令的操作约束由本次明确授权替代；正式 challenge、登录、CSRF、operation 绑定的密码重认证、CAS 和 Host 权限检查全部保留，不读取浏览器已有 cookie，不使用内部服务凭据冒充 owner。

先由固定脚本 `target/installed-status-authorization-evidence/start-core-owner-http-20261102.ps1` 只读预检，核对原安装、owner、release、配置和未决操作；`-Submit` 才创建不可覆盖的无秘密 intent，并以固定 `maintenance:acceptance-start-20261102` 提交一次 StartCore。已有 intent 只能 `-Observe` 查询原操作，POST 超时不换号或重发。回执、SCM、配置 acknowledgement、readiness 与真实 provider 分开验证。此授权不改变 Windows UAC 或系统认证交互，也不授权任意 shell、脑数据写入或未知操作重试。

## 9. 首次主脑运行与升级前正式停脑

2026-11-02 使用 owner 授权的测试凭据，通过公开 challenge/login 建立独立内存 session，正式维护 StartCore 只 POST 一次。原 `maintenance:acceptance-start-20261102` 已 Committed，product revision `2 -> 9`；SCM 观察 Brain Running、PID `43784`，原 Host/Web PID 不变。配置 revision 1 的 acknowledgement 已到达，正确 `/api/v1/product/config?schema_version=2` 读取为 `configured=true`、`active_revision=1`、`acknowledged_revision=1`、`pending_restart=false`。初次脚本误查 `/product/configuration` 得到的 404 不是产品接口故障；原报告保留，正确路由的后续只读复核另存。

真实 Web→core 的 runtime manifest、doctor 和 runs 均返回 200；doctor 的 owner/store、事件记忆、认知、协调/编排、context、scheduler 等为 ready，run 数为 0。doctor 同时明确 `notification-backend=blocking`（managed worker binding unavailable），因此本轮不能声明 BrainReady 或真实 provider 已通过；旧 Host 维护健康仍报 starting，`/product/status` 的已知 ConfigurationRejected 仍在。这个阻断与首次启动成功分开记录，升级后须重新核验。

随后通过同一正式认证/重认证边界提交一次独立 `maintenance:acceptance-stop-20261102`，为升级前离线数据核验释放 writer。StopCore 已 Committed、product revision `9 -> 16`，SCM Brain Stopped、Host/Web 继续运行、配置 ack 保持 1。两次操作都是已知终态，不是未知操作重试；没有模型调用、手工创建脑或程序切换。Stop 报告沿用了早期脚本的 outcome 文案 `original_start_committed_readiness_separate`，其实际 action、operation id、原回执与 stopped 独立观察均为 StopCore；该标签瑕疵不改写原件。

| 本机 ignored 证据 | SHA-256 |
|---|---|
| `start-core-owner-result-20261102.json` | `e9066d260f8cfeaf94de9d40e6f6d8208e7f2b1a025191a8802be85bfa84d454` |
| `start-core-owner-observation-20261102T083330117Z-20261102.json` | `6f9a4a804f5fff81d80764979f052d69eb334ba2be543e6b896fe3370ab3a310` |
| `stop-core-owner-result-20261102.json` | `7ed2d415d576a4acf61711a382bd309aa2edd130712b5ebde358b49c79d47afa` |

### 9.1 初始化之后的新候选入库准备

新的固定脚本 `target/installed-status-authorization-evidence/prepare-signed-inbox-after-start-20261102.ps1` 绑定上述两次已提交操作和 product revision 16 的 stopped 状态，保留原失败入库脚本/报告。它再次核对五配置/六程序、独立信任根、实际离线 owner/store 身份和签名闭合文件集；`-PrepareInbox` 只 create-new 复制/flush/验签该候选，再同父目录发布到原目标 inbox。脚本不提交维护、不改变服务、程序、配置、ACL 基线或脑数据；只为新 staging 设置原已批准的私有 ACL。已有目标/marker、不明身份或并发状态变化均停止留存，不覆盖或删除。实际 schema/升级兼容性仍待正式 updater 持锁检查。

固定主脚本 SHA-256 为 `e57b1ce26a640988a05b64b63c19f8657986cb719cf587ab137ed08214adcd55`；共用文件复制/不可覆盖证据 helper 在 Windows PowerShell 5.1 和 PowerShell 7 各 12 项通过。该准备属于原已授权的本机签名升级范围；需要管理员令牌时由 Windows UAC 交付，不能用测试口令代替系统确认。后续仍严格按 §3 执行正式 StageSignedRelease/Upgrade，不直接替换运行版本。

## 10. 新入库与正式签名暂存已完成

新准备进程 PID `65996` 已结束，`signed-inbox-after-start-20261102.json` 为 `inbox_prepared_only`、SHA-256 `4da0f673ff9fb9c31410fa6b6d2651c62bdd54253a702059a9a55fc471156707`。准确候选的 manifest/signature 与 18 个 content 文件共 20 项完成 create-new、flush、摘要与独立验签后发布；服务动作/维护命令/程序切换均为零。离线 inspector 验证 1 个事件、水位 1、3 个 authenticated content rows，owner/store 身份完整。该回执仍不包含实际 SQLite schema，不据候选兼容声明推断 schema 已验证。

公开正式维护随后仅提交一次 `maintenance:acceptance-stage-20261102-db96ec2`，结果 Committed、product revision `16 -> 23`，无 pending/failure；target 为固定 `20261102-db96ec2`，active release 仍为原版。Host/Web PID 保持、Brain/updater Stopped。这只是签名暂存，不是 Upgrade/IP14 PASS。

下一事务由 `release-owner-http-20261102.ps1 -Action upgrade -Submit` 提交一次固定 `maintenance:acceptance-upgrade-20261102-db96ec2`，绑定新读 product revision、同一 release/digest、`backup:upgrade-20261102-db96ec2` 和最长五分钟 deadline。先 create-new 保存无秘密 intent；已有 intent 拒绝再次 Submit，仅 Observe 原操作。Host/updater 自行排空、持锁核验实际 schema/身份、一致性备份和切换；客户端遇断线只查询原 operation，不能重发。运行清单、config ack、SCM image、状态缺陷修复、通知 backend 和真实 provider 仍须各自验证。

## 11. Upgrade 实际预检拒绝与路径表示缺陷

固定 Upgrade 已正式 POST 一次，收到确定的 HTTP 409 `ConfigurationRejected`。后续只查询原操作，未重发；产品 revision 保持 23，operation 列表仍只有此前三个 Committed、没有该 Upgrade，updater 仍 Stopped/ExitCode 1077，Host/Web 原 PID 保持，active release 未变。`upgrade-release-owner-result-20261102.json` SHA-256 为 `86038998947c35464bb0b26da00db8bf7db7162ad2a795f10854a62bbf73181c`。这是未接纳的预检拒绝，不能写成升级已开始/已完成，也不能用通用错误归咎于 provider key。

源码审查定位到一个确定的预检阻断：`registration/release_lifecycle` 使用 `bin/...` 和 `core/core.json` 生成预期路径，而初始 SCM 为同一文件的反斜杠表示；`matches_config` 先前按原始命令串逐字比较，因而拒绝等价路径。`694b6bb` 与 `db96ec2` 的这些实现相同。独立 SCM 只读补充报告 `release-command-path-observation-v2-20261102.json` 确认三个服务均原字符串不匹配、仅归一分隔符后完整字符串匹配；无服务/产品变更。首份诊断的 PowerShell 数组拼接误差另有原报告保留，不作为根因证据。

修复遵守 prd/24 §4.10.2 的同一固定参数/身份边界，只接受 Windows 等价路径分隔符；新增完整服务配置正反回归及 updater mode/目标路径/参数否定断言。尚未把修复安装到本机，不能宣称正式升级已畅通。当前 Brain 是已知 StopCore 停止，Host/Web 仍可认证维护；原候选、暂存、数据、keys、配置、失败请求和所有回执继续保留。
