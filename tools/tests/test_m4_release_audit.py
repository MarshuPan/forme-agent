from __future__ import annotations

import importlib.util
import json
import subprocess
import sys
import tempfile
import unittest
from unittest import mock
from pathlib import Path


SCRIPT = Path(__file__).resolve().parents[1] / "m4_release_audit.py"
SPEC = importlib.util.spec_from_file_location("forme_m4_release_audit", SCRIPT)
m4 = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
sys.modules[SPEC.name] = m4
SPEC.loader.exec_module(m4)


class M4ReleaseAuditProfileTests(unittest.TestCase):
    def test_m4_profile_has_an_independent_type_domain_and_verifier(self) -> None:
        self.assertEqual(m4.audit.TOOL_VERSION, "forme-release-audit/4")
        self.assertEqual(m4.audit.ARTIFACT_TYPE, "forme-m4-release-audit")
        self.assertEqual(
            m4.audit.ARTIFACT_PREFIX,
            "docs/acceptance/m4-release-audit-artifacts/",
        )
        self.assertEqual(
            m4.audit.SELF_REFERENTIAL_EVIDENCE,
            {
                "docs/acceptance/m4-a-acceptance-report.md",
                "docs/acceptance/m4-b-acceptance-report.md",
                "docs/acceptance/m4-c-acceptance-report.md",
                "docs/acceptance/m4-acceptance-report.md",
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
            "federation_threat_profile",
        ]
        self.assertEqual(m4.audit.EXPECTED_CHECK_IDS, check_ids)
        checks = [
            m4.audit.ComplianceCheckResult(
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
        body = m4.audit.ReleaseAuditBody(
            schema_version=1,
            tool_version=m4.audit.TOOL_VERSION,
            root_digest="sha256:" + "a" * 64,
            outcome="pass",
            blocking=False,
            checks=checks,
        )
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            receipt = m4.audit.write_artifact(root, body)
            self.assertEqual(m4.audit.verify_artifact_set(root), receipt)
            artifact = json.loads(receipt.path.read_text(encoding="utf-8"))
            self.assertEqual(artifact["artifact_type"], "forme-m4-release-audit")
            self.assertEqual(artifact["body"]["tool_version"], "forme-release-audit/4")
            self.assertTrue(artifact["digest"].startswith("sha256:"))

    def test_federation_threat_profile_fails_closed_when_fixtures_are_absent(self) -> None:
        def passing_runner(command: list[str], root: Path):
            return subprocess.CompletedProcess(command, 0, "", "")

        with tempfile.TemporaryDirectory() as temporary:
            result = m4.federation_threat_check(Path(temporary), passing_runner)
        self.assertTrue(result.blocking)
        self.assertEqual(result.outcome, "fail")
        self.assertIn(
            "FEDERATION_THREAT_FIXTURE_MISSING",
            {finding.code for finding in result.findings},
        )

    def test_relocated_gateway_cases_remain_required_after_the_module_split(self) -> None:
        def passing_runner(command: list[str], root: Path):
            return subprocess.CompletedProcess(command, 0, "", "")

        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            tracked = list(m4.FEDERATION_THREAT_MARKERS)
            for relative, markers in m4.FEDERATION_THREAT_MARKERS.items():
                path = root / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text("\n".join(markers), encoding="utf-8")
            cases = "crates/gateway/src/tests/mod.rs"
            decoy = "crates/gateway/src/lib.rs"
            (root / decoy).write_text((root / cases).read_text(encoding="utf-8"), encoding="utf-8")
            tracked.append(decoy)
            with mock.patch.object(m4.audit, "_git_files", return_value=(tracked, [])):
                self.assertFalse(m4.federation_threat_check(root, passing_runner).blocking)
                (root / cases).write_text("// Removed test cases", encoding="utf-8")
                result = m4.federation_threat_check(root, passing_runner)
            self.assertTrue(result.blocking)
            self.assertTrue(any(
                finding.code == "FEDERATION_THREAT_CASE_MISSING" and finding.path == cases
                for finding in result.findings
            ))


if __name__ == "__main__":
    unittest.main()
