#!/usr/bin/env python3
"""Blocking M5 governed-ecosystem release audit profile."""

from __future__ import annotations

import argparse
import importlib.util
import sys
from pathlib import Path


AUDIT_PATH = Path(__file__).with_name("release_audit.py")
AUDIT_SPEC = importlib.util.spec_from_file_location("forme_m5_release_audit_engine", AUDIT_PATH)
if AUDIT_SPEC is None or AUDIT_SPEC.loader is None:
    raise RuntimeError("M5 release audit engine could not be loaded")
audit = importlib.util.module_from_spec(AUDIT_SPEC)
sys.modules[AUDIT_SPEC.name] = audit
AUDIT_SPEC.loader.exec_module(audit)


audit.TOOL_VERSION = "forme-release-audit/5"
audit.ARTIFACT_TYPE = "forme-m5-release-audit"
audit.ARTIFACT_DOMAIN = b"forme-m5-release-audit-v1\0"
audit.ARTIFACT_PREFIX = "docs/acceptance/m5-release-audit-artifacts/"
audit.SELF_REFERENTIAL_EVIDENCE = {
    "docs/acceptance/m5-a-acceptance-report.md",
    "docs/acceptance/m5-b-acceptance-report.md",
    "docs/acceptance/m5-c-acceptance-report.md",
    "docs/acceptance/m5-acceptance-report.md",
}
audit.EXPECTED_CHECK_IDS = [*audit.EXPECTED_CHECK_IDS, "ecosystem_threat_profile"]

ECOSYSTEM_THREAT_MARKERS = {
    "crates/protocol/tests/m5_contract.rs": [
        "m5_taxonomy_is_additive_after_the_exact_m4_prefix",
        "signed_declarative_package_is_content_addressed_closed_and_tamper_evident",
        "distribution_envelope_and_executor_record_are_closed_content_addresses",
    ],
    "crates/capabilities/tests/m5_ecosystem_contract.rs": [
        "s87_real_ed25519_verification_is_digest_key_and_grant_bound",
        "s88_every_supply_chain_hard_failure_blocks_admission",
        "s94_package_hook_and_agent_profile_remain_declarative_registry_entries",
    ],
    "crates/store/tests/m5_ecosystem_contract.rs": [
        "ecosystem_cas_projects_publisher_admission_and_lifecycle_atomically",
        "package_archive_is_content_addressed_conflict_safe_and_restart_durable",
        "ecosystem_controls_cannot_use_generic_append_and_nonce_is_restart_durable",
    ],
    "crates/harness/tests/m5_ecosystem_contract.rs": [
        "s89_real_registry_read_is_single_approved_untrusted_and_admission_only",
        "s92_s93_update_rollback_and_publisher_revoke_preserve_history_and_fence_visibility",
        "s91_s97_restart_reverifies_archive_signature_publisher_dependencies_and_policy",
    ],
    "crates/harness/tests/m5_distribution_contract.rs": [
        "s95_distribution_rejects_non_executor_stale_and_revoked_inputs_before_network",
        "s96_s97_distribution_unknown_recovers_after_authority_and_executor_restart_once",
        "s98_three_process_registry_authority_executor_golden_is_governed_end_to_end",
    ],
    "crates/eval/tests/m5_artifact_contract.rs": [
        "s99_artifact_gate_rejects_tamper_unknown_fields_extra_and_missing_files",
        "s99_artifact_gate_rejects_sensitive_material_and_cross_reference_drift",
    ],
    "crates/gateway/src/lib.rs": ["prepare_capability_distribution"],
    "crates/gateway/src/server.rs": ["/v1/ecosystem/distributions/prepare"],
    "tools/compliance_doctor.py": ["copy"],
}

ECOSYSTEM_THREAT_COMMANDS = [
    ["cargo", "test", "-p", "forme-protocol", "--test", "m5_contract"],
    ["cargo", "test", "-p", "forme-capabilities", "--test", "m5_ecosystem_contract"],
    ["cargo", "test", "-p", "forme-store", "--test", "m5_ecosystem_contract"],
    ["cargo", "test", "-p", "forme-harness", "--test", "m5_ecosystem_contract"],
    ["cargo", "test", "-p", "forme-harness", "--test", "m5_distribution_contract"],
    ["cargo", "test", "-p", "forme-eval", "--test", "m5_artifact_contract"],
    [
        "cargo",
        "run",
        "-p",
        "forme-eval",
        "--bin",
        "forme-m5-artifact-verify",
        "--",
        "docs/acceptance/m5-artifacts",
    ],
]


