# M5-C Protocol Compatibility

Status: frozen before M5-C federated distribution implementation.

## Compatibility boundary

- The first 93 `EventKind` values remain an exact M4 prefix. M5 keeps the four
  additive Q events at the end and does not add a fetch, install-progress, or
  worker-self-report event.
- `EventStore`, `FederationEventStore`, `ActionBackend`, `AgentHarness`, and
  the M4 remote transport traits keep their existing signatures.
- Distribution is an M4 `Remote` action whose inner operation is a bounded
  `File::Write` carrying a versioned declarative package envelope. It is not a
  package-specific execution backend.

## Distribution envelope

The authority-to-executor payload is the following closed, non-fact DTO:

```text
CapabilityPackageDistributionEnvelope {
  schema_version,
  install_plan,          // operation = Distribute, exact package/release/digest
  package,               // signed declarative bytes, no credentials or paths
  admission,             // authority result, never trusted by itself
  dependency_admissions, // exact closed dependency admissions, already on peer
  publisher_grant,       // public-key digest and scope only
  authority_policy,      // policy snapshot used for the local recheck
  target_peer,
  authority_epoch,
  content_digest,
}
```

The envelope is `serde(deny_unknown_fields)`, schema-versioned, and its
`content_digest` is the canonical digest of all fields except the digest
itself. The package is independently validated on the executor. The executor
compares the supplied grant and policy references to its provisioned local
configuration; a supplied grant or policy cannot provision trust or permission.
The envelope contains no endpoint, host path, `SecretRef`, credential, or
private-key material. A package with dependencies is accepted only when every
dependency is already present in the executor's content-addressed ledger; no
network fallback is allowed.

## Executor ledger record

The executor persists a second closed DTO for each accepted package:

```text
CapabilityExecutorInstallRecord {
  schema_version,
  peer,
  package,
  release,
  package_digest,
  envelope_digest,
  installed_generation,
  record_digest,
}
```

The bundle bytes and record are written content-addressedly before the remote
driver returns. Existing identical `(package, release, package_digest)` data
returns the original record; a different semantic envelope for the same
identity fails closed. The record is executor-local ground truth and is not an
authority event. The outer M4 replay ledger still owns dispatch idempotency;
therefore a timeout or process restart never invokes package install a second
time under the same lease.

## Admission and local policy

Before writing the ledger, the receiver rechecks:

1. envelope schema, canonical content/package digest, closed resource paths and
   package size;
2. exact publisher grant, public-key digest, TTL, signature and allowed scope;
3. every dependency against already admitted local ledger records;
4. SBOM/license/secret/risk/network/hook policy using the provisioned local
   policy; and
5. target peer, authority epoch, capability and the bounded logical receiver
   path.

Any missing or uncertain input yields no record and no success receipt. The
executor cannot enable the package, select a package, write authority events,
spawn an agent, or resolve credentials.

## Receipt and unknown semantics

- Authority claims the M4 dispatch before network I/O. A transport timeout after
  the claim is `ActionOutcomeUnknown` and enters the existing `RunWaiting` path.
- Recovery fetches the original driver receipt or performs a read-only probe of
  the exact lease. It never sends another install under that lease and never
  changes peer to hide an unknown outcome.
- Only after M4 authority verification and an independent ledger observation may
  Harness append `CapabilityPackageDistributionRecorded`. The Q receipt binds
  package/release/digest, peer/grant, epoch, capability-install plan digest,
  remote lease, fence, installed generation, and ground-truth evidence.
- Revoke or epoch rotation fences a not-yet-started distribution. It does not
  claim that bytes already observed on a peer were erased.

## Legacy and replay

- A legacy M4 store opens with an empty M5 distribution projection.
- Unknown envelope/record schema, digest mismatch, semantic identity conflict,
  stale epoch, wrong peer role, missing archive, or missing local dependency is
  a fail-closed rejection; no guessed upcast is permitted.
- Reopening an executor ledger preserves its record digest and generation.
  Replaying the same receipt is idempotent; changing package, release, digest,
  lease, fence, or envelope semantics is rejected.

## Portable artifact projection

The real golden validates the full distribution envelope, executor ledger
record, authority receipt, peer-grant binding, remote-plan binding, lease and
fence before artifact generation. The five-file offline artifact is a stable
evidence projection and is never accepted as a runtime DTO. It excludes
volatile TTL/admission timestamps, generated transport identities, outer
remote receipt ids and their derived digests. Stable semantic digests bind the
package/release/digest, publisher/policy refs, capability plan, peer, epoch,
lease, fence and executor generation; required-true fields record that the
omitted runtime bindings were checked. Two independent executions of the real
golden must therefore produce a content-equal artifact set.
