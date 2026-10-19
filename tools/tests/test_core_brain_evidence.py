from __future__ import annotations

import contextlib
import importlib.util
import io
import json
import os
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest import mock


SPEC = importlib.util.spec_from_file_location("forme_evidence_fixture", Path(__file__).parents[1] / "core_brain_evidence.py")
evidence = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(evidence)


class EvidenceTests(unittest.TestCase):
    """Self-authored schema/negative fixtures, never live-model evidence."""

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="forme-evidence-test-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.write("crates/fixture.rs", "self-authored fixture\n")
        self.git("init", "-q")
        self.git("add", ".")
        self.git("-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "commit", "-qm", "fixture")
        self.source = self.git("rev-parse", "HEAD").strip()
        self.rows = [dict(wave=wave, id="C" + str(index + 1), package="forme-protocol",
                          target=["--lib"], test="self_authored_" + wave.lower(), include_ignored=False)
                     for index, wave in enumerate(evidence.FILES)]
        for wave in evidence.FILES:
            for name in evidence.FILES[wave]:
                value = {"schema_version": 1, "graph_snapshot": "sha256:" + "c" * 64}
                if name == "entrypoints.json":
                    value = self.entrypoint()
                elif name == "tick.json":
                    value = self.tick()
                elif name == "product-golden.json":
                    value = {"schema_version": 1, "source_commit": self.source, "grade": "Unavailable"}
                elif name == "score.json":
                    value = {"verdict": "Unverifiable"}
                self.write(f"{evidence.BASE}/core-brain-{wave.lower()}-artifacts/{name}",
                           json.dumps(value) if name.endswith(".json") else "Self-authored report\n")
            self.runtime(wave)

    def git(self, *args):
        return subprocess.run(["git", *args], cwd=self.root, check=True, capture_output=True, text=True).stdout

    def write(self, name, value):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(value, encoding="utf-8")
        return path

    def entrypoint(self):
        binding = dict(composition_manifest="sha256:" + "a" * 64, policy_profile="policy:fixture",
                       model_profile="model:fixture", toolset_ref="toolset:fixture", workspace="workspace:fixture")
        kinds = ("RunAccepted", "SessionBound", "ContextBuildStarted", "ContextBuildFinished",
                 "ModelCallStarted", "ModelCallFinished", "RunComplete")
        return dict(schema_version=1, kind="forme-core-brain-entrypoints", grade="ControlledRuntime",
                    source_commit=self.source, composition_digest=binding["composition_manifest"],
                    taxonomy_digest="sha256:" + "b" * 64, model_calls=2,
                    streams=[dict(run="fixture:" + source, source=source, session_bound=binding,
                                  positions=[dict(seq=i, kind=kind, digest="sha256:" + "c" * 64)
                                             for i, kind in enumerate(kinds, 1)])
                             for source in ("UserTurn", "Scheduler")])

    def runtime(self, wave):
        value = dict(schema_version=1, kind="forme-core-brain-runtime", wave=wave, source_commit=self.source,
                     scenarios=[dict(**row, passed=1, failed=0, ignored=0) for row in self.rows if row["wave"] == wave])
        path = evidence.wave_dir(self.root, wave) / "runtime.json"
        evidence.write(path, value)
        return evidence.runtime_report(self.root, path, wave, self.rows, seal=True)

    def tick(self):
        kinds = ("ObservationRecorded", "ProactiveProposalEmitted", "ReflectionProduced", "MemoryMaintenanceApplied")
        return dict(schema_version=1, kind="forme-core-brain-tick", grade="ControlledRuntime",
                    source_commit=self.source, composition_digest="sha256:" + "a" * 64,
                    cognition_snapshot="sha256:" + "b" * 64, report_digest="sha256:" + "c" * 64,
                    run="fixture:tick", stream_heads={"fixture:tick": len(kinds)},
                    phases=["ObserveAndPropose", "ReflectAndCandidate", "MaintainMemory"],
                    positions=[dict(seq=i, kind=kind, digest="sha256:" + "c" * 64)
                               for i, kind in enumerate(kinds, 1)],
                    tick_model_calls=0, endpoint_calls_before=1, endpoint_calls_after=1)

    def seal(self):
        for wave in evidence.FILES:
            evidence.write(evidence.wave_dir(self.root, wave) / "manifest.json",
                           evidence.sealed(evidence.manifest_body(self.root, wave, self.rows)))
        path = self.root / evidence.BASE / "core-brain-closure-artifacts/manifest.json"
        evidence.write(path, evidence.sealed(evidence.bundle_body(self.root, self.rows)))
        return path

    def test_closed_bundle_roundtrip_preserves_unverifiable_product(self):
        self.seal()
        value = evidence.verify_bundle(self.root, self.rows)
        self.assertEqual(value["verdict"], "Unverifiable")
        self.assertEqual([row["wave"] for row in value["waves"]], list(evidence.FILES))
        with mock.patch.object(evidence, "inventory", return_value=self.rows), contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(evidence.main(["--root", str(self.root), "--verify", "--require-pass"]), 2)
            self.assertEqual(evidence.main(["--root", str(self.root), "--verify"]), 0)
            self.assertEqual(evidence.main(["--root", str(self.root), "--verify-wave", "A", "--require-pass"]), 0)
            self.assertEqual(evidence.main(["--root", str(self.root), "--verify-wave", "D", "--require-pass"]), 2)

    def test_runtime_requires_exact_execution_counts_and_inventory(self):
        path = evidence.wave_dir(self.root, "A") / "runtime.json"
        good = evidence.load(path)
        for field, bad in (("passed", 0), ("passed", True), ("ignored", 1), ("failed", 1), ("test", "not_executed")):
            value = json.loads(json.dumps(good))
            value["scenarios"][0][field] = bad
            evidence.write(path, value)
            with self.subTest(field=field, bad=bad), self.assertRaises(ValueError):
                evidence.runtime_report(self.root, path, "A", self.rows)
        evidence.write(path, good)
        with self.assertRaises(ValueError):
            evidence.runtime_report(self.root, path, "A", self.rows + [dict(self.rows[0], test="new_test")])

    def test_source_and_untracked_changes_invalidate_receipts(self):
        path = evidence.wave_dir(self.root, "A") / "runtime.json"
        self.write("crates/fixture.rs", "changed source\n")
        with self.assertRaises(ValueError):
            evidence.runtime_report(self.root, path, "A", self.rows)
        self.write("crates/fixture.rs", "self-authored fixture\n")
        self.write("tools/untracked.py", "untracked source\n")
        with self.assertRaises(ValueError):
            evidence.runtime_report(self.root, path, "A", self.rows)

    def test_closed_file_set_and_byte_tamper_are_rejected(self):
        self.seal()
        folder = evidence.wave_dir(self.root, "B")
        extra = self.write(f"{evidence.BASE}/core-brain-b-artifacts/extra.json", "{}")
        with self.assertRaises(ValueError):
            evidence.verify_bundle(self.root, self.rows)
        extra.unlink()
        path = folder / "relevance-comparison.json"
        path.write_text(path.read_text() + "\n", encoding="utf-8")
        with self.assertRaises(ValueError):
            evidence.verify_bundle(self.root, self.rows)
        path.unlink()
        with self.assertRaises(ValueError):
            evidence.manifest_body(self.root, "B", self.rows)

    def test_unknown_and_duplicate_json_fields_cannot_be_resealed(self):
        path = self.seal()
        value = evidence.load(path)
        value["unknown"] = True
        evidence.write(path, evidence.sealed(value))
        with self.assertRaises(ValueError):
            evidence.verify_bundle(self.root, self.rows)
        path.write_text('{"schema_version":1,"schema_version":1}', encoding="utf-8")
        with self.assertRaises(ValueError):
            evidence.load(path)
        path.write_text('{"invalid":NaN}', encoding="utf-8")
        with self.assertRaises(ValueError):
            evidence.load(path)

    def test_private_material_is_rejected_without_echoing_values(self):
        path = evidence.wave_dir(self.root, "B") / "relevance-comparison.json"
        for text in ('{"base_url":"hidden"}', '{"note":"https://private.example.invalid"}',
                     '{"note":"/home/owner/data"}', '{"note":"bearer fixture-value"}',
                     json.dumps({"note": r"C:\Users\fixture\private"}),
                     '{"note":"\\u0068ttps://private.example.invalid"}'):
            path.write_text(text, encoding="utf-8")
            with self.subTest(text=text), self.assertRaises(ValueError):
                evidence.manifest_body(self.root, "B", self.rows)
        with mock.patch.object(evidence, "inventory", return_value=self.rows), contextlib.redirect_stdout(io.StringIO()) as output:
            self.assertEqual(evidence.main(["--root", str(self.root), "--capture-wave", "B"]), 1)
            self.assertNotIn("fixture-value", output.getvalue())

    def test_entrypoint_ground_truth_and_sequence_cannot_be_forged(self):
        good = self.entrypoint()
        for change in (lambda v: v.update(model_calls=1),
                       lambda v: v["streams"][1].update(source="UserTurn"),
                       lambda v: v["streams"][1]["session_bound"].update(policy_profile="different"),
                       lambda v: v["streams"][1]["positions"][0].update(seq=2),
                       lambda v: v["streams"][1]["positions"].pop()):
            value = json.loads(json.dumps(good))
            change(value)
            with self.assertRaises(ValueError):
                evidence.entrypoint_report(value)

    def test_controlled_or_unavailable_product_does_not_become_live_pass(self):
        folder = evidence.wave_dir(self.root, "D")
        evidence.write(folder / "score.json", {"verdict": "Pass"})
        for grade in ("Unavailable", "Controlled"):
            evidence.write(folder / "product-golden.json", {"grade": grade, "source_commit": self.source})
            self.assertEqual(evidence.manifest_body(self.root, "D", self.rows)["verdict"], "Unverifiable")

    def test_tick_requires_real_ordered_range_and_exact_model_counts(self):
        good = self.tick()
        evidence.tick_report(good)
        for change in (lambda v: v.update(tick_model_calls=1),
                       lambda v: v.update(endpoint_calls_after=2),
                       lambda v: v["positions"][0].update(seq=2),
                       lambda v: v["positions"].pop(),
                       lambda v: v["stream_heads"].update({"fixture:tick": 99}),
                       lambda v: v["phases"].reverse()):
            value = json.loads(json.dumps(good))
            change(value)
            with self.assertRaises(ValueError):
                evidence.tick_report(value)

    def test_symlink_artifact_is_rejected(self):
        target = self.root / "linked.json"
        target.write_text("{}")
        link = evidence.wave_dir(self.root, "B") / "relevance-comparison.json"
        link.unlink()
        try:
            os.symlink(target, link)
        except OSError:
            self.skipTest("symlink creation unavailable")
        with self.assertRaises(ValueError):
            evidence.manifest_body(self.root, "B", self.rows)


if __name__ == "__main__":
    unittest.main()
