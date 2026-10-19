#!/usr/bin/env python3
"""V1 closure profile over the existing M5 release audit, with live C24 proof."""

from __future__ import annotations

import argparse
import importlib.util
import json
import os
import re
import subprocess
import sys
from dataclasses import replace
from pathlib import Path


# A private profile instance keeps the historical M5 type/check set unchanged.
PROFILE_SPEC = importlib.util.spec_from_file_location(
    "forme_core_brain_m5_profile", Path(__file__).with_name("m5_release_audit.py")
)
if PROFILE_SPEC is None or PROFILE_SPEC.loader is None:
    raise RuntimeError("core-brain release audit profile could not be loaded")
m5 = importlib.util.module_from_spec(PROFILE_SPEC)
sys.modules[PROFILE_SPEC.name] = m5
PROFILE_SPEC.loader.exec_module(m5)
EVIDENCE_SPEC = importlib.util.spec_from_file_location(
    "forme_core_brain_evidence", Path(__file__).with_name("core_brain_evidence.py")
)
if EVIDENCE_SPEC is None or EVIDENCE_SPEC.loader is None:
    raise RuntimeError("core-brain evidence verifier could not be loaded")
evidence = importlib.util.module_from_spec(EVIDENCE_SPEC)
EVIDENCE_SPEC.loader.exec_module(evidence)
audit = m5.audit
audit.TOOL_VERSION = "forme-core-brain-release-audit/1"
audit.ARTIFACT_TYPE = "forme-core-brain-release-audit"
audit.ARTIFACT_DOMAIN = b"forme-core-brain-release-audit-v1\0"
audit.ARTIFACT_PREFIX = "docs/acceptance/core-brain-closure-release-audit-artifacts/"
REPORT_EXCLUSIONS = {
    "docs/acceptance/core-brain-d-acceptance-report.md",
    "docs/acceptance/core-brain-closure-acceptance-report.md",
}
audit.SELF_REFERENTIAL_EVIDENCE = set(REPORT_EXCLUSIONS)
audit.EXPECTED_CHECK_IDS = [*audit.EXPECTED_CHECK_IDS, "core_brain_closure"]
GATE = "tools/verify-core-brain-closure.ps1"
WORKSPACE_GATE = "tools/verify-core-brain-workspace-contract.ps1"
PRODUCT_ARTIFACT = "docs/acceptance/core-brain-d-artifacts/product-golden.json"
M5_RECEIPT = re.compile(
    r"docs/acceptance/m5-release-audit-artifacts/release-audit-[0-9a-f]{64}\.json"
)
WAVES = {"A": (1, 6), "B": (7, 12), "C": (13, 18), "D": (19, 24)}
# D-specific integration targets are closed inventories. Shared targets select
# modules, case prefixes or exact names; historical S tests remain outside D.
D_TARGETS = {
    ("forme-harness", ("--test", "v1_brain_integration_closure")): (
        "production_m3_resolves_immutable_bodies_binds_activated_versions_and_recovers",
    ),
    ("forme-harness", ("--test", "m3_b_harness_contract")): ("c19_",),
    ("forme-harness", ("--test", "v1_core_brain_d_product")): (),
    ("forme-harness", ("--test", "v1_core_brain_d_runner")): (),
    ("forme-harness", ("--bin", "forme-core-brain-d-golden")): ("product_golden_runner::tests::",),
    ("forme-store", ("--test", "v1_data_protection")): (),
    ("forme-store", ("--test", "v1_legacy_migration")): (),
    ("forme-store", ("--test", "v1_data_migrate_operator")): (),
    ("forme-eval", ("--test", "v1_core_brain_d_artifacts")): (),
    ("forme-gateway", ("--test", "v1_data_controls")): (),
    ("forme-store", ("--lib",)): (
        "content::tests::", "backup::tests::",
        "at_rest::auxiliary_tests::c22_all_auxiliary_blob_writers_authenticate_insert_update_and_row_binding",
    ),
    ("forme-models", ("--lib",)): (
        "registry::tests::",
        "tests::endpoint_error_text_cannot_forge_retryability_or_escape_to_diagnostics",
    ),
    ("forme-eval", ("--lib",)): ("dashboard::tests::",),
    ("forme-harness", ("--lib",)): ("production::protected_artifact_tests::",),
}
REQUIRED_FILES = {
    GATE,
    WORKSPACE_GATE,
    "tools/verify-m2-workspace-contract.ps1",
    "tools/core_brain_release_audit.py",
    "tools/tests/test_core_brain_release_audit.py",
    "tools/core_brain_evidence.py",
    "tools/tests/test_core_brain_evidence.py",
    "crates/store/src/content.rs",
    "crates/store/src/backup.rs",
    "crates/store/src/at_rest.rs",
    "crates/store/src/bin/forme-data-migrate.rs",
    "crates/harness/src/production.rs",
    "crates/models/src/registry.rs",
    "crates/eval/src/dashboard.rs",
    "crates/eval/src/core_brain_d.rs",
    "crates/eval/src/bin/forme-core-brain-d-artifact-verify.rs",
    "crates/harness/src/product_golden_runner.rs",
    "crates/harness/src/product_golden_runner_tests.rs",
    "crates/harness/src/bin/forme-core-brain-d-golden.rs",
    "docs/acceptance/core-brain-closure-artifacts/manifest.json",
    *{f"docs/acceptance/core-brain-{wave}-artifacts/manifest.json" for wave in "abcd"},
}


