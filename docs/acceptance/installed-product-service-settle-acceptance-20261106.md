# 已安装服务终态与在线/离线备份摘要验收（2026-11-06）

承接 [PRD24](../prd/24-installer-and-service-lifecycle.md) §4.2、[产品架构](../architecture/10-installed-product-and-web-workbench-architecture.md) §9 和 IP12/IP14–15/IP24。本报告记录真实安装暴露的实现缺陷、修正及验证；不扩展冻结 DTO、trait、EventKind、crate 图或产品恢复权限。

## 1. 双向 peer 修复与此前已结束的回归

`de830a0` 先记录服务对象最小查询权限，`8fde42c` 实现双向 Service SID 的进程 limited-query，以及 Brain 向 Host 的主 token query/duplicate。未启用生产 SeDebugPrivilege，未放宽 workspace、数据、服务或程序目录 ACL。第二个原文件 Unknown `run:1793950668761966500:1` 已通过正式 owner 裁决 Cancelled，未产生新的 dispatch 或文件副作用；不改写成成功。

干净源码 `0252270ca3889ef1eef502f469f337be3bc8fcf5` 的既有验证：

| 检查 | 结果与限度 |
|---|---|
| 完整 M5 | exit 0，`M5 FINAL ACCEPTANCE: PASS`；182 Rust targets、1298 passed / 0 failed / 16 默认 ignored；Python 95 项，1 skipped；fmt/check/strict Clippy/合规与独立发布回执核对通过。 |
| 永久宿主门 | exit 0，384/384；明确不安装服务，IP01–IP24 NOT_RUN。 |
| closure A / D RuntimeOnly | exit 0，分别 47/47、71/71；D 不含真实 C24/Final。 |
| 开发候选 | release `forme-windows-x86_64-20261106-0252270`；Rust verifier 已独立验证，release digest `sha256:f3969e84542b6c121c00d472b3b9b66f0040d363c37a2d1e243799e7415b67af`。未部署，`release_ready=false`。 |

以上是本次终态/摘要修正之前的源码快照，不自动证明修正后的版本。候选安装器 SHA-256 为 `2148fec19490151068ad509ceef9f5ffdbc851d8a27e8bfe4c43074ec11f44dd`；只有本地开发签名，不是正式签名发布。

## 2. 实际备份与恢复候选矩阵

原生安装仍为 `installation:0d87010c913c2299385d98cbf952696c078834adaa52147126e42a626be222a1`、active release `forme-windows-x86_64-20261106-d22b6a3`，Host 使用首层管道诊断修复 image。沿正式 Web 认证/CSRF/owner 密码重认证及原数据 API：

1. 一次创建 `native-peer-query-backup-20261106`，Confirmed，223 events；在线完整性、事件连续性和 replay 验证通过。
2. 一次准备恢复到新对象 `native-peer-query-restored-20261106`。替换 plan digest 收到 409，查询原命令无 receipt，未接纳。
3. 原 exact plan Apply 一次，Confirmed；恢复候选的实际验证通过，execution fence 为 `sha256:528dac6e066eb82ba2655f6e26a235b22951f216cab2adf28d18d97638f3d4f0`。
4. 同 command 重放返回完全相同 receipt；换新 command 再 Apply 同 plan 被 409 拒绝，查询没有新 receipt。
5. active release、active/ack revision 1 均保持，pending_restart=false；没有切换 active brain。

这是已安装旧版的真实数据路径证据，不是修正版安装、完整 CommitRestore 或完整 IP15 通过。

## 3. 真实停止与实现缺陷

正式原操作 `maintenance:offline-backup-stop-20261106` 只提交一次，返回 product revision 35、RecoveryRequired、pending Checking，failure ref 为 `sha256:9e55715363436dedd25298adc8d238a860400a073ae39bcea771f442d7539bd6`。该摘要精确对应既有 `native service did not settle` 错误；请求后先观察到 Brain Stop Pending，随后独立 SCM 为 Stopped/PID 0。Host/Web PID 64472/16780 保持；未重发 Stop、未另起 Start、未改写 journal。

