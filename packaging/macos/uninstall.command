#!/bin/bash
# Installed outside the app so it remains available after the app is trashed.
set -euo pipefail

if [[ "$(id -u)" != 0 ]]; then
    exec sudo /bin/bash "$0" "$@"
fi

hal=/Library/Audio/Plug-Ins/HAL
app=/Applications/OpenKrisp.app

# Give the application's normal shutdown time to stop routes and save settings.
for ((attempt=0; attempt<30; attempt++)); do
    if ! /usr/bin/pgrep -x openkrisp-desktop >/dev/null; then break; fi
    sleep 0.2
done
if /usr/bin/pgrep -x openkrisp-desktop >/dev/null; then
    echo 'Quit OpenKrisp before uninstalling.' >&2
    exit 1
fi
if [[ -e "$app" || -L "$app" ]]; then
    if [[ -L "$app" || -L "$app/Contents" || -L "$app/Contents/Info.plist" ]] ||
        [[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$app/Contents/Info.plist" 2>/dev/null || true)" != app.openkrisp.desktop ]]; then
        echo "Refusing to remove an unrecognized app: $app" >&2
        exit 1
    fi
fi

# Validate every existing target before deleting anything. Never follow a symlink
# or remove an unrelated driver that happens to occupy an OpenKrisp path.
for name in OpenKrispMicrophone OpenKrispSpeaker; do
    bundle="$hal/$name.driver"
    case "$name" in
        OpenKrispMicrophone) identifier=app.openkrisp.audio.microphone ;;
        OpenKrispSpeaker) identifier=app.openkrisp.audio.speaker ;;
    esac
    if [[ -e "$bundle" || -L "$bundle" ]]; then
        if [[ -L "$bundle" || -L "$bundle/Contents" || -L "$bundle/Contents/Info.plist" ]] ||
            [[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$bundle/Contents/Info.plist" 2>/dev/null || true)" != "$identifier" ]]; then
            echo "Refusing to remove an unrecognized driver: $bundle" >&2
            exit 1
        fi
    fi
done

for name in OpenKrispMicrophone OpenKrispSpeaker; do
    rm -rf -- "$hal/$name.driver"
done
rm -rf -- "$app"
for identifier in app.openkrisp.audio.microphone app.openkrisp.audio.speaker app.openkrisp.desktop; do
    if pkgutil --pkg-info "$identifier" >/dev/null 2>&1; then
        pkgutil --forget "$identifier" >/dev/null
    fi
done

echo 'OpenKrisp and its audio drivers removed.'
echo 'Restart your Mac to unload the drivers. Select your physical audio devices in calling apps.'
