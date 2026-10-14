"""The top of the game's flow: Main_ScopedFadeCheck 0x203fb20 (called every frame by GameHeap_UpdateGameState) and the
top state machine Mod204af08_RunGameStateMachine 0x204b0c0 (depth 2), with its boot overlay sequence 0x204afdc.
Ported from the game's code; constants in the code's literal pools are read from the game image (W).

States (see docs/GAME_FLOW.md): boot 0..3, title 10/11, title menu + file select 0x1e/0x1f/0x20, the room hub 0x28,
scenes 0x5a/0x5b, the per-activity states the room hands off to, and the day / date / overnight sequences."""
from .names import F, W
from . import screens

BASE = W(0x204c078)            # 0x10e: the date / overnight block of states
OV_TITLE = W(0x204c07c)        # 13
OV_TIME = W(0x204c080)         # 43: game-time reconciliation
DATE_ENTRY = W(0x204c084)      # 0x122
OV_DATE = W(0x204c088)         # 2: the date scene
OV_MINIGAME = W(0x204c08c)     # 37
OVERNIGHT = W(0x204c090)       # 0x136
OV_WEB = W(0x204c094)          # 36
SCOPED_BIT = W(0x204c098)      # 0xe9a
OV_MENU38 = W(0x204c09c)       # 38
OV_CLOCK = W(0x204c0a0)        # 20
DATE_BIT = W(0x204c0a4)        # 0xeb3
OV_OVERNIGHT = W(0x204c0a8)    # 41
BOOT_COUNTER = W(0x204b0b8)    # 0x21415b0
OV_BOOT = W(0x204b0bc)         # 0


def scoped_fade_check(cx):
    """0x203fb20: runs the top machine while the frame loop's own state is 0; 2 when the game shuts down"""
    cx.call(F.Main_IncStateDepth)
    r = 0
    if cx.call(F.Main_GetStateAtDepth) == 0 and cx.call(F.Mod204af08_RunGameStateMachine) < 0:
        r = 2
    cx.call(F.Main_DecStateDepth)
    return r


def boot_overlays(cx):
    """0x204afdc: load overlay 0 and run its three passes (6 states); 1 when done"""
    r = 0
    cx.call(F.Main_IncStateDepth)
    s = cx.call(F.Main_GetStateAtDepth)
    adv = False
    if s == 0:
        cx.store(BOOT_COUNTER, 0); adv = True
    elif s == 1:
        cx.call(F.Overlay_Load, OV_BOOT); cx.call(0x2161674); adv = True
    elif s == 2:
        adv = cx.call(0x2161528) != 0
    elif s == 3:
        cx.call(0x216168c, cx.load(BOOT_COUNTER)); adv = True
    elif s == 4:
        if cx.call(0x2161528) != 0:
            n = cx.load(BOOT_COUNTER)
            if n < 2:
                cx.store(BOOT_COUNTER, n + 1)
                cx.call(F.Main_SetState, 3)
            else:
                adv = True
    elif s == 5:
        cx.call(0x21616a8); adv = True
    elif s == 6:
        if cx.call(0x2161528) != 0:
            cx.call(F.Overlay_Unload, OV_BOOT)
            cx.call(F.SaveData_InvokeCallbackWithField1ec)
            r = 1
    if adv:
        cx.call(F.Main_AdvanceStateSequence)
    cx.call(F.Main_DecStateDepth)
    return r


def _saved(cx):
    """the persistent screen block's first word (the room's entry argument)"""
    return cx.call(F.Screen_GetSaved)


