import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("release_notices", ROOT / "scripts/prepare-release-notices.py")
notices = importlib.util.module_from_spec(spec)
spec.loader.exec_module(notices)


class ReleaseNoticesTests(unittest.TestCase):
    def test_license_fallback_rejects_missing_upstream_declarations(self):
        cases = [("realfft", "3.5.0", "MIT", "README.md"),
                 ("selectors", "0.36.1", "MPL-2.0", "nth_index_cache.rs")]
        for name, version, license_name, declaration in cases:
            with self.subTest(package=name), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                (root / declaration).write_text("missing license declaration")
                package = {"id": name, "name": name, "version": version, "source": "registry",
                           "manifest_path": str(root / "Cargo.toml"), "license": license_name}
                metadata = {"packages": [package, {"id": "app", "name": "openkrisp-desktop", "source": None}],
                            "resolve": {"nodes": [
                                {"id": "app", "deps": [{"pkg": name, "dep_kinds": [{"kind": None}]}]},
                                {"id": name, "deps": []}]}}
                args = SimpleNamespace(output=root / "collected", target="x86_64-unknown-linux-gnu", fetch_missing=False)
                with patch.object(notices, "json_command", return_value=metadata):
                    with self.assertRaisesRegex(ValueError, "missing its expected .* license declaration"):
                        notices.collect(args)
                self.assertFalse(args.output.exists())

    def test_pinned_upstream_license_directory_is_collected_and_cached(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            revision = "a" * 40
            crate = root / "crate"
            crate.mkdir()
            (crate / ".cargo_vcs_info.json").write_text(json.dumps({"git": {"sha1": revision}}))
            package = {"name": "fixture", "repository": "https://github.com/owner/project"}
            responses = [
                json.dumps({"tree": [{"type": "tree", "path": "LICENSES", "sha": "b" * 40}]}).encode(),
                json.dumps({"tree": [{"type": "blob", "path": "MIT.txt"}], "truncated": False}).encode(),
                b"pinned license",
            ]
            with patch.object(notices, "ROOT", root), patch.object(notices, "fetch", side_effect=responses) as fetch:
                cache, files, sources = notices.upstream_notices(package, crate, True)
                self.assertEqual(files, [cache / "LICENSES/MIT.txt"])
                self.assertEqual(files[0].read_bytes(), b"pinned license")
                self.assertIn(f"/{revision}/LICENSES/MIT.txt", sources[0]["url"])
                self.assertEqual(notices.upstream_notices(package, crate, False), (cache, files, sources))
                self.assertEqual(fetch.call_count, 3)

    def test_libappindicator_license_fallback_rejects_a_different_revision(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            packages = []
            for name, revision in [("libappindicator", "a" * 40), ("libappindicator-sys", "b" * 40)]:
                base = root / name
                base.mkdir()
                (base / ".cargo_vcs_info.json").write_text(json.dumps({"git": {"sha1": revision}, "path_in_vcs": ""}))
                packages.append({"id": name, "name": name, "version": "0.9.0", "source": "registry",
                                 "manifest_path": str(base / "Cargo.toml"), "license": "MIT", "repository": None,
                                 "authors": []})
            (root / "libappindicator/LICENSE").write_text("fixture")
            packages.append({"id": "app", "name": "openkrisp-desktop", "source": None})
            metadata = {"packages": packages, "resolve": {"nodes": [
                {"id": "app", "deps": [{"pkg": name, "dep_kinds": [{"kind": None}]} for name in
                                        ["libappindicator", "libappindicator-sys"]]},
                {"id": "libappindicator", "deps": []}, {"id": "libappindicator-sys", "deps": []}]}}
            destination = root / "collected"
            args = SimpleNamespace(output=destination, target="x86_64-unknown-linux-gnu", fetch_missing=False)
            with patch.object(notices, "json_command", return_value=metadata):
                with self.assertRaisesRegex(ValueError, "provenance does not match"):
                    notices.collect(args)
            self.assertFalse(destination.exists())

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
