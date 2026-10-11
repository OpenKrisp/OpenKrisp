# Releases through GitHub Actions

All releases belong to `saeta-eth/OpenKrisp`. Keep this repository private for now.
Merging a **version change** into `main` authorizes automatic publication:

1. **CI** passes for that exact main commit.
2. [Release](../.github/workflows/release.yml) validates versions and release notes,
   creates the version tag, and builds/verifies all installers and draft assets.
3. [Publish release](../.github/workflows/publish-release.yml) starts only after the
   complete preparation run succeeds, verifies its draft against the retained
   artifact, and publishes those files.
4. [Update changelog](../.github/workflows/changelog.yml) synchronizes
   [CHANGELOG.md](../CHANGELOG.md) with all published GitHub release notes and
   commits it to the default branch. Drafts are excluded; prereleases are included.

Changelog synchronization also runs for releases published manually and can be
rerun through **Actions → Update changelog → Run workflow**. It retrieves all
release pages, orders entries by publication date, and makes no commit when the
file already matches. The generated file is owned by the workflow; edit release
notes on GitHub and rerun synchronization to correct an entry.

The completion triggers cover automatic and manual Actions publication because
releases published with `GITHUB_TOKEN` do not trigger another `release` workflow.
Synchronization requires the default branch to allow the workflow token to write
`CHANGELOG.md`; branch protection that blocks bot commits must allow this update.

There is no approval pause between automatic preparation and publication. Ordinary
merges without a version change create no release. Tag creation alone does not
start a release. Published versions are never rebuilt or overwritten.

## Packages and checks

| Package | Native build environment | Installation checks |
| --- | --- | --- |
| macOS ARM64 `.pkg` | Apple Silicon macOS runner | Package inspection and driver probes; fresh-Mac installation remains manual |
| Linux x86-64 `.deb` | Debian 12 | APT installation on Debian 12 and Ubuntu 24.04 |
| Linux x86-64 `.rpm` | Fedora 44 | DNF installation on Fedora 44 |

Linux builds run in distribution containers on x86-64 runners. Separate baselines
avoid building the Debian binary against Fedora's newer libraries. No macOS Intel
or Linux ARM binaries are produced. All packages are unsigned. macOS app/driver
signatures are ad hoc, without notarization or Apple Developer credentials. See
[community distribution](COMMUNITY_RELEASE.md).

Linux release builds and installation checks use GitHub-hosted `ubuntu-24.04`
x86-64 runners. CI, release orchestration, and the macOS build use the on-demand
ARM64 runners described in [runner setup](../.github/runners/README.md). Start
both on-demand runners before opening or merging a version PR, and keep them
running through publication and changelog synchronization. Hosted jobs remain
subject to the account's existing Actions quota and spending settings.

The workflow uses the automatic `GITHUB_TOKEN`; no personal token is required.
Updater-enabled releases additionally require the persistent Tauri signing key
secrets and public key variable described in [automatic updates](AUTOMATIC_UPDATES.md).
Validation and builds have read-only repository permissions. Only tag creation,
draft upload, publication, and changelog synchronization receive repository write
permission. These workflows do not change visibility or collaborator access.

These workflows require actual successful GitHub Actions CI runs; local tests
cannot override that requirement. The native release build/install matrix must
complete successfully before a version can be published.

## Prepare a version PR

Choose a new version, such as `0.1.0-beta.2`. Update:

- `Cargo.toml` → `workspace.package.version` and workspace versions in `Cargo.lock`.
- `apps/desktop/package.json` and both root version fields in `package-lock.json`.
- `apps/desktop/src-tauri/tauri.conf.json` → `version`.
- `docs/releases/<version>.md` with changes, installation/signing restrictions,
  and manual acceptance results and limitations for that version.

Review and perform any intended real-machine acceptance **before merging** the
version PR. After the merge, the workflows publish automatically if all checks
pass. The main commit must change the workspace version relative to its first
parent; standard merge commits and squash merges both support this. For a rebase
merge, put the complete version change in the final commit so the main CI commit
carries it. Avoid separating version changes from their release notes.

Version tags use SemVer without `+build` metadata. Prerelease tags, such as
`v0.1.0-beta.2`, produce GitHub prereleases; stable tags produce regular releases.
Neither is automatically marked Latest. The workflow creates a lightweight tag
on the exact tested commit and refuses to move an existing tag to another commit.
CI reruns for an already published version skip release creation.

The earlier manually prepared `v0.1.0-beta.1` remains a historical draft. Its
original tag/assets are preserved; automatic releases cannot overwrite or publish
it. Use a new version for the first automated release.

