# 安装版桌面截图结果实施契约

本契约细化 owner 已明确批准的[截图结果方案](proposals/installed-desktop-output-closure.md)，承接 canonical §29 与桌面授权契约。批准范围不变；本文是实施规范，不能当作实现、部署或 IP 验收证明。原方案的预算、原件核验、零重派、删除和原生验收要求均继续生效。

## 1. 版本和原任务绑定

原 DesktopSessionBinding/ActionRequest/ActionReceipt 和 WorkerJob/Permit/ResultManifest/Read/Chunk 均保持 schema 1 及原序列化形状。新增输出对象采用独立 `DESKTOP_OUTPUT_SCHEMA_VERSION = SchemaVersion(2)`；不提升原桌面 schema 常量。

`DesktopExchangeBinding` 字段按下列顺序 canonical 序列化：schema_version、installation_ref:String、owner_ref:VerifiedPrincipal、job_id:String、permit_ref:String、permit_digest:SchemaDigest、action_ref:ActionId、workspace_scope:Scope、plan_digest:PlanDigest、normalized_action_digest:SchemaDigest、authority_epoch:AuthorityEpoch、nonce:Nonce、grant_ref:String、os_session_ref:String、session_generation:u64、deadline:Timestamp。Host 从原已验证 job/permit/execution/request 产生；安装引用沿原 WorkerResultBinding。所有 ref 沿 product id 上限与字符约束；digest 必须为完整 SHA-256；generation/epoch 非零，deadline 晚于原 grant 的签发时刻且不超过原 job/permit/grant。grant 可在 permit 之后签发，构造时按 grant 的签发时刻核对完整原 permit/job，拒绝早于 permit 的 grant；不能用 permit 的旧签发时刻误判后签发的有效 grant。

binding 摘要域为 `forme.desktop.output.binding.v2`，header 摘要域为 `forme.desktop.output.header.v2`，descriptor 摘要域为 `forme.desktop.output.descriptor.v2`。摘要统一对 `(domain, 完整对象)` 计算；对象中不含自身摘要字段。

新增 closed `DesktopOutputTransfer`：schema_version、binding、transfer_ref:String、output_budget_bytes:u64、width:u32、height:u32。Host 在发送动作前生成，transfer_ref 为 `desktop-transfer:` 加 binding canonical 摘要的 64 位十六进制部分；它不是外部读权限。预算为原 execution output budget 与 64 MiB 的最小值，必须非零；width/height 从原 ComputerActionSpec.bounds 的非空差值精确派生，像素乘积不超过 16,777,216。

transfer 必须逐项匹配原 DesktopActionRequest 和原 Screenshot intent（包括完整 normalized digest、surface、bounds、workspace、action）；helper 不可改变。sink 只向 adapter 暴露该不可变 transfer，不能提供路径、密钥或 journal。

## 2. 帧与验证顺序

沿原已认证 named pipe 增加固定 `--desktop-session-helper-output-v2` 启动模式；旧模式的消息及 hello 保持。v2 Hello/Ready/Ping/Pong 必须严格匹配版本；v2 Execute 包含原 request、intent 和 transfer。双方在原 Started 前完成协议确认；未知/旧 peer 不降级执行 Screenshot。

Windows 的输出支持声明固定为 `share/forme/desktop-output-v2.json`，closed 内容是 `{schema_version:2, helper_digest:SchemaDigest, helper_bytes:u64}`，精确绑定同一发布中的 `bin/forme-executord.exe`。声明与 helper 都必须进入签名 ReleaseManifest；Host 只从已验证活动发布建立支持状态，持有声明、helper 及目录祖先的原句柄，在取得保管后重新核验发布。仅有路径或未签名初装目录的旧构造函数不启用截图；首次安装的可信支持仍需独立接通，不用声明文件存在替代签名验证。打包器生成声明不代表原生行为已验收；实际 v2 Hello/Ready/Ping/Pong 仍须在 Started 前成功。

v2 双端使用非阻塞 pipe；控制帧与 raw chunk 写入/读取共享从原 request deadline 一次换算的 monotonic deadline，部分进展不延长期限。连接/握手使用独立短上限并受 grant 到期限制；不改变 v1 模式。helper 从 live pipe server PID 验证现有 SCM Running、own-process、LocalSystem 与同发布 Host image，沿已有 AgentExecution 身份检查，不放宽 ACL。原 helper 在 Finish 后退出；Host 对同一 grant 的晚到观察只能复用已验证的一次交换并重查实际 session/generation，不得重启 helper，未确认交换保持拒绝。

