# 具体记忆到数据生命周期组件验收

日期：2026-10-24。分支：`shu/installed-product`。设计 `9551a81`，实现 `91072be`；承接 prd/26 §5.6.2。此报告不提升 IP20/IP24 或原生安装状态。

## 1. 已验证行为

从当前记忆行核对 node/run/event、scope、snapshot 和真实 writer 登记后，只读解析 subject 与生命周期版本；不解密正文、不创建 key、不写事件、不调用模型。伪造节点、跨 owner/scope、陈旧列表、内部 companion、没有可信登记或已删除的材料均不能成为清理目标。二进制和超限正文可以定位其真实范围，但不会因此解密或展示正文。

Web 只将核对后的范围以内存带到现有数据管理区，不自动 mutation。版本改变时拒绝借用新目录版本，必须重新解析来源或明确重选目录。正式 PrepareLifecycle 预览与独立 Apply 保留口令、摘要、有效期和不可逆范围；同一 subject 可能含多条来源及派生项，不宣称只删除当前卡片。

正式 Web→签名桥→ProductionRuntime 集成验证：错密码零事件；预览后正文仍可读；替换 plan digest 拒绝；批准原计划才真实清理内容与密钥；同号重复返回原回执，换号重复批准冲突；原号 Inspect 可读回；清理后来源解析与下载拒绝。Harness 场景还验证真实 Retain 推进版本、旧版本拒绝和删除后的 SQLite 重启不复活。测试只操作仓库自建临时数据。

## 2. 当前源码验证

- 完整 `tools/verify-code-quality.ps1`：PASS；161 suites，871 Rust passed / 0 failed / 5 默认 ignored。strict Clippy、前端 58/58、bundle 9/9、打包输入/控制流 15/15、实际 Rust 验签 5/5、临时 NTFS 7/7 和完整默认合规通过。
- 其中 `installed_memory_material` 6/6、正式工作台命名集成场景 1/1；不重复累计到全量数字。
- `tools/verify-core-brain-workspace-contract.ps1`：PASS，18 crates、99 EventKinds 与 97-kind strict prefix 不变。
- 593 个受检源文件、63 处既有规模债务未增长，未刷新 baseline。统一门运行期间源码固定，仅补充后续设计文档。

本机证据：`target/installed-product-evidence/memory-lifecycle-20261024/`。

| 日志 | SHA-256 |
|---|---|
| code-quality.log | `ECB321EDE58F1E8FBA1020EFCB581711C984690C6A5487FE6C18546EAE345494` |
| frozen-contract.log | `79742EC771406A7C670DF8A4750C73F6CA7847CAC435331487500F73F22D1D4D` |

## 3. 证据边界与继续义务

没有新增真实浏览器跨页/双 tab 证据，没有使用真实 owner/key/provider，没有注册系统服务、合入 main 或推送。正式 HTTP 与前端纯行为回归不能替代浏览器或完整安装产品验收。

文字纠正仍未实现，约束见 prd/26 §5.6.3；撤回、来源读取和本组件的真实遗忘不等同于文字纠正。继续按整体交付清单实现，不在本组件结束整体里程碑。用户批准的 `target/verify-tests-20261023` 在本轮开始时已不存在，本轮未删除该目录或其他构建/个人数据。
