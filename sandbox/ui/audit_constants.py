"""Provenance audit of the screen-logic constants: each value a module uses is checked against where the game keeps it.
  table   the game's data table at an address (read and compared; such values should be loaded, not copied)
  code    instruction immediates / literal-pool words inside the named function (searched in its disassembly)
  runtime set while a screen runs or read from a file: listed as open, with what is known
    .venv/bin/python sandbox/ui/audit_constants.py [--md OUT.md]"""
import os, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from lpui import gamedata as G
from lpui.logic import (timesetting, fileselect, titlemenu, msgwin, profile, schedule, confirmdialog, hud, roommenu,
                        soundsettings, displaysettings, statmeter, dialogue, choices)

E = []          # (module, item, kind, source, check) ; check() -> (ok, detail)


def tab(module, item, ov, addr, fmt, n, want, cols=None, note=''):
    def check():
        got = G.table(addr, fmt, n, ov)
        if cols is not None:
            got = [tuple(r[c] for c in cols) if isinstance(r, tuple) else r for r in got]
        got = [tuple(r) if isinstance(r, tuple) else r for r in got]
        w = [tuple(r) if isinstance(r, (tuple, list)) else r for r in want]
        return got == w, 'game %s' % (got if got != w else 'equal')
    E.append((module, item, 'table', '%s %#x %s x%d%s' % ('static' if ov is None else 'ov%02d' % ov, addr, fmt, n,
                                                         (' ' + note) if note else ''), check))


def ptab(module, item, ov, lit, fmt, n, want, cols=None, note=''):
    """a table reached through a literal-pool pointer"""
    addr = G.word(lit, ov)
    tab(module, item, ov, addr, fmt, n, want, cols, ('via %#x ' % lit) + note)


def code(module, item, ov, lo, size, values, func):
    def check():
        missing = [v for v in values if not G.find_value(lo, lo + size, v, ov)]
        return not missing, 'not found: %s' % missing if missing else 'all %d found' % len(set(values))
    E.append((module, item, 'code', '%s %s %#x+%#x' % ('static' if ov is None else 'ov%02d' % ov, func, lo, size), check))


def runtime(module, item, what):
    E.append((module, item, 'runtime', what, lambda: (None, 'open')))


# ---- clock setting (overlay 42)
tab('timesetting', 'EDIT_TARGETS', 42, 0x216c6e0, '6i', 12, timesetting.EDIT_TARGETS, cols=(0, 1, 2, 3, 4))
tab('timesetting', 'OK_TARGET', 42, 0x216c6c8, '6i', 1, [timesetting.OK_TARGET], cols=(0, 1, 2, 3, 4))
code('timesetting', 'FIELDS (columns, centres, sprite slots)', 42, 0x216bb98, 308,
     [x for f in timesetting.FIELDS for x in f[1:]], 'Ov42TimeSetting_RenderEditedDateTime')
code('timesetting', "digit sequence = (c - '0') + 3, 16 px pitch", 42, 0x216bae0, 176, [0x30, 3], 'Ov42TimeSetting_RenderTimeDigits')
code('timesetting', 'year wrap 99', 42, 0x216c498, 48, [99], 'Ov42TimeSetting_IncrementEditedYear')
code('timesetting', 'month wrap 12', 42, 0x216c504, 60, [12], 'Ov42TimeSetting_IncrementEditedMonth')
code('timesetting', 'hour 23 / minute 59 wrap, repeat 60 frames', 42, 0x216aa2c, 0x216b178 - 0x216aa2c, [0x17, 0x3b, 0x3c],
     'Ov42TimeSetting_RunTimeSettingStateMachine')

# ---- file select (overlay 27)
ptab('fileselect', 'POS (disc x)', 27, 0x216bedc, 'i', 3, list(fileselect.POS))
ptab('fileselect', 'HITS', 27, 0x216c41c, '6i', 6, fileselect.HITS, cols=(0, 1, 2, 3, 4))
ptab('fileselect', 'SPEED_LEFT', 27, 0x216c9f4, 'i', 3, list(fileselect.SPEED_LEFT))
ptab('fileselect', 'SPEED_RIGHT', 27, 0x216cebc, 'i', 3, list(fileselect.SPEED_RIGHT))
code('fileselect', 'arc: centre 0x80, divisor 0x9000, top 0x60', 27, 0x216bf70, 52, [0x80, 0x9000, 0x60],
     'Ov27FileSelect_MapSlotXToArcY')