输出控制帧为 closed schema 2 对象：

- `DesktopOutputHeader { schema_version, binding, transfer_ref, media_type:String, bytes:u64, content_digest:SchemaDigest, width:u32, height:u32 }`；只允许 image/png，编码不超过 8 KiB，正文非零且不超过 transfer 预算。
- `DesktopOutputChunkHeader { schema_version, binding_digest, transfer_ref, offset:u64, bytes:u32, chunk_digest, next_offset:u64, finished:bool }`；随后紧跟准确 bytes 个 raw bytes。bytes 在 1..=32768，最多 2048 块；offset 从零开始严格连续、next_offset 等于 offset+bytes，只有最后一块 finished=true。
- `DesktopOutputFinish { schema_version, binding_digest, transfer_ref, header_digest, bytes, content_digest, receipt:DesktopActionReceipt }`；只允许一次，必须位于完整正文之后，receipt 为原 exact request 的 Committed，时间在原 deadline 内。
- `DesktopOutputDescriptor { schema_version, binding, manifest_digest, content_digest, bytes, media_type, width, height }`；仅由 Host 在解码校验成功后生成。与原 manifest 的 binding、digest、bytes、Available/Binary/Completed 和原 desktop grant 完全一致，manifest 产出时刻不得超过原 deadline。

普通控制帧仍最多 256 KiB。正文既不派生 Debug，也不使用 JSON 数组/base64；raw read/write 是独立长度限定接口，保持原 peer 身份与 SQOS。所有步骤共享原 deadline，不因读到下一块而延长。

解码必须先核对 PNG signature、chunk 边界、唯一 IHDR/IEND 和 IEND 后零尾随正文；拒绝 APNG、多帧、未知 critical chunk、重复头、截断及尺寸不符。每个 chunk 均通过显式 CRC 检查，chunk 总数上限 65,536；再用锁定的 png 解码库检查压缩数据和实际解码尺寸。解码工作预算最多 128 MiB（包含输出像素和 decoder 分配限额），编码正文另受 64 MiB 限额；不只读取 IHDR 后宣称图片有效。库是独立可审查外部依赖，不新增内部 crate 依赖边。

## 3. Host sink、封存与恢复

保留 `DesktopHelperAdapter::execute`；新增 `execute_with_output(request, intent, output_sink: &mut dyn DesktopOutputSink)`，返回原 DesktopActionReceipt。默认实现拒绝 Screenshot，其余动作仍可调用 execute。新增只读 `supports_output() -> bool` 缺省 false；Host 在 Started 前拒绝不支持输出的 adapter。`DesktopOutputSink` 只含不可变 `transfer()` 读取及 `begin(header)`、`write_chunk(header, bytes)`、`finish(finish)`，不承担动作授权或重试。

Host 在已有 worker operations 排他区内接收，一次至多一份结果；该配额从创建暂存到封存/明确失败清理一直占用。Host 为原 job 在私有结果目录 create_new 固定暂存槽，拒绝链接/替换、限制长度，保存原 transfer 元数据。该单槽跨 job/进程重启保持；失败或不明时保留并阻止新截图准入，不能通过换 job 累积暂存。Windows 从创建到封存清理持有原文件和目录祖先句柄，清理经已有审计边界按原句柄删除；不关闭句柄后重新按路径删除。每步错绑定/顺序/摘要或超时使 sink 永久失败，不接纳补发或第二个 header。

finish 后 Host 重算原件 SHA-256、长度并完整验证 PNG，核对当前原 session/generation，然后沿既有 WorkerResultStore 写入 AEAD。封存前再次核验已消费 grant、当前 consent 与会话，持授权锁完成封存/回执接纳，防止传输期间已提交的撤销被晚到成功绕过。生成 Host 结果引用，manifest 固定 Available/Binary/Completed。原 helper receipt 的引用只指 helper 的一次 capture，不直接作为 Host/core 原件身份。

