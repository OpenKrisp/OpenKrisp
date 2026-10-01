# Install OpenKrisp on an Apple Silicon Mac

Download the `macos-arm64.pkg` asset for the version you selected. The package
contains the app and the OpenKrisp Microphone and OpenKrisp Speaker audio drivers.
It targets M-series Macs with a build minimum of macOS 11.0; the complete macOS
and device matrix has not been qualified. Intel and Linux binaries are not included.

The app and drivers are ad-hoc signed. The installer is unsigned and **not
notarized by Apple**. For a download you trust, System Settings > Privacy &
Security > Open Anyway may allow a per-app exception. Managed Macs may prohibit
it. Follow [Apple's instructions](https://support.apple.com/en-us/102445). If macOS
reports damaged software or malware, stop and report the exact message.

1. Close audio apps, open the installer, and authorize installation.
2. Restart if prompted, then open OpenKrisp from Applications.
3. Allow microphone access. Select your physical microphone and headphones.
4. In the call app, select OpenKrisp Microphone as input and OpenKrisp Speaker as
   output. Start the matching microphone and incoming routes in OpenKrisp.

Headphones are recommended. Speakerphone echo cancellation, device changes,
long-running sessions, and fresh-Mac installation still need broader testing.
Read the version's release notes and validation record for its actual results.

Updates are manual: quit OpenKrisp and audio apps before running a newer installer.
To uninstall, quit these apps, move OpenKrisp from Applications to Trash, and use
Finder's Go > Go to Folder to open `/Library/Audio/Plug-Ins/HAL`. Move only
`OpenKrispMicrophone.driver` and `OpenKrispSpeaker.driver` to Trash, authorize the
change, and restart. Leave other audio drivers in place.

`SHA256SUMS` detects changed downloads; it is not an Apple signature. The release
also includes the matching source snapshot and build provenance. Original
OpenKrisp code uses PolyForm Perimeter 1.0.0; third-party terms remain separate.
