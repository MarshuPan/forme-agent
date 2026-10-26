# 安装运行时剩余承重契约草案

状态：**owner 已批准，结果通道已激活；POSIX 恢复选择 A**。本稿记录现存两项承重决策及其实现边界；不修改既有脑事件、crate 图或执行授权语义，也不宣称原生平台/IP 通过。结果通道的协议、Host 封存、AgentExecution 只读路由和开发 Process worker 已实现并接受定向测试；正式 ProductionRuntime 接入、原生平台隔离、安装包和完整 IP 仍按总清单独立验收。

## 1. 已核实的缺口

现有 `WorkerBrokerTransport` 和 `IsolatedExecutorHost` 只有 dispatch/probe/cancel。`WorkerObservation.safe_result_ref` 仅是引用，没有取得正文、校验实际文件变更和构造既有 `ActionResult` 的正式通道。Windows/Linux/macOS adapter 的退出回执不能单独证明任务达到 DoneContract；ProductionRuntime 仍未接入完整的安装执行链。

POSIX 恢复也不能沿用 Windows 的支持结论。当前 systemd 模板提供固定退避与启动次数保护，launchd 模板不启用无界 KeepAlive；均未证明 canonical §29.6 的有限 `5s → 30s → 120s`、24 小时重置语义。

## 2. 决策一：有界、原操作绑定的 worker 结果通道

本节已批准为 additive 产品契约。保持现有 crate 图、99 EventKinds、原 ActionResult、原 policy/approval 和既有 dispatch/probe/cancel 语义。结果读取是独立的只读接口；旧 transport 调用保持兼容，只有实现了结果通道并通过对应 Host/平台门的 backend 才能声明结果可用。

### 2.1 归属与身份

- protocol 定义 closed/schema-versioned DTO；execution 拥有 Host 结果存储、transport 与现有 ActionBackend 的安装适配；harness 仅通过现有 execution 边界消费。
- Windows 复用当前 Brain Service SID/image/受保护 named pipe 和独立 bearer；POSIX 必须先验证 core 的 OS peer 身份，不能把 loopback 来源或 bearer 本身当作账户隔离。
- 只有已认证 core 可读；Web/worker/OwnerMaintenance 不能直接调用该接口。正文经 core 按原授权输出/产物路径提供给 Web。
- 读取绑定历史 job 的 owner、action、scope、plan digest、permit 引用/摘要、authority epoch、nonce 与最终 receipt；请求字段只能用于匹配 Host 已有记录，不能创造身份或权限。历史 permit 到期不阻止查原结果，但绝不允许再次执行。

### 2.2 对象与边界

| 对象 | 必需内容与语义 |
|---|---|
| WorkerResultBinding v1 | install/job/action、owner/scope、plan/normalized-action digest、permit ref/digest、authority epoch、nonce、receipt ref。由已接纳记录与最终执行规格产生。 |
| WorkerResultManifest v1 | binding、结果 ref、实际 outcome、正文总字节数/摘要、encoding、截断标志、产生时间、Host 核实的变更/证据摘要、正文可用状态。不可由 worker 自报验证成功。 |
| WorkerResultRead v1 | binding、manifest digest、offset、max_bytes。只使用 opaque refs，不接收文件路径、URL、环境或程序名。 |
| WorkerResultChunk v1 | 原 manifest digest、offset、实际 bytes/摘要、next offset 和 finished；所有范围检查使用实际落盘长度。 |

正文总预算不超过最终 WorkerExecutionSpec 的 output_budget_bytes，且不超过既有 64 MiB 上限；单次读取最多 256 KiB。JSON/base64 后仍受既有 wire 上限约束；不能只限制解码后大小。正文按字节重组、整体摘要核验后再解码，跨 chunk 的 UTF-8 不得错误截断。

无结果、尚未终结、已清理和数据不一致是不同状态。结果缺失/摘要错误/绑定错误均不能合成空的成功结果；如果动作已开始而无法证明终态，维持原 Unknown。只能核实原结果或等待 owner 裁决。

### 2.3 发布、恢复与数据保护

1. 隔离 worker 只向本 job 的有限输出通道写数据；没有 Host/brain key，不能写 Host manifest、receipt 或 journal。读取与输出都要同时限制内存、磁盘、时间和句柄。
2. Host 校验结果结构、预算、执行绑定与实际退出。文件动作另核对完整变更集、baseline 与回写 receipt，不能把 exit 0 当作文件已提交。
3. 输出由 Host 封存为不可变、受安装数据目录权限保护的结果工件；journal 只留摘要/引用。结果工件的应用层加密必须接入产品执行数据域的真实密钥解析后才能宣称完成，本轮不以自制密码学或 AgentExecution bearer digest 冒充加密。不能把原文复制到日志、管理回执或公共配置。临时输出与 overlay 的原生权限和清理须分别验证。
4. 先发布内容并同步，再提交绑定 manifest/终态 receipt。发布中断只能读取已完成的原工件；孤立工件不成为成功事实，终态指向缺失工件时继续拒绝成功。恢复不重新启动 worker、不重复回写。
5. 保留/清理有独立产品数据记录和读取 fence；显式清理后结果显示不可用，不把缺失正文解释为成功，也不通过备份恢复重开旧执行权限。原审计元数据不包含正文。

### 2.4 ProductionRuntime 接入

正式服务装配从受保护安装配置构造既有 backend 的 installed adapter。没有可信 Host、匹配 launch profile、真实隔离或结果读取能力时，该 backend 不可执行，不能退回 brain 账户的本地 shell/file。

