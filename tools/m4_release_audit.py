#!/usr/bin/env python3
"""Blocking M4 release audit profile built on the frozen release-audit engine."""

from __future__ import annotations

import argparse
import importlib.util
import sys
from pathlib import Path


AUDIT_PATH = Path(__file__).with_name("release_audit.py")
AUDIT_SPEC = importlib.util.spec_from_file_location("forme_m4_release_audit_engine", AUDIT_PATH)
if AUDIT_SPEC is None or AUDIT_SPEC.loader is None:
    raise RuntimeError("M4 release audit engine could not be loaded")
audit = importlib.util.module_from_spec(AUDIT_SPEC)
sys.modules[AUDIT_SPEC.name] = audit
AUDIT_SPEC.loader.exec_module(audit)


audit.TOOL_VERSION = "forme-release-audit/4"
audit.ARTIFACT_TYPE = "forme-m4-release-audit"
audit.ARTIFACT_DOMAIN = b"forme-m4-release-audit-v1\0"
audit.ARTIFACT_PREFIX = "docs/acceptance/m4-release-audit-artifacts/"
audit.SELF_REFERENTIAL_EVIDENCE = {
    "docs/acceptance/m4-a-acceptance-report.md",
    "docs/acceptance/m4-b-acceptance-report.md",
    "docs/acceptance/m4-c-acceptance-report.md",
    "docs/acceptance/m4-acceptance-report.md",
}
audit.EXPECTED_CHECK_IDS = [*audit.EXPECTED_CHECK_IDS, "federation_threat_profile"]

FEDERATION_THREAT_MARKERS = {
    "crates/execution/tests/m4_tls_contract.rs": [
        "s72_s74_real_mutual_tls_dispatch_is_durable_and_exactly_once_at_the_driver",
        "s71_s73_stale_fence_and_unscoped_credentials_stop_before_the_inner_driver",
    ],
    "crates/store/tests/m4_federation_contract.rs": [
        "s70_s71_s77_peer_epoch_lease_and_dispatch_claim_are_single_writer",
        "s75_s76_replication_is_filtered_contiguous_atomic_and_idempotent",
    ],
    "crates/store/tests/m4_replicad_process_contract.rs": [
        "s76_replicad_process_is_atomic_restart_idempotent_and_rejects_reorder_or_tamper",
    ],
    "crates/harness/tests/m4_federation_contract.rs": [
        "s71_plan_schema_grant_and_epoch_drift_stop_before_remote_driver",
        "s73_authenticated_remote_injection_and_secret_echo_never_enter_authority_facts",
        "s72_s78_s79_s82_authority_restart_preserves_recovery_and_replay_ledgers",
        "s79_cross_device_owner_control_is_dual_bound_one_shot_and_fences_late_receipts",
    ],
    "crates/gateway/src/lib.rs": [
        "s79_owner_client_requires_peer_channel_and_independent_owner_before_harness_control",
        "s75_s78_s82_peer_ingress_is_channel_bound_before_harness",
    ],
    "crates/eval/tests/m4_artifact_contract.rs": [
        "s84_artifact_gate_rejects_tamper_unknown_fields_extra_and_missing_files",
        "s84_artifact_gate_rejects_secrets_host_paths_and_endpoints",
        "s84_plan_lease_fence_cursor_and_trace_mismatches_fail_closed",
    ],
}

FEDERATION_THREAT_COMMANDS = [
    ["cargo", "test", "-p", "forme-execution", "--test", "m4_tls_contract"],
    ["cargo", "test", "-p", "forme-store", "--test", "m4_federation_contract"],
    ["cargo", "test", "-p", "forme-store", "--test", "m4_replicad_process_contract"],
    ["cargo", "test", "-p", "forme-harness", "--test", "m4_federation_contract"],
    ["cargo", "test", "-p", "forme-gateway", "--lib"],
    ["cargo", "test", "-p", "forme-eval", "--test", "m4_artifact_contract"],
]


