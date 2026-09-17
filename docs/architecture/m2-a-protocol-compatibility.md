# M2-A Protocol Compatibility Note

本文固定 M2-A 对 `forme-protocol` 的 additive 变更、legacy decode、安全默认和 replay 边界。它承接 `05-m2-cross-system-collaboration-architecture.md` 与 `prd/19`，不得被用于放松 canonical §24。

## 1. 增量

- `BackendKind` 追加 `Browser`、`Computer`、`Pty`；`AppApi` 留 M2-B additive activation。
- `ActionParameters` 追加对应 versioned action spec。
- `SecretRef` 成为既有 `CredentialRef` 的 wire-compatible canonical 名；新增 `ExternalInput`、Browser/Computer/PTY action DTO、`SecretBinding` 和 external receipt/ref DTO。
- `ActionOutputDeltaPayload` 追加 `trust` 与 `content_ref`。
- `ActionCompletedPayload` 追加 optional external receipt；receipt 随权威事件持久化。
- `ConfigCheck` 追加 Browser、Computer、Pty；Connector/Sync 分别留 M2-B/C。
- `EventKind` 不变，`EventKind::ALL` 仍为 86；既有 action/policy/approval/verification/capability 事件表达 M2-A 事实。

## 2. Legacy decode

- 旧 `BackendKind`、`ActionParameters` 和 `ConfigCheck` 序列值保持原语义与编码，不重排、不重命名。
- 旧 `ActionOutputDeltaPayload` 缺少 `trust/content_ref` 时，读时安全默认 `Untrusted/None`；旧 `ActionCompletedPayload` 缺 receipt 时默认 `None`。默认 Untrusted 只收紧输入解释，不改变历史 action terminal 状态。
- 新 backend/action spec 的 `schema_version=0`、unknown enum、空 driver/surface/program、无 allowlist/root、非法 URL scheme、无效 coordinate/size 或 raw secret binding 必须 decode/validation fail closed。

## 3. Replay

- 历史事件不可重写；M2 reader 可读取 M0/M1 action output，并按安全默认呈现来源。
- Replay snapshot 继续记录 event schema、policy、model、tool schema；M2 real-world eval 额外在 profile/ref 中记录 driver profile 与 action schema digest，不把 driver binary path、endpoint credential 或 ResolvedSecret 写入事件。
- M2-A 不注册 EventKind migration；payload 增量用 serde default + contract test 证明兼容。

## 4. 执行拒绝边界

Legacy decode 成功不等于可执行。Browser/Computer/PTY 在执行前必须重新确认：

- backend 与 action parameters 一致；
- capability、permission、scope、origin/surface/program/root 仍有效；
- plan digest、policy version、tool/action schema 和 SecretRef 未漂移；
- canonical §24 的 L3/L5/CompetenceGate/envelope 条件满足；
- driver 已显式配置并受当前 runtime allowlist 管理。

任何缺失或不一致均在 `ActionStarted` 前拒绝。

## 5. Contract tests

- 所有新 DTO round-trip，invalid/unknown/zero boundary fail closed。
- M1 action payload 在 M2 reader 中保持可读。
- 缺少 output trust 的 legacy JSON 读为 Untrusted。
- 缺少 completion receipt 的 legacy JSON 保持可读；M2 external completion 必须携带 receipt。
- M2 payload 不能由 M1 reader 误解释成其他 backend；unknown enum 必须显式拒绝。
- EventKind taxonomy snapshot 保持 86。
