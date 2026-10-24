# Windows Sandbox Lease Journal 组件验收

日期：2026-10-25。实现提交：`82493ea`，分支 `shu/installed-product`。范围：PRD24 §4.1.2 的 Windows per-job sandbox lease 持久化与恢复底座；不等同于真实 AppContainer adapter、IP10 或 IP24 验收。

## 1. 已实现范围

- 固定 `runtime_root/sandbox-leases.jsonl` 路径、单 OS writer lock、有界 append-only JSONL、closed schema、单调序号和哈希链；损坏、截断、超限或并发写者均 fail closed，不自动修复。
- 以 job、plan digest、AppContainer identity、规范化 job 目录和目录 file identity 绑定 lease；未完成的 lease 不能共享 job 目录路径或 file identity。
- 按 intent/readback 记录 ACL grant/revoke，记录进程树终止、AppContainer 移除、job 目录移除和最终 cleanup receipt。恢复只投影原 lease，不重放 worker 副作用。
- 启动准入检查会阻止尚无 durable cleanup receipt 的 lease；journal 与 brain EventKind/brain store 分离。
- Windows 扩展规范路径会跳过 drive-prefix 元件的文件系统查询，再检查其余组件，避免把 `\\?\` 前缀误当作普通目录组件。

## 2. 验证结果

| 检查 | 结果 |
|---|---|
| lease 专项测试 | 7/7 PASS：intent 重启恢复、精确撤权、回执重开、双 writer/torn journal、哈希篡改、身份绑定、活动目录路径/file identity 唯一性 |
| `tools/verify-installed-host.ps1` | PASS；新增的 7 个 lease 场景均被永久门实际执行 |
| `cargo fmt --all -- --check` | PASS |
| `cargo check --workspace --all-targets` | PASS |
| `cargo test --workspace --all-targets` | PASS，使用默认测试过滤；显式 ignored 的浏览器/额外进程前置场景未因此验收 |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS |
| `py -3 tools/code-quality.py` | PASS（693 files，62 existing size debts） |
| `bash tools/compliance-doctor.sh` | PASS |
| `git diff --check` | PASS |

`verify-m5.ps1` 在本轮早期运行时，workspace/M5 回归和合规检查通过；S99 唯一失败为 `RELEASE_TREE_DIRTY`，因为当时存在未提交的其他工作区改动。之后新增目录唯一性约束后，以上 workspace fmt/check/test/strict Clippy、质量、合规和 installed-host 门均针对最终实现复跑通过；没有把早期 S99 结果写成此提交的 clean-tree release audit PASS。

## 3. 未闭合的安全边界

本组件只提供 journal、状态投影和恢复准入原语；它不执行 OS ACL 修改，不创建/核验 AppContainer，不创建受限 token 或启动 worker，也没有接入生产 dispatch。`record_acl_readback`、进程终止证明和 AppContainer 不存在证明仍是由未来平台 adapter 提交的声明，本报告没有真实 NTFS ACL 正反访问断言或 token 证据。`record_job_directory_removed` 会核验路径已不存在，但真实删除前对绑定 file identity 的复核仍须由 OS adapter 执行。

因此生产 worker dispatch 仍保持 blocked；服务账户/peer 身份、有效 token、ACL、网络与 credential 拒绝、崩溃后原 lease 的真实 OS 清理、管理员 SCM 安装及 Linux/macOS 原生隔离仍待实现和验证。IP01–IP24 均未由本组件门验收，尤其 IP10/IP24 不得标记 PASS。
