# 历史升级原生材料的只读预检（2026-11-05）

承接已批准的[独立历史审查](../architecture/proposals/installed-legacy-release-recovery-review.md)。本预检只补充材料清单，**不生成可执行审查、不挂载 collector、不替换程序、不迁移 journal、不启停服务、不重跑升级**。当前源码 HTTP/Web 实现和回归另见[组件报告](installed-legacy-release-http-web-20261105.md)。

## 1. 新找到的历史材料

除原方案五份档案外，安装验收还保留了下列独立只读观察。以下摘要是本次重算，不应误写为当时已经签名或受保护的来源证明。

| 文件（原工作区 `target/installed-file-worker-evidence/`） | SHA-256 | 可提供的线索 |
|---|---|---|
| `native-host-readonly.json` | `8c86b4f4343f26a49a8f67fec6843f4db6649fa4cfc963495c3354fdfe8ff2b9` | 首装后的固定文件、服务及原生查询观察。 |
| `native-host-readonly-v2.json` | `f8dba2e835cccdd7b620b0f908e6309a3b8f520a21d49d486a4b6aff4b2060de` | 当时 `file-worker.journal`、`sandbox-leases.jsonl` 均为 0 bytes；安装目录和原生查询。 |

原方案的 `service-path-repair-20261102.json` 确实包含 before/after 服务账户、依赖和 `NativeSecurityAndRecovery` 查询。不能继续笼统说它完全没有服务恢复策略；但其完整性、独立来源、与原程序/当前 OS 注册的一致性仍需核实。

零字节历史观察增加了 worker 连续性的线索；它尚不是可信的完整因果链。不能把当前同样静止、同样长度或一个新算摘要直接提升为连续性通过。对两个受保护的旧配置备份子目录，本次非管理员搜索遇到拒绝访问，未读取、降权或改 ACL；检查范围并不完整。

## 2. 精确只读脚本

脚本：`C:\Users\shu\.codex\worktrees\bootstrap-claim-journal\forme\target\legacy-release-review\native-source-preflight-20261105.ps1`。

SHA-256：`fd535c4e44ed2668ec1379789bef0461934d1d2508e90b344f9c3f6cb204b5d7`。只运行一次默认模式，没有 Apply 参数。输出使用 CreateNew：同目录 `native-source-preflight-20261105.json`；已有报告不覆盖。

只读目标为固定 installation `installation:969387966500ba7c649fcc449c073cd5d27042b2b2989f2eda3c7a8d5f376258`、原 release `forme-windows-x86_64-20261027-7cb5b8b`：

- 通过只读 CIM 和 `sc.exe qc/qfailure/qfailureflag/qsidtype` 观察四个固定服务，比较前后状态/PID/账户/命令。
- 核对原 installation identity；只计算六个固定配置和六个固定程序的 bytes、SHA-256 与 ACL 摘要。不读取 credential/key 文件，不输出配置正文或模型端点/密钥。
- 对 `host.journal`、`file-worker.journal`、`file-worker-runtime/sandbox-leases.jsonl` 只取长度和时间；运行中的 writer 可能持有文件，不读其内容、不解除锁、不据此裁决。
- 拒绝链接、超限、观察中变化和未知 installation。失败保留唯一报告，不自动重试；不调用模型、数据 worker、维护 POST 或任何写产品命令。

该脚本需要管理员只读访问。若 Windows 弹出 UAC，系统认证必须由 owner 操作；已有隔离实现授权不等于可以自动填写系统认证。没有原生结果前，不把语法/夹具检查写成管理员预检通过。

## 3. 八类材料当前边界