def _finding(code: str, path: str, detail: str):
    return audit.AuditFinding("CORE_BRAIN_" + code, path, detail)


def _invoke(command: list[str], root: Path, runner):
    try:
        return runner(command, root)
    except (OSError, subprocess.SubprocessError):
        # Do not export raw provider/process errors or private absolute paths.
        return subprocess.CompletedProcess(command, 127, "", "")


def _powershell(script: str, *arguments: str) -> list[str]:
    return ["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", script, *arguments]


def _fixture_path(package: str, target: tuple[str, ...]) -> str:
    directory = "crates/" + package.removeprefix("forme-")
    if target == ("--lib",):
        return directory + "/src/lib.rs"
    if len(target) == 2 and target[0] == "--bin":
        return directory + f"/src/bin/{target[1]}.rs"
    if len(target) == 2 and target[0] == "--test":
        return directory + f"/tests/{target[1]}.rs"
    raise ValueError("unsupported scenario target")


def _selected_test(name: str, selectors: tuple[str, ...]) -> bool:
    # Module prefixes end in ::, case prefixes in _, and other names are exact.
    return not selectors or any(
        name.startswith(selector) if selector.endswith(("::", "_")) else name == selector
        for selector in selectors
    )


def _inventory(raw: str) -> list[dict]:
    value = json.loads(raw)
    if not isinstance(value, dict) or set(value) != {"schema_version", "scenarios"}:
        raise ValueError("inventory fields are incomplete or unknown")
    if type(value["schema_version"]) is not int or value["schema_version"] != 1:
        raise ValueError("unsupported inventory schema")
    rows = value["scenarios"]
    if not isinstance(rows, list) or not rows or len(rows) > 1000:
        raise ValueError("inventory must contain a bounded nonempty scenario set")
    seen = set()
    coverage = {wave: set() for wave in WAVES}
    d_targets: dict[tuple, set[str]] = {}
    for row in rows:
        if not isinstance(row, dict) or set(row) != {
            "wave", "id", "package", "target", "test", "include_ignored"
        }:
            raise ValueError("scenario fields are incomplete or unknown")
        if any(type(row[key]) is not str or not row[key] for key in ("wave", "id", "package", "test")):
            raise ValueError("scenario identifiers must be nonempty strings")
        if row["wave"] not in WAVES or type(row["include_ignored"]) is not bool:
            raise ValueError("scenario wave or ignored-test flag is invalid")
        if not re.fullmatch(r"forme-[a-z]+", row["package"]) or not re.fullmatch(r"[A-Za-z0-9_:]+", row["test"]):
            raise ValueError("scenario package or exact test name is invalid")
        target = row["target"]
        if not isinstance(target, list) or not (
            target == ["--lib"] or (
                len(target) == 2 and target[0] in {"--test", "--bin"}
                and isinstance(target[1], str) and re.fullmatch(r"[a-z0-9][a-z0-9_-]*", target[1])
            )
        ):
            raise ValueError("scenario target is not a library, named integration or binary target")
        key = (row["wave"], row["package"], tuple(target), row["test"])
        if key in seen:
            raise ValueError("duplicate exact scenario")
        seen.add(key)
        for first, last in re.findall(r"C(\d+)(?:-C(\d+))?", row["id"]):
            coverage[row["wave"]].update(range(int(first), int(last or first) + 1))
        if row["wave"] == "D":
            d_targets.setdefault((row["package"], tuple(target)), set()).add(row["test"])
    for wave, (first, last) in WAVES.items():
        if coverage[wave] != set(range(first, last + 1)):
            raise ValueError(f"wave {wave} does not cover its exact closure scenario range")
    if set(d_targets) != set(D_TARGETS):
        raise ValueError("D runtime target inventory is incomplete or unexpected")
    for key, selectors in D_TARGETS.items():
        if any(not _selected_test(test, selectors) for test in d_targets[key]) or any(
            not any(_selected_test(test, (selector,)) for test in d_targets[key])
            for selector in selectors
        ):
            raise ValueError("D runtime selected-test inventory is incomplete or unexpected")
    return rows