旧适配器在一次 OS Start/Stop 已获接受后，第一次轮询如仍为旧 Running/Stopped，会立即错误退出，而非等异步 handler 发布 Starting/Stopping。修正将观察独立为私有模块：仅在原 10 秒截止内允许初始旧状态，接受匹配方向的 pending，只有目标终态才返回 receipt；pending 后回退、反方向状态、Unknown/Failed/NotInstalled、查询失败或期限耗尽均失败，绝不再发控制命令。原失败没有逐次 poll 轨迹，不能仅凭同一错误摘要断言当时恰好是哪一次观察触发；定向测试复现并约束了确定存在的旧状态竞态。长于原观察预算的排空仍可能要求恢复，不凭本修正宣称所有慢停服已闭合。

原维护未决事务继续占用 journal 全局互斥；当前没有通用的 StopCore 恢复裁决接口。虽然旧 status 的 allowed_operations 仍列出启动命令，实际新命令准入会拒绝，不能把列表当作恢复已获准或直接伪造 Committed。此次不扩展恢复协议；后续安装验收沿用 owner 明确授权的测试实例重置，旧失败随测试实例销毁而处置，不冒充恢复成功。

## 4. 离线摘要缺陷与真实拒绝检查

Brain 实际停止时，`/api/v1/product/data/verify` 已返回 verified=true、223 events；在线数据入口明确 503，`../forbidden` 源被 400 拒绝，Host/Web/Brain 状态与 PID 均保持。离线验证没有启动 Brain、调用模型或切换数据。

但在线 `DataBackupView.digest` 为真实数据库摘要 `sha256:01540c057ff02750da2c06ce6b48e4401e94cf07e17c00e6d6dd9a58d34dfc72`，离线却返回 manifest 摘要 `sha256:c742b9ba0747f7ad14380873c6aa8fa96d0b1489be6cbaec934f4c05a82bb264`。因此首份联合比较报告正确保留 stopped，没有将两者强行当作一致。

修正离线响应沿用原 manifest 已验证的 `database_digest`，与在线入口及 snapshot 原始字节保持同一含义；manifest 完整性检查没有删除。现有真实加密备份回归另断言响应摘要等于独立读取的 snapshot 字节 SHA-256，继续核对原脑/备份不改写、零模型调用及敏感路径不泄露。

## 5. 当前修正验证与后续窗口

已通过 5 项服务观察测试（含 3 项新增正反场景）和 3 项真实加密离线备份组件测试。新增场景纳入永久宿主命名门；规模棘轮通过，未刷新 baseline。完整源码质量门已结束，exit 0：182 Rust targets、1301 passed / 0 failed / 16 默认 ignored，strict Clippy、前端 115/115、bundle/签名验证、NTFS 访问描述符、只读防火墙策略及 Inno 成功/失败退出夹具均通过，完整合规 PASS。这些是源码与组件证据，不证明安装版已经运行修正。

修正后的永久宿主门也已结束，exit 0、387/387 命名场景 PASS，含全部 3 项新增服务观察反例；该门仍明确 IP01–IP24 NOT_RUN。整轮构建空间报告 PASS，无超预算工作树，没有清理缓存或刷新质量 baseline。

本机窗口已在 19:54 实际执行，但在安装器准备阶段停止，结果见 §7。原定顺序为：核对当前 installation、四服务绑定、两个固定根无链接、源码和安装器摘要后，仅清除已授权 ForMe 测试实例；安装修正版；使用既有本地测试凭据经正式认领/配置自动启动主脑；实际验证拒绝/批准/重放、文件读写删，再验证正式 Stop→离线备份核验→Start。只要一个动作结果不明就保留原号并停止，不自动重复。仓库、外部项目和原验收工件不在重置范围。

