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
        self.assertEqual(len(choices), 3)
        for variant in installer.BLACKHOLE_VARIANTS:
            identifier = f"audio.existential.BlackHole{variant}"
            self.assertEqual(choices[identifier].get("selected"), f"needsBlackHole('{variant}')")
            restart = root.find(f"pkg-ref[@id='{identifier}']")
            self.assertEqual(restart.get("onConclusion"), "RequireRestart")
        # Execute the actual Installer JavaScript against missing, old and newer bundles.
        # Apple's compareVersions is supplied by Installer; mimic numeric dotted versions here.
        javascript = """
const assert = require('node:assert/strict');
const installed = {};
const system = { files: { plistAtPath: (path) => installed[path] }, compareVersions: (a, b) => {
  const aa = a.split('.').map(Number), bb = b.split('.').map(Number);
  for (let i = 0; i < Math.max(aa.length, bb.length); i++) {
    const delta = (aa[i] || 0) - (bb[i] || 0); if (delta) return Math.sign(delta);
  } return 0;
}};
""" + root.find("script").text + """
const variants = ['2ch', '16ch'];
const path = (v) => '/Library/Audio/Plug-Ins/HAL/BlackHole' + v + '.driver/Contents/Info.plist';
const states = [[null, true], ['0.6.1', true], ['0.7.1', false], ['0.10.0', false]];
// Check every mixed installation: either driver can independently need repair/upgrade.
for (const [first, firstExpected] of states) {
  for (const [second, secondExpected] of states) {
    [first, second].forEach((version, i) => {
      installed[path(variants[i])] = version && {
        CFBundleIdentifier: 'audio.existential.BlackHole' + variants[i],
        CFBundleShortVersionString: version,
      };
    });
    assert.equal(needsBlackHole('2ch'), firstExpected);
    assert.equal(needsBlackHole('16ch'), secondExpected);
  }
}
for (const variant of variants) {
  installed[path(variant)] = { CFBundleIdentifier: 'wrong.driver', CFBundleShortVersionString: '9.0.0' };
  assert.equal(needsBlackHole(variant), true);
}

"""
        subprocess.run(["node", "-e", javascript], check=True)

    def test_default_linux_bundles_can_resolve_system_dependencies(self):
        config = ROOT / "apps/desktop/src-tauri/tauri.linux.conf.json"
        self.assertEqual(json.loads(config.read_text())["bundle"]["targets"], ["deb", "rpm"])


if __name__ == "__main__":
    unittest.main()