def _local_metadata(root: Path, runner):
    findings = []
    configured = os.environ.get("CARGO_TARGET_DIR")
    if configured and (root / configured).resolve() != (root / "target").resolve():
        return None, [_finding("SHARED_TARGET_DIR", "CARGO_TARGET_DIR", "release requires checkout-local target/debug")]
    if os.environ.get("CARGO_BUILD_TARGET"):
        return None, [_finding("BUILD_TARGET_OVERRIDE", "CARGO_BUILD_TARGET", "release requires native target/debug")]
    result = _invoke(["cargo", "metadata", "--no-deps", "--format-version", "1"], root, runner)
    try:
        if result.returncode:
            raise ValueError("metadata unavailable")
        metadata = json.loads(result.stdout)
        if Path(metadata["target_directory"]).resolve() != (root / "target").resolve():
            raise ValueError("shared target")
        if Path(metadata["workspace_root"]).resolve() != root.resolve():
            raise ValueError("wrong checkout")
        packages = metadata["packages"]
        members = metadata["workspace_members"]
        names = {p["name"] for p in packages if p["id"] in members}
        if len(members) != 18 or len(names) != 18 or not all(name.startswith("forme-") for name in names):
            raise ValueError("crate graph size")
    except (ValueError, KeyError, TypeError):
        findings.append(_finding("WORKSPACE_METADATA_INVALID", "Cargo.toml", "current 18-crate checkout metadata or local target is unavailable or invalid"))
        return None, findings
    return metadata, findings


