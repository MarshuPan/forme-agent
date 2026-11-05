# 历史安装缺少恢复基线时的独立审查与迁移方案

状态：**owner 已于 2026-11-04 批准协议细化、文档回填和隔离实现；尚非本机 Apply 授权。** 本方案承接已批准并实现的[有限恢复契约](installed-release-interruption-adjudication.md)，只处理正常 `AbortBeforeSwitch` 明确不适用的旧安装。先冻结精确协议并完成隔离验证，再准备确切签名包、程序摘要与实际迁移脚本；不能倒置顺序。

## 1. 精确对象与证据缺口

| 项 | 当前已知 |
|---|---|
| Installation | `installation:969387966500ba7c649fcc449c073cd5d27042b2b2989f2eda3c7a8d5f376258`。 |
| 原未决 operation | `maintenance:acceptance-upgrade-after-path-20261102-db96ec2`。 |
| 原 command digest | `sha256:e818974260c114ed931bfd344c66d8c24fee3f8627f4c3a700b3ee6d68425b94`。 |
| 最近正式产品水位 | revision 28、Validated / pending Quiescing；配置/ack revision 1。实际实施前必须再次核实。 |
| 原 active release | `forme-windows-x86_64-20261027-7cb5b8b`；未切换至目标。 |
| 实际 SCM | Host/Web Running，Brain/updater Stopped；本轮只读核对，没有原生恢复动作。 |
| 协议缺口 | 原 journal 没有升级前 `ReleaseRecoveryBaselinePrepared`；旧 Host/Web/updater 不认识 v2 facts/Aborted。 |

保留的历史材料包括以下摘要，均为本轮重新计算；路径在 ignored 的 `target/installed-status-authorization-evidence/`。它们不含密码/API key，也不因在本地存在就自动成为可信授权。

| 原文件 | SHA-256 |
|---|---|
| `signed-inbox-after-start-20261102.json` | `4da0f673ff9fb9c31410fa6b6d2651c62bdd54253a702059a9a55fc471156707` |
| `service-path-repair-20261102.json` | `8800fcae667836696808ac8e9fbceac90fd41515a9834db5ad098292705f2aba` |
| `upgrade-after-path-owner-intent-20261102.json` | `757d82e033314face57ca5d4ac7efa0f28857bdb9a6806940a9c13eff8c45c8d` |
| `upgrade-after-path-owner-result-20261102.json` | `982e289ed80e2ce6512385cf9bf3b663f30a45195f6405bd613496e3a6ca334d` |
| `restored-web-auth-observation-20261102.json` | `12184715565dda90ba95394dc84868252920482cdf02b93dca71cdaa8c4afc69` |

原离线入库报告能证明当时实际解密检查到的 owner/store/schema/数据库摘要和事件水位，但不能补出未记录的数据库版本、完整 worker 水位或服务恢复配置。后续必须列出每项信息来自何处及其可验证性；缺项保持拒绝，不能让 owner 的一句批准变成缺失证据的替代物。

## 2. 推荐的独立历史审查通道

正常恢复协议和“无基线则拒绝”的测试全部保留。历史兼容采用单独的、显式标注证据来源的本机恢复审查，不往原 journal 回填一条声称发生在升级前的 baseline。

审查对象至少固定 schema、installation/owner、原 operation/command、原 journal 的精确 head/sequence、当前配置/active release、历史档案闭合集合与摘要、当前实际数据/SCM/worker 观察、独立修复包摘要、reader/updater 兼容下限、审查结论与期限。文件路径只由受保护的本机修复配置确定，Web 只能引用 opaque review id/digest，不能上传“已经验证”的布尔值、程序或任意路径。

每一项通过都必须有独立依据：

