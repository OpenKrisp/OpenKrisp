# Public source launch

The maintainer explicitly authorized making `OpenKrisp/OpenKrisp` public on
2026-10-20. Development, tags, and published releases remain in this repository.
Source publication does not qualify the experimental beta as a production-ready
release; the platform and hardware limitations documented below still apply.

## Licensing and terminology

The selected license for current development source is **GNU GPLv3 only**
(`GPL-3.0-only`), an [OSI-approved open-source license](https://opensource.org/license/gpl-3-0).
It permits commercial use, modification, and redistribution, including competing
forks. Preserve GPLv3 terms and provide corresponding source for covered binary
distributions and modifications as required by the license.

Use **open source** for the current project. Earlier beta installers predate the
2026-10-20 licensing transition and retain their original bundled terms; publish
a new version for GPLv3-labeled binaries. See [licensing status](LICENSING.md) for
third-party scope, source distribution, and historical permission records.
The maintainer authorized both the license transition and public source access.

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
  public release downloads and the remaining manual acceptance work.
- The [Actions workflow](RELEASING.md) prepares version merges after successful CI
  and automatically publishes their verified files after successful preparation.
  Manual operations remain available. Beta 2 and beta 3 were published through
  these checks; the earlier local beta remains a historical draft.
- The workflow includes macOS ARM64 and Linux x86-64 DEB/RPM packages. It requires
  native builds and Linux package installation checks before assembling a draft.
  Published beta 2 and beta 3 passed those gates; each new release must pass again.

## Remaining release validation and maintenance

- Continue reviewing public history, issues, pull requests, attachments, releases,
  Actions logs, and artifacts for unintended disclosures. A limited credential-pattern
  scan of 1,001 historical text blobs up to 1 MiB found no matches at source
  publication; this was not a complete privacy or secrets audit. GitHub documents
  that [Actions history and logs become public](https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/managing-repository-settings/setting-repository-visibility).
- Verify contributor setup from a fresh checkout on the supported platforms and
  recheck introductory documentation against the version chosen for public launch.
- Record a passing main CI run and a successful native release build/install run
  for the launch version. Hosted CI resumed on 2026-10-01. Local beta results are
  documented separately and do not substitute for those release checks.
- The persistent updater signing key and public key variable are configured. Test
  signed upgrade/restart on each desktop platform. The anonymous update feed is now
  publicly accessible; see [automatic updates](AUTOMATIC_UPDATES.md).
- Run the beta on a fresh Mac: browser download, Gatekeeper exception, installation,
  microphone permission, virtual device enumeration, upgrade/restart, and real-call
  listening. Record outcomes and retain explicit limitations for unqualified
  speakerphone, long-run, or platform scenarios.
- Run the Linux packages on Debian/Ubuntu and Fedora desktops: launch the GUI,
  start both PipeWire routes, place a real call, and test update/removal. Container
  package checks do not qualify desktop audio or hardware behavior.
- Choose and document a maintained channel for private vulnerability reports before
  inviting public reports; do not advertise a reporting feature that is not enabled.
- Review the version and release notes being exposed, including the licensing chronology of historical
  beta installers. Preserve those historical license and validation records.

## Keep the release history in one place

Published releases and prereleases are publicly downloadable. Draft releases
remain visible to maintainers.
See [GitHub's release documentation](https://docs.github.com/en/repositories/releasing-projects-on-github/about-releases).

Keep using the existing public Releases page. Build
future versions from an exact commit, retain the matching source snapshot and
checksums, and publish a new version for changed binaries. Preserve beta validation
records as historical evidence instead of rewriting them to imply later checks
were performed on earlier builds.
