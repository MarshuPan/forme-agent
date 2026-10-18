from __future__ import annotations

import contextlib
import importlib.util
import io
import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from dataclasses import asdict, replace
from pathlib import Path
from unittest import mock


SCRIPT = Path(__file__).resolve().parents[1] / "core_brain_release_audit.py"
SPEC = importlib.util.spec_from_file_location("forme_core_brain_release_audit_tests", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
core = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = core
SPEC.loader.exec_module(core)


def passing_body():
    checks = [core.audit._result(name, "self-authored-fixture", "fixture/1", "sha256:" + "a" * 64, [])
              for name in core.audit.EXPECTED_CHECK_IDS]
    return core.audit.ReleaseAuditBody(1, core.audit.TOOL_VERSION, "sha256:" + "b" * 64, "pass", False, checks)


def inventory_fixture():
    rows = []
    for wave, (first, last) in core.WAVES.items():
        if wave == "D":
            continue
        rows.extend(dict(wave=wave, id=f"C{n}-fixture", package="forme-protocol",
                         target=["--test", "fixture_" + wave.lower()], test=f"witness_{n}", include_ignored=False)
                    for n in range(first, last + 1))
    for (package, target), selectors in core.D_TARGETS.items():
        for index, selector in enumerate(selectors or ("",)):
            rows.append(dict(wave="D", id="C19-C24-fixture", package=package, target=list(target),
                             test=selector + f"witness_{index}" if not selector or selector.endswith(("::", "_")) else selector,
                             include_ignored=False))
    return {"schema_version": 1, "scenarios": rows}


class ScriptedCommands:
    """Self-authored subprocess responses for audit glue; no live evidence."""

    def __init__(self, root: Path):
        self.root = root
        self.inventory = inventory_fixture()
        self.calls = []
        self.failure = None
        self.verdict = "Pass"
        self.verifier_code = 0
        self.empty_lists = False
        self.extra_d_test = False
        self.extra_model_tests = set()
        self.extra_m3_tests = set()
        self.target_directory = str(root / "target")

    def __call__(self, command, root):
        self.calls.append(command)
        code, output = 0, ""
        if command[0] == "cargo" and command[1] == "metadata":
            output = json.dumps(dict(workspace_root=str(root), target_directory=self.target_directory,
                                     workspace_members=list(range(18)),
                                     packages=[dict(id=n, name="forme-" + chr(97 + n)) for n in range(18)]))
        elif "-ListScenarios" in command:
            output = json.dumps(self.inventory)
        elif command[0] == "powershell" and core.WORKSPACE_GATE in command:
            output = "core-brain-workspace-contract: PASS\n"
        elif command[0] == "powershell" and "-RuntimeOnly" in command:
            output = "core-brain-runtime (D): PASS (runtime only; not wave D or Final acceptance)\n"
        elif command[:2] == ["cargo", "test"] and "--list" in command:
            package = command[command.index("-p") + 1]
            flag = next(flag for flag in ("--lib", "--test", "--bin") if flag in command)
            target = [flag] if flag == "--lib" else [flag, command[command.index(flag) + 1]]
            names = {r["test"] for r in self.inventory["scenarios"] if r["package"] == package and r["target"] == target}
            if self.extra_d_test and target == ["--test", "v1_data_controls"]:
                names.add("unlisted_d_security_regression")
            if package == "forme-models":
                names.update(self.extra_model_tests)
            if target == ["--test", "m3_b_harness_contract"]:
                names.update(self.extra_m3_tests)
            output = "" if self.empty_lists else "".join(name + ": test\n" for name in sorted(names))
        elif Path(command[0]).name.startswith("forme-core-brain-d-artifact-verify"):
            output, code = f"core-brain-d artifact: {self.verdict}\n", self.verifier_code
        if self.failure is not None and self.failure(command):
            code = 1
        return subprocess.CompletedProcess(command, code, output, "")


class CoreBrainReleaseAuditTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="forme-core-audit-test-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        self.environment = mock.patch.dict(os.environ, {"CARGO_TARGET_DIR": "", "CARGO_BUILD_TARGET": ""})
        self.environment.start()
        self.addCleanup(self.environment.stop)
        self.commands = ScriptedCommands(self.root)

    def write(self, relative, value="self-authored fixture\n"):
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(value, encoding="utf-8")
        return path

    def git(self, *args):
        return subprocess.run(["git", *args], cwd=self.root, check=True, capture_output=True, text=True)

    def fixtures(self):
        paths = set(core.REQUIRED_FILES) | {"Cargo.toml", "Cargo.lock"}
        paths.update(core._fixture_path(row["package"], tuple(row["target"]))
                     for row in self.commands.inventory["scenarios"])
        for relative in paths:
            self.write(relative)
        self.product = self.write(core.PRODUCT_ARTIFACT, json.dumps({"source_commit": "a" * 40, "grade": "LiveConfigured"}))
        self.executable = self.write("target/debug/forme-core-brain-d-artifact-verify" + (".exe" if os.name == "nt" else ""))
        self.git("init", "-q")
        self.git("add", "--", ".")

    def codes(self, findings):
        return {finding.code for finding in findings}

    def rewrite_receipt(self, root, mutate):
        receipt = core.audit.write_artifact(root, passing_body())
        raw = json.loads(receipt.path.read_text(encoding="utf-8"))
        mutate(raw)
        raw["digest"] = core.audit._sha256(core.audit.ARTIFACT_DOMAIN + core.audit._canonical(raw["body"]))
        receipt.path.unlink()
        path = root / ("release-audit-" + raw["digest"].removeprefix("sha256:") + ".json")
        path.write_text(json.dumps(raw), encoding="utf-8")

    def test_profile_reuses_m5_with_independent_closed_schema(self):
        self.assertEqual(core.audit.EXPECTED_CHECK_IDS[-2:], ["ecosystem_threat_profile", "core_brain_closure"])
        self.assertEqual(core.audit.ARTIFACT_TYPE, "forme-core-brain-release-audit")
        self.assertNotEqual(core.audit.ARTIFACT_DOMAIN, b"forme-m5-release-audit-v1\0")
        body = passing_body()
        receipt = core.audit.write_artifact(self.root, body)
        self.assertEqual(core.audit.verify_artifact_set(self.root), receipt)
        self.assertEqual(json.loads(receipt.path.read_text())["body"], asdict(body))
        spec = importlib.util.spec_from_file_location("forme_m5_independence_fixture", SCRIPT.with_name("m5_release_audit.py"))
        other = importlib.util.module_from_spec(spec)
        sys.modules[spec.name] = other
        spec.loader.exec_module(other)
        self.assertEqual(other.audit.ARTIFACT_TYPE, "forme-m5-release-audit")
        self.assertNotIn("core_brain_closure", other.audit.EXPECTED_CHECK_IDS)

    def test_missing_reordered_or_duplicate_checkset_is_rejected(self):
        body = passing_body()
        for index, checks in enumerate((body.checks[:-1], list(reversed(body.checks)), [*body.checks, body.checks[-1]])):
            with self.subTest(index=index), self.assertRaises(ValueError):
                core.audit.write_artifact(self.root / str(index), replace(body, checks=checks))

    def test_resealed_unknown_fields_and_wrong_primitive_types_are_rejected(self):
        mutations = [
            lambda a: a.update(schema_version=True),
            lambda a: a["body"].update(schema_version="1"),
            lambda a: a["body"].update(blocking="false"),
            lambda a: a["body"].update(root_digest="not-a-digest"),
            lambda a: a["body"]["checks"][-1].update(blocking=0),
            lambda a: a["body"]["checks"][-1].update(tool_version=1),
            lambda a: a["body"]["checks"][-1].update(unknown=True),
            lambda a: a["body"]["checks"][-1].update(observations=[dict(code=1, path="fixture", detail="fixture")]),
            lambda a: a.update(artifact_type="forme-m5-release-audit"),
        ]
        for index, mutate in enumerate(mutations):
            path = self.root / str(index)
            with self.subTest(index=index):
                self.rewrite_receipt(path, mutate)
                with self.assertRaises(ValueError):
                    core.audit.verify_artifact_set(path)

    def test_unavailable_check_cannot_be_a_pass_receipt(self):
        body = passing_body()
        check = core.audit._result("core_brain_closure", "fixture", "fixture/1", "sha256:" + "c" * 64,
                                   [core._finding("C24_UNVERIFIABLE", core.PRODUCT_ARTIFACT, "no credentials")])
        body = replace(body, checks=[*body.checks[:-1], check], blocking=True, outcome="fail")
        receipt = core.audit.write_artifact(self.root, body)
        self.assertEqual(core.audit.verify_artifact_set(self.root, require_pass=False), receipt)
        with self.assertRaises(ValueError):
            core.audit.verify_artifact_set(self.root)
        self.assertEqual(core._exit_code(check.findings), 2)

    def test_validated_inventory_and_product_are_required_without_recursive_final(self):
        self.fixtures()
        check = core.core_brain_closure_check(self.root, self.commands)
        self.assertFalse(check.blocking, check.findings)
        self.assertEqual(check.check_id, "core_brain_closure")
        self.assertTrue(any("-RuntimeOnly" in c for c in self.commands.calls))
        self.assertFalse(any("Final" in c for c in self.commands.calls))
        verifier = [c for c in self.commands.calls if Path(c[0]).name.startswith("forme-core-brain-d-artifact-verify")]
        self.assertEqual(verifier, [[str(self.executable), str(self.product), "--require-pass"]])
        self.assertIn(["cargo", "test", "--locked", "-p", "forme-harness", "--bin",
                       "forme-core-brain-d-golden", "--", "--list"], self.commands.calls)

    def test_fixtures_and_artifact_fail_closed_even_if_commands_return_success(self):
        check = core.core_brain_closure_check(self.root, self.commands)
        self.assertTrue(check.blocking)
        self.assertIn("CORE_BRAIN_FIXTURE_MISSING", self.codes(check.findings))
        self.assertIn("CORE_BRAIN_ARTIFACT_MISSING", self.codes(check.findings))
        self.fixtures()
        (self.root / "crates/store/src/content.rs").unlink()
        check = core.core_brain_closure_check(self.root, self.commands)
        self.assertIn("CORE_BRAIN_FIXTURE_MISSING", self.codes(check.findings))

    def test_zero_test_success_and_incomplete_d_inventory_are_rejected(self):
        self.fixtures()
        self.commands.empty_lists = True
        check = core.core_brain_closure_check(self.root, self.commands)
        self.assertIn("CORE_BRAIN_SCENARIO_MISSING", self.codes(check.findings))
        self.commands.empty_lists = False
        self.commands.extra_d_test = True
        check = core.core_brain_closure_check(self.root, self.commands)
        self.assertIn("CORE_BRAIN_SCENARIO_MISSING", self.codes(check.findings))

    def test_invalid_inventory_and_unavailable_commands_are_blocking(self):
        self.fixtures()
        self.commands.inventory["scenarios"][0]["include_ignored"] = "false"
        check = core.core_brain_closure_check(self.root, self.commands)
        self.assertIn("CORE_BRAIN_INVENTORY_INVALID", self.codes(check.findings))
        self.commands.inventory = inventory_fixture()
        self.commands.failure = lambda c: core.WORKSPACE_GATE in c or "-RuntimeOnly" in c
        check = core.core_brain_closure_check(self.root, self.commands)
        self.assertTrue({"CORE_BRAIN_WORKSPACE_CONTRACT_FAILED", "CORE_BRAIN_RUNTIME_FAILED"} <= self.codes(check.findings))
        def unavailable(command, root):
            raise FileNotFoundError("self-authored missing command")
        self.assertTrue(core.core_brain_closure_check(self.root, unavailable).blocking)

    def test_models_selects_complete_registry_and_only_the_exact_endpoint_test(self):
        self.fixtures()
        endpoint = "tests::endpoint_error_text_cannot_forge_retryability_or_escape_to_diagnostics"
        self.commands.extra_model_tests = {"tests::unrelated_historical_unit", endpoint + "_suffix"}
        self.assertFalse(core.core_brain_closure_check(self.root, self.commands).blocking)
        self.commands.extra_model_tests.add("registry::tests::new_required_security_case")
        check = core.core_brain_closure_check(self.root, self.commands)
        self.assertIn("CORE_BRAIN_SCENARIO_MISSING", self.codes(check.findings))
        row = next(r for r in self.commands.inventory["scenarios"] if r["test"] == endpoint)
        for invalid in ("tests::unrelated_historical_unit", endpoint + "_suffix"):
            with self.subTest(invalid=invalid):
                row["test"] = invalid
                with self.assertRaises(ValueError):
                    core._inventory(json.dumps(self.commands.inventory))

    def test_d19_prefix_requires_new_cases_and_excludes_historical_s_tests(self):
        self.fixtures()
        self.commands.extra_m3_tests = {"s65_historical_adaptation", "historical_c19_not_a_prefix"}
        self.assertFalse(core.core_brain_closure_check(self.root, self.commands).blocking)
        self.commands.extra_m3_tests.add("c19_new_required_adaptation_case")
        check = core.core_brain_closure_check(self.root, self.commands)
        self.assertIn("CORE_BRAIN_SCENARIO_MISSING", self.codes(check.findings))

    def test_typed_inventory_rejects_missing_duplicate_and_unsafe_entries(self):
        mutations = [
            lambda a: a.update(schema_version=True),
            lambda a: a["scenarios"].pop(),
            lambda a: a["scenarios"].append(a["scenarios"][0]),
            lambda a: a["scenarios"][0].update(target=["--all-targets"]),
            lambda a: a["scenarios"][0].update(target=["--bin", "../escape"]),
            lambda a: a["scenarios"][0].update(package="forme-../../escape"),
            lambda a: a["scenarios"][0].update(test="--ignored"),
            lambda a: a["scenarios"][0].update(id="C25-fixture"),
        ]
        for mutate in mutations:
            value = inventory_fixture()
            mutate(value)
            with self.assertRaises(ValueError):
                core._inventory(json.dumps(value))

    def test_unverifiable_or_false_success_verifier_never_passes_c24(self):
        self.fixtures()
        for verdict, code in (("Unverifiable", 2), ("Unverifiable", 0), ("Fail", 0), ("Pass", 1), ("", 0)):
            with self.subTest(verdict=verdict, code=code):
                self.commands.verdict, self.commands.verifier_code = verdict, code
                self.assertTrue(core.verify_product_artifact(self.root, self.product, self.commands))
        self.commands.verdict, self.commands.verifier_code = "Pass", 0
        for grade in ("Controlled", "Unavailable"):
            self.product.write_text(json.dumps({"source_commit": "a" * 40, "grade": grade}))
            self.assertIn("CORE_BRAIN_C24_UNVERIFIABLE", self.codes(core.verify_product_artifact(self.root, self.product, self.commands)))

    def test_missing_invalid_or_missing_binary_product_is_not_pass(self):
        missing = self.root / core.PRODUCT_ARTIFACT
        self.assertEqual(core._exit_code(core.verify_product_artifact(self.root, missing, self.commands)), 2)
        self.fixtures()
        self.executable.unlink()
        self.assertIn("CORE_BRAIN_C24_VERIFIER_UNAVAILABLE", self.codes(core.verify_product_artifact(self.root, self.product, self.commands)))
        self.product.write_text('{"source_commit":"missing"}')
        self.assertIn("CORE_BRAIN_C24_INVALID", self.codes(core.verify_product_artifact(self.root, self.product, self.commands)))

    def test_source_commit_must_exist_be_ancestor_and_match_head_and_worktree(self):
        self.fixtures()
        for failure in (lambda c: c[:2] == ["git", "cat-file"],
                        lambda c: c[:2] == ["git", "merge-base"],
                        lambda c: c[:3] == ["git", "diff", "--quiet"] and "a" * 40 in c,
                        lambda c: c[:4] == ["git", "diff", "--quiet", "HEAD"]):
            self.commands.failure = failure
            self.assertIn("CORE_BRAIN_C24_SOURCE_MISMATCH", self.codes(core.verify_product_artifact(self.root, self.product, self.commands)))
        self.commands.failure = None
        def untracked(command, root):
            if command[:3] == ["git", "ls-files", "--others"]:
                return subprocess.CompletedProcess(command, 0, "crates/store/src/new.rs\n", "")
            return self.commands(command, root)
        self.assertIn("CORE_BRAIN_C24_SOURCE_MISMATCH", self.codes(core.verify_product_artifact(self.root, self.product, untracked)))

    def test_shared_or_config_redirected_build_outputs_are_rejected(self):
        self.fixtures()
        with mock.patch.dict(os.environ, {"CARGO_TARGET_DIR": str(self.root / "shared")}):
            self.assertIn("CORE_BRAIN_SHARED_TARGET_DIR", self.codes(core.verify_product_artifact(self.root, self.product, self.commands)))
        self.commands.target_directory = str(self.root / "shared-by-config")
        self.assertIn("CORE_BRAIN_WORKSPACE_METADATA_INVALID", self.codes(core.verify_product_artifact(self.root, self.product, self.commands)))
        self.commands.target_directory = str(self.root / "target")
        with mock.patch.dict(os.environ, {"CARGO_BUILD_TARGET": "fixture-target"}):
            self.assertIn("CORE_BRAIN_BUILD_TARGET_OVERRIDE", self.codes(core.verify_product_artifact(self.root, self.product, self.commands)))

    def test_real_git_source_allows_docs_only_ancestor_and_rejects_new_crate_sources(self):
        self.fixtures()
        def commit():
            self.git("add", "--", ".")
            self.git("-c", "user.name=Audit fixture", "-c", "user.email=audit@example.invalid", "commit", "-q", "-m", "self-authored fixture")
            return self.git("rev-parse", "HEAD").stdout.strip()
        source = commit()
        self.product.write_text(json.dumps({"source_commit": source, "grade": "LiveConfigured"}))
        commit()
        def actual_git(command, root):
            return core.audit._run(command, root) if command[0] == "git" else self.commands(command, root)
        self.assertEqual(core.verify_product_artifact(self.root, self.product, actual_git), [])
        self.write("crates/store/src/content.rs", "changed source\n")
        self.assertIn("CORE_BRAIN_C24_SOURCE_MISMATCH", self.codes(core.verify_product_artifact(self.root, self.product, actual_git)))
        commit()
        self.assertIn("CORE_BRAIN_C24_SOURCE_MISMATCH", self.codes(core.verify_product_artifact(self.root, self.product, actual_git)))

    def test_m5_receipts_and_core_reports_do_not_create_digest_cycle(self):
        self.write("Cargo.toml")
        receipt_path = "docs/acceptance/m5-release-audit-artifacts/release-audit-" + "a" * 64 + ".json"
        for relative in (receipt_path, *core.REPORT_EXCLUSIONS):
            self.write(relative, "{}")
        self.write(core.audit.ARTIFACT_PREFIX + "release-audit-" + "b" * 64 + ".json", "{}")
        self.write("docs/acceptance/m5-artifacts/nonreceipt.json", "{}")
        self.git("init", "-q")
        self.git("add", "--", ".")
        with mock.patch.object(core.m5, "run_m5_release_audit", return_value=replace(passing_body(), checks=passing_body().checks[:-1])), \
                mock.patch.object(core, "core_brain_closure_check", return_value=passing_body().checks[-1]):
            core.run_core_brain_release_audit(self.root)
        def digests():
            _, tracked, digest = core.audit._release_tree_check(self.root)
            return digest, core.audit._artifact_check(self.root, tracked).input_digest
        before = digests()
        for relative in (receipt_path, *core.REPORT_EXCLUSIONS,
                         core.audit.ARTIFACT_PREFIX + "release-audit-" + "b" * 64 + ".json"):
            self.write(relative, '{"self_authored":"updated receipt"}')
        self.git("add", "--", ".")
        self.assertEqual(digests(), before)
        self.write("docs/acceptance/m5-artifacts/nonreceipt.json", '{"witness":"changed"}')
        self.git("add", "--", ".")
        self.assertNotEqual(digests(), before)

    def test_current_receipt_comparison_rejects_stale_content(self):
        first, second = self.root / "first", self.root / "second"
        body = passing_body()
        core.audit.write_artifact(first, body)
        core.audit.write_artifact(second, replace(body, root_digest="sha256:" + "c" * 64))
        with self.assertRaises(ValueError):
            core.audit.compare_artifact_sets(first, second)

    def test_cli_missing_product_returns_two_without_writing_evidence(self):
        with contextlib.redirect_stdout(io.StringIO()) as output:
            code = core.main(["--root", str(self.root), "--verify-product", str(self.root / "missing.json")])
        self.assertEqual(code, 2)
        self.assertIn("UNVERIFIABLE", output.getvalue())
        self.assertEqual(list(self.root.iterdir()), [])

    @unittest.skipUnless(shutil.which("powershell"), "PowerShell gate is Windows-specific")
    def test_repository_inventory_contains_every_d_test_and_explicit_ignored_cli(self):
        root = SCRIPT.parents[1]
        result = subprocess.run(core._powershell(str(root / core.GATE), "-ListScenarios"),
                                cwd=root, check=True, capture_output=True, text=True)
        rows = core._inventory(result.stdout)
        d_rows = [r for r in rows if r["wave"] == "D"]
        ignored = [r for r in d_rows if r["include_ignored"]]
        self.assertEqual(len(ignored), 1)
        self.assertEqual(ignored[0]["test"], "c20_c21_c23_cli_data_and_brain_use_authenticated_real_daemon_without_local_writer")
        import re
        for (package, target), selectors in core.D_TARGETS.items():
            if target[0] != "--test":
                continue
            source = (root / core._fixture_path(package, target)).read_text(encoding="utf-8")
            tests = set(re.findall(r"#\[test\]\s*(?:#\[[^\n]+\]\s*)*fn\s+(\w+)\s*\(", source))
            tests = {name for name in tests if core._selected_test(name, selectors)}
            inventory = {r["test"] for r in d_rows if r["package"] == package and r["target"] == list(target)}
            self.assertEqual(inventory, tests)

    @unittest.skipUnless(shutil.which("powershell"), "PowerShell gate is Windows-specific")
    def test_runtime_only_cannot_be_used_for_other_waves_or_final(self):
        root = SCRIPT.parents[1]
        for wave in ("A", "B", "C", "Final"):
            result = subprocess.run(core._powershell(str(root / core.GATE), "-Wave", wave, "-RuntimeOnly", "-ListScenarios"),
                                    cwd=root, capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("supported only with -Wave D", result.stdout)


if __name__ == "__main__":
    unittest.main()