1. 校验完整原 journal 因果链，确认原阶段仅到 Quiescing，没有备份/切换 intent、image claim、config release binding 或 active release 变化。核对原固定 updater 的实际二进制与对应版本的“intent 先于副作用”实现；未知代码不能仅凭缺 marker 被认定安全。
2. 以旧档案中的原数据库摘要、owner/store/schema 和事件水位对照当前只读、独占的实际检查。实际 schema 重新测量，明确标成当前值；不填写旧档案中的 null。任何数据变化或锁冲突均停止。
3. 对照独立签名内容/安装记录与历史受保护回执核实原六程序、配置和服务身份；已获批的同文件路径表示修复单独验证，不能靠字符串规范化隐藏不同文件、账户、参数或恢复策略。
4. 读取真实 worker journal 和实际子进程/lease，拒绝 Pending/Running/Unknown。历史连续性只能由可校验档案/完整因果链建立，不能把“现在静止”说成“之前从未工作”。不能建立连续性时不生成可执行裁决。
5. 新旧 reader/updater、原配置和实际 OS 注册的兼容顺序必须完整：新事实后旧 writer 不得再启动；固定初始 updater 必须明确处理，不能以新业务 Host 自报替代。所有检查先于任何 v2 或更高版本事实。

建议审查输出区分 `Verified`、`Rejected`、`Unverifiable`，仅 Verified 可形成一个绑定完整 review digest 的独立 owner 恢复决定。新增历史裁决事实必须保留“历史档案审查”来源，原 baseline 仍不存在；一次 commit 才使原 operation Aborted、新历史恢复操作 Committed。它不表示原升级成功，也不改变任何 Agent Unknown。

精确 DTO、fact/reader 版本、授权和 apply/verify/replay 签名须先回填 PRD24–26/canonical/architecture，再写实现；本方案没有把暂定字段当作已冻结接口。不得借复用正常恢复函数抹去两种证据来源的差别。

## 3. 隔离实现的完成标准

- 从原创 v1 journal 和复制的历史材料构造测试，不复制真实账号/密钥或用户正文进入 fixture；验证已缺失基线的正常入口仍拒绝。
- 完整材料可形成独立历史审查；材料缺失/篡改/不同时点/错误 owner/store、data 漂移、未知 worker、晚阶段、原代码无法验证或旧 reader/updater 未迁移时，零裁决、零服务/数据副作用。
- 原始 journal 字节/hash 不改；独立新 intent/commit、CAS、跨域幂等和崩溃 verify 全部验证。未知结果只查原号；不删除 claim/handoff，不恢复旧 journal 伪装成功。
- 管理程序修复有独立的签名/授权和持久 binding，不能暗改初始安装 manifest，也不能以“管理修复”名义偷偷更换业务 active release。证明旧程序拒绝新事实且不会被自动恢复启动。
- 明确审查即使实现也可能对本机返回 Unverifiable；不能为了让现场通过而放宽必需材料。

## 4. 实际本机迁移前的第二道审查

隔离标准通过后，才准备可执行的精确管理员方案，至少包含：新的已验证 commit、独立本地开发签名的闭合包/摘要、固定输入/输出绝对路径、原件受保护备份、旧/新六程序及四服务绑定、产品/配置/worker/数据 head、一次性尝试 marker、逐步停止和启动顺序与有界期限。

顺序必须先让旧 writer 停止并取得原独占锁，原件保护备份完成后才可能接入新 reader；固定 updater 的更换和所有新管理绑定必须有独立回执。不能先追加新事实再尝试补上能读它的程序。失败保留现场，不回滚 journal、不重新发送原 Upgrade，也不自动恢复旧 writer。Brain 保持停止，直到裁决、后续原版本准入及正式启动条件全部满足。

届时实际动作仍需单独批准。当前不运行安装器、不替换 `C:\Program Files\ForMe`、不修改 `C:\ProgramData\ForMe`、不启停服务、不迁移 journal、不删除任何原记录，也不把本方案称为 ready-to-apply 脚本。完整 IP14–15/IP24 继续开放。

## 5. 本方案请求的确认范围

owner 已批准**独立历史审查协议的细化、文档回填、实现和隔离测试**。不包含本机迁移或原生升级。此次独立批准依据 AGENTS.md 硬规则 3；原有限恢复授权没有覆盖缺基线旧操作的新证据来源或裁决通道。批准是实现权限，不是缺失材料的替代证明，也不意味着本机一定能取得 Verified。

## 6. 精确协议：独立审查与决定

以下是本次批准后的细化，全部为 closed/schema 1 产品对象；不修改普通恢复请求的证据语义。安全摘要不包含路径、正文、密码、token、key 或 private key。`SchemaDigest` 必须为规范 SHA-256；ref 使用既有有界产品 id 校验。

