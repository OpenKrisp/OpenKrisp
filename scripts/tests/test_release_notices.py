import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("release_notices", ROOT / "scripts/prepare-release-notices.py")
notices = importlib.util.module_from_spec(spec)
spec.loader.exec_module(notices)


class ReleaseNoticesTests(unittest.TestCase):
    def test_tampered_cached_upstream_license_is_rejected_without_network(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            revision = "a" * 40
            cache = root / "target/license-cache" / f"owner-project-{revision}"
            cache.mkdir(parents=True)
            (cache / "LICENSE").write_bytes(b"tampered")
            (cache / "provenance.json").write_text(json.dumps([{
                "file": "LICENSE", "url": "https://example.invalid/LICENSE",
                "sha256": hashlib.sha256(b"original").hexdigest(),
            }]))
            crate = root / "crate"
            crate.mkdir()
            (crate / ".cargo_vcs_info.json").write_text(json.dumps({"git": {"sha1": revision}}))
            package = {"name": "fixture", "repository": "https://github.com/owner/project"}
            with patch.object(notices, "ROOT", root), patch.object(notices, "fetch") as fetch:
                with self.assertRaisesRegex(ValueError, "Corrupt cached notice"):
                    notices.upstream_notices(package, crate, False)
                fetch.assert_not_called()


if __name__ == "__main__":
    unittest.main()
