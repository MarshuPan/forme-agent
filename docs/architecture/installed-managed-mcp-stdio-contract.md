# 受管 MCP stdio 隔离会话实施契约

本文件细化 owner 已批准的[受管消费者](proposals/installed-managed-consumers-closure.md)、[发现动作](proposals/installed-managed-mcp-discovery-action.md)和[程序注册](installed-managed-mcp-programs-contract.md)。不增加事件、crate、依赖边、unsafe 文件或 Worker 协议字段。它规定下一实施单元的行为，不是实现、部署或原生验收完成证明。

## 1. 程序和权限

首个 profile 为 `windows-appcontainer-mcp-stdio-v1`。Host 从当前注册原件取得 exe、固定 argv、实际文件身份与内容摘要；exe 自身成为 AppContainer 根进程，不由 Brain、shell 或通用 worker 再派生。保留低完整性、空网络 capabilities、OS 禁止子进程策略和原子 Job 加入。需要解释器、依赖目录、网络、外部资源或凭据的 server 在这一 profile 下不可用；后续 profile 的义务继续保留。

只授予该 exe 读/执行及独立空工作目录所需权限，不给原 workspace、sources.json、注册日志、产品配置、脑库、keys 或备份任何访问权。程序与祖先的 handle custody 持续到进程树结束、I/O 收束及 ACL 撤销完成。ACL 的前置原件、目标身份、readback 和恢复沿原 SandboxLeaseJournal；MCP 不走文件 overlay，也不写回原 workspace。

子进程只继承 stdin/stdout/stderr 三个明确 pipe 端点，使用 Windows `PROC_THREAD_ATTRIBUTE_HANDLE_LIST`；父端不继承。环境由 Host 从固定系统位置组装，仅保留 Windows 加载所需系统变量及隔离目录，不能继承 Host 的代理、模型 key、内部认证信息、任意环境变量或 worker 管理路径。Win32 调用仍只允许位于 canonical §29.9 已审计文件；纯 Rust buffer、状态、I/O 和生命周期协调置于独立安全模块。

## 2. 发现交换和总预算

发现只发送固定 `initialize`（protocolVersion=`2025-06-18`、空客户端能力）、`notifications/initialized`，以及 server 声明支持的 `tools/list`、`resources/list`。初始化必须确认同一版本、对象类型 capabilities 和 serverInfo；未知 metadata 只能被忽略或作为数据，不生成新请求。

请求 id 单调递增且精确匹配。server 主动 request/notification、错 id、重复回应、错误 JSON-RPC、error 回应、非法 UTF-8/JSON、未完成行、夹杂日志或额外帧均使整个发现失败。列表每类最多 8 页、128 项，cursor 最多 2048 bytes，拒绝空值、控制字符、重复 cursor 或第八页仍需后续页。工具名和资源 URI 唯一；逐项继续使用已批准 catalog 校验，不截断或返回部分 catalog。

整个会话使用原 spec/job/permit 共同约束的一次单调截止时间；初始化、分页、写入、读取和收束都消耗该窗口。stdout 与 stderr 的实际读取字节累计共用 output_budget_bytes，包含丢弃的 stderr 和最终帧后的尾部；单帧、队列、分页和最终序列化结果均有界。stderr 正文不进入日志或脑。只在完整 exchange、正常根进程退出、Job 树清空、stdout/stderr EOF 和 I/O 线程回收全部确认后封存结果。

## 3. 实际写入、撤销与取消

编码请求或将其放入队列不代表 egress 已完成。独立 I/O controller 在实际写线程中按 config → registry 的顺序，重查当前 owner/epoch、server 的 enabled/trust/scope/settings 摘要和原注册后才尝试写入。检查与本次实际写入受同一排他区间约束；禁用或撤销与该次写入形成明确先后顺序，后续写入必须重新检查。

controller 不持 worker 锁等待 RPC。获取配置锁使用可取消的 try_lock 循环，反复检查同一 cancel/deadline；不能在维护持 config 并等待 worker 取消时，反向无界等待 config。独立期限监督持原 Job 的复制句柄，不获取 config、worker 或 registry 锁；取消/超时终止原 Job，使阻塞 pipe 收束。系统调用成功不能代替实际树退出与 I/O join 证明。不能确认时保留原 controller、句柄、lease 和日志，阻止新准入并报告原 Unknown。

正常结束时显式关闭 stdin，等待 server 退出；不另发协议未规定的 shutdown，不续期等待。发生失败或取消后零后续 RPC。Host 崩溃恢复只接管并核实原命名 Job、sandbox lease 和原 worker 记录，不重新启动程序或重做发现。

## 4. 普通调用与结果来源

普通调用保持既有 `ActionParameters::Mcp` 和 `McpStdioSpec` 字段及版本。受管 stdio 的 command 必须是当前不可变 program_ref，args/env 必须为空；绝不把 command 当路径执行。真实程序/argv 只能来自同一注册 lease；registration digest 与 `mcp-program:<digest>` 的映射须精确校验。选择受管 profile 在最终 ExecutionPlan 生成前完成，Host 独立核对，禁止批准后改 permit/spec。

Host 必须核对当前 server/program/settings、原已完成 discovery job、受保护 catalog、精确工具 schema_digest 与 allowlist 后才允许 `tools/call`；Core 的 metadata 缓存不能代替该核对。调用使用相同固定握手与期限/预算规则，不支持 server 主动请求或任意 method。实际尝试写入 tools/call 前记录本次派发可能开始；部分写入也不能断言无副作用。缺可信结果或未确认清理时保持原 Unknown，禁止换号自动重试。

