# Releasing the game app

A release contains only our own work: the program, the translation delta and the notices. Nothing from the game is
in it. On first run the app asks for the player's own ROM and reads it into the per-user data folder (macOS
`~/Library/Application Support/LovePlus+`, Windows `%APPDATA%\LovePlus+`, Linux `~/.config/loveplus`), then rebuilds
the translation from `translation-en.lpdelta` against those files.

## Steps

1. Rebuild the translation delta from the current patch tree, and check that it inserts next to nothing that also occurs
   in the original files (its last line):

       python3 tools/lp_delta.py build content patch dist/translation-en.lpdelta

2. Package on each platform (each script builds the release program, writes `THIRD_PARTY_NOTICES.txt` for that
   target with `tools/package/notices.py`, and puts the result in `dist/`):

   | platform | command | result |
   |---|---|---|
   | macOS | `tools/package/macos.sh` (`UNIVERSAL=1` for Apple silicon + Intel) | `LovePlus+.app`, `LovePlus+-VERSION-macos.dmg` |
   | Windows | `powershell tools\package\windows.ps1` | `LovePlus+-VERSION-windows-x64.zip` |
   | Linux | `tools/package/linux.sh` (needs ALSA, udev and GTK 3 dev packages) | `LovePlus+-VERSION-linux-ARCH.tar.gz` |

3. Try the result on a clean account: the first run asks for the ROM, the title screen comes up in English, the
   Game / View / Sound / Controls / Help menus work, and a save survives a restart.

## What needs a person

- **Signing.** macOS: `SIGN_IDENTITY="Developer ID Application: NAME (TEAMID)"` signs with the hardened runtime and the
  microphone entitlement; with `NOTARY_PROFILE` too (from `xcrun notarytool store-credentials`) the .dmg is notarized
  and stapled. Without it the app is signed ad hoc and other Macs need right-click > Open. Windows: `SIGN_CERT` (.pfx)
  and `SIGN_PASSWORD` sign with signtool; unsigned builds get a SmartScreen warning.
- **The bundle identifier** (macOS, `BUNDLE_ID`; the default `io.github.lp-engine.loveplus` is a placeholder).
- **An icon**: `tools/package/icon.icns` is used when present.
- **The engine's own licence**: there is no LICENSE file yet.

## Notices

The Siren14 attribution is in the notices header and in Help > About ("Siren14™ / G.722.1C technology licensed from
Polycom®, Inc."). Check this wording against the licence text you have from Polycom. The Rust crates the program links
are all under permissive licences (MIT, Apache-2.0, Zlib, ISC, BSD, CC0, Unlicense); `notices.py` includes each crate's
licence and notice files.

## Player-facing behaviour worth knowing

- The microphone opens only when the game first listens (macOS asks for permission then); Sound > Use the Microphone
  turns it off.
- Minimizing the window, or Game > Close the Lid, closes the DS lid: the game sleeps as on the hardware.
- Wireless features (talk mode, card exchange, pass-by, save send/receive and transfer, regional capsules) need a
  second DS: choosing one shows a notice and returns to the menu.
- Game > Clock shifts the DS clock from the computer's. Like changing a DS's clock, the game notices.
- Each session is recorded under `recordings/` and deleted on a clean exit unless Game > Keep Session Recordings is on.
  It is kept when the game's code stops, so the problem can be replayed exactly (`game_probe CONTENT N --replay DIR`).
