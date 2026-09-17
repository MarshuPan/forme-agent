from __future__ import annotations

import hashlib
import importlib.util
import json
import shutil
import sys
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).resolve().parents[1] / "compliance_doctor.py"
SPEC = importlib.util.spec_from_file_location("forme_compliance_doctor", SCRIPT)
doctor = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
sys.modules[SPEC.name] = doctor
SPEC.loader.exec_module(doctor)


class ComplianceDoctorTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.write(
            "Cargo.toml",
            "[workspace]\nmembers = [\"crates/core\"]\nresolver = \"2\"\n",
        )
        self.write(
            "crates/core/Cargo.toml",
            "[package]\nname = \"self-authored-core\"\nversion = \"0.0.1\"\nedition = \"2021\"\n",
        )
        self.write(
            "crates/core/src/lib.rs",
            "pub fn project_owned_calculation(value: u64) -> u64 { value.saturating_add(17) }\n",
        )
        self.write(
            "research/upstreams/reference/source.py",
            "def unrelated_reference(value):\n    return value * 913\n",
        )

    def tearDown(self) -> None:
        self.temp.cleanup()

    def write(self, relative: str, content: str) -> Path:
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding="utf-8")
        return path

    def finding_codes(self, report) -> set[str]:
        return {
            finding.code
            for check in report.checks
            for finding in check.findings
        }

    def test_compliant_tree_passes_all_three_scopes(self) -> None:
        report = doctor.run_doctor(self.root)
        self.assertFalse(report.blocking)
        self.assertEqual([check.outcome for check in report.checks], ["pass", "pass", "pass"])

    def test_build_reference_is_a_blocking_upstream_finding(self) -> None:
        self.write(
            "crates/core/src/lib.rs",
            'include_str!("../../../research/upstreams/reference/source.py");\n',
        )
        report = doctor.run_doctor(self.root)
        self.assertIn("UPSTREAM_BUILD_REFERENCE", self.finding_codes(report))
        self.assertTrue(report.blocking)

    def test_project_owned_embedded_assets_are_not_upstream_references(self) -> None:
        self.write("crates/core/assets/view.html", "<main>project owned</main>\n")
        self.write("crates/core/assets/view.css", "main { display: block; }\n")
        self.write(
            "crates/core/src/lib.rs",
            'const VIEW: &str = include_str!("../assets/view.html");\n'
            'const STYLE: &[u8] = include_bytes!("../assets/view.css");\n',
        )
        report = doctor.run_doctor(self.root)
        self.assertNotIn("UPSTREAM_BUILD_REFERENCE", self.finding_codes(report))
        self.assertFalse(report.blocking)

    def test_exact_and_normalized_fixture_copy_are_blocking(self) -> None:
        copied = (
            "This deliberately distinctive fixture sentence has enough content to detect exact, "
            "normalized, prompt, and error-expression copying across project boundaries.\n"
            "another_distinctive_identifier = 918273645\n"
        )
        self.write("research/upstreams/reference/tests/rare_fixture.txt", copied)
        self.write("crates/core/tests/local_fixture.txt", copied.replace("\n", "\r\n"))
        report = doctor.run_doctor(self.root)
        self.assertTrue(
            {"EXACT_FILE_COPY", "NORMALIZED_FILE_COPY"}.intersection(self.finding_codes(report))
        )
        self.assertTrue(report.blocking)

    def test_long_expression_copy_is_detected_inside_other_content(self) -> None:
        phrase = (
            "A uniquely authored diagnostic explains that the bounded decision trace must retain "
            "the workspace snapshot before any governed outward action can be considered complete."
        )
        self.write("research/upstreams/reference/prompt.md", f"header\n{phrase}\nfooter\n")
        self.write("crates/core/src/lib.rs", f"const MESSAGE: &str = \"{phrase}\";\n")
        report = doctor.run_doctor(self.root)
        self.assertIn("TOKEN_SEQUENCE_COPY", self.finding_codes(report))

    def test_common_rust_derive_is_not_treated_as_copied_prose(self) -> None:
        derive = "#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]"
        self.write(
            "research/upstreams/reference/types.rs",
            f"{derive}\npub enum UpstreamOnly {{ Alpha }}\n",
        )
        self.write(
            "crates/core/src/lib.rs",
            f"{derive}\npub enum ProjectOnly {{ Beta }}\n",
        )
        report = doctor.run_doctor(self.root)
        self.assertNotIn("LONG_EXPRESSION_COPY", self.finding_codes(report))
        self.assertFalse(report.blocking)

    def test_missing_upstream_corpus_fails_closed(self) -> None:
        shutil.rmtree(self.root / "research" / "upstreams")
        report = doctor.run_doctor(self.root)
        self.assertIn("UPSTREAM_CORPUS_MISSING", self.finding_codes(report))
        self.assertTrue(report.blocking)

    def test_ci_mode_skips_only_an_absent_corpus_and_keeps_other_scopes_blocking(self) -> None:
        shutil.rmtree(self.root / "research" / "upstreams")
        report = doctor.run_doctor(self.root, allow_missing_corpus=True)
        copy = next(check for check in report.checks if check.scope == "copy")
        self.assertEqual("skip", copy.outcome)
        self.assertFalse(copy.blocking)
        self.assertIn("UPSTREAM_CORPUS_MISSING", self.finding_codes(report))
        self.assertFalse(report.blocking)

        self.write(
            "crates/core/src/embedded.rs",
            'pub const SOURCE: &str = include_str!("../../../research/upstreams/reference/source.py");\n',
        )
        report = doctor.run_doctor(self.root, allow_missing_corpus=True)
        self.assertIn("UPSTREAM_BUILD_REFERENCE", self.finding_codes(report))
        self.assertTrue(report.blocking)

    def test_ci_mode_still_runs_copy_detection_when_the_corpus_exists(self) -> None:
        report = doctor.run_doctor(self.root, allow_missing_corpus=True)
        copy = next(check for check in report.checks if check.scope == "copy")
        self.assertEqual("pass", copy.outcome)

    def test_linked_worktree_uses_the_main_worktree_corpus(self) -> None:
        phrase = (
            "This shared upstream corpus phrase is intentionally distinctive enough to prove "
            "that a linked worktree scans the main worktree reference snapshot before passing."
        )
        shared_root = self.root / ".shared-main"
        shared_git = shared_root / ".git"
        linked_git = shared_git / "worktrees" / "fixture"
        linked_git.mkdir(parents=True)
        (linked_git / "commondir").write_text("../..\n", encoding="utf-8")
        (self.root / ".git").write_text(f"gitdir: {linked_git}\n", encoding="utf-8")
        shutil.rmtree(self.root / "research" / "upstreams")
        upstream = shared_root / "research" / "upstreams" / "reference" / "prompt.md"
        upstream.parent.mkdir(parents=True)
        upstream.write_text(phrase + "\n", encoding="utf-8")
        self.write("crates/core/src/lib.rs", f'const MESSAGE: &str = "{phrase}";\n')

        report = doctor.run_doctor(self.root)
        self.assertNotIn("UPSTREAM_CORPUS_MISSING", self.finding_codes(report))
        self.assertIn("TOKEN_SEQUENCE_COPY", self.finding_codes(report))

    def test_gpl_and_unrecorded_vendor_material_are_blocking(self) -> None:
        self.write(
            "crates/core/src/lib.rs",
            "// SPDX-License-Identifier: " + "G" + "PL-3.0-only\npub fn copied_component() {}\n",
        )
        self.write("crates/core/vendor/foreign.rs", "pub fn foreign_material_without_a_license_record() {}\n")
        report = doctor.run_doctor(self.root)
        codes = self.finding_codes(report)
        self.assertIn("COPYLEFT_MATERIAL", codes)
        self.assertIn("UNKNOWN_VENDOR_LICENSE", codes)

    def test_distinctive_directory_mirror_is_detected(self) -> None:
        for name in ("alpha_unique.rs", "beta_unique.rs", "gamma_unique.rs"):
            self.write(
                f"crates/core/src/{name}",
                f"pub fn project_{name[:-3]}() -> &'static str {{ \"main-{name}\" }}\n",
            )
            self.write(
                f"research/upstreams/reference/copied/src/{name}",
                f"def upstream_{name[:-3]}():\n    return 'upstream-{name}-different-content'\n",
            )
        report = doctor.run_doctor(self.root)
        self.assertIn("DIRECTORY_STRUCTURE_MIRROR", self.finding_codes(report))

    def test_narrow_path_and_digest_record_allows_reviewed_copy(self) -> None:
        content = (
            "Reviewed interoperability sample with a deliberately exact body and an explicit "
            "path plus digest allow record for this one synthetic test artifact.\n"
        )
        self.write("research/upstreams/reference/reviewed.txt", content)
        main = self.write("crates/core/tests/reviewed.txt", content)
        digest = hashlib.sha256(main.read_bytes()).hexdigest()
        self.write(
            "compliance/borrowing-records.json",
            json.dumps(
                {
                    "schema_version": 1,
                    "records": [
                        {
                            "source": "synthetic public fixture",
                            "license": "MIT",
                            "copy_risk": "low",
                            "implementation_rule": "Only this reviewed fixture is reused.",
                            "notice_action": "Keep the source notice with the fixture.",
                            "allowed_paths": ["crates/core/tests/reviewed.txt"],
                            "allowed_sha256": [digest],
                        }
                    ],
                }
            ),
        )
        report = doctor.run_doctor(self.root)
        self.assertNotIn("EXACT_FILE_COPY", self.finding_codes(report))
        self.assertFalse(report.blocking)

    def test_broad_or_incomplete_record_cannot_suppress_findings(self) -> None:
        self.write(
            "compliance/borrowing-records.json",
            json.dumps(
                {
                    "schema_version": 1,
                    "records": [
                        {
                            "source": "unknown source",
                            "license": "",
                            "copy_risk": "whatever",
                            "implementation_rule": "",
                            "notice_action": "",
                            "allowed_paths": ["crates/core/src/lib.rs"],
                            "allowed_sha256": ["*"],
                        }
                    ],
                }
            ),
        )
        report = doctor.run_doctor(self.root)
        self.assertIn("BORROWING_RECORD_INCOMPLETE", self.finding_codes(report))
        self.assertTrue(report.blocking)


if __name__ == "__main__":
    unittest.main()
