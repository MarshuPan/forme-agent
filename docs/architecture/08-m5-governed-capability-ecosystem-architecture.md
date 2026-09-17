# M5 受治理能力生态架构

本文实现 `requirements/13-m5-scope-decisions.md` 与 `14-m5-verification-strategy.md`。它是 M0-M4 架构之上的 additive 增量：18-crate 图、Harness-first、单 authority EventStore、执行前重查、能力门在出口、candidate/stable/active/permission 分离、M2 对外动作治理、M3 run-pinned strategy 与 M4 lease/fencing 全部继续有效。

M5 激活 canonical §27。Catalog、publisher 与 package 只扩大能力来源，不产生新的 trust/permission/authority。首版 package 是声明式 closed bundle，不执行 package 内代码。

## 0. 状态与冻结基线

- 冻结基线：M4 封板时的 main、18 crates、93 EventKinds、S1-S84、350 个常规 Rust tests + 3 个显式 real goldens、21 个 Python gates、M4 artifact receipt `sha256:c24a84ef85d918654258a3108a5bf8bd5523361271e76dfdad529ce106319022` 与 release receipt `sha256:4ce2cc4b13ddfd0b15404ad7f6503347ec970f15f25afb25035da239d83239e2`。
- M5-A 建立 publisher/keyring、bundle/admission 和真实 registry intake；没有 A gate 不激活 install/update/distribution。
- M5-B 建立 lifecycle/CAS 与现有 registry 的原子切换；没有 B gate 不允许 remote distribution。
- M5-C 复用 M4 remote lease/fencing 做受限分发，完成真实多进程 golden、typed artifacts 与 release audit。
- 不新增内部 crate 或 dependency edge；冻结 EventStore/ActionBackend/Harness trait 不改签名。增量通过 protocol DTO、store companion trait、capabilities service、Harness/Gateway companion control 接入。

## 1. 架构目标

```text
Untrusted catalog bytes
 -> governed AppApi read receipt
 -> quarantine
 -> publisher grant + digest + signature + policy admission
 -> immutable admitted release
 -> plan-bound owner approval
 -> ecosystem CAS state change
 -> existing capability/provider registry
 -> normal Harness/Policy/CompetenceGate use
 -> optional M4 plan/lease/fence-bound distribution
 -> authority verification + typed artifact
```

必须同时成立：

1. valid signature 不生成 trust/permission/install。
2. admission 不生成 active contribution。
3. enabled contribution 不绕过 per-run toolset/policy/approval。
4. package state 只由 authority EventStore 写入。
5. catalog/package text 永远是数据，不是指令。
6. remote receipt 不自证 installed；unknown 不盲重试。

## 2. Crate 与依赖边

M5 保持 18-crate 图：

| crate | M5 增量职责 | 依赖约束 |
|---|---|---|
| `protocol` | package/publisher/admission/lifecycle/distribution DTO；Q 组 4 events。 | 无新内部依赖。 |
| `store` | ecosystem aggregate CAS、publisher/admission/state/distribution projection、owner nonce/recovery ledger。 | 仍只依赖 protocol。 |
| `capabilities` | keyring、Ed25519 verifier、admission pipeline、declarative package registry 与 contribution adapter。 | 仍依赖 protocol/policy；可增加 exact external crypto dependency。 |
| `policy` | package plan 参数和 risk/scope/permission recheck helper。 | 不依赖 capabilities/store。 |
| `harness` | owner control choke point、plan/approval/CAS、registry apply、M4 distribution composition。 | 使用既有依赖，不新增 edge。 |
| `gateway` | owner-authenticated publisher/admit/install/update/revoke/distribute control/read API。 | 仍只经 Harness companion trait。 |
| `eval` | M5 typed artifact writer/verifier 与 supply-chain checks。 | 不执行 install。 |
| `config` | publisher public-key refs、catalog profile、package limits、doctor checks；secret 不进 config report。 | 不做 key generation。 |
| `execution` | catalog fetch 继续使用 AppApi；remote distribution 继续使用 RemoteExecutorBackend。 | 不新增 package 私有 backend。 |

Ed25519 verification 使用成熟 crate 的公开 API。Private signing key 只出现在 repository-owned test fixture 进程内，绝不进入 runtime config/event/artifact。

## 3. Protocol 增量

### 3.1 Refs、enums 与 aggregate

