# 原安装恢复：证据状态与旧 updater 源码审查（2026-11-06）

本记录集中描述原 installation 的恢复依赖，承接[已批准历史恢复协议](../architecture/proposals/installed-legacy-release-recovery-review.md)与[原件采样记录](installed-source-window-and-data-custody-20261105.md)。它不是新的恢复授权、签名历史报告或 IP PASS；不改变冻结接口，不把源码推导升级成真实副作用观察。

## 1. 当前现场和执行范围

本轮只读 SCM 复核：Host、Web、Brain、release-updater 均 Stopped/PID 0；Host/updater ExitCode 1，Web/Brain ExitCode 0。Codex 当前进程没有 Windows 管理员令牌。本轮没有发起 UAC、修改 SCM、启动服务、替换已安装文件、迁移数据或重新派发原 Upgrade。网页没有由本批恢复。

[原 Host 路径恢复计划](installed-original-host-path-restore-20261105-plan.md)已准备，固定脚本 SHA-256 本轮复算仍为 `93defb96100806aac062a3298cadee9d098144b563f9ae657e38a7fd2a9d43bf`。一次 Apply 的精确问题已发出但尚未取得答复；此前“继续”对应只读 UAC，不代指这个新增 SCM 变更。只读预检也没有新的管理员结果，不能记作通过或反复发起无新价值的系统弹窗。

原 operation 仍为 `maintenance:acceptance-upgrade-after-path-20261102-db96ec2`；原 installation、业务 active release、缺 baseline 的状态和真实产品水位以重新取得的原件检查为准。下文 11 月 5 日文件指历史采样，不能代替未来执行前的重查。

## 2. 八类材料：已实现部分与未闭合依赖

| 材料 | 已有实现或实际观察 | 原生恢复仍缺什么 |
|---|---|---|
| `OriginalJournal` | 原持锁 writer 的有界重读、hash/schema/因果链和 checkpoint 检查已有生产实现；11 月 5 日离线原件采样实际成功。 | 实际修复/裁决时重新取得同一原件水位，纳入完整原生 collector；旧采样不作为持续锁。 |
| `ArchivedDataIdentity` | 私有来源验签、原档案 closed reader、身份比较与 custody 场景已实现，模块目前仅在测试编译。 | 独立核实并预置的原来源记录、公钥与摘要，以及生产 collector 装配。本机尚无可接纳来源，原报告存在/摘要相同不足以取得 Verified。 |
| `UpdaterCodeAudit` | 本节下方已审查精确历史 Git 源码，重算原候选和有关档案摘要，确定备份/切换的 intent 顺序。 | 审查权威可接纳的原二进制到被审计源码/构建来源的绑定；现存包装 receipt 不能独立证明该绑定。 |
| `SourceImagesAndServices` | 11 月 5 日管理员预检记录六程序摘要、四服务身份/参数/有限恢复策略；本轮原候选文件摘要与该历史 Host 观察一致。 | 收集器中的独立原来源核实、当前受保护原文件 custody 与执行前真实 SCM 重查。不能用旧输出代替当前文件。 |
| `ConfigurationContinuity` | 历史预检记录六配置原字节/ACL 摘要；同文件路径表示修正有独立记录。 | 完整连续性核实与当前重查；拟执行的 Host 路径恢复也必须保留独立记录，不能规范化后隐藏变更。 |
| `WorkerContinuity` | `f83b63e` 已修复当前原 worker journal 与内存水位一致性；`eb99013` 进一步要求调用者只能确认原件已保存的完整 Host 回执，真实 Windows Job 自报终态反例及本批完整质量/宿主门通过，见[回执来源验收](installed-worker-receipt-origin-acceptance-20261106.md)。 | 历史独立来源、实际进程/lease 守卫与连续性、原生 collector。资源守卫的新增平台接口尚待确认，当前静止/空日志不证明历史从未执行。 |
| `ManagementReaderRepair` | 签名分发/plan/receipt 读取、实际六程序/配置/服务/准确 3/3 能力检查及持有期 custody 已实现。 | 精确签名修复材料、原生 Apply 的单独方案与授权，以及实际新 reader/updater 安装回执；本轮未替换任何程序。 |
| `CurrentExclusiveInspection` | 固定数据 child 的独占检查与提交前存活/传输/期限检查已有实现和实际临时数据子进程场景。 | 覆盖实际本机裁决窗口的原数据检查与持锁证据，及收集器中的完整组合。临时数据测试不证明原脑通过。 |

完整生产 `LegacyReviewCollector` 目前仍未挂载；上述任一材料未闭合就不能将原安装报告为 Verified。独立签名只证明出具者，不替代材料语义。不得给现有无来源报告现场生成 origin、倒填时间或补签使其通过；缺历史来源属于证据限制，继续重跑源码测试不能补出它。

## 3. 精确历史源码与执行顺序

审查的提交为 `694b6bba98e9a89119001bf5ce12f717ce9da5eb`，不是当前 HEAD。下列行号仅属于该提交，可用 `git show 694b6bb:<path>` 核对；不能跳到当前文件同一行代替历史实现。

| 历史文件（均在 `crates/execution/src/installed/`） | Git blob |
|---|---|
| `host_release_takeover.rs` | `8f1ae8a0ab06f52ccf8f69d22a5c3d592c4a08ba` |
| `release_updater.rs` | `3fec20868c4c963223a5a8fad3b30604c88a7d5f` |
| `release_updater/transaction.rs` | `afe3eee7d1bfc7805b9d66e72486af0f10a87a2d` |
| `release_updater/storage.rs` | `dfbefa400e5a21589430e18e02f55a3808b0a7a4` |
| `release_services.rs` | `2ea53fa0fdb4f832770d57488dfe803d6f933858` |
| `journal.rs` | `ef2634431cd5fbf7fadce6e466b186437c56c419` |

