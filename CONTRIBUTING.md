# Contributing to OpenKrisp

The repository is private during beta preparation. Contributors with access can
open issues and pull requests here. Releases also stay in this repository; see
the [release guide](docs/COMMUNITY_RELEASE.md).

Original OpenKrisp code uses [PolyForm Perimeter 1.0.0](LICENSE), with the scope in
[NOTICE](NOTICE). This is a source-available project. Preserve existing license
and attribution notices, and identify the source and license of any third-party
code or assets you add. See [licensing status](docs/LICENSING.md).

## Set up a development environment

Use macOS or Linux. Install Git, Node.js 24 with npm, Python 3 with venv/pip,
Rust through rustup, and a native C/C++ compiler. `rust-toolchain.toml` pins Rust
1.98.0 with Clippy and rustfmt. Native AEC builds require Meson 1.9.2 and Ninja
1.13.0, installed below. On macOS, install the Xcode command-line tools.

Linux also needs the audio and Tauri development packages listed in the
[CI workflow](.github/workflows/ci.yml) and [desktop setup](apps/desktop/README.md).
The release workflow includes native x86-64 DEB/RPM builds and container installation
checks; its first Linux run is still pending GitHub Actions access. Linux desktop
audio acceptance remains manual. Cross-compiling the native AEC build is not supported.

From the repository root:

```sh
python3 -m venv target/aec-tools
target/aec-tools/bin/python -m pip install meson==1.9.2 ninja==1.13.0
npm ci --prefix apps/desktop
npm run --prefix apps/desktop build
npm run --prefix apps/desktop tauri -- dev
```

The frontend build is needed before workspace Cargo checks because the packaged
desktop feature embeds its assets. Initial setup needs network access to fetch
dependencies. Follow the [installation guide](docs/INSTALLATION.md) for virtual
audio device setup; starting the development app does not install macOS drivers.

## Validate a change

Run the checks appropriate to the changed code. Full software checks are:

```sh
npm run --prefix apps/desktop typecheck
npm test --prefix apps/desktop
cargo ci-fmt
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo ci-test --locked
python3 -m unittest discover -s scripts/tests -v
```

Dependency changes also need `cargo deny check licenses` with cargo-deny installed.
Native audio and installer changes need their focused checks from the
[test strategy](docs/09_TEST_STRATEGY.md) and [release guide](docs/COMMUNITY_RELEASE.md).
Do not describe software tests as proof of installed-device or listening quality.

## Report an issue or submit a pull request

For bugs, include the app version, OS and CPU architecture, reproduction steps,
expected behavior, and actual behavior. For audio issues, include device names,
routing choices, and a sanitized diagnostic report. Avoid uploading private call
recordings, credentials, personal configuration, or private correspondence.

Keep each pull request focused. Explain the user-visible behavior, relevant
implementation choices, checks run, and remaining limitations. Preserve the
[audio architecture invariants](README.md#core-architectural-invariants), especially
the real-time callback rules and the boundary that keeps PCM out of Tauri IPC.
For larger changes, open an issue describing the proposal before implementation.

Release preparation and publication use the manual [Actions workflow](docs/RELEASING.md).
Do not replace a published version's assets with a different build; use a new
version so its source commit, checksums, and validation remain traceable.
