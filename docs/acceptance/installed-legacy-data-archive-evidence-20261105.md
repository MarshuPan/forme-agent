# 历史数据档案来源与身份比较组件（2026-11-05）

状态（2026-11-06）：测试专用的来源与身份比较组件完成，代码提交 `0471750`；当前源码完整 M5/S1–S99 终验已通过。本记录不是历史恢复 Verified、生产 collector 接通、原生安装恢复或 IP14–15/IP24 的通过证据。对应已批准协议的私有细节见[历史审查 §9.7](../architecture/proposals/installed-legacy-release-recovery-review.md#97-历史数据档案的来源与比较)。

## 1. 实现与边界

`execution/installed/legacy_data_archive` 在 `cfg(test)` 下提供固定目录原件读取、独立来源摘要与 Ed25519 严格验签、完整安装/原 operation/command/release/config 绑定及逐字段数据比较。它不进入生产构建。沿用既有 `ReadCustody`，不增加 crate、依赖、脑事件、unsafe 文件或公开产品 DTO；不写产品/脑 journal，不启停服务或调用模型。

原来源记录的原字节摘要和公钥必须由独立核实的受保护配置预置。仅有现场文件、当前新算摘要、自签记录或 owner 批准不构成历史来源。本机此前保存的报告没有在本轮生成 origin、补签、倒填时间或物化成该受信目录。

历史 `database_schema_version=null` 保持 null；当前实际版本独立进入观察摘要，不能通过比较动作回填历史。owner 摘要沿用 store 的原始 UTF-8 SHA-256；独立 golden 同时拒绝把 owner 当规范 JSON 求摘要的编码。历史服务条目只验证档案格式和内部一致性，不证明当前 SCM、完整恢复策略或 worker 连续性。

这个组件只返回 `ArchivedDataIdentity` 一项 evidence。调用者仍须持有真实数据 worker、验证原 journal 和其余六类材料，并完成全部八项的执行前重查。当前生产 `LegacyReviewCollector`、信任配置和管理修复的整条装配仍未闭合，未因本组件挂载默认成功 collector。

## 2. 验证

原创临时目录、原创档案内容和独立测试 key；不复制真实账号、密钥或用户正文。17 项测试覆盖正常比较和零写入，以及缺来源/错误来源/替换来源、错误公钥和签名域、原件字节漂移、安装/owner/operation/发布/配置不匹配、时间与水位冲突、可空/缺失/零版本、数据逐字段变化、错误 owner 编码、语义不符、未知/重复/错误类型字段、服务记录冲突、额外/缺失文件、硬链接、大小限制、持有期变化与超时。Windows 文件持有测试检查实际拒绝修改；它仍是临时文件证据。

- 定向：`cargo test -p forme-execution --lib legacy_data_archive`，17 passed / 0 failed。
- 当前提交质量门：`cargo fmt --all -- --check`、`py -3 tools/code-quality.py`（859 files，63 项既有规模债务未增长）、`cargo check --workspace --all-targets`、`cargo test --workspace --all-targets`、`cargo clippy --workspace --all-targets -- -D warnings`、`bash tools/compliance-doctor.sh` 均 PASS；测试构建收口后重跑的全 workspace suite exit 0。
- 永久 Host 命名门：`powershell -NoProfile -ExecutionPolicy Bypass -File tools/verify-installed-host.ps1` exit 0，348 个命名检查 PASS，包含本组件 17 项。完整输出留在本机 `target/legacy-release-review/legacy-data-archive-installed-host-gate-20261106.log`，SHA-256 `1a2b0446fe53ef641cc500a705ce2035746cee542403e0f0d80af918bd36c8ef`。末尾明确 `IP01-IP24: NOT_RUN`。
- 首次 workspace strict Clippy 曾因组件在生产构建未接线而报 19 项 dead-code；已将模块限定为 `cfg(test)`，然后重新执行格式、quality、check、全量 tests、strict Clippy、compliance 和永久 Host 门。
- 最终整门：`powershell -NoProfile -ExecutionPolicy Bypass -File tools/verify-m5.ps1` exit 0，实际终线 `M5 FINAL ACCEPTANCE: PASS`。包含 S1–S99、C1–C24、M4/M5 多进程 golden、全工作区、95 项 Python（1 项环境跳过）、strict Clippy、完整合规以及当前树发布回执独立验证和比较。workspace 阶段为 182 targets、1252 passed / 0 failed / 15 默认 ignored；新增档案组件相对前一统计增加 17 项。
- 完整终验输出留在本机 `target/legacy-release-review/legacy-data-archive-final-m5-20261106.log`，SHA-256 `06b2f74c2892fb81bb3b7cc6a5ce1942ca1c0cd7eec93b79f078ccc2f4ee02c2`。最终 release receipt 已由完整门生成、独立验证，并与仓库 current receipt 比较 PASS；最新摘要见 M5 acceptance report。

## 3. 本机与后续闭合

本轮只读观察确认四个原服务均 Stopped/PID 0；`legacy-data-archives` 和 `management-repair` 的正式固定材料目录不存在。没有改已安装程序、SCM、配置、keys、原数据或 journal，也未重发原 Upgrade。

原 Host 路径恢复继续按[既有一次性精确方案](installed-original-host-path-restore-20261105-plan.md)，不能由本组件测试推出 Apply 授权。其后仍需独立历史来源、原 updater 代码/构建与 worker 连续性、签名管理 reader 修复、真实 collector 和迁移/原生产品验收。旧 M5/closure 报告保留其原源码水位，不自动覆盖本轮代码；整体目标保持 active。
