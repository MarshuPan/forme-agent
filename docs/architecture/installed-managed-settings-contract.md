# 安装版受管配置 v3 契约

本文件是 PRD25 §11.14 与 architecture/10 §5.3 的规范性补充。2026-11-09 owner 已批准受管消费者、桌面授权及截图输出三项提案。本文固定第一项配置接缝；授权和结果交换继续遵守各自已批准提案，不能仅凭配置启用宣称消费者或原生验收完成。

## 1. 兼容与事务

独立常量 `PRODUCT_MANAGED_SETTINGS_SCHEMA_VERSION = SchemaVersion(3)`。原 `PRODUCT_CONFIG_SCHEMA_VERSION = 1`、`PRODUCT_SETTINGS_SCHEMA_VERSION = 2` 和旧验证器接受集不变。`ProductConfigViewV3/PatchV3/ResponseV3/CoreActivationResponseV3` 是独立 closed DTO：逐字段继承 v2 的语义，顶层 schema_version 为 3，新增 `managed_capabilities` 和 `channels`。patch 的两个新增字段均为 Option；省略保持原状态，显式空集合清空相应配置。View 不返回 publisher 公钥原文、凭据、程序路径或固定参数。

沿现有配置锁、CAS、不可变 revision、journal prepare/publication/commit、启动 reconciliation 与真实 core acknowledgement；不得另建配置事实权威。Host 私有 ConfigState 新版 10 增加 optional managed 配置、渠道和 publisher 材料引用；旧 2–9 的字段/序列化/发布内容不重写。任何新版字段只能出现在 state 10；state 10 至少有一项新字段。旧客户端保存保留新版字段并保持 state 10。恢复从原 snapshot 重放发布，不重新执行外部操作。

v3 request digest 为 canonical_digest(("forme.product.settings.v3", 完整 typed patch))，包括省略/显式值、owner、operation、expected revision 和写入的公钥。Host journal 只持 digest，不存 patch 原文。相同 operation/digest 返回原结果；不同 digest 拒绝。schema、未知字段、集合/长度、key digest、scope 与消费者来源静态验证均早于 credential/staging 写入。保存配置不会做 discovery 或启动外部程序。

## 2. 新配置 DTO

所有内层新对象 schema_version=1 且 deny_unknown_fields。所有 ref/Scope 使用 validate_product_id（最多 128 字节，禁止任意路径）；不同集合的引用空间独立，同一集合禁止重复。所有数值为有界整数，拒绝 unknown enum。

| 对象 | 字段与精确限制 |
| --- | --- |
| ProductManagedCapabilitySettings | schema_version、mcp_servers: Vec<ProductMcpServerConfig>、plugins: Vec<ProductPluginConfig>、publisher_keys: Vec<ProductPublisherKeyView>、computer: ProductComputerPlacementSettings。前三项最多各 64 项。 |
| ProductManagedCapabilityPatch | 与 Settings 相同，但 publisher_keys 为 Vec<ProductPublisherKeyPatch>。没有 Debug 公钥原文；view 由 Host 的已验证材料生成。 |
| ProductMcpServerConfig | schema_version、server_ref: String、program_ref: String、scope: Scope、allowlist: ProductMcpAllowlist、enabled: bool（默认 false）、trust: ProductSkillTrust（默认 Untrusted）、timeout: DurationMs（1–60000）、output_budget: u64（1–1048576）。 |
| ProductMcpAllowlist | schema_version、tools: Vec<String>、resources: Vec<String>。各最多 128 项；每项 1–2048 字节，不含控制字符，不重复；不接受 `*`；空集不给任何使用权。 |
| ProductPluginConfig | schema_version、plugin_ref: String、source_ref: String、expected_source_digest: SchemaDigest、scope: Scope、enabled: bool（默认 false）、trust: ProductSkillTrust（默认 Untrusted）。来源必须由 Host 验证，不能通过 manifest 自报。 |
| ProductPublisherKeyPatch | schema_version、publisher_ref: String、public_key_digest: SchemaDigest、public_key: [u8;32]。digest 必须为这 32 字节的 sha256_content_digest；必须是有效的非弱 Ed25519 验证公钥。公钥只写入 Host 专用受保护不可变材料文件。 |
| ProductPublisherKeyView | schema_version、publisher_ref、public_key_digest、configured: bool；无公钥值或路径。 |
| ProductComputerPlacementSettings | schema_version、enabled: bool（默认 false）、scope: Scope、surface_refs: Vec<String>（至多 64，唯一）。启用需非空且经 Host 校验；默认 scope 为 workspace:default。授权/会话选择由桌面协议单独执行。 |
| ProductChannelSettings | schema_version、channels: Vec<ProductChannelConfig>（最多 64）。 |
| ProductChannelConfig | schema_version、channel_ref: String、adapter_ref: ProductChannelAdapter、connector_ref: String、scope: Scope、enabled: bool（默认 false）、participant_binding: String、credential_ref: Option<String>。 |
| ProductChannelAdapter | text、loopback、local_device 三个固定枚举；不是第三方平台适配声明。participant_binding 是 Host 认证映射引用，不是消息自报身份。 |

managed_capabilities 编码总量最多 65536 字节，channels 最多 16384 字节，旧 skills 仍最多 65536 字节。现有整体 256 KiB HTTP/config 上限、128 KiB state 上限保持，组合后超限必须在写入前失败。单项上限不是保证所有集合可同时填满。公钥文件只由 Host 配置发布向同一个 registry 预置；publisher grant/admission/lifecycle 仍由原 M5 事件控制。

## 3. 读取与认证

现有 `/api/v1/product/config?schema_version=3` 返回 v3；POST 原路径按 schema 解析 v3 body，owner 仍只来自认证会话。publisher key 变更要求原有 owner 重新认证，将密码校验绑定 v3 digest，密码不传 Host、配置或日志。Host 内部仅增加 `GET /v1/host/configuration/v3`，POST 原配置路径按版本解码。旧默认 GET 和 v1/v2 body 精确兼容。

ProductHostBroker 新增 `configuration_view_v3(&HostAuth) -> Result<ProductConfigViewV3>` 和 `apply_configuration_v3(&HostAuth, ProductConfigPatchV3) -> Result<ProductConfigResponseV3>`；沿既有 WebConfiguration 认证和 single writer。ProductConfigStore 新增 `snapshot_settings_v3`、`apply_settings_v3`；LocalConfigurationClient 对应 snapshot_v3/apply_v3。以上增量不扩大原 trait 的执行权。

## 4. 拒绝与可用性

不支持 schema 返回原 UnsupportedVersion；未知字段/超限/无效引用/key 返回原 InvalidRequest；CAS/异义 operation 返回 Conflict；无认证/CSRF 失败沿原语义。缺 program/source/connector/placement 或实际消费者时不得保存 enabled=true，不创建进程，不发布配置。禁用项可保存为待配置，并明确 unavailable 原因。feature 的 configurable 只在实际持久配置和运行消费者均接通后生效。

动态撤销和变更在下一次执行出口重查；不借重启自动重发历史 Unknown。测试必须覆盖 v1/v2 bytes 与摘要、state 2–9 重开、v3 CAS/幂等、错 key/source/schema、未知字段、超限早拒绝、旧客户端保留新字段、重启公钥重建和零隐式 discovery。
