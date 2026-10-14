"""The scene machine Mod2049a04_RunMainStateMachine 0x204a0c4 (top state 0x5a, depth 3): enters a scene (the map's
active character and mode decide which script runs), plays it through the scene overlay (ov02 0x2158000..: its
polls decide where the scene goes next), and, when it ends, does the end-of-scene bookkeeping and returns an exit
code to the top machine:
    1 the flow is in mode 0xc (a UI choice follows)      2 a normal end (back to the room)
    3 flag 0x80000   4 flag 0x100000   8 flag (0, 0x100)   5 the map mode is 0xb   6 the time hook asks for ov38
States: 0..2 enter (0 also saves the clock), 10 re-enter, 0x14..0x16 start, 0x1e playing, 0x1f, 0x28 clock widget,
0x29, 0x32 / 0x33 / 0x3c the end, 0x46 / 0x50 / 0x5a / 100 the scene's sub-screens."""
from .names import F, W

P = W(0x204aef4)                # 0x213fcb4: the scene machine's block (+8 heroine, +0xc map kind, +0x10, +0x14, +0x1c, +0x20)
NONE16 = W(0x204aef8)           # 0xffff
WHITE = W(0x204aefc)            # 0x7fff: the sub screen's backdrop colour for the sub-screens
OV_SUB = W(0x204af00)           # 0x13
LAST_STATE = W(0x204af04)       # 0x212420c: the machine's state, kept for others


def _s8(v):
    v &= 0xff
    return v - 0x100 if v & 0x80 else v


def _s16(v):
    v &= 0xffff
    return v - 0x10000 if v & 0x8000 else v


def run(cx):
    code = 0
    cx.call(F.Main_IncStateDepth)
    s = cx.call(F.Main_GetStateAtDepth)
    act = None
    if s > 0x28:
        if s == 0x33:
            if cx.call(F.ScreenFade_IsActive, 1) == 0 and cx.call(F.ScreenFade_IsActive, 2) == 0:
                code = _end(cx)
        elif s == 0x29:
            if cx.call(F.GameClock_ClearManualTime, cx.load(P + 0x1c)) == 0:
                cx.store(P + 0x1c, 0)
                act = ('set', 0x1e)
        elif s == 0x32:
            act = _closing(cx)
        elif s == 0x3c:
            if not (cx.call(F.ScreenFade_IsActive, 1) != 0 or
                    (cx.call(F.MapArc_GetCurrentMode) == 0xb and cx.call(F.ScreenFade_IsActive, 2) != 0)):
                cx.call(F.Mod2049a04_TeardownSceneIfMapState0xb_0x14)
                act = ('set', 10)
        elif s == 0x46:
            if cx.call(0x215847c) == 5:
                cx.call(0x2158298)
                act = ('set', 0x1e)
        elif s == 0x50:
            r = cx.call(0x2159c84, cx.load(P))
            if r == 0x1e:
                cx.call(0x21598d4)
                if cx.call(F.Bg_ComputeTimedValuePrimary) != 0 and cx.call(F.Camera_GetAreaForCurrentPreset) == 0:
                    cx.call(F.Bg_ResetAuxiliary, 1)
                cx.call(F.UiLists_ClearBoth)
                cx.call(F.GameFlags_Assign, 0x800000, 0, 0)
                cx.call(F.GameFlags_SetField0x110, 0)
                act = ('set', 0x1e)
        elif s == 0x5a:
            if cx.call(0x215a148, cx.load(P)) == 0x1e:
                cx.call(0x21598d4)
                if cx.call(F.Overlay_IsLoaded, OV_SUB) != 0:
                    cx.call(F.Overlay_Unload, OV_SUB)
                    cx.call(F.GameHeap_LoadOverlayInfo, OV_SUB)
                act = ('set', 0x1e)
        elif s == 100:
            if cx.call(0x2158584) == 5:
                cx.call(0x2158298)
                act = ('set', 0x1e)
    elif s == 0x28:
        r = cx.call(F.ClockWidget_RunStateMachine)
        if r in (2, 3, 4, 5):
            cx.call(F.GameFlow_InvokeCallbackMode8, 0)
            if r == 5:
                act = ('set', 0x1e)
    elif s == 0x16:
        if cx.call(F.ScreenFade_IsActive, 1) == 0:
            act = _start(cx)
    elif s == 0x1e:
        act = _playing(cx)
    elif s == 0x1f:
        act = ('set', 0x32)
    elif 0 <= s <= 2 or s == 10:
        if s == 0:
            cx.call(F.Mod2049a04_EnterSceneSaveClock)
            cx.call(F.Main_AdvanceStateSequence)
        if s in (0, 1):
            cx.call(F.Main_AdvanceStateSequence)
        if s == 10:
            _reenter(cx)
        else:
            _enter(cx)
        act = ('set', 0x14)
    elif s == 0x14:
        cx.call(F.Main_AdvanceStateSequence)
        cx.call(F.Main_AdvanceStateSequence)
    elif s == 0x15:
        cx.call(F.Main_AdvanceStateSequence)
    if act:
        cx.call(F.Main_SetState, act[1])
    # every frame
    cx.call(F.GameFlags_ConfigureField0x90)
    cx.store(LAST_STATE, cx.call(F.Main_GetStateAtDepth))
    if cx.call(F.MapArc_GetCurrentMode) != 0xb:
        cx.call(F.DateUi_SelectNightStyle)
    cx.call(F.Bg_DispatchState)
    cx.call(F.Main_DecStateDepth)
    return code


