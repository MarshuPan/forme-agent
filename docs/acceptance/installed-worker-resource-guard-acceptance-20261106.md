# worker 恢复窗口的实际资源守卫（2026-11-06）

本批承接 PRD24 §4.1.4、architecture/10 §7.1.4 和[已确认资源守卫协议](../architecture/proposals/installed-worker-native-quiescence.md)，契约先以 `0ba3f3e` 提交，实现为 `e649b84`。owner 的“继续下一步”同时授权独立固定 Host 路径恢复；该原机动作的[实际回执](installed-original-host-path-restore-20261106-result.md)不替代本报告的源码验证。

## 1. 问题与实现

原恢复入口仅持 Host operations 互斥并检查原 worker journal，即使日志终态有效，也没有要求隔离器观察并保持实际进程登记与 sandbox lease 的静止。新 `WorkerIsolation::with_quiescent_resources` 缺省返回 unavailable、零 callback，不以空日志取得观察能力。

正式路径顺序为 operations → adapter children 排他 → 原 sandbox lease writer（若有）→ 原 worker journal 有界原件检查 → 原恢复 callback。实际资源锁保持到 callback 返回，callback 错误原样返回；不主动 probe、终止或清理 job，不追加新事实或更改原 worker 摘要域。callback 不得重入 worker/lease 控制方法。

- AppContainer 要求 registry 为空、没有 recovery-required，并在原 lease writer 中有界重放原件，核对 schema、序号、hash/fold 与内存水位，全部 lease 都已有完整 cleanup receipt。
- Process、Windows Job、Linux systemd 与 macOS launchd 在各自实际 children mutex 内检查空 registry，并持锁回调。当前 registry 静止不证明未知历史进程不存在，也不提高 POSIX 隔离或发布状态。
- lease 原件检查保留同一文件句柄、OS 锁、字节和位置。损坏、合法截断、额外有效事实、同水位异链、IO/位置恢复失败使 writer 保持 poisoned；有效但未 cleanup 的 lease 只拒绝当前恢复，不 poison 正常 cleanup。
- startup lease gate 复用同一原件检查。缺观察能力的测试 adapter 必须显式补齐受控观察；正常恢复/历史审查正向夹具改用无 launch profile 的真实 Process adapter 空 registry，不改生产默认拒绝。

本批没有新增 crate、依赖边、EventKind、unsafe 边界或产品 DTO。Linux/macOS 原有相同的网络判断合并为 POSIX 私有 helper，行为保持。

## 2. 定向与命名验证

| 本批执行 | 实际结果与范围 |
|---|---|
| lease 守卫定向 | 6/6：原件字节/位置/水位/OS 锁保持，未 cleanup 不 poison，poisoned 与 callback 错误保持，损坏/无换行/未知 schema，合法截断/异链，额外事实/超长原件。 |
| execution 单元目标 | 267 passed / 0 failed / 1 默认 ignored。含两个实际 registry 排他/poison 场景及一个受控 AppContainer recovery-required/锁持有场景。 |
| `installed_worker_contract` | 24/24。三个新增场景验证缺观察能力即使空日志仍拒绝、终态日志不能掩盖实际登记资源，以及 callback 期间并发准入等待且返回原错误。 |
| `installed_release_staging` | 36/36。正常/历史恢复继续要求实际资源守卫，不借默认 unavailable 获得成功。 |
| `installed_windows_job_worker_contract` | 3/3，真实临时 Windows Job 进程。既有活进程自报反例仍要求恢复零 callback，真实取消后才可取得静止水位；另有实际完成/失败及后代取消。 |

本批十二个新增命名场景已经加入 `tools/verify-installed-host.ps1`。lease 原件损坏通过测试自己的原持锁句柄注入，未绕过 Windows 文件锁。所有进程和数据夹具均使用独立临时目录，未接管真实安装或原脑。

## 3. 完整检查与本地证据

完整源码质量门 `tools/verify-code-quality.ps1` 已结束、exit 0：182 个 Rust targets、**1281 passed / 0 failed / 15 默认 ignored**；fmt、locked workspace check/build/test、strict Clippy、结构门与完整合规通过。前端 **115/115**、依赖审计零漏洞、嵌入资源、打包/签名边界、临时 NTFS **8/8** 和只读防火墙夹具 **25/25** 通过。规模门为 867 个源文件、63 处既有规模债务，未刷新基线。未提供 Inno toolchain 的安装器 exit fixture 为 NOT_RUN。

