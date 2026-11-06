# worker 恢复静止的实际资源守卫（已确认）

2026-11-06 owner 在上轮明确列出两项下一步后指示“继续下一步”，本协议与固定 Host 路径恢复据此推进；正式契约已回填 PRD24 §4.1.4 和 architecture/10 §7.1.4。实现/检查结果另记，不由批准推出已通过。

本提案只补齐 PRD24 与 canonical §29.11/29.13 已要求的真实 worker 静止检查，不增加产品能力、脑事件、crate、依赖边或 unsafe 边界。回执自报修正不能独自满足本项：当前 `with_quiescent_workers` 持 operations 锁并核对原 worker journal，但没有经过隔离 adapter 的实际子进程登记和 sandbox lease。

## 1. 唯一接口增量

保留 `IsolatedExecutorHost`、WorkerJob/Permit/Receipt 和全部 HTTP 协议。给既有 `WorkerIsolation` 增加以下方法，缺省实现返回 unavailable，绝不缺省调用 action：

```rust
fn with_quiescent_resources(
    &self,
    action: &mut dyn FnMut() -> forme_protocol::Result<()>,
) -> forme_protocol::Result<()>;
```

该接缝只证明并保持本 adapter 所管理执行资源的当前静止，不证明历史从未执行、不创建权限、不主动终止或清理 job、不探测后改写 job 终态、不修复日志、不新增结果封存。所有检查成功才调用一次 action；检查失败或静止不可证明时调用零次。action 的错误原样返回，守卫在 action 返回后释放；同一 callback 不可重入 worker/lease 控制方法。

## 2. Host 与平台执行顺序

顺序固定为：Host operations 排他 → adapter 当前资源守卫 → 原 worker journal 的有界原件/终态检查 → 原恢复 callback。保持资源守卫直到 callback 返回，不能检查后释放再裁决；不改变已有 `forme-worker-quiescent-v1` 摘要域。调用者无须也不能提交“资源已释放”证据。

- `UnavailableWorkerIsolation` 及没有实现真实资源检查的 adapter 使用缺省拒绝；空 worker journal 不能使缺失的观察能力变为成功。
- Windows AppContainer 在实际 children mutex 内要求 registry 为空、没有 recovery-required 状态；再持原 sandbox lease writer 检查有界原件、schema/序号/hash/fold 和内存水位一致，且全部 lease 有完整 cleanup receipt。任一活进程登记、未清理 lease、poisoned writer、原件不一致或锁/IO 失败均拒绝；原 lease writer 与 registry 排他保持到 callback 返回。
- Windows Job、开发 Process、Linux systemd 与 macOS launchd adapter 只可在其自身实际 children 排他内证明当前登记为空，并持锁执行 callback；自然完成/取消仍须先经过既有 OS 观察、结果封存和回收链。受限平台的当前 registry 检查不提升隔离能力或发布状态，不能证明未知历史进程不存在。做不到所需当前资源证明的 adapter 继续 unavailable。
- 桌面 helper 的空闲会话不要求注销；任何未定 job 仍由原 worker journal 的 Unknown/在途检查阻断。此增量不替代桌面会话、本源档案或原生服务身份验证。

sandbox lease 的原件检查保留原句柄、OS 锁、字节和位置，不开第二 writer。损坏、合法截断、额外记录、同水位异链或 IO/位置恢复失败使 writer 保持 poisoned；一致但未清理的 lease 只拒绝当前静止检查，不 poison 正常在途状态。既有 `verify_startup_recovery_complete` 同样不能忽略 poisoned 或只信缓存。原 journal 摘要与历史保留，不补签、不回填历史。

## 3. 验证与实施边界

先回填 PRD24 与 architecture/10 的以上接口/顺序，再实施。测试至少覆盖：缺省 adapter 零 callback；日志已终态但实际 registry 非空仍拒绝；原 lease 损坏/截断/poisoned/未清理拒绝；有效静止成功；callback 期间并发 admission 不进入平台；callback 错误保持；真实 Windows Job 在运行/取消前后的拒绝/成功；原件字节/位置/锁保持。可信测试 adapter 必须显式实现受控观察，不能沿用默认成功。

按仓库规则执行定向、完整源码质量和永久宿主门，代码与文档分开提交。真实 AppContainer/SCM、完整历史 collector 和 IP01–IP24 另取证；不从默认 ignored 或非 Windows 交叉编译宣称原生通过。本资源守卫项的批准范围为契约细化与代码/隔离测试；同条“继续下一步”中原 Host 的 SCM 路径修正和限定启动按独立[固定恢复计划](../../acceptance/installed-original-host-path-restore-20261105-plan.md)执行，已取得[实际结果](../../acceptance/installed-original-host-path-restore-20261106-result.md)。两项均不扩展为程序替换、数据迁移、旧 Upgrade 重派或卸载。

## 4. 确认依据与后续边界

AGENTS.md 硬规则 3：**“PRD 里的 trait 签名与协议对象是冻结契约……要改先提文档变更，经确认后代码跟进。”** 本项新增平台 trait 方法，已先提出完整协议再取得上述继续指示，不夹带于 `eb99013` 的回执修复。批准覆盖本文契约细化与代码/隔离测试，未完成的实现/真实平台证据继续如实记录；不扩展为其他本机操作授权。

## 5. 本批实施与证据

正式契约提交 `0ba3f3e`，实现提交 `e649b84`。十二个新增命名场景已进入永久宿主门；完整源码门（1281 Rust passed / 0 failed / 15 默认 ignored、strict Clippy、前端 115/115 与完整合规）及永久宿主门 379/379 均通过。原件、并发和平台证据的具体范围、失败日志、Linux 工具链限制与未部署边界见[本批验收](../../acceptance/installed-worker-resource-guard-acceptance-20261106.md)。原安装的完整历史 collector、管理程序迁移与 IP 不由此完成。
