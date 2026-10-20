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

Updater-enabled versions automatically download new application releases when
the repository is public. When both routes are off, a 30-second countdown installs
and restarts automatically; select **Later** to postpone. macOS may request administrator authentication. Older versions need one manual
upgrade. Audio driver or uninstaller changes require the combined `.pkg`: quit
OpenKrisp and audio apps before running it, and restart if prompted.

To uninstall, choose **OpenKrisp → Uninstall OpenKrisp…**, or open
**Uninstall OpenKrisp** in Applications if you have already trashed OpenKrisp.
The installer includes this app; no additional download is needed. Close calling
and audio apps, confirm removal, and authorize it with your Mac's administrator
password. The uninstaller quits OpenKrisp and removes the application, its two
audio drivers, and package receipts. Restart afterward to unload the drivers.
Other audio drivers and saved settings are preserved. The uninstaller remains
in Applications for retrying cleanup; you can move it to Trash when finished.

For older installations without the uninstaller, open `/Library/Audio/Plug-Ins/HAL`
and move only `OpenKrispMicrophone.driver` and `OpenKrispSpeaker.driver` to Trash,
authorize the change, and restart.

`SHA256SUMS` detects changed downloads; it is not an Apple signature. The release
also includes the matching source snapshot and build provenance. Original
Current OpenKrisp source uses GNU GPLv3 only; third-party terms remain separate.
Earlier beta installers retain the license and notices bundled with their release.
