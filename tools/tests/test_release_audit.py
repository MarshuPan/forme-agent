from __future__ import annotations

import importlib.util
import json
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock


SCRIPT = Path(__file__).resolve().parents[1] / "release_audit.py"
SPEC = importlib.util.spec_from_file_location("forme_release_audit", SCRIPT)
audit = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
sys.modules[SPEC.name] = audit
SPEC.loader.exec_module(audit)


def passing_vulnerability_scan(_root: Path):
    return audit.VulnerabilityScan(
        tool="fixture-audit",
        version="fixture-audit/1",
        completed=True,
        return_code=0,
        payload={"vulnerabilities": {"count": 0, "list": []}, "warnings": {}},
        error=None,
    )


def failing_vulnerability_scan(_root: Path):
    return audit.VulnerabilityScan(
        tool="fixture-audit",
        version="unavailable",
        completed=False,
        return_code=2,
        payload=None,
        error="fixture scanner failed",
    )


def warning_vulnerability_scan(warnings: dict[str, object]):
    return audit.VulnerabilityScan(
        tool="fixture-audit",
        version="fixture-audit/1",
        completed=True,
        return_code=1,
        payload={"vulnerabilities": {"count": 0, "list": []}, "warnings": warnings},
        error=None,
    )


def malformed_vulnerability_scan(payload: object):
    return audit.VulnerabilityScan(
        tool="fixture-audit",
        version="fixture-audit/1",
        completed=True,
        return_code=0,
        payload=payload,
        error=None,
    )


class ReleaseAuditTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.write(
            "Cargo.toml",
            "[workspace]\n"
            'members = ["crates/core"]\n'
            'resolver = "2"\n\n'
            "[workspace.package]\n"
            'license = "Apache-2.0"\n',
        )
        self.write(
            "crates/core/Cargo.toml",
            "[package]\n"
            'name = "forme-release-fixture"\n'
            'version = "0.0.1"\n'
            'edition = "2021"\n'
            "publish = false\n",
        )
        self.write("crates/core/src/lib.rs", "#![forbid(unsafe_code)]\npub fn fixture() {}\n")
        self.write("LICENSE", "Apache License\nVersion 2.0, January 2004\n")
        self.write(
            "NOTICE",
            "forme\nCopyright 2026 forme contributors\nApache-2.0 release fixture.\n",
        )
        self.write(
            "docs/compliance/third-party-dependencies.md",
            "# Third-party dependencies\n\n"
            "| Package | Version | Source | Repository | License | Copy risk | Implementation rule | Copied material | Release action |\n"
            "|---|---:|---|---|---|---|---|---|---|\n",
        )
        self.write(
            "compliance/borrowing-records.json",
            json.dumps({"schema_version": 1, "records": []}),
        )
        self.write(".gitattributes", "* text\n")
        self.write(".gitignore", "research/upstreams/\n")
        self.write(
            "research/upstreams/reference/source.py",
            "def unrelated_reference(value):\n    return value * 913\n",
        )
        self.write("docs/acceptance/fixture.json", json.dumps({"schema_version": 1}))
        self.run_command(["cargo", "generate-lockfile", "--offline"])
        self.run_command(["git", "init", "-q"])
        self.run_command(["git", "config", "user.email", "release-audit@example.invalid"])
        self.run_command(["git", "config", "user.name", "Release Audit Fixture"])
        self.run_command(["git", "config", "core.autocrlf", "true"])
        self.run_command(["git", "add", "."])
        self.run_command(["git", "commit", "-qm", "fixture"])

    def tearDown(self) -> None:
        self.temp.cleanup()

    def write(self, relative: str, content: str) -> Path:
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding="utf-8")
        return path

    def run_command(self, command: list[str]) -> None:
        subprocess.run(command, cwd=self.root, check=True, capture_output=True)

    def report(self, runner=passing_vulnerability_scan):
        return audit.run_release_audit(self.root, vulnerability_runner=runner)

    @staticmethod
    def codes(report) -> set[str]:
        return {
            finding.code
            for check in report.checks
            for finding in check.findings
        }

    @staticmethod
    def observation_codes(report) -> set[str]:
        return {
            observation.code
            for check in report.checks
            for observation in check.observations
        }

    def test_clean_fixture_passes_and_typed_artifact_verifies(self) -> None:
        report = self.report()
        self.assertFalse(report.blocking)
        self.assertEqual(report.outcome, "pass")
        self.assertEqual(len(report.checks), 7)
        artifact_root = self.root / "release-audit-artifact"
        receipt = audit.write_artifact(artifact_root, report)
        self.assertEqual(audit.verify_artifact_set(artifact_root), receipt)
        comparison_root = self.root / "release-audit-comparison"
        comparison = audit.write_artifact(comparison_root, report)
        self.assertEqual(audit.compare_artifact_sets(artifact_root, comparison_root), comparison)

    def test_upstream_copy_evidence_binds_the_corpus_snapshot(self) -> None:
        first = self.report()
        first_copy = next(check for check in first.checks if check.check_id == "upstream_copy")
        first_root_digest = first.root_digest
        self.write(
            "research/upstreams/reference/second.py",
            "def second_unrelated_reference(value):\n    return value + 271828\n",
        )
        second = self.report()
        second_copy = next(check for check in second.checks if check.check_id == "upstream_copy")
        self.assertEqual(first_root_digest, second.root_digest)
        self.assertNotEqual(first_copy.input_digest, second_copy.input_digest)

    def test_clean_receipt_is_stable_across_worktree_line_endings(self) -> None:
        linked_root = self.root.with_name(f"{self.root.name}-linked")
        self.run_command(["git", "worktree", "add", "--detach", str(linked_root), "HEAD"])
        try:
            root_manifest = self.root / "Cargo.toml"
            root_manifest.write_bytes(root_manifest.read_bytes().replace(b"\r\n", b"\n"))
            self.run_command(["git", "add", "Cargo.toml"])
            self.assertNotEqual(
                root_manifest.read_bytes(),
                (linked_root / "Cargo.toml").read_bytes(),
            )
            first = self.report()
            second = audit.run_release_audit(
                linked_root,
                vulnerability_runner=passing_vulnerability_scan,
            )

            status = subprocess.run(
                ["git", "status", "--porcelain=v1", "--untracked-files=all"],
                cwd=self.root,
                check=True,
                capture_output=True,
                text=True,
            )
            self.assertFalse(first.blocking, f"{self.codes(first)} status={status.stdout!r}")
            self.assertFalse(second.blocking, self.codes(second))
            self.assertEqual(first.root_digest, second.root_digest)
            self.assertEqual(
                [check.input_digest for check in first.checks],
                [check.input_digest for check in second.checks],
            )
            self.assertEqual(audit._artifact_digest(first), audit._artifact_digest(second))
        finally:
            subprocess.run(
                ["git", "worktree", "remove", "--force", str(linked_root)],
                cwd=self.root,
                check=False,
                capture_output=True,
            )

    def test_missing_notice_and_unknown_or_gpl_license_are_blocking(self) -> None:
        (self.root / "NOTICE").unlink()
        self.write("LICENSE", "GNU " + "GENERAL PUBLIC LICENSE\n")
        self.write(
            "compliance/borrowing-records.json",
            json.dumps(
                {
                    "schema_version": 1,
                    "records": [
                        {
                            "source": "fixture",
                            "license": "UNKNOWN",
                            "copy_risk": "low",
                            "implementation_rule": "fixture",
                            "notice_action": "review",
                            "allowed_paths": [],
                            "allowed_sha256": [],
                        }
                    ],
                }
            ),
        )
        codes = self.codes(self.report())
        self.assertIn("NOTICE_MISSING", codes)
        self.assertIn("PROJECT_LICENSE_UNSUPPORTED", codes)
        self.assertIn("BORROWING_LICENSE_BLOCKED", codes)

    def test_unpinned_dependency_and_lock_table_drift_are_blocking(self) -> None:
        self.write(
            "crates/core/Cargo.toml",
            "[package]\n"
            'name = "forme-release-fixture"\n'
            'version = "0.0.1"\n'
            'edition = "2021"\n'
            "publish = false\n\n"
            "[dependencies]\n"
            'serde = "1"\n',
        )
        self.write(
            "docs/compliance/third-party-dependencies.md",
            "# Third-party dependencies\n\n"
            "| Package | Version | Source | Repository | License | Copy risk | Implementation rule | Copied material | Release action |\n"
            "|---|---:|---|---|---|---|---|---|---|\n"
            "| serde | 9.9.9 | [crates.io](https://crates.io/crates/serde) | [serde](https://example.invalid) | UNKNOWN | Low | fixture | none | review |\n",
        )
        codes = self.codes(self.report())
        self.assertIn("UNPINNED_DEPENDENCY", codes)
        self.assertIn("DIRECT_DEPENDENCY_NOT_LOCKED", codes)
        self.assertIn("DEPENDENCY_TABLE_DRIFT", codes)
        self.assertIn("DEPENDENCY_SOURCE_OR_LICENSE_INVALID", codes)

        self.write("Cargo.lock", "not valid toml = [\n")
        codes = self.codes(self.report())
        self.assertIn("LOCKFILE_INVALID", codes)
        self.assertIn("LOCK_METADATA_FAILED", codes)

    def test_secret_private_path_dirty_tree_and_scanner_failure_are_blocking(self) -> None:
        self.write(
            "docs/acceptance/leak.json",
            json.dumps(
                {
                    "schema_version": 1,
                    "value": "credential:raw-value",
                    "path": "C:\\Users\\owner\\private",
                }
            ),
        )
        self.run_command(["git", "add", "docs/acceptance/leak.json"])
        report = self.report(failing_vulnerability_scan)
        codes = self.codes(report)
        self.assertIn("RELEASE_TREE_DIRTY", codes)
        self.assertIn("ARTIFACT_SECRET_VALUE", codes)
        self.assertIn("ARTIFACT_PRIVATE_PATH", codes)
        self.assertIn("VULNERABILITY_SCAN_FAILED", codes)
        self.assertTrue(report.blocking)

    def test_unreachable_advisory_and_yanked_warnings_are_observations(self) -> None:
        scan = warning_vulnerability_scan(
            {
                "unsound": [
                    {
                        "kind": "unsound",
                        "package": {"name": "unsafe-fixture", "version": "1.2.3"},
                        "advisory": {"id": "RUSTSEC-2099-0001"},
                    }
                ],
                "yanked": [
                    {
                        "kind": "yanked",
                        "package": {"name": "yanked-fixture", "version": "4.5.6"},
                        "advisory": None,
                    }
                ],
            }
        )
        with mock.patch.object(audit, "_package_reachability", return_value=(False, None)):
            report = self.report(lambda _root: scan)

        self.assertFalse(report.blocking, self.codes(report))
        observations = self.observation_codes(report)
        self.assertIn("TARGET_UNREACHABLE_ADVISORY_WARNING", observations)
        self.assertIn("TARGET_UNREACHABLE_YANKED_PACKAGE", observations)

    def test_reachable_advisory_and_yanked_warnings_block_release(self) -> None:
        scan = warning_vulnerability_scan(
            {
                "unsound": [
                    {
                        "kind": "unsound",
                        "package": {"name": "unsafe-fixture", "version": "1.2.3"},
                        "advisory": {"id": "RUSTSEC-2099-0001"},
                    }
                ],
                "yanked": [
                    {
                        "kind": "yanked",
                        "package": {"name": "yanked-fixture", "version": "4.5.6"},
                        "advisory": None,
                    }
                ],
            }
        )
        with mock.patch.object(audit, "_package_reachability", return_value=(True, None)):
            report = self.report(lambda _root: scan)

        codes = self.codes(report)
        self.assertIn("RELEASE_TARGET_ADVISORY_WARNING", codes)
        self.assertIn("RELEASE_TARGET_YANKED_PACKAGE", codes)
        self.assertTrue(report.blocking)

    def test_reachable_informational_warning_is_observation(self) -> None:
        scan = warning_vulnerability_scan(
            {
                "unmaintained": [
                    {
                        "kind": "unmaintained",
                        "package": {"name": "legacy-fixture", "version": "7.8.9"},
                        "advisory": {"id": "RUSTSEC-2099-0002"},
                    }
                ]
            }
        )
        with mock.patch.object(audit, "_package_reachability", return_value=(True, None)):
            report = self.report(lambda _root: scan)

        self.assertFalse(report.blocking, self.codes(report))
        self.assertIn(
            "RELEASE_TARGET_INFORMATIONAL_WARNING",
            self.observation_codes(report),
        )

    def test_unknown_or_malformed_warning_fails_closed(self) -> None:
        unknown = self.report(
            lambda _root: warning_vulnerability_scan(
                {
                    "future-kind": [
                        {
                            "kind": "future-kind",
                            "package": {"name": "future-fixture", "version": "1.0.0"},
                            "advisory": {"id": "RUSTSEC-2099-0003"},
                        }
                    ]
                }
            )
        )
        malformed = self.report(
            lambda _root: warning_vulnerability_scan({"unsound": "not-a-list"})
        )
        with mock.patch.object(
            audit,
            "_package_reachability",
            return_value=(None, "fixture reachability failed"),
        ):
            unreachable_failure = self.report(
                lambda _root: warning_vulnerability_scan(
                    {
                        "unsound": [
                            {
                                "kind": "unsound",
                                "package": {
                                    "name": "unsafe-fixture",
                                    "version": "1.2.3",
                                },
                                "advisory": {"id": "RUSTSEC-2099-0001"},
                            }
                        ]
                    }
                )
            )

        self.assertIn("VULNERABILITY_WARNING_KIND_UNKNOWN", self.codes(unknown))
        self.assertIn("VULNERABILITY_WARNING_REPORT_INCOMPLETE", self.codes(malformed))
        self.assertIn(
            "VULNERABILITY_WARNING_REACHABILITY_FAILED",
            self.codes(unreachable_failure),
        )
        self.assertTrue(unknown.blocking)
        self.assertTrue(malformed.blocking)
        self.assertTrue(unreachable_failure.blocking)

    def test_invalid_scanner_payload_fails_closed(self) -> None:
        root_payload = self.report(lambda _root: malformed_vulnerability_scan([]))
        missing_vulnerabilities = self.report(
            lambda _root: malformed_vulnerability_scan({"warnings": {}})
        )
        invalid_count = self.report(
            lambda _root: malformed_vulnerability_scan(
                {
                    "vulnerabilities": {"count": False, "list": []},
                    "warnings": {},
                }
            )
        )

        self.assertIn("VULNERABILITY_SCAN_PAYLOAD_INVALID", self.codes(root_payload))
        self.assertIn(
            "VULNERABILITY_REPORT_INCOMPLETE",
            self.codes(missing_vulnerabilities),
        )
        self.assertIn("VULNERABILITY_REPORT_INCOMPLETE", self.codes(invalid_count))
        self.assertTrue(root_payload.blocking)
        self.assertTrue(missing_vulnerabilities.blocking)
        self.assertTrue(invalid_count.blocking)

    def test_artifact_tamper_extra_entry_and_failed_report_are_rejected(self) -> None:
        report = self.report()
        artifact_root = self.root / "release-audit-artifact"
        receipt = audit.write_artifact(artifact_root, report)
        raw = json.loads(receipt.path.read_text(encoding="utf-8"))
        raw["body"]["root_digest"] = "sha256:" + "0" * 64
        receipt.path.write_text(json.dumps(raw), encoding="utf-8")
        with self.assertRaises(ValueError):
            audit.verify_artifact_set(artifact_root)

        shutil.rmtree(artifact_root)
        receipt = audit.write_artifact(artifact_root, report)
        (artifact_root / "unexpected.txt").write_text("unexpected", encoding="utf-8")
        with self.assertRaises(ValueError):
            audit.verify_artifact_set(artifact_root)

        shutil.rmtree(artifact_root)
        failed = self.report(failing_vulnerability_scan)
        audit.write_artifact(artifact_root, failed)
        with self.assertRaises(ValueError):
            audit.verify_artifact_set(artifact_root)


if __name__ == "__main__":
    unittest.main()
