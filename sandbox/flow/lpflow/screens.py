"""The screen manager (static 0x2089d8c..0x208ad90; the auto-labels call it Sprite_*): menu-type screens are requested,
run until they finish, and can hand over to another screen. Ported from the ARM code.

Request block REQ (0x2152ea4):
    +0x00 run state  0 idle, 1 start, 2 build, 3 first draw, 5 running, 6 switched (restart at 1), 7 finished (result),
          8 finished (alternative result)
    +0x04 kind (0: a screen with an overlay; 1, 2: special set-ups)   +0x0c screen id   +0x10 next screen
    +0x14 result   +0x18 alternative result   +0x1c entry argument   +0x20 the screen object (0x29c bytes)
    +0x24 switch pending
A screen tells the manager what to do by the value its frame function returns (Screen_RunCurrent 0x20882e0):
3 switch to the screen it set with 0x20882c8, 4 finish with the result it set with 0x2088624, 5 finish with the
alternative result (0x208a200)."""
from .names import F, W
from .gamecode import case_literals

REQ = W(0x2089e00)
OVERLAY_OF_SCREEN = case_literals(0x208a994)          # screen id -> overlay (None: the screen is static code)
_UNLOAD_OF_SCREEN = case_literals(0x208abd4)


def request(cx, kind, screen, arg):
    """0x2089d8c Screen_Request(kind, screen, arg)"""
    cx.store(REQ + 4, kind)
    cx.store(REQ, 1)
    if kind == 0:
        cx.call(F.Screen_LoadOverlay, screen)
        cx.store(REQ + 0xc, screen)
        cx.store(REQ + 0x10, -1)
        cx.store(REQ + 0x1c, arg)
    elif kind in (1, 2):
        cx.store(REQ + 0xc, -1)
        cx.store(REQ + 0x10, -1)
        cx.store(REQ + 0x1c, arg)


def load_overlay(cx, screen):
    """0x208a994: Overlay_Load + GameHeap_LoadOverlayInfoAlt of the screen's overlay"""
    ov = OVERLAY_OF_SCREEN.get(screen) if 0 <= screen <= 20 else None
    if ov is not None:
        cx.call(F.Overlay_Load, ov)
        cx.call(F.GameHeap_LoadOverlayInfoAlt, ov)


def unload_overlay(cx, screen):
    """0x208abd4: Overlay_Unload(all) + GameHeap_LoadOverlayInfo(the screen's overlay)"""
    ov = _UNLOAD_OF_SCREEN.get(screen) if 0 <= screen <= 20 else None
    if ov is not None:
        cx.call(F.Overlay_Unload, -1)
        cx.call(F.GameHeap_LoadOverlayInfo, ov)


def transition(cx, mode):
    """0x208a2b4: what the running screen asked for"""
    if mode == 3:                                      # switch to the next screen
        cx.call(F.Screen_UnloadOverlay, cx.load(REQ + 0xc))
        cx.store(REQ, 6)
        nxt = cx.call(F.Screen_GetNext)
        cx.store(REQ + 0xc, nxt)
        cx.call(F.Screen_LoadOverlay, nxt)
    elif mode == 4:                                    # finished with a result
        cx.call(F.Screen_UnloadOverlay, cx.load(REQ + 0xc))
        cx.store(REQ, 7)
        cx.store(REQ + 0xc, -1)
        cx.store(REQ + 0x14, cx.call(F.Screen_GetResult))
    elif mode == 5:                                    # finished with the alternative result
        cx.store(REQ + 0x18, cx.call(F.Screen_GetAltResult))
        cx.store(REQ, 8)


DISPCNT_A, DISPCNT_B = 0x4000000, 0x4001000
BG_MODE_MASK = W(0x208a1d4)                            # 0xffcfffef: clears the OBJ mapping bits


def _rmw(cx, addr, clear, set_=0, size=4):
    cx.store(addr, (cx.load(addr, size) & ~clear) | set_, size)