Harness 继续负责最终计划、policy/envelope/能力门、审批和 nonce。adapter 使用确定绑定的原 job/permit dispatch 一次，随后只读 probe/read；截止、取消或崩溃不换 job。Host 验证结果后，core 按既有 ActionResult/事件路径记录实际 Completed/Failed/Cancelled/Unknown、输出和文件证据，Verifier 再判定任务是否满足 DoneContract。worker 自报的 confidence、success、rollback 或任意证据引用都不是授权或验证结论。

文件回滚不能从 worker 接收任意原文/任意路径。Host 若支持补偿，须保留原 baseline 并形成受原治理约束的新动作；不改变既有 rollback 真实性要求。尚不能提供可靠 diff/补偿语义的 backend 保持不可用，而不是返回伪造附件。

### 2.5 实施顺序与拒绝证据

1. 协议、manifest 校验与独立只读接口；保持旧 wire/trait 的正反兼容。**已完成。**
2. Host 私有工件、分块、原 job receipt 与中断恢复；重复读取零 driver/零回写。**本轮已完成 bounded/private artifact；应用层加密仍以 product key-domain 接入门为准。**
3. worker 的真实有限执行入口和三个 OS adapter 的结果封存；单平台先证明，再声明该平台可用。
4. 正式 backend/ProductionRuntime 装配与 core→peer→worker→原结果→Verifier 的完整路径。
5. 产品浏览器与原生 IP10/IP17/IP18/IP24；真实 provider、实际服务身份和临时/模拟证据分开记录。

## 2.6 已批准的正式接入边界

结果通道启用后，下一步不是让 core 直接调用 Host API，而是新增 `WorkerActionBackend` 这一执行 crate 内 adapter，并在受管 composition 中显式装配。它接收安装侧的 `ManagedWorkerBinding`：endpoint、credential 文件、authority epoch 和 sandbox profile；Windows managed 只允许安装派生 named pipe，当前 POSIX/开发路径只允许有界 loopback TCP。binding 缺失或校验失败时，相关 capability 必须保持 unavailable，不能悄悄继续使用 brain 账户的 File/Shell/MCP backend。

adapter 的唯一副作用提交链为：`ExecutionPlan -> WorkerJob/WorkerPermit/WorkerExecutionSpec -> WorkerBrokerTransport::dispatch`。dispatch 返回后只对同一 job 进行 bounded `probe`、`cancel` 和 `result_manifest/read_result`；job id、plan digest、authority epoch、nonce 和 permit digest 全程固定。dispatch/结果读取不明、终态与 manifest 不一致、`Completed` 没有可验证正文或 Host binding 失败，都返回既有 `ActionOutcomeUnknown`，不创建替代 job。可验证的 worker outcome 再按既有 `ActionResult`/Verifier 路径进入脑事件。

该 adapter 首轮只承诺通用 worker protocol 接缝和可被 Host 证明的 backend；文件变更 diff/overlay 回写、桌面 helper binding、真实 Windows SCM/ACL 和 Linux/macOS 原生安装仍保持各自的独立验收门。开发进程 worker 的测试只证明 wire/result 协议，不提高产品发布状态。

必须覆盖跨 owner/job/scope、permit/plan/nonce 替换、过期执行许可、维护凭据、结果篡改、越界 offset、超预算、linked path、磁盘满、工件/receipt 两侧崩溃、并发读/清理、取消/Host 死亡和恢复。拒绝路径证明零新增执行；已开始但不明的路径证明原 Unknown 且不重派。

## 3. 决策二：POSIX 的发布边界或恢复契约变更

| 选择 | 实施影响 |
|---|---|
| A：保持当前契约与不发布边界（已选择） | 继续要求原有限恢复策略；Linux/macOS 的 `release_ready=false`/IP NOT_RUN 保留。可推进不依赖该决策的实现，但不宣布三平台产品完成。无需放宽任何治理。 |
| B：先定义 one-shot 启动门，再逐平台实现 | OS 仍是唯一重启管理者。每次 OS 启动只允许一次有界、持久化准入；门可拒绝启动或报告冷却，绝不在进程内做重启 loop。必须先确认下列细节与实际平台可表达性，不能仅修改模板。 |

B 需在写代码前明确：失败计数/24 小时重置的唯一持久 writer；有意停服与崩溃的区分；OS 重启、时钟回拨、torn write 和并发启动处理；systemd 的 RestartPreventExitStatus 与 launchd 的 SuccessfulExit/手动启动差异；连续失败阈值后的确切退出码和人工恢复动作。退避必须说明是从真实失败时间还是启动观察时间计算。

若采用单次启动 wrapper，须只启动子进程一次、等待并记录该次退出，不做内部重启；同时先更新 Host/core 的实际 service PID/image 绑定，防止 wrapper 存在时把普通子进程当成 OS 已认证服务。不能为了统一表面行为绕过固定服务身份、权限或 Unknown 恢复。

没有 macOS 原生证据时，不能从 systemd 或 Windows 测试推出 launchd 的等价性；签名、公证、账户、TCC 与无桌面启动仍独立验收。B 是需要进一步冻结细节的方案方向，不是“改成无限 KeepAlive”。

## 4. 明确未完成的部分

本稿不增加第四内核、M6、第二大脑或新的脑事件。桌面 helper 的真实 OS 会话/隐私授权、管理员 SCM 安装、各目标原生构建签名、真实 provider、结果工件应用层密钥接入及完整 IP01–IP24 仍按总交付清单开放。

浏览器组件截图、源码质量门及已有 AppContainer 临时目录证据不能替代上述批准、实现或原生验收。两项决策只阻断依赖它们的工作。
