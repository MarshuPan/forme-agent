"""Reject new size debt and forbidden dependency aliases without a real build."""

import tempfile
import unittest
import importlib.util
from pathlib import Path

_spec = importlib.util.spec_from_file_location("forme_code_quality", Path(__file__).parents[1] / "code-quality.py")
_quality = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_quality)
dependency_errors, file_metrics = _quality.dependency_errors, _quality.file_metrics


class CodeQualityTests(unittest.TestCase):
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


if __name__ == "__main__":
    unittest.main()
