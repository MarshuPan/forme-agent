# 受管 MCP 发现动作的冻结类型补充

状态：2026-11-10 owner 明确回复“批准”，本项具体类型增量生效，按本文实施与验证。受管 MCP/插件/渠道、桌面授权及截图输出三项原方案的授权继续有效。本文件不是实现、部署或验收完成证明。

## 1. 已验证的缺口

已批准的[受管接入方案](installed-managed-consumers-closure.md)要求 metadata discovery 与调用均通过隔离 worker，禁止 Brain 进程内 fallback。源码 `protocol/src/primitives.rs::ActionParameters::Mcp` 的字段是 server、tool、arguments、schema_digest、transport、stdio、timeout，描述一次具体工具调用。`capabilities/src/mcp.rs::discover_detailed` 当前直接进入本地 stdio 发现，不能装进受管 Brain。

`WorkerExecutionSpec` 必须携带真实 ActionIntent，并按同一规范化摘要、affected_resources、job/permit/plan 校验。因此不能把发现伪装为一个并不存在的 MCP 工具、用 shell 参数编码 RPC，或在配置读取/启动期间直接创建进程。这里需要补充原方案中尚未明确的 ActionParameters 分支；不需要新增 crate、依赖边、BackendKind、EventKind 或 unsafe 边界。

本文件涉及根 AGENTS.md 第 3 条冻结对象，已作为独立差异取得 owner 确认；Windows UAC 和原生验证状态仍独立记录。

## 2. 唯一动作增量

在现有 ActionParameters 末尾 additive 增加 `McpDiscovery(ManagedMcpDiscoverySpec)`。现有 `Mcp { ... }` 的字段、序列化形状和规范化结果不变；不修改已有 WorkerJob/WorkerPermit/WorkerExecutionSpec 的字段或版本。

新增 closed/versioned 对象 `ManagedMcpDiscoverySpec`：

| 字段 | 类型与约束 |
| --- | --- |
| schema_version | 独立版本 1；未知字段/版本拒绝。 |
| server | McpServerRef；通过 validate_product_id，最多 128 UTF-8 bytes。 |
| program_ref | Host 已注册的 opaque String；同上，不允许路径或命令。 |
| registration_digest | SchemaDigest；完整 sha256，绑定 Host 受保护的程序注册版本、实际文件身份/摘要、固定 argv、资源范围、sandbox profile 与凭据引用。 |
| settings_digest | SchemaDigest；canonical_digest(("forme.managed-mcp.settings.v1", 完整 ProductMcpServerConfig))，绑定 scope、allowlist、enabled、trust、timeout、output_budget 等当前配置。 |
| timeout | DurationMs；1–60000，且不能大于当前受管配置和原 job/permit 剩余期限。 |
| output_budget_bytes | u64；1–1048576，且不能大于当前受管配置和 ExecutionSpec 预算。完整结果超限拒绝，不能截断成可用 catalog。 |

该分支没有 tool、arguments、stdio、shell、URL、任意环境变量或原始凭据字段。实际程序和参数只能由 Host 的注册原件解析。

`backend_hint/backend = Mcp`；`action_type = Execute`、`risk_hint = High`、`expected_effect = Outward`，始终走原 Harness 和 exact-plan 审批。发现会启动外部程序，不能因为只读取 metadata 就伪装为无外部作用的内部观察。其能力引用与普通 MCP 工具分开，不能用 tools allowlist 的通配或空集合替代发现授权。

受影响资源精确为排序去重后的 `mcp-discovery:{server}` 与 `program-registration:{program_ref}`。完整 spec 参加原 normalized_action_digest 与 PlanDigest；注册内容、settings 或请求预算的任何改变均使旧批准失效。

## 3. 授权与执行约束