def run(cx):
    """0x204b0c0: one frame of the top machine; -1 when the game shuts down"""
    nxt = 0x28
    ret = 0
    cx.call(F.Main_IncStateDepth)
    s = cx.call(F.Main_GetStateAtDepth)
    act = _state(cx, s)
    if isinstance(act, tuple):                         # ('set', state) / ('ret', value[, state]) / ('quit',)
        if act[0] == 'set':
            cx.call(F.Main_SetState, act[1])
        elif act[0] == 'ret':
            ret = act[1]
            if len(act) > 2:
                cx.call(F.Main_SetState, act[2])
        elif act[0] == 'quit':
            ret = -1
    elif act == 'adv':
        cx.call(F.Main_AdvanceStateSequence)
    # every frame
    cx.call(F.GameClock_LoadState)
    cx.call(F.SceneRoute_ProcessClockTransition)
    cx.call(F.Transform_UpdateFrame)
    cx.call(F.GameFlags_AdvanceCalendarEvents)
    cx.call(F.MapCtl_UpdateTouchSelection)
    cx.call(F.SprFx_SelectDataMu_d2c38)
    cx.call(F.Main_DecStateDepth)
    return ret


def _room(cx, arg):
    """request the room (screen 0x11) with an entry argument and go to the hub"""
    cx.call(F.Screen_Request, 0, 0x11, arg)
    return ('set', 0x28)


def _clock_hook0(cx):
    if cx.call(F.GameState_GetField0x78) != 0:
        cx.call(F.GameClock_CallHookWith0)


