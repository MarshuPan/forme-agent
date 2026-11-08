# 安装版桌面结果工件接线草案

状态：可审查的冻结契约变更提案，尚未批准、回填或实施。承接 PRD19 A2/A4、PRD24 §4.1.1a/§4.11、PRD26 §4.4 与 canonical §29.9–29.10。它只补齐既有截图的安装版结果链，不新增内核、crate、依赖边、脑事件或 unsafe 文件。它依赖桌面授权提案的正式 Host grant，不代替该提案。当前原生升级仍沿原进程等待 OS 确认，本提案不改变现场。

独立先行修复：原始摘要/长度回执已在 `85afa70` 修复并验证；§1 保留接线审计时的问题描述。该修复不构成本文其余协议的批准，原始工件草案也保持原件。

## 1. 已定位的实际接缝

| 环节 | 当前源码 | 必须改变的行为 |
| --- | --- | --- |
| helper | `windows_desktop_helper_runtime.rs::execute_action` 拒绝 Screenshot；`RejectingArtifactStore` 没有结果消费者。 | 原 Screenshot 进入绑定原 grant/surface/bounds 的真实 driver；产生的字节必须交给原 job 的 Host。 |
| adapter | `DesktopHelperAdapter::execute` 只返回 `DesktopActionReceipt`。 | 新增显式结果交接接口；不能把正文放进隐含全局缓存或伪造 result_ref。 |
| Host | `worker_host.rs` 桌面分支以 `source=None` 封存。 | 接收并验证原字节，按既有顺序封存、追加 manifest、清理和追加 receipt。 |
| core | `WorkerActionBackend::read_result` 完整校验后无条件要求 UTF-8。 | 按真实 encoding 和原 action 解释；截图成为保护工件，文本保持原规则。 |
| 产品 | 下载与附件目录主要按上传会话 scope 取原件。 | 工具产物必须从原 run/session/workspace 的可信关系授权，不能因为持有一个 ref 就允许读取。 |
| 逻辑摘要 | `ProtectedRuntimeArtifacts` 返回加密文件的 digest/bytes。 | 返回原始内容的 digest/bytes；底层密文地址保持内部用途。该项按既有 ArtifactStore 契约修复，不等待本提案。 |

这里的原生截图正向证据尚未取得。类型中已有 Binary、一个有效 ref 或一次组件测试不能代替整条链。

## 2. 版本与兼容边界

1. 现有 desktop schema 1 的 DTO、握手、控制帧和 journal 原件保持可读且不重算摘要。WorkerJob、WorkerPermit、WorkerResultBinding/Manifest/Read/Chunk v1 继续复用。
2. 增加独立的 desktop exchange schema 2 和固定启动模式；不全局提升 `DESKTOP_SESSION_SCHEMA_VERSION`，不把可选新字段塞进旧 closed 消息。
3. Host 只启动当前受信活动发布中的固定 helper。发布材料声明的协议集必须与实际握手一致；旧 helper 不支持 v2 时，在 `Started` 前拒绝截图，不把同一请求降级到 v1。新 helper 与旧 Host 的不兼容同样在接纳前停止。
4. v2 协议仅增加 Screenshot 的结果交换。现有有限 Computer 动作沿原语义；Content/Secret 输入和真实 PTY 不借此启用。

## 3. 原 job 绑定和有界交换

所有新 wire 对象闭合字段并带独立 SchemaVersion(2)。新协议对象归 forme-protocol；受保护正文仅存在于执行数据通道，不进入 Debug、普通日志、Web 请求、产品配置或脑事件。

`DesktopExchangeBinding` 的字段固定为：schema_version、installation_ref、owner_ref、job_id、permit_ref、permit_digest、action_ref、workspace_scope、plan_digest、normalized_action_digest、authority_epoch、nonce、grant_ref、os_session_ref、session_generation、deadline。每个值都从已核验的原 request/job/permit/grant 派生，不接受 helper 更换。