```rust
pub struct CapabilityPackageRef(pub String);
pub struct CapabilityReleaseRef(pub String);
pub struct CapabilityPublisherRef(pub String);
pub struct CapabilityPublisherGrantRef(pub String);
pub struct CapabilityAdmissionRef(pub String);
pub struct CapabilityPolicyRef(pub String);
pub struct CapabilityInstallPlanRef(pub String);
pub struct CapabilityDistributionReceiptRef(pub String);
pub struct EcosystemAggregateRef(pub String);
pub struct PackageSignature(pub String); // ed25519:<hex>, public material

pub struct EcosystemAggregateVersion {
    pub schema_version: SchemaVersion,
    pub value: u64,
}

pub enum CapabilityPackageKind {
    Connector,
    Plugin,
    Skill,
    McpServer,
    AgentProfile,
}

pub enum CapabilityPublisherStatus { Active, Revoked }
pub enum CapabilityLifecycleState {
    Quarantined,
    Admitted,
    Installed,
    Enabled,
    Disabled,
    Revoked,
}
pub enum CapabilityPackageOperation {
    Install,
    Enable,
    Disable,
    Update,
    Rollback,
    Revoke,
    Distribute,
}
pub enum CapabilityAdmissionVerdict { Pass, Fail, Unverifiable }
pub enum CapabilityAdmissionCheckKind {
    Schema,
    Size,
    ClosedSet,
    Path,
    Digest,
    Publisher,
    Signature,
    Dependency,
    Sbom,
    License,
    Secret,
    Risk,
    Policy,
}
```

All refs are non-empty；digest uses `sha256:<64 hex>`；aggregate version begins at zero and only authority CAS advances it。

### 3.2 Publisher、manifest 与 signed bundle

```rust
pub struct CapabilityPublisherGrant {
    pub schema_version: SchemaVersion,
    pub reference: CapabilityPublisherGrantRef,
    pub publisher: CapabilityPublisherRef,
    pub public_key_digest: SchemaDigest,
    pub allowed_kinds: Vec<CapabilityPackageKind>,
    pub scope: Scope,
    pub expires_at: Timestamp,
    pub version: Version,
    pub status: CapabilityPublisherStatus,
}

pub struct CapabilityPackageDependency {
    pub schema_version: SchemaVersion,
    pub package: CapabilityPackageRef,
    pub release: CapabilityReleaseRef,
    pub version: Version,
    pub digest: SchemaDigest,
}

pub struct CapabilityContributionDescriptor {
    pub schema_version: SchemaVersion,
    pub kind: CapabilityPackageKind,
    pub capability: CapabilityRef,
    pub payload_digest: SchemaDigest,
    pub required_permissions: Vec<PermissionRef>,
    pub risk: Risk,
    pub network: bool,
    pub hook: bool,
}

pub struct CapabilityPackageManifest {
    pub schema_version: SchemaVersion,
    pub package: CapabilityPackageRef,
    pub release: CapabilityReleaseRef,
    pub version: Version,
    pub kind: CapabilityPackageKind,
    pub publisher: CapabilityPublisherRef,
    pub scope: Scope,
    pub contributions: Vec<CapabilityContributionDescriptor>,
    pub dependencies: Vec<CapabilityPackageDependency>,
    pub sbom_digest: SchemaDigest,
    pub license_expression: String,
    pub body_digest: SchemaDigest,
    pub max_unpacked_bytes: u64,
    pub contains_executable: bool,
}

pub struct CapabilityPackageResource {
    pub schema_version: SchemaVersion,
    pub relative_path: String,
    pub content: String,
    pub digest: SchemaDigest,
}

pub struct SignedCapabilityPackage {
    pub schema_version: SchemaVersion,
    pub manifest: CapabilityPackageManifest,
    pub resources: Vec<CapabilityPackageResource>,
    pub package_digest: SchemaDigest,
    pub signature: PackageSignature,
}
```

`package_digest` is `canonical_digest((manifest, resource path+digest ordered list))`; signature signs the raw 32-byte SHA-256 digest. `resources` must be sorted by normalized path, unique case-insensitively, closed by contribution payload refs, UTF-8, and below policy size. `contains_executable=true` always fails M5 admission.

### 3.3 Admission、plan 与 lifecycle