| 对象 | 字段与约束 |
|---|---|
| `LegacyReleaseReviewBinding` | `schema_version, review_id, install_ref, owner, original_operation_id, original_command_digest, original_release_ref, original_release_digest, config_revision, config_digest, journal_seq, journal_head, expires_at`。绑定完整原 journal 快照；期限只用于新的审查，不延长原升级授权。 |
| `LegacyReleaseEvidenceKind` | 固定八项：`OriginalJournal`、`ArchivedDataIdentity`、`UpdaterCodeAudit`、`SourceImagesAndServices`、`ConfigurationContinuity`、`WorkerContinuity`、`ManagementReaderRepair`、`CurrentExclusiveInspection`。不是八个可由请求填 true 的开关。 |
| `LegacyReleaseEvidence` | `schema_version, kind, artifact_digest, binding_digest, observation_digest`。`binding_digest` 必须等于同一 `LegacyReleaseReviewBinding` 的 domain digest。每项 artifact 只能来自受保护配置指定的闭合集合，并由可信收集器重新读取、验证其本源及语义；浏览器不能提交本对象。 |
| `LegacyReleaseReviewStatus` | `Verified / Rejected / Unverifiable`。缺材料或无法确认来源为 Unverifiable；已证明不匹配为 Rejected。 |
| `LegacyReleaseReview` | `schema_version, binding, evidence[], status, issues[]`。evidence 最多八项、按上表严格顺序且不可重复；Verified 必须恰好八项且 issues 为空；其他状态至少一个安全 issue ref。report digest 覆盖完整对象和固定 legacy-review domain。 |
| `SignedLegacyReleaseReview` | `schema_version, review, signature:64 bytes`。签名只来自独立审查权威，验证公钥由本机受保护修复配置预置，不能由 review、包或 Web payload 携带。签名只证明既定权威出具了这份报告；必须先完成真实材料核验，不能用签名代替证据语义。 |
| `LegacyReleaseRecoveryRequest` | `schema_version, operation_id, idempotency_key, expected_revision, owner, review_id, review_digest, requested_deadline`。只引用 Host 已持久化、可验签的报告，不上传 report、路径、证据或成功断言。新授权窗口最多 120 秒。 |
| `LegacyReleaseRecoveryVerification` | `schema_version, operation_id, request_digest, expected_revision, owner, requested_deadline`。只核实原决定，有独立 digest 和重新认证；不能创建替代决定或延长原升级。 |
| `LegacyReleaseRecoveryView` | `schema_version, request, request_digest, revision, stage:Intent/Committed, review_digest, receipt_digest?`。Intent 无 receipt；Committed 必须有，且引用原报告。 |
| `LegacyReleaseRecoveryList` | `schema_version, product_revision, owner, operations[]`。只含当前 owner 的最近至多 25 项，按 revision 严格降序，操作号不重复且水位不超过 product revision；读取不推进操作。 |

schema 与摘要结构校验属于 protocol；材料来源、实际文件、签名及独占检查属于 Host/收集器；纯 journal fold 不打开文件、不调用服务或模型。反序列化一个自洽报告不等于报告可信。

## 7. 证据收集与判定算法

收集器必须从原 journal 和管理员预置修复配置派生路径，不接受请求携带路径或公钥。它持有配置锁与 worker operations 锁，按以下顺序取得材料：

1. 从当前 writer 快照取得 install、完整 head/seq、原 command/takeover 和阶段；原 deadline 已过，只允许未完成的 Quiescing。原 baseline 必须确实缺失；有 baseline 的正常情况仍走普通入口。
2. 验证整个原因果链没有备份/切换/checking/image claim、业务 active release 或 release config binding 变化。单个 marker 不存在不是证据。
3. 校验闭合历史档案的原字节摘要及其独立来源，读取原 owner/store/schema/数据库摘要和事件水位，与当前独占只读数据检查逐项比较。旧 `database_schema_version=null` 保留 null；实际 schema 版本只记为本次观察，不回填历史。
4. 核实原 updater 精确 image 与被审计源码/构建来源，证明该版本先写 intent 再发生相关动作；无法确认实现时 Unverifiable。历史 worker 连续性必须有独立档案或完整可核实因果链，当前静止不够。
5. 核实原六程序、配置、服务账户/参数/恢复策略及唯一获准的路径表示修复。签名管理修复回执必须绑定新 reader/updater 至少 3、实际文件及 OS 注册、原 installation 和业务 active release；不能改写初始安装 manifest。
6. 实际 Brain/updater 停止，Host/Web 可维护；worker 无 Pending/Running/CancelRequested/Unknown。数据独占检查与 worker fence 保留到提交完成或明确释放，任何不明结果拒绝。
7. 八类材料全部核实才构造 Verified 并由独立审查权威签名；缺项保留具体安全 issue ref，拒绝的审查不写可执行裁决事实。当前收集器未能实现某类原生核验时必须 Unverifiable，不提供默认成功 adapter。

