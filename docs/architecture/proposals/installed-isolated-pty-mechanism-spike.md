# 安装版隔离 PTY：Windows 机制验证范围

状态：机制组件已在 `1207740` 实施，真实终端、后代取消及原文件派生限制三个显式场景通过；高输出、阻塞输入和初始化/关闭故障矩阵仍开放，详见[本轮验收](../../acceptance/installed-isolated-pty-mechanism-acceptance-20261109.md)。本文不冻结生产 PTY 协议、不批准生产 profile 放开子进程，也不是 IP11/IP24 的通过证据。承接 PRD19 A5、architecture05 §4.4、PRD24 §4.1.2–4.1.3/§4.11、canonical §29.9–29.10，以及桌面结果提案 §8 的既有义务。

## 1. 要回答的唯一问题

能否把一个固定的、仓库自建的 PTY runner **先**放进逐 job 的 Low AppContainer 和 kill-on-close Job，完成 token/权限/Job 回读后才 resume，再由该受限 runner 使用精确锁定的 `portable-pty 0.9.0` 创建真实终端及固定子程序，并让终端相关进程始终保持所需隔离与树托管？

现有文件 adapter 的 child-process policy 固定禁止派生，不能直接运行这项验证；普通 `PtyBackend::spawn_command` 又不提供独立受限 token 或原子 Job 注入。验证的是 OS 继承路线，不能把启动后取得一个 child handle 当作首次指令前隔离的证明。ConPTY 是否在这条路径内可用、其相关进程是否继承边界，目前都没有正向证据。

## 2. 代码与权限范围

- 原生代码继续只在已有五文件白名单内。此验证只涉及两个既有 AppContainer adapter；不增加第六个 unsafe 文件、不使用 C/PInvoke/临时系统服务绕过白名单，不增加 crate、依赖边或脑事件。
- 生产 File profile、`WorkerLaunchProfileConfig`、permit/plan/grant 和 launch registry 保持原语义。实验入口仅存在于 `cfg(test)` 的显式 ignored 单元场景；不是 Cargo feature、产品配置、环境开关或可供服务调用的新 profile。
- 只为这个固定测试 runner 增加测试内部的“允许派生固定夹具”启动模式；正常测试与生产请求仍使用原 child restriction。不能把通用 `allow_children` 暴露给 Web、Core 或 WorkerJob。
- 继续复用原 `SECURITY_CAPABILITIES`、原子 `PROC_THREAD_ATTRIBUTE_JOB_LIST`、挂起创建、resume 前特权移除及精确 Low/token 回读。Job 不启用 breakaway 或 silent breakaway。
- 测试 observer 若需按 PID 独立检查后代，新增原生调用范围仅为 `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)`、`IsProcessInJob`、`GetProcessTimes`、`QueryFullProcessImageNameW`；token/Job 内容复用现有只读查询。持续持有查询句柄，绑定 creation time/image/job，拒绝 PID 复用或不可读状态；不对已运行后代执行特权调整。
- 上述四项只读调用和测试启动分支都留在 `windows_appcontainer_native.rs`，ACL/profile 继续归 `windows_appcontainer_acl.rs`。前者当前正好 600 行；新增前先把纯数据校验/生命周期编排移到 safe 模块，保持原有失败、清理及恢复测试。不得刷新规模基线、挤成难读长行或用 include 拼接。

本计划把测试入口与生产 profile 分开。实现须逐项确认观测调用属于现有 token/Job 最小封装职责；若需要改变冻结 DTO/trait、生产 profile、五文件白名单或现有原生职责以外的能力，先另提具体契约变更确认。现有生产文件隔离授权不能被解释成可随意放开该 profile 的派生能力。

## 3. 自建夹具与独立观察

使用 execution crate 内独立的测试二进制；基础 runner 使用既有 `portable-pty` 与必要的原协议类型。追加的 I/O 故障分支复用同 crate 已有 `PtyBackend`，用于证明普通 backend 与 Low AppContainer/Job 的组合行为，不另写一份看起来相同的生产 I/O 算法。两条分支均不调用桌面 driver、实际模型或真实配置。子程序由仓库自建，源文件、编译程序和依赖版本均记摘要，不复制上游 fixture。

测试根固定在当前工作树的证据区域中，以独占临时目录承载可写副本、空 profile、两份不可读 sentinel 目录和有界结果。不得访问已安装实例、真实脑目录、keys、浏览器会话或 owner 文件。测试外层保留原 Job/lease 资源所有权；普通权限不能完成时如实停止，不发起新的 RunAs。

夹具只接受固定 program、cwd、有限 cols/rows 和一份自建 Literal 输入；不接入尚待设计的 ContentRef/SecretRef 输入通道。环境从空集合构造，系统项按既有 OS 查询规则提供；父环境的验证标记只检查存在性，不输出其值。

外层独立 observer 使用 OS 句柄检查 root、PTY child 和可见终端相关进程的 image、创建时间、Job membership、AppContainer SID、完整性、capabilities 与剩余特权。子程序的 `IsTerminal`/往返结果用于验证终端机制，不能代替外层隔离观察。无法枚举或独立核验终端相关进程时，该维度为 UNVERIFIABLE，不能报整体 PASS。

