# M2-B Protocol Compatibility Note

本文固定 M2-B 对 `forme-protocol` 的 additive 变更、执行拒绝边界和 replay 语义。它承接 canonical §15、§23、§24、`05-m2-cross-system-collaboration-architecture.md` 与 `prd/19`；任何 adapter 或 connector 都不得把兼容读取解释为执行授权。

## 1. 增量

- `BackendKind`、`ProviderKind` 和 `ConfigCheck` 分别追加 `AppApi`、`AppApi` 和 `Connector`；已有枚举值不重排、不改名。
- `ActionParameters` 追加 versioned `AppApiActionSpec`；read/mutation、method、endpoint、schema digest、SecretRef、timeout、participant、representation 与 disclosure request 都进入最终 plan digest。
- `DisclosurePolicyAppliedPayload` additive 追加可选、versioned `DisclosureBinding`，把 session、participant、purpose、content ref 和 disclosure policy 输入绑定到该次决定；旧事件缺字段按 `None` 读取，但不能授权新的 `Deliver`。
- `AppApiConnector: ExternalProvider` 只声明 provider 边界；真实调用仍经 capabilities lifecycle、policy、Harness、execution backend。
- `EventKind` 不变，`EventKind::ALL` 仍为 86；provider/capability/policy/approval/action/verification/communication/revocation 既有事件表达 M2-B 事实。

## 2. Legacy Decode 与 Replay

- M0/M1/M2-A payload 继续按原 wire 语义读取；M2-B reader 不把未知 enum 解释成相近 backend。
- `disclosure_request` 或 `DisclosurePolicyAppliedPayload.binding` 缺失时可作为 `None` 读取旧 fixture，但任何新的 `Deliver` 在 policy/Harness 执行边界必须 fail closed。
- 历史事件不重写；connector trust、grant、schema、revocation 和 communication budget 从事件与当前 runtime projection 重建，不能由 adapter 私有状态覆盖权威事实。
- 新类型不注册 EventKind migration；additive DTO round-trip、86-kind snapshot 和 M2-A regression contract 共同守住兼容边界。

## 3. 对外执行拒绝边界

AppApi 在 `ActionStarted` 前必须重新确认：

- connector 已 configured/enabled/trusted/granted，且未撤销；新 connector 默认 Untrusted；
- endpoint 位于结构化解析后的 approved base 内，schema digest、SecretRef、timeout、method、scope、permission 和 participant 未漂移；
- mutation 有 bounded idempotency key、rate limit 和明确不可撤销边界；未知结果进入 `ActionOutcomeUnknown`，不得自动重发；
- `Deliver` 的 `DisclosureRequestRef`、允许结果、representation、session、participant、purpose、content ref 和 policy 输入与当前 action spec 精确一致，且 disclosure ingress stamp 绑定到当前 Harness/Gateway 实例；
- approval 绑定包含上述字段的最终 plan digest；项目自带 content resolver 的引用注册后不可覆盖。

## 4. Trust 与凭据

- HTTP/API 返回和外部参与者内容统一以 `Untrusted` receipt/provenance 进入事件链；同一 source/trust/treatment envelope 必须贯通到下一次 `ModelRequest`，并在 action 出口继续按真实 ingress source 治理。它们是数据，不是 owner 或 system 指令。
- credential value 只在 `SecretResolver` 执行边界短暂解析；protocol、event、debug、trace 和验收工件只含 SecretRef，不含 Authorization 或 resolved value。
- project-owned loopback runtime 默认无 credential；通用 AppApi backend 支持 SecretRef，但 resolver、policy 和 connector identity 必须同时匹配。

## 5. Contract Tests

- S43 覆盖 lifecycle、index、schema/credential/endpoint recheck、真实 read/mutation、rate limit、revocation 和 unknown outcome exactly-once。
- S44/S45 覆盖真实 loopback participant、owner-authenticated grant、session budget、disclosure-before-plan、exact binding、instance-bound ingress stamp、representation、owner approval 与对外 ground truth。
- S46 覆盖 device grant、identity、foreground、no-raw persistence、TTL expiry、Gateway provenance 和 owner revocation。
- S47 覆盖 mutual-channel external Agent ingress、purpose drift 和硬终止；内部 subagent 身份不能复用该入口。