正式 apply 和 verify 都要重新取得同一组真实观察并比较完整 report binding；首次只读报告的存在不能替代执行前重查。报告过期、journal/config/worker/data/program/SCM 漂移或不同 owner 均拒绝。恢复 intent 引起的唯一 journal 变化由原 request 精确记录；verify 不要求伪造最初 head，但必须验证从原 head 到当前只有同一恢复 intent 及允许的只读审计，没有旁路操作。

## 8. journal、接口与恢复

产品 journal 增加独立 `LegacyReleaseRecoveryIntent` 与 `LegacyReleaseRecoveryCommitted`，最低 envelope schema 为 **3**，不借普通 `ReleaseRecoveryBaselinePrepared/Intent/Committed` 伪装来源。Intent 包含完整、已经验签的安全 review、原 request、actor 和时间；Committed 绑定 request/report digest、执行前最终观察摘要、owner 授权期限和时间。一次 committed fact 原子使原维护 operation Aborted、新历史恢复决定 Committed，原 baseline 仍缺失，所有历史字节/hash、失败和 marker 保留。

reader 支持 v1/v2/v3 且 schema floor 单调；v1/v2 reader 遇 v3 拒绝。固定 updater 能力探测报告真实上限；普通恢复兼容准确的 2/2 或 3/3 组合，历史通道仅接受 3/3。新管理代码的业务发布声明最低 upgrader 3，实际管理程序修复继续由第二道精确管理员计划完成。不能为兼容而让旧程序跳过新事实。

Host 的独立接口为 `legacy_release_review`、`owner_legacy_release_recovery`、`verify_owner_legacy_release_recovery`、`owner_legacy_release_recovery_operation`、`owner_legacy_release_recoveries`。它们沿用 Configuration 域真实认证与 owner/操作绑定的单次口令重认证，不接受 LocalAdministrator 字符串或 body owner 自证。内部路由使用 `/v1/host/configuration/legacy-release-recovery` 及其 `review/{original_id}`、`operations/{id}`、`operations`、`verify` 子路径；公开 Web 对应 `/api/v1/product/legacy-release-recovery`。读取只查报告/原号，不重采后暗中提交。

同 key 同 request 是纯查询，即使过期也不重试；异义、跨普通维护/普通恢复/历史恢复的 key 或 operation 碰撞均拒绝。历史 intent 参与同一全局维护互斥。重启只 fold，不收集、签名、停服务或提交；只有 owner 显式 verify 原号并再次认证，才可重新核实和结束 intent。已 Committed 的查询不重复动作。不能开放普通维护入口来直接转阶段或绕过旧操作的 Unknown。

## 9. 分层实现与证据界限

按协议与签名/结构反例 → 真实临时 journal 的 CAS/幂等/重放/原字节保持 → 原创文件/受控 collector 的材料缺失/漂移/错误来源 → 正式认证 HTTP 与只读 Web → 原生收集器和第二道本机方案推进。每层单独记证据；可信 collector fixture 不能证明真实 SCM、历史原件或本机数据已经核实。任何未实现依赖继续不可用，不能用报告结构存在声明整条历史通道已闭合。

### 9.1 Host 隔离接缝与时间边界