```rust
pub struct CapabilityAdmissionPolicy {
    pub schema_version: SchemaVersion,
    pub reference: CapabilityPolicyRef,
    pub version: Version,
    pub allowed_kinds: Vec<CapabilityPackageKind>,
    pub allowed_licenses: Vec<String>,
    pub max_package_bytes: u64,
    pub max_dependencies: u32,
    pub max_depth: u32,
    pub allow_network: bool,
    pub allow_hooks: bool,
}

pub struct CapabilityAdmissionCheck {
    pub schema_version: SchemaVersion,
    pub kind: CapabilityAdmissionCheckKind,
    pub verdict: CapabilityAdmissionVerdict,
    pub evidence: EvidenceRef,
}

pub struct CapabilityPackageAdmission {
    pub schema_version: SchemaVersion,
    pub reference: CapabilityAdmissionRef,
    pub package: CapabilityPackageRef,
    pub release: CapabilityReleaseRef,
    pub package_digest: SchemaDigest,
    pub publisher_grant: CapabilityPublisherGrantRef,
    pub publisher_version: Version,
    pub policy: CapabilityPolicyRef,
    pub policy_version: Version,
    pub checks: Vec<CapabilityAdmissionCheck>,
    pub dependencies: Vec<CapabilityPackageDependency>,
    pub admitted_at: Timestamp,
}

pub struct CapabilityInstallPlan {
    pub schema_version: SchemaVersion,
    pub reference: CapabilityInstallPlanRef,
    pub operation: CapabilityPackageOperation,
    pub package: CapabilityPackageRef,
    pub release: CapabilityReleaseRef,
    pub package_digest: SchemaDigest,
    pub admission: CapabilityAdmissionRef,
    pub scope: Scope,
    pub policy: CapabilityPolicyRef,
    pub policy_version: Version,
    pub expected_version: EcosystemAggregateVersion,
    pub previous_release: Option<CapabilityReleaseRef>,
    pub rollback_boundary: RollbackBoundary,
    pub digest: PlanDigest,
}

pub struct CapabilityPackageApproval {
    pub schema_version: SchemaVersion,
    pub approval: ApprovalId,
    pub plan_digest: PlanDigest,
    pub principal: VerifiedPrincipal,
    pub nonce: Nonce,
    pub expires_at: Timestamp,
}

pub struct CapabilityPackageStateChange {
    pub schema_version: SchemaVersion,
    pub plan: CapabilityInstallPlanRef,
    pub approval: ApprovalId,
    pub package: CapabilityPackageRef,
    pub release: CapabilityReleaseRef,
    pub from: CapabilityLifecycleState,
    pub to: CapabilityLifecycleState,
    pub active_generation: u64,
    pub reason: ReasonRef,
    pub external_effects_reverted: bool,
}
```

`CapabilityInstallPlan::validate` recomputes digest and validates legal transitions. `external_effects_reverted` must always be false for rollback/revoke. Approval principal must equal configured owner；nonce/plan digest/expiry one-shot semantics live in store ledger。

### 3.4 Distribution

```rust
pub struct CapabilityPackageDistributionReceipt {
    pub schema_version: SchemaVersion,
    pub reference: CapabilityDistributionReceiptRef,
    pub package: CapabilityPackageRef,
    pub release: CapabilityReleaseRef,
    pub package_digest: SchemaDigest,
    pub peer: FederatedPeerRef,
    pub peer_grant: FederatedPeerGrantRef,
    pub authority_epoch: AuthorityEpoch,
    pub plan_digest: PlanDigest,
    pub lease: RemoteExecutionLeaseRef,
    pub fence_token: u64,
    pub installed_generation: u64,
    pub ground_truth: EvidenceRef,
    pub verified: RequiredTrue,
}
```

Receipt validates exact M4 binding and never contains endpoint/path/SecretRef/key. Worker transport response is converted to this object only after authority verifier checks an independent executor package ledger/probe.

### 3.5 Q 组事件与 97-kind prefix

```rust
CapabilityPublisherChanged {
    grant: CapabilityPublisherGrant,
    previous: Option<CapabilityPublisherGrantRef>,
    committed_version: EcosystemAggregateVersion,
}
CapabilityPackageAdmitted {
    admission: CapabilityPackageAdmission,
    committed_version: EcosystemAggregateVersion,
}
CapabilityPackageStateChanged {
    change: CapabilityPackageStateChange,
    committed_version: EcosystemAggregateVersion,
}
CapabilityPackageDistributionRecorded {
    receipt: CapabilityPackageDistributionReceipt,
    committed_version: EcosystemAggregateVersion,
}
```

