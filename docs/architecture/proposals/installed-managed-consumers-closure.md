# 安装版既有能力消费者接线草案

状态：2026-11-09 owner 已明确批准并要求实施（“批准三项协议补充并实施”）。本文的受管 MCP、插件、能力包与渠道接入范围生效；先回填正式契约，再分模块实现与验证。批准不等于实现或原生验收完成。下文安装状态是提案编写时的历史观察，当前现场以新的安装回执为准。

## 1. 本轮源码核对

| 原设计义务 | 当前真实实现 | 还缺什么 |
| --- | --- | --- |
| 安装版 MCP 配置及使用 | `protocol/product_config/capabilities.rs` 只有 skills；`harness/managed_config/skills.rs` 只转换 skills。开发 MCP registry 的 discovery 在 `capabilities/mcp.rs` 直接创建子进程。 | Host 持久配置、受管 registry 装配、隔离 discovery/call、真实设置 UI。不能把开发进程内 driver 装进 Brain。 |
| 插件配置及生命周期 | `capabilities/plugin.rs` 已有声明式贡献、trust、toggle、hook 重查和 managed binding；受管配置没有插件输入。 | 用同一 registry 消费受保护 manifest/已准入 package 的贡献，持久绑定来源、版本与启用状态。 |
| 能力包准入及生命周期 | M5 的 `EcosystemGatewayControl`、`gateway/server/evolution.rs` 和 `/v1/ecosystem/*` 已存在。Web 的显式 allowlist 不含这些路由。`configure_ecosystem_publisher_key` 是 authority-local 预置方法，不接受 catalog 或 owner-control payload，keyring 当前在内存。 | Web 的 typed owner adapter；Host 管理并在重建 registry 前加载 publisher 公钥；安装后重启不丢校验材料。包生命周期仍用原脑事件与 CAS，不能复制进产品配置成为第二权威。 |
| 消息渠道配置 | `communication` 已有 Text/Loopback/LocalDevice adapter、AuthContext、bounded session 与 disclosure；`gateway/ingress.rs` 已有 normalize/submit/delivery。受管配置无渠道输入。 | 持久注册已实现的 adapter/connector，绑定真实凭据与 participant，接入同一 Gateway。不能用 Web owner session 把外部参与者提升成 owner，也不能把新的平台名称当成已实现 adapter。 |

以上对应 requirements/17 P07、PRD25 `ProductConfigSnapshot.channel_configs/capability_options`、PRD25 §11.7/11.10、PRD07、PRD14 和 M5。`unavailable` 显示正确只能证明状态真实，不能关闭这些实施义务。

## 2. 拟批准的协议增量

保留设置 schema 1/2、现有内层 DTO 和旧摘要的精确读写形状；新增独立设置 schema 3，不把共用的 v1 常量整体升级。所有新对象 closed/versioned；旧客户端省略的新设置一律保留，未知字段/版本在 staging、凭据写入或进程创建前拒绝。

| 新增对象/接缝 | 确定的字段与语义 |
| --- | --- |
| `ProductConfigViewV3` / `ProductConfigPatchV3` / response | 继承 v2 的模型、脑偏好、网络与代理语义；新增 typed `managed_capabilities`、`channels`，沿同一 operation、expected revision、Host journal、配置 publication 与真实 core acknowledgement。不能改写 v1/v2。 |
| `ProductManagedCapabilitySettings` | `schema_version, mcp_servers[], plugins[], publisher_keys[]`。MCP/插件列表描述配置，不发放执行权；包安装/启用/撤销状态不存这里。现有 skills 字段继续按原协议处理。 |
| MCP server 配置 | `schema_version, server_ref, program_ref, scope, allowlist, enabled, trust, timeout, output_budget`。program_ref 解析到 Host 已注册的固定程序/参数/资源/凭据配置；模型和 tool arguments 不可改程序、环境、sandbox profile 或网络边界。默认 disabled/untrusted，allowlist 空不能表示全部。 |
| 插件配置 | `schema_version, plugin_ref, source_ref, expected_source_digest, scope, enabled, trust`。source_ref 只能绑定 Host 受保护的声明式 manifest 或 M5 已准入的精确 package contribution；不是 URL、任意路径或可执行模块。manifest 不能自报可信、扩大权限或改变 Host 配置。 |
| Publisher 公钥写入/视图 | 写入绑定 `publisher_ref, public_key_digest, Ed25519 public key`；只接受已认证 owner 的显式配置。Host 保存并在启动前核对 digest；view 只返回 publisher/digest/configured。密钥材料不由 catalog 提供，不进入脑事件或普通 snapshot；publisher grant 的 kind/scope/TTL/revoke 继续由 M5 原事件控制。产品 release 签名信任根与这些包公钥分开。 |
| 渠道配置 | `schema_version, channel_ref, adapter_ref, connector_ref, scope, enabled, participant_binding, credential_ref`。adapter_ref 只能选择已有实现。participant_binding 由认证映射验证，不接受消息正文声明 owner；敏感 credential 只写、不可读回或进事件。启用不自动创建 ExternalCommunicationGrant。 |
| Host 程序注册 | 对 MCP 所需本地程序提供独立、typed、owner 重新认证的注册事务，绑定实际文件身份/digest、固定 argv、资源范围、sandbox profile 和凭据引用。返回 opaque program_ref，视图不返回秘密。它不执行 shell，不安装包，不因注册而启动程序；任意文件编辑和 Agent 自注册不属于接口。 |

