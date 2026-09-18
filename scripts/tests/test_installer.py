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
        document = installer.distribution("0.1.0", "1.0.0", "11.0", ["arm64"])
        root = document.getroot()
        choices = {choice.get("id"): choice for choice in root.findall("choice")}
        self.assertEqual(choices[installer.APP_ID].get("selected"), "true")
        self.assertEqual(len(choices), 3)
        for driver in installer.DRIVERS:
            identifier = driver["identifier"]
            self.assertEqual(choices[identifier].get("selected"), installer.driver_choice(driver))
            restart = root.find(f"pkg-ref[@id='{identifier}']")
            self.assertEqual(restart.get("onConclusion"), "RequireRestart")
        # Execute the actual Installer JavaScript against missing, old and newer bundles.
        # Apple's compareVersions is supplied by Installer; mimic numeric dotted versions here.
        javascript = """
const assert = require('node:assert/strict');
const installed = {};
const files = new Set();
const system = { files: { plistAtPath: (path) => installed[path], fileExistsAtPath: (path) => files.has(path) }, compareVersions: (a, b) => {
  const aa = a.split('.').map(Number), bb = b.split('.').map(Number);
  for (let i = 0; i < Math.max(aa.length, bb.length); i++) {
    const delta = (aa[i] || 0) - (bb[i] || 0); if (delta) return Math.sign(delta);
  } return 0;
}};
""" + root.find("script").text + """
const variants = ['OpenKrispMicrophone', 'OpenKrispSpeaker'];
const identifiers = ['app.openkrisp.audio.microphone', 'app.openkrisp.audio.speaker'];
const path = (v) => '/Library/Audio/Plug-Ins/HAL/' + v + '.driver/Contents/Info.plist';
const binary = (v) => '/Library/Audio/Plug-Ins/HAL/' + v + '.driver/Contents/MacOS/' + v;
const states = [[null, true], ['0.9.0', true], ['1.0.0', false], ['1.10.0', false]];
// Shared BlackHole installations never satisfy or suppress the new device installation.
installed['/Library/Audio/Plug-Ins/HAL/BlackHole2ch.driver/Contents/Info.plist'] = {
  CFBundleIdentifier: 'audio.existential.BlackHole2ch', CFBundleShortVersionString: '9.0.0'
};
for (const [first, firstExpected] of states) {
  for (const [second, secondExpected] of states) {
    [first, second].forEach((version, i) => {
      installed[path(variants[i])] = version && {
        CFBundleIdentifier: identifiers[i], CFBundleShortVersionString: version,
        CFBundleExecutable: variants[i],
      };
      files.add(binary(variants[i]));
    });
    assert.equal(needsDriver(variants[0], identifiers[0]), firstExpected);
    assert.equal(needsDriver(variants[1], identifiers[1]), secondExpected);
  }
}
for (let i = 0; i < variants.length; i++) {
  installed[path(variants[i])].CFBundleIdentifier = 'wrong.driver';
  assert.equal(needsDriver(variants[i], identifiers[i]), true);
  installed[path(variants[i])].CFBundleIdentifier = identifiers[i];
  files.delete(binary(variants[i]));
  assert.equal(needsDriver(variants[i], identifiers[i]), true);
}

"""
        subprocess.run(["node", "-e", javascript], check=True)

    def test_default_linux_bundles_can_resolve_system_dependencies(self):
        config = ROOT / "apps/desktop/src-tauri/tauri.linux.conf.json"
        self.assertEqual(json.loads(config.read_text())["bundle"]["targets"], ["deb", "rpm"])


if __name__ == "__main__":
    unittest.main()