# ---- title menu (overlay 21)
for _n in (3, 4, 5, 6):
    tab('titlemenu', 'LAYOUT[%d] (at (n-3)*0x54)' % _n, 21, 0x21705c4 + (_n - 3) * 0x54, '6h', _n, titlemenu.LAYOUT[_n])
code('titlemenu', 'page offsets', 21, 0x216d630, 132, [5, 10, 0x10, 0x16, 0x19, 0x1c], 'Ov21TitleMenu_GetTitleMenuVariantIndex')
code('titlemenu', 'page transitions', 21, 0x216d0f8, 1212, [0x21, 5, 10, 0x10, 0x16, 0x19, 0x1c],
     'Ov21TitleMenu_UpdateTitleMenuGfxTransition')

# ---- message window (static)
tab('msgwin', 'SPEED', None, 0x21274e0, 'i', 5, msgwin.SPEED)
tab('msgwin', 'LAYOUT3 (pen x, map left)', None, 0x212750c, '2i', 3, msgwin.LAYOUT3)
tab('msgwin', 'LAYOUT2', None, 0x21274f4, '2i', 3, msgwin.LAYOUT2)
code('msgwin', 'line width 13 cells', None, 0x208afa8, 328, [msgwin.LINE_CELLS], 'TextLayout_BuildGlyphs')

# ---- profile (overlay 6)
tab('profile', 'DAKUTEN_MAP from', 6, 0x2184224, 'i', 9, [f for f, t in profile.DAKUTEN_MAP])
tab('profile', 'DAKUTEN_MAP to', 6, 0x2184248, 'i', 9, [t for f, t in profile.DAKUTEN_MAP])
tab('profile', 'HANDAKUTEN_MAP from', 6, 0x218426c, 'i', 3, [f for f, t in profile.HANDAKUTEN_MAP])
tab('profile', 'HANDAKUTEN_MAP to', 6, 0x2184278, 'i', 3, [t for f, t in profile.HANDAKUTEN_MAP])
tab('profile', 'PAGE_COUNTS (sub-pages / prefecture entries)', 6, 0x2184058, 'B', 52, list(profile.PAGE_COUNTS))
code('profile', 'summary columns / y', 6, 0x21824dc, 520, [0xdc, 0xc6, 0xaf, 0x99, 0x83, 0x6b, 0x54, 0x3e, 0x4c, 0xe, 0xf],
     'Ov06ProfileDlt_DrawProfileSelectionValues')
code('profile', 'value pages geometry', 6, 0x217ee74, 1460, [0xf2, 0xc, 0xdb, 0x32, 0xa, 0xd3, 0x66, 0xea, 0x36, 0x2e],
     'Ov06ProfileDlt_DrawProfileEditDialog')
code('profile', 'voicing marks 0x32 / 0x33', 6, 0x2181364, 640, [0x32, 0x33], 'Ov06ProfileDlt_BuildProfileEditMenu')
code('profile', 'grid builder (10/9 rows, 5/6 columns, marks 0x32/0x33)', 6, 0x217f4d4, 256, [10, 6, 5, 9, 0x32, 0x33, 0x168],
     'Ov06ProfileDlt_ClearProfileEditSelection')

# ---- schedule (overlay 3)
tab('schedule', 'MAIN targets', 3, 0x215ea80, '6i', 14, schedule.MAIN, cols=(0, 1, 2, 3, 4))
ptab('schedule', 'COLUMN_X', 3, 0x215cf38, 'i', 4, list(schedule.COLUMN_X))
ptab('schedule', 'LAYOUT_Y', 3, 0x215cf34, '10i', 10,
     [tuple(list(r) + [0] * (10 - len(r))) for r in schedule.LAYOUT_Y])
ptab('schedule', 'LAYOUT_ID', 3, 0x215cf3c, '10i', 10,
     [tuple(list(r) + [-1] * (10 - len(r))) for r in schedule.LAYOUT_ID])
runtime('schedule', 'activities per period', 'game simulation (EventLogic_UpdateTimeData 0x20ccf34 / 0x20cd334): out of UI scope')

