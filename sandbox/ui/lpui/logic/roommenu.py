"""Room / main scene bottom-screen menu (overlay 35; decompiled Ov35MainScene_*, fid/decomp/ov35.c).

State (pointer at 0x2173b04): 12 menu slots at +0xcf8 + 0x10 i: sprite, +0xcfc aux sprite (kinds 7..9, 0x13, 0x14),
+0xd00 kind, +0xd04 pressed; +0x38 + 4 i slot enabled; +0xcf4 selected slot; +0x78 the action the scene takes next.
Button sprite sequence (SwitchMainSceneMenu 0x216cae8) by kind: (pressed, normal, disabled); kinds 7..9 / 0x13 / 0x14 use
their own path (UpdateMainSceneMenuSprites: kind - 1 / 0x47 / 0x48 with aux sprite sequence 9).
Selecting (SwitchMainSceneSubmenuC 0x216cf58): action = kind, except 4 -> 0x15 when game flag field 0xe0 is set,
0x13 -> 0x16, 0x14 -> 0x17, 0x15 -> 0x1b, 0x16 -> 0x1c; kind 0x12 = go out (map; action 0x1d with flag 0x2000)."""

# kind: (pressed seq, normal seq, disabled seq or None)
SEQ = {1: (10, 0, None), 2: (0xb, 1, 0x29), 3: (0xc, 2, 0x2a), 4: (0xd, 3, None), 5: (0xe, 4, None), 6: (0xf, 5, None),
       7: (0x44, 6, None), 8: (0x45, 7, None), 9: (0x46, 8, None), 10: (5, 4, None),
       0xb: (0x13, 0x10, 0x2b), 0xc: (0x17, 0x14, 0x2c), 0xd: (0x1b, 0x18, 0x2d), 0xe: (0x1f, 0x1c, 0x2e),
       0xf: (0x23, 0x20, None), 0x10: (0x27, 0x24, None), 0x11: (0x32, 0x2f, None), 0x12: (0x36, 0x33, None),
       0x13: (0x4a, 0x48, None), 0x14: (0x49, 0x47, None), 0x15: (0x53, 0x51, 0x56), 0x16: (0x54, 0x52, 0x55)}
ACTION_REMAP = {0x13: 0x16, 0x14: 0x17, 0x15: 0x1b, 0x16: 0x1c}


def button_seq(kind, pressed=False, enabled=True):
    """SwitchMainSceneMenu's sequence for a slot"""
    p, n, dis = SEQ[kind]
    s = p if pressed else n
    if not enabled and dis is not None:
        s = dis
    return s


def action(kind, flag_e0=False):
    if kind == 4 and flag_e0:
        return 0x15
    return ACTION_REMAP.get(kind, kind)

# In the scene's other mode (state +0 != 0) kinds 0xb..0x12 take SwitchMainSceneSubmenuA's sequences (enabled, disabled);
# SwitchMainSceneSubmenuB (from UpdateMainSceneSpriteState) is the companion set one frame further.
SEQ_ALT_A = {0xb: (0x11, 0x37), 0xc: (0x15, 0x39), 0xd: (0x19, 0x3b), 0xe: (0x1d, 0x3d), 0xf: (0x21, 0x21),
             0x10: (0x25, 0x25), 0x11: (0x30, 0x30), 0x12: (0x34, 0x34)}
SEQ_ALT_B = {0xb: (0x12, 0x38), 0xc: (0x16, 0x3a), 0xd: (0x1a, 0x3c), 0xe: (0x1e, 0x3e), 0xf: (0x22, 0x22),
             0x10: (0x26, 0x26), 0x11: (0x31, 0x31), 0x12: (0x35, 0x35)}


def slot_seq(kind, pressed=False, enabled=True, mode=0, selecting=False):
    """the sequence a slot shows: UpdateMainSceneMenuSprites draws kinds 0xb..0x12 with SubmenuA's set when the scene
    state +0 != 0; once a slot has been selected (+0x74 == 2) HandleMainSceneMenuAction redraws every slot with
    SwitchMainSceneMenu (the normal set)"""
    if mode != 0 and not selecting and kind in SEQ_ALT_A:
        return SEQ_ALT_A[kind][0 if enabled else 1]
    return button_seq(kind, pressed, enabled)