def _selection(cx, mode, sel, kind, second_lookup_ok):
    """Schedule_DispatchEventLoad and the map selection made from it -> (selection struct, flag)"""
    v = cx.call(F.Schedule_DispatchEventLoad, mode, sel)
    lo, b2, b3 = _s16(v), _s8(v >> 16), _s8(v >> 24)
    if mode == 0xb:
        return (300, b2, b3), 1
    ident = lo
    if kind == 0x10:
        if cx.call(F.StateLogic_CalculateData_dd2e8, 0x10, b2) != NONE16:
            ident = cx.call(F.StateLogic_CalculateData_dd2e8)
    elif second_lookup_ok and kind == 0x11:
        return (0xd5, b2, b3), 0
    elif second_lookup_ok and mode == 0x14:
        ident = 0x8e
    return (ident, b2, b3), 0


def _enter(cx):
    """states 0..2: set the scene up from the map's active character"""
    cx.call(F.Gfx_SetBgVisibleFlag, 1, 0, 1)
    cx.call(F.Gfx_SetBgVisibleFlag, 1, 0, 0)
    kind_o, ptr_o = cx.out(1), cx.out(1)
    kind_o[0], ptr_o[0] = 0, -1
    cid = cx.call(F.MapArc_GetActiveCharIdAndMode, 0, kind_o, ptr_o)
    kind, ptr = kind_o[0], ptr_o[0]
    if ptr is not None and ptr & 0xffffffff == 0xffffffff:
        ptr = 0
    cx.call(F.GameState_SetPointer, ptr)
    if cx.call(F.GameFlow_GetStateField4) == 9:
        cx.call(F.GameFlags_GetField0x0)
        cx.call(F.TextRes_UpdateRandomSelection)
    if cid == 0:
        cx.call(F.ScreenFade_ToClear, 1, 10, 0)
        cx.call(F.ScreenFade_ToClear, 2, 10, 0)
    else:
        mode = cx.call(F.MapArc_GetCurrentMode)
        sel = cid
        if cx.call(F.GameFlags_Test, 1, 0) != 0:
            mode = 1
            if cx.call(F.GameFlow_GetStateField4) == 9:
                sel = cx.call(F.TextRes_GetRandomSelection)
        selection, flag = _selection(cx, mode, sel, kind, True)
        cx.call(F.Mod2049a04_ApplyMapSelection, selection, flag)
        g = cx.call(F.GameFlags_GetField0x0)
        cx.call(F.GameFlags_PushField0x4)
        cx.call(F.GameFlags_SetField0x0, -1)
        cx.call(F.DayEvent_GetResultIdForSlotEvent, kind, g)
        cx.call(F.GameSnd_PlaySceneAudio, kind, 0, 0x69)
        if cx.call(F.GameFlags_Test, 1, 0) == 0 or cx.call(F.GameFlow_GetStateField4) != 9:
            if kind == 0xe:
                cx.call(F.Schedule_IncrementPrimaryNormalCounters, g)
        else:
            cx.call(F.Schedule_IncrementPrimaryCounters, g)
        if kind == 0x10 and cx.call(F.StateLogic_ClearDataPi_dd750, cid, ptr) != 0:
            cx.call(F.GameSnd_SetSceneAudioLock, 3)
            cx.call(F.GameSnd_PlaySceneAudio, -1, 0, 0x69)
        cx.call(F.ColorMath_ComputeRampWrapper)
        if kind == 0xe:
            cx.call(F.FlowState_SetFlag0x20AndNotify)
        cx.call(F.UiMain_UpdateMode, kind, 0)
        if cx.call(F.GameFlow_GetStateField4) != 0xc:
            per = cx.call(F.TimePeriod_FromHour, cx.call(F.GameClock_GetHour))
            y = cx.call(F.GameClock_GetYear)
            m = cx.call(F.GameClock_GetMonth)
            d = cx.call(F.GameClock_GetDay)
            cx.call(F.StateLogic_UpdateDataTau_dc9f8, y, m, d, per, kind, cid)
        if kind is not None and 0 <= kind - 0x17 < 2:
            cx.call(F.ItemDb_StoreQueuedValue, cid, 0)
        if kind in (1, 0xe):
            cx.call(F.EventLogic_SetRecordFlagPrimary, 0x400, 1)
            cx.call(F.SaveSlot_ClearWord_Ofs0x50)
    if cx.call(F.Mod2049a04_IsIdInTables, cid) != 0:
        cx.call(F.Transform_SetContextEnabled, 1)