# ---- confirmation dialog (static)
ptab('confirmdialog', 'OK target', None, 0x20be650, '6i', 1, confirmdialog.OK, cols=(0, 1, 2, 3, 4))
ptab('confirmdialog', 'page arrows (next, prev)', None, 0x20bf594, '6i', 2, [confirmdialog.NEXT, confirmdialog.PREV], cols=(0, 1, 2, 3, 4))
ptab('confirmdialog', 'YES_NO targets', None, 0x20bee24, '6i', 2, [(80, 64, 16, 28, 2), (80, 128, 16, 28, 3)], cols=(0, 1, 2, 3, 4),
     note='MsgBox_HandleInput(2); values the trace recorded')

# ---- HUD / time period (static)
tab('hud', 'PERIOD_BOUNDS', None, 0x2101d48, 'B', 5, list(hud.PERIOD_BOUNDS))
tab('hud', 'NIGHT_START', None, 0x2101d4d, 'B', 1, [hud.NIGHT_START])
code('hud', 'time slot sequence base 0x28', None, 0x2080d70, 36, [0x28], 'DateUi_AdvanceTimeSequence')

# ---- room menu (overlay 35)
code('roommenu', 'SEQ', 35, 0x216cae8, 804, sorted({v for t in roommenu.SEQ.values() for v in t if v is not None} - {0, 1, 2, 3, 4, 5}),
     'Ov35MainScene_SwitchMainSceneMenu')
code('roommenu', 'SEQ_ALT_A', 35, 0x216ce10, 160, sorted({v for t in roommenu.SEQ_ALT_A.values() for v in t}), 'Ov35MainScene_SwitchMainSceneSubmenuA')
code('roommenu', 'SEQ_ALT_B', 35, 0x216ceb4, 160, sorted({v for t in roommenu.SEQ_ALT_B.values() for v in t}), 'Ov35MainScene_SwitchMainSceneSubmenuB')
code('roommenu', 'ACTION_REMAP', 35, 0x216cf58, 0x216d150 - 0x216cf58, sorted(set(roommenu.ACTION_REMAP.values())), 'Ov35MainScene_SwitchMainSceneSubmenuC (with its jump-table cases)')
tab('roommenu', 'slot table (x, y, half w, half h, id, enabled)', 35, 0x21732c0, '6i', 13, roommenu.slot_table())
code('roommenu', 'menu kinds by game state', 35, 0x216b9a4, 1028, [0x15, 0x16, 0x11, 0x12, 0xb, 0xc, 0xd, 0xe, 0xf, 0x10, 4, 5],
     'Ov35MainScene_UpdateMainSceneEventState')
code('roommenu', 'settings submenu kinds 7, 8', 35, 0x21708ac, 0x21709a0 - 0x21708ac, [7, 8], 'Ov35MainScene_LoadMainSceneDateResources')
code('roommenu', 'slide offset 0xa0, slot stride 0x18', 35, 0x216c220, 0x40, [0xa0, 0x18], 'Ov35MainScene_UpdateMainSceneMenuSprites')

# ---- settings
code('soundsettings', 'KNOB_X (bar 1)', None, 0x20c0efc, 140, [soundsettings.KNOB_X[0], 10, 0xb], 'MicUi_DrawPrimarySprite')
code('soundsettings', 'KNOB_X (bar 2)', None, 0x20c0f8c, 108, [soundsettings.KNOB_X[1]], 'MicUi_DrawSecondarySprite')
code('soundsettings', 'KNOB_X (bar 3)', None, 0x20c0ff8, 108, [soundsettings.KNOB_X[2]], 'MicUi_DrawTertiarySprite')
code('soundsettings', 'bar hit band (-8..+30, +-80, 10 px)', None, 0x20c1170, 176, [8, 0x1e, 0x50, 10], 'MicUi_HitTestLevelBar')
def _layout(module, item, name, check_fn):
    E.append((module, item, 'file', 'menu/mount_layout.bin %s (lpui.layoutfile)' % name, check_fn))
_layout('soundsettings', 'bar anchors (slots 2..4)', '0003',
        lambda: (soundsettings.bar_anchors() == [(183, 85), (138, 85), (93, 85)], 'emulator slot records 183/138/93, 85'))
_layout('soundsettings', 'knob x (slots 10..12) = code immediates', '0003',
        lambda: ([(s['x']) for s in soundsettings.layout()['slots'][10:13]] == list(soundsettings.KNOB_X), 'file vs MicUi_Draw*Sprite'))
