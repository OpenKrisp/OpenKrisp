# OpenKrisp licensing

## Current project license

On 2026-10-20 the maintainer selected the **GNU General Public License, version 3
only** for original OpenKrisp code and documentation, except where other terms are
identified. The SPDX expression is `GPL-3.0-only`. The complete, unmodified license
is in [LICENSE](../LICENSE), with copyright and third-party scope in
[NOTICE](../NOTICE). The desktop npm package carries an identical copy in
[apps/desktop/LICENSE](../apps/desktop/LICENSE).

OpenKrisp is **open source** under an
[OSI-approved license](https://opensource.org/license/gpl-3-0). The license allows
use, modification, commercial use, and redistribution, including competing forks.
There is no project-specific noncompete restriction. When conveying covered
modified versions, preserve GPLv3 licensing and provide corresponding source to
recipients as the license requires. Private modifications do not require public
disclosure merely because they are made. The license text controls.

This decision records the maintainer's authorization to relicense the original
project code. Contributors retain copyright in their contributions; this change
does not assign their copyright or relicense independently licensed third-party
materials. Future original contributions are submitted under `GPL-3.0-only`
unless separately licensed terms are explicitly identified and compatible.

## Source and release distribution

Cargo workspace packages inherit `license = "GPL-3.0-only"`. npm and Tauri bundle
metadata use the same SPDX expression. Desktop bundles include LICENSE and NOTICE
as resources, alongside native AEC and model notices. The combined macOS installer
displays the OpenKrisp and BlackHole notices and license texts.

For each new binary release, provide the matching, complete corresponding source
and build/install inputs under GPLv3, including project changes and any covered
driver changes. Keep the exact release commit, source snapshot, dependency source
and attribution materials required by their terms, and source download directions
alongside the binaries. A link to a changing development branch is not a substitute
for the corresponding source of a particular binary. Follow
[community release preparation](COMMUNITY_RELEASE.md).

The maintainer made the source repository public on 2026-10-20, separately from
the licensing transition. Source and published releases are available without
repository invitations. Recipients receive the GPL rights granted with the source.
See the [public-source launch record](PUBLIC_LAUNCH.md) for scope and remaining
release validation work.

## Third-party scope and packaging

Dependencies, model assets, vendored sources, and files carrying their own notices
retain their existing terms. Keep their source and attribution obligations intact.
The combined application is distributed under GPLv3 in compliance with applicable
component terms; this does not replace their individual source licenses.

- DeepFilterNet model materials retain the upstream MIT/Apache-2.0 choices and
  notices in `models/`.
- Native AEC and Abseil materials retain the licenses and patent notices recorded
  in `third_party/aec/README.md` and `third_party/aec/notices/`.
- The macOS installer builds two drivers from pinned BlackHole 0.7.1 source:
  OpenKrisp Microphone (2 channels) and OpenKrisp Speaker (16 channels). Upstream
  GPL-3.0 notices, the original source archive, and exact build inputs remain
  included. BlackHole's published
  [integration guidance](https://github.com/ExistentialAudio/BlackHole#can-i-integrate-blackhole-into-my-app)
  permits GPL-3.0 applications; current GPLv3 distribution follows those terms.
- Other dependencies and separately licensed files retain their own terms.

This migration does not claim to finish a dependency-wide release attribution
review or to validate a new installer. Run the dependency-license checks, notice
collector, and installer verification for each release as documented.

## Historical releases and permission records

Earlier tagged releases, including `v0.1.0-beta.1`, `v0.1.0-beta.2`, and
`v0.1.0-beta.3`, predate this transition and retain the license and notices packaged
with their original artifacts. Their release notes and synchronized changelog are
historical records. Publish a new version for GPLv3-labeled binary packages;
do not replace old assets or rewrite old validation results to imply they shipped
under the new terms. The current development source is licensed as stated above.

The project previously used PolyForm Perimeter 1.0.0. On 2026-09-25 the maintainer
confirmed written permission for that earlier BlackHole integration. The agreement
itself was not added to or independently reviewed within the repository. The
[permission request](BLACKHOLE_PERMISSION_REQUEST.md) is an archived, unsent draft,
not the actual agreement. This history does not add restrictions to the current
GPLv3 grant or publish private correspondence.

## License provenance

- [GNU GPLv3 text](https://www.gnu.org/licenses/gpl-3.0.txt)
- [SPDX GPL-3.0-only license text](https://github.com/spdx/license-list-data/blob/main/text/GPL-3.0-only.txt)

Both project license copies contain the same unmodified standard GPLv3 text.
The version-only choice is expressed by `GPL-3.0-only` in project metadata and
the project notice, rather than by editing the license document.
