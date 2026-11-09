# MCP stdio discovery 与调用 I/O 组件验收（2026-11-09）

分支 `shu/closure-inventory`。调用端实现为 `320d0b00888f301b18f19168bb2bdfc280d298f9`，发现端实现为 `a372499bd675f872418b29addc142a647a28573b`。对应实施细化先分别提交于 `4cf35fc`（PRD08 §5.1）和 `8709992`（PRD07 §5.1）。本轮只修复既有普通 stdio 的期限、容量和原进程/I/O 所有权；没有改变公开 DTO/trait、依赖、18-crate 图、99 EventKinds、unsafe 边界或安装版启用状态。

## 1. 修复行为

- 调用端不再在主线程同步写入子进程 stdin。原 deadline 和 CancelToken 覆盖编码前后、入队、写回执与响应；读队列最多两帧，写队列最多一帧，输入/输出单帧均最多 8 MiB（含换行）。传输上限与已审批 OutputBudget 分别执行。
- 发现端用一个受监督 I/O worker 顺序完成 initialize、notification、tools/resources list；全部阶段与 metadata 解析共用一个 deadline，不因无效帧或进入下一阶段续期。metadata 在显式 cleanup 成功后才发布或缓存。
- 两个入口各最多八个进程/I/O 槽位。关闭通道后保留原 child 和所有 worker，由 cleanup 停止、wait 和 join；主调用最多等待两秒。不能确认清理时保留原材料、阻止新的 admission，清理线程创建失败也不丢弃原句柄。原 child 的 wait 结果是退出依据，避免把退出与 kill 的竞争误判为清理失败。
- 调用端清理不明只产生 `McpCallEvent(server_error)` 和 `ActionOutcomeUnknown`，不追加 Completed/Failed/Cancelled；稍后的清理不重写该终态。发现端返回明确的 unverified ServerError，零 metadata、零 McpDiscovered，不虚构脑动作事件。

## 2. 真实子进程专项

调用端实际运行 9 个 Rust tests，发现端 5 个；其中新增六个调用端与两个发现端场景。fixture 为本仓库自建有限进程，不依赖真实 MCP 服务或模型。

| 场景 | 实际断言与结果 |
| --- | --- |
| initialize 后不再读输入，大请求超时 | 400 ms deadline 下实测 456 ms 返回，timeout 分类成立，没有完成或 Unknown。 |
| initialize 后不再读输入，启动后取消 | 实测 149 ms 返回，唯一 ActionCancelled，没有完成、失败或 Unknown。 |
| 超长输入 | 8 MiB 传输上限拒绝；独立 marker 证明 tools/call 未派发。 |
| 超长无换行响应 | schema 错误；调用端零完成/输出，发现端零 metadata/事件。 |
| 噪声、不匹配帧与输出预算 | 错误帧不改变原期限；截断与严格预算分别保持既有行为。 |
| 后代持有原调用管道 | 实测 2554 ms 返回清理未确认、唯一 Unknown；新调用被阻止。有限持有者结束且原 worker join 后，仅新的不同动作可执行。 |
| 后代持有原发现管道 | 原 discovery 不发布；新 provider 在原清理未完成时不能启动，原 worker join 后才可发布新 metadata；原次结果不迟到发布。 |

这里证明有界返回与原清理监督，不证明后代被强制隔离或终止。有限 fixture 自行结束后才能 join，不能把它作为安装版 sandbox 或 Host grant 的证据。

## 3. 最终完整组件门与证据绑定

本地时间 **10:10:11–10:30:36**，八个阶段全部 exit 0：fmt、调用端专项、发现端专项、workspace all-targets check/test、workspace strict Clippy、结构质量与完整 compliance。workspace 为 **187 targets、1,337 passed / 0 failed / 22 默认 ignored**；两个专项的结果不叠加进该数字。质量门为 897 files / 63 existing size debts。

