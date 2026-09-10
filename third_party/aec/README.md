# Pinned AEC prototype sources

These are source archives, not prebuilt libraries. `scripts/build-aec.py` verifies every
entry in `SHA256.json` before extraction. Compilation does not download dependencies or
select a system Abseil installation. Native build products stay in Cargo's `OUT_DIR`.

| File | Origin |
| --- | --- |
| `webrtc-audio-processing-sys-2.1.0.crate` | [Published crate](https://static.crates.io/crates/webrtc-audio-processing-sys/webrtc-audio-processing-sys-2.1.0.crate); bindings commit `c14d7af1760baff83e8210fee336a0cae0faaa7d`, native submodule `d0569cfa50c1858ee279d77b3fc8870be6902441` |
| `abseil-cpp-20240722.0.tar.gz` | [Abseil tag archive](https://codeload.github.com/abseil/abseil-cpp/tar.gz/refs/tags/20240722.0); matches the upstream wrap's release archive checksum |
| `abseil-meson.build` | [Meson wrapdb build definition](https://raw.githubusercontent.com/mesonbuild/wrapdb/b71ad4206991215f6e9db9a7948bdfccd7d25e0e/subprojects/packagefiles/abseil-cpp/meson.build), tag `abseil-cpp_20240722.0-3`; Apache-2.0 header retained |

Only the native C++ APM is compiled from the sys crate. The upstream Rust wrapper, bindgen,
and its allocating per-call channel-vector conversion are not used. Our bridge is in
`crates/aec-webrtc/src/bridge.cc`. Build-time changes to upstream Meson select the vendored
Abseil subproject explicitly and add that bridge; original archives remain unmodified.

`notices/` reproduces the native BSD license, patent grant, author list, additional Ooura,
FFT, PFFFT, RNNoise and square-root notices, and Abseil's Apache license. The complete
archives retain upstream file-level notices too. These notices must accompany any distributed
AEC-enabled binary; normal desktop packages do not enable this prototype. A final product
notice/packaging review remains E17-07, not a completed outcome of this slice.

See [the source evaluation](../../docs/AEC_EVALUATION.md) and
[the prototype report](../../docs/AEC_PROTOTYPE_IMPLEMENTATION_REPORT.md).
