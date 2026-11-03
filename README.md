# forme

A persistent, governed personal agent written in Rust.

forme brings memory, cognition, coordination, and proactive work into one
owner's persistent runtime. It learns from task results and feedback, keeps
project context across sessions, and makes actions subject to explicit scope,
permissions, approval, and verification.

The product and architecture contracts live under [docs/](docs/README.md).

## Capabilities

- **Persistent memory and cognition:** event-backed memory, UserModel,
  AgentSelfModel, PartnershipModel, and scoped cognitive maps. New understanding
  passes through candidates, evidence review, promotion, and retraction.
- **Long-running projects:** workspaces, project charters, goals, intentions,
  restart recovery, and coordinated child runs share the same authoritative brain.
- **Governed proactivity:** background observation, reflection, and consolidation
  can produce suggestions and follow-up work. Value, competence, and permission
  checks govern when they become actions.
- **Tools and extensions:** file, shell, browser, computer, MCP, skills, plugins,
  and App API adapters use the common execution and audit path. Availability
  depends on configured providers, grants, and platform support.
- **Owner control:** trace inspection, approvals, cancellation, candidate review,
  data lifecycle controls, and a browser workbench expose the runtime's state
  through authenticated interfaces.

## Current status

Status as of **2026-11-03**:

| Area | State and evidence |
|---|---|
| Core runtime | M0-M5 and V1 core-brain closure are implemented and merged into `main`. Recorded technical Final runs cover S1-S99, C1-C24, configured-model workflows, and release audits. Exact inputs and limits are in the [closure report](docs/acceptance/core-brain-closure-acceptance-report.md). |
| Installed product | Service hosting, authenticated setup and configuration, managed workers, release maintenance, packaging, and the React workbench have component implementations. Windows also has local installation and service observations. Joint product acceptance remains in progress; see the [implementation record](docs/acceptance/installed-product-implementation-progress.md). |
| Distribution | Windows development bundles, native installer candidates, and local signing evidence exist. Candidates remain `release_ready=false`; native release acceptance for Linux/macOS remains open. See the [platform acceptance strategy](docs/requirements/18-installed-product-verification-strategy.md). |

The installed Windows workflow has recorded owner login, configuration
acknowledgement, and signed-release staging. An upgrade interrupted before
backup or switching remains under explicit recovery work; see the
[upgrade and recovery record](docs/acceptance/installed-product-windows-upgrade-20261102-db96ec2-plan.md).
BrainReady, real provider execution through the installed product, installed
worker/helper behavior, upgrade/rollback, and complete IP01-IP24 acceptance still
need joint evidence. A successful source check or an available Web page does not
close those items.

Core technical acceptance is also distinct from owner daily-use acceptance and
months-long cognitive benefit. Reports retain their tested commit and evidence
scope rather than asserting that every later revision has passed every gate.

## Architecture

forme keeps **one owner, one authoritative brain, and one event store** across
CLI, Gateway, scheduler, and workspace activity. Child agents are execution
units; they do not own independent cognitive state.

The existing 18-crate workspace contains three cooperating kernels:

| Kernel | Responsibility |
|---|---|
| Cognitive evolution | Turn observations, verified results, and feedback into reviewed memory and cognition. |
| Coordination | Select resources and execution routes against goals, context, capabilities, and completion criteria. |
| Proactivity | Notice relevant changes, manage intentions, and propose useful work within the owner's authority. |

All formal runtime entry points compose the same `ProductionRuntime` and enforce
governance through the Harness. Permissions are rechecked before execution;
trust does not grant permission, and an unknown side effect is never blindly
retried. The browser uses authenticated APIs rather than calling models, tools,
or the brain database directly. Installation and service management have their
own journal and authorization domain.

Start with the [canonical contract](docs/architecture/canonical-contract.md),
[core runtime architecture](docs/architecture/09-v1-core-brain-runtime-closure-architecture.md),
and [installed-product architecture](docs/architecture/10-installed-product-and-web-workbench-architecture.md).

## Build from source

The current local development workflow uses Windows PowerShell and the GNU
Windows Rust toolchain. Platform release targets are tracked separately in the
[product scope](docs/requirements/17-installed-product-scope-decisions.md).

Requirements:

- Rust stable with `rustfmt` and Clippy. `rust-toolchain.toml` selects
  `stable-x86_64-pc-windows-gnu` for this checkout.
- MinGW-w64: `gcc` and `ar` on `PATH` for bundled SQLite.
- Python 3 (`py -3` on Windows), PowerShell, and Bash for repository checks.
- Node.js **22.12 or newer** and npm for frontend development and verification.
  Installed products serve embedded static assets and do not require Node.js.
- `cargo-audit 0.22.2` for the milestone release audits.

From the repository root:

```powershell
py -3 tools/build_resources.py check
cargo check --locked --workspace --all-targets
```