本轮增加 Host 内部的独立审查接缝，不改变已有 HostManagement 或脑 trait。`legacy_release_review(auth, original_id, workers, now)` 只读；`owner_legacy_release_recovery(auth, request, workers, now)` 与 `verify_owner_legacy_release_recovery(auth, verification, workers, now)` 返回 `LegacyReleaseRecoveryView`；`owner_legacy_release_recovery_operation(auth, id)` 只查原号，`owner_legacy_release_recoveries(auth)` 返回 `LegacyReleaseRecoveryList`。均使用 Configuration 域身份，核对持久配置的真实 owner；产品 HTTP 的口令重新认证仍由 PRD25 规定的认证层承担，Host 方法本身不能证明浏览器已重认证。

独立公钥和 collector 只能由受保护的 Host 装配提供；包、报告和请求均不能提供公钥。Ed25519 严格验签的消息为 `LegacyReleaseReview::digest()` 所得完整 `sha256:...` 字符串的 UTF-8 字节，摘要域沿用 §6。collector 读取其自身固定的持久报告，重新验证八类材料的来源与语义，并在持有数据独占观察期间重查；Host 另外强制核对真实 worker fence、服务管理器观察到的 Host/Web/Brain 状态、collector 对固定独立 updater 的身份/有限恢复策略/Stopped 与准确 3/3 能力观察，以及配置/active release 与 journal CAS。最终观察摘要覆盖完整已签报告摘要、八项重查结果和 worker 水位。单有一个可解析或可验签的报告不允许提交。

首次请求必须在原 takeover 授权已过期后发起，新的请求/核实各自最多 120 秒，且都不得超过报告期限。核实可以晚于首次请求期限，但必须仍在报告有效期内，绑定同一 request digest 和 intent revision；提交时间不得早于 intent。query/idempotent lookup 不延长期限、不重查后提交。原 takeover 的 deadline 永不修改。

生产装配在真实原生 collector、独立信任配置和管理 reader 修复尚未闭合时不挂载该接缝，Host 返回 unavailable；不得提供默认 Verified collector 或借测试注入开启生产。隔离 collector 只验证编排、验签与拒绝语义。正式 HTTP/Web 接线与本机迁移分别记录证据，不把私有 journal 场景称为 Host/HTTP 验收。

### 9.2 正式 HTTP/Web 接线与中断后只读恢复

本节细化已批准的五个接口与原号核实语义，不新增 DTO、trait 或授权来源。Host 路由仅在 Configuration listener 认证后分派；Maintenance 和 AgentExecution 域不能使用这组路径。Web 的两个 POST 先核对实际 owner session、Origin/CSRF、`Idempotency-Key=operation_id`、closed body 和独立 request/verification digest 的口令重新认证，再转发不含密码的产品对象；请求不能指定 owner、报告正文、证据、文件路径、公钥或 Verified。

页面刷新只 GET 最近列表和原报告。对已记录的 Intent，Host 读取 journal 中与该原升级绑定的**同一份已签报告**，仍核对签名、owner/config/active release、原 request、期限与 intent 水位；不得重新出具报告、替换 digest、追加事实或调用 collector 来完成决定。原请求已过期但报告仍有效时，查询仍可取得该报告的原期限；报告过期或水位/绑定漂移则拒绝。首次新请求仍要求报告的原 head/sequence 为当前 checkpoint，普通缺基线入口继续拒绝。

显式 verify 使用查询取得的报告原期限及当前 intent revision，重新认证且最多授权 120 秒；不能猜测期限、重发初始 apply 或延长原升级。Host 在 verify 中重新取得真实材料/worker/服务观察并执行 CAS。断线只保留原号查询；已 Committed 的结果不能再次执行。owner/API/维护水位变化后，页面丢弃旧报告并屏蔽迟到响应。

浏览器重建 report binding/report/request digest 时，沿用 protocol 的 domain 与 Rust struct 声明字段顺序；不使用 sorted-key JSON 或 HTTP 属性到达顺序。Rust/TypeScript 使用共同 golden 验证，浏览器检查只用于发现传输/绑定错误，不能替代 Host 的签名、材料来源和执行前重查。组件与正式 HTTP 的证据见[接线验收](../../acceptance/installed-legacy-release-http-web-20261105.md)，原生材料和实际迁移继续单列。

### 9.3 受保护管理修复的私有证据

