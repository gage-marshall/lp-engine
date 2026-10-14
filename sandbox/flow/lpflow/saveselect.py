"""SaveSelect_ProcessScreen 0x2074600 (top state 0x1f, depth 3): the title menu and everything it starts, and the
SceneLoop A / B helpers it runs the file select and file delete screens with.

State 0x28 runs the title menu (screen 0) and branches on its result (ov21 maps the button to it: table 0x217041c).
Each single-player branch is ported; the wireless ones (communication talk, card exchange, すれちがい, save data send /
receive / transfer, ご当地カプセル) raise NotPorted with the state. A branch ends by requesting the title menu again
with the page to open (its entry argument) and restarting at 0, or by leaving SaveSelect:
    returns -1   back to the title screen (the menu timed out)
    returns  1   a game is to start (new: flag 0x20000000 set by the profile; continue otherwise)"""
from .names import F, W

CTX = W(0x20755c0)                 # 0x2146e94
DISP_OR = W(0x20755c4)             # 0x200010
DISP_MASK = W(0x20755c8)           # 0xffcfffef
ILLUS = W(0x20755cc)               # 0x119
DISPCNT_B = W(0x20755e0)           # 0x4001000
DISPCNT_A = 0x4000000
OV_MENU38 = W(0x2076670)           # 38
OV_ILLUS = W(0x2076674)            # 29
OV_GALLERY_XFER = W(0x2076664)     # 10
OV_OWN_CARD = W(0x2076c60)         # 18
OV_FRIENDS = W(0x2076c64)          # 16
OV_STAFF = W(0x2076c6c)            # 23
OV_GOTOCHI_CHARA = W(0x2076c54)    # 15
OV_PAIR_EVENT = W(0x2076c5c)       # 0
TRIPS = W(0x20755dc)               # 0x209
NO_SAVE_PALETTE = W(0x2076c70)     # 0x2102b8c
NO_SAVE_TEXT = W(0x2076c74)

# title menu result -> SaveSelect state (the switch in state 0x28); None: no fade before
_BRANCH = {0: 0x3c, 1: 0x50, 2: 100, 3: 0x78, 4: 0x8c, 5: 0xa0, 6: 0xb4, 7: 200, 8: 0xdc, 10: 0x118, 11: 300,
           12: 0x1b8, 13: 0x1cc, 14: 0x1e0, 15: 0x17c, 16: 400, 0x11: 100, 0x12: 500, 0x13: 0x230, 0x14: 0x208,
           0x15: 0x1a4, 0x16: 0x21c}
_NO_FADE = {6, 7}
# the wireless features: state ranges not ported
WIRELESS = [(0x51, 0x88), (100, 0x6a), (0x78, 0x88), (0x17c, 0x1b8), (400, 0x1a4), (0x1a4, 0x1b8), (500, 0x208),
            (0x185, 0x1b8)]


class NotPorted(Exception):
    pass


def _disp(cx, addr):
    cx.store(addr, (cx.load(addr) & DISP_MASK) | DISP_OR)


def run(cx):
    ret = 0
    cx.call(F.Main_IncStateDepth)
    s = cx.call(F.Main_GetStateAtDepth)
    act = _state(cx, s)
    if act is not None:
        kind = act[0]
        if kind == 'set':
            cx.call(F.Main_SetState, act[1])
        elif kind == 'adv':
            cx.call(F.Main_AdvanceStateSequence)
        elif kind == 'menu':                           # back to the title menu on page act[1]
            cx.call(F.Screen_Request, 0, 0, act[1])
            cx.call(F.Main_SetState, 0)
        elif kind == 'ret':
            ret = act[1]
            cx.call(F.Main_SetState, act[2])
    cx.call(F.Main_DecStateDepth)
    return ret


def _fades_done(cx):
    return cx.call(F.ScreenFade_IsActive, 1) == 0 and cx.call(F.ScreenFade_IsActive, 2) == 0


def _menu_after_reset(cx, page):
    cx.call(F.Display_ResetBgAndVram)
    return ('menu', page)


