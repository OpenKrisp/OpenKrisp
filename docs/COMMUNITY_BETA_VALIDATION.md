# Community beta 1 validation

Recorded on 2026-09-25 for `0.1.0-beta.1`, built locally on Apple Silicon.

## Passed

- Frontend TypeScript check and 63 tests across 9 test files.
- `cargo ci-fmt` and workspace Clippy with all targets/features and warnings denied.
- `cargo ci-test --locked --offline`: 217 passed; the two ignored wall-clock
  benchmarks were subsequently run explicitly in release mode and passed.
- `cargo deny --offline check licenses`.
- 13 Python tests, including rejection of a tampered signed app before driver
  downloads and rejection of corrupted cached dependency notices.
- Optimized Tauri community build with explicit ad-hoc signing. Final app signature
  verification passed, the executable is ARM64, and its minimum macOS version is 11.0.
- Project/model license resource contents and the generated dependency inventory
  matched their build inputs. The inventory contains 353 Rust dependencies
  (including build dependencies), 4 production npm dependencies, and 6 source
  archives (5 MPL crates plus RealFFT's original source/declarations).
- Combined community installer built and inspected without installing it. The
  package verifier checked license contents, pinned driver source/build inputs,
  both native driver probes, signatures, identities, and restart choices.
- Local documentation links and `git diff --check`.
- A limited credential-pattern scan of 706 historical text blobs found no matches.
  It was not an exhaustive audit of all private information.

The installer is unsigned and the app/drivers are ad-hoc signed. No Apple
notarization or identified-developer signature is claimed.

## Not performed

No clean-Mac browser-download, Gatekeeper exception, real system installation,
upgrade/restart, microphone permission, real-call listening, or long-run device
acceptance was performed for this release. Isolated driver probes are not a
substitute for those checks. Intel and Linux release artifacts were not built or
qualified. Release notes describe these beta limitations.

The GitHub Actions run for licensing PR #18 did not start its jobs because of
account billing/spending restrictions. These are local validation results;
the remote CI run is not passing. Account billing settings were not changed.

The maintainer confirmed written permission for the current BlackHole integration.
The agreement itself was not independently reviewed or published; see
[licensing status](LICENSING.md).

The release's generated provenance file records its source commit, and its
SHA256SUMS file records the exact downloadable assets.
