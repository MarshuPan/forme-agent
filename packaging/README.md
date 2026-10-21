# Native packaging inputs

These files are release inputs, not proof that a native package has been built or installed. The bundle inventory must be generated first and signed by a release pipeline that records the target OS/CPU, exact binary/UI digests, package-tool version and signing receipt.

- Windows: compile `windows/forme.iss` with a pinned Inno Setup toolchain, then sign the installer. The referenced `--install-service` commands are accepted only after the typed service lifecycle implementation is present.
- Linux: install the three units, control files and post-install policy through a pinned `.deb` or `.rpm` builder. The post-install hook must run only as the package manager's root transaction and must be tested on each declared distribution.
- macOS: use `Distribution.xml`, `scripts/postinstall` and the three LaunchDaemon plists with pinned `pkgbuild`/`productbuild` inputs, then Developer ID sign and notarize each architecture package. The post-install script prepares the unclaimed setup state and starts only Host/Web; the brain remains stopped until claim and model configuration.

No template accepts a model key, arbitrary executable, arbitrary service name or shell command from Agent input. Native installation, ACL, service account, signing, notarization and IP01-IP24 remain separate acceptance evidence.
