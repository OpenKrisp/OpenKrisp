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
