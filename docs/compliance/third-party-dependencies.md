# Third-party dependencies

## Windows installation access helpers (2026-10-22)

`packaging/windows/install-access.ps1` and `service-access.psm1` are project-authored
scripts using the OS-provided Windows PowerShell 5.1/.NET `System.Security.AccessControl`
public APIs; PowerShell 7 is an additional component-test host. They redistribute no
Microsoft implementation, new runtime package, upstream script or fixture, and add
no Cargo/npm dependency. Inno Setup compiler pinning, installer licensing/signing and
native service-account acceptance remain release work, not established by these tests.

Development native compilation (2026-10-23) used an isolated, non-redistributed
`Tools.InnoSetup 6.6.1` NuGet wrapper from
`https://api.nuget.org/v3-flatcontainer/tools.innosetup/6.6.1/tools.innosetup.6.6.1.nupkg`,
SHA256 `9c994256f6fb3ed7f28e9637b97a3a328c2a3d1d5ba57537ac6b7dcc414ddbe2`.
The ISCC executable is Authenticode-valid (Pyrsys B.V.), independently pinned to
`f8deb3e9e54303c178fd3513357ebc4d580a63bd072e24c221442b06f884b8fc`.
The package license and tool usage were reviewed; no tool implementation, example
or installer script was copied. Tool licensing/neighbor-file pinning and official
distribution provenance must be reviewed again for production release; this
development compile is neither a native package signature nor an installation.

## Web workbench dependency review (2026-10-21)

The workbench uses project-authored React/TypeScript components; no DeepSeek source,
server, plugin, prompt or fixture is imported. The limited frontend reuse permission
does not expand to other material. Exact full build inventory and licenses are
generated at `crates/gateway/assets/product/dist/dependencies.json` from the npm
lockfile. Runtime license texts accompany the embedded assets in `LICENSES.txt`.

| Package | Exact version | License | Scope |
|---|---:|---|---|
| react / react-dom | 19.3.1 | MIT | Browser runtime; complete license texts shipped |
| scheduler | 0.28.0 | MIT | Transitive browser runtime; complete license text shipped |
| typescript | 7.0.2 | Apache-2.0 | Build only |
| vite | 8.3.5 | MIT | Build only |
| @vitejs/plugin-react | 6.1.2 | MIT | Build only |
| @types/react / @types/react-dom | 19.3.0 | MIT | Build only |
| @types/node | 24.10.1 | MIT | Build only |
| lightningcss and platform packages | As locked in full inventory | MPL-2.0 | Unmodified build dependencies only, not authored or embedded implementation |

Installation uses `npm ci --ignore-scripts`; no dependency source is a copying
reference. Build tooling refuses licenses outside its reviewed allowlist and
records platform optional packages as well. npm advisory review remains a separate
current-tree check; a successful bundle does not establish native product acceptance.

## Rust dependency review