def run(cx):
    """0x2089e04 Screen_Run: one frame of the manager; -1 while the screen runs, its result when it finishes"""
    ret = -1
    st = cx.load(REQ)
    if st == 1:                                        # start: display set-up, the screen object
        cx.call(0x2047d40)                             # GfxRes_CallHookPtr
        _rmw(cx, DISPCNT_A, 0xe000)
        _rmw(cx, DISPCNT_B, 0xe000)
        winout = W(0x208a1d0)                          # sub WINOUT 0x400104a
        _rmw(cx, 0x400004a, 0x3f, size=2)
        _rmw(cx, winout, 0x3f, size=2)
        _rmw(cx, 0x4000048, 0x3f00, size=2)
        _rmw(cx, winout - 2, 0x3f00, size=2)
        cx.call(0x201a950, 0x4000050, 0, 0, 0)         # G2x_SetBlendAlpha_ (main)
        cx.call(0x201a950, winout + 6, 0, 0, 0)        # (sub)
        kind = cx.load(REQ + 4)
        if kind == 0:
            if cx.load(REQ + 0xc) != 0:
                cx.call(0x201a73c)                     # GX_DispOn
                _rmw(cx, DISPCNT_B, 0, 0x10000)
                cx.call(0x20410e0)
                cx.call(0x2041e1c)                     # 2D-only VRAM banks
                _rmw(cx, DISPCNT_A, 0x1f00, 0x1f00)
                _rmw(cx, DISPCNT_B, 0x1f00, 0x1f00)
                cx.call(0x2084ed4)                     # Bg_ClearAll
            screen = cx.load(REQ + 0xc)
            mask = BG_MODE_MASK
            if screen == 5:
                cx.store(DISPCNT_A, (cx.load(DISPCNT_A) & mask) | 0x200010)
                cx.store(DISPCNT_B, (cx.load(DISPCNT_B) & mask) | 0x10)
            elif screen in (3, 2, 1, 6, 8, 7, 9, 0xf, 0x10, 0x11, 4, 0x12, 0xb, 0xc, 0xd, 0xe, 0x13, 0x14):
                cx.store(DISPCNT_A, (cx.load(DISPCNT_A) & mask) | 0x200010)
                cx.store(DISPCNT_B, (cx.load(DISPCNT_B) & mask) | 0x200010)
            else:
                cx.store(DISPCNT_A, (cx.load(DISPCNT_A) & mask) | 0x10)
                cx.store(DISPCNT_B, (cx.load(DISPCNT_B) & mask) | 0x10)
            cx.store(REQ + 0x20, cx.call(0x203fda8, 0x29c))      # GameHeap_AllocZeroed
            cx.call(0x208757c)
        elif kind == 1:
            cx.call(0x201a73c)
            _rmw(cx, DISPCNT_B, 0, 0x10000)
            cx.call(0x2041de0)                         # default VRAM banks
            _rmw(cx, DISPCNT_A, 0x1f00, 0x1800)
        cx.store(REQ, 2)
    elif st == 2:                                      # build the screen
        if cx.load(REQ + 4) == 0:
            cx.call(F.ResTable_Open, 1, W(0x208a1d8))
            proc = cx.call(0x2087538, cx.load(REQ + 0xc))          # the screen's procedure table
            obj = cx.load(REQ + 0x20)
            cx.call(0x20875fc, obj, proc)
            res = cx.load(cx.load(REQ + 0x20) + 0x11c)
            cx.call(F.ResTable_Open, 1, res if res != 0 else W(0x208a1dc))
            cx.call(0x208a344)
            cx.call(0x208a488)
            cx.call(F.ResTable_Open, 1, W(0x208a1dc))
        cx.store(REQ, 3)
    elif st == 3:                                      # first frame
        kind = cx.load(REQ + 4)
        if kind == 0:
            cx.call(0x2088298, cx.load(REQ + 0xc))
            if cx.load(cx.load(REQ + 0x20) + 0x298) != 0:
                cx.call(0x2085360, 2, 1)               # Bg_SelectState
            if cx.load(cx.load(REQ + 0x20) + 0x11c) != 0:
                cx.call(F.ResTable_Open, 1)
            cx.call(0x208a5e4)
            cx.call(F.ResTable_Open, 1, W(0x208a1dc))
        elif kind == 2:
            cx.call(0x2085360, 2, 0)
            cx.call(0x2043084, 1, 0, 0)                # Gfx_SetBgVisibleFlag
        cx.store(REQ, 5)
    elif st == 4:
        cx.store(REQ, 5)
    elif st == 5:                                      # running
        if cx.load(REQ + 4) == 0:
            mode = cx.call(F.Screen_RunCurrent)
            cx.call(F.Screen_Transition, mode)
        cx.call(0x2085568)                             # Bg_DispatchState
    elif st == 6:                                      # switched: restart with the next screen
        cx.call(0x2088288)
        cx.call(0x2085018)
        cx.store(REQ + 0x24, 0)
        cx.call(0x2047d4c)
        cx.store(REQ, 1)
    elif st == 7:                                      # finished: hand back the result
        ret = cx.load(REQ + 0x14, signed=True)
        cx.call(0x2088288)
        cx.call(0x2085018)
        cx.store(REQ + 0x24, 0)
        cx.call(0x2047d4c)
        cx.store(REQ, 0)
        cx.store(DISPCNT_A, (cx.load(DISPCNT_A) & BG_MODE_MASK) | 0x200010)
    elif st == 8:
        ret = cx.load(REQ + 0x18, signed=True)
        cx.store(REQ, 5)
    return ret
