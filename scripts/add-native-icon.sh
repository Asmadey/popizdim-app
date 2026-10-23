#!/bin/bash
# Add the macOS 26 native icon (Icon Composer bundle) to a built Popizdim.app.
#
# The Tauri bundler can compile src-tauri/icons/icon.icon itself, but on the
# macos-26 runner its actool call crashes ("attempt to insert nil object") while
# the identical command line run by hand succeeds. So compile the icon here,
# put Assets.car into the bundle, name it in Info.plist and sign again.
#
#   scripts/add-native-icon.sh path/to/Popizdim.app
#
# Needs Xcode 26+ (actool) selected with xcode-select.
set -euo pipefail

APP=${1:?usage: add-native-icon.sh path/to/App.app}
ROOT=$(cd "$(dirname "$0")/.." && pwd)
ICON_NAME=AppIcon

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
cp -R "$ROOT/src-tauri/icons/icon.icon" "$work/$ICON_NAME.icon"
mkdir "$work/out"

xcrun actool "$work/$ICON_NAME.icon" --compile "$work/out" \
  --output-format human-readable-text --notices --warnings --errors \
  --output-partial-info-plist "$work/out/partial.plist" \
  --app-icon "$ICON_NAME" --include-all-app-icons \
  --enable-on-demand-resources NO --development-region en \
  --target-device mac --minimum-deployment-target 26.0 \
  --platform macosx

test -f "$work/out/Assets.car"
cp "$work/out/Assets.car" "$APP/Contents/Resources/Assets.car"

plist="$APP/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Delete :CFBundleIconName" "$plist" 2>/dev/null || true
/usr/libexec/PlistBuddy -c "Add :CFBundleIconName string $ICON_NAME" "$plist"

# Changing the bundle invalidates the signature; sign it the way tauri.conf.json does.
codesign --force --deep --sign - --options runtime \
  --entitlements "$ROOT/src-tauri/Entitlements.plist" "$APP"
codesign --verify --deep --strict "$APP"
echo "native icon added to $APP"