def verify_product_artifact(root: Path, path: Path, runner=None):
    """Return findings; only the real typed verifier's Pass can close C24."""
    runner = runner or audit._run
    findings = []
    if path.is_symlink() or not path.is_file():
        return [_finding("C24_UNVERIFIABLE", PRODUCT_ARTIFACT, "six-task product artifact is missing or not a regular file")]
    try:
        if path.stat().st_size > 33_554_432:
            raise ValueError("artifact exceeds bound")
        value = json.loads(path.read_text(encoding="utf-8"))
        source = value["source_commit"]
        if not isinstance(source, str) or not re.fullmatch(r"(?:[0-9a-f]{40}|[0-9a-f]{64})", source):
            raise ValueError("invalid source commit")
    except (OSError, ValueError, KeyError, TypeError):
        return [_finding("C24_INVALID", PRODUCT_ARTIFACT, "product artifact identity is invalid")]
    for command in (
        ["git", "cat-file", "-e", source + "^{commit}"],
        ["git", "merge-base", "--is-ancestor", source, "HEAD"],
        ["git", "diff", "--quiet", source, "HEAD", "--", "crates"],
        ["git", "diff", "--quiet", "HEAD", "--", "crates"],
    ):
        if _invoke(command, root, runner).returncode:
            findings.append(_finding("C24_SOURCE_MISMATCH", PRODUCT_ARTIFACT, "artifact source must exist, be an ancestor, and match current crate sources"))
            break
    untracked = _invoke(["git", "ls-files", "--others", "--exclude-standard", "--", "crates"], root, runner)
    if untracked.returncode or untracked.stdout.strip():
        findings.append(_finding("C24_SOURCE_MISMATCH", "crates", "untracked crate sources prevent source-commit attestation"))
    _, local_findings = _local_metadata(root, runner)
    findings.extend(local_findings)
    if findings:
        return findings
    build = _invoke([
        "cargo", "build", "--locked", "--target-dir", str(root / "target"),
        "-p", "forme-eval", "--bin", "forme-core-brain-d-artifact-verify",
    ], root, runner)
    if build.returncode:
        return [_finding("C24_VERIFIER_UNAVAILABLE", PRODUCT_ARTIFACT, "checkout-local typed verifier could not be built")]
    executable = root / "target/debug" / ("forme-core-brain-d-artifact-verify.exe" if os.name == "nt" else "forme-core-brain-d-artifact-verify")
    if executable.is_symlink() or not executable.is_file():
        return [_finding("C24_VERIFIER_UNAVAILABLE", "target/debug", "checkout-local typed verifier is missing")]
    result = _invoke([str(executable), str(path), "--require-pass"], root, runner)
    if result.returncode == 2 or re.search(r"^core-brain-d artifact: Unverifiable$", result.stdout, re.M):
        findings.append(_finding("C24_UNVERIFIABLE", PRODUCT_ARTIFACT, "configured live evidence is unavailable; runtime/offline evidence cannot pass C24"))
    elif result.returncode or not re.search(r"^core-brain-d artifact: Pass$", result.stdout, re.M):
        findings.append(_finding("C24_FAILED", PRODUCT_ARTIFACT, "six-task typed verifier did not return a live Pass"))
    elif value.get("grade") != "LiveConfigured":
        findings.append(_finding("C24_UNVERIFIABLE", PRODUCT_ARTIFACT, "controlled or unavailable evidence cannot support a live release"))
    return findings