def ecosystem_threat_check(root: Path, runner=audit._run):
    findings: list[audit.AuditFinding] = []
    observations: list[audit.AuditFinding] = []
    tracked, tracked_findings = audit._git_files(root)
    tracked_set = set(tracked)
    findings.extend(tracked_findings)
    for relative, markers in ECOSYSTEM_THREAT_MARKERS.items():
        path = root / relative
        if relative not in tracked_set or not path.is_file():
            findings.append(
                audit.AuditFinding(
                    "ECOSYSTEM_THREAT_FIXTURE_MISSING",
                    relative,
                    "required ecosystem threat fixture is not tracked",
                )
            )
            continue
        text = path.read_text(encoding="utf-8")
        for marker in markers:
            if marker not in text:
                findings.append(
                    audit.AuditFinding(
                        "ECOSYSTEM_THREAT_CASE_MISSING",
                        relative,
                        f"required case is absent: {marker}",
                    )
                )
    for command in ECOSYSTEM_THREAT_COMMANDS:
        result = runner(command, root)
        target = " ".join(command[1:])
        if result.returncode != 0:
            findings.append(
                audit.AuditFinding(
                    "ECOSYSTEM_THREAT_FIXTURE_FAILED",
                    target,
                    "required ecosystem threat fixture did not pass",
                )
            )
        else:
            observations.append(
                audit.AuditFinding(
                    "ECOSYSTEM_THREAT_FIXTURE_PASSED",
                    target,
                    "typed ecosystem threat fixture passed",
                )
            )
    paths = [
        *ECOSYSTEM_THREAT_MARKERS,
        "Cargo.lock",
        *(path for path in tracked if path.startswith("docs/acceptance/m5-artifacts/")),
    ]
    return audit._result(
        "ecosystem_threat_profile",
        "cargo",
        audit._tool_version(root, ["cargo", "--version"]),
        audit._digest_paths(root, paths),
        findings,
        observations,
    )


def run_m5_release_audit(root: Path):
    body = audit.run_release_audit(root)
    checks = [*body.checks, ecosystem_threat_check(root)]
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
            print(f"M5 RELEASE AUDIT COMPARISON: FAIL: {error}", file=sys.stderr)
            return 1
        print(f"[PASS] m5-release-audit-current {receipt.digest} {receipt.content_ref}")
        print("M5 RELEASE AUDIT COMPARISON: PASS")
        return 0

    if args.verify is not None:
        try:
            receipt = audit.verify_artifact_set(args.verify.resolve())
        except (OSError, ValueError) as error:
            print(f"M5 RELEASE AUDIT VERIFICATION: FAIL: {error}", file=sys.stderr)
            return 1
        print(f"[PASS] m5-release-audit {receipt.digest} {receipt.content_ref}")
        print("M5 RELEASE AUDIT VERIFICATION: PASS")
        return 0

    root = args.root.resolve()
    output = (
        args.output.resolve()
        if args.output is not None
        else root / "docs/acceptance/m5-release-audit-artifacts"
    )
    body = run_m5_release_audit(root)
    try:
        receipt = audit.write_artifact(output, body)
    except (OSError, ValueError) as error:
        print(f"M5 RELEASE AUDIT: FAIL: artifact write failed: {error}", file=sys.stderr)
        return 1
    for check in body.checks:
        print(f"[{check.outcome.upper()}] {check.check_id} {check.input_digest}")
        for finding in check.findings:
            print(f"  {finding.code} {finding.path}: {finding.detail}")
        for observation in check.observations:
            print(f"  [OBSERVED] {observation.code} {observation.path}: {observation.detail}")
    print(f"M5 release audit artifact {receipt.digest} {receipt.path}")
    print(f"M5 RELEASE AUDIT: {body.outcome.upper()}")
    return 1 if body.blocking else 0


if __name__ == "__main__":
    sys.exit(main())
