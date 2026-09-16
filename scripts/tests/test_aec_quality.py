import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location(
    "aec_quality", Path(__file__).resolve().parents[1] / "check-aec-quality.py")
QUALITY = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(QUALITY)


class AecQualityGate(unittest.TestCase):
    def test_only_complete_execution_can_pass(self):
        tests = {"a", "b"}
        passed = ("test result: ok. 2 passed; 0 failed; 0 ignored; "
                  "0 measured; 0 filtered out; finished in 0.01s\n")
        self.assertTrue(QUALITY.verdict(0, passed, tests))
        for code, output, expected in [
            (1, passed, tests),  # A crash after an apparently successful summary.
            (0, "", tests),
            (0, passed, set()),
            (0, passed.replace("2 passed", "0 passed"), tests),
            (0, passed.replace("0 ignored", "1 ignored"), tests),
            (0, passed.replace("0 filtered out", "1 filtered out"), tests),
            (0, passed, {"a", "b", "new_acceptance_gate"}),
        ]:
            with self.subTest(code=code, output=output, expected=expected):
                self.assertFalse(QUALITY.verdict(code, output, expected))

    def test_inventory_requires_full_names(self):
        self.assertEqual(QUALITY.inventory(
            "gate_extra: test\ngate: benchmark\nmodule::gate: test\n2 tests, 0 benchmarks\n"
        ), {"gate_extra", "module::gate"})

    def test_missing_required_gate_cannot_turn_into_zero_test_success(self):
        listing = subprocess.CompletedProcess([], 0, "renamed_gate: test\n", "")
        with tempfile.TemporaryDirectory() as tmp, patch.object(
            QUALITY.subprocess, "run", return_value=listing
        ) as run:
            result = QUALITY.run_target("unused", {"required_gate"}, Path(tmp), "suite")
            self.assertEqual(result["status"], "failed")
            self.assertEqual(result["missing"], ["required_gate"])
            self.assertEqual(run.call_count, 1)

    def test_all_discovered_tests_run_including_ignored_and_failure_logs_survive(self):
        listing = subprocess.CompletedProcess([], 0, "required: test\nfuture_gate: test\n", "")
        failure = subprocess.CompletedProcess([], 101, "quality failure\n", "assertion failed\n")
        with tempfile.TemporaryDirectory() as tmp, patch.object(
            QUALITY.subprocess, "run", side_effect=[listing, failure]
        ) as run:
            result = QUALITY.run_target("unused", {"required"}, Path(tmp), "suite")
            self.assertEqual(result["status"], "failed")
            self.assertEqual(result["tests"], ["future_gate", "required"])
            self.assertIn("--include-ignored", run.call_args.args[0])
            self.assertNotIn("--exact", run.call_args.args[0])
            self.assertEqual((Path(tmp) / "suite.log").read_text(),
                             "quality failure\nassertion failed\n")


if __name__ == "__main__":
    unittest.main()
