# Native packaging inputs

These files are release inputs, not proof that a native package has been built or installed. The bundle inventory must be generated first and signed by a release pipeline that records the target OS/CPU, exact binary/UI digests, package-tool version and signing receipt.

## Service recovery status

The canonical service policy is three OS-level restarts with delays `5s -> 30s -> 120s`, then diagnosis/recovery required, with a 24-hour failure-count reset. Windows SCM registration expresses and verifies that policy directly. The current Linux units use a finite systemd guard (the initial start plus at most three automatic restarts in a 24-hour window, with a fixed 5-second delay); this is a safety fallback and does not prove the canonical delay sequence. The current macOS LaunchDaemon inputs deliberately omit `KeepAlive` because launchd has no finite failure-count and escalating-delay primitive. Linux and macOS native packages remain `release_ready: false` until their platform-specific recovery adapters and native evidence are complete.

No package or acceptance report may describe these POSIX inputs as supported merely because the unit/plist parses or a bundle was staged. A platform becomes supported only after its service manager expresses the bounded policy, the installed service is observed under the target OS, and the corresponding IP evidence is recorded.

- Windows: compile `windows/forme.iss` with a pinned Inno Setup toolchain, then sign the installer. The two mandatory original access helpers use the OS-provided Windows PowerShell/.NET ACL APIs, protect empty default roots before writing data, and grant/verify fixed service roles before starting Host/Web. Existing installations require the separate lifecycle/recovery path. Installation checks every typed registration result; uninstall stops before program removal if a typed unregister fails. These inputs have not yet been compiled into an accepted native installer.
- Linux: install the three units, control files and post-install policy through a pinned `.deb` or `.rpm` builder. The post-install hook must run only as the package manager's root transaction and must be tested on each declared distribution.
- macOS: use `Distribution.xml`, `scripts/postinstall` and the three LaunchDaemon plists with pinned `pkgbuild`/`productbuild` inputs, then Developer ID sign and notarize each architecture package. The post-install script prepares the unclaimed setup state and starts only Host/Web; the brain remains stopped until claim and model configuration.

No template accepts a model key, arbitrary executable, arbitrary service name or shell command from Agent input. Native installation, ACL, service account, signing, notarization and IP01-IP24 remain separate acceptance evidence.

## Native compilation

`node tools/build-native-package.mjs --bundle <absolute-bundle> --output <new-absolute-directory> --toolchain <absolute-json> --version <major.minor.patch>` invokes the actual local package compiler. The bundle must come from `build-product-bundle.mjs`; debug inputs require an explicit `--allow-development`. Existing output is never replaced. Input bundles and compilers must be direct files without linked ancestors or hardlinks; unexpected files and changed checksum indexes fail closed.

The toolchain JSON has `schema_version: 1` and a `tools` array. Every entry contains `id`, canonical absolute `path`, exact lowercase SHA-256 `sha256`, and an operator-supplied `version_label`. Required ids are `iscc` on Windows, `dpkg-deb` on Ubuntu 24.04, `rpmbuild` on Rocky 9, or both `pkgbuild` and `productbuild` on macOS 14+. The label is recorded as supplied, not claimed as an independently detected compiler version. Obtain and review compiler provenance, license and binary pins separately; this command does not download them or silently trust an executable on PATH.

The builder checks native OS/CPU and executable headers, snapshots reviewed payloads, executes a fixed argument plan with a restricted environment, and rechecks both snapshot and source before recording the artifact digest. A failed build retains its output for diagnosis and creates no success receipt. Compiler output is bounded and represented by a digest. The resulting `native-build-receipt.json` explicitly records unsigned and uninstalled status and `release_ready: false`. Header inspection is not runtime verification. Cross-compilation and fixture layout tests never prove native installation.

POSIX first-install hooks reject nonempty retained roots and linked ancestors before extraction. Their fixed access helper partitions Host, Brain and Web directories; only the selected service group can read published configuration and immutable model credentials. New core-file inodes and credential revisions preserve this access rather than reverting to administrator-only files. Uninstall stops and observes Brain, then Web, then Host, before disabling each service; failure stops removal. These controls still require native account, permission and service-token acceptance on every declared platform.

The offline `node tools/sign-product-release.mjs --bundle <absolute-bundle> --output <new-absolute-directory> --key-file <private-Ed25519-PEM> --key-id <pinned-id> --compatibility-file <absolute-json>` command produces the protocol's signed release envelope. The compatibility file explicitly supplies schema_version 1, config_range, data_range and minimum_upgrader; ranges use the protocol's schema_version/read_min/read_max/write_version fields. Do not guess compatibility from a version number. Private keys stay outside input/output and must be private on Unix.

The resulting directory has manifest.json, raw signature.ed25519, content/ and a safe signing receipt. Web files are mapped from share/forme/web to content/web to match the kernel's closed-set verifier. Verify with the built forme-hostd `--verify-release <directory> --trust-config <administrator-trust-json> --target <wire-target>`. The trust JSON uses ReleaseStagingConfig; it must come from an independently pinned administrator/distribution root. The receipt's public key is informational and is never automatically trusted. This signs release contents; it does not Authenticode-sign an installer, notarize macOS packages, or certify native installation/CPU behavior.

The signer accepts the builder's single derived `SHA256SUMS` only when it exactly matches the declared inventory order and digests. It is not an exception allowing arbitrary extra files.

## Windows Authenticode signing

The Windows installer has a separate native-signing step after `build-native-package.mjs`. It copies the unsigned installer to a new output path and refuses to replace either the source or an existing output:

```text
node tools/sign-windows-package.mjs --input <absolute-unsigned-exe> --output <absolute-new-signed-exe> --certificate-thumbprint <40-hex-sha1> [--store-location CurrentUser|LocalMachine]
```

The certificate is selected by its explicit thumbprint from the Windows certificate store. Its private key is never exported, copied into the bundle, or written to a receipt. The tool invokes the repository-owned `tools/windows/sign-authenticode.ps1` through a fixed Windows PowerShell 5.1 module path, signs with SHA-256, then independently reads `Get-AuthenticodeSignature` and requires the observed signer thumbprint and `Authenticode` type to match the requested certificate. The signed file gets a sibling `<signed-exe>.signing-receipt.json` with source/output SHA-256 values, signer identity, raw signature status and a normalized `trust_status`; binding the receipt to the output filename allows multiple candidates to coexist without overwriting evidence.

`Valid` is the only `trusted` state. `NotTrusted` remains `untrusted`; `UnknownError` and other unresolved states remain `unverifiable`. They are never reported as a valid release. `--allow-untrusted` is an explicit local-development escape that permits evidence generation while retaining `release_ready: false`; it is not appropriate for a published package. Native signing does not install the package, update the Host trust root, or close any IP01-IP24 acceptance item.
