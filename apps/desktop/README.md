# OpenKrisp desktop

The Phase 4 desktop control plane uses Tauri v2, Vite, React, and strict TypeScript. It owns only
commands, low-rate events, snapshots, profiles, windows, and tray lifecycle; PCM remains entirely
inside the Rust audio engine.

Useful commands:

```text
npm ci
npm run dev
npm run typecheck
npm test
npm run build
npm run tauri dev
npm run tauri build -- --debug --bundles app
```

Build the frontend before invoking workspace-wide Cargo checks directly; the default Tauri
`custom-protocol` feature embeds `dist` for normal and packaged binaries.

From the repository root, the focused Rust integration harness is:

```text
cargo test -p openkrisp-desktop --lib control_plane_harness
```

It submits real mock-runtime `InvokeRequest`s to the same handler used in production, uses a
deterministic engine double, crosses the bounded Rust event bridge, reopens profile storage, and
checks the production capability allowlist. `npm test --prefix apps/desktop` independently verifies
the TypeScript command envelopes, listener lifecycle, and snapshot reconciliation. Neither suite
opens an audio device or sends PCM across the desktop boundary.