为减少零散管理员启动，同一固定窗口还准备独立的正常签名升级场景：从同一干净源码和已观察构建程序生成另一个明确带 `-ip14` 的候选，沿用既有本地开发签名，不声称它有新的业务功能。新安装仍未认领且 Brain 停止时，仅把公开发布信任绑定到该 installation，准备 Host 私有 inbox，限定停启 Host/Web；不修改系统证书信任或防火墙。前述文件和停启矩阵全部结束后，才通过正式 owner 重认证提交 StageSignedRelease 和 Upgrade 各一次，断连只查询原操作。最终另核对 SCM 的实际新 image、原 owner/store/workspace、原配置 ack 和已完成文件 run 的事件保持。它不为旧 StopCore 解锁、不迁移旧失败 journal，也不替代回滚/CommitRestore、OS 冷启动或其他 IP。

正常完整签名升级、真实 peer/文件隔离和其余 IP 仍未完成；MCP、plugins、packages、channels 的受管配置消费者及 notification/其他 backend/helper 接线也仍开放。OS 无登录冷启动/注销、LAN 客户端信任及其他平台继续独立取证，不用源码测试替代。整体目标仍未完成。

## 6. 本地工件

均位于本工作树 `target/native-legacy-review-20261106/`；只保留脱敏结果，不包含口令、cookie、proof 或 provider key。

| 工件 | SHA-256 或状态 |
|---|---|
| `peer-query-m5-gate.log` | `5d034a4bbba5c58e1e793c6f175fea1862300cde2683b597088954bbfef4c053` |
| `peer-query-installed-host-gate.log` | `72be8eb7336bd8ce8ea0f2dd5baf4d1f1cb0f79093d9a599259b5dc7f28aec85` |
| `peer-query-closure-a.log` | `248b6e858b07d18be1ba42e2b65729384fa15fb52e1d6f820a81dbaa13885cd7` |
| `peer-query-closure-d-runtime.log` | `1dc8aff7a542b3491bff63c49d037d9b51b6780542ae1ca5128cfcc827ab3c0c` |
| `installed-data-candidate-matrix.json` | 已结束，真实备份/候选/一次应用与重放拒绝。 |
| `installed-offline-backup-window.json` | 保留原 StopCore RecoveryRequired；未继续 Start。 |
| `offline-backup-after-stop.json` | 保留摘要比较失败，同时保存实际离线 verified 结果。 |
| `offline-backup-rejection-checks.json` | 已结束，在线 503、错误源 400、服务状态不变。 |
| `service-settle-source-gate.log` | exit 0，`8f5a89e44c8d445970675dc99ac1174e2464139f19daa2ac6a2cf96e99a605ce`。 |
| `service-settle-installed-host-gate.log` | exit 0，387/387，`5213c26f0a5d14594a544d9f707a55bf0beffef65deccec3b739842f37b7cd8d`。 |
| `service-settle-build-resources.json` | PASS，`9e4915840a9ac81f2aeec16efe8c639f2119679eff8f67efb093958baf2ed07e`。 |

此前一次合并升级窗口的 UAC 被 Windows 取消，没有启动管理员脚本，没有 Stage/Upgrade；`peer-query-native-window-launch-1.md` 保留该结果。现在没有待确认的旧 UAC，不重复启动那个已过时的升级窗口。

## 7. 实际窗口：安装准备失败与当前现场

代码提交分别为 `4c7062f`（服务观察）和 `46c0168`（离线摘要），验收文档提交后以干净 `fac72eb5265b12f4b6f472dc0ab909f767ad501e` 构建。候选安装器 SHA-256 为 `f212370339bb431f4bb98e443ca60ffa68dbdda17e1ec0b24b92d928466aa34d`；新 `-ip14` 候选复用同一批已观察编译程序，独立签名/验证 digest 为 `sha256:cdc780d80548d881b7a5490fa8574652342b982f9ce26a7105eae8d26066b078`。两个候选均只使用本地开发签名，不构成正式发布。

