# Releasing the game app

A release contains only our own work (MIT, `LICENSE`): the program, the translation delta, the notices and the icon.
Nothing from the game is in it. On first run the app asks for the player's own ROM and reads it into the per-user data
folder (macOS `~/Library/Application Support/LovePlus+`, Windows `%APPDATA%\LovePlus+`, Linux `~/.config/loveplus`),
then rebuilds the translation from `translation-en.lpdelta` against those files.

## Steps (all from this Mac)

1. Rebuild the translation delta from the current patch tree, and check that it inserts next to nothing that also occurs
   in the original files (its last line):

       python3 tools/lp_delta.py build content patch dist/translation-en.lpdelta

2. Package. Each script builds the release program, writes `THIRD_PARTY_NOTICES.txt` for its target
   (`tools/package/notices.py`) and puts the result in `dist/`. Run them one after another: they share
   `dist/THIRD_PARTY_NOTICES.txt`.

   | platform | command | result |
   |---|---|---|
   | macOS | `UNIVERSAL=1 SIGN_IDENTITY="Developer ID Application: …" NOTARY_PROFILE=… tools/package/macos.sh` | `LovePlus+.app` (signed, notarized, stapled; Apple silicon + Intel) and `LovePlus+-VERSION-macos.zip` holding it |
   | Windows | `AZURE_SIGNING_ACCOUNT=… AZURE_SIGNING_PROFILE=… tools/package/windows.sh` | `LovePlus+-VERSION-windows-x64.exe`, portable (translation and notices built in) |
   | Linux | `tools/package/linux.sh` (Docker; `ARCH=arm64` for ARM) | `loveplus_VERSION_amd64.deb`, `LovePlus+-VERSION-x86_64.AppImage` (translation and notices built in) |

   Tools: Xcode (codesign, notarytool), `rustup target add x86_64-apple-darwin x86_64-pc-windows-gnu`,
   `brew install mingw-w64 jsign`, Docker, the Azure CLI signed in.

3. Try each on a clean account: the first run asks for the ROM, the title screen comes up in English, the
   Game / View / Sound / Controls / Help menus work, and a save survives a restart.

## Signing

- macOS: Developer ID with the hardened runtime and the microphone entitlement. The notary profile is the one stored
  with `xcrun notarytool store-credentials`. Bundle identifier `io.github.lp-engine.loveplus` (`BUNDLE_ID` overrides).
- Windows: Azure Artifact Signing through jsign with a token from `az account get-access-token`. The signed-in
  identity needs the "Artifact Signing Certificate Profile Signer" role on the account.
- Linux packages are not signed.

## Notices

The Siren14 attribution is in the notices header and in Help > About ("Siren14™ / G.722.1C technology licensed from
Polycom®, Inc."). The Rust crates the program links are all under permissive licences (MIT, Apache-2.0, Zlib, ISC, BSD,
CC0, Unlicense). `notices.py` includes each crate's licence and notice files. Help > Third-Party Notices opens the file.

## The icon

`tools/package/icon/make_icon.py` draws it from shapes (a rose tile, a white heart, a plus badge). It writes
`icon_1024.png`, `icon_256.png` (the window icon, built into the program), `icon.ico` (Windows resources via `build.rs`)
and the iconset that `iconutil` turns into `tools/package/icon.icns`.

## Player-facing behaviour worth knowing

- The microphone opens only when the game first listens (macOS asks for permission then). Sound > Use the Microphone
  turns it off.
- Minimizing the window, or Game > Close the Lid, closes the DS lid: the game sleeps as on the hardware.
- Wireless features (talk mode, card exchange, pass-by, save send/receive and transfer, regional capsules) need a
  second DS: choosing one shows a notice and returns to the menu.
- Game > Clock shifts the DS clock from the computer's. Like changing a DS's clock, the game notices.
- Each session is recorded under `recordings/` and deleted on a clean exit unless Game > Keep Session Recordings is on.
  It is kept when the game's code stops, so the problem can be replayed exactly (`game_probe CONTENT N --replay DIR`).
