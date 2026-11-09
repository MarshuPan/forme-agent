# MCP 未确认副作用与 Harness 恢复组件验收（2026-11-09）

分支 `shu/closure-inventory`。执行层提交 `1b6884f`，Harness 提交 `d43e27a`；实施细化先在 `2ba38d2` 回填 PRD08 §5.1。本轮修复已有 Unknown 不变量，不新增公开 DTO/trait、EventKind、crate、内部依赖边或 unsafe 例外。

## 1. 已复现的问题

自建有限 MCP 子进程在收到 `tools/call` 后，先追加并同步真实文件 marker，再丢失响应、发出无效响应或等待取消。原执行层会把已有副作用的未知结果记为 Failed/Cancelled。修正执行层后，原 Harness 仍将 backend 的 Err 当普通工具失败：前两种情形进入下一轮模型并结束为 Complete，取消情形结束为 Aborted；对已挂起 Unknown 再取消也会错误结束原 run。

这三组问题分别保留独立失败原件，不能用修复后的通过结果覆盖。另有早期 fixture 的构造函数名错误，以及以文件存在代替完整写入的同步错误；原日志保留，修正后才取得上述 Harness 复现。夹具取消现在等待完整 marker 内容；断言失败时保留其目录，避免析构二次 panic 掩盖原故障。

## 2. 修复行为与边界

- 编码/排队不计作工具派发。原 writer 在检查 deadline/cancel 后、首次尝试写 `tools/call` 前记录 dispatch；有效 JSON-RPC envelope、同一 request id 和可用工具结果/明确协议错误分别确认 outcome。
- 同时具有 result/error、缺失任一分支、错误 JSON-RPC 版本、无效协议 error，以及缺失/无效 content、structuredContent 或 isError 均不能冒充工具结果。结果仍受既有传输容量和输出预算约束。
- 工具请求可能已写入且没有确认结果时，timeout、断线、无效响应或取消只保留 `ActionOutcomeUnknown`，MCP 错误分类不丢失。kill/wait/join 确认清理不撤销实际副作用；派发前取消、有效成功和明确协议失败仍保持各自语义。
- Harness 的 sink 只在 store observer 成功后保存事件。当前 intent 已持久化 Unknown 时，backend Err 先挂起原 run，不能进入普通失败续接或把取消当结果；共用原有 Unknown 挂起/恢复逻辑。后续取消停止进一步执行，但不替代结果裁决。
- 原 run 同键提交、SQLite 关闭重开和再次启动恢复扫描，都不增加模型调用、工具派发、marker 或 Unknown。owner 核实原 marker 后使用持久裁决命令补原 action 终态；同号同义重放不再产生事件或副作用。

普通 stdio 仍不等于安装版 MCP 隔离或 Host grant。测试模型为显式 scripted provider，工具是真实有限子进程，存储为真实受保护 SQLite；不是外部真实 provider 或已安装系统服务验收。

## 3. 验证结果

三个专项分别为执行端 **13/13**、已有 owner/recovery **11/11**、新增 Harness **5/5**（含一个 fixture 子进程入口）。Harness 覆盖丢响应、无效响应、副作用后取消，以及有效成功/明确协议错误的正常续接；前三项均覆盖后续取消、原号重放、重启恢复和持久裁决。专项数量不重复计入 workspace 总数。

完整 workspace 为 **188 targets、1,346 passed / 0 failed / 22 默认 ignored**。fmt、all-targets check/test、strict Clippy、结构质量与完整合规均取得 exit 0；质量门为 898 files / 63 existing size debts。没有修改质量基线或减少测试。

原检查在本地时间 **11:07:13–11:26:15** 完成 fmt/check/test 后，下一阶段的原 20 GiB 空间门观察到 18.8 GiB，回执如实为 exit 1。随后实测可用空间恢复；**11:27:33–11:29:10** 仅续跑尚未执行的 Clippy/质量/合规，开始时原空间门为 35.5 GiB，续跑 exit 0。没有重跑完整测试、手工清理或压缩缓存，也不把卷空闲变化全部归因于构建。两个会话绑定同一 base `2ba38d2` 与七份精确源码；实现提交前后再次逐份核对 SHA-256 相同。

独立 verifier 核对两份回执、六阶段日志、验证脚本、相同源码、专项名称、全量计数和资源报告，落盘 PASS。末次资源报告 PASS：固定 target 缓存约 44.8 GiB 逻辑 / 17.2 GiB 分配，低于原 64 GiB 预算。检查日志与原件位于本工作树 `target/closure-inventory-evidence/`：

| 工件 | SHA-256 |
| --- | --- |
| `mcp-effect-outcome-final-checks-process.json`（原空间门停止） | `af13dbc7f148f203e21c5fcdf161a885b80103383c6c1c39085641025df90de8` |
| `mcp-effect-outcome-continuation-process.json` | `0bf846080c447f99011d0829be1014c5292ae5b936b8ac07d265c7fd56a21713` |
| `mcp-effect-outcome-evidence-verification.json` | `45dfd6de5903fcf1db88b3c47c245d5908882b92f1f539ead4a4643eef254aa5` |
| `mcp-effect-outcome-final-test.log` | `61d574fb562992418fdd308313102b74ba1b23a143862dfb2150fd84c9dfc2f8` |
| `mcp-effect-outcome-continuation-build-resources.json` | `c02bb9b868a0df6aaa729a667d8d627fdb061e2c8d9fbaf811acc7ff4d3aae9d` |
| `mcp-effect-outcome-original-reproduction.log` | `281c849a7c40a1d6a88698b0559ad4700684b69b8c26c8f5d5262fc204e74d0a` |
| `mcp-harness-unknown-original-reproduction-run.log` | `2fb76d67ef83aa3d7ac5c06244c349b592e22013208bf91f8553d1aea6af512d` |
| `mcp-harness-unknown-later-cancel-reproduction-run.log` | `dc43d2e3423f1f4ac138574a74af0ed3f7295e023eac46c01ee7e7803008bce5` |

## 4. 仍开放的义务

安装版 managed MCP、受管来源/程序、受保护输入/结果、真实 placement 和完整 IP 继续见[剩余义务审计](installed-product-remaining-obligations-audit-20261109.md)。本轮没有部署该修复、调用真实模型、刷新完整 Final/M5 或提升 IP01–IP24。普通 Shell 继承管道的清理仍只有源码定位，须另用有限子进程复现并闭合。

原本机升级 `maintenance:correction-scope-upgrade-20261109` 仍未结算。后续管理员只读诊断的启动回执为 `LAUNCH_REFUSED` / `InvalidOperationException`，没有实际诊断结果；不能据此断言精确根因、用户未确认或自动审批拒绝。本轮未重跑 RunAs、旧升级或原 Unknown，也未更改全机 UAC 策略。原生状态和受审提案保持开放，整体目标尚未完成。
