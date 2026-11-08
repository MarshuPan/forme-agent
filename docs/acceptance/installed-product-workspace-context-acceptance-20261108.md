# 已安装产品的项目监管与会话工作空间验收（2026-11-08）

本记录承接 prd/26 的会话上下文、项目监管、幂等和并发义务。实际入口为 `http://127.0.0.1:18765/`，实际安装版本为 `forme-windows-x86_64-20261107-cf3fbc9`。这是该安装版本的部分 IP17/IP19 证据，不是新候选已部署、文件 worker 成功或完整 IP24 通过。

## 1. 范围与合并回执

本轮只新增隔离测试 workspace、项目、空会话及一次真实对话。测试项目最终暂停后续监管；未执行已规划路线，未重新派发任何历史 Unknown。登录密码、会话 cookie 和模型密钥没有写入证据。

工件根为当前工作树的 `target/native-legacy-review-20261106/`。合并回执为 `installed-project-context-live-receipt-20261108.json`，SHA-256：`619365f101c1dbd0c64f6808548f8f662a71ffd953411fd1484df6ba78c1a59c`。其中封存 12 份 JSON/截图的摘要，历史三个 run 的事件数量、顺序摘要和终态保持。

## 2. 项目监管与并发

测试 workspace 为 `workspace:installed-project-20261108`，charter 明确 `read-only`、`require-review`、`max-nodes:1`，并禁止 File/Shell/Browser/Computer。通过正式 prepare/apply API 提交版本 1，同命令重放返回原事件。

浏览器登记 `project:installed-project-20261108`，随后暂停、恢复；正式 API 独立回读监管版本 1、2。原恢复命令重放不新增 ledger revision；同号异义与新号陈旧版本均返回 409。两个实际并发 HTTP 请求竞争版本 2，只有一个返回 200，另一个返回 409；ledger revision 只从 4 增到 5，最终监管版本 3、enabled=false、review=Paused，浏览器刷新后显示一致。

登记至首次暂停之间，正式后台调度生成一条路线：`project-route:sha256:04f0b11cf94cd93b3258f487a77ba98dab677e1920ac48519d51b7f7f57e8ee7`。路线开放，唯一节点 Pending、attempts/reserved/consumed/spent 均为 0，结果为空、验证为 `Unverifiable("not executed")`。暂停不取消这条既有路线；本轮没有点击执行或取消，不声称整个登记窗口从未发生规划。

证据：`installed-project-scope-preparation-20261108.json`、`installed-project-paused-20261108.json`、`installed-project-resumed-20261108.json`、`installed-project-control-cas-20261108.json`、`installed-project-final-20261108.json`，以及 `browser-live-20261108/installed-project-final.png`。

## 3. 会话绑定进入真实主脑

正式 API 创建空会话，同号重放仍是原记录，没有 run。浏览器选择该会话，将“后续运行工作空间”绑定到上述 workspace，再发送一次固定标记请求。

| 观察 | 实际结果 |
|---|---|
| 原会话 | `session:sha256:7029888fa00f93545328b5712b2f663d548db5a4f98c412ba8e9336911596898`。 |
| 唯一新增 run | `run:1794149306623626000:2`，Complete，19 个连续且不重复的事件。 |
| `SessionBound` | workspace 为测试空间，policy 为 `policy:default-deny`，含正式 production composition digest。 |
| Context | 完成事件包含测试 charter 引用、`brain:cognition-summary`、`brain:identity`、`brain:workspace-state`。 |
| 实际 provider | `ModelCallStarted/Finished` 的同一 call id、同一 `compatible:gpt-6.1-sol`；不能把绑定配置名 `primary` 当作实际 provider。 |
| 模型与工具 | 一次模型调用，631 input / 13 output tokens；ToolCallProposed=0、ActionStarted=0。 |
| 回答 | `FORME_PROJECT_CONTEXT_20261108_DONE`，随后为正式 `RunComplete`。 |
| 刷新 | readiness probe 后重新选择原会话，workspace、原回答、事件进度 19 均恢复。不是刷新后自动保留选中状态的声明。 |
| 原有运行 | 原三个 run 的事件与终态保持，没有用新请求替代原 Unknown。 |

证据：`installed-workspace-context-before-20261108.json`、`installed-workspace-context-after-20261108.json`，截图 `browser-live-20261108/installed-workspace-context-complete.png` 与 `installed-workspace-context-reloaded.png`。刷新并重新选择会话后的独立 API 回执为 `installed-workspace-context-reloaded-20261108.json`，SHA-256 `1b5b7ab6010ba616aec00802ed1d5bbd44aa1c02a77d2e5dd582621a6744feb1`：总 run 仍为 4，该会话仅原 run，19 个原事件的 ID/顺序、workspace、一次实际模型调用和零动作均保持。

## 4. 真实只读与认证拒绝

此前同批实际安装只读检查的最终回执 `installed-readonly-auth-and-evidence-final-20261108.json` 为 18 checks / 0 failed：匿名读取 401、真正不携带 CSRF 的请求 403、错误 Origin 403；原三个 run 与记忆/认知/Inbox 快照保持。

保留三份失败检查，不能改写成通过：最早 runtime digest 受 PowerShell 无序字典序列化影响；后一次直接比较带 `observed_at` 的动态项目状态；首次 missing-CSRF 检查的 WebRequestSession 仍缓存原 header。修正检查使用明确字段、稳定快照及无缓存 header 的独立 session，范围和限制均写入最终回执。

## 5. 路线状态展示修正

旧安装将所有 `WorkRouteStatus::Running` 都显示为“执行中”，与上述节点尚未开始的真实证据不符。实现提交 `7863920` 只修正前端投影文案：有实际 Running 节点才显示执行中；Waiting 显示等待中；未开始且预算计数均为零显示“已规划，待执行”；缺少节点证据只显示路线开放；未知结果和矛盾关闭状态保留待核实。

不新增 wire 状态，不改变 route/node 状态机、命令、审批或执行按钮的准入条件。缺失节点 status 不再被前端补成 Pending。定向 5 项回归及 TypeScript 检查通过；实际安装路线的独立 JSON 输入也在当前源码投影中得到“已规划，待执行”。新文案尚未部署，不能拿当前源码结果代替旧安装截图。

完整源码质量门已 exit 0，日志为 `route-status-quality-20261108.log`：Rust 183 个测试目标、1304 passed / 0 failed / 16 默认 ignored；strict Clippy、fmt/check/build、前端 118/118、资源可重复生成、依赖审计、bundle/签名/PE、NTFS、防火墙只读策略、实际 Inno 退出夹具 2/2、完整默认合规和 whitespace 全部通过。结构门为 881 files / 63 份既有规模债务，未刷新基线。本轮没有再次运行完整 `verify-m5.ps1` 或生成新源码发布审计，不把源码质量门提升为最终 M5/产品验收。

## 6. 尚未闭合

本轮提权启动被 Windows 取消，管理员进程与原生升级回执均未产生；没有入库或执行签名升级。当前源码改动后的候选和精确源门必须重新绑定，不运行绑定旧源码的验收脚本。

真实文件矩阵、数据/维护/升级联合路径、双 tab/多进程/故障恢复、LAN 跨设备及桌面授权等义务仍分别开放；已有待确认协议草案不算已批准实现。Linux/macOS 原生与正式签名发布边界保持，`release_ready=false`。本报告不关闭整体目标。