# ---- which buttons the room menu shows (Ov35MainScene_UpdateMainSceneEventState 0x216b9a4) -------------------------
# Out: 12 kinds (+ enabled flags) for the menu slots. In: game-state queries, named after the game functions that answer
# them (they are game simulation, supplied by the caller):
#   heroine       GameState +0x78 (a girlfriend route is set up)
#   flags         GameState +0xbf8 word (bits 1 / 0 / 2 checked, 0x1000 for the time-state rule)
#   period        GameClock_GetTimePeriod();  hour   the clock hour (GameClock_Thunk_205f6a4)
#   e0            GameFlags field 0xe0 (holiday / special day)
#   special       GameFlags_Thunk_2057a24()
#   ev4240 / ev42e8 / ev40b4 / ev4198   Mod20e3b00_ProcTime_20e4240 / _20e42e8 / _20e40b4(0) / _20e4198
#   night_event   EventLogic_Wrap_Get_206325c_20c97b8(0x100);  outing  Mod20ae6e8_Proc_20aede0 (night) / FUN_020f2498 (day)
#   test8         GameFlags_Test(0, 8)
MENU_SLOTS = 12
# the menu's slot rectangles and positions (x, y, half w, half h, id, enabled), overlay 35 0x21732c0; shifted 0xa0 in x
# while the scene state is 1 (UpdateMainSceneMenuSprites)
def slot_table():
    from lpui import gamedata as _G
    return [r for r in _G.table(0x21732c0, '6i', 13, ov=35)]


def time_state(q):
    """Ov35MainScene_CheckMainSceneTimeState 0x216b8f0"""
    if not q['heroine'] or q['special'] or q['ev4240'] or q['ev42e8']:
        return 0
    if q['e0']:
        return 1
    if q['flags'] & 0x1000:
        return 1
    late = q['period'] == 5 and (q['hour'] < 0x15 or q['hour'] > 0x17)
    return 1 if (not late and q['period'] != 0) else 0


def build_kinds(q):
    k = [0] * MENU_SLOTS
    en = [0] * MENU_SLOTS
    if not q['heroine']:
        k[0], en[0], k[1] = (4 if q['period'] == 5 else 1), 1, 2
        k[2] = 3
        en[2] = 1 if (q['flags'] & 4 or q['flags'] & 1 or q['flags'] & 2) else 0
        for i, kind in zip(range(5, 11), (0xb, 0xc, 0xd, 0xe, 0xf, 0x10)):
            k[i] = kind
        en[9] = en[10] = 1
        return k, en
    if not q['special']:
        if q['ev4240']:
            en[0], k[0], k[1], en[1], k[2], en[2] = 1, 0x15, 2, 0, 3, 0
        elif q['ev42e8']:
            en[0], k[0], k[1], en[1], k[2], en[2] = 0, 0x15, 2, 1, 3, 0
        else:
            if (not q['ev40b4'] or q['e0']) and (not q['ev4198'] or q['e0']):
                k[0] = 4 if (q['period'] == 5 and q['e0']) else 1
                en[0], k[1], en[1], k[2] = 1, 2, 1, 3
                en[2] = time_state(q)
            else:
                k[0], en[0], k[1], en[1], k[2] = 5, 1, 2, 1, 3
                en[2] = 1 if (q['e0'] or q['period'] != 5) else 0
    else:
        en[0] = 1 if q['test8'] else 0
        k[0], k[1], en[1], k[2], en[2] = 0x16, 0, 0, 0, 0
    if not q['e0'] and (q['special'] or q['ev4240']):
        k[3], en[3] = 0, 0
    else:
        k[3], en[3] = 0x11, 1
    if q['period'] == 5 and not q['night_event'] and q['e0']:
        k[4], en[4] = 0, 0
    else:
        k[4], en[4] = (0x12, 1) if q['outing'] else (0, 0)
    k[5], en[5] = 0xb, 1
    if not q['special']:
        for i, kind in zip((6, 7, 8), (0xc, 0xd, 0xe)):
            k[i], en[i] = kind, 1
    k[9], en[9], k[10], en[10] = 0xf, 1, 0x10, 1
    return k, en
