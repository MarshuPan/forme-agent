# 安装版受管文本渠道入口

本文细化 owner 已批准的 installed-managed-consumers-closure §2–4 的输入接线，不扩大对外发送或能力门例外。已有 ProductChannelConfig 保持原形；受管来源、凭据及参与者映射只由安装/管理域提供。

## 来源和发布

固定文件为 core_config_path.with_extension("channel-sources.json")，closed schema 1 ProductChannelSourceCatalog，最多 64 条、128 KiB，拒绝链接。每条 ProductChannelSource 包含 schema_version、channel_ref、connector_ref、scope、participant_binding、participant、credential_ref、credential_digest。前三个引用及 scope/binding/credential_ref 使用既有 128 字节引用检查；participant 是外部 ParticipantId，禁止 owner/forme-representative/当前 owner。credential_digest 是独立至少 32 字节渠道 token 的 SHA-256 摘要，不使用 Web/Core/Host 的管理凭据。公开设置只显示既有 opaque 引用；token、摘要和私人来源均不返回浏览器。

本入口只支持已有 Text adapter。Loopback 的对外 connector 与 LocalDevice 的观察授权尚未齐备时，不得通过此入口启用。Host 启用前校验来源与设置的 channel/connector/scope/participant_binding/credential_ref 精确匹配且 scope 为当前 workspace。来源快照写入 ConfigState schema 14 的 channel_sources；旧序列化省略空值。Core 根对象增加可选 channel_ingress={schema_version,configuration_path,sources}，只由 Host 发布。保存/启动不会发送消息或创建 ExternalCommunicationGrant。

## 请求边界

Core 增加唯一 POST /v1/channels/{channel}/messages；该路由不在 Web owner 代理 allowlist 中。请求必须使用来源绑定的 Bearer token，禁止 Origin、Cookie 或 Web assertion；未知渠道、错误凭据、已禁用或漂移来源统一拒绝。认证恒等比较摘要，每次提交前复核当前 core 发布与原来源目录，安装工作准入/停止排空和有界阻塞执行继续生效。

closed schema 1 请求为 {schema_version,message_id,conversation,text}，引用校验、正文至多 16 KiB且非空。收到时间由服务器赋值，session 由 channel/participant/conversation 域摘要生成，消息 id 由 channel/participant/message_id 域摘要生成，不能指定 owner session。TextChannelAdapter 固定 External(participant) 与 scope，AuthContext 为 Token 外部身份；随后仅用 LocalGateway.start_channel_event（复用 normalize/身份校验与原 Harness accepted-run dispatcher，HTTP 接收不等待模型完成） 进入同一 ProductionRuntime，保留 CommunicationEventReceived 与 UntrustedData provenance。

响应只包含 {schema_version,accepted:true}，不返回 run id、模型文本或 owner 状态，避免把消息接收回执当成未经披露批准的对外回答。重复提交交给既有 durable command 去重；异义同 id 拒绝，重启不重复创建 run。对外回复、Loopback/设备、完整安装验收仍是独立剩余义务，本入口不能替代。

## 有限设置接口

独立 schema 1 ProductChannelSettingsPatch 为 schema_version/operation_id/expected_revision/owner/channels，摘要域 forme.product.channel-settings.v1。View 为 schema_version/owner/active_revision/pending_restart/configured/channels/sources/sources_available，sources 只返回禁用的 Text 配置选择；来源损坏仍可禁用。Response 为 schema_version/operation_id/applied/view，Operation 为 schema_version/owner/operation_id/request_digest/committed。GET/POST /api/v1/product/channel/settings 与 GET operations/{id} 使用 server-owned owner、CSRF、绑定原摘要的密码重认证、原 journal/CAS 和生命周期排他；只改 channels，不覆盖其他只写凭据。未知结果只查询原操作。