code('displaysettings', 'knob (0x9e, 40 c + 0x1c)', None, 0x20d090c, 88, [0x9e, 0x28, 0x1c], 'StateLogic_DrawGraphicsData_d090c')
code('displaysettings', 'slider hit band (-8..+16, +-80, 40 px)', None, 0x20d0a78, 176, [8, 0x10, 0x50, 0x28], 'StateLogic_ClearData_d0a78')
_layout('displaysettings', 'touch rectangles (ids 0, 1, 5..14), slider anchor', '0004',
        lambda: (displaysettings.slider_anchor() == (148, 85) and sorted(displaysettings.touch_targets()) == [0, 1] + list(range(5, 15)),
                 'slider (148, 85) as the emulator read it'))

# ---- stat meters (static)
code('statmeter', 'scale 0xb0, piece sequences', None, 0x20d2344, 452, [0xb0, 0xf, 0x17, 0x18, 0x21, 0x20], 'SprFx_UpdateData_d2344')
code('statmeter', '/192 multiplier', None, 0x20d2344, 0x1d0, [0x2aaaaaab], 'SprFx_UpdateData_d2344 (pool)')

# ---- dialogue / choices text boxes
from lpui import textlayout
for _g in textlayout.GROUPS:
    ptab('textlayout', 'canvas params %s (screen, bg, chars, lines, spacing)' % _g[0], None, _g[2], 'i', 6,
         list(textlayout.params(_g[2])))
ptab('textlayout', 'OPTION_LAYOUT (pen x, left) x2 lines', None, 0x208b41c, '4i', 3, textlayout.OPTION_LAYOUT)
code('textlayout', 'name box layout (0xc, left 7, top 1, centred 2)', None, 0x208b4b0, 148, [0xc, 7, 1, 2],
     'TextLayout_ProcessParagraph')
code('textlayout', 'option box top 3', None, 0x208b1ec, 556, [3], 'TextLayout_BuildParagraph')
code('textlayout', '3D allocator bases 0x1180 / 0x1e00, 12 px per char', None, 0x2084024, 424, [0x1180, 0x1e00, 0xc],
     'TextCanvas_InitForBG')
def _check_geometry():
    import struct, glob
    caps = sorted(glob.glob('/private/tmp/claude-501/-Users-gagemarshall-loveplus/55c2bc91-97e1-41ba-829c-04358d33ce89/scratchpad/rt/cap-f*'))
    if not caps:
        return None, 'no captures to compare'
    bad = 0
    for d in caps:
        ram = open(d + '/ram.bin', 'rb').read()
        # the boxes keep the allocation made when the window was set up: the 3D state then, not at the capture
        if not any(all(struct.unpack_from('<3I', ram, 0x2364094 + v['obj_offset'] + 0x28 - 0x2000000) == (v['w'], v['h'], v['char_ofs'])
                       for v in textlayout.boxes(c3d).values()) for c3d in (False, True)):
            bad += 1
    return bad == 0, '%d of %d captures differ' % (bad, len(caps))
E.append(('textlayout', 'derived box geometry vs RAM (all captures)', 'code', 'TextLayout_BuildLine + TextCanvas_InitForBG', _check_geometry))


def main(md=None):
    rows = []
    for module, item, kind, source, check in E:
        try:
            ok, detail = check()
        except Exception as ex:                                     # noqa: BLE001
            ok, detail = False, 'error: %s' % ex
        rows.append((module, item, kind, source, ok, detail))
    w = max(len(r[0]) for r in rows)
    for r in rows:
        mark = {True: 'ok  ', False: 'FAIL', None: 'open'}[r[4]]
        print('%s %-*s %-8s %-48s %s' % (mark, w, r[0], r[2], r[1][:48], r[3] + ('' if r[4] else '  -> ' + r[5])))
    n = len(rows)
    print('\n%d constants: %d tables, %d code, %d layout file, %d runtime/open; failures %d' % (
        n, sum(r[2] == 'table' for r in rows), sum(r[2] == 'code' for r in rows), sum(r[2] == 'file' for r in rows),
        sum(r[2] == 'runtime' for r in rows),
        sum(r[4] is False for r in rows)))
    if md:
        with open(md, 'w') as f:
            f.write('| module | value | kind | source | result |\n|---|---|---|---|---|\n')
            for r in rows:
                f.write('| %s | %s | %s | %s | %s |\n' % (r[0], r[1], r[2], r[3], {True: 'ok', False: 'FAIL: ' + r[5], None: 'open: ' + r[3]}[r[4]]))
    return rows


if __name__ == '__main__':
    main(sys.argv[sys.argv.index('--md') + 1] if '--md' in sys.argv else None)