def _screen_branch(cx, screen):
    """display / sound settings, gallery settings: a static-code screen requested with the sub LCD set up"""
    cx.call(F.SaveSelect_Shutdown)
    cx.call(F.Display_SetLcdScreenSwap, 2)
    _disp(cx, DISPCNT_B)
    cx.call(F.Screen_Request, 0, screen, -1)
    return ('adv',)


def _overlay_branch(cx, ov, both=True):
    cx.call(F.SaveSelect_Shutdown)
    cx.call(F.Display_SetLcdScreenSwap, 2)
    _disp(cx, DISPCNT_A)
    if both:
        _disp(cx, DISPCNT_B)


def _state(cx, s):
    if s == 0:
        if cx.call(F.GameFlags_Test, 0x1000000, 0) != 0:
            return ('set', 0x3d)                       # straight to the file select (after a soft reset)
        if cx.call(F.SaveSelect_Init) != 0:
            cx.call(F.Main_SetState, 0x28)
            cx.call(F.SaveSelect_RefreshNewestSlotDisplay)
            return None
        return ('set', 0x154)                          # the save data cannot be used
    if s == 0x14:
        return ('set', 0x28)
    if s == 0x28:                                      # the title menu
        r = cx.call(F.Screen_Run)
        if r == 0:
            cx.call(F.ScreenFade_ToWhite, 1, 0x1e)
            cx.call(F.ScreenFade_ToWhite, 2, 0x1e)
            return ('set', 0x3c)
        if r == 9:                                     # timed out: back to the title screen
            cx.call(F.ScreenFade_ToWhite, 1, 0x1e)
            cx.call(F.ScreenFade_ToWhite, 2, 0x1e)
            cx.call(F.Main_SetState, 0x104)
            cx.call(F.UiFlow_ThunkTertiary, 0)
            cx.call(F.SaveSelect_TickWaitCounter)
            return None
        if r in _BRANCH:
            if r not in _NO_FADE:
                cx.call(F.ScreenFade_ToWhite, 1, 0x1e)
                cx.call(F.ScreenFade_ToWhite, 2, 0x1e)
            cx.call(F.Main_SetState, _BRANCH[r])
            cx.call(F.UiFlow_ThunkTertiary, 0)
        return None
    # ---- start: file select -> time setting -> profile (SceneLoop A) ----
    if s in (0x3c, 0x8c, 0xa0, 0xdc):                  # fade out, then the branch's screen
        if not _fades_done(cx):
            return None
        cx.call(F.SaveSelect_Shutdown)
        return ('adv',)
    if s == 0x3d:
        r = cx.call(F.SceneLoop_RunSpriteSystemA)
        if r == 1:                                     # backed out of the file select
            return _menu_after_reset(cx, 0)
        if r == 2:                                     # a game was chosen / set up
            return ('set', 0x140)
        return None
    if s == 0x140:
        return ('ret', 1, 0)
    if s == 0xf0:
        return ('set', 0x140) if _fades_done(cx) else None
    if s == 0x104:                                     # back to the title screen
        if not _fades_done(cx):
            return None
        cx.call(F.SaveSelect_Shutdown)
        return ('ret', -1, 0)
    # ---- settings page ----
    if s == 0xb4:
        return _screen_branch(cx, 0x10)                # display settings
    if s == 200:
        return _screen_branch(cx, 0xf)                 # sound settings
    if s in (0xb5, 0xc9):
        return None if cx.call(F.Screen_Run) == -1 else _menu_after_reset(cx, 1)
    if s == 0xdd:                                      # delete data (SceneLoop B: screen 2)
        return _menu_after_reset(cx, 1) if cx.call(F.SceneLoop_RunSpriteSystemB) == 1 else None
    if s == 0x1b8:                                     # staff roll (ov23)
        cx.call(F.SaveSelect_Shutdown)
        cx.call(F.Display_SetLcdScreenSwap, 2)
        _disp(cx, DISPCNT_B)
        cx.call(F.GfxRes_CallHookPtr)
        cx.call(F.Overlay_Load, OV_STAFF)
        cx.call(F.GameHeap_LoadOverlayInfoAlt, OV_STAFF)
        cx.call(0x216cfc4)
        return ('adv',)
    if s == 0x1b9:
        if cx.call(0x216cfdc) == 0:
            return None
        cx.call(0x216cff0)
        cx.call(F.Overlay_Unload, -1)
        cx.call(F.GameHeap_LoadOverlayInfo, OV_STAFF)
        cx.call(F.BgLoad_CallHookPtr)
        return ('adv',)
    if s == 0x1ba:
        return _menu_after_reset(cx, 1)
    # ---- gallery page ----
    if s == 0x8d:                                      # confession events
        return _menu_after_reset(cx, 3) if cx.call(F.SelectScreen_Update) == 1 else None
    if s == 0xa1:                                      # dream events
        return _menu_after_reset(cx, 3) if cx.call(F.MainScene_RunStateMachine) == 1 else None
    if s == 0x118:                                     # illustrations (ov29)
        _overlay_branch(cx, OV_ILLUS)
        cx.call(F.Main_AdvanceStateSequence)
        cx.call(F.Overlay_Load, OV_ILLUS)
        cx.call(F.GameHeap_LoadOverlayInfoAlt, OV_ILLUS)
        return None
    if s == ILLUS:
        if cx.call(0x216b3f8) == 0:
            return None
        cx.call(F.Overlay_Unload, -1)
        cx.call(F.GameHeap_LoadOverlayInfo, OV_ILLUS)
        return _menu_after_reset(cx, 3)
    if s == 300:
        return _screen_branch(cx, 0xd)                 # gallery settings
    if s == ILLUS + 0x14:
        return None if cx.call(F.Screen_Run) == -1 else _menu_after_reset(cx, 3)
    if s == 0x208:                                     # trip events
        if not _fades_done(cx):
            return None
        cx.call(F.SaveSelect_Shutdown)
        return ('set', TRIPS)
    if s == TRIPS:
        if cx.call(0x20f444c) != 1:
            return None
        return _menu_after_reset(cx, 3)
    # ---- communication page: own card, friends' cards (no wireless needed) ----
    if s == 0x1cc:                                     # own card; a pending event pair plays its event first
        _overlay_branch(cx, OV_OWN_CARD)
        if cx.call(F.GameState_HasAsymmetricEventPair) == 0:
            cx.call(F.Main_AdvanceStateSequence)
            cx.call(F.GfxRes_CallHookPtr)
            cx.call(F.Schedule_AllocateAuxState)
            cx.call(F.Schedule_AllocateEventState)
            cx.call(F.TextRes_ProcessInputText, 6)
            cx.call(F.Overlay_Load, OV_OWN_CARD)
            cx.call(F.GameHeap_LoadOverlayInfoAlt, OV_OWN_CARD)
            return None
        cx.call(F.GfxRes_CallHookPtr)
        cx.call(F.GameState_SetSecondEventBits)
        cx.call(F.Overlay_Load, OV_PAIR_EVENT)
        cx.call(0x2161624)
        return ('set', cx.call(F.Main_GetStateAtDepth) + 3)
    if s == 0x1cd:
        if cx.call(0x218c048) == 0:
            return None
        cx.call(F.Overlay_Unload, -1)
        cx.call(F.GameHeap_LoadOverlayInfo, OV_OWN_CARD)
        cx.call(F.TextRes_FreeBuffer, 6)
        return ('adv',)
    if s in (0x1ce, 0x1e2):
        cx.call(F.Schedule_FreeEventState)
        cx.call(F.Schedule_FreeAuxState)
        cx.call(F.BgLoad_CallHookPtr)
        return _menu_after_reset(cx, 2)
    if s == 0x1cf:
        if cx.call(0x21613e8) == 0:
            return None
        cx.call(F.Overlay_Unload, OV_PAIR_EVENT)
        cx.call(F.Bg_Initialize, 0xd5, -1, -1, 0)
        cx.call(F.Bg_SelectState, 1, 1)
        cx.call(F.ScreenFade_ToClear, 1, 0, 0)
        cx.call(F.RandEvent_UpdateData_d9ce8, 10, 2, 0, -1)
        return ('adv',)
    if s == 0x1d0:
        if cx.call(F.RandEvent_SelectDataUpsilon_d9c7c) != 0:
            cx.call(F.RandEvent_UpdateData_d9d94, 10, 2)
            cx.call(F.ScreenFade_ToWhite, 1, 0)
            cx.call(F.Main_SetState, cx.call(F.Main_GetStateAtDepth) - 3)
            cx.call(F.Schedule_AllocateAuxState)
            cx.call(F.Schedule_AllocateEventState)
            cx.call(F.TextRes_ProcessInputText, 6)
            cx.call(F.Overlay_Load, OV_OWN_CARD)
            cx.call(F.GameHeap_LoadOverlayInfoAlt, OV_OWN_CARD)
            cx.call(F.Bg_ClearState)
        return None
    if s == 0x1e0:                                     # friends' cards (ov16)
        _overlay_branch(cx, OV_FRIENDS)
        cx.call(F.Main_AdvanceStateSequence)
        cx.call(F.GfxRes_CallHookPtr)
        cx.call(F.Schedule_AllocateAuxState)
        cx.call(F.Schedule_AllocateEventState)
        cx.call(F.TextRes_ProcessInputText, 6)
        cx.call(F.Overlay_Load, OV_FRIENDS)
        cx.call(F.GameHeap_LoadOverlayInfoAlt, OV_FRIENDS)
        return None
    if s == 0x1e1:
        if cx.call(0x2173844) == 0:
            return None
        cx.call(F.Overlay_Unload, -1)
        cx.call(F.GameHeap_LoadOverlayInfo, OV_FRIENDS)
        cx.call(F.TextRes_FreeBuffer, 6)
        return ('adv',)
    # ---- ご当地 page: characters ----
    if s == 0x21c:
        _overlay_branch(cx, OV_GOTOCHI_CHARA)
        cx.call(F.Main_AdvanceStateSequence)
        cx.call(F.GfxRes_CallHookPtr)
        cx.call(F.TextRes_ProcessInputText, 6)
        cx.call(F.Schedule_AllocateAuxState)
        cx.call(F.Schedule_AllocateEventState)
        cx.call(F.Overlay_Load, OV_GOTOCHI_CHARA)
        cx.call(F.GameHeap_LoadOverlayInfoAlt, OV_GOTOCHI_CHARA)
        return None
    if s == 0x21d:
        if cx.call(0x2178804) == 0:
            return None
        cx.call(F.Overlay_Unload, -1)
        cx.call(F.GameHeap_LoadOverlayInfo, OV_GOTOCHI_CHARA)
        cx.call(F.TextRes_FreeBuffer, 6)
        return ('adv',)
    if s == 0x21e:
        cx.call(F.Schedule_FreeEventState)
        cx.call(F.Schedule_FreeAuxState)
        cx.call(F.BgLoad_CallHookPtr)
        cx.call(F.Display_ResetBgAndVram)
        return ('menu', 4)
    # ---- gallery data transfer (ov10) ----
    if s == 0x230:
        _overlay_branch(cx, OV_GALLERY_XFER)
        cx.call(F.Overlay_Load, OV_GALLERY_XFER)
        cx.call(F.GameHeap_LoadOverlayInfoAlt, OV_GALLERY_XFER)
        cx.call(0x2170890)
        return ('adv',)
    if s == 0x231:
        if cx.call(0x2171910) != 0:
            return None
        cx.call(0x21709d4)
        cx.call(F.Overlay_Unload, -1)
        cx.call(F.GameHeap_LoadOverlayInfo, OV_GALLERY_XFER)
        cx.call(F.Display_ResetBgAndVram)
        return ('menu', 6)
    # ---- the save data cannot be used ----
    if s == 0x154:
        cx.call(F.SaveSelect_Shutdown)
        cx.call(F.Gfx_Apply2dOnlyBanksAndInitVramMgrs)
        for i in range(8):                             # the message's palette, copied to the stack
            cx.load(NO_SAVE_PALETTE + 4 * i, 2)
            cx.load(NO_SAVE_PALETTE + 4 * i + 2, 2)
        cx.call(F.GX_LoadBGPltt, None, 0, 2)
        cx.call(F.GXS_LoadBGPltt, None, 0, 2)
        cx.store(DISPCNT_A, (cx.load(DISPCNT_A) & ~0x1f00) | 0x1400)
        cx.store(DISPCNT_B, (cx.load(DISPCNT_B) & ~0x1f00) | 0x1000)
        cx.call(F.MsgBox_Open, NO_SAVE_TEXT, 2, 0, 0xf)
        cx.call(F.MsgBox_DrawAdvanceMarker, 0)
        for i, row in enumerate((7, 6, 5, 4, 3)):
            cx.call(F.MsgBox_PrintLine, i, row, 1)
        cx.call(F.Main_SetState, 0x168)
        cx.call(F.ScreenFade_ToClear, 1, 0x14, 0)
        cx.call(F.ScreenFade_ToClear, 2, 0x14, 0)
        return None
    if s == 0x50:                                      # (result 1) ov38
        cx.call(F.SaveSelect_Shutdown)
        cx.call(F.Overlay_Load, OV_MENU38)
        cx.call(F.GameHeap_LoadOverlayInfoAlt, OV_MENU38)
        cx.call(0x21708ac, 0)
        return ('adv',)
    for lo, hi in WIRELESS:
        if lo <= s < hi:
            raise NotPorted('SaveSelect state %#x (a wireless feature)' % s)
    return None