顺序固定为：实际 capture 一次 → 完整传输与验证 → AEAD 原件 → 原 manifest → Host descriptor → result_sealed 暂存清理 → 原 WorkerReceipt。上述完成前不能返回 Completed。Started 后任一不明确失败保持原 job Unknown；重复 dispatch 只返回 journal 原件，driver 不再调用。完整暂存/封存可供核对，不凭文件存在推断终态，也不自动从 Unknown 追加成功回执。

worker journal 的新截图 Admitted 追加可选 `output_transfer`，在 Started 前记录完整原传输绑定；普通旧记录省略该字段，原 schema 1 前缀字节和摘要保持。新截图准入与 descriptor 事实使用 envelope schema 2，成功 receipt 必须已有相同 transfer 的 manifest 和 descriptor。首次 v2 后不降回 v1；旧 reader 必须拒绝 v2。重放校验 descriptor 对原 job、permit、desktop binding、execution binding 和 manifest；缺 manifest、错序、替换、第二个异义 descriptor 均拒绝。非截图旧记录保持原语义。

`WorkerBrokerTransport` 和 `IsolatedExecutorHost` 增加只读 `desktop_output(job_id)`，缺省拒绝；Host 使用 `/v1/agent/workers/{id}/desktop-output`，限定 AgentExecution domain。原件读取仍经既有 manifest/chunk，不增加 Host 公共下载 URL。查询必须核验持锁 journal 原件，不能创建 descriptor 或执行恢复副作用。

## 4. Core 受保护投影

`WorkerActionBackend` 新增显式 `with_artifact_store(Arc<dyn ArtifactStore>) -> Self` 装配；原构造接口保留且默认没有截图消费者。正式 managed Computer factory 必须注入现有 ProtectedRuntimeArtifacts；缺 sink 在新 Screenshot dispatch 前拒绝。execution 不依赖 harness/store。

`ArtifactStore` 新增拒绝型默认方法 `write_worker_output(binding: &WorkerArtifactBinding, bytes: &[u8]) -> Result<ArtifactRecord>`。closed schema 2 `WorkerArtifactBinding` 包含 `schema_version, manifest:WorkerResultManifest, descriptor:DesktopOutputDescriptor`，逐项核对原件及 binding。新增只读 `supports_worker_output() -> bool` 缺省 false，只有受保护实现显式返回 true，backend 在新 dispatch 前核对。普通 `write` 语义保持；FileArtifactStore 不可因此被当成受保护消费者。

ProtectedRuntimeArtifacts 以 `forme.runtime.worker-artifact.v2` 加完整 binding 的 canonical digest 生成确定逻辑 object id（底层 content_ref 仍由既有 seal_record 派生）。同一 job/manifest 只登记一个逻辑产物，AEAD nonce 继续随机且唯一。首次写入沿现有内容保护/lifecycle 单写者；后续先读取原内容/材料登记，验证 scope、subject、digest、bytes 和完整 binding，成功则复用。缺失部分只按原 object 完成登记，不重新执行截图；冲突/删除 fence 拒绝，不覆盖历史。

core 完整保留原 job/permit/manifest/receipt、offset/chunk、budget、整体 digest 校验。Utf8 文本继续验证 UTF-8；Binary 只接受原 Screenshot、完整 Host descriptor、独立 PNG 校验及正确保护登记。完成事件前必须取得内容与材料回执。输出事件只记录 Untrusted 摘要和安全 content_ref，ExternalActionReceipt 指向原始 PNG 的 digest；不写图片/base64。截图是观察效果，按既有 EffectStatus::Observed 表示；worker Completed 只是该观察动作完成。

## 5. 认证产物目录与分块读取

上传 ArtifactCatalog/Upload/Download v1 和 4 MiB 上传限制保持。独立 v2 工具产物查询 `ToolArtifactCatalogQuery {schema_version:2, session:SessionId, run:RunId, page:DataPageQuery}`，返回该 session/run 下已完成动作的安全元数据：content_ref、action、scope、原 bytes/digest、image/png、width/height。目录摘要及分页使用现有 snapshot 约束，单页最多 100 项。

查询从 authority 的 RunAccepted/SessionBound、原 ActionCompleted 的 ExternalActionReceipt 与保护材料登记解析归属，要求认证 owner、原 run/session/workspace 一致。pending、Unknown、没有可信材料登记或已删除的内容不发布为可预览原件。浏览器传来的 ref 不构成归属证明。

