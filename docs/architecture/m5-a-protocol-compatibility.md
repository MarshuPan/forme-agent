# M5-A Protocol Compatibility

Status: frozen before M5-A Rust activation.

## Event prefix

- M4 `EventKind::ALL` first 93 entries remain exact in name, order, string form and payload mapping.
- M5 appends exactly four Q events in this order: `CapabilityPublisherChanged`、`CapabilityPackageAdmitted`、`CapabilityPackageStateChanged`、`CapabilityPackageDistributionRecorded`。
- `EventKind::ALL.len() = 97`; no existing event is repurposed。

## DTO and serde

- All M5 DTOs carry non-zero `SchemaVersion` and reject empty refs, zero versions and malformed `sha256:` digests。
- Optional compatibility fields use `#[serde(default)]`; absence means the historical event carries no M5 authority and never grants install/distribution。
- Package release identity is exact `(package, version, digest)`; same id with different semantics is invalid rather than last-write-wins。
- `PackageSignature` is public signature material only; private key has no protocol type or serialization path。

## Legacy/open-store

- Opening a valid 93-kind M4 store creates empty `ecosystem_*` tables with aggregate version zero and no synthetic Q event/publisher/admission/state/distribution。
- M0-M4 replay remains byte/semantic compatible; M5 projection ignores non-Q events except immutable refs explicitly validated by authority control。
- Unknown M5 schema/version gap/digest mismatch disables ecosystem operations fail-closed while local M0-M4 runs remain available。

## Invalid contracts

- Catalog/publisher self-report never constructs `VerifiedPrincipal` or publisher grant。
- Signature pass without active matching grant/policy/dependency/license/SBOM checks cannot produce `CapabilityPackageAdmission`。
- Admission cannot deserialize as Installed/Enabled and does not populate toolset。
- Q event payload kind mismatch, wrong committed version or illegal lifecycle transition writes zero rows。
- M5 aggregate versions cannot be passed to run/evolution/federation CAS traits。

## Replay and artifacts

- Replay order uses authority `stream_seq`, not catalog timestamp/package version/download time。
- Artifacts store refs/digests/check verdicts, not package body/catalog text/key bytes/credential/private endpoint/owner path。
- The repository tests preserve exact 93-prefix and assert legacy open before M5-A is accepted。