| 对象 | 字段与语义 |
| --- | --- |
| `DesktopOutputHeader` | schema_version、binding、transfer_ref、media_type、bytes、content_digest、width、height。media_type 本轮只接受 image/png；尺寸须精确对应原 bounds。transfer_ref 为 Host 签发的原 job 内 opaque ref，不是文件路径或公开下载凭证。 |
| `DesktopOutputChunkHeader` | schema_version、binding_digest、transfer_ref、offset、bytes、chunk_digest、next_offset、finished。随后紧跟该帧精确长度的原始字节，不使用 JSON 数字数组、base64 或日志字符串承载正文。 |
| `DesktopOutputFinish` | schema_version、binding_digest、transfer_ref、header_digest、bytes、content_digest、原 DesktopActionReceipt。它是 helper 声称的终态，仍需 Host 独立核验。 |
| `DesktopOutputDescriptor` | schema_version、完整 binding、WorkerResultManifest.manifest_digest、原始 content_digest/bytes、image/png、width/height。由 Host 独立解析正文后写入原 job journal；不是 helper 可信输入。 |

上限和检查固定为：

- 控制帧继续不超过 256 KiB；新 header 编码不超过 8 KiB；每个原始正文 chunk 不超过 32 KiB。
- 正文总量不超过 `min(原最终 output_budget_bytes, 64 MiB)`；最多 2048 个非空 chunk，offset 连续且严格前进，唯一 finish 恰在声明总长处。零字节截图拒绝。
- 交换、PNG 校验与 Host 封存均不延长原 deadline。core 原结果读取仍使用既有 `min(plan timeout, 30s)` 和最多 4096 次读取限制。
- PNG 的编码签名、类型、实际解码尺寸和 bounds 一起核验；像素总量不超过 16,777,216，解码工作内存不超过 128 MiB。Host 在动作前据 bounds 检查像素上限，之后再核对真实正文。未知格式、额外结果、截断、解码炸弹、错尺寸和尾部附加内容均拒绝。
- 完整 transfer 的 binding、header_digest、chunk digests 与整体原始摘要分别验证；不能以重新计算某一个摘要掩盖换 job、换 plan 或错顺序。
- 同一 job 至多一个正在组装的结果；Host 的并发配额在开始接收前预留并持续占用，直至封存或明确失败清理。满额不能创建失控后台接收任务。

raw bytes 使用专用的长度限定读写接口，不能为正文类型派生通用 Debug/日志序列化。原生 named-pipe peer、完整双向身份和 identification-only SQOS 不变；本轮不新建任意 loopback 或文件传输入口。

## 4. adapter 与 Host 的显式交接

保留 `DesktopHelperAdapter::execute` 原签名；增加 `execute_with_output(request, intent, output_sink)`，返回原 DesktopActionReceipt。新增方法的默认实现对 Screenshot 明确拒绝，防止旧 adapter 自报支持。

`DesktopOutputSink` 是 Host 持有的窄接口：`begin(header)`、`write_chunk(header, bytes)`、`finish(finish)`。接口不向 helper 暴露路径、文件句柄、key 或 Host journal；每步失败都停止后续动作。所有实现只能接受一个精确 request 和一份有界结果。

Host 为该 job 建立私有、原子创建、禁止链接/替换的暂存目标，收到完整数据前不发 WorkerReceipt。原始顺序固定为：

1. 原 job/permit/plan/grant/session/peer/replay 检查并认领；在发送动作前固定唯一 transfer_ref 和预算。
2. helper 接收 exact Screenshot，报告 Started 后只执行一次 capture。调用原 NativeComputerDriver；工件 sink 接收原 PNG，不自行再次截图。
3. Host 接收完整原件，验证所有绑定、长度、摘要、PNG 和 session generation；收到 finish 不等于动作完成。
4. 使用原 WorkerResultStore 封存 Host 私有 AEAD 工件；原 manifest 为 Available/Binary，摘要和 bytes 都针对原始 PNG。
5. journal 追加原 manifest 和 Host-authored DesktopOutputDescriptor，再执行 `result_sealed` 交接清理；清理成功后追加原 WorkerReceipt。
6. 最后向 core 返回原 observation。helper 自报的 result_ref 不能替代 Host 生成的工件身份或完成这次事务。

