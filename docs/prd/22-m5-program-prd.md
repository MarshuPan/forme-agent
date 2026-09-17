# PRD 22：M5 受治理能力生态实施计划

本文是 M5 实施级总 PRD。它承接 `requirements/13-m5-scope-decisions.md`、`14-m5-verification-strategy.md`、`architecture/canonical-contract.md` §27、`architecture/08-m5-governed-capability-ecosystem-architecture.md` 和冻结的 M0-M4 PRD。实现不得在模块内重定义 publisher/package/admission/lifecycle/distribution，也不得因为接入真实 catalog 而放松任何治理不变量。

## 0. 定位与状态

- **冻结基线**：M4 封板时的 main、18 crates、93 EventKinds、S1-S84、M4 typed artifacts/release receipt、canonical §1-26。
- **M5 additive 目标**：97 EventKinds；所有 M5 DTO versioned；无内部 crate/edge 变化；历史前 93 strict prefix。
- **需求**：D96-D115。
- **验收**：S85-S99。
- **交付节奏**：A/B/C 三波；每波实现、全回归、typed evidence、验收报告后进入下一波。Owner 已指示整里程碑连续实施，因此以逐波自验收替代人工停顿，最终仍需完整 owner 验收。
- **当前状态**：M5-A/B/C 已实现；S1-S99、97 EventKinds、18 crates、真实 registry/authority/executor golden、typed artifacts 与 clean-tree release audit 工程验收 PASS。工程结果已合入 main；历史 acceptance report 保留验收当时的分支状态。

## 1. 做与不做

### 1.1 做

- M5-A：publisher grant/keyring、signed declarative package、catalog quarantine、admission pipeline、真实 registry golden。
- M5-B：plan-bound lifecycle/CAS、existing registry atomic switch、update/rollback/revoke、hook/agent-profile boundary。
- M5-C：M4-bound remote distribution、executor package ledger、unknown recovery、real authority/registry/executor golden、artifact/release gate。

### 1.2 不做

- 不做多租户/public marketplace/payment/reputation/self-service publisher。
- 不做 native/WASM/container/install script/build-from-source 或 package 内任意 executable。
- 不做多主/CRDT/gossip/P2P package spread、peer self-install 或 Replica writer。
- 不做自动 install/update/publish/trust/permission/grant/envelope、standing L5。
- 不做独立 Agent cognition/team/spawn network、package memory/strategy/policy owner。
- 不自研 crypto/dependency solver，不复制第三方 marketplace schema/source/fixture/error text。

## 2. 全局集成契约

### 2.1 Crate owner

| 增量 | Owner crate | Consumer |
|---|---|---|
| M5 DTO/EventKind | protocol | all event consumers |
| ecosystem CAS/projection/runtime ledger | store | harness/gateway/eval |
| keyring/signature/admission/package contribution adapter | capabilities | harness |
| package control choke point | harness | gateway/tests |
| owner HTTP control/read | gateway | owner clients |
| typed artifacts/verifier | eval | verify-m5/release audit |
| config/doctor | config | runtime/operator |
| real catalog read/remote transport | existing execution AppApi/Remote | harness |

### 2.2 Frozen seams

- `EventStore`、`VersionedEventStore`、`EvolutionEventStore`、`FederationEventStore`、`ActionBackend`、`AgentHarness` signatures unchanged。
- M5 adds `EcosystemProjection/EventStore/RuntimeLedger` companion traits；aggregate newtypes cannot mix with run/evolution/federation versions。
- capabilities does not append events/store state；it verifies/stages declarative sources and returns typed results。
- store only accepts authority-constructed Q events and validates event/payload/version/transition agreement。
- Gateway only binds authenticated owner/channel and calls Harness companion control；no direct registry/store mutation。
- Remote distribution reuses M4 placement/lease/fence/unknown path；package score/catalog rank cannot authorize peer or install。

### 2.3 Event mainlines

Publisher:

```text
RunAccepted -> SessionBound
 -> CapabilityPublisherChanged{Active|Revoked, committed_version}
 -> RunComplete
```

Admission:

```text
governed AppApi read -> ActionCompleted -> VerificationFinished{Pass}
 -> quarantine -> all hard checks Pass
 -> CapabilityPackageAdmitted{admission, committed_version}
```

Lifecycle:

```text
ApprovalRequested -> ApprovalResolved
 -> final grant/admission/policy/CAS recheck
 -> CapabilityPackageStateChanged{Installed|Enabled|Disabled|Revoked}
 -> registry switch -> toolset verification
```

Distribution:

```text
ActionPlanned -> Approval -> CompetenceGate
 -> RemoteExecutionLeaseChanged{Acquired}
 -> ActionStarted -> remote receipt/probe
 -> authority Verification
 -> CapabilityPackageDistributionRecorded
 -> RemoteExecutionLeaseChanged{Released}
```

Unknown:

```text
ActionStarted -> ActionOutcomeUnknown -> RunWaiting
 -> original receipt / read-only probe / owner disposition
```