本节细化已经批准的 `ManagementReaderRepair` 依赖，不新增 Web DTO、脑事件、冻结 trait、授权域或 unsafe 边界；实际管理员 Apply 仍受 §4 的第二道精确审查约束。独立修复只可更换初始固定目录内的 Host（与 fixed updater 共用）、Web 与 hostctl，保留 Brain/CLI/executord、初始 InstallationManifest、业务 active release、六配置、原 journal、数据与 keys。原 Upgrade 的 dispatch marker/handoff 不删除、不重新派发。

以下私有磁盘对象均为 closed/schema 1，字段顺序固定；不是浏览器可提交的协议对象：

| 对象 | 精确字段 |
|---|---|
| `RepairImageBinding` | `schema_version, image, source_size, source_digest, target_size, target_digest`。image 固定顺序为 `host, web, brain, cli, hostctl, executord`；只允许 host/web/hostctl 的 source 与 target 不同。 |
| `RepairPlan` | `schema_version, repair_id, distribution_digest, install_ref, installation_digest, owner, original_operation_id, original_command_digest, original_release_ref, original_release_digest, journal_seq, journal_head, config_files_digest, service_configuration_digest, images, journal_read_max, updater_version, authorized_at, authorization_deadline`。准确 reader/updater 3/3，授权时间窗正数且不超过 15 分钟。 |
| `RepairReceipt` | `schema_version, repair_id, plan_digest, distribution_digest, install_ref, installation_digest, owner, original_operation_id, original_command_digest, original_release_ref, original_release_digest, journal_seq, journal_head, config_files_digest, service_configuration_digest, installed_images_digest, journal_read_max, updater_version, committed_at`。所有共同字段必须等于 plan；commit 必须在原授权窗口内且不在未来。 |
| `SignedPlan / SignedReceipt` | 分别为 `schema_version, plan/receipt, signature`，signature 恰为 64 bytes；不带验证公钥或路径。 |

plan/receipt 的签名消息分别为 `canonical_digest(("forme-management-reader-repair-plan-v1", plan))` 与 `canonical_digest(("forme-management-reader-repair-receipt-v1", receipt))` 的完整 `sha256:...` UTF-8 字符串。`installed_images_digest` 使用独立域 `forme-management-reader-repair-images-v1`，覆盖固定六项 `(image, target_size, target_digest)`。receipt 的长期重读不要求旧授权窗口仍有效，也不因此重新获得 Apply 权限。

修复材料使用已有完整签名 ReleaseManifest 与闭合 content；manifest、plan、receipt 独立签名且不能循环引用摘要。三者使用分发 manifest 的 signature_ref，在管理员独立预置的 release trust 中验签；材料包和 Web 不能增加信任根。三个可变 image 的 target bytes/digest 必须来自同一分发；其他三个 image 保留原字节，不能借完整分发暗换业务程序。该有限适配只针对原业务仍处于初始 release 的历史安装，其他布局保持 unavailable。

固定根为 `installation.paths.host_state/management-repair`，只含 `receipt.json` 和一个目录；目录名为 repair_id UTF-8 的 SHA-256 小写 hex，不直接把 opaque id 拼为 Windows 路径。该目录只含 `plan.json` 与 `distribution`；distribution 只含 `manifest.json`、`signature.ed25519`、`content`。内容逐项受原 ReleaseManifest 闭合集合约束。记录 ≤64 KiB、manifest ≤8 MiB、单个内容/程序 ≤512 MiB、分发总量 ≤2 GiB；拒绝链接、hardlink alias、额外文件、未知字段、无界或冲突材料。

实现明确分为两层：`ManagementReaderRepairEvidence::load` 仅验证独立信任、完整分发、签名及原安装/owner/operation/release/journal checkpoint 的绑定；它不返回历史审查 Verified。`inspect_current` 才读取固定六个实际 image、六配置、真实四服务的身份/参数/恢复策略和固定 updater 的实际 3/3 离线能力，核对与签名 receipt 一致。Windows 保持实际文件与祖先目录的只读 handle custody，拒绝多硬链接，直到 inspection 释放；提交前 `recheck` 重查记录、闭合文件集合、程序、配置、SCM 与准确能力。非 Windows 没有原生适配时拒绝，受控观察只用于隔离测试。

