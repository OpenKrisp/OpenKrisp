# Preparing this repository for a public launch

Decision recorded on 2026-09-25: `saeta-eth/OpenKrisp` remains private for now.
Development, tags, and releases all stay in this repository. The preparation
below does not change repository visibility or invite additional collaborators.

## Licensing and terminology

The selected license is PolyForm Perimeter 1.0.0. With that license, a public
launch makes OpenKrisp **source available**, not open source under the
[Open Source Definition](https://opensource.org/osd). The competing-product
restriction is incompatible with that definition. Public access to GitHub does
not remove the restriction.

If the maintainer later chooses an OSI-approved open-source license, that is a
separate licensing change: open-source licensing must permit commercial use and
redistribution. Until then, keep the current license and describe the project as
source available. See [licensing status](LICENSING.md) for third-party scope and
the recorded written permission for the current BlackHole integration.

## Prepared

- Project license, copyright notice, third-party scope, and BlackHole permission
  status are recorded without publishing private correspondence.
- The README introduces the app and points to this repository's releases.
- [Contributor setup and validation instructions](../CONTRIBUTING.md) are available.
- The README, desktop setup, and installation instructions use the current named
  devices and automatic settings behavior; phase reports remain historical evidence.
- The first beta has an Apple Silicon installer, exact source snapshot, installation
  guide, validation record, provenance, and SHA-256 checksums. Uploaded draft assets
  were verified against local hashes before publication.
- The [release process](COMMUNITY_RELEASE.md) documents signing limitations,
  private download access, and the remaining manual acceptance work.
- The [Actions workflow](RELEASING.md) prepares version merges after successful CI
  and automatically publishes their verified files after successful preparation.
  Manual operations remain available. The earlier local beta is back in
  draft; no Actions release has been run as part of workflow setup.
- The workflow includes macOS ARM64 and Linux x86-64 DEB/RPM packages. It requires
  native builds and Linux package installation checks before assembling a draft.
  The Linux jobs are configured; their first actual run is still pending.

## Remaining before changing visibility

- Review all history and repository content that will become public, including
  issues, pull requests, attachments, releases, Actions logs, and artifacts. A
  limited credential-pattern scan of 706 historical text blobs found no matches
  during beta preparation; it was not a full privacy or secrets audit. GitHub
  explicitly notes that [Actions history and logs become public](https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/managing-repository-settings/setting-repository-visibility).
- Verify contributor setup from a fresh checkout on the supported platforms and
  recheck introductory documentation against the version chosen for public launch.
- Record a passing main CI run and a successful native release build/install run
  for the launch version. Hosted CI resumed on 2026-10-01. Local beta results are
  documented separately and do not substitute for those release checks.
- Provision the persistent updater signing key and public key variable, then test
  signed upgrade/restart on each desktop platform. The anonymous update feed becomes
  accessible after the repository is public; see [automatic updates](AUTOMATIC_UPDATES.md).
- Run the beta on a fresh Mac: browser download, Gatekeeper exception, installation,
  microphone permission, virtual device enumeration, upgrade/restart, and real-call
  listening. Record outcomes and retain explicit limitations for unqualified
  speakerphone, long-run, or platform scenarios.
- Run the Linux packages on Debian/Ubuntu and Fedora desktops: launch the GUI,
  start both PipeWire routes, place a real call, and test update/removal. Container
  package checks do not qualify desktop audio or hardware behavior.
- Choose and document a maintained channel for private vulnerability reports before
  inviting public reports; do not advertise a reporting feature that is not enabled.
- Review the version and release notes being exposed, including the source-available
  wording. The maintainer must explicitly choose when to make this repository public.

## Keep the release history in one place

Private prereleases can be downloaded by people with repository read access.
Repository access, not possession of a release URL, controls private downloads.
See [GitHub's release documentation](https://docs.github.com/en/repositories/releasing-projects-on-github/about-releases).

When the repository becomes public, keep using its existing Releases page. Build
future versions from an exact commit, retain the matching source snapshot and
checksums, and publish a new version for changed binaries. Preserve beta validation
records as historical evidence instead of rewriting them to imply later checks
were performed on earlier builds.
