# 安装版渠道的单次本地交付

本文落实 owner 于 2026-11-11 批准的[有限出站代理](proposals/installed-channel-delivery-closure.md)。保留 WorkerJob、WorkerPermit、WorkerExecutionSpec、原事件集合与五处原生边界。此契约是实施约束，不是已完成或安装验收声明。

## 来源与固定正文

渠道来源 schema 1 的旧字节形状保持。schema 2 可新增可选 `delivery`，其 schema 1 字段为 `endpoint, schema_digest, credential_ref, credential_digest, receipt_public_key, capability_ref, permission, max_input_bytes, max_output_bytes, timeout`。所有对象拒绝未知字段；空可选值不写入旧来源。`receipt_public_key` 为预置的 32 字节 Ed25519 公钥，来源目录不能由模型或消息注册。凭据仅由 Host 从其安装派生的私有目录按 credential_ref 的摘要读取并核对 credential_digest，不由 Core 或 worker 读取。

endpoint 仅接受规范的 `http://127.0.0.1:<port>/<path>` 或 `http://[::1]:<port>/<path>`：显式非零端口、无 query、fragment、userinfo、hostname、反斜线或编码后的路径分隔符。代理只 POST 此精确地址，禁止重定向、代理环境与 DNS。输入/回执各至多 65,536 字节，期限至多 60,000 ms 且不超过原 job/permit。配置启用 Loopback 前必须有此来源及实际隔离消费者；Text 来源仍只启用接收。

动作复用现有 `AppApiActionSpec` 的 `Mutation(Post, ExternalInput::Content, idempotency_key)` 和 `ActionType::Deliver`。connector、endpoint、schema_digest、credential 引用、participant、capability、permission、scope 全部匹配预置来源；representation 与 disclosure_request 必须存在。正文为受保护的已披露内容，原计划的 idempotency_key 固定为 `communication:<message_id>:sha256:<正文十六进制摘要>`。这一摘要在计划/审批之前确定，Host 对原内容字节独立复核；不能在批准后替换正文。消息正文不会进入普通事件、配置或日志。

## 授权与接缝

`ManagedChannelDeliveryGrantV1` 的字段依次为 `schema_version, grant_ref, installation_ref, owner, channel_ref, connector_ref, source_digest, participant, scope, job_id, action_ref, plan_digest, normalized_action_digest, permit_digest, authority_epoch, nonce, endpoint, method, content_digest, max_input_bytes, max_output_bytes, expires_at`。schema 为 1，method 固定 `POST`。grant_ref 为 `channel-delivery-grant:<hex>`，摘要域为 `forme.managed-channel.delivery-grant.v1`；计算时只将 grant_ref 置空，不改变其他字段。

`ManagedChannelDeliveryInputV1` 为 `schema_version, execution, grant_ref, content_bytes`；execution 是原 `WorkerDispatchRequest`，不是新的计划。使用专用 AgentExecution 路由 `POST /v1/agent/workers/channel/dispatch`，受管 Windows 必须通过原 Brain Service SID/image 认证的 named pipe。普通 worker dispatch 拒绝本专用 profile，旧 transport 默认拒绝新方法。Web、OwnerMaintenance、worker 本身不拥有此路由。

Host 在配置排他锁内依据当前 owner、enabled/source/participant/credential、原 execution/job/permit 重建 grant。客户端仅携带 grant_ref；不接受调用者自报的任意 grant。原 journal 的 admitted/started/nonce、结果封存与 probe/cancel 继续是唯一执行事实。授权、尝试发送和原收件方回执的有限 companion 仅保存绑定与摘要；发送 intent 必须先落盘，重启或重复请求不重新发送。

## 隔离进程与代理

复用已安装并验证摘要的 `forme-file-worker` image 的独立有限入口，不开放 shell。每 job 创建 Low AppContainer 和 kill-on-close Job，使用既有有界 stdio 原生接缝。worker 只拿到本 grant 的有限描述，可请求交付这一个固定 grant，不能选择地址、凭据或替换正文。Host controller 负责原 Job 的期限/取消、I/O 收束和至多一次代理调用；其他 job、用户/脑数据、网络及凭据仍不可访问。

正式发送前再次验证当前配置/source、期限和取消状态；请求可能写出后任何失联、超时、取消或崩溃均为 Unknown。原结果只能按原编号读取，不因 HTTP 200、worker 退出或新建 nonce 推断成功。静止检查必须覆盖此 adapter 的实际 worker/controller/lease 资源。

收件方返回独立 closed schema 1 的签名回执：`schema_version, grant_ref, job_id, nonce, plan_digest, content_digest, participant, accepted, signature`。accepted 必须为 true；signature 是按字段顺序对不含 signature 的回执、域 `forme.managed-channel.delivery-receipt.v1` 签名的 Ed25519 64 字节值。Host 用来源绑定公钥核验后，原回执进入既有 AEAD worker result store，再由 Core 原摘要和删除 fence 核验。错误签名/绑定、缺回执和截断响应保持 Unknown，重定向也不产生第二个连接。

## 大脑与验收

输入仍由已有 adapter/Gateway 进入同一 ProductionRuntime。输出必须先经 ExternalCommunicationGrant、disclosure、representation、termination/envelope、Harness 原策略和普通能力门，再得到最终计划及逐次批准。一次性的本地交付只绑定一份正文/接收方；取消原 run 同步撤销尚未发送的本 job，不生成隐式持续授权。owner-discovery 的能力门例外不得用于本路径。

测试覆盖精确准入/正文/回执、零审批前连接、变更/撤销/错误接收方/重定向/秘密泄露拒绝、发送后失联 Unknown/零重派、原号恢复和真实 Low worker/SCM。安装验收使用仓库自建本地收件方；源码、模拟代理和组件回执都不能替代安装后的联合证据。