结果使用原 WorkerResultManifest、分块读取与 Host 加密封存，绑定原 job/permit/plan、installation/owner/workspace、期限、预算和正文摘要。发现内层使用已批准 ManagedMcpDiscoveryCatalogV1；Host 不发脑事件。Core 独立校验原完成回执、原件、当前配置/注册、catalog/schema 和删除 fence 后，才沿原 McpDiscovered 发布 allowlist 范围内的 UntrustedData metadata。

持久 metadata 必须绑定原受保护材料和 completed job；重启只复用可验证原件。原件缺失、配置/来源改变、删除 fence 或摘要不符保持 unavailable，零自动 rediscovery。配置 enabled 的消费者保护只有在整个正式执行和 metadata 链接通后才可开放，不能用注册成功或解析器测试替代。

## 5. 验证和边界

会话测试验证实际出站方法/顺序、分页、重复/错序/错误帧、恶意 schema、累计 stdout/stderr 与最终结果预算、原 deadline、失败后零后续请求。原生组件使用自建 exe，经正式 AppContainer adapter 独立验证 token/Job、三个 pipe、环境、文件/密钥/网络/派生拒绝、阻塞读写、取消、树退出、ACL 恢复、结果封存及 Unknown 零重派。

还需正式 Host/Core/Web 联调：owner 显式提出发现、原 Harness exact-plan 审批、禁用/撤销与实际写入竞争、原结果丢响应查询、真实重启零 rediscovery、删除和 schema 漂移拒绝。注册、解析或非安装身份的组件结果都不代替 IP01–IP24；插件、渠道、后续 profile、桌面和完整安装验收继续按原范围完成。

## 6. Core 原件消费的实现分工

本节细化已批准 §4 的结果保护与恢复，不改既有 ArtifactStore 截图接口、Worker wire 对象、脑 EventKind 或权限范围。

- WorkerActionBackend 可绑定一个 Core 拥有的窄 discovery consumer，实现内接口只传原 WorkerDispatchRequest、WorkerObservation、WorkerResultManifest 与完整有界 bytes。发现缺 consumer 时在计划/派发前拒绝；consumer 不启动程序，也不发布 registry metadata。
- consumer 先独立核对原请求、Completed 观察、manifest、正文与 catalog。保护材料用原 job/permit/manifest 派生的确定性地址，写入同一个 StoreContentProtection/lifecycle 服务；私有版本化 companion 绑定原请求/观察/manifest，另沿既有 material receipt 登记原文。正文、companion 和登记全部可重读后才能产生 ActionCompleted 的原件引用。
- 同一原件的重复保存只补全中断的保护材料；不创建新作业、不改原内容。观察时刻可以在原号查询时前进，不能改变终态和原 receipt/binding。body、metadata 或 material receipt 冲突立即拒绝。pending/完成删除 fence 使保存和读取均拒绝，不复活已删除来源。
- 完成事件仅返回有界摘要与受保护引用；目录正文仍为 UntrustedData。Core 的后续 registry 发布必须另外核对原 run 的 RunAccepted/SessionBound/ActionPlanned/Started/Completed 与当前配置/allowlist。单独存好材料不表示工具可用，不能提前放开 enabled 保护。
- 核验覆盖同原件重放/重启、三处写入中断、owner/原件/摘要替换、原件删除与部分 key 清理、consumer 缺失/失败/伪造引用时无可信完成，以及 completion append 失败不追加矛盾终态。真实安装身份与完整 Web 联合证据继续独立取得。

## 7. 受管 registry 的读取边界

本节细化已批准 §4 的 registry 接线，不改变原 McpRegistry/ExecutionCapabilityRechecker 签名、协议对象或事件类型。开发 stdio 行为保留；正式 managed composition 必须选择独立受管模式。受管模式缺 Core 原件来源即 unavailable，不能落回 discover_over_stdio，即使 provider 已 enabled/trusted/granted。

受管模式可以绑定一个 Core 拥有的窄目录来源；只返回当前 ProductMcpServerConfig 和已核验原事件链/保护材料的 ManagedMcpDiscoveryCatalogV1。来源的每次读取必须核对当前原件、注册/settings 与删除 fence，不能执行程序、自动发现、重派或发布脑事实。registry 另行核对 config/spec/catalog、provider grant scope、opaque program_ref、空 args/env、allowlist 和预算约束，再建立可丢弃的目录/延迟 schema 缓存。

discovery/search、schema 选择、准备调用、能力索引及执行重查都重新验证来源与已装载 catalog 摘要。来源缺失、变更、撤销或校验失败即移除该来源的缓存；显式重新装载只能读取仍有效的原件。受管目录读取不产生新的 McpDiscovered；该事实由完成原动作与完整校验的 Core 路径负责发布，重启读取不能伪造第二次发现。

受管普通调用仍使用既有 Mcp 参数形状，只携带 opaque program_ref 和空 args/env，并作为 High/Outward 进入原 Harness；不能因 metadata 被读取而降为内部无副作用操作。受管来源检查、缓存消费和 provider 生命周期变更在 registry 内形成排他顺序，Core 来源不可回调同一 registry。真实 pipe 写入仍由 Host 独立重查，缓存不能代替 Host 准入。

该边界可先于 Core 来源读取器和 owner/Web 接线完成；来源未接通时明确不可用，公开 enabled 保护继续保持。验证必须证明无来源/失败来源/缓存失效时零开发进程启动，并覆盖 allowlist、schema 漂移、禁用、重启读取和实际执行前重查；不能把受控来源测试当作 Core 或安装版完成。
