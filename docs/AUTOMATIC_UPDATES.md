# Automatic application updates

Release builds use a native background scheduler to check for updates on launch
and every six hours while OpenKrisp is running, including with its window hidden. Focus also triggers a check when six
hours have elapsed. A check downloads the newest eligible release in the background
and verifies its signature. **Install and restart** appears when it is ready.
Once both audio routes are off, a 30-second countdown installs and restarts
automatically. **Later** postpones installation for this session, and **Install
and restart** installs immediately. Starting audio cancels the countdown; a new
countdown starts once audio is off again. Installation may request OS administrator
authentication; users do not need to find or download the package themselves.
**Help → Check for updates** checks immediately. Offline checks retry automatically,
and cancelled installation keeps the verified download available for retry. An
installation error pauses automatic installation for this session so administrator
prompts do not repeat; use **Install and restart** to retry.

The feed is this repository's public GitHub releases. It will work after the
repository becomes public; private releases require GitHub authentication and
are not anonymously downloadable. No repository credentials are shipped in the
app. Older builds without this updater need one manual installation of a release
that includes it. Debug builds and builds without a public key disable updates.

## Release signing setup

Before preparing the first updater-enabled release, generate one persistent
Tauri signing key outside the repository and back it up securely:

```sh
npm run --prefix apps/desktop tauri -- signer generate -w /secure/location/openkrisp-updater.key
gh secret set TAURI_SIGNING_PRIVATE_KEY --repo saeta-eth/OpenKrisp < /secure/location/openkrisp-updater.key
gh variable set OPENKRISP_UPDATER_PUBLIC_KEY --repo saeta-eth/OpenKrisp --body "$(cat /secure/location/openkrisp-updater.key.pub)"
gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD --repo saeta-eth/OpenKrisp
```

The last command stores the password chosen during generation. For an unencrypted
key, omit the password secret. Do not commit the private key or change the public
key for already installed users without planning a signed key migration.
This update signature is separate from Apple signing/notarization and native
package signing. Existing community distribution limitations still apply.

Release jobs embed `OPENKRISP_UPDATER_PUBLIC_KEY` into the Rust app at compile time
and pass the same key to the Tauri bundler through its updater configuration.
They require the key variable and private key secret, and verify each resulting
signature against that public key before uploading artifacts. This prevents a
misconfigured key pair from publishing updates existing apps cannot verify.
Ordinary CI builds need no signing secrets and make no update requests.

## Release artifacts and channels

Each new release includes `latest.json`, the macOS `.app.tar.gz` update archive,
and signatures for that archive and the final DEB/RPM packages. The manifest maps
`darwin-aarch64`, `linux-x86_64-deb`, and `linux-x86_64-rpm` to the versioned files
and contains the signature contents. DEB signatures are generated **after** native
version rewriting. All files participate in checksums, provenance, retained
artifact comparison, and draft/publication guards. Schema 3 provenance identifies
releases containing updater assets; historical schema 2 releases remain verifiable.

The app reads the most recent 100 published GitHub releases and chooses the
highest newer SemVer with a manifest. Drafts, releases without a manifest, and
older/equal versions are excluded. A stable installation accepts stable releases
only. A beta accepts newer beta or stable versions; after moving to stable it
follows the stable channel. This does not depend on GitHub's **Latest** flag,
which excludes prereleases and is disabled by our publication workflow.

## Architecture and installation

[The native updater](../apps/desktop/src-tauri/src/updates.rs) uses the pinned
Tauri updater plugin. HTTPS and signature verification protect downloads. The
WebView can query status, request preparation, or install the managed download;
it cannot supply URLs, paths, signatures, or installer commands. Direct updater
plugin IPC permissions are not granted.

Download and network work run asynchronously in the desktop control plane.
Only one update is prepared at a time. Verified bytes remain in native memory
until installation or process exit. Reloading the UI recovers native status;
quitting before installation causes a new check/download next time.
Progress snapshots have monotonic revisions so stale IPC responses cannot hide
a ready update. No update code runs in CPAL callbacks or DSP workers.

Installation runs on a blocking worker. A native gate serializes it with route
requests and shutdown, including accepted starts not yet visible in the engine
snapshot. Both routes must be stopped; failed routes must be explicitly released.
Quit is deferred during installation. On success the engine shuts down, joins
workers, flushes accepted settings writes, and restarts through Tauri. Settings
are outside the application bundle and survive the update.

macOS replaces only `OpenKrisp.app`; installed audio drivers and the separate
uninstaller are preserved. Changes to those components still require the combined
`.pkg`. Linux uses Tauri's native DEB/RPM installation and OS privilege prompts.
Packages declare `pkexec` (DEB) or `polkit` (RPM) for graphical authorization.
Those installers use `dpkg`/`rpm`, so releases introducing new system dependencies
need the full APT/DNF installation procedure if dependencies are unavailable.

## Verification and acceptance

Automated tests cover channel selection, native command permissions, queued/active
route exclusion, quit/install serialization, scheduled checks, stale progress,
cleanup, installation controls, manifest consistency, missing/malformed signatures,
and publication integrity. `verify_updater` verifies real signed artifact bytes
against the configured key in each release job.

Local verification passed: 24 desktop Rust tests, 95 frontend tests, 75 Python
tests, frontend production build, warnings-denied desktop Clippy, Rust formatting,
dependency license checks, actionlint, and `git diff --check`. A debug macOS app
was bundled with the updater configuration and its archive signature verified.
Temporary test keys also demonstrated rejection of tampered bytes and wrong keys;
they were discarded. No update was installed into the system or published.

Before launch, test two signed release builds on a Mac, Debian/Ubuntu desktop,
and Fedora desktop: background download, hidden-window check, both routes active,
stop/install/restart, preserved settings, administrator cancellation/retry,
offline recovery, wrong-key/tampered downloads, and beta-to-stable migration.
Also verify root-owned `/Applications/OpenKrisp.app` replacement and OS trust prompts.
Local software tests do not establish these OS installation results.
