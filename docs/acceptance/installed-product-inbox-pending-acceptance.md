# 运行待决收件箱投影组件验收

2026-10-23，施工分支 `shu/installed-product`，实现提交 `95b9d71`。范围为 [prd/26 §5.5](../prd/26-web-workbench.md)、architecture/10 §10 与 canonical §29.7 中运行待决项进入收件箱的组件义务。本报告不是 IP20、IP24 或完整安装/Web 产品验收。

## 实现

- 正式 Harness workbench 投影按 run 使用现有 `project_run_pending`；只为已接纳且当前 `pending != null` 的非终态运行产生 `WorkbenchItemKind::Waiting`。
- `ApprovalWait`、`ToolInterrupt`、`Handoff`、`ResolutionUnknown` 保留 typed `state`，项目事件/序号指向原 pending checkpoint；用户可读摘要是固定文案，不复制原始参数、外部内容或未授权文本。
- Web 收件箱统一从正式 Inbox 投影读取待决项；条目可导航至原 run，Inbox 不新增批准、探测、裁决或新 command 路径。
- 终态 `Failed`/`Limited` 不再按运行状态伪装为 waiting。正式 failure evidence 仍依原投影作为 Failure 条目展示。
- 未新增 `EventKind`、protocol 对象、冻结 trait、状态机、依赖边或第二事实源；静态前端 bundle 已重新生成并通过锁定资源校验。

## 验证

- `cargo test -p forme-harness inbox_projects_only_active_pending_checkpoints_with_fixed_safe_summaries --lib`：PASS。
- `cargo test -p forme-gateway --lib installed_restore::installed_restore_worker_holds_both_real_databases_backs_up_and_fences_sessions -- --nocapture`：PASS。
- `cargo test --workspace --all-targets -j 1`：PASS。首次并行 workspace 执行有一个 Windows worker HTTP 测试遇到 `ConnectionAborted (10053)`；该用例单独复跑通过，串行全量复跑通过。回归同时发现认证 identity schema v8 不被本地 session fence 接受，修正为允许现有受支持的 v6–v8；恢复 fencing 集成用例随后通过。
- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets`、`cargo clippy --workspace --all-targets -- -D warnings`：PASS。
- `npm run verify`：TypeScript 检查通过，前端 35/35 测试通过，嵌入静态资源校验通过（7 个文件、70 个锁定依赖）。
- `pwsh -NoProfile -ExecutionPolicy Bypass -File tools/verify-web-workbench.ps1`：8 个命名 Web 工作台集成场景通过；该门使用计数模型 fixture。
- `py -3 tools/code-quality.py`：PASS（548 个文件，63 项历史规模债务未增长）；`bash tools/compliance-doctor.sh`：PASS（upstream/license/copy）；`git diff --check`：PASS。

## 未覆盖

本组件没有真实已认领浏览器会话、浏览器刷新/断线恢复、多 Web 进程竞争、真实 provider 或系统服务安装证据。普通主动建议 Unknown 的刷新后原操作恢复、完整收件箱 workflow、完整 IP20 与 IP01–IP24 联合验收仍开放。对应产品化状态见 [实施进度](installed-product-implementation-progress.md) 与 [设计交付台账](approved-design-completion-ledger.md)。
