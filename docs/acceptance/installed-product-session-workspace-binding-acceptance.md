# 会话 Workspace 运行上下文绑定组件验收

日期：2026-10-23

范围：`prd/26 §4.5/§5.2`、`architecture/10` 与 canonical §29 中“会话关联 workspace 影响该会话后续新 run”的实现义务。此报告只记录该组件，不代表完整 Web 工作台、安装产品或 IP17–IP24 通过。

## 已实现

- 已认证 Web 服务从 owner 的持久会话记录解析 workspace；浏览器提交的 `RunRequest` 没有 workspace 覆盖入口。新 command 在 identity DB 单写者事务中固定 owner、session、规范化 request digest 和 workspace。未知响应沿原 command 核实；同 key 异请求冲突。
- workspace 在签名 Web→core 请求摘要覆盖的 `WorkbenchRunSubmission` 中传递。core 校验 AuthContext/bridge owner、session 一致性、envelope schema 与请求摘要，再调用现有 `ProductionRuntime::submit_workspace_run`。core/runtime 继续检查 workspace 注册与项目约束，并通过既有 `SessionBound` 事件固定 run 的实际 workspace；未新增脑 EventKind、crate 或内部依赖边。
- 同 key 同请求重放沿用原 workspace/run 回执，即使此后会话 workspace 被改绑或归档；已归档会话不能创建新 command。未关联 workspace 时使用正式 runtime manifest 中的默认 workspace。workspace 关联不授予额外工具或执行权限。
- 正式 workspace run 在接受后由后台执行，不会让 Gateway 的 HTTP blocking pool 等待模型调用完成。

## 已观察到的验证

- `server::lifecycle_tests::web_workbench::web_workbench_runs_through_the_brain_once_and_logout_fences_core_proofs`：真实 loopback Web→签名 bridge→正式 ProductionRuntime 集成测试通过；观察到 `SessionBound` 指向关联 workspace，重复请求得到同一 run，异义同 key 冲突，归档 session 的新请求冲突。
- `one_owner_brain_runs_distinct_workspace_projects_and_replays_both`：既有主脑 workspace 隔离与重启 replay 永久回归通过；它只证明 runtime workspace 隔离/replay，不被单独用作 Web 绑定证据。
- `cargo test --workspace --all-targets --locked`：通过（完整 V1 A/B/C/D、C24 golden 与永久回归；`profile.dev.debug=0`、`profile.test.debug=0` 仅减少调试符号，输出在本轮专用 target）。
- `cargo clippy --workspace --all-targets --locked -j 1 -- -D warnings`：通过；`cargo fmt --all -- --check`、`py -3 tools/code-quality.py`（547 个文件、63 条既有超限记录未增长）、`bash tools/compliance-doctor.sh` 均通过。
- `frontend/npm run verify`：TypeScript、33/33 前端测试及嵌入静态资源核验通过。`tools/verify-web-workbench.ps1`：8/8 命名组件场景通过。
- 本轮使用仓库内专用 `target/verify-tests-20261023` 显式运行 `cargo check --workspace --all-targets --locked -j 1`：通过，耗时约 3 分 18 秒，实际锁定 `ctutils 0.4.5`。全量 `cargo test --workspace --all-targets --locked -j 1` 的前次完整运行通过；本轮确认性重跑在 Gateway 测试目标链接期间因 C 盘可用空间降至约 3.1 GiB 而主动中止，没有观察到测试断言失败，故本轮重跑不记为完成。专用 target 产物保留，未清理。
- 本轮 `cargo fmt --all -- --check`、`py -3 tools/code-quality.py`（547 个文件、63 条既有超限记录未增长）、`bash tools/compliance-doctor.sh`、`frontend/npm run verify`（TypeScript、33/33 前端测试与嵌入静态资源）均通过。全目标 strict Clippy 与 `tools/verify-web-workbench.ps1` 8/8 命名组件场景沿用本组件前次完整运行的通过结果，未在本轮因磁盘空间再次执行。
- M5 最终 release audit 的前次尝试因未提交工作树及当时锁定的受影响 `ctutils 0.4.3` 失败；锁文件已更新至修复版 `0.4.5`。本轮未重跑 clean-tree audit：其会重建并执行 M0-M5 全部历史门，而 C 盘仅余约 1.9 GiB；该门仍待磁盘空间充足后，在代码与文档分别提交后的干净树上完成。

## 未由本组件证明

- 真实已安装产品中的 Chrome/Edge 端到端交互、多 Web 进程争用和崩溃窗口恢复；当前证据是 Rust/SQLite/loopback integration fixture。
- Windows/Linux/macOS 原生服务账户、安装器、LAN TLS、完整工作台及 requirements/18 的 IP01–IP24。
- 本组件通过不表示 prd/26 全部完成；仍需实现并验收其他已批准产品义务，并与 prd/24–25 完成正式联合验收。