预分配暂存元数据属于 Host 原 job 事务；Host 收不到完整结果时不得用 source=None 生成成功结果。临时正文不直接成为脑事实，不进入 workspace overlay 回写，也不赋予文件 worker 读取桌面的权利。

## 5. core 的原件投影

1. `read_result` 保留全部现有 job/permit/manifest/receipt/budget/chunk/digest 校验，改为返回经过验证的 bytes 与真实 encoding。不是简单删掉 UTF-8 断言。
2. 现有文本动作仍要求 Utf8；其原错误 UTF-8 负例继续失败。Binary 只在原 action 为 Screenshot、Host descriptor 完全匹配且 PNG 独立校验通过时接纳；不猜测或转码未知二进制。
3. WorkerActionBackend 的受管 factory 显式注入既有 ArtifactStore 抽象，由 Harness 提供现有 ProtectedRuntimeArtifacts；execution 不依赖 harness/store。没有该消费者时在新截图 dispatch 前保持 unavailable。
4. 原件写入 core 受保护工件域，并保留服务器产生的材料回执。ExternalActionReceipt 的 content_ref/digest 指向这份可解密的原始 PNG；摘要不引用 Host 密文或 core 密文。
5. `ActionOutputDelta` 仅输出受限 Untrusted 观察摘要和安全原件引用；像素、base64 和 transport payload 不进 EventStore。完成使用既有 ActionCompleted，不增加事件。
6. core 保护写入与材料登记完成之前不得提交完成事件。失败保留原 job Unknown，只读原 manifest 重取和完成本地投影，不重新 dispatch。保护对象的对象身份从原 job/manifest/receipt 推导，使同号恢复不因随机对象 nonce 创建重复逻辑产物；底层 AEAD nonce 仍唯一。
7. 重取原结果先核对已有材料登记与原始摘要/长度；一致则复用，不一致拒绝。不能修改或“修复”已经落盘的历史 ActionCompleted/receipt。

本项要先回填 factory 的精确构造接缝和幂等材料登记方法，再实现；不能在 trait 外藏一个私有内容数据库。原影像与材料 metadata 仍沿现有内容保护/单写者/删除 fence 管理，不成为另一套 run 状态。

## 6. Web 与模型的消费者边界

- 产物目录从原权威 run/session/workspace、ActionCompleted 与材料回执解析可下载对象；session 由可信 SessionBound 关系给出。worker、helper 和 Web payload 不能自报归属。原上传附件目录和会话绑定行为保持。
- `ArtifactCatalogQuery/View` 的 v1 公共字段与上传的 4 MiB 限制保持不变。工具工件目录采用独立 v2 查询/响应，明确原 run、媒体类型、尺寸和原始 bytes/digest；不借截图扩大普通上传或控制请求上限。
- v2 下载为认证后的有界分块读取：绑定 owner、session、run、content_ref、原始 digest、offset 和 max_bytes；单次最多 256 KiB，总量受原 manifest 限制。请求不包含磁盘路径或 Host result_ref。每次读取重新核对 session/workspace 归属、data fence、当前内容及删除状态。
- 公共 API 只经 Web → 正式 core 的 typed owner adapter；不开放 Host 工件 URL。响应使用正确 image/png、nosniff、no-store；前端只在认证内存中组装受限 Blob URL，关闭、登出、失效或数据删除立即 revoke，不写 localStorage/IndexedDB。
- UI 对完成前的产物显示核验中；拒绝或 Unknown 不渲染部分图片。对完整原件重算长度/摘要后才提供预览；错 session/owner/ref 的读取无原始内容回显。
- 当前模型内容路径对二进制只提供引用和状态，本提案不伪称模型已经看过图片，也不自动把像素发送给 provider。完成截图动作与完成依赖像素理解的任务分开验收；后者只有实际视觉消费者与任务证据成立才可通过。

## 7. 崩溃、取消与删除