Linux metadata uses native ordering. For `0.1.0-beta.2`, DEB uses
`Version: 0.1.0~beta.2`; RPM uses `Version: 0.1.0` and `Release: 0.beta.2`.
Stable RPMs use release `1`. The compiled app, source tag, and download filenames
keep the complete SemVer version. This follows [RPM version rules](https://rpm.org/docs/6.0.x/man/rpm-version.7)
and supports beta-to-stable upgrades.

## Automatic preparation and publication

Only successful **push CI runs on main in this repository** authorize preparation.
PR CI and fork runs cannot authorize releases. Workflow event data is checked
against GitHub's run/attempt records before it is used. Version consistency,
main ancestry, required notes, tag ownership, and draft ownership are validated.

Native jobs fetch locked dependencies, collect platform-specific notices and
required source archives, and build optimized packages. The macOS job verifies
signatures/architecture, runs release performance checks, and inspects the combined
installer. It does not install drivers on the runner. Linux jobs verify package
metadata, architecture, GUI/audio dependencies, model resources, and notices.
All build jobs run Python tests.

Fresh Debian 12, Ubuntu 24.04, and Fedora 44 containers install the actual packages
through APT/DNF, inspect linked libraries with `ldd`, and check PipeWire tools.
Any failed build or installation check blocks draft assembly.

The complete draft and its 30-day Actions artifact contain fifteen files: `.pkg`,
`.deb`, `.rpm`, exact source archive, both installation guides, `RELEASE_NOTES.md`,
`VALIDATION.md`, `PROVENANCE.json`, `SHA256SUMS`, the macOS `.app.tar.gz` update,
three updater signatures, and `latest.json`. Uploaded hashes are verified.
Provenance records the source commit, exact CI run, preparation run/attempt, and
manual or automatic trigger.
New provenance uses schema 3; historical schema 2 releases keep their original
ten-file validation. Updater signatures are checked against the app's public key
in native build jobs, and the manifest is checked against the exact version,
package URLs, release notes, and signature files before publication.

Automatic publication requires the completed successful preparation attempt and
its artifact. It rechecks CI for the exact source commit, tag, provenance, file
set, checksums, and equality with the original artifact before publishing. A
changed/missing file, failed preparation, moved tag, expired artifact, unrelated
draft, or artifact from a different attempt causes failure. A publication retry
verifies an already published release and exits without editing it.

This uses GitHub's [workflow completion events](https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows#workflow_run)
in the sequence **CI → Release → Publish release**. Artifact data is read as data;
build steps do not receive publication permissions.

## Retry or use manual draft review

For a failed automatic preparation, use **Re-run all jobs** on its Release run.
This keeps every installer and final artifact on the same attempt. Do not rerun
only a failed draft-upload job. A retry may replace assets only in a draft owned
by this workflow for the same tag/commit. Review new files if you are investigating
a release failure. If publication failed after preparation succeeded, rerun its
**Publish release** workflow; it verifies the same retained files again.

If preparation fails because the workflow itself needs a fix, merge the workflow
fix and use manual **prepare** from the updated `main` for the existing tag, then
manual **publish** after all preparation checks pass. Re-running the old run keeps
its original commit and workflow, so it cannot pick up that fix; see
[GitHub's re-run behavior](https://docs.github.com/en/actions/how-tos/manage-workflow-runs/re-run-workflows-and-jobs).
The source tag stays on its tested commit. Changes to the app or release scripts
instead require a new version and tag; do not move the existing tag.

Manual operations remain available under **Actions → Release → Run workflow**,
with **Use workflow from: main**. They do not automatically publish: a manual
**prepare** creates a review draft, and a separate **publish** publishes it.
Choose an existing version tag on main with successful CI. For example:

```sh
gh workflow run release.yml --repo saeta-eth/OpenKrisp --ref main \
  -f operation=prepare -f tag=v0.1.0-beta.2
# After reviewing the successful preparation and its files:
gh workflow run release.yml --repo saeta-eth/OpenKrisp --ref main \
  -f operation=publish -f tag=v0.1.0-beta.2
```

An automatic version merge starts publication, so use an existing tagged version
and manual preparation when a draft review pause is required. Do not use a manual
upload or GitHub's Publish button to bypass verification. Use a new version/tag
for changed source or binaries. Never move or reuse a published tag.

If the artifact expires before a manual publication, prepare again and review the
new files. Repository policy can shorten the nominal 30-day retention.

## Customer access and remaining acceptance

Publication stays in this repository. While it is private, downloads require
repository read access. Drafts are preparation artifacts and are not the download
channel for customers with read-only access. Making the repository public remains
a separate maintainer decision; see [public launch preparation](PUBLIC_LAUNCH.md).

Fresh-Mac download/Gatekeeper behavior, system installation, upgrade/restart,
microphone permission, Linux desktop launch/PipeWire routing, real calls, device
transitions, and long-running behavior require manual acceptance. Container
installation and software tests do not establish those results. Record untested
scenarios in version notes before merging the release PR.