def _state(cx, s):
    """the state's work; returns ('set', n), 'adv', ('quit',) or None"""
    # ---- boot and title ----
    if s == 0:
        cx.call(F.Gfx_ApplyDefaultBanksAndInitVramMgrs)
        cx.call(F.Main_AdvanceStateSequence)
        s = 1
    if s == 1:
        cx.call(F.Mod204af08_InitGameSystems)
        return 'adv'
    if s == 2:
        return 'adv' if cx.call(F.Mod204af08_StepOverlayLoadSequence) != 0 else None
    if s == 3:
        return ('set', 10) if cx.call(F.Transform_GetInputContextStatus) != 0 else None
    if 4 <= s <= 9:
        return None
    if s == 10:
        cx.call(F.Overlay_Load, OV_TITLE)
        cx.call(F.GameHeap_LoadOverlayInfoAlt, OV_TITLE)
        return 'adv'
    if s == 11:
        r = cx.call(0x216a9a8)                         # the title screen (ov13); its value is the machine's
        if r < 1:
            return ('ret', r)
        cx.call(F.Overlay_Unload, -1)
        cx.call(F.GameHeap_LoadOverlayInfo, OV_TITLE)
        return ('ret', r, 0x1e)
    if s == 0x1e:                                      # back to the title menu
        cx.call(F.Display_ResetBgAndVram)
        cx.call(F.Sprite_ClearAnimation)
        cx.call(F.Screen_Request, 0, 0, -1)
        return ('set', 0x1f)
    if s == 0x1f:                                      # title menu, file select, onboarding (SaveSelect)
        r = cx.call(F.SaveSelect_ProcessScreen)
        if r == -1:
            return ('set', 10)
        if r != 1:
            return None
        cx.call(F.FlowState_InitSingleton)
        if cx.call(F.GameFlags_Test, 0x20000000, 0) != 0:          # a new game: straight to the opening
            cx.call(F.GameFlags_Assign, 0x20000000, 0, 0)
            return ('set', 0x118)
        cx.call(F.UiChoice_ResetPrimary)
        return 'adv'
    if s == 0x20:                                      # continue: reconcile the game time with the clock
        if cx.call(F.UiChoice_DispatchInput) == 0:
            return None
        cx.call(F.UiChoice_ResetSecondary)
        cx.call(F.Transform_UpdateInputContext)
        cx.call(F.Overlay_Load, OV_TIME)
        nxt = DATE_ENTRY
        if cx.call(0x216ac5c) == 0:                    # ov43
            nxt = BASE if cx.call(F.GameState_GetField0x78) != 0 else 0x118
        cx.call(F.Main_SetState, nxt)
        cx.call(F.Overlay_Unload, OV_TIME)
        return None
    if 0x21 <= s <= 0x27:
        return None
    # ---- the room hub ----
    if s == 0x28:
        r = cx.call(F.Screen_Run)
        return _hub(cx, r)
    if s == 0x32:
        r = cx.call(F.UiScreen_RunStateMachine)
        return _room(cx, 10) if r in (1, 2) else None
    if s == 0x3c:
        r = cx.call(F.DebugMenu_Main)
        if r == 0:
            return ('set', 0x28)
        return ('set', 0x32) if r == 1 else None
    if s == 0x50:
        cx.call(F.Overlay_Load, OV_WEB)
        cx.call(F.GameHeap_LoadOverlayInfoAlt, OV_WEB)
        cx.call(0x216f07c)
        return 'adv'
    if s == 0x51:
        if cx.call(0x216f110) != 0:
            return None
        cx.call(0x216f0e4)
        cx.call(F.Overlay_Unload, -1)
        cx.call(F.GameHeap_LoadOverlayInfo, OV_WEB)
        return _room(cx, 10)
    if s == 0x5a:
        return _scene_end(cx, cx.call(F.Mod2049a04_RunMainStateMachine))
    if s == 0x5b:
        return ('set', 0x5a)
    if s == 100:
        if cx.call(0x216f858) == 0:
            return None
        cx.call(F.Overlay_Unload, -1)
        cx.call(F.GameHeap_LoadOverlayInfo, OV_MINIGAME)
        cx.call(F.Screen_Request, 0, 0x11, cx.load(_saved(cx)))
        cx.call(F.Transform_SetContextEnabled, 0)
        _clock_hook0(cx)
        return ('set', 0x28)
    if s == 0x6e:
        return _event_end(cx)
    if s in (0x82, 0x8c):
        r = cx.call(F.Menu_RunOptionScreenState if s == 0x82 else F.Settings_RunScreenState)
        return _room(cx, -1) if r != 0 else None
    if s == 0x96:
        cx.call(F.GameFlags_Assign, 0x10000000, 0, 1)
        cx.call(F.TextRes_FreeBuffer, 1)
        cx.call(F.BgLoad_CallHookPtr)
        cx.call(F.GfxRes_CallHookPtr)
        cx.call(F.TextRes_ProcessInputText, 2)
        cx.call(F.Overlay_Load, OV_MENU38)
        cx.call(F.GameHeap_LoadOverlayInfoAlt, OV_MENU38)
        cx.call(0x21708ac, 0)
        return 'adv'
    if s == 0x97:
        r = cx.call(0x21709f8)
        if r == 0:
            return None
        cx.call(0x21709a8)
        cx.call(F.Overlay_Unload, -1)
        cx.call(F.GameHeap_LoadOverlayInfo, OV_MENU38)
        cx.call(F.Transform_UpdateInputContext)
        cx.call(F.TextRes_FreeBuffer, 2)
        cx.call(F.BgLoad_CallHookPtr)
        cx.call(F.GfxRes_CallHookPtr)
        cx.call(F.TextRes_ProcessInputText, 1)
        cx.call(F.GameFlags_Assign, 0x10000000, 0, 0)
        cx.call(F.GameFlags_ClearSequenceState)
        cx.call(F.Screen_Request, 0, 0x11, 100)
        cx.call(F.Transform_SetContextEnabled, 0)
        if r == 1:
            return ('set', 0x28)
        if r != 2:
            return None
        cx.call(F.ScreenFade_ToClear, 1, 1, 0)
        cx.call(F.ScreenFade_ToClear, 2, 1, 0)
        cx.call(F.Schedule_AllocateStorage)
        slot = cx.call(F.GameState_GetField0x98)
        cx.call(F.GameState_GetField0x98)
        hook = cx.call(F.SaveSlot_CallStateHook)
        cx.call(F.Schedule_LoadStorage, 1, slot, hook)
        return ('set', 300)
    if s == 0xa0:
        r = cx.call(F.UiScreen_RunStateMachine)
        if r == 1:
            return _room(cx, 0x28)
        return _room(cx, 0x32) if r == 2 else None
    if s == 0xbe:
        cx.call(F.Overlay_Load, OV_CLOCK)
        cx.call(F.GameHeap_LoadOverlayInfoAlt, OV_CLOCK)
        return 'adv'
    if s == 0xbf:
        if cx.call(0x216a920) == 0:                    # ov20
            return None
        cx.call(F.Overlay_Unload, -1)
        cx.call(F.GameHeap_LoadOverlayInfo, OV_CLOCK)
        cx.call(F.SaveData_InitializeClockFields)
        return ('set', 0x1e)
    if s == 0xd2:
        return _date_start(cx)
    if s == 0xd3:
        if cx.call(F.ScreenFade_IsActive, 1) != 0 or cx.call(F.ScreenFade_IsActive, 2) != 0:
            return None
        cx.store(_saved(cx), -1)
        cx.call(0x216e790)
        cx.call(F.Sprite_ProcessAnimationFrame)
        cx.call(F.GameClock_CallHookWith1)
        return ('set', 0x5a)
    if s == 0xdc:
        if cx.call(F.Overlay_UpdateInitState) == 0:
            return None
        cx.call(F.Overlay_UnloadCleanup)
        cx.call(F.GameFlags_Assign, 0x1000000, 0, 1)
        cx.call(F.SaveData_InitializeClockFields)
        return ('set', 0x1f)
    if s == 0xe6:
        r = cx.call(F.UiScreen_RunStateMachine)
        if r not in (1, 2):
            return None
        cx.call(F.Script_WriteVariable, 0x26, 0 if r == 1 else 1)
        return ('set', 0x5a)
    if s == 0xf0:
        return _new_day(cx)
    if s == 0xfa:
        return _time_mode_change(cx)
    if s == 0x104:
        cx.call(F.Mod204af08_ShutdownGameSystems)
        return ('quit',)
    if s == BASE:                                      # 0x10e
        cx.call(F.Screen_Request, 0, 0xc, -1)
        return 'adv'
    if s == BASE + 1:
        return ('set', 0x118) if cx.call(F.Screen_Run) != -1 else None
    if s == 0x118:
        if cx.call(F.GameState_GetField0x7a) != 0:
            return 'adv'
        cx.call(F.UiInput_Reset)
        cx.call(F.Main_SetState, 0x5b)
        cx.store(_saved(cx), -1)
        return None
    if s == BASE + 0xb:                                # 0x119: clock synchronisation
        cx.call(F.GameFlags_Assign, 0x40000000, 0, 1)
        if cx.call(F.GameFlags_RunClockSynchronization) == 0:
            cx.call(F.Screen_Request, 0, 0x11, -1)
            n = 0x28
        else:
            cx.call(F.GameClock_CallHookWith1)
            n = 0x5b
        cx.call(F.Main_SetState, n)
        cx.call(F.GameFlags_Assign, 0x40000000, 0, 0)
        if cx.call(F.GameFlags_CallHookWithConst) != 0:
            cx.store(_saved(cx), 0x82)
        return None
    if s == DATE_ENTRY:                                # 0x122: a date
        cx.call(F.Overlay_Load, OV_DATE)
        cx.call(F.Mod2049a04_CallThreeExtFuncs)
        if cx.call(F.DateScene_Initialize) == 0:
            cx.call(F.Main_SetState, BASE)
            cx.call(F.Overlay_Unload, OV_DATE)
            return None
        return ('set', BASE + 0x15)
    if s == BASE + 0x15:
        if cx.call(F.DateScene_Update) == 0:
            return None
        cx.call(F.Overlay_Unload, OV_DATE)
        cx.call(F.Overlay_Load, OV_TIME)
        cx.call(0x216a920)                             # ov43
        return ('set', 0x124)
    if s == 0x124:
        if cx.call(0x216a938) == 0:
            return None
        cx.call(F.Overlay_Unload, OV_TIME)
        return ('set', BASE)
    if s == 300:
        if cx.call(F.Schedule_ProcessStorage) == 0:
            cx.call(F.Main_SetState, 0x96)
            cx.call(F.Schedule_UpdateStorage)
            cx.call(F.Schedule_FreeStorage)
            p = cx.call(F.GameState_GetPtrForSlotOrDefault, -1)
            cx.call(F.GameState_UpdateProgressUnlocks, p + 0x3a20)
        return None
    if s == OVERNIGHT:                                 # 0x136
        cx.call(F.Overlay_Load, OV_OVERNIGHT)
        cx.call(F.GameHeap_LoadOverlayInfoAlt, OV_OVERNIGHT)
        cx.call(0x216aa98, cx.call(F.MapArc_GetCurrentCharId), 0)
        return ('set', OVERNIGHT + 1)
    if s == OVERNIGHT + 1:
        if cx.call(0x216acbc) == 0:
            return None
        cx.call(0x216ace0)
        return ('set', OVERNIGHT + 2)
    if s == OVERNIGHT + 2:
        cx.call(F.Overlay_Unload, -1)
        cx.call(F.GameHeap_LoadOverlayInfo, OV_OVERNIGHT)
        cx.call(F.Screen_Request, 0, 0x11, cx.load(_saved(cx)))
        cx.call(F.Transform_SetContextEnabled, 0)
        return ('set', 0x28)
    return None


