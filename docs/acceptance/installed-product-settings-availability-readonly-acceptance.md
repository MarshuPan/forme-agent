# Product Settings Availability Read-only Guard Acceptance

## Scope

Commit `a6e9a7c` closes a Web boundary in PRD25 §11.10 and PRD26 §5.7: a setting reported as unavailable must not present an editable or executable control. The change is limited to the network setting, whose availability can be `unavailable` when an older installation lacks managed network paths.

## Behavior

- Network controls are enabled only when the authenticated schema-v2 snapshot contains schema-v1 availability and marks `network` as `configurable`.
- Missing availability, a legacy schema, or an unavailable state fails closed. Network fields remain visible but read-only, and the network-application component is not mounted.
- Other supported settings retain their existing save flow. This does not imply LAN readiness, certificate trust, firewall verification, or an applied listener.

## Verification

- Frontend TypeScript, 76 frontend tests, and generated Web bundle verification: PASS.
- `cargo fmt --all -- --check`, `cargo check --workspace --all-targets`, `cargo test --workspace --all-targets`: PASS; 5 existing tests remain ignored by their default external-binary/browser prerequisites.
- Strict Clippy, `py -3 tools/code-quality.py` (689 files; 62 existing size debts), and `bash tools/compliance-doctor.sh`: PASS.
- Build-resource and code-quality checker regression suites: 12/12 and 6/6 PASS.

## Evidence Boundary

No installed service, administrator firewall, real browser session, cross-device certificate trust, or IP09/IP24 product acceptance was performed. This component does not close PRD25, PRD26, or the installed-product milestone.