精确长度/列表上限、canonical digest 域、Host state 新版本和拒绝表必须在回填 PRD25/architecture10 时固定并由协议测试覆盖，不能留到业务实现自行解释。既有 v2 65,536 字节 skills 限制保持；不靠扩大整体 body 上限接收任意 capability JSON。

## 3. 运行与事实边界

1. Host 配置事务管理来源、凭据与受管配置；只有原 authority EventStore 管理 publisher grant、admission、包生命周期和使用事实。认证库不存第二套包状态。
2. MCP 的 metadata discovery 与工具调用都必须有明确的隔离启动来源和有界结果；既有 per-job `WorkerJob/Permit/ExecutionSpec` 已能绑定 Mcp intent，不为每个 server 增加大脑或 event writer。配置保存只做静态校验，真正运行测试必须是 owner 显式动作；不能由读设置、启动 Web 或 catalog 文本暗中启动进程。
3. discovery 输出仍为 UntrustedData。新启动时没有当前来源/digest/allowlist 的有效 metadata 就保持该 server unavailable，不能复用别的配置的发现结果。运行中禁用/revoke 按原 registry 和执行前重查阻止后续使用；Unknown 只核实原 job。
4. 插件贡献进入现有 registry，hook 变换后仍重查最终 plan。插件内 MCP/工具引用必须指向已有受管 binding，缺项按项 blocked；不凭插件 enabled 给 Brain 注册进程内 backend。
5. Web 仅新增有限的 M5 typed routes：snapshot、publisher grant、admission、prepare plan、apply change、distribution prepare；复用原请求/响应与认证 owner。不开放 `/v1/*` 通配代理，不从浏览器接受 approver/principal。正式批准读取原计划及一次性 nonce，绑定确切 plan digest、版本和期限。
6. 渠道输入经现有 adapter → Gateway → ProductionRuntime；输出经原 disclosure/termination/envelope → ActionIntent → Harness → 隔离 backend。关闭网页不取消渠道或创建第二个轮询 supervisor。
7. 保留 18-crate 图、99 EventKinds、五处已审计 unsafe 文件、单写者和所有恢复拒绝规则。现有 worker DTO 可复用的地方不改；若实现证明还必须扩大这些边界，另提具体差异，不以本草案作无限授权。

## 4. 实施与验收闭合

按同一批次交付，内部依赖顺序如下；每完成可验证模块独立提交，整体目标保持不变。

1. 回填 PRD25/26、architecture10 和必要的 canonical 文字；固定新 DTO/摘要/兼容与错误表。实现 Host 配置/state/凭据、公钥预置、旧 revision 保留和恢复路径。
2. 接通正式 managed composition 的来源/registry/隔离 backend，再做 typed Web adapters 和设置页面。缺实际消费者的 feature 不提前改 configurable；有消费者但尚未启用、缺信任或缺 native binding 分别显示真实状态。
3. 协议/组件正反验证：旧 1/2 精确兼容、CAS/异义幂等、未认证/CSRF/伪造 principal、错 source/key/schema、禁用/撤销、配置 TOCTOU、进程内 fallback=0、Unknown 零重派、旧状态重启与 keyring 重建。
4. 自建 MCP fixture 真正经过 worker：discovery/call/schema 延迟、timeout、恶意参数、文件/keys/网络拒绝、取消/进程树回收及原 job 加密结果；不能用原开发 stdio driver 冒充安装版隔离。
5. M5 正式链：受治理 catalog read → 真实签名准入 → owner exact plan approval → 贡献可见/禁用 → 重启保留；错误签名/公钥/依赖/授权均零安装。插件 hook 反例保持最终重查。
6. 渠道以真实本地 connector + 已实现 adapter 验消息进入同一脑、披露/终止、去重及撤销；未知第三方平台保持明确未支持，不伪造外部消息发送结果。
7. 新签名安装包的浏览器/系统服务联合验证纳入 IP08/IP10/IP18/IP21/IP24；已有核心 S1–S99/C1–C24 与工程/合规门按相关变更回归。Linux/macOS 仍按 owner 选定的有限策略与不发布边界。

## 5. 与当前安装候选的关系

2026-11-07 草案初稿时，`cf3fbc9` 候选的第一次提权启动被 Windows 返回“操作已被用户取消”，当时没有安装回执、现场仍为零服务。此为历史状态：后续同版本已实际安装，2026-11-09 的独立观察确认 Host/Web/Brain Running，updater Stopped。当前已签名的新候选为 `forme-windows-x86_64-20261109-7b13529`，原升级批次仍在等待同一个 UAC，没有管理员启动或升级回执；不能将源码/签名检查解释为已经升级。

以上候选与 UAC 描述保留提案编写时的观察，不代表后续安装现场。2026-11-09 owner 已批准 §2–4 并要求实施；2026-11-10 又明确批准[发现动作类型补充](installed-managed-mcp-discovery-action.md)，该范围无需重复确认。当前接线与验收进度见[产品化实施记录](../../acceptance/installed-product-implementation-progress.md)，安装/UAC、源码组件与 IP 验收继续分别取证。