def _reenter(cx):
    """state 10: the next scene of a chain"""
    cx.call(F.Mod2049a04_ResetSceneSystems)
    kind_o, ptr_o = cx.out(1), cx.out(1)
    kind_o[0], ptr_o[0] = 0, -1
    cid = cx.call(F.MapArc_GetActiveCharIdAndMode, 0, kind_o, ptr_o)
    kind = kind_o[0]
    cx.call(F.GameState_SetPointer, ptr_o[0])
    if cx.call(F.GameFlow_GetStateField4) == 9:
        cx.call(F.GameFlags_GetField0x0)
        cx.call(F.TextRes_UpdateRandomSelection)
    if cid == 0:
        cx.call(F.ScreenFade_ToClear, 1, 10, 0)
        cx.call(F.ScreenFade_ToClear, 2, 10, 0)
        return
    mode = cx.call(F.MapArc_GetCurrentMode)
    if cx.call(F.GameFlags_Test, 1, 0) != 0:
        mode = 1
        if cx.call(F.GameFlow_GetStateField4) == 9:
            cid = cx.call(F.TextRes_GetRandomSelection)
    selection, flag = _selection(cx, mode, cid, kind, False)
    cx.call(F.Mod2049a04_ApplyMapSelection, selection, flag)
    g = cx.call(F.GameFlags_GetField0x0)
    if g != -1:
        cx.call(F.GameFlags_PushField0x4)
        cx.call(F.DayEvent_GetResultIdForSlotEvent, mode, g)
        if cx.call(F.GameFlags_Test, 1, 0) == 0 or cx.call(F.GameFlow_GetStateField4) != 9:
            if kind == 0xe:
                cx.call(F.Schedule_IncrementPrimaryNormalCounters, g)
        else:
            cx.call(F.Schedule_IncrementPrimaryCounters, g)
    cx.call(F.GameFlags_SetField0x0, -1)
    cx.call(F.GameSnd_PlaySceneAudio, kind, 0, 0x69)
    cx.call(F.ColorMath_ComputeRampWrapper)
    cx.call(F.UiMain_UpdateMode, kind, 0)


def _start(cx):
    """0x16: the scene's character and music; no character -> straight to the end"""
    kind_o, ptr_o = cx.out(1), cx.out(1)
    cid = cx.call(F.MapArc_GetActiveCharIdAndMode, 0, kind_o, ptr_o)
    cx.call(F.GameState_SetPointer, ptr_o[0])
    cx.call(F.GameFlags_SetField0x98, -1)
    cx.call(F.MapArc_ClearCurrentCharId)
    cx.call(F.MapArc_SetField34, cid)
    cx.call(F.MapArc_SetBgmId, kind_o[0])
    if cid == 0:
        return ('set', 0x32) if cx.call(F.FadeState_InvokeCallbackA) != 0 else None
    cx.store(P + 8, cid)
    cx.store(P + 0xc, kind_o[0])
    cx.call(F.GameClock_SaveState, cid, 0)
    cx.store(P + 0x14, 0)
    return ('set', 0x1e)


