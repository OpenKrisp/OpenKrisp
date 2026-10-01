# App icon

`source.svg` is the editable master. The icon matches the desktop header and menu
bar: three rounded strokes tilted 18 degrees, in mint `#63D9B3`, centered on a
dark green tile. Transparent outer padding gives the icon room in the macOS Dock.

Generate the platform assets from `apps/desktop`:

```sh
npm run tauri -- icon src-tauri/icons/source.svg --output /tmp/openkrisp-icons --ios-color '#08110f'
```

Copy the generated files over their existing counterparts here. Tauri also emits
Windows assets; OpenKrisp targets macOS and Linux, so do not add those files.
The existing iOS and Android images are kept visually consistent with the master;
their presence does not imply mobile app support.

The desktop bundle uses the PNG sizes and `icon.icns` listed in
`../tauri.conf.json`. The macOS installer also uses `icon.icns` for its helper app.
