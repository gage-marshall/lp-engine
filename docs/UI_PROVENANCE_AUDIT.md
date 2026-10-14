# UI provenance audit

Every constant the Python screen logic (`sandbox/ui/lpui/logic`, `lpui/textlayout.py`) uses, with where the game keeps it.
Regenerate and re-check: `.venv/bin/python sandbox/ui/audit_constants.py --md docs/UI_PROVENANCE_AUDIT.md`.

Kinds:
- **table**: a data table in the game image (`content/engine/`). The modules *load* these at runtime through
  `lpui/gamedata.py` (no copies in source); the audit re-reads the table and compares it with the module's values.
- **code**: values the game itself hard-codes as instruction immediates or literal-pool words; the audit finds each
  value in the named function's disassembly.
- **file**: the game's text screen layouts (`menu/mount_layout.bin`: calendar, profile, sound, display), parsed by
  `lpui/layoutfile.py` (sprite slots in file order, `x,y,cell,visible` records, then touch rectangles).
- **runtime**: decided by game simulation, supplied to the UI as inputs (schedule activities per period).

Findings while auditing:
- Text box geometry is derived, not fixed: `TextLayout_BuildLine` parameter tables (screen, bg, chars, lines, spacing),
  sizes from the font's default glyph width, tiles allocated per screen from 0 or 0x1180 / 0x1e00 while the 3D capture
  is on. All 13 boxes match RAM in all 208 captures.
- The room menu's buttons come from game state (`UpdateMainSceneEventState`), its slot rectangles from overlay 35
  0x21732c0. The settings submenu's kinds 7 / 8 are immediates.
- The profile grid pages are built by `ClearProfileEditSelection`; the byte table 0x2184058 is shared by sub-page counts
  and prefecture page sizes.
- Not covered here: the screen descriptions the renderer draws (which files each screen loads, where its sprites start)
  are still taken from the recorded session; the setup routines that produce them are the next thing to port.

