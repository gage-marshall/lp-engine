# Handoff: emulator ground-truth work (state as of 2026-10-12)

## Standing rules (unchanged)
- Nintendo SDK docs/headers/sources under ~/Downloads/Official Nintendo SDKs 2010/... are confidential read-only references: never copy into repo or content/.
- No ROM-derived bytes in the repo. `tools/emu_scripts/states/` (save states, screenshots) is git-ignored and user-local.
- Final product is Rust; Python/C here are prototypes/oracles. Engine runs from `content/` (extracted from the user's ROM by `tools/lp_extract.py`) overlaid by optional `patch/`.
- Verify renders visually, not only numerically. Avoid unnecessary agents (max 3-4 reviewers at once). The user is usage-limit conscious: ask before heavy runs.

## Harness
- `tools/emu_driver.py`: `Emu(rom, state=None)`: `step(n)`, `press(*keys,hold,release)`, `touch(x,y,hold,release)`, `drag(pts)`, `image()`, `shot(path, rotate, scale)`, `save/load`, `read32`; `page_to_touch(rx,ry)` converts reading-orientation page coords to raw touch coords (console is held sideways).
- `tools/emu_script_oracle.py content --state STATE.dst --advances N`: loads one disposable state, records the live `ScriptContext` reference/PC/command and decoded dialogue buffers, then optionally taps through dialogue. Add `--compare-event EVENT_ID` to verify its observed rendered text is an exact ordered subsequence of `event_trace.py`; this does not claim native-command parity. Its JSON is observation-only; the ROM and state remain untracked.
- `tools/emu_scripts/` (run from repo root with the repo venv/py-desmume, ROM at `rom/loveplus_plus_j.nds`; always `< /dev/null`):
  - `nav.py` helpers (`D`, `S`, `sheet`); `S` = state dir (env `EMU_STATES`, default `tools/emu_scripts/states`).
  - `mk_states.py` rebuilds title/menu/slots/intro states from a fresh boot. `story3.py STATE N FRAMES OUT` taps through dialogue and auto-picks choice 1, saves `OUT.dst` + contact sheet `OUT.png`.
  - `choice3.py` choice-menu input demo; `h1.py` reads frame-divider bytes + full screenshot; `h2.py` per-frame screen-change log (update rate); `m_div.py STATE` divider phase sequence; `m_type/m_adv/m_ff.py` typewriter, advance, fast-forward probes; `geom2.py` window geometry; `keys.py`, `stroke.py` input experiments.
- Key states (rebuilt by the scripts if missing): `st1` first choice menu, `st3` after it, `st4` school gate, `st5` first Nene dialogue (break room, 3D heroine).

## py-desmume gotchas
One Emu per process (global core). Use `cycle(False)` (joystick poll hangs), `volume_set(0)`. Save states load reliably only in a fresh process and only if saved from a fresh-run chain (states saved after loading a state may fail: regenerate). Display buffer is RGBX with unusable alpha: convert to RGB. No `timeout` on macOS. Prints appear before savestate chatter: `grep` for them rather than `tail`.

## Input recipes (raw bottom-screen touch coordinates)
Title (128,96); menu ゲームスタート (183,90) twice; slot DATA01 (130,98) then held (127,98); clock: time buttons (15,115) DS-sync, 決定 (15,28), OK (80,92); profile: 決定 (15,158), handwriting stroke along page ry=79 rx 25..115, candidate (91,20), month (213,67), 決定 (15,156), day (213,46), blood A (205,89), hometown 北海道 (230,34), kana あ (237,17), candidate (237,33), OK (80,94); final はい (80,53). Dialogue advance: tap (30,100). Choices: tap badge (choice 1 (180,16), 2 ~(115,16), 3 ~(51,16); only 1 verified) to highlight, then tap again at another x inside the box (165,10) to confirm. Keys do nothing.

## Verified facts
- `st5` executes event `0x08006603` (slot 3, script 1, entry 3). A 50-tap oracle pass matched all 18 remaining displayed trace lines through its return: 17 literal matches and one explicit runtime-variable substitution. This validates that event's traced dialogue/control flow, not general native-command equivalence.
- Typewriter 8 frames/char, 60 updates/s in 2D scenes; tap completes text, next tap advances; LC12 layout pixel-exact (advance 12, pitch 16, no indent, origin x=8; rows 203/219).
- 3D heroine dialogue: 4-vblank cycle (phase bytes @0x213d2e4+0xa0/a1/a3: (1,0,1)->(2,0,2)->(2,1,2)->(1,0,0)); touch-screen image changes every 4th frame (15 Hz) while idle; 2D page every 3rd frame. Caveat: idle only, check during lip-sync/motion.
- Scene tint in the break room is blue (toon/light time-of-day or room light): compare against toonrender.

## Open items (priority order)
1. Toon accuracy: take `st5` frame (the 3D screen is the touch screen), render the same heroine/pose/camera/time of day with `tools/toonrender.py`, compare visually and numerically (edge colour, toon table, blue tint source).
2. Update rate during motion/lip-sync and in date/3D-walk scenes (confirm 15 Hz is the real animation step or just idle).
3. `eye00` track meaning (watch eye texture index vs. motion over frames).
4. Message advance details: AUTO button behavior, fast-forward (hold?), name-tag/box absolute positions on screen; verify choice 2/3 badge positions.
5. Event selection (largest reimplementation gap): log memory (script cmd/flag reads) while driving scenes; use as test oracle for the Rust port.
6. Port test-oracle harness: frame/text/state diffs between the Rust engine and the emulator at fixed save states.
7. Longer: date scenes, music/sound parity checks, other heroines (Rinko, Manaka) via story branching.

Related: `docs/CONTENT_LAYOUT.md` (section "Emulator ground truth"), memory note `loveplus-emulator`.
