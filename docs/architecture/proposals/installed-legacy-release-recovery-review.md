# 历史安装缺少恢复基线时的独立审查与迁移方案

状态：**owner 已批准协议细化与隔离实现；本方案仍不是本机 Apply 授权。** 本方案承接已批准并实现的[有限恢复契约](installed-release-interruption-adjudication.md)，只处理正常 `AbortBeforeSwitch` 明确不适用的旧安装。当前仅落地冻结口径、产品协议和 journal/replay/CAS 隔离验证；确切签名包、程序摘要与实际迁移脚本必须另行设计和批准，不能倒置顺序。

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

### 2.1 本轮冻结的产品 DTO 与事实

本轮将上述边界冻结为 schema 1 的 `LegacyReleaseRecoveryEvidence`、`LegacyReleaseRecoveryReviewRequest`、`LegacyReleaseRecoveryReviewVerification`、`LegacyReleaseRecoveryReviewView` 和 `LegacyReleaseRecoveryReviewList`。证据 status 只有 `Verified`、`Rejected`、`Unverifiable`；只有 `Verified` 且所有历史/当前摘要完整一致时才允许 request。request digest 覆盖完整证据，journal 再以安装身份、owner、原 operation/command/release、当前 head/sequence 和 CAS 重核，避免浏览器或当前状态替换历史来源。head 与完整历史摘要是独立证据字段，不能以其中一个替代另一个；verification 只能绑定原 review id 与 request digest 重新核实未完成 intent。

产品 journal 新增 schema 2 facts `LegacyRecoveryReviewIntent` 与 `LegacyRecoveryReviewCommitted`。它们只产生产品侧历史审查记录；commit 原子将原 operation 置 `Aborted`、review 置 `Committed`，保留原历史和 active release，不做 backup/switch/service/data/journal migration，不产生 brain event。旧 reader 遇到新 fact fail closed；没有受信 Host 证据采集器时不开放 Web/Apply 路由。

## 3. 隔离实现的完成标准

- 从原创 v1 journal 和复制的历史材料构造测试，不复制真实账号/密钥或用户正文进入 fixture；验证已缺失基线的正常入口仍拒绝。
- 完整材料可形成独立历史审查；材料缺失/篡改/不同时点/错误 owner/store、data 漂移、未知 worker、晚阶段、原代码无法验证或旧 reader/updater 未迁移时，零裁决、零服务/数据副作用。
- 原始 journal 字节/hash 不改；独立新 intent/commit、CAS、跨域幂等和崩溃 verify 全部验证。未知结果只查原号；不删除 claim/handoff，不恢复旧 journal 伪装成功。
- 管理程序修复有独立的签名/授权和持久 binding，不能暗改初始安装 manifest，也不能以“管理修复”名义偷偷更换业务 active release。证明旧程序拒绝新事实且不会被自动恢复启动。
- 明确审查即使实现也可能对本机返回 Unverifiable；不能为了让现场通过而放宽必需材料。

本轮“实现完成”仅指上述 closed DTO、产品 journal facts、replay/CAS/幂等投影和隔离测试闭合；不指本机旧安装已裁决、程序已替换、journal 已迁移或服务已启停。

## 4. 实际本机迁移前的第二道审查

隔离标准通过后，才准备可执行的精确管理员方案，至少包含：新的已验证 commit、独立本地开发签名的闭合包/摘要、固定输入/输出绝对路径、原件受保护备份、旧/新六程序及四服务绑定、产品/配置/worker/数据 head、一次性尝试 marker、逐步停止和启动顺序与有界期限。

顺序必须先让旧 writer 停止并取得原独占锁，原件保护备份完成后才可能接入新 reader；固定 updater 的更换和所有新管理绑定必须有独立回执。不能先追加新事实再尝试补上能读它的程序。失败保留现场，不回滚 journal、不重新发送原 Upgrade，也不自动恢复旧 writer。Brain 保持停止，直到裁决、后续原版本准入及正式启动条件全部满足。

届时实际动作仍需单独批准。当前不运行安装器、不替换 `C:\Program Files\ForMe`、不修改 `C:\ProgramData\ForMe`、不启停服务、不迁移 journal、不删除任何原记录，也不把本方案称为 ready-to-apply 脚本。完整 IP14–15/IP24 继续开放。

## 5. 本方案请求的确认范围

只请求批准**独立历史审查协议的细化、文档回填、实现和隔离测试**。不请求批准本机迁移或原生升级。原因是 AGENTS.md 硬规则 3 要求冻结协议变更先经确认，且前一批准范围没有授权为缺基线旧操作增加另一种证据来源或裁决通道。
