"""Reject new size debt and forbidden dependency aliases without a real build."""

import tempfile
import unittest
import importlib.util
import subprocess
from pathlib import Path

_spec = importlib.util.spec_from_file_location("forme_code_quality", Path(__file__).parents[1] / "code-quality.py")
_quality = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_quality)
dependency_errors, file_metrics = _quality.dependency_errors, _quality.file_metrics


class CodeQualityTests(unittest.TestCase):
    def test_standard_rust_test_modules_use_the_reviewed_test_limit(self):
        self.assertEqual(_quality.limit_for("crates/gateway/src/web/identity/tests.rs"), 1200)
        self.assertEqual(_quality.limit_for("crates/gateway/src/web/identity/storage.rs"), 600)

    def test_existing_debt_can_shrink_but_not_grow(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path = "module.rs"
            (root / path).write_text("line\n" * 700, encoding="utf-8")
            baseline = {path: {"max_lines": 700, "reason": "reviewed pre-existing debt"}}
            self.assertFalse(file_metrics(root, [path], baseline)[0])
            (root / path).write_text("line\n" * 701, encoding="utf-8")
            self.assertTrue(file_metrics(root, [path], baseline)[0])
            self.assertTrue(file_metrics(root, [path], {})[0])

    def test_unexplained_or_boolean_cap_is_not_an_exemption(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "module.rs").write_text("line\n", encoding="utf-8")
            for entry in [{"max_lines": 800}, {"max_lines": True, "reason": "test"}]:
                self.assertTrue(file_metrics(root, ["module.rs"], {"module.rs": entry})[0])

    def test_alias_and_target_dependency_cannot_evade_the_graph(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            subprocess.run(["git", "init", "--quiet", str(root)], check=True)
            crate = root / "crates/gateway"
            crate.mkdir(parents=True)
            (crate / "Cargo.toml").write_text(
                '[package]\nname="forme-gateway"\nversion="0.0.1"\n'
                '[target.\'cfg(windows)\'.dev-dependencies]\n'
                'renamed = { package="forme-store", path="../store" }\n', encoding="utf-8",
            )
            errors = dependency_errors(root)
            self.assertIn("gateway: forbidden internal dependency forme-store", errors)
            self.assertIn("crate set differs from the frozen 18-crate graph", errors)

    def test_every_rust_binary_entrypoint_requires_unsafe_prohibition(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            subprocess.run(["git", "init", "--quiet", str(root)], check=True)
            crate = root / "crates/gateway"
            (crate / "src/bin/nested").mkdir(parents=True)
            (crate / "Cargo.toml").write_text(
                '[package]\nname="forme-gateway"\nversion="0.0.1"\n', encoding="utf-8",
            )
            (crate / "src/lib.rs").write_text("#![forbid(unsafe_code)]\n", encoding="utf-8")
            (crate / "src/bin/tool.rs").write_text("fn main() {}\n", encoding="utf-8")
            (crate / "src/bin/nested/main.rs").write_text("fn main() {}\n", encoding="utf-8")
            errors = dependency_errors(root)
            self.assertIn("crates/gateway/src/bin/tool.rs: missing unsafe prohibition", errors)
            self.assertIn("crates/gateway/src/bin/nested/main.rs: missing unsafe prohibition", errors)

    def test_native_exception_is_confined_and_cannot_be_added_to_a_third_file(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            subprocess.run(["git", "init", "--quiet", str(root)], check=True)
            for relative in _quality.NATIVE_UNSAFE_DENY_BOUNDARIES:
                path = root / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text("#![deny(unsafe_code)]\n", encoding="utf-8")
            for relative in _quality.NATIVE_UNSAFE_BOUNDARIES:
                path = root / relative
                path.write_text("#![allow(unsafe_code)]\n", encoding="utf-8")
            forbidden = "crates/execution/src/installed/foreign.rs"
            path = root / forbidden
            for text in ["#![allow(unsafe_code)]\n", "#![ allow ( unsafe_code ) ]\n",
                         "#![expect(unsafe_code)]\n", "unsafe fn escape() {}\n",
                         "unsafe impl Send for Escaped {}\n"]:
                with self.subTest(source=text):
                    path.write_text(text, encoding="utf-8")
                    errors = dependency_errors(root)
                    self.assertIn(
                        f"{forbidden}: unsafe code is outside the audited native boundary", errors,
                    )
                    for approved in _quality.NATIVE_UNSAFE_BOUNDARIES:
                        self.assertFalse(any(error.startswith(approved + ":") for error in errors))
            approved = next(iter(_quality.NATIVE_UNSAFE_BOUNDARIES))
            (root / approved).write_text("fn missing_scope() {}\n", encoding="utf-8")
            self.assertIn(
                f"{approved}: native unsafe boundary is not explicitly scoped", dependency_errors(root),
            )

    def test_frontend_and_build_scripts_are_checked_but_vendor_output_is_not(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            subprocess.run(["git", "init", "--quiet", str(root)], check=True)
            for relative in ["frontend/src/App.tsx", "frontend/src/tests/client.test.ts",
                             "tools/build-web.mjs", "packaging/windows/service-access.psm1",
                             "packaging/windows/forme.iss", "frontend/dist/assets/app.js",
                             "frontend/node_modules/foreign/index.js"]:
                target = root / relative
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_text("line\n" * 601, encoding="utf-8")
            paths = _quality.source_paths(root)
            self.assertEqual(paths, ["frontend/src/App.tsx", "frontend/src/tests/client.test.ts",
                                     "packaging/windows/forme.iss", "packaging/windows/service-access.psm1",
                                     "tools/build-web.mjs"])
            errors, _ = file_metrics(root, paths, {})
            self.assertEqual(len(errors), 4)
            self.assertTrue(any("App.tsx" in error for error in errors))
            self.assertTrue(any("build-web.mjs" in error for error in errors))


if __name__ == "__main__":
    unittest.main()