## 4. 必跑矩阵

| 场景 | 必需证据 |
| --- | --- |
| 原文件模式反例 | 原 file worker 派生仍被 OS 拒绝；没有生产 profile 或配置变更。 |
| 真实终端 | 固定尺寸、stdin/stdout 为 terminal、有限输入往返、可信 root exit；不能以普通 pipe 进程替代。 |
| 树与 token | root 在 resume 前已入 Job 并通过回读；后代及 ConPTY 相关进程的真实边界分别记录。仅依据 helper 自报或 PID 存在不通过。 |
| 文件范围 | 仅本 job 副本可读写；两份自建 sibling/brain-like sentinel 不可读且原字节保持；原 workspace 无回写。 |
| 网络 | 只访问本次自建监听端；分别观察实际连接拒绝和 listener 零接纳。未测试的网络路径保持未验证，不以空 capabilities 代替运行证据。 |
| 环境 | 父 PATH、代理和自建凭据标记不继承；不打印真实环境值。 |
| 后代取消 | 自建 root/child/grandchild 在原 Job 内；取消后 root 退出、Job 为空、所有已观察后代终止，之后才清理。 |
| 超时及高输出 | 10 秒独立外层期限、有限队列/输出预算；不因持续输出或 ConPTY 关闭而永久等待。超时只终止原 Job，不启动替代进程。 |
| 初始化与退出故障 | 打不开 PTY、子程序启动失败、runner 中断、错误 exit、输出截断分别保留真实状态；无 shell/file/普通用户降级。 |
| 清理与再准入 | 原 Job 为空、查询句柄释放、profile/ACL 按原 lease 收回；不确定时保留证据与阻断，不删除现场后报成功。 |

任何 Started 后的终态或清理不明都保留原号，不重复启动该动作。独立单元场景不替代 Host worker journal 或 core ActionOutcomeUnknown；本验证不向脑写事件。

### 4.1 普通 backend 与隔离机制的组合验证

`451f3b4` 的八项普通 Windows I/O 场景已经通过，但它们没有运行在本设计的 Low AppContainer 中。追加固定 fixture 的八个封闭测试模式：输入不读超时、启动后取消、连续输出、严格预算、输出 append 拒绝、正常尾部、截断预算与错误 exit。它们只调用既有公开 backend 和进程内 EventSink，不创建 Harness/模型/脑写者，不改任何产品 DTO/trait 或生产派生限制。

子程序先报告 terminal/PID/边界并等待测试 gate；外层独立核对 runner/conhost/child 的原句柄、token 与 Job 后才打开 gate。取消还须等待程序进入输入测试正文。backend 的 3500 ms 期限包括这段观察时间，外层继续强制原 10 秒界限，不通过延长期限隐藏阻塞。输出拒绝在 gate 后才激活，保证与初始化前失败区分。所有结果同时记录事件种类、真实 outcome、有限输出状态及独立树终止/清理；正文只含自建内容，不包含真实输入或秘密。

fixture 的新固定模式及 case-result 是测试内部对象，不是新的 worker wire 协议。每个 case 创建新 experiment 并保留独立回执；失败停止后续 case，清理只处理原 Job/lease。组合通过仍不关闭 PTY 初始化/线程资源/OS 关闭异常的完整注入矩阵、生产 placement 或 IP10/IP11/IP24。

## 5. 执行和证据

先提交安全边界说明，再按上一节的适用性检查实现测试入口、夹具和原生只读观测；通过原生产负例与 fmt/check/workspace test/strict Clippy/质量/合规，之后显式执行 ignored 机制矩阵。固定复用本工作树 target，每个构建阶段仍检查 20 GiB 余量。

每次实际采样记录唯一 experiment/job、源码与程序摘要、Windows build、真实执行身份、原 profile/Job 绑定、每个进程的观察结果、控制前后计数、实际 exit/取消/清理及工件摘要。保留 PASS/FAIL/UNVERIFIABLE/NOT_RUN，且证据不包含 token、秘密正文或真实用户路径。失败只检查原 experiment，不自动重试。

## 6. 结论怎样进入生产设计

机制通过后，才据证据细化生产专用 PTY placement：真实 owner OS session、Host-issued grant、最终 plan/permit、逐 job overlay/回写、protected I/O、版本兼容和同号恢复仍需完整契约与确认；本验证不会自动启用能力或扩大原桌面提案的批准范围。

机制失败时保留原产物和具体失效层。不能通过降低 token、脱离 Job、使用 Brain/SYSTEM、去掉文件 worker 限制或假称普通 pipe 为 PTY 来取得通过。若确需另一组原生 API/审计边界，先给出新的具体变更依据。

正式 IP10/IP11/IP24 最终仍须由受管配置、真实 SCM Host、当前签名安装版本、真实用户 session/helper/grant 和 ProductionRuntime 联合证明。组件机制通过不关闭这些安装验收，也不改变 POSIX 当前不发布边界。