# what the room asks for (its screen result) -> the next state
def _hub(cx, r):
    if r == 0:
        cx.call(F.UiScreen_InitializeDateFields, 0, -1)
        return ('set', 0x32)
    if r == 1:
        return ('set', 0x5a)
    if r == 2:
        return ('set', 0x3c)
    if r == 3:
        return ('set', 0x50)
    if r == 4:                                         # back to the title menu
        cx.call(F.Display_ResetBgAndVram)
        cx.call(F.SaveData_InitializeClockFields)
        cx.call(F.Screen_Request, 0, 0, -1)
        return ('set', 0x1f)
    if r == 5:
        cx.call(F.Overlay_Load, OV_MINIGAME)
        cx.call(F.GameHeap_LoadOverlayInfoAlt, OV_MINIGAME)
        cx.call(F.Transform_SetContextEnabled, 0)
        cx.call(0x216f5e0, cx.call(F.MapArc_GetCurrentCharId), 0)
        return ('set', 100)
    if r == 6:
        cx.call(F.EventLogic_SelectEvent)
        return ('set', 0x6e)
    if r in (7, 8, 9, 0xd, 0xe):
        return ('set', {7: 0x82, 8: 0x8c, 9: 0x96, 0xd: 0xf0, 0xe: 0xfa}[r])
    if r == 10:
        p = cx.call(F.Screen_GetSaved)
        cx.call(F.UiScreen_InitializeDateFields, 1, cx.load(p + 0x127c) + 1)
        return ('set', 0xa0)
    if r == 11:
        return ('set', 0xbe)
    if r == 12:                                        # a date with the heroine
        cx.call(F.GfxRes_CallHookPtr)
        cx.call(F.TaskList_InitGlobalWithFnPtr)
        if cx.call(F.MapArc_GetCurrentMode) == 0x14:
            who = cx.call(F.GameState_GetSlotTableValue_0210e1ac)
        else:
            who = cx.call(F.MapArc_GetCurrentCharId)
        cx.call(F.GameClock_SaveState, who, 0)
        cx.call(F.GameClock_ClearUpdateCallback)
        return ('set', 0xd2)
    if r == 0xf:
        return ('set', OVERNIGHT)
    return None


