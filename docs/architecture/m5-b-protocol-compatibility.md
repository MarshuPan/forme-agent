# M5-B Protocol Compatibility

Status: frozen before M5-B lifecycle and registry activation.

## Plan and approval binding

- `CapabilityInstallPlan` binds operation, exact package/release/digest/admission, scope, policy version, expected ecosystem version, previous release and rollback boundary in one recomputed digest.
- `CapabilityPackageApproval` is configured-owner only, one-shot nonce bound, unexpired and exact-plan bound. Publisher trust, admission, catalog rank or an older approval never substitutes for it.
- The Harness re-reads package archive, admission, publisher status/key binding, policy and ecosystem CAS immediately before committing a lifecycle event.

## Lifecycle table

- Legal first install is `Admitted -> Installed`; admission alone never implies install or registry visibility.
- Enable is `Installed|Disabled -> Enabled`; disable is `Enabled -> Disabled`.
- Update and rollback are `Enabled -> Enabled` with a different exact release, new plan, new approval and incremented generation.
- Revoke is `Admitted|Installed|Enabled|Disabled -> Revoked`; revoked state has no visible contribution.
- `external_effects_reverted` is always false. Rollback and revoke only change governed registry visibility and never rewrite history or claim to undo prior external effects.

## Store, restart and registry

- Q event append, ecosystem version and package-state projection remain one SQLite immediate transaction. Generic `EventStore::append` cannot write Q events.
- Package bytes are content-addressed and restart durable. Rebuild requires the exact archived body, admission, active publisher grant/key digest, current policy and stored package digest; missing or drifted input fails closed before visibility.
- A complete declarative source is staged and validated before the Q state event. Registry replacement is source-atomic; no partial contribution is visible.
- Registry cache is never authority. Restart derives visibility only from store projection plus reverified archived bytes.

## Contribution boundary

- Connector, Plugin, Skill, MCP and AgentProfile descriptors map only into the existing `CapabilityRegistry`; a package does not add an execution backend or private toolset path.
- Package hook output is a proposal. Any resulting action still traverses Harness, ToolsetResolver, Policy/Approval, CompetenceGate and the execution-time recheck.
- AgentProfile is a stateless child-run profile owned by the authority Harness. It has no memory, self-model, policy owner, event writer, spawn network or stable-state write path.

## Legacy and invalid data

- Legacy/admission-only rows decode as no installed state and no active contribution.
- Missing archive, unknown schema, digest/signature/publisher/policy drift, illegal transition, stale CAS, replayed nonce or registry-generation mismatch produces no new visible source.
- Disabled/revoked sources cannot be revived from a process-local cache, old signature, old approval or package self-report.
