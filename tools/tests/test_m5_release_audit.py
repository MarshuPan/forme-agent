from __future__ import annotations

import importlib.util
import json
import subprocess
import sys
import tempfile
import unittest
from unittest import mock
from pathlib import Path


SCRIPT = Path(__file__).resolve().parents[1] / "m5_release_audit.py"
SPEC = importlib.util.spec_from_file_location("forme_m5_release_audit", SCRIPT)
m5 = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
sys.modules[SPEC.name] = m5
SPEC.loader.exec_module(m5)


class M5ReleaseAuditProfileTests(unittest.TestCase):
    def test_m5_profile_has_an_independent_type_domain_and_closed_check_set(self) -> None:
        self.assertEqual(m5.audit.TOOL_VERSION, "forme-release-audit/5")
        self.assertEqual(m5.audit.ARTIFACT_TYPE, "forme-m5-release-audit")
        self.assertEqual(
            m5.audit.ARTIFACT_PREFIX,
            "docs/acceptance/m5-release-audit-artifacts/",
        )
        self.assertEqual(
            m5.audit.SELF_REFERENTIAL_EVIDENCE,
            {
                "docs/acceptance/m5-a-acceptance-report.md",
                "docs/acceptance/m5-b-acceptance-report.md",
                "docs/acceptance/m5-c-acceptance-report.md",
                "docs/acceptance/m5-acceptance-report.md",
            },
        )

        check_ids = [
            "release_tree",
            "license_notice",
            "direct_dependencies",
            "borrowing_records",
            "upstream_copy",
            "acceptance_artifacts",
            "vulnerability_audit",
            "ecosystem_threat_profile",
        ]
        self.assertEqual(m5.audit.EXPECTED_CHECK_IDS, check_ids)
        checks = [
            m5.audit.ComplianceCheckResult(
                check_id=check_id,
                outcome="pass",
                blocking=False,
                tool="fixture",
                tool_version="fixture/1",
                input_digest="sha256:" + f"{index:064x}",
                findings=[],
                observations=[],
            )
            for index, check_id in enumerate(check_ids, start=1)
        ]
        body = m5.audit.ReleaseAuditBody(
            schema_version=1,
            tool_version=m5.audit.TOOL_VERSION,
            root_digest="sha256:" + "a" * 64,
            outcome="pass",
            blocking=False,
            checks=checks,
        )
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            receipt = m5.audit.write_artifact(root, body)
            self.assertEqual(m5.audit.verify_artifact_set(root), receipt)
            artifact = json.loads(receipt.path.read_text(encoding="utf-8"))
            self.assertEqual(artifact["artifact_type"], "forme-m5-release-audit")
            self.assertEqual(artifact["body"]["tool_version"], "forme-release-audit/5")
            self.assertTrue(artifact["digest"].startswith("sha256:"))

    def test_ecosystem_threat_profile_fails_closed_when_fixtures_are_absent(self) -> None:
        def passing_runner(command: list[str], root: Path):
            return subprocess.CompletedProcess(command, 0, "", "")

        with tempfile.TemporaryDirectory() as temporary:
            result = m5.ecosystem_threat_check(Path(temporary), passing_runner)
        self.assertTrue(result.blocking)
        self.assertEqual(result.outcome, "fail")
        self.assertIn(
            "ECOSYSTEM_THREAT_FIXTURE_MISSING",
            {finding.code for finding in result.findings},
        )

    def test_ecosystem_threat_profile_fails_closed_on_a_command_failure(self) -> None:
        def failing_runner(command: list[str], root: Path):
            return subprocess.CompletedProcess(command, 1, "", "fixture failed")

        with tempfile.TemporaryDirectory() as temporary:
            result = m5.ecosystem_threat_check(Path(temporary), failing_runner)
        self.assertTrue(result.blocking)
        self.assertIn(
            "ECOSYSTEM_THREAT_FIXTURE_FAILED",
            {finding.code for finding in result.findings},
        )

    def test_relocated_implementation_cannot_be_replaced_by_an_old_file_decoy(self) -> None:
        def passing_runner(command: list[str], root: Path):
            return subprocess.CompletedProcess(command, 0, "", "")

        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            tracked = list(m5.ECOSYSTEM_THREAT_MARKERS)
            for relative, markers in m5.ECOSYSTEM_THREAT_MARKERS.items():
                path = root / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text("\n".join(markers), encoding="utf-8")
            decoy = "crates/gateway/src/lib.rs"
            (root / decoy).write_text("prepare_capability_distribution", encoding="utf-8")
            tracked.append(decoy)
            with mock.patch.object(m5.audit, "_git_files", return_value=(tracked, [])):
                self.assertFalse(m5.ecosystem_threat_check(root, passing_runner).blocking)
                implementation = root / "crates/gateway/src/ecosystem.rs"
                implementation.write_text("// Removed implementation", encoding="utf-8")
                result = m5.ecosystem_threat_check(root, passing_runner)
            self.assertTrue(result.blocking)
            self.assertTrue(any(
                finding.code == "ECOSYSTEM_THREAT_CASE_MISSING"
                and finding.path == "crates/gateway/src/ecosystem.rs"
                for finding in result.findings
            ))


if __name__ == "__main__":
    unittest.main()