## 3. M5-A：Supply-chain Trust Plane

### A0 Compatibility

1. Add `architecture/m5-a-protocol-compatibility.md` before Rust DTO activation。
2. Freeze 93 -> 97 exact prefix, serde defaults, invalid/legacy/open-store semantics。
3. Add protocol tests for validation/digest/signature envelope/tamper and event payload round-trip。

### A1 Protocol/Store

1. Add `protocol/src/m5.rs` and new string refs/enums/DTOs exactly per architecture/08 §3。
2. Append Q events to event macro and mirror store `PayloadType` exact mapping。
3. Add `ecosystem_*` schema tables, projections and CAS transaction；legacy M4 DB produces version zero/empty projection。
4. Reject wrong Q event, version gap, invalid transition, duplicate semantic id with different digest and unknown schema。

### A2 Capabilities verifier

1. Add exact-pinned Ed25519 dependency and borrowing/license records。
2. Implement in-memory authority public-key keyring; public key digest must match owner grant。
3. Implement closed bundle canonical digest, signature verification, path/size/duplicate/executable/secret checks。
4. Implement exact dependency DAG checks, SBOM/license/risk/network/hook policy checks。
5. Return admission only if every required check is `Pass`; never emit active registry entry。

### A3 Harness/Gateway intake

1. Add `EcosystemHarnessRuntime` and `EcosystemGatewayControl` companion。
2. Owner-only publisher provision/update/revoke writes audit run + Q event via store CAS。
3. Admission consumes a verified content receipt/digest from governed AppApi read；raw catalog bytes remain Untrusted。
4. Add redacted snapshot/read endpoint; never return key bytes/package body/private endpoint。

### A4 S85-S89 and artifacts

- Positive/negative protocol/store/capabilities/harness tests。
- Real loopback HTTP registry fetch exactly once through governed action path。
- Generate typed publisher/admission evidence and secret/key/path scan。

### M5-A gate

S1-S89 + 97-kind/prefix + 18 crates + fmt/check/strict clippy/all tests/Python/compliance + real registry admission golden。Write `docs/acceptance/m5-a-acceptance-report.md`。

## 4. M5-B：Governed Lifecycle 与 Registry

### B0 Compatibility

- Add `architecture/m5-b-protocol-compatibility.md`；freeze plan/approval/lifecycle defaults and transition table。
- Legacy/admission-only data never imply Installed/Enabled。

### B1 Immutable plan and approval

1. Build install plan from current admission/policy/state/ecosystem version；compute digest over all authority-relevant fields。
2. Require configured owner、one-shot nonce、expiry and exact plan digest。
3. Re-read publisher/admission/policy/CAS immediately before mutation；drift writes zero state/registry change。
4. Claim nonce durably before applying state change；replay fails across restart。

### B2 Declarative package registry

1. Implement `CapabilityPackageRegistry` backed by existing `InMemoryCapabilityRegistry` source replacement。
2. Stage complete source, validate all descriptors/refs/permissions/risk/scope and resource digests before visibility。
3. On Enabled atomically replace package source; on Disabled/Revoked remove it。
4. Add `Capability::AgentProfile` and keep profile data stateless/Harness-only。
5. Map plugin/skill/MCP/connector contributions to existing registry types without dynamic code/loading private backend。

### B3 Update、rollback and revoke

1. Update installs immutable v2 alongside v1 then switches active generation by CAS。
2. Any staging/verification failure retains v1 and records failure evidence。
3. Rollback requires new plan/approval and never sets `external_effects_reverted=true`。
4. Publisher/package revoke removes active source and invalidates pending plans/distribution。

### B4 Hook/agent boundary

- Package hook output remains proposal and forces existing execution-time policy recheck。
- AgentProfile may limit role/model/toolset/permission/budget/DoneContract only；no memory/cognition/policy/event writer。
- Child run still emitted through `SubagentSpawned/ResultReturned` and cannot write parent stable state。

### B5 S90-S94

Protocol transition tests、store CAS/restart tests、registry atomic switch tests、Harness approval/drift/revoke tests、hook/agent negative tests。

### M5-B gate

S1-S94 + A gate + lifecycle restart + atomic registry projection + compliance。Write `docs/acceptance/m5-b-acceptance-report.md`。

## 5. M5-C：Federated Distribution 与 Release

### C0 Compatibility

- Add `architecture/m5-c-protocol-compatibility.md`；freeze distribution receipt and M4 binding/default behavior。
- Legacy remote receipt or self-reported installed state cannot produce M5 distribution fact。

### C1 Distribution composition

1. Add repository-owned declarative package receiver to executor process/profile。
2. Encode exact package/release/digest as typed Remote inner operation or closed receiver request without host path/credential。
3. Harness final recheck package admission/revoke/policy/owner approval plus M4 peer/epoch/grant/lease/fence。
4. Executor rechecks digest/signature/schema/local policy and writes content-addressed package ledger once。

### C2 Receipt and unknown recovery

