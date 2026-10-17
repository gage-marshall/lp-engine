#!/bin/sh
# Inside the Linux build container (linux.sh): the program, then the .deb and the AppImage.
set -eu
ROOT=/src
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/engine/Cargo.toml" | head -1)
TARGET=$(rustc -vV | sed -n 's/^host: //p')
export CARGO_TARGET_DIR="$ROOT/engine/target/linux-$ARCH"
python3 "$ROOT/tools/package/notices.py" "$ROOT/dist/THIRD_PARTY_NOTICES.txt" --target "$TARGET"
(cd "$ROOT/engine" && cargo build --release --bin game --features embedded)
BIN="$CARGO_TARGET_DIR/release/game"
strip "$BIN"
WORK="$CARGO_TARGET_DIR/pkg"
rm -rf "$WORK"
DESKTOP="[Desktop Entry]
Type=Application
Name=LovePlus+
Comment=LovePlus+ from your own game file, in English
Exec=loveplus
Icon=loveplus
Categories=Game;Simulation;
Terminal=false"

# ---- .deb ----
DEB="$WORK/deb"
mkdir -p "$DEB/DEBIAN" "$DEB/usr/bin" "$DEB/usr/share/applications" "$DEB/usr/share/icons/hicolor/256x256/apps" \
    "$DEB/usr/share/icons/hicolor/1024x1024/apps" "$DEB/usr/share/doc/loveplus"
cp "$BIN" "$DEB/usr/bin/loveplus"
echo "$DESKTOP" > "$DEB/usr/share/applications/loveplus.desktop"
cp "$ROOT/tools/package/icon/icon_256.png" "$DEB/usr/share/icons/hicolor/256x256/apps/loveplus.png"
cp "$ROOT/tools/package/icon/icon_1024.png" "$DEB/usr/share/icons/hicolor/1024x1024/apps/loveplus.png"
{ echo "LovePlus+ engine"; echo; cat "$ROOT/LICENSE"; echo; echo; cat "$ROOT/dist/THIRD_PARTY_NOTICES.txt"; } > "$DEB/usr/share/doc/loveplus/copyright"
SIZE=$(du -sk "$DEB/usr" | cut -f1)
cat > "$DEB/DEBIAN/control" <<CONTROL
Package: loveplus
Version: $VERSION
Section: games
Priority: optional
Architecture: $ARCH
Installed-Size: $SIZE
Depends: libc6 (>= 2.35), libgtk-3-0, libasound2, libudev1, libxkbcommon0, libvulkan1 | libgl1
Maintainer: gage-marshall
Description: LovePlus+ on a native engine, in English
 Runs LovePlus+ (Konami, 2010) from the player's own game file, with an English
 translation. Nothing from the game is included: on first run it asks for the
 player's ROM. Not affiliated with or endorsed by Konami.
CONTROL
dpkg-deb --root-owner-group --build "$DEB" "$ROOT/dist/loveplus_${VERSION}_$ARCH.deb"

# ---- AppImage ----
case "$ARCH" in amd64) AARCH=x86_64 ;; arm64) AARCH=aarch64 ;; *) AARCH=$ARCH ;; esac
APPDIR="$WORK/LovePlus.AppDir"
mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/share/applications" "$APPDIR/usr/share/icons/hicolor/256x256/apps"
cp "$BIN" "$APPDIR/usr/bin/loveplus"
echo "$DESKTOP" > "$APPDIR/loveplus.desktop"
cp "$APPDIR/loveplus.desktop" "$APPDIR/usr/share/applications/"
cp "$ROOT/tools/package/icon/icon_256.png" "$APPDIR/loveplus.png"
cp "$ROOT/tools/package/icon/icon_256.png" "$APPDIR/usr/share/icons/hicolor/256x256/apps/loveplus.png"
cat > "$APPDIR/AppRun" <<'APPRUN'
#!/bin/sh
HERE=$(dirname "$(readlink -f "$0")")
exec "$HERE/usr/bin/loveplus" "$@"
APPRUN
chmod +x "$APPDIR/AppRun"
TOOL="$WORK/appimagetool"
curl -sSfL -o "$TOOL" "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-$AARCH.AppImage"
chmod +x "$TOOL"
# the AppImage magic at ELF offset 8 stops emulated runs (Docker on Apple silicon): blank it in the tool's copy
printf '\000\000\000' | dd of="$TOOL" bs=1 seek=8 count=3 conv=notrunc 2>/dev/null
ARCH=$AARCH "$TOOL" --appimage-extract-and-run "$APPDIR" "$ROOT/dist/LovePlus+-$VERSION-$AARCH.AppImage"
ls -la "$ROOT/dist"
