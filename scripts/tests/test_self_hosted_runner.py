import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("self_hosted_runner", ROOT / "scripts/self-hosted-runner.py")
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)


class SelfHostedRunnerTests(unittest.TestCase):
    def test_existing_bom_configuration_is_reused_without_new_credentials(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            (directory / ".runner").write_text(json.dumps({
                "agentName": "fixture", "gitHubUrl": "https://github.com/OpenKrisp/OpenKrisp/",
            }), encoding="utf-8-sig")
            with patch.object(runner, "github") as github:
                runner.prepare(directory, "fixture", "openkrisp")
            github.assert_not_called()
            with self.assertRaisesRegex(SystemExit, "another runner/repository"):
                runner.prepare(directory, "another", "openkrisp")

    def test_registration_token_is_not_in_command_arguments(self):
        with patch.object(runner.subprocess, "run") as run:
            runner.configure(Path("/runner"), "fixture", "openkrisp", token="fixture-secret")
        command = run.call_args.args[0]
        self.assertNotIn("fixture-secret", command)
        self.assertNotIn("--token", command)
        self.assertEqual(run.call_args.kwargs["env"]["ACTIONS_RUNNER_INPUT_TOKEN"], "fixture-secret")

    def test_download_rejects_unexpected_origin_before_opening_url(self):
        bootstrap = {"token": "fixture-secret", "package": {
            "os": "linux", "architecture": "arm64", "filename": "runner.tar.gz",
            "download_url": "https://example.invalid/runner.tar.gz", "sha256_checksum": "0" * 64,
        }}
        with tempfile.TemporaryDirectory() as temporary, \
                patch.object(runner, "runner_platform", return_value=("linux", "arm64")), \
                patch.object(runner.urllib.request, "urlopen") as urlopen:
            with self.assertRaisesRegex(SystemExit, "Unexpected runner download origin"):
                runner.prepare(Path(temporary), "fixture", "openkrisp", bootstrap)
            urlopen.assert_not_called()


if __name__ == "__main__":
    unittest.main()
