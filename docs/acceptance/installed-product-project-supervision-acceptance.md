# 项目监管与工作空间调整组件验收

2026-10-24，施工分支 `shu/installed-product`，实现提交 `a53557f`。范围为 [prd/26 §4.7、§5.4](../prd/26-web-workbench.md)、architecture/10 §10 与 canonical §28.5/§29.7 的项目监管与后续分段调整。本报告是组件级实现证据，不是 IP19、IP24 或完整安装/Web 产品验收。

## 实现

- 主脑通过只读 `WorkspaceControlSnapshot` 返回有界工作空间/项目、权威 project digest、监管版本、检查点、活跃 route、下次 cadence、审查状态、理由与证据引用；超过 16 项的 review 容量时未评估项目明确标记 `Unverifiable`，不伪报 `NotDue`。
- 暂停/恢复使用版本化 closed command、owner 身份、project digest/version CAS 与原命令幂等。事实折叠进既有 `DecisionTraceRecorded.orchestration`，未增加 `EventKind`、crate 或依赖边；旧 ledger 序列化与 digest 保持兼容。
- 暂停仅阻止之后的自动规划，不取消在途 route，也不阻止 owner 显式规划；恢复不立即调用模型/driver。背景 tick 与暂停提交并发时，最终 route append 重新检查监管状态。
- 工作空间页显示权威监管状态，提供暂停/恢复、后续项目分段参数编辑与长期意图取消。未确认的项目命令保留原 key/body；冲突必须显式重读后才能生成新命令。

## 验证

- `cargo test --locked --workspace --all-targets`：158 个 suite，851 passed / 0 failed / 5 默认 ignored。
- `pwsh -NoProfile -File tools/verify-code-quality.ps1`：PASS；包括 Rust fmt/check/strict Clippy、前端类型与回归、依赖审计、静态 bundle/包边界、release verifier、Windows NTFS access 组件 7/7、原创与许可证门及 `git diff --check`。
- `pwsh -NoProfile -NonInteractive -File tools/verify-web-workbench.ps1`：8 个命名工作台集成场景 PASS；包含正式 ProductionRuntime、认证/CSRF/签名 bridge、项目快照读取、owner 暂停、同命令重放、陈旧版本拒绝和无额外 route 断言。测试使用仓库自建计数模型 fixture。
- 另有 7 个 harness 级项目监管场景覆盖重启 replay、owner/digest/version/idempotency、并发 CAS、暂停与 tick 竞争、review 容量、历史 ledger digest 和伪造 provenance 拒绝；已包含于全量 workspace 回归。

## 未覆盖

- 真实已认领浏览器会话及窄屏/键盘交互未验证，因此 IP19 仍为 `NOT_RUN`；本地非管理员上下文没有注册 forme 系统服务，真实服务账户、有效权限拒绝与 IP01–IP24 均未因此获得通过证据。
- 计数模型 fixture 不证明真实模型质量；源码门、Vite/组件测试或本报告均不能替代正式安装包上的 IP24 联合回归。

后续须在受控安装环境完成真实浏览器项目编辑/监管/重连流程，并按 IP01–IP24 独立保留安装包、OS 服务管理器、有效权限和反向断言证据。