本次 UAC 实际放行，管理员窗口启动。19:54（Asia/Shanghai），核对原 installation、四服务 image/账户、两个固定根及子项无 reparse 后，已按 owner 授权禁用并停止四服务，删除四服务、唯一 Inno 卸载登记、原 ProgramRoot（18 文件 / 440,312,058 bytes）及 DataRoot（730 文件 / 51,632,480 bytes）。旧测试数据没有备份，不提供恢复副本；没有保留或迁移旧账号、key、脑、配置、workspace、备份与未决 Stop journal。仓库、外部项目、本地测试配置、开发签名材料和验收工件不在删除范围。

唯一固定新安装器于 19:54:05–19:54:09 执行，exit 7。Inno 日志记录 `protect empty installation roots` 后 `PrepareToInstall failed`，发生于 payload 复制、hostctl prepare、服务登记与模型/脑创建之前。父窗口按约定停止；没有执行发布信任准备、认领/配置、文件动作、正式 Stop/Start 或 Stage/Upgrade，没有重跑原号。该结果不计 IP01 或 IP24 PASS。

随后独立 SCM 查询没有 `forme-*` 服务，Product URL 不可用；新 ProgramRoot 存在，普通 token 无法枚举其受保护内容，DataRoot 未创建。当前还不能从通用错误推断是 owner、DACL、目录创建还是另一项检查失败，不能直接放宽权限或把安装误判为成功。

`inspect-service-settle-root-preparation.ps1` 只读检查当前根权限/服务，并在工作树隔离目录比较 PowerShell 5.1/7 的原生创建行为，不安装、启停服务或重派任务。19:57 首次诊断 UAC 被 Windows 返回“操作已被用户取消”；未取得有效管理员进程 ID，脚本未启动。外层启动命令在 Start-Process 非终止错误后继续打印了 `Started=true, ProcessId=null`，该包装输出不是启动证据。后续 launcher 已改为终止错误即停。

owner 回复重新唤起后，20:05 独立诊断实际完成：SCM 无 ForMe 服务；ProgramRoot 无子项、无 reparse，owner 为 Administrators，protected DACL 仅 SYSTEM/Administrators FullControl 且逐条匹配原策略；DataRoot 不存在。PowerShell 5.1 和 7 分别在新的隔离目录运行原 `New-ForMeProtectedDirectory`，均取得 verified 回执。首次安装失败没有复现，尚不能认定根因或声称修正了 ACL；两个空 probe 目录保留。

已准备独立的 `complete-service-settle-recovery-window.ps1`，入口 SHA-256 为 `01a9a1ef0ca5cbdf4310529965b798f683313f5a02f1147424ddd70908dec9dd`。它重查原 exit 7/源码/包/诊断、零登记及根为空的前提，只用原 ACL 模块创建缺少的空根后明确恢复安装；使用独立回执保留原失败，并设置 `clean_install_acceptance=false`。后续正式信任/认领/模型、文件、停启备份及升级沿既定顺序，任何失败或 Unknown 即停止。该入口前两次管理员启动均由 Windows 返回“操作已被用户取消”（launcher sessions 84405、3673），均未取得有效管理员进程 ID；该时点尚无恢复回执。

20:24 第三次启动实际取得管理员进程 PID 30788。原生安装器于 20:24:44–20:24:48 再次在 `PrepareToInstall / protect empty installation roots` exit 7。窗口只按原 ACL 策略创建了缺少的空 DataRoot；没有复制 payload、创建 installation identity 或登记服务，所有信任/认领/配置/文件/生命周期/升级步骤均未执行，原 reset 没有重跑。两个固定根现均为空且 protected，SCM 零 ForMe 服务，Product URL 不可用。该已结束窗口不能再次执行或覆盖其回执。