def core_brain_closure_check(root: Path, runner=None):
    runner = runner or audit._run
    tracked, findings = audit._git_files(root)
    tracked_set = set(tracked)
    observations = []
    paths = set(REQUIRED_FILES) | {PRODUCT_ARTIFACT, "Cargo.toml", "Cargo.lock"}
    result = _invoke(_powershell(GATE, "-ListScenarios"), root, runner)
    try:
        if result.returncode:
            raise ValueError("inventory command failed")
        rows = _inventory(result.stdout)
    except (ValueError, TypeError):
        findings.append(_finding("INVENTORY_INVALID", GATE, "C1-C24 exact scenario inventory is unavailable or invalid"))
        rows = []
    targets: dict[tuple, list[dict]] = {}
    for row in rows:
        key = (row["package"], tuple(row["target"]))
        targets.setdefault(key, []).append(row)
        paths.add(_fixture_path(*key))
    for relative in sorted(paths):
        path = root / relative
        if relative not in tracked_set or path.is_symlink() or not path.is_file():
            findings.append(_finding("ARTIFACT_MISSING" if relative == PRODUCT_ARTIFACT else "FIXTURE_MISSING", relative, "release input must be a regular tracked file"))
    metadata, local_findings = _local_metadata(root, runner)
    findings.extend(local_findings)
    if metadata is not None:
        graph = _invoke(_powershell(WORKSPACE_GATE), root, runner)
        if graph.returncode or "core-brain-workspace-contract: PASS" not in graph.stdout.splitlines():
            findings.append(_finding("WORKSPACE_CONTRACT_FAILED", WORKSPACE_GATE, "current crate edges and 99-kind taxonomy contract did not pass"))
        for (package, target), entries in targets.items():
            listed = _invoke(["cargo", "test", "--locked", "-p", package, *target, "--", "--list"], root, runner)
            names = re.findall(r"^([A-Za-z0-9_:]+): test$", listed.stdout, re.M)
            registered = set(names)
            expected = {entry["test"] for entry in entries}
            d_expected = {entry["test"] for entry in entries if entry["wave"] == "D"}
            selectors = D_TARGETS.get((package, target), ())
            d_actual = {name for name in registered if _selected_test(name, selectors)}
            if listed.returncode or len(names) != len(registered) or not expected <= registered or (
                d_expected and d_expected != d_actual
            ):
                findings.append(_finding("SCENARIO_MISSING", _fixture_path(package, target), "compiled exact test inventory differs from the closure gate; zero-test success is rejected"))
        runtime = _invoke(_powershell(GATE, "-Wave", "D", "-RuntimeOnly"), root, runner)
        if runtime.returncode or "core-brain-runtime (D): PASS (runtime only; not wave D or Final acceptance)" not in runtime.stdout.splitlines():
            findings.append(_finding("RUNTIME_FAILED", GATE, "D exact runtime scenarios, including the ignored CLI case, did not pass"))
    findings.extend(verify_product_artifact(root, root / PRODUCT_ARTIFACT, runner))
    try:
        bundle = evidence.verify_bundle(root, rows)
        if bundle["verdict"] != "Pass":
            findings.append(_finding("EVIDENCE_UNVERIFIABLE", "docs/acceptance/core-brain-closure-artifacts", "all four wave manifests must pass; runtime evidence alone cannot close D"))
    except (OSError, ValueError, KeyError, TypeError, RecursionError):
        findings.append(_finding("EVIDENCE_INVALID", "docs/acceptance/core-brain-closure-artifacts", "closed wave inventories, source receipts or aggregate references are missing, stale or unsafe"))
    # A manifest digest never replaces the existing independent typed verifier.
    for wave, names in (("b", ("relevance-comparison",)), ("c", ("patterns", "conflict", "recovery", "isolation", "supervision", "learning"))):
        for name in names:
            typed = _invoke([
                "cargo", "run", "--locked", "--target-dir", str(root / "target"), "-p", "forme-eval",
                "--bin", f"forme-core-brain-{wave}-artifact-verify", "--",
                f"docs/acceptance/core-brain-{wave}-artifacts/{name}.json",
            ], root, runner)
            if typed.returncode or f"core-brain-{wave} artifact: PASS" not in typed.stdout.splitlines():
                findings.append(_finding("TYPED_EVIDENCE_FAILED", f"docs/acceptance/core-brain-{wave}-artifacts/{name}.json", "typed evidence did not pass its independent verifier"))
    if not findings:
        observations.append(_finding("CLOSURE_VERIFIED", PRODUCT_ARTIFACT, "C1-C24 compiled inventory, current graph, D runtime and six live C24 tasks verified"))
    return audit._result("core_brain_closure", "forme-core-brain-verifier", audit.TOOL_VERSION,
                         audit._digest_paths(root, paths), findings, observations)


# M5 receipts include the core receipt on their next refresh. Exclude only
# receipt filenames (not arbitrary M5 evidence) from every core input digest.
_artifact_scan = audit._artifact_check


def _core_artifact_scan(root: Path, tracked: list[str]):
    return _artifact_scan(root, [path for path in tracked if not M5_RECEIPT.fullmatch(path)])


audit._artifact_check = _core_artifact_scan
_body_parser = audit._body_from_dict


