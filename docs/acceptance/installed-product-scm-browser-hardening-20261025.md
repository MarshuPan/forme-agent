# Installed Product SCM and Browser Hardening Acceptance

Date: 2026-10-25. Branch: `shu/installed-product`.

This report records two focused hardening changes. It does not claim a complete
installed product or close IP01-IP24.

## 1. SCM Host process binding

Commit: `85b59f5`.

- Managed Windows Host startup now validates the exact fixed SCM registration
  before loading runtime credentials or accepting requests.
- After reporting `Running`, Host requires `OWN_PROCESS`, `Running`, and the
  exact nonzero current PID. Registration is rechecked between observations.
- StartPending PID values are not used as identity proof; the bounded wait is
  read-only and fail-closed. A foreign, missing, zero, transitional, or changed
  PID closes admission.
- The permanent installed-host gate includes the two process-binding tests.

Evidence:

- `target/scm-installed-host-final.log`: 163/163 named scenarios passed.
- `target/scm-browser-final-quality.log`: unified source quality passed,
  including 1021 Rust tests, 77 frontend tests, strict Clippy, packaging,
  signature, and compliance checks.

This proves SCM registration/process binding only. It is not proof of an
effective LocalSystem token, core IPC peer identity, administrator installation,
or IP10/IP24.

## 2. Browser golden request framing

Commit: `a8ac11b`.

- The loopback browser fixture now frames complete HTTP headers with a bounded
  read deadline and an 8192-byte header limit.
- Accepted sockets are made blocking for the request handler, while bounded
  connection workers prevent idle browser preconnections from starving accept.
- Incomplete requests produce no mutation. The existing approval, mutation,
  event, and artifact assertions are unchanged.

Evidence:

- Fragmentation regression: 1/1 passed.
- Real Chrome/Edge S38: 1/1 passed.
- Real Chrome/Edge S68: 1/1 passed.
- Real Windows AppContainer/overlay scenarios: 3/3 passed.

## 3. Product boundary

The production runtime still uses `UnavailableWorkerIsolation`. These changes
do not certify LocalSystem service identity, installed core peer authentication,
administrator SCM installation, Linux/macOS native services, LAN trust, signed
packages, session helpers, or IP01-IP24. Those remain explicit platform/product
acceptance work.
