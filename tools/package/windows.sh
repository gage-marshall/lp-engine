#!/bin/sh
# A Windows release of the game app, cross-built on macOS or Linux: dist/LovePlus+-VERSION-windows-x64.exe, one portable
# program carrying the translation and the notices (feature `embedded`) and its icon. Nothing from the game is in it:
# the app reads the player's ROM on first run. Needs the x86_64-pc-windows-gnu Rust target and mingw-w64.
#
# Signing (Azure Artifact Signing through jsign, signed in with `az login`): AZURE_SIGNING_ACCOUNT and
# AZURE_SIGNING_PROFILE name the account and certificate profile, AZURE_SIGNING_ENDPOINT its region's endpoint
# (default eus.codesigning.azure.net). The signed-in identity needs the "Artifact Signing Certificate Profile Signer"
# role on the account. Without them the program is left unsigned (SmartScreen warns on first run).
set -eu
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
DIST="$ROOT/dist"
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/engine/Cargo.toml" | head -1)
TARGET=x86_64-pc-windows-gnu
[ -f "$DIST/translation-en.lpdelta" ] || { echo "no dist/translation-en.lpdelta: python3 tools/lp_delta.py build content patch dist/translation-en.lpdelta" >&2; exit 1; }
python3 "$ROOT/tools/package/notices.py" "$DIST/THIRD_PARTY_NOTICES.txt" --target "$TARGET"
(cd "$ROOT/engine" && cargo build --release --bin game --features embedded --target "$TARGET")
OUT="$DIST/LovePlus+-$VERSION-windows-x64.exe"
cp "$ROOT/engine/target/$TARGET/release/game.exe" "$OUT"
x86_64-w64-mingw32-strip "$OUT"
if [ -n "${AZURE_SIGNING_ACCOUNT:-}" ] && [ -n "${AZURE_SIGNING_PROFILE:-}" ]; then
    TOKEN=$(az account get-access-token --resource https://codesigning.azure.net --query accessToken -o tsv)
    jsign --storetype TRUSTEDSIGNING --keystore "${AZURE_SIGNING_ENDPOINT:-eus.codesigning.azure.net}" --storepass "$TOKEN" \
        --alias "$AZURE_SIGNING_ACCOUNT/$AZURE_SIGNING_PROFILE" --tsaurl http://timestamp.acs.microsoft.com/ --tsmode RFC3161 \
        --name "LovePlus+" "$OUT"
else
    echo "no AZURE_SIGNING_ACCOUNT / AZURE_SIGNING_PROFILE: left unsigned"
fi
echo "$OUT"
