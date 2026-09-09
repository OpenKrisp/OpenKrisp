import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("installer", ROOT / "scripts/build-macos-installer.py")
installer = importlib.util.module_from_spec(spec)
spec.loader.exec_module(installer)


class InstallerTests(unittest.TestCase):
    def test_corrupted_cached_dependency_is_rejected_without_running_it(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "driver.pkg"
            path.write_bytes(b"not the pinned installer")
            with self.assertRaisesRegex(ValueError, "SHA-256 mismatch"):
                installer.verified_download({"sha256": "0" * 64}, path)

    def test_installer_repairs_missing_driver_and_preserves_newer_versions(self):
        document = installer.distribution("0.1.0", "0.7.1", "11.0", ["arm64"])
        root = document.getroot()
        choices = {choice.get("id"): choice for choice in root.findall("choice")}
        self.assertEqual(choices[installer.APP_ID].get("selected"), "true")
        self.assertEqual(choices[installer.BLACKHOLE_ID].get("selected"), "needsBlackHole()")
        restart = root.find(f"pkg-ref[@id='{installer.BLACKHOLE_ID}']")
        self.assertEqual(restart.get("onConclusion"), "RequireRestart")
        # Execute the actual Installer JavaScript against missing, old and newer bundles.
        # Apple's compareVersions is supplied by Installer; mimic numeric dotted versions here.
        javascript = """
const assert = require('node:assert/strict');
let info;
const system = { files: { plistAtPath: () => info }, compareVersions: (a, b) => {
  const aa = a.split('.').map(Number), bb = b.split('.').map(Number);
  for (let i = 0; i < Math.max(aa.length, bb.length); i++) {
    const delta = (aa[i] || 0) - (bb[i] || 0); if (delta) return Math.sign(delta);
  } return 0;
}};
""" + root.find("script").text + """
for (const [version, expected] of [[null, true], ['0.6.1', true], ['0.7.1', false], ['0.10.0', false]]) {
  info = version && { CFBundleIdentifier: 'audio.existential.BlackHole2ch', CFBundleShortVersionString: version };
  assert.equal(needsBlackHole(), expected);
}
info = { CFBundleIdentifier: 'wrong.driver', CFBundleShortVersionString: '9.0.0' };
assert.equal(needsBlackHole(), true);
"""
        subprocess.run(["node", "-e", javascript], check=True)

    def test_default_linux_bundles_can_resolve_system_dependencies(self):
        config = ROOT / "apps/desktop/src-tauri/tauri.linux.conf.json"
        self.assertEqual(json.loads(config.read_text())["bundle"]["targets"], ["deb", "rpm"])


if __name__ == "__main__":
    unittest.main()