# the scene machine's exit code -> the next state
def _scene_end(cx, code):
    if code == 0:
        return None
    if code == 1:
        cx.call(F.Transform_SetContextEnabled, 0)
        cx.call(F.UiScreen_InitializeDateFields, 4, -1)
        return ('set', 0xe6)
    if code == 2:
        cx.call(F.Transform_SetContextEnabled, 0)
        cx.call(F.Screen_Request, 0, 0x11, cx.load(_saved(cx)))
        cx.call(F.Main_SetState, 0x28)
        _clock_hook0(cx)
        return None
    if code == 5:
        cx.call(F.Transform_SetContextEnabled, 0)
        cx.call(F.Overlay_LoadInit)
        return ('set', 0xdc)
    if code == 6:
        cx.call(F.Main_SetState, 0x96)
        _clock_hook0(cx)
        if cx.call(F.GameFlags_CallHookWithConst) != 0:
            cx.call(F.GameFlags_Assign, 0, 0x20000, 1)
        return None
    if code == 7:
        cx.call(F.Main_SetState, OVERNIGHT)
        _clock_hook0(cx)
        return None
    if code in (3, 4, 8):
        cx.call(F.Transform_SetContextEnabled, 0)
        if cx.call(F.GameFlags_Test, 0, 0x10) == 0:
            cx.call(F.Screen_Request, 0, 0x11, cx.load(_saved(cx)))
            _clock_hook0(cx)
        else:
            cx.call(F.GameFlags_SetByte0x104, 0)
            cx.call(F.GameFlags_SetByte0x105, 0)
            cx.call(F.GameFlags_Assign, 0, 0x10, 0)
            cx.call(F.Screen_Request, 0, 4, 2)          # the schedule
        return ('set', 0x28)
    return None