def federation_threat_check(root: Path, runner=audit._run):
    findings: list[audit.AuditFinding] = []
    observations: list[audit.AuditFinding] = []
    tracked, tracked_findings = audit._git_files(root)
    tracked_set = set(tracked)
    findings.extend(tracked_findings)
    for relative, markers in FEDERATION_THREAT_MARKERS.items():
        path = root / relative
        if relative not in tracked_set or not path.is_file():
            findings.append(
                audit.AuditFinding(
                    "FEDERATION_THREAT_FIXTURE_MISSING",
                    relative,
                    "required federation threat fixture is not tracked",
                )
            )
            continue
        text = path.read_text(encoding="utf-8")
        for marker in markers:
            if marker not in text:
                findings.append(
                    audit.AuditFinding(
                        "FEDERATION_THREAT_CASE_MISSING",
                        relative,
                        f"required case is absent: {marker}",
                    )
                )
    for command in FEDERATION_THREAT_COMMANDS:
        result = runner(command, root)
        target = " ".join(command[1:])
        if result.returncode != 0:
            findings.append(
                audit.AuditFinding(
                    "FEDERATION_THREAT_FIXTURE_FAILED",
                    target,
                    "required federation threat fixture did not pass",
                )
            )
        else:
            observations.append(
                audit.AuditFinding(
                    "FEDERATION_THREAT_FIXTURE_PASSED",
                    target,
                    "typed federation threat fixture passed",
                )
            )
    paths = [
        *FEDERATION_THREAT_MARKERS,
        "Cargo.lock",
        *(path for path in tracked if path.startswith("docs/acceptance/m4-artifacts/")),
    ]
    return audit._result(
        "federation_threat_profile",
        "cargo",
        audit._tool_version(root, ["cargo", "--version"]),
        audit._digest_paths(root, paths),
        findings,
        observations,
    )


def run_m4_release_audit(root: Path):
    body = audit.run_release_audit(root)
    checks = [*body.checks, federation_threat_check(root)]
    blocking = any(check.blocking for check in checks)
    return audit.ReleaseAuditBody(
        schema_version=audit.SCHEMA_VERSION,
        tool_version=audit.TOOL_VERSION,
        root_digest=body.root_digest,
        outcome="fail" if blocking else "pass",
        blocking=blocking,
        checks=checks,
    )


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--output", type=Path)
    parser.add_argument("--verify", type=Path)
    parser.add_argument(
        "--compare",
        type=Path,
        nargs=2,
        metavar=("CURRENT", "REPOSITORY"),
    )
    args = parser.parse_args(argv)

    if args.compare is not None:
        try:
            receipt = audit.compare_artifact_sets(
                args.compare[0].resolve(), args.compare[1].resolve()
            )
        except (OSError, ValueError) as error:
            print(f"M4 RELEASE AUDIT COMPARISON: FAIL: {error}", file=sys.stderr)
            return 1
        print(f"[PASS] m4-release-audit-current {receipt.digest} {receipt.content_ref}")
        print("M4 RELEASE AUDIT COMPARISON: PASS")
        return 0

    if args.verify is not None:
        try:
            receipt = audit.verify_artifact_set(args.verify.resolve())
        except (OSError, ValueError) as error:
            print(f"M4 RELEASE AUDIT VERIFICATION: FAIL: {error}", file=sys.stderr)
            return 1
        print(f"[PASS] m4-release-audit {receipt.digest} {receipt.content_ref}")
        print("M4 RELEASE AUDIT VERIFICATION: PASS")
        return 0

    root = args.root.resolve()
    output = (
        args.output.resolve()
        if args.output is not None
        else root / "docs/acceptance/m4-release-audit-artifacts"
    )
    body = run_m4_release_audit(root)
    try:
        receipt = audit.write_artifact(output, body)
    except (OSError, ValueError) as error:
        print(f"M4 RELEASE AUDIT: FAIL: artifact write failed: {error}", file=sys.stderr)
        return 1
    for check in body.checks:
        print(f"[{check.outcome.upper()}] {check.check_id} {check.input_digest}")
        for finding in check.findings:
            print(f"  {finding.code} {finding.path}: {finding.detail}")
        for observation in check.observations:
            print(f"  [OBSERVED] {observation.code} {observation.path}: {observation.detail}")
    print(f"M4 release audit artifact {receipt.digest} {receipt.path}")
    print(f"M4 RELEASE AUDIT: {body.outcome.upper()}")
    return 1 if body.blocking else 0


if __name__ == "__main__":
    sys.exit(main())
