# 运行挂起恢复与裁决工作台组件验收

日期：2026-10-23

范围：`prd/03 §5.2.1`、`architecture/09 §12`、`prd/26 §5.3` 中的 pending 查询、owner 裁决恢复和模型调用未知结果处理。

## 已实现

- 增加 owner 认证后的 `RunPendingView`，从完整有序事件流只读投影审批等待、工具中断、handoff 和裁决结果未知状态；不改变 `RunSummary` 与冻结的 99 个 `EventKind`。
- `ResolveToolOutcome`/`ResolveHandoff` 保留完整 command。owner/run/command/resolution/checkpoint 摘要写入既有受保护内容 envelope；密文伴随记录与 `RunResumed` claim 在 `forme-store` 单写者的同一 SQLite immediate transaction 中提交，并通过现有数据生命周期目录登记。
- 同一 command 同义重放读取原结果；异义、跨 run 复用和同一 checkpoint 换号裁决被拒绝。claim 丢失结果、action 终态后 run 终态缺失、已解决 handoff 后续中断均保持 Unknown，不调用工具或模型。
- 启动恢复扫描发现没有配对 `ModelCallFinished` 的 `ModelCallStarted` 时，以 `model_call_outcome_unknown_after_restart` 终止原 run；不重发请求、不切 fallback、不补造费用或回复。
- Web 工作台读取 pending 投影，为 owner 提供明确裁决与原 command 核实入口；刷新后只恢复后端验证过的原命令，不自动提交、不生成替代命令。模型回复/费用未知有专门说明。
- 从 Harness/Gateway/Store 大入口中拆出运行控制、恢复、密文伴随记录及领域幂等实现，未新增 crate 或依赖边。

## 验证

- `cargo fmt --all -- --check`、`cargo check --workspace --all-targets`、`cargo test --workspace --all-targets`：均返回 0。完整 workspace 回归包含长时项目/DAG 与 V1 A/B/C/D 场景。
- `forme-harness/tests/control_recovery.rs`：11/11 通过，覆盖并发重复裁决、进程重启、丢失结果、handoff 续推中断和模型调用未知。
- `forme-store` 安装数据场景：`installed_artifacts` 5/5、`installed_data_workbench` 7/7 通过，覆盖加密 companion、生命周期 fence、删除后不可复活和 owner/session 绑定。
- `pwsh -NoProfile -ExecutionPolicy Bypass -File tools/verify-code-quality.ps1`：返回 0。strict Clippy、结构检查（545 个文件；63 处既有规模债务未增长）、前端 31/31、嵌入资源复现、bundle 9/9、原生打包 14/14、Rust 签名验签 5/5、Windows NTFS/ACL 7/7、合规与许可证均通过。
- 默认忽略的真实 Chrome/Edge 浏览器和独立多进程测试未由普通 workspace 测试执行；产品安装 IP01–IP24 仍需单独验收。

## 边界

这是运行恢复和工作台能力的组件证据，不代表整个已批准设计实现里程碑完成。没有使用真实 owner 数据、模型凭据或系统服务；真实 provider 质量、管理员 SCM/服务账户有效拒绝、跨平台原生安装、完整 Upgrade/Rollback 与 IP01–IP24 联合验收仍按各自报告保持未验证/未完成。