1. Executor ledger returns original receipt for duplicate identical dispatch and rejects semantic mismatch。
2. Authority claim occurs before network；timeout becomes ActionOutcomeUnknown。
3. Recovery only reads original receipt/probes exact ledger record；same lease never invokes install twice。
4. Authority verifier compares receipt binding and independent ledger ground truth before Q distribution event。

### C3 Real golden

- Three independently launched roles: loopback registry、authority Harness/store、executor receiver。
- Signed package fetched once、admitted、installed/enabled、used through normal policy、distributed once、revoked。
- Assertions: registry request=1、authority generation=1、executor install=1、post-revoke use/distribution=0、non-authority append=0、secret/private-key/path matches=0。

### C4 Artifacts and release

1. Add eval M5 five-artifact writer/verifier and CLI verifier。
2. Add `tools/m5_release_audit.py` and negative fixtures for tamper/signature/license/SBOM/secret/private path/extra file/copy/advisory/dirty tree。
3. Add `tools/verify-m5-workspace-contract.ps1` for 18-crate graph、exact dependency pins、97-kind prefix and expected binaries。
4. Add `tools/verify-m5.ps1` composing M4 permanent regression, S85-S99, real golden, artifacts, fmt/check/clippy/tests/Python/compliance/release audit。

### C5 S95-S99

- remote authorization/fence、unknown no retry、restart/idempotency、real E2E、supply-chain release threat tests。

### M5-C/final gate

Success requires the exact final line from requirements/14 §7 and both `m5-c-acceptance-report.md` / `m5-acceptance-report.md`。

## 6. Error classification and user-visible results

| Failure | FailureTaxonomy | User-visible result |
|---|---|---|
| catalog injection/owner impersonation | `safety_policy_failure` | quarantined/rejected；no trust/install。 |
| signature/digest/path/secret/license failure | `safety_policy_failure` | admission blocked with check category, no sensitive echo。 |
| dependency gap/cycle | `resource_selection_failure` | exact missing/conflicting refs, no fallback fetch。 |
| approval/policy/permission failure | `trust_failure` / `safety_policy_failure` | denied/expired/drift; no registry mutation。 |
| contribution staging/registry verification | `execution_failure` / `verification_failure` | previous release retained。 |
| remote timeout/receipt mismatch | `execution_failure` / `verification_failure` | unknown/probe or failed verification; no blind retry。 |
| self-evaluation says safe while hard check fails | `self_eval_trap` | hard result dominates。 |

## 7. Test strategy

### 7.1 Protocol/Store

- 93 -> 97 exact prefix/string/payload/serde round trip。
- DTO validation: empty/version/digest/signature/path/dependency/transition/plan/receipt negative matrix。
- ecosystem CAS, atomic projections, legacy open, replay, idempotency and restart。

### 7.2 Capabilities/Harness/Policy

- real Ed25519 valid/tamper/wrong key/rebind。
- admission hard checks and content quarantine。
- owner/nonce/expiry/plan drift/publisher revoke/package revoke。
- registry all-or-nothing switch and existing toolset/policy recheck。
- agent profile no cognition and hook final recheck。

### 7.3 Federation/Golden

- active Executor only；Replica/wrong scope/stale epoch/fence rejected before network。
- executor install count=1 under timeout/restart/duplicate receipt。
- real loopback registry + independent authority/executor process ground truth。

### 7.4 Artifacts/Compliance

- closed five-file set、content address、cross-ref、event order、secret/key/path scan。
- exact dependency pins、license/NOTICE、cargo-audit、borrowing record、copy detector、dirty tree。

## 8. Acceptance and commit gates

Every implementation commit runs at minimum:

```powershell
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
bash tools/compliance-doctor.sh
```

Wave/final gates additionally run `tools/verify-m5.ps1` in the activated range. Reports must name executable test、event subsequence、store assertion、negative assertion、artifact digest and residual boundary。

Final definition:

- S1-S99 green；97 EventKinds；18 crates；all listed Rust tests + explicit real goldens + Python gates。
- catalog bytes remain Untrusted；valid signature/admission never authorize by themselves。
- install/update/rollback/revoke/distribute always owner/plan/CAS bound。
- no package code execution、secret/key/path leakage、peer self-install、unknown retry or independent Agent cognition。
- M5 typed artifacts and release receipt independently verifiable offline。

## 9. Originality and dependencies

- Package manifest、admission model、fixture、errors、catalog protocol and tests are repository-owned；do not copy third-party marketplace schema or wording。
- Ed25519 and HTTP dependencies are used only through public API, exact pinned and recorded in `docs/compliance/third-party-dependencies.md` and `compliance/borrowing-records.json`。
- No upstream source/example/fixture/prompt/README enters build or M5 artifacts。
- Release audit must cover dependency license、NOTICE、RustSec advisory and source/copy boundary before PASS。RustSec JSON findings are classified per target: reachable vulnerability/unsound/yanked results block；unreachable results remain typed observations；unmaintained/notice remain non-blocking observations；scanner、schema、warning-kind 或 reachability 不确定性 fail closed。