这些动作不写 product/brain journal、不备份或切换业务数据、不启停服务；离线 capabilities 子进程只读固定协议。原 updater 源码/构建来源、历史材料本源、worker 因果链和数据独占检查仍属于其余七类材料，不能由一个可验签管理回执代替。真实 collector、精确本机修复与 IP14–15/IP24 的整体状态仍独立登记。

### 9.4 原持锁 writer 的真实 journal 检查

`OriginalJournal` 不由 collector 的报告或另一个文件副本自证。Host 从产品 journal 的原持锁文件句柄读取有界原字节，沿用既有 closed envelope、原始 JSON fact 摘要、schema floor 与纯 projection fold，重新验证完整因果链；实际末尾的 sequence/head 必须仍与该 writer 的内存投影一致。检查结束恢复句柄位置，不追加事实、不打开第二 writer、不解除 OS 锁。实际读取、重放或位置恢复失败后，该 writer 拒绝继续变更。

报告的原 checkpoint 必须在实际链中存在且 head 一致。首次观察要求它就是当前末尾；原号 verify 只容许其后恰好存在同一份已保存报告绑定的历史恢复 Intent，仍以既有全局互斥、原 operation 阶段、配置及 CAS 规则校验。任意其他后缀、提前阶段、晚阶段、已完成决定、缺基线条件不符或不同 owner/command/release 均拒绝。

该项 `artifact_digest` 为直到原 checkpoint（含各行换行）的真实原字节 SHA-256；`binding_digest` 沿用同一 report binding digest；`observation_digest` 为 `canonical_digest(("forme-legacy-original-journal-v1", binding_digest, prefix_byte_length, original_takeover_digest))`。摘要不含原文或路径。Host 在真实 inspection 之内、决定追加之前重新计算并与已签报告的第一项逐字段比较，不因 collector 回传相同报告而省略原句柄检查。

这只闭合原 journal 的本源与因果核查，不代替历史数据、原 updater 审计、worker 连续性、管理修复、独占数据检查或完整原生 collector；未实现的材料仍保持 unavailable。

### 9.5 管理修复前的离线原件观察

精确管理员计划可使用独立的 `forme-hostd --inspect-legacy-repair-source --installation <installation.json> --operation <original_id>` 只读入口准备 §9.3 的 journal binding。该入口在服务 dispatcher、运行时装配、配置/凭据加载之前退出，不使用 `ProductJournal::open`，不创建任何文件或 InstallationBound，不启动服务、worker、模型或监听器。它只接受原 manifest 的精确固定配置位置，从 manifest 推导 `host_state/host.journal`；不接受浏览器或网络请求。

安装 manifest 和祖先目录保持只读 custody，原 journal 必须是已存在、单硬链接、有界的普通文件，读取前取得独占 OS 文件锁。现有 Host 正持锁时拒绝，不能绕过锁读取“足够新”的副本；本机停服仍须 §4 的具体批准。原字节沿用 closed envelope、原 fact JSON 摘要、单调 schema floor 和完整 projection fold，核对原 installation manifest digest、原 release 及指定 operation/takeover 的绑定；未知 schema、截断、损坏或身份不符均拒绝，不修复日志。

输出为仅序列化的私有 schema 1 `LegacyRepairSourceSnapshot`：`schema_version, observed_at, install_ref, installation_digest, journal_seq, journal_head, journal_bytes, journal_digest, journal_schema_floor, owner, original_operation_id, original_command_digest, original_release_ref, original_release_digest, stage, pending_stage, config_revision, config_digest, takeover_digest, takeover_deadline, images_claimed, checking_prepared, recovery_baseline_present`。字段全部从原 manifest、原 bytes 和重放投影派生，无路径、nonce、凭据、正文或调用者提供的成功断言。观察可以描述非 Quiescing 状态；生成修复计划及历史裁决的阶段限制保持不变。

该快照只证明本次离线读取及其水位，释放锁后必须重新核查。它不是签名 RepairPlan/Receipt、历史证据 Verified、缺基线裁决或新授权；不能据此启动原升级或写任何新事实。缺文件、空文件、占用、别名和因果破坏必须以原文件零写入的反例验收。候选分发仍绑定它本身已验证的源码和程序摘要，不因增加这一独立预检工具而反复封存同一候选。