`ToolArtifactRead {schema_version:2, session, run, content_ref, content_digest, offset:u64, max_bytes:u32}` 的 max_bytes 在 1..=262144；返回 closed `ToolArtifactChunk {schema_version:2, session, run, content_ref, content_digest, offset, next_offset, total_bytes, finished, bytes}`。内部 typed 传输可沿既有 bounded chunk 编码；Web 响应为实际 PNG 分块字节，Content-Type:image/png、nosniff/no-store，并有明确范围和整体摘要元数据，不返回 Host 引用/路径。

每次读取持 data read fence，重新核对 owner/session/run、材料登记、摘要/长度及删除状态。Web 使用有限 typed 路由 `/api/v2/sessions/{session}/runs/{run}/artifacts` 与对应 `/{content_ref}` 分块读取，不开放通配 core 代理。前端仅在认证内存中组装、重算完整 digest/bytes 后创建 Blob URL；关闭/登出/删除/认证失效立即 abort 并 revoke。未经完整验证不渲染部分图片，不持久缓存。

工具目录的独立常量 `TOOL_ARTIFACT_SCHEMA_VERSION=2`。closed `ToolArtifactEntry` 精确包含 `schema_version, content_ref, content_digest, action, scope, bytes, media_type, width, height`；closed `ToolArtifactCatalogView` 包含 `schema_version, session, run, digest, total, next_offset, artifacts`。目录最多 20,000 项，单次 run 事实读取最多 100,000 条；超界明确拒绝。分页摘要绑定 owner、原 session/run/workspace、原 run head 与完整元数据列表；读取前后 run head 不一致返回 Conflict。工具目录不扫描图片正文：受保护 descriptor 和材料回执给出尺寸/摘要，并通过 store 的当前 owner/scope/subject 原件存在性查询排除 pending/deleted subject 与删除的 object。公开分块读取才解密并完整核验原件；不使用未受删除 fence 保护的引用列表代替存在性核对。

内部只增加 `/v1/artifacts/tools/query` 和 `/v1/artifacts/tools/read` 两个 typed POST 读取入口，仍核验正式 core assertion，HTTP 动词不赋予写权限。Web v2 GET 目录参数固定 `offset, limit, snapshot?`，分块参数固定 `content_digest, offset, max_bytes`，未知字段拒绝；不把这两个内部入口加入旧通配代理。Web 核对 closed typed 响应与原请求，并在回传前再次核对原认证会话。原始分块返回 206、`Content-Range: bytes start-end/total`、`X-Forme-Content-Digest`、`X-Forme-Total-Bytes` 与 `X-Forme-Next-Offset`；无原件身份、总量、范围或格式相符的响应不能显示。整体 source 固定，旧上传附件入口不变。

浏览器使用有限流式读取，单块分配不超过 256 KiB、整图不超过原 64 MiB 预算；拒绝超长正文、错序/截断范围、错摘要和尺寸。校验完成才创建 URL；只读操作允许用户显式重新读取原件，不派发截图。切换运行/会话/工作面、关闭预览、页面隐藏、退出或认证失效会取消读取并回收 URL，数据页的生命周期操作前先卸载图片消费者。CSP 仅对 `img-src` 增加 `blob:`，不增加脚本/对象来源；图片样式使用现有受控 CSS。完整 Host 副本删除及原生联合验收仍遵守 §6。

模型当前仍只看到引用/状态，不据此宣称理解像素。真实视觉模型消费者和依赖像素的任务另行验收。

## 6. 验收和清理边界

按已批方案 §7/§9 验证各 crash cut、原号重取、跨身份/会话/计划拒绝、预算/PNG/尾随内容负例与数据 fence。必须覆盖一次 capture 经 helper→Host AEAD→core AEAD→实际浏览器的原始 digest、bytes、尺寸一致。原生服务/token/session/peer 和独立测试 surface 另取证。

内容删除需对 Host 执行域副本取得真实清理证据；只有 core/Blob 清理不得称全量删除。活动/挂起 run 的原 data gate 保持，不增加隐藏后台清理或修改历史完成事件。Content/Secret 输入、PTY、受管 MCP/插件/渠道及其他平台不因截图输出闭合而视为完成。