Run the unified source-quality gate:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File tools/verify-code-quality.ps1
```

It checks crate boundaries and file-size rules, Rust formatting, all workspace
targets and tests, strict Clippy, frontend dependencies/types/tests, embedded
assets, packaging boundaries, release verification, and originality/license
compliance. Native installation and IP acceptance are separate gates.
Contribution rules and required implementation checks are in [AGENTS.md](AGENTS.md)
and the [code-quality guide](docs/engineering/code-quality.md).

### Build space

Development and test builds use `debug = "line-tables-only"` and
`incremental = false`. Reuse the fixed `target` directory for each worktree;
avoid creating a new timestamped cache for each check.

```powershell
py -3 tools/build_resources.py check
py -3 tools/build_resources.py report --all-worktrees --json
```

The default pre-build free-space requirement is 20 GiB, and the per-worktree
cache reporting budget is 64 GiB. Reporting does not delete artifacts. Cache
maintenance uses an explicit target preview and an idle maintenance window;
preserve runtime data, release packages, and acceptance evidence. See the
[build-resource guide](docs/engineering/build-resources.md).

### Frontend development

The workbench uses React, TypeScript, and Vite. Build it into the exact embedded
asset set with the repository tool:

```powershell
npm ci --prefix frontend --ignore-scripts
node tools/build-web.mjs
npm --prefix frontend run verify
```

Frontend changes must include the generated asset inventory, manifest, and
license metadata. Reuse is limited to the reviewed files and terms in the
[frontend reuse policy](docs/compliance/frontend-reuse-policy.md).

## Development CLI and Gateway

The source-development CLI uses a Chat Completions-compatible HTTPS endpoint.
Supply credentials through the local environment or a secret manager rather
than committing them to repository files. Required settings are:

```text
FORME_MODEL_BASE_URL=https://provider.example/v1
FORME_MODEL_NAME=model-id
FORME_MODEL_API_KEY=your-local-secret
FORME_OWNER_ID=owner-id
```

Optional settings include `FORME_MODEL_PROVIDER`, `FORME_MODEL_CONTEXT_WINDOW`,
`FORME_MODEL_TIMEOUT_MS`, and `FORME_STORE_PATH`. `FORME_CAPABILITY_CONFIG` names
a schema-versioned JSON document with `skills`, `mcp`, `plugins`, and `app_api`
lists; it references credential environment-variable names, not secret values.
Capabilities require explicit owner trust and grant bindings.

One runtime owns a store at a time. Start the development Gateway, which owns
that runtime and its scheduler:

```powershell
cargo run -p forme-gateway --bin forme-gatewayd
```

It prints `FORME_GATEWAY_URL` and the bearer-token file path. With
`FORME_GATEWAY_URL` set, and either `FORME_GATEWAY_TOKEN` or the printed token
file available, the CLI sends work to that daemon:

```powershell
cargo run -p forme-cli -- "your question"
```

When the daemon is stopped, `--embedded` composes the same runtime in-process.
`--manifest` prints its composition manifest digest:

```powershell
cargo run -p forme-cli -- --embedded "your question"
cargo run -p forme-cli -- --manifest
```

These are source-development entry points. Installed services use protected,
versioned configuration and the authenticated product UI. Model configuration,
backup/restore, deletion, and evidence operations are described in the
[core data and evidence runbook](docs/runbooks/v1-core-brain-data-and-evidence.md).

## Milestones and evidence

| Stage | Delivered scope | Acceptance record |
|---|---|---|
| M0 | Governed core loop, event sourcing, policy, memory, and execution. | [M0](docs/acceptance/m0-acceptance-report.md) |
| M1 | Gateway, scheduler, proactive follow-up, compaction, and capability lifecycle. | [M1](docs/acceptance/m1-acceptance-report.md) |
| M2 | Browser/computer/PTY, connectors, resource graph, long-term goals, and hot/cold memory. | [M2](docs/acceptance/m2-acceptance-report.md) |
| M3 | Controlled evolution, replay/evaluation, trust, promotion, activation, and rollback. | [M3](docs/acceptance/m3-acceptance-report.md) |
| M4 | Single-authority federation, remote execution, read-only replicas, and checkpoint handoff. | [M4](docs/acceptance/m4-acceptance-report.md) |
| M5 | Signed declarative packages, governed lifecycle, and authority-bound distribution. | [M5](docs/acceptance/m5-acceptance-report.md) |
| V1 core closure | Default persistent composition, continuous cognition, workspace orchestration, and model/data closure. | [V1 closure](docs/acceptance/core-brain-closure-acceptance-report.md) |
| Installed product | Native services, setup/authentication, browser workbench, workers, and release maintenance; in progress. | [Product progress](docs/acceptance/installed-product-implementation-progress.md) |

V1 closure and installed-product work do not introduce an M6. The frozen graph
remains 18 crates and 99 brain EventKinds; installation, authentication, and
service facts remain separate from brain events.

Milestone and closure gates are maintained in `tools/verify-m0.ps1` through
`tools/verify-m5.ps1` and `tools/verify-core-brain-closure.ps1`. Some gates require
configured models, browsers, and clean-tree release evidence; follow their
reports and runbook rather than treating them as ordinary offline tests.

## License

forme is licensed under the [MIT License](LICENSE). Project and third-party
attributions are retained in [NOTICE](NOTICE); dependencies retain their own
license terms.
