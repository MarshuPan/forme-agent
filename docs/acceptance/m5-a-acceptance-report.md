# M5-A Supply-chain Trust Plane 验收报告

- 验收日期：2026-07-22
- 验收范围：M5-A S85-S89，并永久回归 S1-S84
- 权威规格：`docs/requirements/14-m5-verification-strategy.md` S85-S89；`docs/prd/22-m5-program-prd.md` §3
- 结果：**PASS - 可进入 M5-B**

## 1. S85-S89 逐场景证据

| 场景 | 可执行证据 | 事件或状态序列断言 | 反向断言 | 结果 |
|---|---|---|---|---|
| S85 Publisher authority | Harness `s85_s90_owner_admission_plan_and_lifecycle_are_separate_authority_steps`；store `ecosystem_cas_projects_publisher_admission_and_lifecycle_atomically` | owner control 为 `RunAccepted -> SessionBound -> CapabilityPublisherChanged -> RunComplete`；ecosystem CAS 单调推进 | 非 owner、stale CAS、expired grant、key digest mismatch 均零 publisher mutation；event 只保存 public-key digest | PASS |
| S86 Catalog quarantine | Harness `s89_real_registry_read_is_single_approved_untrusted_and_admission_only` | 真实 AppApi read 的 `ActionOutputDelta/ActionCompleted` payload 与 provenance 均为 `Untrusted`；admission 前 registry/toolset 为空 | catalog 内容不能生成 owner、trust、permission、publisher grant 或 active contribution | PASS |
| S87 Digest/signature | capabilities `s87_real_ed25519_verification_is_digest_key_and_grant_bound`；protocol `signed_declarative_package_is_content_addressed_closed_and_tamper_evident` | package/release/body/resource canonical digest 与真实 Ed25519 signature、owner-provisioned key digest、grant scope 精确绑定 | body/manifest/signature/key/package/release/rebind 任一漂移均零 admission | PASS |
| S88 Admission hard checks | capabilities `s88_every_supply_chain_hard_failure_blocks_admission`；protocol `admission_requires_every_hard_check_to_pass` | 13 类 admission check 全部 `Pass` 才构造 `CapabilityPackageAdmission` | dependency gap/cycle/depth、SBOM/license、secret/path、size、executable、risk/network/hook 任一失败均 fail closed | PASS |
| S89 Real registry golden | Harness `s89_real_registry_read_is_single_approved_untrusted_and_admission_only`、`s89_tampered_extra_bytes_and_unapproved_refetch_fail_closed`、`s89_catalog_evidence_cannot_mix_approvals_or_action_intents`；execution redirect contract | `ApprovalRequested -> RunWaiting -> ApprovalResolved -> RunResumed -> ActionPlanned -> ActionStarted -> ActionOutputDelta -> ActionCompleted -> VerificationStarted/Finished{Pass} -> RunComplete`，随后才 `CapabilityPackageAdmitted` | request count=1；redirect、extra bytes、tamper、未审批二次 fetch、混用 approval/intent 均零 admission；receipt digest 精确绑定完整响应正文 | PASS |

## 2. 冻结契约

- `EventKind::ALL.len() == 97`，前 93 项与 M4 exact prefix 同序同名；M5 只在末尾追加四个 Q 事件。
- workspace 保持 18 crates，冻结 trait 与内部依赖方向未改变。
- catalog/package metadata 始终是未信任数据；signature 只证明 key possession，admission 不产生权限、审批或激活。
- package 是声明式 closed bundle；没有 native/WASM/install script、credential、SecretRef、private key、absolute owner path 或隐式 dependency fetch。
- SQLite schema v6 对 legacy M4 store 创建空 ecosystem projection，不合成 Q 事件或 active state。

## 3. 回归与工程门

- `cargo fmt --all -- --check`：PASS。
- `cargo check --workspace --all-targets`：PASS。
- `cargo clippy --workspace --all-targets -- -D warnings`：PASS。
- `cargo test --workspace --all-targets`：PASS；包含 M5 protocol/store/capabilities/harness/execution 正反合同。
- 默认 ignored 的 S38、S68 真实 Browser golden 与 S74/S83 三进程 mutual-TLS golden 显式执行：3/3 PASS。
- 临时 M4 三进程工件经独立 verifier 复核，receipt：`sha256:34fb21187840d03b5e7d9d5013c979ccd65f28b5eea33866c5c8dd0bbe3a924d`。
- `py -3 -m unittest discover -s tools/tests -v`：21/21 PASS。
- `bash tools/compliance-doctor.sh`：upstream/license/copy 全部 PASS。
- `git diff --check`：PASS。

## 4. 结论

M5-A 已证明真实 catalog intake、publisher identity、内容寻址、签名与 admission 被拆成独立且可审计的门。外部响应不能自报 owner/trust/permission，admission 也不能直接激活能力。**M5-A 自验收 PASS，进入 M5-B。**