| 切点 | 原号处理与必须保留的事实 |
| --- | --- |
| Started 前版本、预算或授权拒绝 | driver 调用为零；原请求明确拒绝，不新建替代 grant/job。 |
| Started 后缺 header/chunk/finish | 原 job Unknown；不再次截图，不用 Empty 或 timeout 推断 Completed。 |
| 原件完整但尚未封存 | 保留原暂存与原 job；恢复只检查原件/绑定。不能从目录存在推断终态。 |
| manifest 后、descriptor/清理/receipt 前 | 原终态未证实，继续 Unknown；只沿原封存/交接恢复，不另造一份 worker receipt。 |
| Host receipt 后、core 内容投影前 | 重取原 receipt/manifest；完成同一逻辑内容登记，零新 dispatch。 |
| core 原件后、完成事件前 | 核对同号材料登记/原终态，按原 command 恢复；不靠随机 ref 生成另一份产物。 |
| lock/logout/session generation 变化 | 立即阻止新动作；已 started 无可信终态保持 Unknown。不能清理证据后宣称取消。 |
| 内容删除或数据恢复 | 正式 data fence 先阻断读取；原件/派生投影/材料登记按原 scope/subject 清理。Host 对其执行域副本给出真实清理证据；外部副本未清理只能报告相应 Unknown。 |

本提案不放宽当前数据生命周期的“有活动或挂起 run 时不清理”规则，也不授权对历史 Unknown 发第二个动作。未封存来源、Host 副本与 core 原件必须分别取证，不能仅删 Web Blob 就报告数据删除完成。

## 8. 输入和 PTY 的独立义务

这份结果协议不启用 ContentRef/SecretRef Type 或 PtyInput。它们仍是原设计待实施义务，并非从目标中删除：

1. 引用输入先解决不可变内容/秘密版本、解析 owner/scope、最终 plan 的输入绑定、protected IPC、撤销/过期、同号恢复和清理。不能把引用改成 Literal 后复用旧审批。
2. Secret 输入后的目标可能回显、变换或截图暴露秘密；仅扫描原字符串不能证明脱敏。未确定输出和解密边界前不能注册 secret-capable consumer，也不能默默把完整输出变成空结果报成功。
3. 真实 PTY 需独立证明 restricted token、首次用户指令前的 Job custody、逐 job 文件/网络/环境边界、ConPTY 进程树取消及 trusted exit。portable-pty 的普通 spawn 不是这些证据。
4. PTY 不复用文件 worker 的 child-process profile，不从普通桌面 helper 继承用户完整权限，不改为 Brain/SYSTEM 执行。若需要变更五个既有原生边界内的职责，先给出确切封装与审计范围；本提案没有批准第六个 unsafe 文件。

这些机制需要技术验证及独立可审查契约。本节列明后续工作，不将尚未可行性验证的方案伪装成已冻结接口。

## 9. 实施和验收顺序

先确认本提案，回填 PRD24/26、architecture10、必要 canonical 条款并单独提交，再依赖桌面授权提案完成生产接线。之后依次实施 v2 closed 协议/peer、Host sink 与封存、core 保护投影/幂等恢复、正式 Web 工件读取，最后对真实安装取证。

必跑正向：原 binary 从自建隔离 surface capture 一次，逐段到 Host AEAD、core AEAD、真实 Web 预览逐份核对同一原始摘要/长度/尺寸；真实 service/session/peer/grant/plan/nonce 均独立核验。测试图片不来自用户当前桌面的任意内容。

必跑负向：旧 peer、unknown schema/字段、错 media type/尺寸、空/超长/截断/尾随 PNG、跨 owner/session/job/nonce/manifest、乱序/重复/停滞 chunk、错误整体摘要、超时/预算/并发满、取消、锁屏/注销、helper/Host/core 各切点断线、原审批重放、同号重取、删除 fence 和浏览器 logout 缓存。所有不支持或预执行拒绝都证明 driver=0；Started 后不明的恢复证明零重新 dispatch。

源码检查与组件回归按模块 PRD 执行；IP10/IP11/IP17/IP18/IP21/IP22/IP24 必须用实际安装包和浏览器独立取证。结果通道组件通过不关闭原生验收，也不关闭输入、PTY、受管消费者或其他平台义务。
