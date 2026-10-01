# Releases through GitHub Actions

All releases belong to `saeta-eth/OpenKrisp`. Keep this repository private for now.
The [Release workflow](../.github/workflows/release.yml) has two manual operations:
**prepare** builds a draft; **publish** releases its verified files without rebuilding.
Pushes, merges, and tag creation do not trigger either operation.

The workflow requires all three community installers:

| Package | Native build environment | Installation checks |
| --- | --- | --- |
| macOS ARM64 `.pkg` | Apple Silicon macOS runner | Package inspection and driver probes; fresh-Mac installation remains manual |
| Linux x86-64 `.deb` | Debian 12 | APT installation on Debian 12 and Ubuntu 24.04 |
| Linux x86-64 `.rpm` | Fedora 44 | DNF installation on Fedora 44 |

Linux builds run in distribution containers on x86-64 runners; separate baselines
avoid building the Debian binary against Fedora's newer system libraries. No
macOS Intel or Linux ARM binaries are produced. All packages are unsigned. macOS
app/driver signatures are ad hoc, without notarization or Apple Developer
credentials. See [community distribution](COMMUNITY_RELEASE.md).

## One-time setup

Merge the release workflow, scripts, and licensing changes into `main`. GitHub
requires a manually run workflow to be present on the default branch. The Release
workflow must be run with **Use workflow from: main**. See
[GitHub's manual workflow instructions](https://docs.github.com/en/actions/how-tos/manage-workflow-runs/manually-run-a-workflow).

The workflow uses the repository's automatic `GITHUB_TOKEN`; no personal access
token or signing secret is needed. Validation can read Actions results. Only the
draft and publish jobs can write repository contents. Build steps have read-only
repository access. It does not change repository visibility or collaborator access.

GitHub Actions jobs were blocked by account billing/spending restrictions during
setup. Hosted CI resumed on 2026-10-01. The workflow refuses to proceed without
successful CI for the exact source commit; local test results do not override
that check. No billing settings are changed by this workflow.

## 1. Prepare the version in a pull request

Choose a new version, such as `0.1.0-beta.2`. Update:

- `Cargo.toml` → `workspace.package.version` and the workspace package versions in
  `Cargo.lock` (regenerate the lockfile with Cargo).
- `apps/desktop/package.json` and both root version fields in its `package-lock.json`.
- `apps/desktop/src-tauri/tauri.conf.json` → `version`.
- `docs/releases/<version>.md` with user-facing changes, install/signing restrictions,
  and the manual acceptance results and limitations for that version.

Merge the version PR into `main` and wait for the **CI** push run on that exact
commit to succeed. PR-only CI results are not a substitute for this run.

## 2. Tag the tested commit

Create a `v<version>` tag on that exact main-branch commit and push it. For example,
after verifying the commit you intend to tag:

```sh
git tag -a v0.1.0-beta.2 <tested-main-commit> -m 'OpenKrisp 0.1.0 beta 2'
git push origin refs/tags/v0.1.0-beta.2
```

The workflow requires matching versions across Cargo, npm, Tauri, and lockfiles,
plus notes for that version. It accepts SemVer tags without `+build` metadata.
Tags with a prerelease suffix are published as GitHub prereleases. Tags without
one are regular releases; neither is automatically marked Latest.

Linux package metadata uses native version ordering. For `0.1.0-beta.2`, DEB uses
`Version: 0.1.0~beta.2`; RPM uses `Version: 0.1.0` and `Release: 0.beta.2`. Stable
RPMs use release `1`. The app, source tag, and download filenames keep the complete
SemVer version. The Linux builder compiles that version first, then bundles it;
only package metadata is adjusted. This avoids RPM's forbidden version hyphen and
makes the stable version newer than its betas. See
[RPM version rules](https://rpm.org/docs/6.0.x/man/rpm-version.7).

Do not move or reuse an existing tag. The earlier manually prepared
`v0.1.0-beta.1` release has been returned to draft; its tag and assets are retained.
It is not a workflow-produced draft, and the new workflow will not overwrite or
publish it. Choose a fresh version for the first Actions build.

## 3. Run prepare

In **Actions → Release → Run workflow**, select branch **main**, operation
**prepare**, and the tag. The equivalent CLI command is:

```sh
gh workflow run release.yml --repo saeta-eth/OpenKrisp --ref main \
  -f operation=prepare -f tag=v0.1.0-beta.2
```

Validation resolves the tag, requires its commit to be on main, checks version
consistency, and checks successful CI. Native macOS and Linux jobs fetch locked
dependencies, collect platform-specific third-party notices/source archives, and
build optimized packages. The macOS job verifies signatures/architecture, runs
both release performance benchmarks, and inspects the combined installer. It does
not install drivers on the runner. Linux jobs check package versions, architecture,
GUI/audio dependencies, models, and bundled notices. All build jobs run Python tests.

Fresh Debian 12, Ubuntu 24.04, and Fedora 44 containers install the actual Linux
packages with APT/DNF, check linked libraries with `ldd`, and verify the PipeWire
tools are present. These checks do not launch a desktop or qualify hardware audio.
Any failed build or installation check blocks assembly of the complete draft.

The workflow retains a 30-day Actions artifact and creates a draft with ten files:
the `.pkg`, `.deb`, `.rpm`, exact source archive, both platform installation guides, `RELEASE_NOTES.md`,
`VALIDATION.md`, `PROVENANCE.json`, and `SHA256SUMS`. It verifies uploaded hashes.
Provenance links to the source commit, normal CI, and the preparation run/attempt.
Validation describes checks actually performed by Actions; it does not copy old
local test counts or claim clean-Mac, Linux desktop, or listening acceptance. The
new Linux release build/install matrix has not yet run; routine CI does not
execute the manual Release workflow.

## 4. Review the draft

Wait for the entire prepare run to succeed. Inspect the draft notes and files and
test installation/audio behavior as needed. Drafts are review artifacts; they are
not yet the download channel for testers with read-only access.

If a binary or its source needs changes, use a new version/tag. If preparation or
upload fails transiently, use **Re-run all jobs** so every job shares the same
attempt and artifact name. Do not use Re-run failed jobs for a failed draft-upload
job. A rerun may replace files only in a draft owned by this workflow for the same
tag/commit. It never replaces a published release or an unrelated/manual draft.
Review the new files after a rerun.

## 5. Run publish when ready

Run **Release** again from **main**, this time with operation **publish** and the
same tag:

```sh
gh workflow run release.yml --repo saeta-eth/OpenKrisp --ref main \
  -f operation=publish -f tag=v0.1.0-beta.2
```

Publish downloads the existing draft and verifies its exact file set, checksums,
source commit, successful preparation attempt, and equality with that attempt's
retained Actions artifact. It rechecks the tag before publishing. No binaries are
rebuilt or replaced. Missing/changed assets, moved tags, expired artifacts, failed
preparation, and already published releases cause failure.

If the Actions artifact expires before publication, run prepare again and review
the new draft files. The 30-day retention may be shortened by repository policy.

Publication stays in this repository. While it remains private, downloads require
repository read access. A future visibility change is a separate maintainer action.