| module | value | kind | source | result |
|---|---|---|---|---|
| timesetting | EDIT_TARGETS | table | ov42 0x216c6e0 6i x12 | ok |
| timesetting | OK_TARGET | table | ov42 0x216c6c8 6i x1 | ok |
| timesetting | FIELDS (columns, centres, sprite slots) | code | ov42 Ov42TimeSetting_RenderEditedDateTime 0x216bb98+0x134 | ok |
| timesetting | digit sequence = (c - '0') + 3, 16 px pitch | code | ov42 Ov42TimeSetting_RenderTimeDigits 0x216bae0+0xb0 | ok |
| timesetting | year wrap 99 | code | ov42 Ov42TimeSetting_IncrementEditedYear 0x216c498+0x30 | ok |
| timesetting | month wrap 12 | code | ov42 Ov42TimeSetting_IncrementEditedMonth 0x216c504+0x3c | ok |
| timesetting | hour 23 / minute 59 wrap, repeat 60 frames | code | ov42 Ov42TimeSetting_RunTimeSettingStateMachine 0x216aa2c+0x74c | ok |
| fileselect | POS (disc x) | table | ov27 0x216ec80 i x3 via 0x216bedc  | ok |
| fileselect | HITS | table | ov27 0x216ec8c 6i x6 via 0x216c41c  | ok |
| fileselect | SPEED_LEFT | table | ov27 0x216ec04 i x3 via 0x216c9f4  | ok |
| fileselect | SPEED_RIGHT | table | ov27 0x216ec10 i x3 via 0x216cebc  | ok |
| fileselect | arc: centre 0x80, divisor 0x9000, top 0x60 | code | ov27 Ov27FileSelect_MapSlotXToArcY 0x216bf70+0x34 | ok |
| titlemenu | LAYOUT[3] (at (n-3)*0x54) | table | ov21 0x21705c4 6h x3 | ok |
| titlemenu | LAYOUT[4] (at (n-3)*0x54) | table | ov21 0x2170618 6h x4 | ok |
| titlemenu | LAYOUT[5] (at (n-3)*0x54) | table | ov21 0x217066c 6h x5 | ok |
| titlemenu | LAYOUT[6] (at (n-3)*0x54) | table | ov21 0x21706c0 6h x6 | ok |
| titlemenu | page offsets | code | ov21 Ov21TitleMenu_GetTitleMenuVariantIndex 0x216d630+0x84 | ok |
| titlemenu | page transitions | code | ov21 Ov21TitleMenu_UpdateTitleMenuGfxTransition 0x216d0f8+0x4bc | ok |
| msgwin | SPEED | table | static 0x21274e0 i x5 | ok |
| msgwin | LAYOUT3 (pen x, map left) | table | static 0x212750c 2i x3 | ok |
| msgwin | LAYOUT2 | table | static 0x21274f4 2i x3 | ok |
| msgwin | line width 13 cells | code | static TextLayout_BuildGlyphs 0x208afa8+0x148 | ok |
| profile | DAKUTEN_MAP from | table | ov06 0x2184224 i x9 | ok |
| profile | DAKUTEN_MAP to | table | ov06 0x2184248 i x9 | ok |
| profile | HANDAKUTEN_MAP from | table | ov06 0x218426c i x3 | ok |
| profile | HANDAKUTEN_MAP to | table | ov06 0x2184278 i x3 | ok |
| profile | PAGE_COUNTS (sub-pages / prefecture entries) | table | ov06 0x2184058 B x52 | ok |
| profile | summary columns / y | code | ov06 Ov06ProfileDlt_DrawProfileSelectionValues 0x21824dc+0x208 | ok |
| profile | value pages geometry | code | ov06 Ov06ProfileDlt_DrawProfileEditDialog 0x217ee74+0x5b4 | ok |
| profile | voicing marks 0x32 / 0x33 | code | ov06 Ov06ProfileDlt_BuildProfileEditMenu 0x2181364+0x280 | ok |
| profile | grid builder (10/9 rows, 5/6 columns, marks 0x32/0x33) | code | ov06 Ov06ProfileDlt_ClearProfileEditSelection 0x217f4d4+0x100 | ok |
| schedule | MAIN targets | table | ov03 0x215ea80 6i x14 | ok |
| schedule | COLUMN_X | table | ov03 0x215e880 i x4 via 0x215cf38  | ok |
| schedule | LAYOUT_Y | table | ov03 0x215e6f0 10i x10 via 0x215cf34  | ok |
| schedule | LAYOUT_ID | table | ov03 0x215e890 10i x10 via 0x215cf3c  | ok |
| schedule | activities per period | runtime | game simulation (EventLogic_UpdateTimeData 0x20ccf34 / 0x20cd334): out of UI scope | open: game simulation (EventLogic_UpdateTimeData 0x20ccf34 / 0x20cd334): out of UI scope |
| confirmdialog | OK target | table | static 0x210b690 6i x1 via 0x20be650  | ok |
| confirmdialog | page arrows (next, prev) | table | static 0x210b7a4 6i x2 via 0x20bf594  | ok |
| confirmdialog | YES_NO targets | table | static 0x210b774 6i x2 via 0x20bee24 MsgBox_HandleInput(2); values the trace recorded | ok |
| hud | PERIOD_BOUNDS | table | static 0x2101d48 B x5 | ok |
| hud | NIGHT_START | table | static 0x2101d4d B x1 | ok |
| hud | time slot sequence base 0x28 | code | static DateUi_AdvanceTimeSequence 0x2080d70+0x24 | ok |
| roommenu | SEQ | code | ov35 Ov35MainScene_SwitchMainSceneMenu 0x216cae8+0x324 | ok |
| roommenu | SEQ_ALT_A | code | ov35 Ov35MainScene_SwitchMainSceneSubmenuA 0x216ce10+0xa0 | ok |
| roommenu | SEQ_ALT_B | code | ov35 Ov35MainScene_SwitchMainSceneSubmenuB 0x216ceb4+0xa0 | ok |
| roommenu | ACTION_REMAP | code | ov35 Ov35MainScene_SwitchMainSceneSubmenuC (with its jump-table cases) 0x216cf58+0x1f8 | ok |
| roommenu | slot table (x, y, half w, half h, id, enabled) | table | ov35 0x21732c0 6i x13 | ok |
| roommenu | menu kinds by game state | code | ov35 Ov35MainScene_UpdateMainSceneEventState 0x216b9a4+0x404 | ok |
| roommenu | settings submenu kinds 7, 8 | code | ov35 Ov35MainScene_LoadMainSceneDateResources 0x21708ac+0xf4 | ok |
| roommenu | slide offset 0xa0, slot stride 0x18 | code | ov35 Ov35MainScene_UpdateMainSceneMenuSprites 0x216c220+0x40 | ok |
| soundsettings | KNOB_X (bar 1) | code | static MicUi_DrawPrimarySprite 0x20c0efc+0x8c | ok |
| soundsettings | KNOB_X (bar 2) | code | static MicUi_DrawSecondarySprite 0x20c0f8c+0x6c | ok |
| soundsettings | KNOB_X (bar 3) | code | static MicUi_DrawTertiarySprite 0x20c0ff8+0x6c | ok |
| soundsettings | bar hit band (-8..+30, +-80, 10 px) | code | static MicUi_HitTestLevelBar 0x20c1170+0xb0 | ok |
| soundsettings | bar anchors (slots 2..4) | file | menu/mount_layout.bin 0003 (lpui.layoutfile) | ok |
| soundsettings | knob x (slots 10..12) = code immediates | file | menu/mount_layout.bin 0003 (lpui.layoutfile) | ok |
| displaysettings | knob (0x9e, 40 c + 0x1c) | code | static StateLogic_DrawGraphicsData_d090c 0x20d090c+0x58 | ok |
| displaysettings | slider hit band (-8..+16, +-80, 40 px) | code | static StateLogic_ClearData_d0a78 0x20d0a78+0xb0 | ok |
| displaysettings | touch rectangles (ids 0, 1, 5..14), slider anchor | file | menu/mount_layout.bin 0004 (lpui.layoutfile) | ok |
| statmeter | scale 0xb0, piece sequences | code | static SprFx_UpdateData_d2344 0x20d2344+0x1c4 | ok |
| statmeter | /192 multiplier | code | static SprFx_UpdateData_d2344 (pool) 0x20d2344+0x1d0 | ok |
| textlayout | canvas params option (screen, bg, chars, lines, spacing) | table | static 0x210318c i x6 via 0x208af90  | ok |
| textlayout | canvas params guide (screen, bg, chars, lines, spacing) | table | static 0x2103144 i x6 via 0x208af94  | ok |
| textlayout | canvas params line (screen, bg, chars, lines, spacing) | table | static 0x210315c i x6 via 0x208af98  | ok |
| textlayout | canvas params name (screen, bg, chars, lines, spacing) | table | static 0x2103174 i x6 via 0x208af9c  | ok |
| textlayout | OPTION_LAYOUT (pen x, left) x2 lines | table | static 0x212754c 4i x3 via 0x208b41c  | ok |
| textlayout | name box layout (0xc, left 7, top 1, centred 2) | code | static TextLayout_ProcessParagraph 0x208b4b0+0x94 | ok |
| textlayout | option box top 3 | code | static TextLayout_BuildParagraph 0x208b1ec+0x22c | ok |
| textlayout | 3D allocator bases 0x1180 / 0x1e00, 12 px per char | code | static TextCanvas_InitForBG 0x2084024+0x1a8 | ok |
| textlayout | derived box geometry vs RAM (all captures) | code | TextLayout_BuildLine + TextCanvas_InitForBG | ok |