| 本地工件（同 §6 目录） | SHA-256 / 结果 |
|---|---|
| `service-settle-reset-result.json` | `7b24fefaf3c3430cac52140c04ceaf83c344c281e8adcef4b740bb658f99ccc8`；停止于新安装，旧测试实例已销毁。 |
| `service-settle-fresh-install.log` | `5e98a309b63084ebf55da535d877d4fb3b0e8d91ed3c6992df7caeedd00abe8c`；Inno exit 7、无 OS reboot。 |
| `service-settle-native-window.json` | `8387ce81f6b00830c0f10955715a2561736c2a188fda86be613fd1f386d7c62d`；仅 reset 步骤执行并失败，后续步骤未激活。 |
| `service-settle-root-preparation-inspection.json` | 20:05 实际完成，`d75f0d8a3dc6e587eb2548a4dfa24fcdbad43933337387e04cac5066e9a3f9af`。 |
| `service-settle-root-probe-desktop.json` / `service-settle-root-probe-core.json` | 原生创建均 verified；仅是隔离目录检查。 |
| `service-settle-recovery-window.json` | 实际执行、停止于 installation；`c110f57d90ad0c0b7d19461408284bc348fc5d63b573326e160f3b4fbe8b741a`。 |
| `service-settle-recovered-installation.json` | 安装器 exit 7、零服务；`506edcece887f77bcebe84db4a0334f0d0d149761b69abc1f331967079555e54`。 |
| `service-settle-recovery-install.log` | `c85195815b57ccdcc5cdfb8ec560ccc76aaf2ed56a3d7e848277f57e9cf2a8ae`。 |

## 8. 安装器比较诊断与错误定位补强

直接检查发现 32-bit PowerShell 的默认 ProgramFiles 与正式 x64 固定根不同，但这只是待验假设。一次独立管理员诊断 wrapper 因自身的子进程退出/断点处理错误停止；其 x64 子脚本完成并不等于 wrapper 整体通过，x86 比较也没有形成有效回执。该失败保留，不能作为产品根因或权限修复证据。

20:35 的独立原生 Inno Prepare-only 夹具保留正式 app-directory 语义，调用包中原权限脚本和模块：实际 child 为 64-bit、两个固定根参数匹配、child exit 0，根字节/ACL 与服务清单保持。它在任何 payload 安装前故意终止，既不是完整安装，也没有复现真实候选的失败。因此不能认定位数解释了原失败，亦未放宽权限。回执 `target/installer-entry-probe/native-entry-observed.json` 的 SHA-256 为 `7eb61fed2882f216e7863e091823b3ba522413be9bb45519e8dfda2f5a799970`。

在取得更多真实证据前，仅补齐私有可观测性：Inno 区分 launch failure 与 child exit；权限 helper 只在固定解包 sibling 新建数字阶段/来源/行号/HRESULT/分类诊断，原始错误、路径、参数和环境不出日志，已有诊断不可覆盖。成功条件、ACL、服务操作和失败即停边界不变，不新增冻结 DTO/trait/脑事件。

实现提交 `de28e02`。定向检查已通过：NTFS/权限 9/9、原生 Inno 成功/失败退出及诊断脱敏 2/2、bundle/native-package 19/19、规模棘轮；未刷新 baseline。完整源码门 exit 0：182 Rust targets、1301 passed / 0 failed / 16 默认 ignored，strict Clippy、前端 115/115、签名验证、原生夹具与完整合规均通过。`installer-diagnostic-source-gate.log` 的 SHA-256 为 `c0536d58b067afcf0112d851f58ceafa3b4b7c25ddc8d9a32eb60e728669f941`。新候选必须从干净提交生成，后续以独立文件记录安装结果；不得覆盖前述两次安装失败。

下一步以包含诊断的新候选确定实际失败阶段，再推进安装、文件、停启备份与升级。整体目标未完成，源码测试通过不证明目前本机有可用 ForMe 服务。