def _event_end(cx):
    """0x6e: the event the room picked has run (EventLogic); where it leads"""
    if cx.call(F.EventLogic_RunEventStateMachine) == 0:
        return None
    cx.call(F.Transform_UpdateInputContext)
    k = cx.call(F.EventLogic_GetContextField)
    if (k == 0 or (k == 2 and cx.call(F.GameFlags_Test, 0x2000000, 0) == 0)) \
            and cx.call(F.GameFlags_Test, 0x100, 0) != 0:                  # into a scene
        cx.call(0x216e790)
        cx.call(F.Sprite_ProcessAnimationFrame)
        cx.call(F.StateSegment_UpdateStateDataPhi_dbf4c)
        cx.call(F.MapArc_SetCurrentCharId)
        cx.call(F.MapArc_SetCurrentMode, 0xe)
        v = cx.call(F.SaveSlot_GetField1c)
        cx.call(F.GameFlags_SetField0xc)
        cx.call(F.GameFlags_SetField0x0, v)
        cx.call(F.SaveSlot_SetField1c, -1)
        return ('set', 0x5a)
    if k in (1, 3) and cx.call(F.GameFlags_Test, 0x100, 0) != 0 \
            and cx.call(F.SaveSlot_GetCombinedFlag_Ofs0x39ff_3a00_3a02) != 0 \
            and cx.call(F.GameFlags_TestScopedBit, SCOPED_BIT) == 0 \
            and cx.call(F.GameState_GetSlotTableValue_0210e1a0) != 0:          # into a date
        cx.call(F.MapArc_SetCurrentCharId)
        cx.call(F.MapArc_SetCurrentMode, 0x14)
        cx.call(F.GameFlags_SetField0x102, 1)
        cx.call(F.GfxRes_CallHookPtr)
        cx.call(F.TaskList_InitGlobalWithFnPtr)
        cx.call(F.GameClock_SaveState, cx.call(F.GameState_GetSlotTableValue_0210e1a0), 0)
        cx.call(F.GameClock_ClearUpdateCallback)
        return ('set', 0xd2)
    if k == 5 and cx.call(F.GameFlags_Test, 0x100, 0) != 0:
        cx.call(F.SaveSlot_SetLowNibble_Ofs0x67, (cx.call(F.SaveSlot_GetLowNibble_Ofs0x67) + 1) & 0xff)
    if k == 6 and cx.call(F.GameFlags_Test, 0x100, 0) != 0:
        cx.call(F.SaveSlot_SetHighNibble_Ofs0x67, (cx.call(F.SaveSlot_GetHighNibble_Ofs0x67) + 1) & 0xff)
    cx.call(F.Sprite_SetField8, 4)
    cx.call(F.Sprite_SetFieldc, 2)
    _clock_hook0(cx)
    return ('set', 0x28)