def _playing(cx):
    """0x1e: the scene runs; the scene overlay's polls open its sub-screens (0x46 / 0x50 / 0x5a / 100) or the clock
    widget (0x28)"""
    if (cx.call(F.GameClock_ClearManualTime, cx.load(P + 8)) == 0 and
            cx.call(F.GameClock_ClearManualTime, cx.load(P + 0x10)) == 0 and
            cx.call(F.GameClock_ClearManualTime, cx.load(P + 0x20)) == 0):
        cx.store(P + 8, 0)
        cx.store(P + 0x10, 0)
        cx.call(F.Main_AdvanceStateSequence)                       # the scene is over: 0x1f
    elif cx.call(F.GameFlags_Test, 0x80000000, 0) != 0:
        cx.call(F.Mod2049a04_ResetClockScriptSoundFade)
        cx.call(F.Main_SetState, 0x32)
    if cx.call(F.ClockWidget_UpdateStateB) != 0:
        return ('set', 0x28)
    if cx.call(0x21582f4) != 0:
        cx.call(0x21583e4, 1)
        return ('set', 0x46)
    if cx.call(0x215836c) != 0:
        _sub_backdrop(cx)
        cx.call(0x21583e4, 0)
        return ('set', 100)
    if cx.call(0x2159a6c) != 0:
        cx.call(F.GameFlags_Assign, 0x80000000, 0, 0)
        cx.call(F.UiSelection_SetValue, 0)
        cx.call(0x2159888, -1)
        cx.call(0x2159a98, 1)
        return ('set', 0x50)
    if cx.call(0x2159a88) != 0:
        _sub_backdrop(cx)
        cx.call(0x2159a98, 0)
        return ('set', 0x5a)
    return None


def _sub_backdrop(cx):
    cx.call(F.UiLists_ClearSub)
    cx.call(F.TransitionBg_ClearVramBanks)
    cx.call(F.GXS_LoadBGPltt, None, 0, 2)              # the colour WHITE from the stack
    cx.call(F.Gfx_GetBgScreenSize, 2, 1)


def _closing(cx):
    """0x32: fade out; a scene that leads on (0x33) or one that re-enters (0x3c -> 10)"""
    cx.call(F.Projection_SetCallback, 0)
    if cx.call(F.GameFlags_Test, 1, 0) == 0:
        if cx.call(F.GameFlags_SetBitRange) != 0:
            if cx.call(F.StateLogic_UpdateData_dd4dc) == 0:
                return _fade_both_and_advance(cx)
            cx.call(F.ScreenFade_ToBlack, 1, 10)
            return ('set', 0x3c)
        if cx.call(F.MapArc_GetCurrentCharId) == 0:
            return _fade_both_and_advance(cx)
        cx.call(F.ScreenFade_ToBlack, 1, 10)
        if cx.call(F.MapArc_GetCurrentMode) == 0xb:
            cx.call(F.ScreenFade_ToBlack, 2, 10)
        return ('set', 0x3c)
    if cx.call(F.GameFlow_GetStateField4) == 0x12 or cx.call(F.GameFlow_GetStateField4) == 0xc:
        return _fade_both_and_advance(cx)
    cx.call(F.ScreenFade_ToBlack, 1, 10)
    return ('set', 0x3c)


def _fade_both_and_advance(cx):
    cx.call(F.ScreenFade_ToBlack, 1, 10)
    cx.call(F.ScreenFade_ToBlack, 2, 10)
    cx.call(F.Main_AdvanceStateSequence)
    return None