# ---- SceneLoop A (file select chain, screen 1) / B (file delete, screen 2) ----------------------------------------
SCROLL = W(0x209f4fc)              # 0x400001c: BG3 offsets of both engines, cleared


def loop_a_init(cx):
    cx.call(F.GfxRes_CallHookPtr)
    cx.call(F.Screen_Request, 0, 1, -1)
    cx.store(SCROLL, 0)
    cx.store(SCROLL + 0x1000, 0)


def loop_b_init(cx):
    cx.call(F.GfxRes_CallHookPtr)
    cx.call(F.Screen_Request, 0, 2, -1)
    cx.store(W(0x209f648), 0)
    cx.store(W(0x209f648) + 0x1000, 0)


def loop_a_run(cx):
    """1: backed out, 2: done (a game to start)"""
    ret = 0
    nxt = None
    cx.call(F.Main_IncStateDepth)
    s = cx.call(F.Main_GetStateAtDepth)
    if s == 0:
        cx.call(F.SceneLoop_InitializeSpriteSystemA)
        nxt = 0x14
    elif s == 0x14:
        r = cx.call(F.Screen_Run)
        nxt = {1: 0x1e, 2: 0x28}.get(r)
    elif s in (0x1e, 0x28):
        if _fades_done(cx):
            nxt = 0x32 if s == 0x1e else 0x3c
    elif s in (0x32, 0x3c):
        cx.call(F.SceneLoop_NopCallbackA)
        nxt = 0
        ret = 1 if s == 0x32 else 2
    if nxt is not None:
        cx.call(F.Main_SetState, nxt)
    cx.call(F.Main_DecStateDepth)
    return ret


def loop_b_run(cx):
    ret = 0
    nxt = None
    cx.call(F.Main_IncStateDepth)
    s = cx.call(F.Main_GetStateAtDepth)
    if s == 0:
        cx.call(F.SceneLoop_InitializeSpriteSystemB)
        nxt = 0x14
    elif s == 0x14:
        if cx.call(F.Screen_Run) == 1:
            nxt = 0x1e
    elif s == 0x1e:
        if _fades_done(cx):
            nxt = 0x32
    elif s == 0x32:
        cx.call(F.SceneLoop_NopCallbackB)
        nxt = 0
        ret = 1
    if nxt is not None:
        cx.call(F.Main_SetState, nxt)
    cx.call(F.Main_DecStateDepth)
    return ret