| 材料 | 此预检可补充 | 仍须完成 |
|---|---|---|
| OriginalJournal | 活文件元数据和最近已保存维护观察。 | 原 writer 协同下完整因果链/head/seq 核实，不能读锁失败后改锁。 |
| ArchivedDataIdentity | 原离线档案已有 owner/store/schema/数据库摘要和事件水位。 | 当前真正的只读解密与独占检查；旧 schema null 不回填。 |
| UpdaterCodeAudit | 实际固定 updater 与 Host 共用 `bin/forme-hostd.exe`；本次摘要匹配已获批 `694b6bb` 诊断替换。`694b6bb` 与 `7cb5b8b` 的 updater transaction 相同：BackedUp intent 先于数据 worker，Switching intent 先于 image switch。 | 完整源码/构建来源与历史替换因果核验；仅 active release 名称不能证明独立 updater 的版本。 |
| SourceImagesAndServices | 原生六程序摘要、四服务参数/账户/恢复策略与历史档案。 | 所有原件来源/变化逐项对照，包含已获批诊断替换和路径表示修复。 |
| ConfigurationContinuity | 固定配置字节及 ACL 摘要。 | 与旧档案、真实 owner/config/active binding 的完整对账。 |
| WorkerContinuity | 更早的零长度观察、当前元数据。 | 独立来源、完整 worker/lease 因果链及实际子进程静止锁。 |
| ManagementReaderRepair | 当前服务/程序仍是旧管理版本的事实。 | 独立签名管理修复、持久 binding、新 reader/updater 3/3；不能先写 v3 再补程序。 |
| CurrentExclusiveInspection | 不占锁的静态预检。 | 在配置/worker/data 的真实独占边界持锁到提交/明确释放。 |

以上不是八项材料已齐全，也不是本机恢复准入。原生 collector、管理 reader 修复及实际迁移仍开放；IP14–15/IP24 与整体目标不因本预检关闭。

## 4. 实际管理员只读结果

首次管理员启动由 Windows 返回“操作已被用户取消”，脚本没有运行，报告不存在。owner 回复确认后，对同一摘要和同一只读范围重新发起；本次在本地时间 **2026-11-05 11:39:02** 完成，结果为 `observed_read_only_not_a_recovery_review`，`failure_stage=null`。报告 `native-source-preflight-20261105.json` 的 SHA-256 为 `b52420f67225f541a56f33f56a3030ebc1b7f4b2b4c2b725214e887843fb3f0f`，原件保留在 §2 的同一 ignored 目录。

- installation/release 固定身份匹配；六配置、六程序均取得有界字节和 ACL 摘要，16 项原生只读查询成功。
- 前后四服务的状态、PID、账户、启动方式和命令完全相同：Host Running/Automatic，PID 37740；Web Running/Automatic，PID 75344；Brain/updater Stopped/Manual。
- 对照 2026-11-02 路径修复档案，四服务 image 参数、账户、启动方式以及 `qsidtype/qfailure/qfailureflag` 的原生输出逐项一致。这里只统一了输出换行，不规范化路径或忽略参数；这证明这两个观察之间的可见配置一致，不证明档案本源、完整中间历史或原生 DACL 已核实。
- 实际 Host/updater 文件为 39,406,437 bytes，SHA-256 `570549c220f410f585c30b3c0819146a5e99fedb36afc18a4702fe59de08fcf2`，与[已获批诊断替换计划](installed-product-windows-host-diagnostic-replacement-plan.md)中的 `694b6bb` 候选相同。原 active release 仍是 `7cb5b8b`；二者必须分开记录。两版本的 `release_updater/transaction.rs` Git blob 均为 `afe3eee7d1bfc7805b9d66e72486af0f10a87a2d`，相关 updater 源码差异为空；这仍不单独构成可复现构建/签名证明。
- 其他五个实际程序的 bytes/SHA-256 均与 `7cb5b8b` 原 bundle inventory 一致；Host 是唯一上述已获批诊断例外。原 bundle 为开发候选，inventory 对账不等同原安装包签名或历史档案本源验证。
- `host.journal` 元数据为 14,777 bytes；worker journal 和 sandbox leases 均为 0 bytes。本次没有读取活 journal 内容、解除锁、核查实际数据库或确认 worker 完整因果链。
- 回执明确 `product_writes=0`、`service_actions=0`、`provider_calls=0`，没有读取 credential/key 文件或打开脑库。预检没有产生恢复授权或可执行 Verified 报告。

这些原生观察缩小了程序身份与服务恢复策略的不确定范围。下一步仍要完成可信历史档案、原 journal/worker 因果链、真实独占数据检查和独立签名管理修复；不能因为文件与静态服务相符，就越过缺失材料迁移现场。
