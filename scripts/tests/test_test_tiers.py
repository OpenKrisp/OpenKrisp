import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("tiers", Path(__file__).parents[1] / "run-extended-tests.py")
tiers = importlib.util.module_from_spec(spec)
spec.loader.exec_module(tiers)

class PerformanceGateTests(unittest.TestCase):
    def setUp(self):
        self.profile = {"minFrames":1000,"minAecFrames":1000,"maxP99Us":8000,"maxMissRate":0.01,"burners":0}
        self.report = {"frames":1000,"aecFrames":1050,"p99Us":4000,"missed":1,"modelErrors":0,"paced":True,"burners":0,"degraded":False,"skippedInferences":0}

    def test_rejects_slow_unpaced_fallback_and_empty_measurements(self):
        tiers.validate_performance(self.report, self.profile)
        for field, value in [("p99Us",8001),("missed",11),("modelErrors",1),("frames",0),("aecFrames",0),("paced",False),("burners",1),("p99Us",None),("skippedInferences",1),("degraded",True)]:
            with self.subTest(field=field), self.assertRaises(ValueError):
                tiers.validate_performance(self.report | {field:value}, self.profile)

    def test_renamed_filters_cannot_pass_with_zero_tests(self):
        with tempfile.TemporaryDirectory() as temporary, patch.object(tiers.subprocess, "check_output", return_value="0 tests"), patch.object(tiers.subprocess, "run") as run:
            with self.assertRaisesRegex(ValueError, "missing test inventory"):
                tiers.run_test(Path(temporary), "fixture", [], ["renamed"])
            run.assert_not_called()

    def test_corpus_checksums_and_pcm_geometry(self):
        import hashlib, json
        root = Path(__file__).parents[2] / "tests/fixtures/speech"
        manifest = json.loads((root / "manifest.json").read_text())
        self.assertEqual(manifest["license"], "CC-BY-4.0")
        self.assertEqual(len(manifest["clips"]), 2)
        for clip in manifest["clips"]:
            data = (root / clip["file"]).read_bytes()
            self.assertEqual(hashlib.sha256(data).hexdigest(), clip["pcmSha256"])
            self.assertEqual(len(data), clip["samples"] * 2)
            self.assertEqual(clip["sampleRateHz"], 16000)
            self.assertEqual(clip["channels"], 1)