def _end(cx):
    """0x33: the scene is over; bookkeeping and the exit code"""
    if cx.call(F.GameFlags_Test, 0x80000000, 0) != 0:
        if cx.call(F.GameFlags_Test, 2, 0) != 0:
            cx.call(F.EventAssets_CheckLoaded)
            cx.call(F.EventAssets_SetPrimary)
            cx.call(F.BgLoad_CallHookPtr)
        cx.call(F.Transform_UpdateInputContext)
    if cx.call(F.MapArc_GetCurrentMode) == 0x1c:
        d = cx.out(4)
        cx.call(F.GameClock_GetDate, d)
        per = cx.call(F.TimePeriod_FromHour, cx.call(F.GameClock_GetHour))
        if cx.call(F.Schedule_TestSecondaryDateAndFlag, None, per) != 0:
            cx.call(F.Schedule_IncrementSecondaryCounters)
            cx.call(F.Schedule_SubtractSecondaryField0x04By16)
            cx.call(F.EventLogic_DispatchTrigger, 0x5d, -1)
            cx.call(F.SaveSlot_UpdateRecordState, 0xd, -1)
    cx.call(F.Mod2049a04_TeardownSceneSystems)
    if cx.call(F.GameFlow_GetStateField4) == 0xc:
        code = 1
    else:
        cx.call(F.GameFlags_UpdateField0xa4)
        code = 2
        if cx.call(F.GameFlags_Test, 4, 0) != 0:
            cx.call(F.SaveCheck_UpdateStateTimerExtended)
            if cx.call(F.GameState_GetField0x78) != 0 and cx.call(F.GameFlags_GetField0xe0) == 0:
                cx.call(F.RandEvent_UpdateData_d9ce8, 0x31, 1, 0, -1)
        if cx.call(F.MapArc_GetCurrentMode) == 0xe:
            cx.call(F.SaveCheck_ResetCounter)
            cx.call(F.FlowState_ClearFlag0x20)
        if cx.call(F.GameFlags_Test, 0x100000, 0) == 0:
            if cx.call(F.GameFlags_Test, 0, 0x100) != 0:
                cx.call(F.Callback_InvokeWith0100)
                code = 8
        else:
            cx.call(F.StateLogic_CalculateDataMu_dd5d8)
            code = 4
        if cx.call(F.GameFlags_Test, 0x80000, 0) != 0:
            cx.call(F.GameFlags_Assign, 0x80000, 0, 0)
            code = 3
        if cx.call(F.MapArc_GetCurrentMode) == 0xb:
            code = 5
        if cx.call(F.MapArc_GetCurrentMode) == 6 and cx.call(F.GameClock_GetTimePeriod) == 3:
            cx.call(F.Schedule_GetDataPsi_d85cc, 0, -1, 3)      # (from a stack struct: 0, -1, period 3)
        if cx.call(F.MapArc_GetCurrentMode) == 0xd:
            low = (cx.call(F.Stats_GetBase_Ofs0x2b5) & 0xffffffff) < 0x81
            cx.call(F.EventLogic_SetRecordFlagPrimary, 0x20, 0 if low else 1)
            cx.call(F.EventLogic_SetRecordFlagPrimary, 0x40, 1 if low else 0)
        if cx.call(F.MapArc_GetCurrentMode) == 0x1b:
            cx.call(F.SaveSlot_UpdateRecordState, 0xc, -1)
            cx.call(F.GameState_SetBitFlag, 0x37, 1)
        cx.call(F.MapArc_GetCurrentMode)
        if code not in (4, 8, 3) and cx.call(F.GameFlags_CallHookWithConst) != 0:
            if cx.call(F.GameClock_GetTimePeriod) == 5 or cx.call(F.GameClock_GetTimePeriod) == 0:
                code = 2
            elif cx.call(F.GameFlags_Test, 0, 1) == 0:
                code = 6
            else:
                code = 2
                cx.call(F.GameFlags_Assign, 0, 1, 0)
        if cx.call(F.MapArc_GetCurrentMode) in (8, 0x17, 0x18):
            cx.call(F.EventQueue_BuildOutput)
        cx.call(F.Transform_SetContextEnabled, 0)
        if code not in (4, 8) and cx.call(F.GameFlow_GetStateField4) != 0xc:
            cx.call(F.GameState_ResetToTitleAudio)
            cx.call(F.StateLogic_UpdateData_dd028)
        bgm = cx.call(F.MapArc_GetBgmId)
        if 0 <= bgm - 0x17 < 2:
            cx.call(F.GameClock_CallHookGetWeekday)
            cx.call(F.GameClock_CallHookGetHour)
            cx.call(F.GameClock_CallHookGetMinute)
            cx.call(F.GameClock_CallHookGetSecond)
            cx.call(F.GameClock_CallHookGetYear)
            cx.call(F.GameClock_CallHookGetMonth)
            cx.call(F.GameClock_CallHookGetDay)
            e = cx.call(F.Schedule_GetDateEntry, None, 0)
            cx.call(F.Schedule_GetFilteredCount, e, None, 0)
            cx.call(F.ItemDb_IncrementQueueIndex)
            cx.call(F.SaveSlot_UpdateRecordState, 0xe, -1)
            if bgm == 0x18:
                cx.call(F.GameFlags_Assign, 0, 0x80, 0)
        cx.call(F.SaveSlot_CloseStorage)
        if bgm in (1, 0xe):
            cx.call(F.SaveSlot_ProcessStateRecord, -1)
            cx.call(F.StateLogic_GetDataChi_cc3bc)
        if cx.call(F.SaveData_IsRecord0Used) == 0:
            for i in range(3):
                cx.call(F.SaveData_EncodeRecord, i)
    cx.call(F.MapArc_SetCurrentMode, 0)
    cx.call(F.GameFlags_Assign, 0x80000000, 0, 0)
    cx.call(F.UiSelection_SetValue, 1)
    return code