Installed HTTPS ingress (2026-10-22) adds exact `tokio-rustls 0.26.6`
(MIT OR Apache-2.0, crates.io, https://github.com/rustls/tokio-rustls),
with default features disabled and only ring/tls12 enabled. It reuses patched
`rustls 0.23.46` and `rustls-pki-types 1.15.0` through their public APIs;
`rcgen 0.14.8` remains test-only for locally generated identities. No source,
example, certificate or fixture is copied. Runtime supports TLS 1.2/1.3,
disables early data, and bounds handshake work. This dependency review is not
a claim of browser certificate trust, firewall acceptance or native installation.
The locked dependency audit and NOTICE remain release gates.

forme 的项目许可证为 MIT，以根 `LICENSE` 和 `Cargo.toml` 为准。下表记录第三方组件自身的实际许可证；它们不会随项目许可证变更，NOTICE 与相应归属义务继续保留。历史发布回执保留原输入，当前发布必须重新审计。

| Package | Version | Source | Repository | License | Copy risk | Implementation rule | Copied material | Release action |
|---|---:|---|---|---|---|---|---|---|
| serde | 1.0.228 | [crates.io](https://crates.io/crates/serde) | [serde-rs/serde](https://github.com/serde-rs/serde) | MIT OR Apache-2.0 | Low | Public-API-only implementation | No copied code, fixture, or naming | Review LICENSE/NOTICE before release |
| serde_json | 1.0.150 | [crates.io](https://crates.io/crates/serde_json) | [serde-rs/json](https://github.com/serde-rs/json) | MIT OR Apache-2.0 | Low | Public-API-only implementation | No copied code, fixture, or naming | Review LICENSE/NOTICE before release |
| rusqlite | 0.40.1 | [crates.io](https://crates.io/crates/rusqlite) | [rusqlite/rusqlite](https://github.com/rusqlite/rusqlite) | MIT | Low | Public API and build artifact only | No source copied into authored crates | Review LICENSE/NOTICE before release |
| libsqlite3-sys | 0.38.1 (transitive, bundled feature) | [crates.io](https://crates.io/crates/libsqlite3-sys) | [rusqlite/rusqlite](https://github.com/rusqlite/rusqlite) | MIT | Low | Transitive FFI and bundled-build artifact only | No source copied into authored crates | Review LICENSE/NOTICE before release |
| SQLite | 3.53.2 (bundled by libsqlite3-sys) | [sqlite.org](https://www.sqlite.org/) | [SQLite source](https://sqlite.org/src/) | Public domain | Low | Bundled build artifact only | No SQLite source copied into authored crates | Review public-domain notice before release |
| Model Context Protocol specification | 2025-06-18 protocol identifier | [modelcontextprotocol.io](https://modelcontextprotocol.io/) | [modelcontextprotocol/modelcontextprotocol](https://github.com/modelcontextprotocol/modelcontextprotocol) | MIT | Low | Implement the public JSON-RPC protocol only; do not copy SDK source, errors, or fixtures | No copied material; repository-owned stdio fixture | Review specification license and NOTICE before release |
| sha2 | 0.10.9 | [crates.io](https://crates.io/crates/sha2) | [RustCrypto/hashes](https://github.com/RustCrypto/hashes) | MIT OR Apache-2.0 | Low | Public digest API only | No copied code or fixtures | Review LICENSE/NOTICE before release |
| chacha20poly1305 | 0.11.0 | [crates.io](https://crates.io/crates/chacha20poly1305) | [RustCrypto/AEADs](https://github.com/RustCrypto/AEADs) | Apache-2.0 OR MIT | Low | Public AEAD API only behind the project-owned protected-content envelope; key material stays outside events and artifacts | No copied code, examples, fixtures, or naming | Review LICENSE/NOTICE and reachable advisories before release |
| axum | 0.8.9 | [crates.io](https://crates.io/crates/axum) | [tokio-rs/axum](https://github.com/tokio-rs/axum) | MIT | Low | Public routing/extractor/response APIs only | No copied code, examples, fixtures, errors, or naming | Review LICENSE before release |
| tokio | 1.53.3 | [crates.io](https://crates.io/crates/tokio) | [tokio-rs/tokio](https://github.com/tokio-rs/tokio) | MIT | Low | Public runtime, listener, task, and signal APIs only | No copied code, examples, fixtures, errors, or naming | Review LICENSE before release |
| is_elevated | 0.1.2 | [crates.io](https://crates.io/crates/is_elevated) | [yandexx/is_elevated](https://github.com/yandexx/is_elevated) | MIT | Low | Windows-only safe public process-token query for local administrator recovery | No copied source, examples, fixture or layout | Review MIT license and transitive Windows bindings before release |
| rustix | 1.1.4 | [crates.io](https://crates.io/crates/rustix) | [bytecodealliance/rustix](https://github.com/bytecodealliance/rustix) | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | Low | Unix-only safe effective-UID query with std/process features; no command execution | No copied source, examples, fixture or layout | Review enabled features and license before release |
| tokio-rustls | 0.26.6 | [crates.io](https://crates.io/crates/tokio-rustls) | [rustls/tokio-rustls](https://github.com/rustls/tokio-rustls) | MIT OR Apache-2.0 | Low | Public async server TLS API only; defaults disabled, ring/tls12 enabled | No copied source, examples, certificate material, fixtures, or naming | Exact pin; review locked licenses and reachable advisories before release |
| tower-http | 0.7.0 | [crates.io](https://crates.io/crates/tower-http) | [tower-rs/tower-http](https://github.com/tower-rs/tower-http) | MIT | Low | Public timeout middleware API only | No copied code, examples, fixtures, errors, or naming | Review LICENSE before release |
| getrandom | 0.4.3 | [crates.io](https://crates.io/crates/getrandom) | [rust-random/getrandom](https://github.com/rust-random/getrandom) | MIT OR Apache-2.0 | Low | Public operating-system entropy API only | No copied code, examples, fixtures, errors, or naming | Review LICENSE/NOTICE before release |
| headless_chrome | 1.0.22 | [crates.io](https://crates.io/crates/headless_chrome) | [rust-headless-chrome/rust-headless-chrome](https://github.com/rust-headless-chrome/rust-headless-chrome) | MIT | Low | Public CDP client API only, behind the project-owned `BrowserDriver` boundary | No copied code, examples, fixtures, errors, flags list, or naming | Review LICENSE and transitive notices before release |
| portable-pty | 0.9.0 | [crates.io](https://crates.io/crates/portable-pty) | [wez/wezterm](https://github.com/wezterm/wezterm) | MIT | Low | Public PTY allocation/process API only, with repository-owned fixture and lifecycle logic | No copied code, examples, fixtures, errors, or naming | Review LICENSE and transitive notices before release |
| url | 2.5.8 | [crates.io](https://crates.io/crates/url) | [servo/rust-url](https://github.com/servo/rust-url) | MIT OR Apache-2.0 | Low | Public URL/origin parsing API only; no string-prefix origin approximation | No copied code, examples, fixtures, errors, or naming | Review LICENSE/NOTICE before release |
| ureq | 3.3.0 | [crates.io](https://crates.io/crates/ureq) | [algesten/ureq](https://github.com/algesten/ureq) | MIT OR Apache-2.0 | Low | Public blocking HTTP client API only, behind the project-owned `AppApiDriver` boundary | No copied code, examples, fixtures, errors, or naming | Review LICENSE/NOTICE and TLS feature choices before release |
| enigo | 0.6.1 | [crates.io](https://crates.io/crates/enigo) | [enigo-rs/enigo](https://github.com/enigo-rs/enigo) | MIT | Low | Windows-only public bounded input API behind the project-owned `ComputerDriver` boundary | No copied code, examples, fixtures, errors, or naming | Review LICENSE and platform notices before release |
| xcap | 0.9.6 | [crates.io](https://crates.io/crates/xcap) | [nashaofu/xcap](https://github.com/nashaofu/xcap) | Apache-2.0 | Low | Windows-only public monitor capture API; pixels remain in the scoped artifact store | No copied code, examples, fixtures, errors, or naming | Preserve Apache-2.0 notice and review transitive notices before release |
| httparse | 1.10.1 | [crates.io](https://crates.io/crates/httparse) | [seanmonstar/httparse](https://github.com/seanmonstar/httparse) | MIT OR Apache-2.0 | Low | Public bounded HTTP/1 parser API only inside the project-owned TLS transport | No copied code, examples, fixtures, errors, or naming | Review LICENSE/NOTICE before release |
| rustls | 0.23.46 | [crates.io](https://crates.io/crates/rustls) | [rustls/rustls](https://github.com/rustls/rustls) | Apache-2.0 OR ISC OR MIT | Low | Public TLS 1.3 client/server APIs only behind project-owned remote transport traits | No copied code, examples, fixtures, errors, protocol names, or naming | Review enabled provider, LICENSE, NOTICE, and reachable advisories before release |
| rustls-pki-types | 1.15.0 | [crates.io](https://crates.io/crates/rustls-pki-types) | [rustls/pki-types](https://github.com/rustls/pki-types) | MIT OR Apache-2.0 | Low | Public certificate and private-key wrapper types only at local identity-loading boundaries | No copied code, examples, fixtures, errors, or naming | Review LICENSE/NOTICE before release |
| rcgen | 0.14.8 | [crates.io](https://crates.io/crates/rcgen) | [rustls/rcgen](https://github.com/rustls/rcgen) | MIT OR Apache-2.0 | Low | Dev-only public certificate generation API for repository-owned loopback test identities | No copied code, examples, fixtures, certificate material, errors, or naming | Review LICENSE/NOTICE; never ship generated test private keys |
| ed25519-dalek | 2.2.0 | [crates.io](https://crates.io/crates/ed25519-dalek) | [dalek-cryptography/curve25519-dalek](https://github.com/dalek-cryptography/curve25519-dalek) | BSD-3-Clause | Low | Public Ed25519 verification API only behind the project-owned publisher keyring, admission verifier, and pinned installed-product release verifier | No copied code, examples, fixtures, errors, vectors, or naming | Preserve BSD notice and review locked transitive notices before release |
| windows-service | 0.8.1 | [crates.io](https://crates.io/crates/windows-service) | [mullvad/windows-service-rs](https://github.com/mullvad/windows-service-rs) | MIT OR Apache-2.0 | Low | Windows-only public typed SCM APIs for the three builtin product services | No copied source, examples, fixtures, errors, or naming | Review locked transitive licenses, notices, and reachable advisories before release |
| argon2 | 0.6.0 | [crates.io](https://crates.io/crates/argon2/0.6.0) | [RustCrypto/password-hashes](https://github.com/RustCrypto/password-hashes) | MIT OR Apache-2.0 | Low | Public Argon2id KDF API for independent Web authentication only; bounded parameters | No copied implementation, examples, fixtures, errors, or naming | Exact pin; review locked transitive licenses and reachable advisories before release |
| zeroize | 1.9.0 | [crates.io](https://crates.io/crates/zeroize/1.9.0) | [RustCrypto/utils](https://github.com/RustCrypto/utils) | Apache-2.0 OR MIT | Low | Public Zeroizing wrapper for short-lived Web secrets | No copied implementation, examples, fixtures, errors, or naming | Exact pin; zeroization does not claim elimination of OS/runtime copies |
| tempfile | 3.27.0 | [crates.io](https://crates.io/crates/tempfile/3.27.0) | [Stebalien/tempfile](https://github.com/Stebalien/tempfile) | MIT OR Apache-2.0 | Low | Public NamedTempFile/persist API for atomic managed configuration and acknowledgement replacement; disposable test directories | No copied source, examples, fixtures, errors, or naming | Exact pin; review locked licenses and advisories; no new registry package |

M1-A dependency check (2026-07-14): the locked Gateway tree has no duplicate package versions. The highest declared Rust requirement among the new direct runtime stack is Rust 1.85 (`getrandom`), below the repository's current stable GNU Windows toolchain (Rust 1.97); the full workspace compiles and tests on `x86_64-pc-windows-gnu`. This does not replace the release-time vulnerability and NOTICE audit.

M2-A dependency check (2026-07-16): all five new direct dependencies are exact-pinned. Their manifests declare MIT, Apache-2.0, or MIT OR Apache-2.0; the highest declared direct MSRV is Rust 1.85 (`headless_chrome` and `enigo`), below the current Rust 1.97 toolchain. The driver stack introduces transitive version duplication (notably Windows bindings and low-level utility crates), so release still requires a full locked-tree license, vulnerability, and NOTICE audit; no transitive crate is treated as an authored-code borrowing source.

M2-B dependency check (2026-07-16): `ureq 3.3.0` is exact-pinned and declares Rust 1.85 plus MIT OR Apache-2.0; `url 2.5.8` is now also a direct exact-pinned dependency of capabilities and communication for structural endpoint checks. The connector and loopback fixtures use only public APIs behind project-owned traits and repository-owned tests. No dependency source, example, fixture, prompt, error text, or naming system was copied.

M3 final release check (2026-07-18): no new Rust dependency was added. All direct registry dependencies remain exact-pinned and match `Cargo.lock`, this table, and Cargo metadata. `cargo-audit 0.22.2` reports two advisories against each of `quick-xml 0.30.0` and `0.39.4`; `cargo tree --target x86_64-pc-windows-gnu -i` proves all four locked findings are unreachable on the supported release target because they exist only below xcap's X11/Wayland branches. The typed release audit retains those four observations and would block any advisory reachable on the release target. Root `LICENSE`/`NOTICE`, borrowing records, originality, secret/private-path artifact scan, and clean tracked tree are separate blocking checks.

M4 dependency check (2026-07-22): the authenticated remote transport uses exact-pinned `rustls 0.23.42`, `rustls-pki-types 1.15.0`, and `httparse 1.10.1`; repository-owned process tests use exact-pinned dev dependency `rcgen 0.14.8`. Cargo metadata declares only MIT, Apache-2.0, ISC, or allowed combinations. The highest declared direct MSRV is Rust 1.88 (`rcgen`), below the current Rust 1.97 toolchain. Private keys and certificates generated by tests remain temporary and outside portable artifacts. Implementation uses public APIs behind project-owned transport/admission traits; no dependency source, example, fixture, error text, wire naming, or certificate material was copied.

M5 dependency check (2026-07-22): `ed25519-dalek 2.2.0` is exact-pinned and used only for verification through repository-owned keyring/admission contracts. Test signing keys are deterministic, temporary test inputs and never enter runtime configuration, events, logs, traces, or acceptance artifacts. No dependency source, example, fixture, error wording, test vector, or naming system was copied.

M5 release-audit recheck (2026-09-02): the lockfile now records transitive
`chacha20 0.10.2` and `event-listener 5.4.2`, removing the currently reported
`yanked` and `unsound` versions from the dependency set. `cargo-audit 0.22.2`
still reports `RUSTSEC-2026-0194/0195` for locked `quick-xml 0.30.0/0.39.4`;
the release audit independently verifies all four findings as unreachable on
the frozen `x86_64-pc-windows-gnu` target and retains them as typed
observations. Reachable advisories, unsound warnings, or yanked packages
remain blocking; scanner or reachability uncertainty fails closed.

Security dependency update (2026-09-17): `RUSTSEC-2026-0285` (TLS 1.3
handshake messages accepted across encryption-level boundaries) affects the
reachable `rustls 0.23.42` on the release target. The exact pin moves to the
patched `rustls 0.23.45` within the same minor line; the enabled `ring` and
`std` features, public API surface, license, and project-owned transport traits
are unchanged, and no dependency source, example, fixture, or naming was copied.

V1 core-brain closure A dependency check (2026-09-27): no new package enters
the lockfile. `forme-harness` now depends directly on the already recorded
`serde 1.0.228` (derive) to read the owner's typed capability document, and
`forme-cli` adds the already recorded `serde_json 1.0.150` as a dev-only
dependency for its gateway-entry contract. Both requirements stay exact-pinned
and are asserted by `tools/verify-core-brain-workspace-contract.ps1`. The
CLI's local gateway client is repository-owned and uses only the standard
library over a loopback socket; no dependency source, example, fixture, error
text, or naming was copied.

V1 core-brain closure D4 dependency check (2026-10-18): `chacha20poly1305
0.11.0` is exact-pinned and used only through the project-owned
`ProtectedContentService` envelope. `getrandom 0.4.3` supplies operating-system
entropy for unique nonces. The implementation stores no key material or
secret reference in protocol objects, events, logs, traces, manifests, or
portable artifacts; no dependency source, example, fixture, error text, or
naming was copied.

V1 core-brain closure B security update (2026-10-17): the clean-tree release
audit blocks `rustls 0.23.45` on `RUSTSEC-2026-0346/0347/0348/0349`, published
on 2026-10-14. Each advisory identifies `>=0.23.46` as patched. The direct
exact pin and lockfile move to `0.23.46`, retaining `default-features = false`
and `ring,std`; no crate, internal dependency edge, transport trait, or
permission changes. The release gate remains blocking, with no new advisory
ignore. Public API compilation, authenticated TLS/process contracts,
S1-S99, and the current-tree audit must pass after the update. Some advisory
conditions are feature-specific; upgrading avoids relying on an inferred
non-exploitability exemption. No dependency implementation or fixture is copied.

V1 core-brain closure D security update (2026-10-21): the current release
audit reported reachable `RUSTSEC-2026-0382` for `rustls-webpki 0.103.15` and
`RUSTSEC-2026-0381` for `webpki-roots 1.0.8` on the frozen
`x86_64-pc-windows-gnu` target. The lockfile moves these transitive TLS
components to patched `0.103.16` and `1.0.10`; direct `rustls 0.23.46`, `ureq`
features, transport traits, and crate graph remain unchanged. The root NOTICE
also records the existing Windows-only `windows-service 0.8.1` direct
dependency. No advisory is ignored; the full current-tree release audit and
S1-S99 gate must pass after the update.

Installed offline maintenance (2026-10-23): `forme-cli` directly uses the
already recorded exact-pinned `serde 1.0.228` to encode typed, versioned
restore plans and receipts. The implementation uses repository-owned data
facades and the standard process/filesystem APIs. No internal crate edge,
brain EventKind, copied implementation, fixture or source layout is added.

Installed native package compilation (2026-10-23): repository-owned Node
standard-library tooling and POSIX access hooks add no runtime package or
third-party implementation. Native compilers are external build tools, supplied
by explicit canonical path and SHA-256 pin; their version labels are operator
metadata, not detected provenance. No native compiler, signing identity or
notarization credential is bundled or silently installed. Actual tool license,
provenance, code signing and native execution remain release obligations.

Installed-product security update (2026-10-24): the clean-tree M5 audit
blocked on `RUSTSEC-2026-0386` for Tokio 1.52.3. The RustSec advisory lists
`>=1.53.3` as patched (and a separate 1.51 maintenance range). The direct
exact pin moves to 1.53.3 without changing runtime features, public transport
traits or the internal crate graph. The reported constructors are Unix I/O
APIs; the release gate remains conservative for the reachable package on
Windows, with no advisory exemption. Compile, runtime, browser/bridge and
current-tree supply-chain gates must pass again. No implementation is copied.

Local recovery authority (2026-10-24): `forme-cli` uses exact-pinned
`is_elevated 0.1.2` on Windows and the already locked `rustix 1.1.4` on Unix.
Only safe public process-token/effective-UID APIs are used, before reading
configuration, password input or the identity database. This replaces
PATH-dependent `whoami`/`id` and localized group-text parsing. The crate
graph and frozen brain protocols remain unchanged; all authored crates
retain `forbid(unsafe_code)`. Dependency manifests, public documentation
and licenses were reviewed without copying implementation or fixtures.
Negative process tests use a repository-authored spoof executable. They
do not reset a real account or establish privileged/native installation
acceptance; that requires its own administrator environment and evidence.
