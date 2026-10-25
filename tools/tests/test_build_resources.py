"""Exercise preservation, path refusal, busy targets and filesystem accounting."""

import json
import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from tools import build_resources as resources


class BuildResourcesTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        subprocess.run(["git", "init", "--quiet", str(self.root)], check=True)
        subprocess.run(["git", "-c", "user.name=Fixture", "-c", "user.email=fixture@invalid.local",
                        "commit", "--allow-empty", "--quiet", "-m", "fixture"], cwd=self.root, check=True)
        (self.root / ".gitignore").write_text("/target/\n.forme/\nresearch/upstreams/\n", encoding="utf-8")
        (self.root / "Cargo.toml").write_text('[workspace]\nmembers=[]\n', encoding="utf-8")
        self.target = self.root / "target"
        self.cache(self.target)
        self.proc = mock.patch.object(resources, "processes", return_value=[])
        self.proc.start()
        self.addCleanup(self.proc.stop)

    def cache(self, target):
        (target / "debug/deps").mkdir(parents=True)
        (target / "debug/.fingerprint").mkdir()
        (target / "CACHEDIR.TAG").write_text(
            "Signature: 8a477f597d28d172789f06886806bc55\n# Local cache fixture.\n", encoding="utf-8")
        (target / "debug/deps/generated.o").write_bytes(b"object data")

    def test_low_space_refuses_and_environment_values_do_not_enter_report(self):
        with mock.patch.object(resources.shutil, "disk_usage", return_value=mock.Mock(free=resources.GIB)), \
                mock.patch.dict(os.environ, {"RUSTFLAGS": "private-value", "CARGO_TARGET_DIR": "target"}):
            result = resources.check_space(self.root, 20)
        self.assertEqual(result["status"], "LOW_SPACE")
        self.assertEqual(Path(result["target"]), self.target)
        self.assertIn("RUSTFLAGS", result["overrides"])
        self.assertNotIn("private-value", json.dumps(result))

    def test_hardlinks_are_counted_once_and_budget_reports_without_deleting(self):
        first = self.target / "debug/deps/generated.o"
        os.link(first, first.with_name("duplicate.o"))
        measured = resources.size_of(self.target / "debug")
        self.assertEqual(measured["files"], 2)
        self.assertEqual(measured["duplicate_hardlinks"], 1)
        self.assertEqual(measured["logical_bytes"], first.stat().st_size * 2)
        result = resources.report(self.root, False, 0.000000001)
        self.assertEqual(result["over_budget"], [str(self.root)])
        self.assertTrue(first.exists())

    def test_relative_outside_and_unrecognized_targets_are_refused(self):
        for target in [Path("target"), self.root, self.root / ".forme", self.target / "debug",
                       self.target / "no-marker", self.target / ".." / "target"]:
            with self.subTest(target=target), self.assertRaises(ValueError):
                resources.validate_target(self.root, target)

    def test_registered_nested_task_cache_is_accepted(self):
        nested = self.target / "contract-check"
        self.cache(nested)
        checkout, _ = resources.validate_target(self.root, nested)
        self.assertEqual(checkout, self.root)

    def test_link_in_deletion_scope_is_refused(self):
        outside = self.root / "preserved"
        outside.mkdir()
        (outside / "keep.txt").write_text("keep", encoding="utf-8")
        link = self.target / "debug/redirect"
        if os.name == "nt":
            subprocess.run(["cmd.exe", "/d", "/c", "mklink", "/J", str(link), str(outside)],
                           check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            self.addCleanup(lambda: os.rmdir(link) if link.exists() else None)
        else:
            link.symlink_to(outside, target_is_directory=True)
        with self.assertRaises(ValueError):
            resources.validate_target(self.root, self.target)
        self.assertEqual((outside / "keep.txt").read_text(), "keep")

    def test_running_binary_and_unknown_build_users_refuse_without_invoking_clean(self):
        for process in [{"ProcessId": 1, "Name": "fixture.exe", "ExecutablePath": str(self.target / "debug/test.exe")},
                        {"ProcessId": 2, "Name": "cargo.exe", "CommandLine": "cargo test"},
                        {"ProcessId": 3, "Name": "rustc.exe", "CommandLine": None},
                        {"ProcessId": 4, "Name": "rust-lld.exe", "CommandLine": None}]:
            with self.subTest(process=process), mock.patch.object(resources, "processes", return_value=[process]), \
                    mock.patch.object(resources, "run", wraps=resources.run) as command:
                result = resources.clean(self.root, self.target, True)
                self.assertEqual(result["status"], "BUSY")
                self.assertFalse(any(call.args[0][0] == "cargo" for call in command.call_args_list))

    def test_observed_other_worktree_does_not_block_an_idle_cache(self):
        other = self.root / "other/target"
        current = [{"ProcessId": 2, "Name": "cargo.exe", "CommandLine": "cargo test"},
                   {"ProcessId": 3, "ParentProcessId": 2, "Name": "fixture.exe",
                    "ExecutablePath": str(other / "debug/test.exe")}]
        self.assertEqual(resources.active_users(self.target, current, [self.target, other]), [])
        current.pop()
        self.assertTrue(resources.active_users(self.target, current, [self.target, other]))

    def test_explicit_other_cargo_target_is_not_mistaken_for_selected_cache(self):
        current = [{"ProcessId": 4, "Name": "cargo.exe",
                    "CommandLine": f'cargo test --target-dir "{self.root / "target-other"}"'}]
        self.assertEqual(resources.active_users(self.target, current, [self.target]), [])

    def test_clean_is_dry_run_by_default_and_applies_only_after_second_idle_check(self):
        actual_run = resources.run

        def command(arguments, root):
            if arguments[0] != "cargo":
                return actual_run(arguments, root)
            self.assertEqual(arguments[1:5], ["clean", "--profile", "dev", "--frozen"])
            self.assertEqual(Path(arguments[arguments.index("--target-dir") + 1]), self.target)
            if "--dry-run" not in arguments:
                shutil.rmtree(self.target / "debug")
            return "fixture Cargo result"

        for name, content in [("release/installer.bin", b"release"), ("receipt.json", b"evidence")]:
            path = self.target / name
            path.parent.mkdir(exist_ok=True)
            path.write_bytes(content)
        state = self.root / ".forme/owner-data"
        state.parent.mkdir()
        state.write_bytes(b"private fixture")
        with mock.patch.object(resources, "run", side_effect=command):
            result = resources.clean(self.root, self.target, False)
            self.assertEqual(result["status"], "DRY_RUN")
            self.assertTrue((self.target / "debug/deps/generated.o").exists())
            result = resources.clean(self.root, self.target, True)
        self.assertEqual(result["status"], "CLEANED")
        self.assertEqual(result["preserved_before"], result["preserved_after"])
        self.assertEqual((self.target / "receipt.json").read_bytes(), b"evidence")
        self.assertEqual((self.target / "release/installer.bin").read_bytes(), b"release")
        self.assertEqual(state.read_bytes(), b"private fixture")

    def test_new_user_between_preview_and_apply_prevents_deletion(self):
        original = resources.run

        def command(arguments, root):
            return "preview" if arguments[0] == "cargo" else original(arguments, root)

        busy = [{"ProcessId": 9, "Name": "fixture.exe", "ExecutablePath": str(self.target / "debug/test.exe")}]
        with mock.patch.object(resources, "run", side_effect=command) as call, \
                mock.patch.object(resources, "processes", side_effect=[[], busy]):
            result = resources.clean(self.root, self.target, True)
        self.assertEqual(result["status"], "BUSY")
        self.assertEqual(sum(c.args[0][0] == "cargo" for c in call.call_args_list), 1)
        self.assertTrue((self.target / "debug/deps/generated.o").exists())

    @unittest.skipUnless(shutil.which("cargo"), "Cargo is required for the native cleanup contract")
    def test_real_cargo_clean_preserves_receipt_and_runtime(self):
        (self.root / "Cargo.toml").write_text(
            '[package]\nname="forme-cache-fixture"\nversion="0.0.0"\nedition="2021"\n', encoding="utf-8")
        (self.root / "src").mkdir()
        (self.root / "src/lib.rs").write_text("#![forbid(unsafe_code)]\n", encoding="utf-8")
        resources.run(["cargo", "generate-lockfile", "--offline"], self.root)
        receipt = self.target / "acceptance-receipt.json"
        receipt.write_bytes(b"original fixture receipt")
        data = self.root / ".forme/brain.db"
        data.parent.mkdir()
        data.write_bytes(b"local fixture data")
        result = resources.clean(self.root, self.target, True)
        self.assertEqual(result["status"], "CLEANED")
        self.assertFalse((self.target / "debug").exists())
        self.assertEqual(receipt.read_bytes(), b"original fixture receipt")
        self.assertEqual(data.read_bytes(), b"local fixture data")

    def test_changed_preserved_content_is_not_reported_as_success(self):
        receipt = self.target / "receipt.json"
        receipt.write_bytes(b"original")
        original = resources.run

        def command(arguments, root):
            if arguments[0] != "cargo":
                return original(arguments, root)
            if "--dry-run" not in arguments:
                shutil.rmtree(self.target / "debug")
                receipt.write_bytes(b"changed")
            return "fixture"

        with mock.patch.object(resources, "run", side_effect=command):
            result = resources.clean(self.root, self.target, True)
        self.assertEqual(result["status"], "PRESERVATION_FAILED")


if __name__ == "__main__":
    unittest.main()
