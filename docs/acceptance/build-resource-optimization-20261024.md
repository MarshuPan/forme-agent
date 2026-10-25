# 构建资源优化实测（2026-10-24）

## 范围与原因

在 `shu/installed-product`、HEAD `d65cf1df7d7dd0ac9c76c2867fa91a492bc53db5` 的未提交工作树实施。仅修改构建参数、工程检查与缓存维护；未修改运行时 Rust 源码、crate 依赖、事件、锁文件或质量基线。

主要来源是完整调试信息、大量独立测试/特性组合生成的二进制、增量编译缓存，以及多个工作树和临时 target 的重复累积。清理前采样的网关测试程序约 801.54 MiB，其中调试段约 604.13 MiB；网关 daemon 约 776.62 MiB，其中调试段约 584 MiB。日常测试反复清空全部 target 会损失复用，也不能解决默认产物过大的原因。

## 已落实的控制

- dev/test 默认 `debug = "line-tables-only"`、`incremental = false`，保留行号回溯和原有运行检查。调试局部变量需临时覆盖；关闭增量编译增加修改后的重编译成本。
- 每个工作树复用固定 target；任务专用 target 保存证据、确认无占用后回收 dev 产物。
- 质量门与 M0–M5、closure 门接入构建前剩余空间检查；默认最低 20 GiB，每工作树开发缓存预算 64 GiB。预算超出只报告，不自动删除。
- `build_resources.py` 提供统计、默认 dry-run 和显式 apply；拒绝越界、链接与活跃/无法确定归属的构建进程。删除只经 Cargo 的 `clean --profile dev --frozen`，前后校验保留文件。
- CI 缓存键包含根 profile 所在的 Cargo.toml；M0 恢复前缀也绑定该配置与工具链，避免恢复旧 profile 的 target。

其他工作树本次只清理缓存，不改变其源码；后续合入配置才会使用新的默认值。规则和使用命令见 [build-resources.md](../engineering/build-resources.md)。

## 已执行的清理与保留证明

所有目标均在无占用后逐个执行。回收量是各操作前后卷可用空间的实测差值，系统同时有其他磁盘活动，不能视为独占卷测量。

| 目标 | Cargo 删除的逻辑量 | 卷可用空间增量 |
| --- | ---: | ---: |
| 主工作树 `target` 的 dev 产物 | 230.0 GiB / 143,557 文件 | 219.42 GiB |
| `harness-control-maintenance` 工作树 `target` 的 dev 产物 | 170.2 GiB / 112,523 文件 | 158.46 GiB |
| 上述工作树 `target/managed-skill-config-check-20261024` 的 dev 产物 | 13.9 GiB / 11,185 文件 | 11.76 GiB |
| 合计 | 414.1 GiB 逻辑量 | 389.64 GiB |

三份清理回执的 `preserved_before == preserved_after`，且剩余 dev 文件数均为 0。主工作树校验覆盖 917 个源码/配置文件、41,491 个运行数据/研究文件，以及 3,025 个保留的 target 文件；release、安装包、验收证据、`.forme` 与 `research/upstreams` 均在保留范围。HEAD 和 Git 状态在各次清理期间一致。

原始回执留在本地 `target/build-resource-evidence/20261024/{main,worktree,isolated}-cleanup.json`，本次新日志也保存在 dev 目录之外。

## 重新构建测量

清理后已确认开发缓存为 0。编译器进程采样确认 `debuginfo=line-tables-only`，没有 `incremental=` 参数。当前同类网关测试程序约 274.02 MiB、daemon 约 263.13 MiB，均约为旧采样的三分之一；新程序仍含行号调试段。

单个程序比较用于说明产物体积变化；旧累计缓存和新完整回归的缓存总量属于不同生命周期，不能把二者差值全部归因于调试参数。容量报告会区分逻辑大小与硬链接去重后的分配大小；阶段采样峰值不代表精确瞬时峰值。

## 质量门验证

`tools/verify-code-quality.ps1` 完整复跑 PASS，耗时 621.20 秒。覆盖结构/缓存安全/字节一致性 Python 回归 20 项、格式、workspace check、167 个 Rust 测试目标（936 passed、0 failed、5 按原定义 ignored）、strict Clippy、前端依赖锁定与漏洞检查、前端验证、包完整性与签名、Windows 权限组件及只读防火墙策略、合规与 diff 检查。依赖审计为 0 vulnerabilities；Windows 组件 8/8、只读策略 25/25，不代表原生服务安装或 IP 产品验收。