第一次完整门在规模检查处因 `verify-installed-host.ps1` 604 行停止，尚未开始整仓 Rust 回归；合并同类完整路径测试清单后为 598 行，所有原命名场景保留，修正后的完整门通过。失败日志保留，不将第一次计为通过，也未无理由重跑第二轮完整 Rust 门。

永久宿主门 `tools/verify-installed-host.ps1` 已结束、exit 0：**379/379** 逐名检查通过，包含本批十二个新增场景和既有三个真实临时 Windows Job 场景。最终输出 `installed-host-foundation and core-shutdown-components: PASS`，同时明确 `IP01-IP24: NOT_RUN`；该门没有安装服务或认证原生安装包。

本批证据固定保存于本工作树 `target/worker-resource-guard-20261106/`；定向结果不与全量或永久宿主门重复相加。日志不含真实登录凭据、模型密钥或原脑正文。

| 固定本地证据 | SHA-256 |
|---|---|
| `full-quality-fixed.log` | `1077fbeab6bc540cd01cf9322d77f1a0a60f9b0342228273e390b754ed40dc23` |
| `installed-host-gate.log` | `14e0f2fa0c48889c7fa8be68a7dfd31900a01a71971f63818615d40be5fc739f` |
| `lease-guard-tests.log` | `2d01c7b463998f34e212367bfec406827cb1a46fef5e2c489e8ca7f312537caa` |
| `execution-unit-tests.log` | `0caafce8246c797bfd46b368a029c1009ae5007993c2b65771ab17b6985ab72e` |
| `execution-contract-tests.log` | `404a14655f42478e5af8bace00730adb01ae5c619be6511cf3a45e52d5d5bbf6` |
| `full-quality.log`（首次规模门失败） | `6c35ee0c3e2a5163dfcfe14cd6062d62828353a41af89fc1ffaae96a9b5d91dd` |
| `linux-cross-check.log`（缺交叉 C 编译器） | `944e193fed097b0789554c4fd0cbd6014e1c7883737e7e40f7866f7a739712ad` |
| `native-final-readonly.json` | `b26173114b59b0ba96c0d9ea52394c35989b38377d45e3f411ca57bc42725bd5` |
| `build-resource-report.json` | `89f6768116e505cee9e43ed162b0b278ade1a44320c97bde63c90f375b229a0c` |

任务收尾已使用固定 target 运行 `py -3 tools/build_resources.py report --json`，结果 PASS、无超预算项，原报告如上表。本轮没有清理缓存。完整源码门和永久宿主门均已结束，后续文档收尾只执行合规与差异检查，不重复整仓 Rust 回归。

## 4. 原机与平台边界

原 Host 路径恢复于 11:38 成功；本批 11:59（Asia/Shanghai）独立只读观察仍为 Host Running/PID 75976、Web Running/PID 32464、Brain/updater Stopped/PID 0。18765 listener 对应 Web，GET `/` 返回 200、HTML 595 字节并包含产品入口标识。原机仍运行旧程序，本资源守卫尚未部署；页面入口恢复不是登录、BrainReady、真实模型或 IP 通过。

Linux 交叉 `cargo check --locked -p forme-execution --target x86_64-unknown-linux-gnu` exit 101，C 依赖编译缺少 `x86_64-linux-gnu-gcc`。该项未通过，不将已安装 Rust target 等同于完整工具链；本轮未再重复同一失败。macOS 没有本轮编译或原生证据，POSIX 继续 `release_ready=false`。受控 AppContainer 单元场景也不能替代真实原生隔离 job 的安装联合验收。

## 5. 仍未闭合的整体义务

完整生产 `LegacyReviewCollector` 仍未挂载，历史来源记录/公钥与档案 pin、原 updater 的可信源码/构建绑定、worker/lease 历史连续性、精确签名管理 reader 修复与原机迁移，以及原 Upgrade 的显式裁决均按[八项材料状态](installed-legacy-recovery-evidence-status-20261106.md)继续保留。不能给现存无来源档案补签或倒填时间使其 Verified，也不能用本批当前资源检查制造历史连续性。

原 Host 已重新持有活动 journal 锁，不从外部绕锁采样、不重跑恢复 Apply、不重派原 Upgrade。真实 Brain/model/worker/helper、后续升级恢复与 IP01–IP24 仍须分别取证；本批不刷新 M5 或发布回执，不合入 main 或推送，整体目标未完成。