def _date_start(cx):
    """0xd2: after the room picked a date (or an event led to one)"""
    if cx.call(F.GameClock_ClearRtcCache) != 0:
        return None
    cx.call(F.TaskList_ClearGlobalPtr)
    cx.call(F.Transform_UpdateInputContext)
    if cx.call(F.MapArc_GetCurrentMode) == 0x14:
        if cx.call(F.Transform_GetContextStatePlusOne) == 1:
            if cx.call(F.GameFlags_GetField0x102) == 0:
                cx.call(F.GameState_RequestFlaggedSlotTransition)
            else:
                cx.call(F.GameState_RequestSlotTransitionIfCond)
            cx.call(F.MapArc_SetCurrentCharId)
            cx.call(F.MapArc_SetCurrentMode, 0x14)
            cx.call(F.ScreenFade_ToWhite, 1, 10)
            cx.call(F.ScreenFade_ToWhite, 2, 10)
            cx.call(F.UiFlow_ThunkTertiary, 0x1e)
            cx.call(F.Main_AdvanceStateSequence)
        else:
            cx.call(F.Sprite_SetField8, 4)
            cx.call(F.Sprite_SetFieldc, 2)
            cx.call(0x216e97c)
            cx.call(F.GameFlags_SetScopedBit, DATE_BIT, 0)
            cx.call(F.MapArc_SetCurrentMode, 0)
            d = -0x1a if cx.call(F.GameFlags_GetField0x102) == 0 else -0x19
            cx.call(F.GameFlags_SetScopedBit, DATE_BIT + d, 1)
            cx.call(F.Main_SetState, 0x28)
        cx.call(F.Gfx_GetBgScreenSize, 1, 3)
        cx.call(F.GameFlags_SetField0x102, 0)
    else:
        cx.call(F.Sprite_SetField8, 4)
        cx.call(F.Sprite_SetFieldc, 2)
        cx.call(0x216e97c)
        cx.call(F.Main_SetState, 0x28)
    _clock_hook0(cx)
    cx.call(F.BgLoad_CallHookPtr)
    cx.call(F.Transform_SetContextEnabled, 0)
    return None


def _new_day(cx):
    """0xf0"""
    cx.call(F.Schedule_UpdateDataPhi_d84c8)
    cx.call(F.GameFlags_HandleTimeModeChange, 1, 0)
    cx.call(F.Schedule_CacheCurrentSlot)
    p = cx.call(F.Screen_GetSaved)
    n = cx.call(F.Screen_GetSavedSize)
    cx.call(F.memset, p, 0, n)
    cx.call(F.Screen_Request, 0, 0x11, -1)
    d = cx.out(4)
    cx.call(F.GameClock_GetDate, None)
    cx.call(F.GameFlags_ComputeField0x104, d[0], d[1], d[2], d[3], 1)
    cx.call(F.Schedule_UpdateTimeDataRho_d7f70, 0)
    cx.call(F.RandEvent_InvokeCallbackData_d9fc8)
    cx.call(F.Sprite_ClearContext)
    cx.call(F.Sprite_SetField20, 0x1e)
    cx.call(F.RandEvent_UpdateData_d9fd4, 0)
    cx.call(F.RandEvent_UpdateData_d9ffc, 0)
    cx.call(F.RandEvent_UpdateData_da024, 0)
    cx.call(F.GameFlags_Assign, 0, 0x1000, 1)
    return ('set', 0x28)


def _time_mode_change(cx):
    """0xfa: the clock moved (time mode change); the day's data is rebuilt, the same day keeps its schedule"""
    cx.call(F.GameFlags_HandleTimeModeChange, 0, 0)
    p = cx.call(F.Screen_GetSaved)
    n = cx.call(F.Screen_GetSavedSize)
    cx.call(F.memset, p, 0, n)
    cx.call(F.Screen_Request, 0, 0x11, -1)
    a = cx.out(4)
    cx.call(F.GameFlags_CopyVec4_Ofs0xb8_0xc8, None, 0)
    b = cx.out(4)
    cx.call(F.GameClock_GetDate, None)
    same_day = None if a[0] is None else a[:3] == b[:3]
    if cx.pick(same_day, {True: F.Schedule_UpdateDataUpsilon_d83e0, False: F.Schedule_UpdateTimeDataRho_d7f70}):
        cx.call(F.Schedule_UpdateDataUpsilon_d83e0, 0)
    else:
        cx.call(F.Schedule_UpdateTimeDataRho_d7f70, 0)
    cx.call(F.GameFlags_ComputeField0x104, b[0], b[1], b[2], b[3], 0)
    cx.call(F.RandEvent_InvokeCallbackData_d9fc8)
    cx.call(F.Sprite_ClearContext)
    cx.call(F.Sprite_SetField20, 0x1e)
    return ('set', 0x28)
