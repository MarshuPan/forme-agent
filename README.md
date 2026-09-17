# forme

forme is an open-source proactive personal-agent kernel written in Rust. The
authoritative product and architecture documents live under `docs/`.

## Project status

**forme is under active development and is not yet ready for packaging or
everyday use.** Milestones M0-M5 are implemented and have passed their
acceptance gates. The current program, V1 core-brain runtime closure, composes
those modules into one persistent default brain in four waves (production
composition, continuous cognition, workspace orchestration, product closure).
The project becomes packageable only after all four waves pass C1-C24 and the
closure release gate.

### Milestone history

M0 is complete. The first governed core loop is
running across 18 workspace crates with event sourcing, policy and approval
enforcement, memory and cognition, coordination, proactivity, communication,
verification, and originality controls on the same harness-first path.

The M0 completion gate covered:

- 164 Rust tests plus 8 compliance-doctor fixtures;
- scenario acceptance S1-S22 with event-order and negative assertions;
- a real configured-model run persisted as a 13-event audited trace; and
- blocking upstream-isolation, license, and copy-detection checks.

See the [M0 acceptance report](docs/acceptance/m0-acceptance-report.md) for the
scenario evidence. Re-run the complete gate from PowerShell:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools/verify-m0.ps1
```

M1-A is complete: the same Harness runtime now has
a loopback Gateway daemon, authenticated local Web console, async run control,
SSE cursor pages, trace/candidate review, and manual eval.

M1-B is complete: event-backed intentions now
drive a leased scheduler, restart recovery, foreground priority, governed
proactive follow-up, and plan-bound local notifications through the same
Harness path.

M1-C is complete: threshold compaction, scoped
memory review, skill 按需加载, deferred MCP schemas with
execution-time rechecks, and isolated plugin reloads now run on the same
governed path. The current gate has 203 Rust tests plus 9 compliance fixtures
and keeps S1-S37 green. See the [M1-A acceptance report](docs/acceptance/m1-a-acceptance-report.md),
[M1-B acceptance report](docs/acceptance/m1-b-acceptance-report.md),
[M1-C acceptance report](docs/acceptance/m1-c-acceptance-report.md), and
[M1 final acceptance report](docs/acceptance/m1-acceptance-report.md). Re-run the
complete current gate with:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools/verify-m1.ps1
```

M1 final passed on 2026-07-15. A real configured model completed the
repository-owned `case:m1-final-only` through Gateway, Harness, verification,
and an 18-event authoritative trace. The exported typed report and portable
trace manifest are retained under `docs/acceptance/`; no credential or endpoint
is stored in either artifact.

M2 final passed on 2026-07-17 after three owner-reviewed delivery waves. Governed
Browser/Computer/PTY, AppApi and real communication adapters, ResourceGraph,
long-term goals, evidence-led capability growth, managed plugins, hot/cold
memory, and one-peer CAS sync all remain on the existing Harness/event-store
path. The gate keeps 18 crates and 86 EventKinds, runs 252 non-ignored Rust tests
plus one real Chrome/Edge golden, and keeps S1-S52 and all originality checks
green. See the [M2-A report](docs/acceptance/m2-a-acceptance-report.md),
[M2-B report](docs/acceptance/m2-b-acceptance-report.md),
[M2-C report](docs/acceptance/m2-c-acceptance-report.md), and
[M2 final report](docs/acceptance/m2-acceptance-report.md).

Re-run the complete M2 gate with:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools/verify-m2.ps1
```

M3 final passed and was sealed after three owner-reviewed waves. Portable replay,
effect-deny simulation, evidence-led evaluation, separate promotion and
activation, run-pinned domain strategies, StrategyMemory, AgentSelf and
Partnership evolution, asymmetric trust/delegation, governed proactivity and
append-only rollback now form one controlled-evolution loop. The frozen graph
remains 18 crates and 89 EventKinds. The gate runs 309 non-ignored Rust tests,
two real Chrome/Edge goldens, S1-S69, 19 Python compliance/release gate tests, strict Clippy,
content-addressed artifacts, and a clean-tree license/dependency/RustSec audit.
See the [M3-C acceptance report](docs/acceptance/m3-c-acceptance-report.md),
[M3 final acceptance report](docs/acceptance/m3-acceptance-report.md), and
`docs/requirements/09-m3-scope-decisions.md`,
`docs/requirements/10-m3-verification-strategy.md`,
`docs/architecture/06-m3-controlled-evolution-architecture.md`, and
`docs/prd/20-m3-program-prd.md`.

Re-run the complete M3 gate with:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools/verify-m3.ps1
```