They append strictly after `ReplicationCheckpointAdvanced`; `EventKind::ALL.len() = 97`. The first 93 names/order/payload mapping are exact M4 prefix. Fetch/progress/rejection use existing Action/Verification/Failure events or typed return values, not new stable facts.

## 4. M5-A Publisher 与 Admission

### 4.1 PublisherKeyring

`PublisherKeyring` is authority-local configuration:

```rust
pub trait PublisherKeyring: Send + Sync {
    fn public_key(&self, publisher: &CapabilityPublisherRef)
        -> Result<PublisherPublicKey>;
}
```

The keyring verifies `sha256(public_key) == grant.public_key_digest`. It never writes keys into Event/Debug/trace. Grant lifecycle is event-sourced；key bytes are replaceable config, and missing/mismatch makes publisher unavailable rather than inferred。

### 4.2 Admission verifier

```rust
pub trait CapabilityPackageVerifier: Send + Sync {
    fn verify(
        &self,
        package: &SignedCapabilityPackage,
        grant: &CapabilityPublisherGrant,
        policy: &CapabilityAdmissionPolicy,
        admitted_dependencies: &[CapabilityPackageAdmission],
        now: Timestamp,
    ) -> Result<CapabilityPackageAdmission>;
}
```

Implementation produces all required checks. Only all-Pass returns admission. Failure does not partially cache an authoritative admission. Quarantine bytes may remain in a bounded ephemeral/content cache but are not event facts and are purged independently.

### 4.3 Real catalog intake

Catalog fetch uses existing `AppApiBackend` read with L3 approval because it sends a real external request. Response `ExternalActionReceipt` remains Untrusted, is size/origin bounded, and its content digest must match the package bytes passed to admission. The package verifier never performs network I/O and cannot follow dependency URLs.

## 5. M5-B Lifecycle 与 Registry

### 5.1 Store-first control flow

```text
owner command
 -> authenticate principal
 -> load publisher/admission/current state/policy
 -> build immutable plan + digest
 -> request/validate one-shot approval
 -> re-read publisher/admission/policy/ecosystem version
 -> stage and validate complete contribution source
 -> append Q event + version/projection in one transaction
 -> atomically replace existing registry source
 -> verify resolved toolset projection
```

Registry staging happens before event commit, but no staged contribution is visible. After store commit, registry switch is deterministic and restart-rebuildable from admitted package body. If registry apply fails after store commit, package state projection is marked unavailable and Harness emits failure; it does not fall back to an unrecorded in-memory state. Reference implementation validates/stages all contributions so post-commit apply is infallible apart from poisoned lock/process failure.

### 5.2 Declarative contribution adapter

`CapabilityContributionDescriptor` maps to existing `CapabilityDescriptor`:

| Package kind | Existing registry view |
|---|---|
| Connector | `Capability::AppApi(ProviderId)` metadata only；actual connector must be pre-supported or separately configured。 |
| Plugin | `Capability::PluginContribution(PluginContributionRef)`；no dynamic code。 |
| Skill | `Capability::Skill(SkillRef)`；body loaded from admitted bounded resource。 |
| McpServer | `Capability::McpTool(McpToolRef)` metadata；executable/transport comes from managed local config, not bundle。 |
| AgentProfile | `Capability::AgentProfile(AgentProfileRef)`；Harness-only child-run profile。 |

Enabled package source trust is `ApprovedSource`, not `OwnerInput` or `VerifiedProcess`. Actual output remains Untrusted until action verifier records result evidence.

### 5.3 Update/rollback/revoke

- Update stages exact new release alongside old, validates full source, then CAS switches active generation.
- Failure before switch leaves old release/source untouched and records `FailureEvidence`.
- Rollback requires a new plan/approval and targets an already admitted/installed known-good release.
- Publisher revoke cascades future package availability by projection; active package contributions are removed on the same controlled reconciliation pass. It never deletes historical bundle/admission/events.

## 6. M5-C Federated Distribution

### 6.1 M4 composition

Distribution is represented as an existing Remote action whose typed inner operation targets a repository-owned executor package receiver. The immutable M4 placement/lease includes M5 package/release/digest in the operation digest. Harness final recheck verifies:

1. package admission and non-revoked publisher/package state;
2. owner package approval plan/nonce/expiry;
3. active Executor grant, scope/capability, epoch/fence and local executor profile;
4. receiver schema and policy compatibility;
5. no credential/key/private path in payload.

The executor receiver is a thin data-plane service with no catalog search, model, cognition, policy owner or event writer. It validates digest/signature/publisher binding against provisioned public data, writes a content-addressed declarative package record once, and returns a closed receipt. It cannot enable or use the package unless a separate authority-approved target state permits it.

### 6.2 Idempotency and unknown

Executor ledger key = `(peer, package, release, digest, dispatch_id)`；same semantics returns original receipt, different semantics fails. Authority claims remote dispatch before network. Timeout after claim produces `ActionOutcomeUnknown`; recovery only fetches original receipt or probes the exact content-addressed record. New install is a new plan/approval/lease and cannot pretend the previous effect did not occur.

### 6.3 Revoke fence

Publisher/package revoke or authority epoch rotation blocks new distribution before driver. It cannot prove already distributed bytes were erased. Optional removal is a separate plan-bound action with receipt honesty identical to M4 retention.

## 7. Store companion traits

```rust
pub trait EcosystemProjection {
    fn publisher(&self, publisher: &CapabilityPublisherRef)
        -> Result<Option<CapabilityPublisherGrant>>;
    fn admission(&self, release: &CapabilityReleaseRef)
        -> Result<Option<CapabilityPackageAdmission>>;
    fn package_state(&self, package: &CapabilityPackageRef)
        -> Result<Option<CapabilityPackageState>>;
    fn snapshot(&self, scope: Scope) -> Result<CapabilityEcosystemSnapshot>;
}

pub trait EcosystemEventStore: EventStore {
    fn ecosystem_version(&self, aggregate: &EcosystemAggregateRef)
        -> Result<EcosystemAggregateVersion>;
    fn append_ecosystem_expected(
        &self,
        event: Event,
        aggregate: &EcosystemAggregateRef,
        expected: EcosystemAggregateVersion,
    ) -> Result<ExpectedAppend>;
}

pub trait EcosystemRuntimeLedger {
    fn claim_ecosystem_nonce(
        &self,
        nonce: &Nonce,
        plan: &PlanDigest,
    ) -> Result<bool>;
    fn record_distribution_attempt(
        &self,
        receipt: &CapabilityPackageDistributionReceipt,
    ) -> Result<bool>;
    fn distribution_receipt(
        &self,
        reference: &CapabilityDistributionReceiptRef,
    ) -> Result<Option<CapabilityPackageDistributionReceipt>>;
}
```

New SQLite tables are separate `ecosystem_*` domain tables. Q event append + version + publisher/admission/state/distribution projection is one immediate transaction. The trait does not overload M2 run CAS, M3 evolution CAS or M4 federation CAS.

Legacy M4 DB migration creates empty tables with version zero and no synthetic events/state. Unknown Q schema or event/version mismatch makes ecosystem unavailable while local M0-M4 runs remain governed.

## 8. Harness/Gateway companion control

```rust
pub trait EcosystemGatewayControl: Send + Sync {
    fn provision_capability_publisher(...) -> Result<CapabilityPublisherGrant>;
    fn admit_capability_package(...) -> Result<CapabilityPackageAdmission>;
    fn prepare_capability_change(...) -> Result<CapabilityInstallPlan>;
    fn apply_capability_change(...) -> Result<CapabilityPackageState>;
    fn distribute_capability_package(...) -> Result<CapabilityPackageDistributionReceipt>;
    fn capability_ecosystem_snapshot(...) -> Result<CapabilityEcosystemSnapshot>;
}
```

Gateway only authenticates/binds owner and forwards typed commands. It never verifies signatures, mutates registry, resolves approval from payload self-report or appends events. Read endpoints redact key material and package body.

Harness creates owner-control audit runs with `RunAccepted -> SessionBound -> Q event -> RunComplete`; validation failure before authoritative mutation writes zero Q event and returns typed error. Where a real fetch/distribution action occurs, the normal Action/Approval/Verification sequence is linked by evidence refs.

## 9. Security、failure 与 observability

