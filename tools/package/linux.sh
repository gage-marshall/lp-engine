#!/bin/sh
# A Linux release of the game app: dist/LovePlus+-VERSION-linux-ARCH.tar.gz with the program, the translation, the
# notices and a .desktop entry. Nothing from the game is in it: the app reads the player's ROM on first run.
# Building needs the ALSA, udev and GTK 3 development packages (cpal, gilrs, rfd's dialogs).
set -eu
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
DIST="$ROOT/dist"
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/engine/Cargo.toml" | head -1)
DELTA="$DIST/translation-en.lpdelta"
[ -f "$DELTA" ] || { echo "no $DELTA: python3 tools/lp_delta.py build content patch dist/translation-en.lpdelta" >&2; exit 1; }
(cd "$ROOT/engine" && cargo build --release --bin game)
TARGET=$(rustc -vV | sed -n 's/^host: //p')
ARCH=$(uname -m)
DIR="$ROOT/engine/target/LovePlus+-$VERSION"
rm -rf "$DIR"
mkdir -p "$DIR"
cp "$ROOT/engine/target/release/game" "$DIR/loveplus"
cp "$DELTA" "$DIR/"
python3 "$ROOT/tools/package/notices.py" "$DIR/THIRD_PARTY_NOTICES.txt" --target "$TARGET"
cat > "$DIR/loveplus.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=LovePlus+
Comment=LovePlus+ from your own game file, in English
Exec=loveplus
Categories=Game;Simulation;
DESKTOP
OUT="$DIST/LovePlus+-$VERSION-linux-$ARCH.tar.gz"
tar -C "$(dirname "$DIR")" -czf "$OUT" "$(basename "$DIR")"
echo "$OUT"