M4 passed owner acceptance and was sealed after repairing the
Browser completion race exposed by the first unified owner gate. The
single-owner federated runtime keeps one authoritative Harness and EventStore
while adding mutually authenticated remote execution, finite read-only
replicas, independently authenticated cross-device owner control, and verified
checkpoint handoff. The frozen graph remains 18 crates and the first 89
EventKinds remain an exact prefix of the additive 93-kind taxonomy. The gate
runs 350 non-ignored Rust tests plus three explicit real-process goldens,
S1-S84, 21 Python compliance/release tests, strict Clippy, a three-process
mutual-TLS golden, five content-addressed federation artifacts, and a clean-tree
license/dependency/RustSec/federation threat audit. See the
[M4 final acceptance report](docs/acceptance/m4-acceptance-report.md) and
the frozen requirements, architecture, and PRD in
`docs/requirements/11-m4-scope-decisions.md`,
`docs/requirements/12-m4-verification-strategy.md`, canonical section 26,
`docs/architecture/07-m4-federated-runtime-architecture.md`, and
`docs/prd/21-m4-program-prd.md`.

Re-run the complete M4 gate with:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools/verify-m4.ps1
```

M5 passed its engineering final gate and is merged. The
governed capability ecosystem adds signed content-addressed declarative
packages, fail-closed admission, owner/plan/CAS-bound lifecycle changes,
existing-registry integration, and M4-bound distribution without adding a
second authority. The frozen graph remains 18 crates and the exact 93-kind M4
prefix is followed by four additive events, for 97 total. The gate covers
S1-S99, 379 non-ignored Rust tests plus four explicit real goldens, 29 Python
gates, deterministic five-file ecosystem artifacts, strict Clippy, and a
clean-tree release audit. See the
[M5 final acceptance report](docs/acceptance/m5-acceptance-report.md),
`docs/requirements/13-14`, canonical section 27,
`docs/architecture/08-m5-governed-capability-ecosystem-architecture.md`, and
`docs/prd/22-m5-program-prd.md`.

Re-run the complete M5 gate with:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools/verify-m5.ps1
```

The next program is **V1 core-brain runtime closure**, not M6. Its frozen
document chain is `docs/requirements/15-16`, canonical section 28,
`docs/architecture/09-v1-core-brain-runtime-closure-architecture.md`, and
`docs/prd/23-v1-core-brain-closure-program-prd.md`. It composes the existing
M0-M5 modules into one persistent default brain in four waves: production
composition, continuous cognition, long-running workspace/orchestration, and
model/data/value closure. The target remains 18 crates with the existing 97
EventKinds as an exact prefix followed only by two closure events. Implementation
is in progress; completion requires S1-S99 plus C1-C24 and the closure release
gate.

## Build requirements

- Rust stable with the GNU Windows host, selected automatically by
  `rust-toolchain.toml`.
- Install MinGW-w64. Both `gcc` and `ar` must be available on `PATH` for bundled
  SQLite.
- Bash is required for the compliance gate.
- `cargo-audit 0.22.2` is required for the M3-M5 release audits.

Verify the environment and repository from the workspace root:

```text
rustup show active-toolchain
gcc --version
ar --version
cargo check
cargo test
bash tools/compliance-doctor.sh
cargo audit --version
```

## CLI model configuration

The reactive CLI uses a Chat Completions-compatible HTTPS endpoint. Configure
the runtime without putting credentials in repository files. A configured
owner identity is required for the persistent brain:

```text
FORME_MODEL_BASE_URL=https://provider.example/v1
FORME_MODEL_NAME=model-id
FORME_MODEL_API_KEY=secret-value
FORME_OWNER_ID=owner-id
```

Optional settings are `FORME_MODEL_PROVIDER`, `FORME_MODEL_CONTEXT_WINDOW`,
`FORME_MODEL_TIMEOUT_MS`, and `FORME_STORE_PATH`. Run a question through the
same Gateway/Harness protocol used by other entries:

```text
cargo run -p forme-cli -- "your question"
```

## License

forme is licensed under [Apache License 2.0](LICENSE). Required project and
third-party attributions are retained in [NOTICE](NOTICE).
