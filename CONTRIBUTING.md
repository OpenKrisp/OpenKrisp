# Contributing to OpenKrisp

The source repository is public. Open issues and pull requests here to contribute.
Releases also stay in this repository; see the
[release guide](docs/COMMUNITY_RELEASE.md).

Original OpenKrisp code uses [GNU GPLv3 only](LICENSE) (`GPL-3.0-only`), with the
scope in [NOTICE](NOTICE). This is an open-source project. Original contributions
are submitted under the same GPLv3 terms unless compatible separate terms are
explicitly identified; contributors retain their copyright. Preserve existing license
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
checks; its first release build/install run is still pending. Hosted CI is running. Linux desktop
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

## Project documentation

1. [Product Scope and Requirements](docs/01_PRODUCT_SCOPE.md)
2. [System Architecture](docs/02_SYSTEM_ARCHITECTURE.md)
3. [Real-Time Audio Engine](docs/03_REALTIME_AUDIO_ENGINE.md)
4. [DeepFilterNet Integration](docs/04_DEEPFILTERNET_INTEGRATION.md)
5. [Tauri Application and IPC](docs/05_TAURI_APP_AND_IPC.md)
6. [Platform Routing](docs/06_PLATFORM_ROUTING.md)
7. [Rust Workspace and Public APIs](docs/07_RUST_WORKSPACE_AND_APIS.md)
8. [Implementation Roadmap](docs/08_IMPLEMENTATION_ROADMAP.md)
9. [Test Strategy](docs/09_TEST_STRATEGY.md)
10. [Autonomous Coding Assistant Guide](docs/10_AUTONOMOUS_AGENT_GUIDE.md)
11. [Architecture Decisions, Risks, and Open Questions](docs/11_DECISIONS_RISKS_AND_OPEN_QUESTIONS.md)
12. [Implementation Backlog](docs/12_IMPLEMENTATION_BACKLOG.md)

## Report an issue or submit a pull request

For bugs, include the app version, OS and CPU architecture, reproduction steps,
expected behavior, and actual behavior. For audio issues, include device names,
routing choices, and a sanitized diagnostic report. Avoid uploading private call
recordings, credentials, personal configuration, or private correspondence.

Keep each pull request focused. Explain the user-visible behavior, relevant
implementation choices, checks run, and remaining limitations. Preserve the
[audio architecture invariants](docs/02_SYSTEM_ARCHITECTURE.md#core-architectural-invariants), especially
the real-time callback rules and the boundary that keeps PCM out of Tauri IPC.
For larger changes, open an issue describing the proposal before implementation.

Merging a version change into `main` starts release preparation after CI passes,
then automatically publishes the verified packages. Include matching versions,
release notes, and acceptance results in the version PR; its merge authorizes
publication. Ordinary merges with no version change do not create releases.
The [Actions release guide](docs/RELEASING.md) also covers manual preparation and retries.
Do not replace a published version's assets with a different build; use a new
version so its source commit, checksums, and validation remain traceable.