同一提交的 `Cargo.lock` blob 为 `e295d0ab305d3346da2100fdc5ff39c188c239a0`，`rust-toolchain.toml` blob 为 `34d2cd6609c69ef0adf9d3c8bd8cd7c162fe7a52`。这些是被审查源码的固定身份，不证明原 exe 实际由它们构建。

1. Host 先记录 Validated、takeover 与 Quiescing intent，再发布 handoff（`host_release_takeover.rs:130–148`）。旧 updater 的版本常量为 1；新历史恢复要求的准确 3/3 并不存在于它。
2. updater 在第一次 Stop 前创建并 sync 一次性 dispatch claim（`transaction.rs:25`、`storage.rs:34–61`）；随后可依次停止 Brain/Web/Host，此时产品 journal 仍可能只到 pending Quiescing。它要等 Host 释放 writer 后才打开 journal，因此缺 BackedUp 不代表没有服务停止或 dispatch marker。
3. 备份前先完成 Quiescing，再 `begin(BackedUp)`（`transaction.rs:103–109`），随后才启动数据 worker 并发送 Backup（同文件 `123–132`）。
4. image/SCM 切换前先 `begin(Switching)`（`transaction.rs:140`）；`PreparedReleaseServiceSwitch::execute` 又在 `switch_and_verify` 前持久 claim images（`release_services.rs:204–217`）。release config binding 与 active release 的变更更晚（`transaction.rs:184–192`）。
5. `begin_stage` 等事实经 `write_fact` 执行 expected revision 检查、先在副本 fold，再写原文件并 `sync_all`，最后更新内存 projection（`journal.rs:433–468`）。写失败会保留 poisoned，不允许继续依赖旧缓存追加。

因此，**在实际运行 image 确实对应这些源码、完整原 journal 未丢失/替换/截断、没有旁路写入且实际数据/服务/worker 连续性均另行核实时**，只有 pending Quiescing 的原件能支持“该源码路径尚未进入备份或切换”的判断。它不能单独证明没有任何副作用，也不能代替其余材料。没有重跑原 updater 来验证这一推导。

## 4. 本轮重算的现存档案

| 固定本地工件 | SHA-256 |
|---|---|
| `C:\code\forme\target\product-bundles\forme-windows-x86_64-20261027-694b6bb\bin\forme-hostd.exe` | `570549c220f410f585c30b3c0819146a5e99fedb36afc18a4702fe59de08fcf2` |
| 同 bundle 的 `bundle-inventory.json` | `8a900e91763871d1bae9dff1a1ea664bc8362fe1ab14d924c9a5bd0ee90d89ca` |
| 对应 `-package\native-build-receipt.json` | `ccb7c517e8f8ce7bf5ed1f4d91af159f3fb50d6a47122418fb51f5fd0d9303c7` |
| `C:\code\forme\target\installed-file-worker-evidence\native-host-diagnostic-replacement.ps1` | `70338f9252f4a68a8363b4b2068a920ffab2f400704895253d7a0672b97b7380` |
| 同证据目录的 `native-host-diagnostic-replacement.json` | `9ab46bea74c8383fc6e6141fc474ee5c684243798f17b94a8bbb07e003af721c` |
| 本工作树 `target\legacy-release-review\native-source-preflight-20261105.json` | `b52420f67225f541a56f33f56a3030ebc1b7f4b2b4c2b725214e887843fb3f0f` |

旧 diagnostic 脚本锁定的候选摘要、bundle inventory 的 Host 摘要、11 月 5 日管理员预检记录的实际 Host 摘要与本轮原候选重算一致。updater 在该安装中复用这个 Host image 的 `--release-updater` 模式。这建立了现存候选和历史观察之间的摘要对应；本轮没有重新读取受保护的当前 installed exe。

原 `native-build-receipt.json` 记录的是 Inno 安装器包装、输入 bundle 摘要、工具/产物摘要和 `header_only` 目标检查，声明 unsigned/development/release_ready=false；它没有绑定 Rust source commit、原 Rust 编译调用或可独立接纳的构建本源。因此不能把 release_id 中的 `694b6bb` 文本和这个 receipt 当作已经完成的 `UpdaterCodeAudit`，也不能在本轮补签后升级为历史来源。

## 5. 后续执行顺序与本次结束边界

1. 取得此前已发出的一次路径恢复 Apply 的精确答复后，按该固定计划核对原件、只修正 Host SCM image 的路径分隔符，并限定启动原 Host/Web。没有答复时不执行；失败保留现场，不重试。
2. 在原文件与 Unknown 历史完整保留的前提下，补齐生产 collector、可独立接纳的历史来源、签名管理修复和准确 native 迁移方案。缺原来源时明确保持 Unverifiable；修复程序与裁决原事务是两个有各自证据的动作。
3. 原安装修复和原操作显式裁决之后，才以新的授权事务继续升级、实际模型/worker/helper 与 IP01–IP24；不重发已过期的原 Upgrade，也不把恢复网页称为整个目标完成。

本轮实现仅为 [worker 原件核验 `f83b63e`](installed-worker-original-journal-acceptance-20261106.md)，其源码质量门与宿主门已实际通过。此处新增的是只读源码/档案审查，不新增测试通过数，不刷新 M5/发布回执，不合入 main 或推送。整体目标保持未完成。
