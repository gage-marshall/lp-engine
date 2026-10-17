# LovePlus+ symbol map

Every function in LovePlus+ (Nintendo DS, Japanese release, game code BOXJ), named by the lp-engine project:
15,499 entries for the static code and all 44 overlays. Browse it at the
[function map](https://gage-marshall.github.io/lp-engine/symbols.html).

| file | what it is |
|---|---|
| `loveplus_plus_boxj.csv` | module, address, size, name, kind, confidence, note |
| `loveplus_plus_boxj.json` | the same, plus the direct calls between functions (what the site's explorer reads) |
| `ghidra_import.py` | a Ghidra script that applies the names (and optionally the notes) to your own program |

**Kind:** `game` (the game's own code), `sdk` (Nintendo's SDK), `nitrosystem` (NitroSystem), `library` (the
libraries' internal helpers).

**Confidence:**

| level | meaning |
|---|---|
| `exact` | matched byte for byte against the library the function comes from |
| `verified` | confirmed against the running game: the ported game flow (checked over 311,719 frames of play) or disassembly checked in the engine |
| `high` | well supported by what the function calls, reads and writes |
| `medium` | a reasonable reading; details may be off |
| `low` | marks the function's area; doesn't claim to know what it does |

Where a name was corrected, the note says what it was labelled before.

**Overlays share addresses.** Overlays are loaded into the same regions at different times, so an address is only
meaningful together with its module (`ov35 0216c800` and `ov28 0216c800` are different functions).

**Calls** in the JSON are direct calls (`BL` / `BLX`) found by a static scan of the code, kept only where they land
on a named function's first instruction. Calls through function pointers are not included.

**What's in here, and what isn't.** Addresses, sizes, and lp-engine's own names and notes. No code or data from the
game, the SDK or the libraries, and none of the databases used to match the library functions. You need your own
copy of the game to make use of it. `tools/symbols_export.py` regenerates these files.