1. 仅已认证 owner 的显式发现请求可提出该动作；配置保存、Web 打开、Brain 启动、普通 catalog 读取均不得触发。Agent 自行注册程序不在范围内。
2. Policy 和 registry 对 discovery 使用独立目标匹配；旧的 `McpTarget { server, tool }`、某个工具的允许项或仅 enabled=true 不能授权发现。审批仍绑定完整规范化动作、资源、policy/tool schema、nonce 与期限。
3. Host 在 dispatch 前核对实际注册原件和当前 settings 摘要、scope、启用/信任状态、已准入 profile、有限资源/凭据与完整 permit；运行中禁用/撤销继续阻止后续出口。任何漂移在创建子进程前拒绝。
4. 仅正式受管隔离 worker 接受该分支。普通进程内 McpBackend、开发 discovery 和 Remote placement 默认拒绝；不能因受管能力不可用而落回这些路径。没有真实 native binding 时保持 unavailable。
5. RPC 仅允许版本握手、初始化通知与 tools/list、resources/list，以及上述两种列表的有限分页；每类列表至多 8 页，cursor 至多 2048 bytes，重复 cursor 立即拒绝。禁止 tools/call、resources/read、任意 method 和主动外发。工具和资源各最多 128 项，重复/越界/无界 cursor 或预算超限拒绝。单次发现共用原 deadline 与总输出预算，不能通过分页重置。
6. 原 job 启动、取消、receipt 和恢复治理不变；未确认进程树结束或结果不明保持原 Unknown，只核实原 job，不换号、不自动重做发现。没有完整可信终态不得发布部分 metadata。

## 4. 结果与恢复接缝

通过现有 WorkerResultManifest/分块读取/Host 加密封存传回 UTF-8 metadata；外层继续绑定 installation、owner、job、permit、plan、workspace、期限、预算与原结果摘要。内层为 closed `ManagedMcpDiscoveryCatalogV1`，包含 `schema_version=1`、原完整 spec、tools、resources、catalog_digest。

工具项包含 schema_version、name、description、input_schema、schema_digest；资源项包含 schema_version、uri、name、description、mime_type。name/uri 最多 2048 bytes、description 最多 16384 bytes、mime_type 最多 256 bytes；name/uri/mime_type 不含控制字符，description 仅允许换行、回车、制表符这三种控制字符；工具名和 URI 各自唯一。input_schema 必须为有界 JSON object，单项最多 65536 bytes，完整 catalog 仍受原 1 MiB 上限约束。schema_digest 是规范化 input_schema 的摘要；catalog_digest 为 canonical_digest(("forme.managed-mcp.discovery.catalog.v1", spec, tools, resources))，数组保持返回顺序。

所有发现内容都是 UntrustedData；合法摘要只证明绑定和完整性，不提升语义信任。Core 独立校验原 worker binding、spec、完整正文摘要、catalog 摘要、项上限与当前配置后，才向原 registry 发布 allowlist 范围内的 metadata，并沿既有 McpDiscovered 事件记录发现事实。不得让 helper/Host 直接写脑事件。

持久 metadata 与原保护材料引用、注册/settings 摘要、owner/scope、原 completed job 一起保存。重启仅复用仍可验证的原件；缺失、删除 fence、来源/配置变化或没有 completed receipt 时保持 unavailable，不能在加载时暗中 rediscover。仅完成原件读取/登记的恢复不重新启动 MCP。调用工具继续使用原 Mcp 分支，并在最终执行前重查当前注册与 metadata schema。

## 5. 兼容及验收

- 旧 ActionParameters 及旧 WorkerJob/Permit/ExecutionSpec 字节/摘要回归保持；旧 reader 遇到新 variant 明确拒绝，不解释成其他动作。
- Protocol 测试覆盖 closed schema、限长/预算、摘要和资源稳定性、逐字段替换使旧 plan/permit 拒绝、Mcp 工具与 discovery 不能混淆。
- Policy/组件验证：无显式 owner 请求、无审批、旧 tool grant、错误/过期注册与配置、缺 sandbox、禁用/revoke、跨 scope 均在启动前拒绝；启动/读配置产生零 discovery。
- 自建 MCP 子进程经过正式 worker，分别验证分页、恶意 schema、输出超限/截断/错序、timeout、取消、后代存活、断连与 Unknown 零重派。独立证明无 Brain 身份/密钥/管理目录权限和未授权网络。
- 保存原有效 metadata 后真实重启，使用绑定原件而无第二次发现；变更来源/allowlist、删除原件、替换摘要和重放旧批准均拒绝。metadata 文本不能授予权限或改 Host 程序。
- 实施分协议、Host 注册/隔离执行、Core metadata 消费和 Web 入口联调；文档与代码分开提交，按现有整仓/永久门及原生 IP 要求取证。单个组件通过不代表受管 MCP 或三项方案整体完成。

## 6. 已确认的实施边界

上述唯一 additive 动作分支及其 closed spec/catalog 已批准，作为受管 MCP 隔离发现的承载协议。同范围不再等待确认；批准不授权新的依赖边、事件、unsafe 文件、进程内 fallback、任意 shell、自动发现或新增操作系统权限；Windows 原生升级仍需完成既有系统 UAC 步骤。
