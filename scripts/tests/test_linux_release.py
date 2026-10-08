import importlib.util
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("verify_installers", ROOT / "scripts/verify-installers.py")
verifier = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verifier)
build_spec = importlib.util.spec_from_file_location("linux_builder", ROOT / "scripts/build-linux-release.py")
builder = importlib.util.module_from_spec(build_spec)
build_spec.loader.exec_module(builder)


class LinuxVersionTests(unittest.TestCase):
    def test_native_prerelease_and_stable_metadata(self):
        self.assertEqual(builder.package_versions("0.1.0-beta.2"), ("0.1.0~beta.2", "0.1.0", "0.beta.2"))
        self.assertEqual(builder.package_versions("0.1.0"), ("0.1.0", "0.1.0", "1"))
        with self.assertRaises(ValueError):
            builder.package_versions("0.1.0+untracked")

    def test_deb_metadata_preserves_other_fields_and_rejects_wrong_input(self):
        with tempfile.TemporaryDirectory() as temporary:
            control = Path(temporary) / "control"
            original = "Package: fixture\nVersion: 0.1.0-beta.2\nDepends: pipewire\n"
            control.write_text(original)
            with self.assertRaisesRegex(ValueError, "Unexpected original"):
                builder.deb_control_version(control, "0.2.0", "0.2.0")
            self.assertEqual(control.read_text(), original)
            builder.deb_control_version(control, "0.1.0-beta.2", "0.1.0~beta.2")
            self.assertEqual(control.read_text(), original.replace("0.1.0-beta.2", "0.1.0~beta.2"))

    def test_rpm_metadata_override_only_applies_to_bundle_after_compilation(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            tauri = root / "apps/desktop/src-tauri"
            tauri.mkdir(parents=True)
            (tauri / "tauri.conf.json").write_text('{"version":"0.1.0-beta.2"}')
            (root / "apps/desktop" / builder.CONFIG).write_text(json.dumps({
                "bundle": {"linux": {"rpm": {"depends": ["pipewire-utils"]}}}}))
            packages = root / "target/release/bundle/rpm"
            packages.mkdir(parents=True)
            (packages / "fixture.rpm").write_bytes(b"fixture")
            with patch.object(builder, "ROOT", root), patch.object(builder, "run") as run, \
                    patch.dict(builder.os.environ, {"TAURI_SIGNING_PRIVATE_KEY": "fixture"}):
                builder.build("rpm", "0.1.0-beta.2")
            self.assertEqual(run.call_count, 4)
            compilation, bundling, signing, verification = [call.args for call in run.call_args_list]
            self.assertIn("--no-bundle", compilation)
            self.assertEqual(compilation[compilation.index("--config") + 1], builder.CONFIG)
            self.assertIn("bundle", bundling)
            config = json.loads(bundling[bundling.index("--config") + 1])
            self.assertEqual(config["version"], "0.1.0")
            self.assertEqual(config["bundle"]["linux"]["rpm"], {"release": "0.beta.2", "depends": ["pipewire-utils"]})
            self.assertEqual(json.loads((tauri / "tauri.conf.json").read_text())["version"], "0.1.0-beta.2")
            self.assertEqual(signing[-3:], ("signer", "sign", packages / "fixture.rpm"))
            self.assertIn("verify_updater", verification)


class LinuxPayloadTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        base = Path(temporary.name)
        self.root, self.payload, self.notices = base / "repo", base / "payload", base / "notices"
        self.resources = self.payload / "usr/lib/OpenKrisp"
        files = ["LICENSE", "NOTICE", "models/LICENSE", "models/LICENSE-MIT", "models/LICENSE-APACHE",
                 "models/manifest.json", "models/DeepFilterNet3_ll_onnx.tar.gz",
                 "third_party/aec/notices/LICENSE"]
        for name in files:
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(f"fixture {name}")
            destination = self.resources / name.replace("third_party/", "third-party/")
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(path, destination)
        (self.notices / "cargo/fixture/LICENSES").mkdir(parents=True)
        (self.notices / "cargo/fixture/LICENSES/MIT.txt").write_text("dependency license")
        (self.notices / "inventory.json").write_text("[]")
        shutil.copytree(self.notices, self.resources / "third-party/dependencies")
        self.executable = self.payload / "usr/bin/openkrisp-desktop"
        self.executable.parent.mkdir(parents=True)
        self.executable.write_bytes(b"\x7fELF\x02\x01" + bytes(12) + b"\x3e\x00")
        root_patch = patch.object(verifier, "ROOT", self.root)
        root_patch.start()
        self.addCleanup(root_patch.stop)

    def test_complete_x86_64_payload_passes(self):
        verifier.verify_linux_payload(self.payload, self.notices)

    def test_missing_runtime_dependencies_are_rejected(self):
        for kind in ("deb", "rpm"):
            with self.subTest(kind=kind), patch.object(verifier, "output", return_value=""):
                with self.assertRaisesRegex(ValueError, "missing runtime dependencies"):
                    verifier.verify_linux(Path(f"fixture.{kind}"))

    def test_wrong_architecture_is_rejected(self):
        self.executable.write_bytes(b"\x7fELF\x02\x01" + bytes(12) + b"\xb7\x00")
        with self.assertRaisesRegex(ValueError, "x86-64"):
            verifier.verify_linux_payload(self.payload, self.notices)

    def test_changed_model_is_rejected(self):
        (self.resources / "models/DeepFilterNet3_ll_onnx.tar.gz").write_bytes(b"different model")
        with self.assertRaisesRegex(ValueError, "Mismatched resource"):
            verifier.verify_linux_payload(self.payload, self.notices)

    def test_missing_nested_license_is_rejected(self):
        (self.resources / "third-party/dependencies/cargo/fixture/LICENSES/MIT.txt").unlink()
        with self.assertRaisesRegex(ValueError, "notices"):
            verifier.verify_linux_payload(self.payload, self.notices)


class OptimizedValidationTests(unittest.TestCase):
    def test_payload_validation_survives_python_optimization(self):
        for optimization in ("-O", "-OO"):
            with self.subTest(optimization=optimization):
                result = subprocess.run([sys.executable, optimization, __file__, "LinuxPayloadTests"],
                                        capture_output=True, text=True)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)


if __name__ == "__main__":
    unittest.main()
