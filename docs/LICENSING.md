# OpenKrisp licensing

Decision recorded on 2026-09-25: original OpenKrisp code and documentation use
**PolyForm Perimeter License 1.0.0**, except where other terms are identified.
The complete, unmodified text is in [LICENSE](../LICENSE), with scope and the
required copyright notice in [NOTICE](../NOTICE). The desktop npm package carries
an identical copy of the license in [apps/desktop/LICENSE](../apps/desktop/LICENSE).

OpenKrisp is **source available**. Perimeter restricts use of the licensed software
to provide competing products, including free competing products. Ordinary use
and private changes for permitted purposes remain allowed. Redistribution must
preserve the license terms or their URL and the required notices. The license
text controls; this explanation adds no conditions or permissions.

The restriction does not satisfy the [Open Source Definition](https://opensource.org/osd).
It does not prohibit independently developed competing software merely because
it has similar functionality. Contributors retain rights in their own work;
the project license does not assign their copyright.

## Scope and packaging

Third-party dependencies, model assets, vendored sources, and files carrying
their own notices retain their existing terms. They are not relicensed under
PolyForm Perimeter. Keep their source and attribution obligations intact.

Rust workspace packages inherit a reference to the root license file. The
desktop npm metadata points to its local copy. Desktop bundles include LICENSE
and NOTICE as resources, alongside the existing AEC notices and the model
licenses. The combined macOS installer displays OpenKrisp and BlackHole terms
separately within one license document and retains the BlackHole source bundle.
This change does not complete a dependency-wide release attribution review.

## BlackHole permission remains unresolved

The current macOS installer builds two drivers from pinned BlackHole 0.7.1
source: OpenKrisp Microphone (2 channels) and OpenKrisp Speaker (16 channels).
The upstream source is compiled with customized names, bundle/device identities,
factory identities, and an OpenKrisp icon. BlackHole source and build inputs
remain included with the installer.

BlackHole's published [integration terms](https://github.com/ExistentialAudio/BlackHole#can-i-integrate-blackhole-into-my-app)
state that integrating applications must use GPL-3.0 or obtain a separate
license from Existential Audio. The project has not obtained that permission.
Choosing Perimeter does not resolve this issue, and a draft letter is not an
agreement. A future objection is not the trigger for seeking permission.

Before distributing the current combined macOS installer, obtain written terms
that cover this integration, or replace it with an implementation whose license
supports the intended distribution. Record the scope of any permission and
the resulting attribution and source obligations. Signing, notarization, and
successful tests do not establish license permission.

The [permission request](BLACKHOLE_PERMISSION_REQUEST.md) is an unsent draft.
Any eventual agreement must identify the parties and covered components and
be reviewed and accepted by authorized representatives. It must not imply that
PolyForm restrictions apply to BlackHole copies received under GPL-3.0.

## License provenance

- [Official PolyForm Perimeter 1.0.0 text](https://github.com/polyformproject/polyform-licenses/blob/1.0.0/PolyForm-Perimeter-1.0.0.md)
- [PolyForm license-text reuse terms](https://github.com/polyformproject/polyform-licenses/blob/1.0.0/README.md#license)

The license text is unchanged. The root NOTICE is separate from that text.
Package metadata uses a file reference or `LicenseRef-PolyForm-Perimeter-1.0.0`;
the latter is a local license reference, not a claim of an SPDX-listed license ID
or OSI approval.
