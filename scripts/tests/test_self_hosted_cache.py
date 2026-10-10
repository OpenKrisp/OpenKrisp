import os
from pathlib import Path
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / "prepare-self-hosted-cache.sh"


class SelfHostedCacheTests(unittest.TestCase):
    def run_helper(self, root):
        environment = dict(os.environ, RUNNER_TOOL_CACHE=str(root / "cache"),
                           RUNNER_OS="fixture", RUNNER_ARCH="fixture", GITHUB_ENV=str(root / "env"))
        return subprocess.run(["bash", str(SCRIPT), "--target"], cwd=root / "workspace",
                              env=environment, text=True, capture_output=True)

    def test_checkout_link_removal_preserves_only_the_runner_owned_cache(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "workspace").mkdir()
            self.assertEqual(self.run_helper(root).returncode, 0)
            target = root / "workspace/target"
            self.assertTrue(target.is_symlink())
            self.assertTrue(target.resolve().is_relative_to((root / "cache").resolve()))
            (target / "evidence").write_text("persisted")
            target.unlink()
            self.assertEqual(self.run_helper(root).returncode, 0)
            self.assertEqual((target / "evidence").read_text(), "persisted")
            environment = (root / "env").read_text()
            self.assertIn("NPM_CONFIG_USERCONFIG=/dev/null", environment)
            self.assertIn(f"npm_config_cache={root}/cache/", environment)
            self.assertIn(f"CARGO_HOME={root}/cache/", environment)

    def test_existing_target_is_never_deleted_or_replaced(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            target = root / "workspace/target"
            target.mkdir(parents=True)
            (target / "keep").write_text("existing")
            self.assertNotEqual(self.run_helper(root).returncode, 0)
            self.assertFalse(target.is_symlink())
            self.assertEqual((target / "keep").read_text(), "existing")


if __name__ == "__main__":
    unittest.main()
