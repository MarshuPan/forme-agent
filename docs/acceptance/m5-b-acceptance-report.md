# M5-B Governed Lifecycle 与 Registry 验收报告

- 验收日期：2026-07-22
- 验收范围：M5-B S90-S94，并永久回归 S1-S89
- 权威规格：`docs/requirements/14-m5-verification-strategy.md` S90-S94；`docs/prd/22-m5-program-prd.md` §4
- 结果：**PASS - 可进入 M5-C**

## 1. S90-S94 逐场景证据

| 场景 | 可执行证据 | 事件或状态序列断言 | 反向断言 | 结果 |
|---|---|---|---|---|
| S90 Plan-bound install/enable | Harness `s85_s90_owner_admission_plan_and_lifecycle_are_separate_authority_steps` | `ApprovalRequested -> ApprovalResolved -> CapabilityPackageStateChanged{Installed}`，新计划后再 `ApprovalRequested -> ApprovalResolved -> CapabilityPackageStateChanged{Enabled}`；ecosystem version 单调推进 | admission、publisher grant、旧 approval 均不能激活；release/digest/contribution/scope/policy/rollback/CAS 任一批后漂移均零 registry mutation | PASS |
| S91 Atomic lifecycle/registry | capabilities `s90_s91_registry_staging_is_invisible_and_switches_complete_sources`；Harness `s91_restart_with_enabled_projection_but_missing_archive_fails_closed` | 完整 source 先 staging，Q state event 与 projection 事务提交后才按 generation 原子替换 registry；restart 从 event projection + archived bytes 重建 | staging/descriptor/archive/rebuild 任一失败保留 previous known-good；无 partial contribution、无进程内 cache 复活 | PASS |
| S92 Update/pin/rollback | Harness `s92_s93_update_rollback_and_publisher_revoke_preserve_history_and_fence_visibility` | v1 enabled -> v2 admitted/installed/enabled -> 新 approval rollback 到 v1；v1/v2 admission、archive 与 state history 均保留 | 不接受 floating latest、同 approval 重放或自动 rollback；`external_effects_reverted` 始终为 false，不重放历史外部动作 | PASS |
| S93 Package/publisher revoke | Harness `s92_s93_update_rollback_and_publisher_revoke_preserve_history_and_fence_visibility`、`s91_s97_restart_reverifies_archive_signature_publisher_dependencies_and_policy` | `CapabilityPublisherChanged{Revoked}` 或 `CapabilityPackageStateChanged{Revoked}` 追加后，active source 从后续 registry resolution 消失 | old signature、old approval、local cache、missing archive、publisher/key/policy drift 均不能恢复可见贡献或新 activation/distribution | PASS |
| S94 Hook/agent boundary | capabilities `s94_package_hook_and_agent_profile_remain_declarative_registry_entries`；既有 Harness S5/S11 回归 | hook package 只解析为 `PluginContribution` proposal；agent package 只解析为 stateless `AgentProfile` ref；实际 hook action 继续 `ToolPolicyEvaluated`，child 继续 `SubagentSpawned -> SubagentResultReturned` | package 不能贡献 executable Hook、独立 Harness、memory/self-model/policy owner、event writer 或 spawn network | PASS |

## 2. 冻结契约

- `EventKind::ALL.len() == 97`，前 93 项保持 M4 exact prefix；B 波没有新增或重解释事件。
- workspace 保持 18 crates；冻结 EventStore、ActionBackend、AgentHarness 与内部依赖方向未改变。
- install/enable/update/rollback/revoke 均由 configured owner、immutable plan、one-shot nonce、expiry、current policy 与 ecosystem CAS 共同约束。
- archive/package digest、Ed25519 signature/key binding、publisher status/TTL、dependency admissions 与 current admission policy 在 prepare、apply 和 restart rebuild 均重新验证。
- registry 只接入现有声明式 contribution；enabled 不等于 per-run permission，真实使用仍走 Harness/Policy/Approval/CompetenceGate/final recheck。

## 3. 回归与工程门

- `cargo fmt --all -- --check`：PASS。
- `cargo check --workspace --all-targets`：PASS。
- `cargo clippy --workspace --all-targets -- -D warnings`：PASS。
- `cargo test --workspace --all-targets`：PASS；374 个 listed Rust tests。
- S38、S68 真实 Browser golden 与 S74/S83 三进程 mutual-TLS golden 显式执行：3/3 PASS。
- 本轮临时 M4 三进程工件 digest：`sha256:049f9caac31ca2cf8d33a7f2752c9aa78f1316eb603b698a46a8613999184f94`。
- `py -3 -m unittest discover -s tools/tests -v`：21/21 PASS。
- `bash tools/compliance-doctor.sh`：upstream/license/copy 全部 PASS。
- `git diff --check`：PASS。

## 4. 结论

M5-B 已证明 package admission、owner approval、lifecycle state、registry visibility 与实际 capability 使用是五个独立门。Update/rollback/revoke 追加历史并原子切换可见性，不扩大 permission、不声称撤销既有外部效果。**M5-B 自验收 PASS，进入 M5-C。**
