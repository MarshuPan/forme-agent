"""Run real nested gate scripts with inert tools; never claim product evidence."""

import json
import shutil
import subprocess
import unittest
from collections import Counter
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
FIXTURE = ROOT / "tools/tests/fixtures/verification_gate_spy.ps1"


def shared_calls(calls):
    result = []
    for call in calls:
        program, args = call["program"], call["arguments"]
        if program == "cargo" and ("--workspace" in args or args[0] == "fmt"):
            result.append("cargo:" + args[0])
        elif program == "py" and "discover" in args:
            result.append("python:discover")
        elif program == "bash":
            result.append("compliance")
    return Counter(result)


def targeted_calls(calls):
    return Counter(
        tuple(call["arguments"])
        for call in calls
        if call["program"] == "cargo"
        and call["arguments"][0] == "test"
        and "--workspace" not in call["arguments"]
    )


class VerificationGateOwnershipTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.shells = list(dict.fromkeys(
            str(Path(found).resolve())
            for name in ("pwsh", "powershell")
            if (found := shutil.which(name))
        ))
        if not cls.shells:
            raise unittest.SkipTest("PowerShell is unavailable for executable gate orchestration tests")

    def run_gate(self, shell, gate, *arguments, success=True):
        result = subprocess.run(
            [shell, "-NoProfile", "-File", str(FIXTURE),
             "-RepositoryRoot", str(ROOT), "-Gate", gate, *arguments],
            cwd=ROOT, capture_output=True, text=True, encoding="utf-8", timeout=90,
        )
        self.assertEqual(result.returncode == 0, success, (result.stdout + result.stderr)[-6000:])
        records = [line.removeprefix("FORME_GATE_SPY=")
                   for line in result.stdout.splitlines() if line.startswith("FORME_GATE_SPY=")]
        self.assertEqual(len(records), 1, result.stderr[-2000:])
        return json.loads(records[0]), result.stdout

    def test_independent_historical_gates_keep_complete_checks(self):
        for shell in self.shells:
            for gate in ("m0", "m1", "m2", "m3"):
                with self.subTest(shell=shell, gate=gate):
                    calls, output = self.run_gate(shell, gate)
                    shared = shared_calls(calls)
                    for name in ("cargo:fmt", "cargo:check", "cargo:test",
                                 "python:discover", "compliance"):
                        self.assertEqual(shared[name], 1, (name, shared))
                    self.assertEqual(shared["cargo:clippy"], int(gate in ("m2", "m3")))
                    marker = "M0 ACCEPTANCE: PASS" if gate == "m0" else f"{gate.upper()} FINAL ACCEPTANCE: PASS"
                    self.assertIn(marker, output)

    def test_deferred_children_keep_targeted_work_but_cannot_claim_acceptance(self):
        for shell in self.shells:
            for gate in ("m0", "m1", "m2", "m3"):
                with self.subTest(shell=shell, gate=gate):
                    extra = ("-RegressionOnly",) if gate == "m3" else ()
                    original, _ = self.run_gate(shell, gate, *extra)
                    deferred, output = self.run_gate(shell, gate, *extra, "-DeferSharedChecks")
                    self.assertEqual(shared_calls(deferred), Counter())
                    self.assertEqual(targeted_calls(deferred), targeted_calls(original))
                    self.assertGreaterEqual(sum(targeted_calls(deferred).values()), 21)
                    self.assertIn("deferred to parent", output)
                    self.assertNotIn("FINAL ACCEPTANCE: PASS", output)
                    self.assertNotIn("M0 ACCEPTANCE: PASS", output)
                    self.assertNotIn("M1 SCENARIO GATE: PASS", output)
                    self.assertNotIn("M3 PERMANENT REGRESSION: PASS", output)

    def test_m3_rejects_defer_without_regression_before_running_commands(self):
        for shell in self.shells:
            with self.subTest(shell=shell):
                calls, output = self.run_gate(shell, "m3", "-DeferSharedChecks", success=False)
                self.assertEqual(calls, [])
                self.assertIn("requires -RegressionOnly", output)

    def test_m5_owns_shared_checks_once_and_keeps_historical_scenarios(self):
        for shell in self.shells:
            with self.subTest(shell=shell):
                historical, _ = self.run_gate(shell, "m3", "-RegressionOnly")
                calls, output = self.run_gate(shell, "m5")
                self.assertEqual(shared_calls(calls), Counter({
                    "cargo:fmt": 1, "cargo:check": 1, "cargo:clippy": 1,
                    "cargo:test": 1, "python:discover": 1, "compliance": 1,
                }))
                self.assertFalse(targeted_calls(historical) - targeted_calls(calls))
                child = next(c for c in calls if c["program"] == "powershell"
                             and any(a.endswith("verify-m3.ps1") for a in c["arguments"]))
                self.assertIn("-RegressionOnly", child["arguments"])
                self.assertIn("-DeferSharedChecks", child["arguments"])
                self.assertIn("M5 FINAL ACCEPTANCE: PASS", output)

    def test_shared_golden_and_artifact_failures_prevent_m5_final_pass(self):
        for shell in self.shells:
            for step in ("cargo:fmt", "cargo:check", "cargo:clippy", "cargo:test",
                         "python:discover", "compliance", "browser-golden", "artifact-verifier"):
                with self.subTest(shell=shell, step=step):
                    _, output = self.run_gate(shell, "m5", "-FailureStep", step, success=False)
                    self.assertNotIn("M5 FINAL ACCEPTANCE: PASS", output)

    def test_m5_has_no_defer_switch(self):
        for shell in self.shells:
            with self.subTest(shell=shell):
                _, output = self.run_gate(shell, "m5", "-DeferSharedChecks", success=False)
                self.assertNotIn("M5 FINAL ACCEPTANCE: PASS", output)


if __name__ == "__main__":
    unittest.main()
