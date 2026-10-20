# OpenKrisp for Linux

The community workflow produces x86-64 packages: a DEB built on Debian 12, with
installation checks on Debian 12 and Ubuntu 24.04, and an RPM built and installation
checked on Fedora 44. Other distributions and Linux ARM are not qualified. Check
the release's `VALIDATION.md` for the results for your version. Container checks
cover package installation and linked libraries; desktop audio and conferencing
still need real-machine testing.

## Download and install

Download the matching `.deb` or `.rpm` from the OpenKrisp repository's release page.
Published releases are publicly downloadable. Download
`SHA256SUMS` from the same release and compare the package's `sha256sum` output
with its entry. Packages are unsigned; a checksum detects corruption but is not
an independent proof of origin.

Install with the package manager so it also installs the GUI and PipeWire runtime
dependencies. Replace `<version>` with the downloaded version:

```sh
# Debian 12 or Ubuntu 24.04
sudo apt install './OpenKrisp-<version>-linux-amd64.deb'

# Fedora 44
sudo dnf install './OpenKrisp-<version>-linux-x86_64.rpm'
```

OpenKrisp needs a graphical desktop with an active PipeWire session and WirePlumber.
Installing dependencies alone does not switch an existing PulseAudio-only session
to PipeWire. Use your distribution's instructions to enable PipeWire, then log out
and back in. OpenKrisp does not run the audio session as root.

## Start audio

1. Open OpenKrisp from the application launcher, or run `openkrisp-desktop`.
2. Choose your physical microphone and headphones in OpenKrisp and start the routes.
3. In your call app, choose **OpenKrisp Microphone** as input and **OpenKrisp Speaker**
   as output. The PipeWire endpoints appear when their routes start and disappear
   when they stop. Start the routes before selecting them in your call app.

If endpoints do not appear, check your user session with
`systemctl --user status pipewire pipewire-pulse wireplumber` and confirm that
`pw-loopback --version` works. The runtime needs PipeWire 0.3.53 or newer. Restart
the call app if it cached the device list. Report persistent errors with the app's
diagnostics and your distribution/version.

## Update or remove

Updater-enabled versions automatically download new releases from the public
release feed. When both routes are off, a 30-second countdown installs and restarts
automatically; select **Later** to postpone. Approve the OS
administrator prompt if shown. Older versions need one manual upgrade. If a new
release needs additional system dependencies, stop the routes, quit OpenKrisp,
and install its package with the same APT/DNF command to resolve dependencies.

Remove OpenKrisp using your distribution's package manager. You can obtain the
installed package name from the original download with
`dpkg-deb --field './OpenKrisp-<version>-linux-amd64.deb' Package` or
`rpm -qp --queryformat '%{NAME}\n' './OpenKrisp-<version>-linux-x86_64.rpm'`, then
pass that name to `sudo apt remove` or `sudo dnf remove`. Stop routes before removal.
Shared PipeWire components may be used by the rest of your desktop.

Project terms, dependency notices, and model notices are installed under
`/usr/lib/OpenKrisp`. Each dependency retains its own license.