def _typed_body(value):
    if not isinstance(value, dict):
        raise ValueError("release body must be an object")
    for key, expected in {"schema_version": int, "tool_version": str, "root_digest": str, "outcome": str, "blocking": bool, "checks": list}.items():
        if type(value.get(key)) is not expected:
            raise ValueError("release body contains an incorrectly typed field")
    if not re.fullmatch(r"sha256:[0-9a-f]{64}", value["root_digest"]):
        raise ValueError("release root digest is invalid")
    for check in value["checks"]:
        if not isinstance(check, dict):
            raise ValueError("release check must be an object")
        for key in ("check_id", "outcome", "tool", "tool_version", "input_digest"):
            if type(check.get(key)) is not str or not check[key]:
                raise ValueError("release check contains an incorrectly typed identifier")
        if type(check.get("blocking")) is not bool:
            raise ValueError("release check blocking flag must be boolean")
        for key in ("findings", "observations"):
            if type(check.get(key)) is not list:
                raise ValueError("release check findings must be arrays")
            for item in check[key]:
                if not isinstance(item, dict) or any(type(item.get(field)) is not str for field in ("code", "path", "detail")):
                    raise ValueError("release finding fields must be strings")
    return _body_parser(value)


audit._body_from_dict = _typed_body
_artifact_verifier = audit.verify_artifact_set


def _typed_artifact_set(root: Path, require_pass: bool = True):
    entries = list(root.iterdir()) if root.is_dir() else []
    if len(entries) == 1 and entries[0].is_file() and not entries[0].is_symlink():
        value = json.loads(entries[0].read_text(encoding="utf-8"))
        if not isinstance(value, dict) or type(value.get("schema_version")) is not int:
            raise ValueError("release artifact schema must be an integer")
    return _artifact_verifier(root, require_pass)


audit.verify_artifact_set = _typed_artifact_set


def run_core_brain_release_audit(root: Path):
    root = root.resolve()
    tracked, _ = audit._git_files(root)
    audit.SELF_REFERENTIAL_EVIDENCE = REPORT_EXCLUSIONS | {
        path for path in tracked if M5_RECEIPT.fullmatch(path)
    }
    body = m5.run_m5_release_audit(root)
    checks = [*body.checks, core_brain_closure_check(root)]
    blocking = any(check.blocking for check in checks)
    return replace(body, checks=checks, blocking=blocking, outcome="fail" if blocking else "pass")


def _exit_code(findings) -> int:
    return 2 if any(f.code in {"CORE_BRAIN_C24_UNVERIFIABLE", "CORE_BRAIN_ARTIFACT_MISSING", "CORE_BRAIN_C24_VERIFIER_UNAVAILABLE", "CORE_BRAIN_EVIDENCE_UNVERIFIABLE"} for f in findings) else (1 if findings else 0)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--output", type=Path)
    mode.add_argument("--verify", type=Path)
    mode.add_argument("--compare", type=Path, nargs=2, metavar=("CURRENT", "REPOSITORY"))
    mode.add_argument("--verify-product", type=Path)
    args = parser.parse_args(argv)
    try:
        if args.verify is not None or args.compare is not None:
            receipt = (audit.compare_artifact_sets(*[p.resolve() for p in args.compare])
                       if args.compare is not None else audit.verify_artifact_set(args.verify.resolve()))
            print(f"[PASS] core-brain-release-audit {receipt.digest} {receipt.content_ref}")
            return 0
        root = args.root.resolve()
        if args.verify_product is not None:
            findings = verify_product_artifact(root, args.verify_product.absolute())
            for finding in findings:
                print(f"{finding.code} {finding.path}: {finding.detail}")
            code = _exit_code(findings)
            print("core-brain-c24: " + ("PASS" if code == 0 else "UNVERIFIABLE" if code == 2 else "FAIL"))
            return code
        body = run_core_brain_release_audit(root)
        output = args.output.resolve() if args.output is not None else root / audit.ARTIFACT_PREFIX
        receipt = audit.write_artifact(output, body)
        for check in body.checks:
            print(f"[{check.outcome.upper()}] {check.check_id} {check.input_digest}")
            for finding in check.findings:
                print(f"  {finding.code} {finding.path}: {finding.detail}")
        print(f"core-brain release audit {receipt.digest}: {body.outcome.upper()}")
        return _exit_code([finding for check in body.checks for finding in check.findings])
    except (OSError, ValueError, subprocess.SubprocessError):
        print("CORE-BRAIN RELEASE AUDIT: FAIL (invalid or unavailable evidence)", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