首轮冷 check 为 47.51 秒，测试编译为 128 秒。首轮完整质量门在网关 `controlled_shutdown_waits_for_the_real_model_run_and_releases_the_listener` 的 3 秒关闭期限处超时而失败；随后该项独立复跑 PASS（测试本身 2.10 秒），完整质量门复跑 PASS，未修改用例或关闭语义。首轮失败回执保留，不将这两轮拼接成一次成功的冷门耗时。缓存复用时 check 为 0.43 秒、测试编译为 0.46 秒。

日志和统计位于本地证据目录的 `quality.log`、`quality-retry.log`、`quality-summary.json`；PowerShell 语法和两份变更 workflow 的 YAML 解析也已通过。

## 永久门与并行工作区边界

S1–S69 永久回归已通过；M4 协议、store、副本进程、TLS、Harness、协调、产物及 Gateway 契约检查已执行通过。M5 整门随后在构建 M4 executor 时被同一工作树内另一项正在进行的运行时修改打断：新 `host_release.rs` 当时无法导入 `ReleaseWorkerSession`。该项修改修正后，原命令 `cargo build --locked -p forme-execution --bin forme-executord` 独立复跑 PASS（0.33 秒）；这不补记为 M5 整门通过。

初次 A/B 门同样被上述导入问题阻断。C 门发现一个既有过时引用：`brain_orchestration::tests::...` 重构后位于 `brain_orchestration::support::tests::...`，原门因实际执行 0 项而拒绝。本次将门中的引用对齐真实模块，保留必须恰好执行 1 项的断言；相关 Python 审计回归 22 项通过，门文件保持 598 行。

D `-RuntimeOnly` 门 PASS（402.26 秒），包括真实 daemon/CLI 入口和受控六工作流；它不评估 live C24，也不关闭 D/Final 产品验收。Final 因当前未提交工作树而拒绝，未修改签名回执或用旧回执替代。

14:52:07 UTC 再次检查源码时，并行修改中的 `crates/execution/src/installed/journal.rs` 为 682 行，超过当前 600 行上限，故全库结构门 FAIL。前述完整质量门 PASS 对应其完成时（14:10:31 UTC）的源码快照，不能表述为随后持续变动的整个工作树仍然通过。本次没有修改这些运行时代码、Cargo.lock 或质量基线；任务文件的 diff 检查及 20 项工具回归另行复跑通过。原始失败、后续复跑、当前源码检查均保留在本地证据目录。

A、B 复跑分别 PASS（119.10 秒、83.04 秒），包含显式忽略入口和各自独立证据校验。

C 修正后完整复跑 PASS（215.42 秒），过时路径对应的测试实际执行 1 项并通过，随后六组新生成和仓库内 C 产物的独立校验均通过。A/B/C 和 D runtime 的通过不替代未完成的 M5 整门与 Final 发布验收。

## 最终空间快照

2026-10-24 14:58:56 UTC（北京时间 22:58:56）测量，分配量均按硬链接去重；统计只覆盖注册工作树及当前 target，不是系统盘全盘扫描。

| 项目 | 结果 |
| --- | ---: |
| 当前主工作树开发缓存逻辑量 | 35.45 GiB |
| 当前主工作树开发缓存分配量 | 32.95 GiB |
| 阶段报告中开发缓存分配量的采样峰值 | 32.95 GiB |
| 主工作树整个 target 分配量（含保留产物/证据） | 36.32 GiB |
| 另两个注册工作树的开发缓存 | 各 0 GiB |
| incremental 目录 | 0 文件 / 0 字节 |
| C盘测量时可用空间 | 361.55 GiB |
| 三次清理的卷差分合计 | 389.64 GiB |

当前开发缓存低于 64 GiB 报告预算，保留该阈值。最终网关测试程序约 274.02–274.10 MiB、daemon 263.23 MiB；期间有并行运行时修改，不能当作完全相同源码的性能基准。清理差分包含同时发生的系统活动，也不能直接与最终卷余量相减得出独占构建消耗。

空间原始数据见 `final-storage.json`、`storage-summary.json`、各 `*-space.json` 和 `binaries.json`；它们位于上述本地证据目录。阶段采样峰值没有连续监测瞬时临时占用。最终全库结构检查仍因并行修改的 journal.rs 682 行失败；本次文件检查通过，Cargo.lock/质量基线均无改动，未提交或刷新发布回执。
