# Function labeling

Every function in the ARM9 static module (6,715) and in all 44 overlays (8,701) has a non-default name.
`fid/labels_all.csv` is the authoritative table (module, addr, name, tier, source, confidence, note).

## Tiers
| Tier | Meaning | Where it comes from |
|---|---|---|
| A | Semantic name | SDK 5.4 and NitroSystem 2.2 byte-matching (`tools/sdk_match.py`), KH Days donor matches, relocation-derived names, and names written by reading decompiled code (`fid/manual_labels.csv`, with confidence and notes) |
| B | Structural role | Decompiled shape: `Get`/`Set` (with global and field offset), `Thunk`, `Wrap_<callee>`, `Clear`, `Switch`, `Const<value>` |
| C | Generic role | `Proc`/`Seq`/`Calc`/`Loop`, with a flavor derived from callee subsystems (`Gfx`, `Text`, `Load`, `Time`, ...) |

Tier B/C names are `<Subsystem>_<Role>_<address>`. The address suffix keeps every name unique and lets tools regenerate/overwrite them.
Subsystem comes from, in order: library address extents (`OS`, `GX`, `NNSG2d`, `Rtl`, ...), module-private globals, direct data-table ownership,
exclusive callers / private helpers, same-subsystem sandwiching, data/call clustering (`Mod<addr>`), and finally the coarse segment (`Seg<addr>`).
Overlay functions are `OvNN_<Role>_<address>`; the dictionary engine (Thumb range of overlay 6) is `Dlt_...`.

A semantic subsystem name (`Sprite`, `GameClock`, ...) is only given on direct evidence; weaker evidence yields a neutral `Mod`/`Seg` name.

## Regenerating
1. `fid/scripts/ExtractFeatures.java` (static: `fid/features/static.tsv`; overlays via `fid/extract_ov_features.sh`)
2. `python3 tools/autolabel.py` -> `fid/auto/*.txt` (+ tier tables)
3. `fid/apply_all_labels.sh` applies manual, static-auto and overlay-auto labels to `fid/rom_proj` and `fid/ov_proj`.

## Known limits
* Tier A is roughly 11% of functions; most of the rest are structural/generic names.
* Subsystem prefixes on tier B/C names are inferences, not facts. Neutral `Mod`/`Seg` prefixes are only address/cluster groupings.
* Some Ghidra "functions" in data-heavy areas may not be real code (overlays 17/18 are mostly Thumb code; remaining non-code is possible).

## Semantic naming (module and overlay level)
Names are added by evidence, highest call-weight first:
* `fid/module_names.csv` maps a neutral static module (`Mod<addr>`/`Seg<addr>`) to a semantic subsystem (`TextRes`, `DotFont`, `Main`, `SaveSlot`, `MsgBox`, `DateUi`, ...).
  Evidence is recorded per row: string literals referenced by the module (file names such as `arc/date_ui.bin`, `dot/word/*.NCGR`, `mainLoop()`), semantic callees, overlay callers and owned data tables.
  Tool: `python3 tools/module_dossier.py MODULE...`.
* `fid/overlay_names.csv` names overlays (scenes) from the file names embedded in them (`skinship/*`, `minigame/pzldm_*`, `/mboot/*`, `Loveplusbutton_*.gtd`, ...).
  Tool: `python3 tools/overlay_dossier.py [ids]`. Overlay functions are named `OvNN<Scene>_<Role>_<addr>`.
* `fid/manual_labels.csv` holds individual function/global names (tier A) with confidence and notes.
* `python3 tools/export_labels.py` regenerates `fid/labels_all.csv` and `fid/semantic_map.csv`.

Method for further naming: `python3 tools/rank_funcs.py` (value ranking), `tools/module_dossier.py` (module evidence), `tools/decomp_ranges.sh` (decompile address ranges); then add rows to the CSVs and run `tools/autolabel.py` + `fid/apply_all_labels.sh`.