最终检查绑定 `8709992b563082ea6185df8f52322a3a2101b879` 加六个精确源码快照；上述两个实现提交仅拆分保存这六份已验证内容，提交前后逐份 SHA-256 相同。独立 verifier 核对八份日志、源码、原验证脚本、时间顺序、专项场景名、完整测试计数和资源报告，落盘结果 PASS。核验脚本末尾对 OrderedDictionary 的摘要投影显示空值；随后将落盘 JSON 重新解析为对象，独立确认 status、所有计数及 installed_acceptance，原回执未改写。

原件位于本工作树 `target/closure-inventory-evidence/`；process 回执包含八份日志及六份源码的全部摘要：

| 工件 | SHA-256 |
| --- | --- |
| `mcp-transport-final-checks-process.json` | `ff3f826a4a23ddcc965e2669c34f3f266ed0fa457b504448dd39b4283ea6fef8` |
| `mcp-transport-evidence-verification.json` | `ef1579137addbca993b4847643801424f9a51bd6eee14f91c733e9a73e813480` |
| `mcp-transport-final-test.log` | `b909e9886aaa0604d273b82fb04df3c7cae1b3d04694c293201d8cee76498f69` |
| `mcp-transport-final-build-resources.json` | `554b06084973a982fc113682b6fecbf916e0502e6c74a03e8d4b71670f434423` |
| `mcp-stdio-io-checks-process.json`（先行停止原件） | `e09e9ca8b5ebf33462d106f2a8bdcc8e782bc0f46ef8638b15caa1bdea5930f3` |
| `mcp-idle-cache-compression.json` | `353680d9968bf3100fae2ee65422e7333d5e73240a92bdf5bb5e7280c445cdde` |

先行调用端版本的整仓测试为 1,335 passed / 0 failed / 22 默认 ignored，随后原 20 GiB 空间门在 Clippy 前拒绝，因此该轮整体为 exit 1，不记录为 PASS。其后复核修正源码并补齐 discovery，最终八阶段针对新的六份快照独立运行，不叠加两轮计数。期间仅对 65 个闲置 Cargo 产物做前后摘要相同的 NTFS 可逆压缩，删除 0 文件；该压缩窗口空闲字节从 21,426,135,040 到 23,848,042,496，不把其他并发磁盘变化归因于本轮。

最终所有构建阶段的原空间门通过；末次资源报告 PASS，缓存约 43.5 GiB 逻辑 / 16.6 GiB 分配，未超过 64 GiB 预算。固定 target、原 dev/test 配置与 20 GiB 下限保留。

## 4. 仍开放的义务

安装版 managed discovery/call、受管 program/source、Host 授权、隔离 placement、受保护输入/结果与设置消费者仍未闭合，见[剩余义务审计](installed-product-remaining-obligations-audit-20261109.md)。本轮不部署、不调用真实模型、不刷新完整 Final/M5、不提升任何完整 IP01–IP24 状态。

该轮结束时，Shell 继承管道清理及 MCP 已产生副作用却无可用响应时的原动作结果/Harness 重试决定仍待核对。随后 `1b6884f` / `d43e27a` 已用真实有限子进程和 SQLite 闭合后者，见[MCP Unknown 组件验收](installed-mcp-unknown-outcome-acceptance-20261109.md)；普通 Shell 仍开放。上表是原轮的历史结果，其中阻塞输入超时/取消的普通终态已由后续修复收紧为 Unknown，不能继续作为当前行为描述。

原本机候选为 `7b13529`，窗口前安装版本为 `cf3fbc9`。本轮收尾期间，原 UAC 请求取得启动回执，10:34:47 原生及产品窗口均以 stopped 留证：候选入库 committed，正式升级的原操作尚未结算，文件矩阵和数据阶段未执行。当前状态及三份原件摘要见[剩余义务审计 §7](installed-product-remaining-obligations-audit-20261109.md#7-原生窗口取得回执后的状态更新)。这不属于本轮 MCP 源码的部署；没有重派原窗口、Unknown 或修改系统安全策略。三份冻结提案等待明确答复，普通 I/O 修复不提供它们的实施授权。
