# 产品化实施中的可维护性检查

## 1. 范围

本记录对应 owner 要求的工程规范与大模块维护，不代表安装器、五面 Web 或全部历史技术债已经完成。规范入口为 [code-quality.md](../engineering/code-quality.md)，产品范围仍按 prd/24–26。

2026-10-26 `ece53be` 将 `AgentHarness`、`GatewayControl`、`Observability` 的实现移入 `crates/harness/src/gateway_control.rs`，根级新增公开控制 trait 契约测试。该提交只移动实现并保留原 API/行为；没有新增 crate、依赖边或 EventKind。`cargo test --workspace --all-targets`、fmt、workspace check、strict Clippy、结构门及其单测、代码质量门和 compliance doctor 均通过；结构门扫描 719 个文件，63 处既有规模债务未增长。首次 `verify-m5.ps1` 已完成 S1–S99、C1–C24、Rust/Python 与合规检查，所有代码门通过；但仓库 S99 回执仍绑定前一源码，故该次终线比较失败，不记作 M5 FINAL PASS。后续回执刷新与 clean-tree 最终状态见 [M5 验收记录 §10](m5-acceptance-report.md#10-产品化分支的永久回归重验)。本次不改变安装、worker 或 IP01–IP24 验收状态。

| 提交 | 结果 |
|---|---|
| `3236e50` | 固定 Rust API/格式、错误、并发与恢复、Web/宿主、测试/审查规范，说明规模阈值是仓库维护约定。 |
| `b64fc04`、`b3b779f` | 自动检查冻结依赖方向、crate unsafe 禁令和文件规模；CI 运行同一检查器，标准 tests.rs 使用测试文件阈值。 |
| `b373aec` | 按控制域与 HTTP 职责拆 Gateway，保留 public API、事件、测试名字与生产装配。 |
| `57e34b8` | M4/M5 审计定位到拆分后的正式测试/实现；新增旧文件诱饵反向测试，不能用残留同名文字代替新文件内容。 |
| `26ce4b4` | `forme-hostd.rs` 从 783 行缩到 40 行；配置/凭据、HTTP、进程装配、SCM 信号分别为 217/294/188/70 行。移除该文件已偿还的规模豁免。 |

拆分没有新增 crate/依赖边、脑 EventKind、公开库 API、第二 supervisor 或另一套运行时。两域 listener、环境配置、请求格式、已有服务名与恢复语义保持不变。

## 2. 验证

最终 Rust 源码 `26ce4b4` 已执行 fmt、workspace check、完整 workspace tests、strict Clippy、合规和 `git diff --check`；结果 688 passed / 0 failed / 5 默认 ignored。宿主专项 60 个命名场景执行通过；含真实子进程/loopback、分域拒绝、并发幂等、取消、原 job Unknown 与受控停服，不是原生安装证据。

Python 工具全量运行 72 项，其中 1 项按平台跳过，其余通过。结构门扫描 341 个源文件；剩余 **65 处历史规模债务**，未扩大基线。M5 永久长链与当前回执结果在 [M5 报告 §10](m5-acceptance-report.md#10-产品化分支的永久回归重验) 单独记录。

## 3. 后续维护约束

- 新生产文件原则 ≤600 行，测试 ≤1200 行。历史超限文件按审查基线不增长；缩至正常范围后移除旧豁免，不保留可回涨的额度。
- 后续优先处理 harness 的运行协调/production 和 store 的事务领域；每批先保持 API、事件顺序、事务与故障恢复，再做独立提交和正反验证。
- 历史债务是公开库存，不是已完成的可维护性认证。不得机械切片、用 include 拼接、增加转发空壳或扩大基线来让数字变好；最新库存见下节。
- Web 新功能按页面/client/session/reducer、服务端认证/配置/bridge 分责，不继续堆入现有大文件。工具门不能替代行为审查、原生权限和产品 IP 证据。

## 4. 受管服务与工作台增量（2026-10-21）

本批拆出 harness ingress、production builder、capability 文档解析和 models 文件凭据 resolver；`production_config.rs` 从历史 841 行降至 493 行，解析子模块 356 行，并移除已偿还的规模豁免。harness lib 约 7390 行、store sqlite 约 8975 行，仍按历史棘轮管理，不宣称全仓大模块债务已清。

结构门当前检查 414 个源文件，剩余 **63 处历史规模债务**，没有扩大基线。`harness/src/brain_orchestration.rs` 已完成首批职责拆分：节点构造、输出归并、签名转换和独立验证移到 `brain_orchestration/support.rs`；公开 API、事件顺序、事务边界和恢复语义保持不变，C16 编排全套 23 项回归通过，并移除该文件的旧规模豁免。统一源码门新增 TypeScript、前端行为、可重复资源和 bundle 保留/完整性验证；GitHub CI 使用同一脚本。组件与最终源码结果详见 [受管服务与工作台组件记录](installed-product-managed-workbench.md)。本节数值更新不覆盖上节 `26ce4b4` 的历史测试回执。

## 5. 模型配置增量（2026-10-22）

`01b77f9` 将配置事务按状态协调、journal、文件发布、模型校验和测试分责；前端将草稿转换与受控启停从设置页拆出。配置入口 389 行，journal/materialize/storage 分别为 304/348/103 行，启停组件 73 行；没有使用 include 拼接、扩大旧基线或新增内部依赖边。

最终结构门扫描 433 个源文件，仍为 **63 处历史规模债务**；新增文件均符合 600/1200 行约定。全量 Rust 749 passed / 0 failed / 5 默认 ignored，strict Clippy、14 项前端测试、7 项 bundle 回归与完整合规门通过。已认证浏览器检查了本次设置交互，其他页面与原生平台证据不由它替代。详细范围和可重复命令见 [配置闭合验收](installed-product-configuration-acceptance.md)。

## 6. 安装权限分区增量（2026-10-22）

`29984a4` 将旧布局验证从 prepare 入口独立为一个小模块，Windows 安装入口、ACL/路径/布局规则和测试分别落文件；不在 host/lib 中新增通用 shell 或另一套策略 owner。三个源文件和测试都低于新增文件阈值。结构检查新增 `packaging/`、`.psm1` 与 `.iss`，不再让安装权限代码游离于规模检查之外；对应检查器反向测试已同步。

最终 438 个源文件、**63 处历史规模债务**，没有刷新基线。统一门将原生 NTFS descriptor/继承/原子替换/拒绝及真实 hostctl 布局联合测试纳入 Windows 源码回归，原生安装/真实服务 token 与产品 IP 验收仍独立。Rust 752 passed / 0 failed / 5 默认 ignored、前端 14/14、bundle 9/9 和完整合规通过；详细执行与中间传输失败说明见 [权限分区验收](installed-product-service-access-acceptance.md)。

## 7. Harness 控制与装配拆分、Windows checkout（2026-10-24）

`3dc30bc` 集成维护 worktree 的 `b74f9c4`，两者 Git tree 均为 `966f45fa508b2c4b34bdb2e5f8e5ce3b676fe299`。Harness 构造/装配、owner 控制与审批只读投影、项目 AppAPI、模型/上下文准备分别移至 253/204/131/263 行的职责模块；原 `lib.rs` 从 6977 行降至 **6159 行**。未改公开 API、事件、执行顺序、事务或恢复行为。新增正式 ProductionRuntime 审批边界测试，证明反复读取/跨 session/重启不调用模型、不写事件、不生成工具副作用。

维护 worktree 完整 Rust 回归为 **159 suites、853 passed / 0 failed / 5 默认 ignored**，fmt/check/strict Clippy 通过。原统一门随后因 Windows autocrlf 改写生成 CSS 而失败；补验中发现未固定 LF 的 HTML/public 输入也会改变 bundle。`63ecc43` 固定输入 LF 与生成资源 exact bytes，并用真实临时 Git 仓库的 add/commit/restore 验证正向保护和未保护反例。Windows PowerShell 在长路径内折行拒绝提示造成的测试误判也已修正，仍检查非零退出、明确拒绝和零写入，不放宽安装边界。

`63ecc43` 的完整统一源码门通过（158 suites、852 Rust tests、49 前端 tests、打包/签名/NTFS/完整合规）；维护分支补验的前端可重复 build、8 项 Python 结构/checkout tests、7 项真实 NTFS/hostctl 场景、打包/签名、最终 check/结构/合规/diff 门通过。原失败日志保留，不把拆分前的源码门冒充拆分后的单次总门。当前结构门 **578 文件、63 处历史规模债务**；没有刷新基线，也不声称大文件债务清零。

日志分别为主工作区 `target/installed-product-quality-20261024-checkout.log`，维护 worktree 的 `target/harness-refactor-quality-final-20261024.log`、`harness-refactor-frontend-lf-20261024.log`、`harness-refactor-*-final-20261024.log`。完整 M5 已走到 S99：当前临时回执生成/验证通过，但仓库旧回执 compare 失败，**当前完整 M5 尚不记 PASS**，须在最终干净源码上更新回执并重验。原生安装、worker 隔离、跨平台和 IP01–IP24 仍按产品记录开放。

## 8. Production composition 职责拆分（2026-10-24）

`727e473` 将正式装配、服务状态、能力摘要和启动 doctor 事件四个完整函数移入 `production/composition.rs`（444 行），`production.rs` 从 3365 行降至 2926 行。只改变模块路径和私有可见性；将这些差异规范化后，四个函数与拆分前原文完全一致。公开 API、装配顺序、事务、事件和恢复逻辑保持。未刷新规模基线，63 处历史债务仍须后续处理。

受管配置边界测试现为 2/2：除原有实际 loopback provider/后台认知/重启场景，新增 build_with_store 与普通 build 的相同装配证明；重复 writer 与外来 store 被拒，拒绝不生成模型调用或脑事件，外来 store 没有 authority。释放后重启保留原 store、aggregate 与 manifest。

最终统一门 `pwsh -NoProfile -File tools/verify-code-quality.ps1` PASS：161 suites、868 Rust tests passed / 0 failed / 5 默认 ignored；strict Clippy、前端 54/54、bundle 9/9、打包输入 15/15、真实 Rust 签名验证 5/5、临时 NTFS 7/7、完整默认合规和 diff 检查通过。结构门 593 文件/63 处既有债务。验证启动后未修改源码；日志 `target/installed-product-evidence/composition-extraction-20261024/code-quality.log` 的 SHA-256 为 `BE865AA58DF22F117C23F0A17475C05C10194FDC5E4F428EAC01332AC86FBC87`。

本次只完成维护拆分与其边界证据，不改变 IP 或发布状态；已确认的模型角色摘要绑定按独立行为变更继续实施，不能用本次纯拆分的回归代替其验收。

## 9. Gateway 控制实现拆分（2026-10-26）

`ece53be` 将 `AgentHarness`、`GatewayControl` 与 `Observability` 的生产实现集中到职责明确的 `gateway_control.rs`；`lib.rs` 只保留入口与重导出。移动前后的实现文本保持一致，并新增根级测试确认原公开 trait 仍由正式 Harness 实现。文件行数为 300，符合新增生产文件上限。提交前完整 workspace tests、fmt、check、strict Clippy、Python 结构门及其回归、`py -3 tools/code-quality.py` 与 `bash tools/compliance-doctor.sh` 均通过；没有扩大 63 项历史规模债务。M5 第一次当前树复跑中，所有行为与质量检查通过，唯一失败是仓库 S99 回执仍对应旧源码；该次不记作完整 M5 PASS，后续刷新与最终状态见 M5 报告 §10。

## 10. Harness 动作策略职责拆分（2026-10-26）

`3c942af` 将 Harness 内的动作分类、策略降级、审批/能力门与能力快照校验提取到 `crates/harness/src/action_policy.rs`（298 行），`lib.rs` 减少 241 行。新增边界测试证明通信来源不能保留 `Allow`。该提交没有改变公开 API、事件、事务、执行顺序或恢复语义，也没有新增 crate、内部依赖边或 EventKind。

本轮 `verify-m5.ps1` 的 S1–S99、C1–C24、workspace Rust tests、strict Clippy、Python 89 项、合规及 M4/M5 专项均通过；默认全量测试中 3 个真实 Windows AppContainer 场景仍为 ignored，需显式原生环境运行。M5 release audit 生成与独立验证通过，唯一失败是仓库回执仍绑定 `3c942af` 之前的源码。后续刷新回执与最终 clean-tree 比较见 [M5 验收记录 §10](m5-acceptance-report.md#10-产品化分支的永久回归重验)。本记录不宣称原生 AppContainer、安装服务或 IP01–IP24 通过。

## 11. Production context source 拆分（2026-10-26）

`ccfaa79` 将正式主脑上下文装载实现从 `production.rs` 提取至 `production/context_source.rs`（206 行），原文件减少 180 行；composition 仅更新模块装配。新增 C3 回归证明跨 workspace scope 在调用模型、追加事件或产生其他副作用前被拒绝。没有新增 crate、依赖边或 EventKind，也未改变公开 API、事务和恢复语义。

提交后的 `tools/verify-m5.ps1` 已运行完 S1–S99、C1–C24、workspace 回归、strict Clippy、Python 89 项（1 项平台跳过）、合规与 M4/M5 专项；生成的 M5 release audit 和独立验证通过。终线仅在仓库 M5 receipt 与当前 clean tree 比对时失败，原因是回执仍绑定此前源码；因此本轮不记为 M5 FINAL PASS。刷新回执和最终重跑结果见 [M5 验收记录 §10](m5-acceptance-report.md#10-产品化分支的永久回归重验)。默认全量测试中的 3 个原生 Windows AppContainer 场景仍为 ignored；本记录不宣称 IP01–IP24、真实系统服务或原生安装验收通过。
