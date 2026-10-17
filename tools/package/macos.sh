#!/bin/sh
# A macOS release of the game app: dist/LovePlus+.app (the program, the translation, the notices) and
# dist/LovePlus+-VERSION-macos.dmg. Nothing from the game is in it: the app reads the player's ROM on first run.
#
#   tools/package/macos.sh                       build for this Mac's architecture
#   UNIVERSAL=1 tools/package/macos.sh           Apple silicon + Intel in one program (both Rust targets installed)
#
# Signing: with SIGN_IDENTITY set ("Developer ID Application: NAME (TEAMID)") the app is signed with the hardened
# runtime and the microphone entitlement; without it, ad hoc (runs on this Mac; other Macs need right-click > Open).
# Notarizing: with NOTARY_PROFILE set too (a `xcrun notarytool store-credentials` profile) the .dmg is notarized and
# stapled. BUNDLE_ID overrides the bundle identifier.
set -eu
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
DIST="$ROOT/dist"
NAME="LovePlus+"
BUNDLE_ID=${BUNDLE_ID:-io.github.lp-engine.loveplus}
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/engine/Cargo.toml" | head -1)
DELTA="$DIST/translation-en.lpdelta"
[ -f "$DELTA" ] || { echo "no $DELTA: python3 tools/lp_delta.py build content patch dist/translation-en.lpdelta" >&2; exit 1; }

cd "$ROOT/engine"
if [ "${UNIVERSAL:-}" = 1 ]; then
    cargo build --release --bin game --target aarch64-apple-darwin
    cargo build --release --bin game --target x86_64-apple-darwin
    BIN="$ROOT/engine/target/game-universal"
    lipo -create -output "$BIN" target/aarch64-apple-darwin/release/game target/x86_64-apple-darwin/release/game
    TARGET=universal-apple-darwin
else
    cargo build --release --bin game
    BIN="$ROOT/engine/target/release/game"
    TARGET=$(rustc -vV | sed -n 's/^host: //p')
fi

APP="$DIST/$NAME.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/$NAME"
cp "$DELTA" "$APP/Contents/Resources/"
python3 "$ROOT/tools/package/notices.py" "$APP/Contents/Resources/THIRD_PARTY_NOTICES.txt" $( [ "$TARGET" = universal-apple-darwin ] || echo --target "$TARGET" )
[ -f "$ROOT/tools/package/icon.icns" ] && cp "$ROOT/tools/package/icon.icns" "$APP/Contents/Resources/AppIcon.icns"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>$NAME</string>
    <key>CFBundleDisplayName</key><string>$NAME</string>
    <key>CFBundleIdentifier</key><string>$BUNDLE_ID</string>
    <key>CFBundleExecutable</key><string>$NAME</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>$VERSION</string>
    <key>CFBundleVersion</key><string>$VERSION</string>
    <key>CFBundleIconFile</key><string>AppIcon</string>
    <key>LSMinimumSystemVersion</key><string>11.0</string>
    <key>LSApplicationCategoryType</key><string>public.app-category.simulation-games</string>
    <key>NSHighResolutionCapable</key><true/>
    <key>NSMicrophoneUsageDescription</key><string>Some moments in LovePlus+ listen through the Nintendo DS microphone; the game hears your Mac's microphone in its place.</string>
</dict>
</plist>
PLIST

ENT="$ROOT/engine/target/entitlements.plist"
cat > "$ENT" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>com.apple.security.device.audio-input</key><true/>
</dict>
</plist>
PLIST
if [ -n "${SIGN_IDENTITY:-}" ]; then
    codesign --force --timestamp --options runtime --entitlements "$ENT" --sign "$SIGN_IDENTITY" "$APP"
else
    echo "no SIGN_IDENTITY: signing ad hoc"
    codesign --force --entitlements "$ENT" --sign - "$APP"
fi
codesign --verify --strict "$APP"

DMG="$DIST/$NAME-$VERSION-macos.dmg"
STAGE="$ROOT/engine/target/dmg"
rm -rf "$STAGE" "$DMG"
mkdir -p "$STAGE"
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
hdiutil create -quiet -volname "$NAME" -srcfolder "$STAGE" -format UDZO "$DMG"
if [ -n "${SIGN_IDENTITY:-}" ]; then
    codesign --force --timestamp --sign "$SIGN_IDENTITY" "$DMG"
    if [ -n "${NOTARY_PROFILE:-}" ]; then
        xcrun notarytool submit "$DMG" --keychain-profile "$NOTARY_PROFILE" --wait
        xcrun stapler staple "$DMG"
    fi
fi
echo "$APP"
echo "$DMG"