## 9. 新诊断候选的实际失败与比较边界

干净源码 `07d0bdc5cbac64286dbe6c2ec5d2b9f480a43cf9` 已构建为 `forme-windows-x86_64-20261106-07d0bdc`，安装器 SHA-256 为 `548280d49f70c3116c9522fd913a8a7679e128fe84d1f399ef1145ec61153707`，构建观察 SHA-256 为 `da264bb93a421c342b9ef4fdcba20cad162a494557a4b5a90703b560de59e04d`。同批程序另有独立验证的 `-ip14` 签名候选，digest 为 `sha256:b87b05f61a245565e9179895fec8d311f7920bb1f8cf9185983764fd718f0240`；尚未 Stage/Upgrade。开发 Authenticode 链仍不被系统信任，`release_ready=false`，没有安装系统信任或改变防火墙。

21:02 固定管理员窗口实际启动（PID 46464），不需要逐子步骤再次提权。新安装器只调用一次；helper child exit 1，诊断为 `stage=5 source=2 line=101 hresult=-2146233087 category=13`，随后 Inno exit 7。该位置为原 ProgramRoot 的 `Get-Acl -LiteralPath`，不是第 106 行的 DACL/owner 不匹配断言。分类 13 为 ObjectNotFound；现有有界诊断尚不能区分 cmdlet 查找、provider/path 或其他具体来源，不能直接认定权限错误、位数错误或根因已修复。

没有复制 payload、创建 installation identity 或登记服务；所有下游步骤均未执行，也没有再次重置两个根。最新失败回执优先，不能因其失败退回旧成功回执。历史原件与本次独立输出都保留：

| 工件（`target/native-legacy-review-20261106/`） | SHA-256 |
|---|---|
| `installer-diagnostic-installation.json` | `054b7c638bfd170c94d234be8373f2ac7f7042c0e738302fcfffcf84fb723c3f` |
| `installer-diagnostic-install.log` | `c780f28ca3f9b7d500d6039d31febca724cca929ef2e82360cab9663e0ba61c3` |
| `installer-diagnostic-window.json` | `7392aa027eeee67bfb872fc58745d21ce0330620c41beda8e672826afb3ddc28` |

21:04 的原生 cold ACL 只读比较和 21:06 未预载 `Get-Acl` 的原 helper 比较均以 64-bit、固定参数完成，child exit 0，零根/服务写入；这些比较没有复现真实包失败。21:10 的完整源模板 Prepare-only 夹具也得到 helper child exit 0，但该夹具未压缩/未签名，且强制在任何 payload 前终止，所以仍不能证明真实签名安装成功或确定环境根因。首版生成器因 JavaScript replacement string 的特殊替换序列生成无效 PowerShell，未取得有效诊断；其工件保留，V2 改用 replacement callback 并通过 PowerShell 5.1/7 解析后才执行，不以首版结果充数。

21:11 发起的独立父进程环境比较只允许复现权限预检和 Prepare-only 夹具，不含实际安装、服务变更或 ACL 修复；launcher session 45067 随后由 Windows 返回“操作已被用户取消”，没有有效管理员 PID，也无 `nested-result.json`。包装命令仍打印了 `process_id=null`，不是启动证据；不得把本次取消视为比较已执行或继续叠加提权请求。最近一次原生观察中两个根为空且受保护，SCM 零 ForMe 服务，Product URL 不可用，IP01/IP24 未通过。

构建空间报告已保存为 `installer-diagnostic-build-resources.json`：统计完成、无扫描错误；当前工作树缓存约 42.12 GiB logical / 39.30 GiB allocated，低于 64 GiB 约定。另一个 `C:\code\forme` 工作树仍超预算；本轮未清理任何缓存、安装工件或验收证据。后续统一从管理员维护进程执行已获授权的固定批次；Codex 完全访问不授予当前 Windows 进程管理员令牌。