| Condition | Required result |
|---|---|
| catalog prompt injection / owner impersonation | quarantine, no grant/admission/state; `safety_policy_failure` evidence when applicable。 |
| wrong key/digest/signature/rebind | no admitted event；typed admission failure。 |
| dependency gap/cycle/confusion | no admission/fetch fallback。 |
| unknown/incompatible license or missing SBOM | blocking admission/release finding。 |
| secret/private key/path marker | reject package/artifact；do not echo matching value。 |
| install plan/approval/policy/CAS drift | zero registry mutation and zero Q event。 |
| contribution staging failure | previous active source retained。 |
| package/publisher revoke | no new resolve/enable/distribute；history retained。 |
| remote timeout after dispatch | ActionOutcomeUnknown；driver count remains one。 |
| worker receipt mismatch | failure evidence before any verified distribution fact。 |

ConfigDoctor adds `ConfigCheck::Ecosystem` and checks keyring/grants/catalog allowlist/package limits/admission policy/executor receiver. Reports contain only refs/digests, never public key bytes when unnecessary and never private key/credential.

DecisionTrace references the ecosystem snapshot/plan/admission/package release used by the run. Package recommendation rationale may be observable, but raw catalog text and model hidden reasoning are not audit facts.

## 10. Typed artifacts and release audit

M5 closed artifact set has exactly five files: publisher、admission、install、distribution、trace. Each is schema-versioned and content-addressed; cross-refs bind package/release/digest/publisher grant/policy/approval/ecosystem version/peer/epoch/lease/fence/ground truth/event stream。

Portable artifacts are deterministic evidence projections, not serialized runtime DTO snapshots. The golden first validates the complete runtime grant/admission/envelope/authority receipt/executor record and their exact bindings. It then omits volatile TTL/admission timestamps, transport-identity-derived grant refs, outer remote receipt ids and other run-instance material; the artifact retains stable refs/versions, semantic digests, counts and required-true verification markers. A semantic digest never replaces the runtime check that produced it, and no projected object may be fed back into lifecycle or execution APIs.

Independent verifier rejects unknown fields where the schema is closed, duplicate semantic ids, extra/missing files, digest mismatch, path escape, raw key/private material, secret/SecretRef/private endpoint/owner path markers and event order violations.

M5 release audit verifies the historical M4 receipt as an immutable artifact, then generates/compares a new current-tree receipt. It covers dependency exact pins, licenses/NOTICE, cargo-audit, borrowing records, copy detector, secret scan, 18-crate graph, 97-kind prefix and M5 threat fixtures.

### 10.1 RustSec warning classification

The release audit consumes the typed `cargo-audit` JSON result; it does not
convert every non-zero scanner exit into an undifferentiated blocking finding.
Each advisory or warning is independently checked against the frozen release
target `x86_64-pc-windows-gnu` with a locked dependency-tree query.

- A target-reachable vulnerability, `unsound` warning, or `yanked` package is
  blocking and must be remediated before release.
- A target-unreachable advisory or warning remains in the typed receipt as an
  observation; it is not ignored, and it must be re-evaluated when the release
  target or enabled features change.
- `unmaintained` and `notice` warnings are retained as non-blocking
  observations; they do not constitute a claim that the dependency is safe or
  suitable indefinitely.
- An unavailable scanner, non-JSON result, unknown warning kind, malformed
  warning/advisory payload, or failed reachability query is blocking
  (`fail-closed`). There is no blanket warning allowlist.

## 11. Implementation order and gates

1. Freeze requirements/13-14、canonical §27、本文、architecture/03 Q taxonomy and prd/22。
2. M5-A：protocol compatibility/types/events -> store ecosystem CAS -> capabilities keyring/verifier/admission -> Harness/Gateway intake -> S85-S89/artifacts。
3. Run S1-S89、97-kind、18-crate、fmt/check/strict clippy/all tests/Python/compliance/real registry golden；write `acceptance/m5-a-acceptance-report.md`。
4. M5-B：lifecycle plan/approval/nonce -> registry atomic switch -> update/rollback/revoke -> agent/hook boundary -> S90-S94；write `m5-b-acceptance-report.md`。
5. M5-C：M4-bound distribution -> executor ledger/process -> unknown recovery -> S95-S99 -> artifacts/release audit；write `m5-c-acceptance-report.md` and final report。

Each wave adds `architecture/m5-*-protocol-compatibility.md` with exact prefix、serde defaults、legacy/open-store、invalid/replay contracts. Any catalog self-trust、signature-as-permission、install without owner approval、partial registry switch、hidden executable、peer self-install、unknown retry、secret/key leak or EventKind reorder fails the wave immediately。
