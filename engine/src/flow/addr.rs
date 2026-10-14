//! Game function addresses the flow machines call, by their label (fid/labels_all.csv; generated once from the names
//! the Python port uses). Labels that turned out wrong carry a corrected name; the original label is noted.
#![allow(non_upper_case_globals, dead_code)]

pub const GX_LoadBGPltt: u32 = 0x201b4c4;
pub const GXS_LoadBGPltt: u32 = 0x201b528;
pub const memset: u32 = 0x202cb10;
pub const Main_IncStateDepth: u32 = 0x203dba4;
pub const Main_DecStateDepth: u32 = 0x203dbbc;
pub const Main_GetStateAtDepth: u32 = 0x203dbd4;
pub const Main_SetState: u32 = 0x203dbf0;   // labelled Main_FreeStateBuffer
pub const Main_AdvanceStateSequence: u32 = 0x203dc58;
pub const GameHeap_LoadOverlayInfo: u32 = 0x203ff24;
pub const GameHeap_LoadOverlayInfoAlt: u32 = 0x203ff9c;
pub const Display_SetLcdScreenSwap: u32 = 0x20414bc;
pub const Gfx_ApplyDefaultBanksAndInitVramMgrs: u32 = 0x2041de0;
pub const Gfx_Apply2dOnlyBanksAndInitVramMgrs: u32 = 0x2041e1c;
pub const Gfx_SetBgVisibleFlag: u32 = 0x2043084;
pub const Gfx_GetBgScreenSize: u32 = 0x2044514;
pub const ScreenFade_ToBlack: u32 = 0x20451f8;
pub const ScreenFade_ToWhite: u32 = 0x2045250;
pub const ScreenFade_ToClear: u32 = 0x20452a4;
pub const ScreenFade_IsActive: u32 = 0x2045318;
pub const GfxRes_CallHookPtr: u32 = 0x2047d40;
pub const BgLoad_CallHookPtr: u32 = 0x2047d4c;
pub const Mod2049a04_ApplyMapSelection: u32 = 0x2049994;
pub const Mod2049a04_IsIdInTables: u32 = 0x2049a4c;
pub const Mod2049a04_EnterSceneSaveClock: u32 = 0x2049aa4;
pub const Mod2049a04_ResetSceneSystems: u32 = 0x2049d3c;
pub const Mod2049a04_TeardownSceneSystems: u32 = 0x2049dfc;
pub const Mod2049a04_CallThreeExtFuncs: u32 = 0x2049f98;
pub const Mod2049a04_TeardownSceneIfMapState0xb_0x14: u32 = 0x2049fd4;
pub const Mod2049a04_ResetClockScriptSoundFade: u32 = 0x204a05c;
pub const Mod2049a04_RunMainStateMachine: u32 = 0x204a0c4;
pub const Mod204af08_InitGameSystems: u32 = 0x204af34;
pub const Mod204af08_ShutdownGameSystems: u32 = 0x204afb8;
pub const Mod204af08_StepOverlayLoadSequence: u32 = 0x204afdc;
pub const Mod204af08_RunGameStateMachine: u32 = 0x204b0c0;
pub const Script_WriteVariable: u32 = 0x204c7a8;
pub const ResTable_Open: u32 = 0x204e644;
pub const MapCtl_UpdateTouchSelection: u32 = 0x204f664;
pub const Camera_GetAreaForCurrentPreset: u32 = 0x20511e8;
pub const Transform_UpdateFrame: u32 = 0x2051fc4;
pub const Transform_UpdateInputContext: u32 = 0x20521bc;
pub const Transform_GetInputContextStatus: u32 = 0x20521e0;
pub const Transform_GetContextStatePlusOne: u32 = 0x205245c;
pub const Transform_SetContextEnabled: u32 = 0x2052568;
pub const Projection_SetCallback: u32 = 0x2052738;
pub const GameFlags_Assign: u32 = 0x2053620;
pub const GameFlags_Test: u32 = 0x2053680;
pub const GameFlags_SetBitRange: u32 = 0x20536c0;
pub const GameFlags_HandleTimeModeChange: u32 = 0x20536d4;
pub const GameFlags_GetField0xe0: u32 = 0x20537f0;
pub const GameFlags_SetField0x0: u32 = 0x2053e68;
pub const GameFlags_GetField0x0: u32 = 0x2053e7c;
pub const GameFlags_PushField0x4: u32 = 0x2053e90;
pub const GameFlags_SetField0xc: u32 = 0x2053ec4;
pub const GameFlags_SetField0x98: u32 = 0x2053f88;
pub const GameFlags_AdvanceCalendarEvents: u32 = 0x2055734;
pub const GameFlags_RunClockSynchronization: u32 = 0x2055890;
pub const SaveData_InitializeClockFields: u32 = 0x2055e04;
pub const GameFlags_ConfigureField0x90: u32 = 0x20568e8;
pub const GameFlags_UpdateField0xa4: u32 = 0x2056bd4;
pub const GameFlags_ClearSequenceState: u32 = 0x20575d0;
pub const GameFlags_SetField0x102: u32 = 0x2057818;
pub const GameFlags_GetField0x102: u32 = 0x205782c;
pub const GameFlags_SetByte0x104: u32 = 0x2057868;
pub const GameFlags_SetByte0x105: u32 = 0x20578ac;
pub const GameFlags_ComputeField0x104: u32 = 0x2057918;
pub const GameFlags_CopyVec4_Ofs0xb8_0xc8: u32 = 0x2057984;
pub const GameFlags_CallHookWithConst: u32 = 0x2057a24;
pub const GameFlags_SetField0x110: u32 = 0x2057a6c;
pub const GameClock_ClearUpdateCallback: u32 = 0x205e52c;
pub const GameClock_LoadState: u32 = 0x205e5a0;
pub const GameClock_SaveState: u32 = 0x205e7c4;
pub const GameClock_ClearRtcCache: u32 = 0x205e99c;
pub const GameClock_ClearManualTime: u32 = 0x205e9f8;
pub const TimePeriod_FromHour: u32 = 0x205ebf4;
pub const GameClock_CallHookWith1: u32 = 0x205f0cc;
pub const GameClock_CallHookWith0: u32 = 0x205f0dc;
pub const GameClock_GetHour: u32 = 0x205f2ac;
pub const GameClock_GetYear: u32 = 0x205f4b0;
pub const GameClock_GetMonth: u32 = 0x205f53c;
pub const GameClock_GetDay: u32 = 0x205f5c8;
pub const GameClock_CallHookGetYear: u32 = 0x205f6a4;
pub const GameClock_CallHookGetMonth: u32 = 0x205f6b0;
pub const GameClock_CallHookGetDay: u32 = 0x205f6bc;
pub const GameClock_CallHookGetWeekday: u32 = 0x205f6c8;
pub const GameClock_CallHookGetHour: u32 = 0x205f6d4;
pub const GameClock_CallHookGetMinute: u32 = 0x205f6e0;
pub const GameClock_CallHookGetSecond: u32 = 0x205f6ec;
pub const GameClock_GetTimePeriod: u32 = 0x205f728;
pub const GameClock_GetDate: u32 = 0x205f97c;
pub const SaveSlot_CloseStorage: u32 = 0x2060728;
pub const SaveSlot_CallStateHook: u32 = 0x2060a68;
pub const SaveSlot_ProcessStateRecord: u32 = 0x2060b00;
pub const SaveSlot_UpdateRecordState: u32 = 0x206112c;
pub const GameState_SetPointer: u32 = 0x2063514;
pub const GameState_GetPtrForSlotOrDefault: u32 = 0x20635ec;
pub const GameState_GetField0x78: u32 = 0x20639e0;
pub const GameState_GetField0x7a: u32 = 0x2063a80;
pub const GameState_GetField0x98: u32 = 0x2063cc4;
pub const SaveSlot_ClearWord_Ofs0x50: u32 = 0x2064ba0;
pub const SaveSlot_GetLowNibble_Ofs0x67: u32 = 0x2064e94;
pub const SaveSlot_SetLowNibble_Ofs0x67: u32 = 0x2064ea8;
pub const SaveSlot_GetHighNibble_Ofs0x67: u32 = 0x2064ed0;
pub const SaveSlot_SetHighNibble_Ofs0x67: u32 = 0x2064eec;
pub const SaveSlot_GetCombinedFlag_Ofs0x39ff_3a00_3a02: u32 = 0x20666f0;
pub const TaskList_InitGlobalWithFnPtr: u32 = 0x206c428;
pub const TaskList_ClearGlobalPtr: u32 = 0x206c450;
pub const MapArc_SetCurrentCharId: u32 = 0x2070228;
pub const MapArc_GetCurrentCharId: u32 = 0x207025c;
pub const MapArc_ClearCurrentCharId: u32 = 0x207026c;
pub const MapArc_SetCurrentMode: u32 = 0x2070280;
pub const MapArc_GetCurrentMode: u32 = 0x2070290;
pub const MapArc_SetField34: u32 = 0x2070390;
pub const MapArc_SetBgmId: u32 = 0x20703b0;
pub const MapArc_GetBgmId: u32 = 0x20703c0;
pub const MapArc_GetActiveCharIdAndMode: u32 = 0x20703d0;
pub const GameSnd_PlaySceneAudio: u32 = 0x2072a88;
pub const GameSnd_SetSceneAudioLock: u32 = 0x2073178;
pub const SaveSelect_Init: u32 = 0x2074470;
pub const SaveSelect_Shutdown: u32 = 0x2074580;
pub const SaveSelect_RefreshNewestSlotDisplay: u32 = 0x20745c0;
pub const SaveSelect_ProcessScreen: u32 = 0x2074600;
pub const Display_ResetBgAndVram: u32 = 0x2076c78;
pub const SaveSelect_TickWaitCounter: u32 = 0x2077370;
pub const DebugMenu_Main: u32 = 0x2077604;
pub const DateUi_SelectNightStyle: u32 = 0x207f910;
pub const UiLists_ClearBoth: u32 = 0x20847c0;
pub const UiLists_ClearSub: u32 = 0x2084808;
pub const Bg_ClearState: u32 = 0x2084e78;
pub const Bg_Initialize: u32 = 0x2085028;
pub const Bg_SelectState: u32 = 0x2085360;
pub const Bg_DispatchState: u32 = 0x2085568;
pub const Bg_ResetAuxiliary: u32 = 0x2085aa0;
pub const Bg_ComputeTimedValuePrimary: u32 = 0x208635c;
pub const Screen_RunCurrent: u32 = 0x20882e0;   // labelled Sprite_DispatchAnimationState
pub const Screen_GetNext: u32 = 0x20885e4;   // labelled Sprite_GetField4
pub const Screen_GetResult: u32 = 0x2088614;   // labelled Sprite_GetField14
pub const Screen_GetAltResult: u32 = 0x2088634;   // labelled Sprite_GetField1c
pub const Sprite_SetField8: u32 = 0x20886b4;
pub const Sprite_SetFieldc: u32 = 0x20886c4;
pub const Sprite_SetField20: u32 = 0x20886e8;
pub const Sprite_ClearAnimation: u32 = 0x2089ce0;
pub const Sprite_ProcessAnimationFrame: u32 = 0x2089d44;
pub const Screen_Request: u32 = 0x2089d8c;   // labelled Sprite_UpdateAnimationFrame
pub const Screen_Run: u32 = 0x2089e04;   // labelled Sprite_DispatchFrameState
pub const Screen_GetSaved: u32 = 0x208a230;   // labelled Sprite_GetContext
pub const Sprite_ClearContext: u32 = 0x208a23c;
pub const Screen_GetSavedSize: u32 = 0x208a288;   // labelled Sprite_GetContextValue
pub const Screen_Transition: u32 = 0x208a2b4;   // labelled Sprite_SelectContextMode
pub const Screen_LoadOverlay: u32 = 0x208a994;   // labelled Sprite_SelectRenderState
pub const Screen_UnloadOverlay: u32 = 0x208abd4;   // labelled Sprite_SelectTextState
pub const ColorMath_ComputeRampWrapper: u32 = 0x209156c;
pub const FadeState_InvokeCallbackA: u32 = 0x209176c;
pub const GameFlow_InvokeCallbackMode8: u32 = 0x2091ff0;
pub const GameFlow_GetStateField4: u32 = 0x20920e4;
pub const SaveSlot_SetField1c: u32 = 0x2092bc8;
pub const SaveSlot_GetField1c: u32 = 0x2092bdc;
pub const SaveCheck_UpdateStateTimerExtended: u32 = 0x20932e0;
pub const SaveCheck_ResetCounter: u32 = 0x20934f8;
pub const SaveData_EncodeRecord: u32 = 0x2093fb4;
pub const SaveData_IsRecord0Used: u32 = 0x209410c;
pub const SaveData_InvokeCallbackWithField1ec: u32 = 0x2094348;
pub const Menu_RunOptionScreenState: u32 = 0x2095f38;
pub const Settings_RunScreenState: u32 = 0x2096b6c;
pub const GameFlags_TestScopedBit: u32 = 0x209a088;
pub const GameFlags_SetScopedBit: u32 = 0x209a0bc;
pub const UiScreen_InitializeDateFields: u32 = 0x209d234;
pub const UiScreen_RunStateMachine: u32 = 0x209d460;
pub const TextRes_ProcessInputText: u32 = 0x209eabc;
pub const TextRes_FreeBuffer: u32 = 0x209ec14;
pub const SceneRoute_ProcessClockTransition: u32 = 0x209f354;
pub const SceneLoop_InitializeSpriteSystemA: u32 = 0x209f4cc;
pub const SceneLoop_NopCallbackA: u32 = 0x209f500;
pub const SceneLoop_RunSpriteSystemA: u32 = 0x209f50c;
pub const SceneLoop_InitializeSpriteSystemB: u32 = 0x209f618;
pub const SceneLoop_NopCallbackB: u32 = 0x209f64c;
pub const SceneLoop_RunSpriteSystemB: u32 = 0x209f658;
pub const EventLogic_GetContextField: u32 = 0x20a3dbc;
pub const EventLogic_SelectEvent: u32 = 0x20a3dd0;
pub const EventLogic_RunEventStateMachine: u32 = 0x20a40b8;
pub const ClockWidget_UpdateStateB: u32 = 0x20a57c0;
pub const ClockWidget_RunStateMachine: u32 = 0x20a59bc;
pub const MainScene_RunStateMachine: u32 = 0x20a674c;
pub const TextRes_UpdateRandomSelection: u32 = 0x20ac120;
pub const TextRes_GetRandomSelection: u32 = 0x20ac138;
pub const Schedule_CacheCurrentSlot: u32 = 0x20b2f98;
pub const Schedule_DispatchEventLoad: u32 = 0x20b3134;
pub const EventQueue_BuildOutput: u32 = 0x20b5d64;
pub const UiFlow_ThunkTertiary: u32 = 0x20b7a68;
pub const UiChoice_ResetPrimary: u32 = 0x20b7b3c;
pub const UiChoice_ResetSecondary: u32 = 0x20b7b5c;
pub const UiChoice_DispatchInput: u32 = 0x20b7b80;
pub const UiInput_Reset: u32 = 0x20b85e0;
pub const SelectScreen_Update: u32 = 0x20b93e0;
pub const UiSelection_SetValue: u32 = 0x20b96cc;
pub const FlowState_InitSingleton: u32 = 0x20b96dc;
pub const FlowState_SetFlag0x20AndNotify: u32 = 0x20b9808;
pub const FlowState_ClearFlag0x20: u32 = 0x20b9830;
pub const UiMain_UpdateMode: u32 = 0x20b9f14;
pub const MsgBox_Open: u32 = 0x20be4a8;
pub const MsgBox_DrawAdvanceMarker: u32 = 0x20bf018;
pub const MsgBox_PrintLine: u32 = 0x20bf060;
pub const EventAssets_SetPrimary: u32 = 0x20bfbc4;
pub const EventAssets_CheckLoaded: u32 = 0x20bfc88;
pub const EventLogic_DispatchTrigger: u32 = 0x20c8e70;
pub const EventLogic_SetRecordFlagPrimary: u32 = 0x20c9788;
pub const Stats_GetBase_Ofs0x2b5: u32 = 0x20cac04;
pub const StateLogic_GetDataChi_cc3bc: u32 = 0x20cc3bc;
pub const SprFx_SelectDataMu_d2c38: u32 = 0x20d2c38;
pub const GameState_SetBitFlag: u32 = 0x20d5388;
pub const TransitionBg_ClearVramBanks: u32 = 0x20d609c;
pub const Schedule_UpdateTimeDataRho_d7f70: u32 = 0x20d7f70;
pub const Schedule_UpdateDataUpsilon_d83e0: u32 = 0x20d83e0;
pub const Schedule_UpdateDataPhi_d84c8: u32 = 0x20d84c8;
pub const Schedule_GetDataPsi_d85cc: u32 = 0x20d85cc;
pub const RandEvent_SelectDataUpsilon_d9c7c: u32 = 0x20d9c7c;
pub const RandEvent_UpdateData_d9ce8: u32 = 0x20d9ce8;
pub const RandEvent_UpdateData_d9d94: u32 = 0x20d9d94;
pub const RandEvent_InvokeCallbackData_d9fc8: u32 = 0x20d9fc8;
pub const RandEvent_UpdateData_d9fd4: u32 = 0x20d9fd4;
pub const RandEvent_UpdateData_d9ffc: u32 = 0x20d9ffc;
pub const RandEvent_UpdateData_da024: u32 = 0x20da024;
pub const StateSegment_UpdateStateDataPhi_dbf4c: u32 = 0x20dbf4c;
pub const StateLogic_UpdateDataTau_dc9f8: u32 = 0x20dc9f8;
pub const StateLogic_UpdateData_dd028: u32 = 0x20dd028;
pub const StateLogic_CalculateData_dd2e8: u32 = 0x20dd2e8;
pub const StateLogic_UpdateData_dd4dc: u32 = 0x20dd4dc;
pub const StateLogic_CalculateDataMu_dd5d8: u32 = 0x20dd5d8;
pub const StateLogic_ClearDataPi_dd750: u32 = 0x20dd750;
pub const DayEvent_GetResultIdForSlotEvent: u32 = 0x20e13b8;
pub const GameState_RequestSlotTransitionIfCond: u32 = 0x20e39f8;
pub const GameState_GetSlotTableValue_0210e1a0: u32 = 0x20e3a4c;
pub const GameState_RequestFlaggedSlotTransition: u32 = 0x20e3a64;
pub const GameState_GetSlotTableValue_0210e1ac: u32 = 0x20e3ac0;
pub const Overlay_LoadInit: u32 = 0x20e4394;
pub const Overlay_UpdateInitState: u32 = 0x20e43cc;
pub const Overlay_UnloadCleanup: u32 = 0x20e4474;
pub const GameState_ResetToTitleAudio: u32 = 0x20e57a8;
pub const Overlay_Load: u32 = 0x20ef978;
pub const Overlay_Unload: u32 = 0x20ef9ec;
pub const Overlay_IsLoaded: u32 = 0x20efa50;
pub const Schedule_AllocateStorage: u32 = 0x20f2608;
pub const Schedule_FreeStorage: u32 = 0x20f2630;
pub const Schedule_LoadStorage: u32 = 0x20f2650;
pub const Schedule_UpdateStorage: u32 = 0x20f277c;
pub const Schedule_ProcessStorage: u32 = 0x20f289c;
pub const Schedule_GetDateEntry: u32 = 0x20f345c;
pub const Schedule_GetFilteredCount: u32 = 0x20f3a3c;
pub const Schedule_IncrementPrimaryCounters: u32 = 0x20f7798;
pub const Schedule_IncrementPrimaryNormalCounters: u32 = 0x20f782c;
pub const Schedule_SubtractSecondaryField0x04By16: u32 = 0x20f7f1c;
pub const Schedule_IncrementSecondaryCounters: u32 = 0x20f8148;
pub const Schedule_TestSecondaryDateAndFlag: u32 = 0x20f82cc;
pub const Schedule_AllocateAuxState: u32 = 0x20f8554;
pub const Schedule_FreeAuxState: u32 = 0x20f8580;
pub const Schedule_AllocateEventState: u32 = 0x20f85d4;
pub const Schedule_FreeEventState: u32 = 0x20f8604;
pub const ItemDb_StoreQueuedValue: u32 = 0x20f8ed0;
pub const ItemDb_IncrementQueueIndex: u32 = 0x20f91f8;
pub const Callback_InvokeWith0100: u32 = 0x20f9554;
pub const DateScene_Initialize: u32 = 0x20f9efc;
pub const DateScene_Update: u32 = 0x20f9f40;
pub const GameState_HasAsymmetricEventPair: u32 = 0x20fb2c4;
pub const GameState_SetSecondEventBits: u32 = 0x20fb310;
pub const GameState_UpdateProgressUnlocks: u32 = 0x20fb65c;
// callees of the natives (runtime)
pub const SceneRoute_SelectTextRoute: u32 = 0x209f020;
pub const StateLogic_UpdateData_dd26c: u32 = 0x20dd26c;
pub const ItemDb_GetSelectedQueueValue: u32 = 0x20f92bc;
pub const MapEvent_CheckWorkState: u32 = 0x20b4784;
pub const SpriteSystem_Init: u32 = 0x208827c;
pub const Sprite_DispatchDrawState: u32 = 0x20887b4;
pub const SaveData_InitializeRecord: u32 = 0x2093b48;
pub const GameState_RunSequence: u32 = 0x20634e4;
pub const SaveData_AllocHeaderBlock: u32 = 0x2093b74;
pub const SaveData_ValidateRecord: u32 = 0x2093ba0;
pub const SlotWorkflow_ClearAction: u32 = 0x2062e10;

/// the label of an address this table names
pub fn name(a: u32) -> Option<&'static str> {
    Some(match a {
        GX_LoadBGPltt => "GX_LoadBGPltt",
        GXS_LoadBGPltt => "GXS_LoadBGPltt",
        memset => "memset",
        Main_IncStateDepth => "Main_IncStateDepth",
        Main_DecStateDepth => "Main_DecStateDepth",
        Main_GetStateAtDepth => "Main_GetStateAtDepth",
        Main_SetState => "Main_SetState",
        Main_AdvanceStateSequence => "Main_AdvanceStateSequence",
        GameHeap_LoadOverlayInfo => "GameHeap_LoadOverlayInfo",
        GameHeap_LoadOverlayInfoAlt => "GameHeap_LoadOverlayInfoAlt",
        Display_SetLcdScreenSwap => "Display_SetLcdScreenSwap",
        Gfx_ApplyDefaultBanksAndInitVramMgrs => "Gfx_ApplyDefaultBanksAndInitVramMgrs",
        Gfx_Apply2dOnlyBanksAndInitVramMgrs => "Gfx_Apply2dOnlyBanksAndInitVramMgrs",
        Gfx_SetBgVisibleFlag => "Gfx_SetBgVisibleFlag",
        Gfx_GetBgScreenSize => "Gfx_GetBgScreenSize",
        ScreenFade_ToBlack => "ScreenFade_ToBlack",
        ScreenFade_ToWhite => "ScreenFade_ToWhite",
        ScreenFade_ToClear => "ScreenFade_ToClear",
        ScreenFade_IsActive => "ScreenFade_IsActive",
        GfxRes_CallHookPtr => "GfxRes_CallHookPtr",
        BgLoad_CallHookPtr => "BgLoad_CallHookPtr",
        Mod2049a04_ApplyMapSelection => "Mod2049a04_ApplyMapSelection",
        Mod2049a04_IsIdInTables => "Mod2049a04_IsIdInTables",
        Mod2049a04_EnterSceneSaveClock => "Mod2049a04_EnterSceneSaveClock",
        Mod2049a04_ResetSceneSystems => "Mod2049a04_ResetSceneSystems",
        Mod2049a04_TeardownSceneSystems => "Mod2049a04_TeardownSceneSystems",
        Mod2049a04_CallThreeExtFuncs => "Mod2049a04_CallThreeExtFuncs",
        Mod2049a04_TeardownSceneIfMapState0xb_0x14 => "Mod2049a04_TeardownSceneIfMapState0xb_0x14",
        Mod2049a04_ResetClockScriptSoundFade => "Mod2049a04_ResetClockScriptSoundFade",
        Mod2049a04_RunMainStateMachine => "Mod2049a04_RunMainStateMachine",
        Mod204af08_InitGameSystems => "Mod204af08_InitGameSystems",
        Mod204af08_ShutdownGameSystems => "Mod204af08_ShutdownGameSystems",
        Mod204af08_StepOverlayLoadSequence => "Mod204af08_StepOverlayLoadSequence",
        Mod204af08_RunGameStateMachine => "Mod204af08_RunGameStateMachine",
        Script_WriteVariable => "Script_WriteVariable",
        ResTable_Open => "ResTable_Open",
        MapCtl_UpdateTouchSelection => "MapCtl_UpdateTouchSelection",
        Camera_GetAreaForCurrentPreset => "Camera_GetAreaForCurrentPreset",
        Transform_UpdateFrame => "Transform_UpdateFrame",
        Transform_UpdateInputContext => "Transform_UpdateInputContext",
        Transform_GetInputContextStatus => "Transform_GetInputContextStatus",
        Transform_GetContextStatePlusOne => "Transform_GetContextStatePlusOne",
        Transform_SetContextEnabled => "Transform_SetContextEnabled",
        Projection_SetCallback => "Projection_SetCallback",
        GameFlags_Assign => "GameFlags_Assign",
        GameFlags_Test => "GameFlags_Test",
        GameFlags_SetBitRange => "GameFlags_SetBitRange",
        GameFlags_HandleTimeModeChange => "GameFlags_HandleTimeModeChange",
        GameFlags_GetField0xe0 => "GameFlags_GetField0xe0",
        GameFlags_SetField0x0 => "GameFlags_SetField0x0",
        GameFlags_GetField0x0 => "GameFlags_GetField0x0",
        GameFlags_PushField0x4 => "GameFlags_PushField0x4",
        GameFlags_SetField0xc => "GameFlags_SetField0xc",
        GameFlags_SetField0x98 => "GameFlags_SetField0x98",
        GameFlags_AdvanceCalendarEvents => "GameFlags_AdvanceCalendarEvents",
        GameFlags_RunClockSynchronization => "GameFlags_RunClockSynchronization",
        SaveData_InitializeClockFields => "SaveData_InitializeClockFields",
        GameFlags_ConfigureField0x90 => "GameFlags_ConfigureField0x90",
        GameFlags_UpdateField0xa4 => "GameFlags_UpdateField0xa4",
        GameFlags_ClearSequenceState => "GameFlags_ClearSequenceState",
        GameFlags_SetField0x102 => "GameFlags_SetField0x102",
        GameFlags_GetField0x102 => "GameFlags_GetField0x102",
        GameFlags_SetByte0x104 => "GameFlags_SetByte0x104",
        GameFlags_SetByte0x105 => "GameFlags_SetByte0x105",
        GameFlags_ComputeField0x104 => "GameFlags_ComputeField0x104",
        GameFlags_CopyVec4_Ofs0xb8_0xc8 => "GameFlags_CopyVec4_Ofs0xb8_0xc8",
        GameFlags_CallHookWithConst => "GameFlags_CallHookWithConst",
        GameFlags_SetField0x110 => "GameFlags_SetField0x110",
        GameClock_ClearUpdateCallback => "GameClock_ClearUpdateCallback",
        GameClock_LoadState => "GameClock_LoadState",
        GameClock_SaveState => "GameClock_SaveState",
        GameClock_ClearRtcCache => "GameClock_ClearRtcCache",
        GameClock_ClearManualTime => "GameClock_ClearManualTime",
        TimePeriod_FromHour => "TimePeriod_FromHour",
        GameClock_CallHookWith1 => "GameClock_CallHookWith1",
        GameClock_CallHookWith0 => "GameClock_CallHookWith0",
        GameClock_GetHour => "GameClock_GetHour",
        GameClock_GetYear => "GameClock_GetYear",
        GameClock_GetMonth => "GameClock_GetMonth",
        GameClock_GetDay => "GameClock_GetDay",
        GameClock_CallHookGetYear => "GameClock_CallHookGetYear",
        GameClock_CallHookGetMonth => "GameClock_CallHookGetMonth",
        GameClock_CallHookGetDay => "GameClock_CallHookGetDay",
        GameClock_CallHookGetWeekday => "GameClock_CallHookGetWeekday",
        GameClock_CallHookGetHour => "GameClock_CallHookGetHour",
        GameClock_CallHookGetMinute => "GameClock_CallHookGetMinute",
        GameClock_CallHookGetSecond => "GameClock_CallHookGetSecond",
        GameClock_GetTimePeriod => "GameClock_GetTimePeriod",
        GameClock_GetDate => "GameClock_GetDate",
        SaveSlot_CloseStorage => "SaveSlot_CloseStorage",
        SaveSlot_CallStateHook => "SaveSlot_CallStateHook",
        SaveSlot_ProcessStateRecord => "SaveSlot_ProcessStateRecord",
        SaveSlot_UpdateRecordState => "SaveSlot_UpdateRecordState",
        GameState_SetPointer => "GameState_SetPointer",
        GameState_GetPtrForSlotOrDefault => "GameState_GetPtrForSlotOrDefault",
        GameState_GetField0x78 => "GameState_GetField0x78",
        GameState_GetField0x7a => "GameState_GetField0x7a",
        GameState_GetField0x98 => "GameState_GetField0x98",
        SaveSlot_ClearWord_Ofs0x50 => "SaveSlot_ClearWord_Ofs0x50",
        SaveSlot_GetLowNibble_Ofs0x67 => "SaveSlot_GetLowNibble_Ofs0x67",
        SaveSlot_SetLowNibble_Ofs0x67 => "SaveSlot_SetLowNibble_Ofs0x67",
        SaveSlot_GetHighNibble_Ofs0x67 => "SaveSlot_GetHighNibble_Ofs0x67",
        SaveSlot_SetHighNibble_Ofs0x67 => "SaveSlot_SetHighNibble_Ofs0x67",
        SaveSlot_GetCombinedFlag_Ofs0x39ff_3a00_3a02 => "SaveSlot_GetCombinedFlag_Ofs0x39ff_3a00_3a02",
        TaskList_InitGlobalWithFnPtr => "TaskList_InitGlobalWithFnPtr",
        TaskList_ClearGlobalPtr => "TaskList_ClearGlobalPtr",
        MapArc_SetCurrentCharId => "MapArc_SetCurrentCharId",
        MapArc_GetCurrentCharId => "MapArc_GetCurrentCharId",
        MapArc_ClearCurrentCharId => "MapArc_ClearCurrentCharId",
        MapArc_SetCurrentMode => "MapArc_SetCurrentMode",
        MapArc_GetCurrentMode => "MapArc_GetCurrentMode",
        MapArc_SetField34 => "MapArc_SetField34",
        MapArc_SetBgmId => "MapArc_SetBgmId",
        MapArc_GetBgmId => "MapArc_GetBgmId",
        MapArc_GetActiveCharIdAndMode => "MapArc_GetActiveCharIdAndMode",
        GameSnd_PlaySceneAudio => "GameSnd_PlaySceneAudio",
        GameSnd_SetSceneAudioLock => "GameSnd_SetSceneAudioLock",
        SaveSelect_Init => "SaveSelect_Init",
        SaveSelect_Shutdown => "SaveSelect_Shutdown",
        SaveSelect_RefreshNewestSlotDisplay => "SaveSelect_RefreshNewestSlotDisplay",
        SaveSelect_ProcessScreen => "SaveSelect_ProcessScreen",
        Display_ResetBgAndVram => "Display_ResetBgAndVram",
        SaveSelect_TickWaitCounter => "SaveSelect_TickWaitCounter",
        DebugMenu_Main => "DebugMenu_Main",
        DateUi_SelectNightStyle => "DateUi_SelectNightStyle",
        UiLists_ClearBoth => "UiLists_ClearBoth",
        UiLists_ClearSub => "UiLists_ClearSub",
        Bg_ClearState => "Bg_ClearState",
        Bg_Initialize => "Bg_Initialize",
        Bg_SelectState => "Bg_SelectState",
        Bg_DispatchState => "Bg_DispatchState",
        Bg_ResetAuxiliary => "Bg_ResetAuxiliary",
        Bg_ComputeTimedValuePrimary => "Bg_ComputeTimedValuePrimary",
        Screen_RunCurrent => "Screen_RunCurrent",
        Screen_GetNext => "Screen_GetNext",
        Screen_GetResult => "Screen_GetResult",
        Screen_GetAltResult => "Screen_GetAltResult",
        Sprite_SetField8 => "Sprite_SetField8",
        Sprite_SetFieldc => "Sprite_SetFieldc",
        Sprite_SetField20 => "Sprite_SetField20",
        Sprite_ClearAnimation => "Sprite_ClearAnimation",
        Sprite_ProcessAnimationFrame => "Sprite_ProcessAnimationFrame",
        Screen_Request => "Screen_Request",
        Screen_Run => "Screen_Run",
        Screen_GetSaved => "Screen_GetSaved",
        Sprite_ClearContext => "Sprite_ClearContext",
        Screen_GetSavedSize => "Screen_GetSavedSize",
        Screen_Transition => "Screen_Transition",
        Screen_LoadOverlay => "Screen_LoadOverlay",
        Screen_UnloadOverlay => "Screen_UnloadOverlay",
        ColorMath_ComputeRampWrapper => "ColorMath_ComputeRampWrapper",
        FadeState_InvokeCallbackA => "FadeState_InvokeCallbackA",
        GameFlow_InvokeCallbackMode8 => "GameFlow_InvokeCallbackMode8",
        GameFlow_GetStateField4 => "GameFlow_GetStateField4",
        SaveSlot_SetField1c => "SaveSlot_SetField1c",
        SaveSlot_GetField1c => "SaveSlot_GetField1c",
        SaveCheck_UpdateStateTimerExtended => "SaveCheck_UpdateStateTimerExtended",
        SaveCheck_ResetCounter => "SaveCheck_ResetCounter",
        SaveData_EncodeRecord => "SaveData_EncodeRecord",
        SaveData_IsRecord0Used => "SaveData_IsRecord0Used",
        SaveData_InvokeCallbackWithField1ec => "SaveData_InvokeCallbackWithField1ec",
        Menu_RunOptionScreenState => "Menu_RunOptionScreenState",
        Settings_RunScreenState => "Settings_RunScreenState",
        GameFlags_TestScopedBit => "GameFlags_TestScopedBit",
        GameFlags_SetScopedBit => "GameFlags_SetScopedBit",
        UiScreen_InitializeDateFields => "UiScreen_InitializeDateFields",
        UiScreen_RunStateMachine => "UiScreen_RunStateMachine",
        TextRes_ProcessInputText => "TextRes_ProcessInputText",
        TextRes_FreeBuffer => "TextRes_FreeBuffer",
        SceneRoute_ProcessClockTransition => "SceneRoute_ProcessClockTransition",
        SceneLoop_InitializeSpriteSystemA => "SceneLoop_InitializeSpriteSystemA",
        SceneLoop_NopCallbackA => "SceneLoop_NopCallbackA",
        SceneLoop_RunSpriteSystemA => "SceneLoop_RunSpriteSystemA",
        SceneLoop_InitializeSpriteSystemB => "SceneLoop_InitializeSpriteSystemB",
        SceneLoop_NopCallbackB => "SceneLoop_NopCallbackB",
        SceneLoop_RunSpriteSystemB => "SceneLoop_RunSpriteSystemB",
        EventLogic_GetContextField => "EventLogic_GetContextField",
        EventLogic_SelectEvent => "EventLogic_SelectEvent",
        EventLogic_RunEventStateMachine => "EventLogic_RunEventStateMachine",
        ClockWidget_UpdateStateB => "ClockWidget_UpdateStateB",
        ClockWidget_RunStateMachine => "ClockWidget_RunStateMachine",
        MainScene_RunStateMachine => "MainScene_RunStateMachine",
        TextRes_UpdateRandomSelection => "TextRes_UpdateRandomSelection",
        TextRes_GetRandomSelection => "TextRes_GetRandomSelection",
        Schedule_CacheCurrentSlot => "Schedule_CacheCurrentSlot",
        Schedule_DispatchEventLoad => "Schedule_DispatchEventLoad",
        EventQueue_BuildOutput => "EventQueue_BuildOutput",
        UiFlow_ThunkTertiary => "UiFlow_ThunkTertiary",
        UiChoice_ResetPrimary => "UiChoice_ResetPrimary",
        UiChoice_ResetSecondary => "UiChoice_ResetSecondary",
        UiChoice_DispatchInput => "UiChoice_DispatchInput",
        UiInput_Reset => "UiInput_Reset",
        SelectScreen_Update => "SelectScreen_Update",
        UiSelection_SetValue => "UiSelection_SetValue",
        FlowState_InitSingleton => "FlowState_InitSingleton",
        FlowState_SetFlag0x20AndNotify => "FlowState_SetFlag0x20AndNotify",
        FlowState_ClearFlag0x20 => "FlowState_ClearFlag0x20",
        UiMain_UpdateMode => "UiMain_UpdateMode",
        MsgBox_Open => "MsgBox_Open",
        MsgBox_DrawAdvanceMarker => "MsgBox_DrawAdvanceMarker",
        MsgBox_PrintLine => "MsgBox_PrintLine",
        EventAssets_SetPrimary => "EventAssets_SetPrimary",
        EventAssets_CheckLoaded => "EventAssets_CheckLoaded",
        EventLogic_DispatchTrigger => "EventLogic_DispatchTrigger",
        EventLogic_SetRecordFlagPrimary => "EventLogic_SetRecordFlagPrimary",
        Stats_GetBase_Ofs0x2b5 => "Stats_GetBase_Ofs0x2b5",
        StateLogic_GetDataChi_cc3bc => "StateLogic_GetDataChi_cc3bc",
        SprFx_SelectDataMu_d2c38 => "SprFx_SelectDataMu_d2c38",
        GameState_SetBitFlag => "GameState_SetBitFlag",
        TransitionBg_ClearVramBanks => "TransitionBg_ClearVramBanks",
        Schedule_UpdateTimeDataRho_d7f70 => "Schedule_UpdateTimeDataRho_d7f70",
        Schedule_UpdateDataUpsilon_d83e0 => "Schedule_UpdateDataUpsilon_d83e0",
        Schedule_UpdateDataPhi_d84c8 => "Schedule_UpdateDataPhi_d84c8",
        Schedule_GetDataPsi_d85cc => "Schedule_GetDataPsi_d85cc",
        RandEvent_SelectDataUpsilon_d9c7c => "RandEvent_SelectDataUpsilon_d9c7c",
        RandEvent_UpdateData_d9ce8 => "RandEvent_UpdateData_d9ce8",
        RandEvent_UpdateData_d9d94 => "RandEvent_UpdateData_d9d94",
        RandEvent_InvokeCallbackData_d9fc8 => "RandEvent_InvokeCallbackData_d9fc8",
        RandEvent_UpdateData_d9fd4 => "RandEvent_UpdateData_d9fd4",
        RandEvent_UpdateData_d9ffc => "RandEvent_UpdateData_d9ffc",
        RandEvent_UpdateData_da024 => "RandEvent_UpdateData_da024",
        StateSegment_UpdateStateDataPhi_dbf4c => "StateSegment_UpdateStateDataPhi_dbf4c",
        StateLogic_UpdateDataTau_dc9f8 => "StateLogic_UpdateDataTau_dc9f8",
        StateLogic_UpdateData_dd028 => "StateLogic_UpdateData_dd028",
        StateLogic_CalculateData_dd2e8 => "StateLogic_CalculateData_dd2e8",
        StateLogic_UpdateData_dd4dc => "StateLogic_UpdateData_dd4dc",
        StateLogic_CalculateDataMu_dd5d8 => "StateLogic_CalculateDataMu_dd5d8",
        StateLogic_ClearDataPi_dd750 => "StateLogic_ClearDataPi_dd750",
        DayEvent_GetResultIdForSlotEvent => "DayEvent_GetResultIdForSlotEvent",
        GameState_RequestSlotTransitionIfCond => "GameState_RequestSlotTransitionIfCond",
        GameState_GetSlotTableValue_0210e1a0 => "GameState_GetSlotTableValue_0210e1a0",
        GameState_RequestFlaggedSlotTransition => "GameState_RequestFlaggedSlotTransition",
        GameState_GetSlotTableValue_0210e1ac => "GameState_GetSlotTableValue_0210e1ac",
        Overlay_LoadInit => "Overlay_LoadInit",
        Overlay_UpdateInitState => "Overlay_UpdateInitState",
        Overlay_UnloadCleanup => "Overlay_UnloadCleanup",
        GameState_ResetToTitleAudio => "GameState_ResetToTitleAudio",
        Overlay_Load => "Overlay_Load",
        Overlay_Unload => "Overlay_Unload",
        Overlay_IsLoaded => "Overlay_IsLoaded",
        Schedule_AllocateStorage => "Schedule_AllocateStorage",
        Schedule_FreeStorage => "Schedule_FreeStorage",
        Schedule_LoadStorage => "Schedule_LoadStorage",
        Schedule_UpdateStorage => "Schedule_UpdateStorage",
        Schedule_ProcessStorage => "Schedule_ProcessStorage",
        Schedule_GetDateEntry => "Schedule_GetDateEntry",
        Schedule_GetFilteredCount => "Schedule_GetFilteredCount",
        Schedule_IncrementPrimaryCounters => "Schedule_IncrementPrimaryCounters",
        Schedule_IncrementPrimaryNormalCounters => "Schedule_IncrementPrimaryNormalCounters",
        Schedule_SubtractSecondaryField0x04By16 => "Schedule_SubtractSecondaryField0x04By16",
        Schedule_IncrementSecondaryCounters => "Schedule_IncrementSecondaryCounters",
        Schedule_TestSecondaryDateAndFlag => "Schedule_TestSecondaryDateAndFlag",
        Schedule_AllocateAuxState => "Schedule_AllocateAuxState",
        Schedule_FreeAuxState => "Schedule_FreeAuxState",
        Schedule_AllocateEventState => "Schedule_AllocateEventState",
        Schedule_FreeEventState => "Schedule_FreeEventState",
        ItemDb_StoreQueuedValue => "ItemDb_StoreQueuedValue",
        ItemDb_IncrementQueueIndex => "ItemDb_IncrementQueueIndex",
        Callback_InvokeWith0100 => "Callback_InvokeWith0100",
        DateScene_Initialize => "DateScene_Initialize",
        DateScene_Update => "DateScene_Update",
        GameState_HasAsymmetricEventPair => "GameState_HasAsymmetricEventPair",
        GameState_SetSecondEventBits => "GameState_SetSecondEventBits",
        GameState_UpdateProgressUnlocks => "GameState_UpdateProgressUnlocks",
        SceneRoute_SelectTextRoute => "SceneRoute_SelectTextRoute",
        StateLogic_UpdateData_dd26c => "StateLogic_UpdateData_dd26c",
        ItemDb_GetSelectedQueueValue => "ItemDb_GetSelectedQueueValue",
        MapEvent_CheckWorkState => "MapEvent_CheckWorkState",
        SpriteSystem_Init => "SpriteSystem_Init",
        Sprite_DispatchDrawState => "Sprite_DispatchDrawState",
        SaveData_InitializeRecord => "SaveData_InitializeRecord",
        GameState_RunSequence => "GameState_RunSequence",
        SaveData_AllocHeaderBlock => "SaveData_AllocHeaderBlock",
        SaveData_ValidateRecord => "SaveData_ValidateRecord",
        SlotWorkflow_ClearAction => "SlotWorkflow_ClearAction",
        _ => return None,
    })
}

/// every address this table names
pub const ALL: &[u32] = &[
    GX_LoadBGPltt,
    GXS_LoadBGPltt,
    memset,
    Main_IncStateDepth,
    Main_DecStateDepth,
    Main_GetStateAtDepth,
    Main_SetState,
    Main_AdvanceStateSequence,
    GameHeap_LoadOverlayInfo,
    GameHeap_LoadOverlayInfoAlt,
    Display_SetLcdScreenSwap,
    Gfx_ApplyDefaultBanksAndInitVramMgrs,
    Gfx_Apply2dOnlyBanksAndInitVramMgrs,
    Gfx_SetBgVisibleFlag,
    Gfx_GetBgScreenSize,
    ScreenFade_ToBlack,
    ScreenFade_ToWhite,
    ScreenFade_ToClear,
    ScreenFade_IsActive,
    GfxRes_CallHookPtr,
    BgLoad_CallHookPtr,
    Mod2049a04_ApplyMapSelection,
    Mod2049a04_IsIdInTables,
    Mod2049a04_EnterSceneSaveClock,
    Mod2049a04_ResetSceneSystems,
    Mod2049a04_TeardownSceneSystems,
    Mod2049a04_CallThreeExtFuncs,
    Mod2049a04_TeardownSceneIfMapState0xb_0x14,
    Mod2049a04_ResetClockScriptSoundFade,
    Mod2049a04_RunMainStateMachine,
    Mod204af08_InitGameSystems,
    Mod204af08_ShutdownGameSystems,
    Mod204af08_StepOverlayLoadSequence,
    Mod204af08_RunGameStateMachine,
    Script_WriteVariable,
    ResTable_Open,
    MapCtl_UpdateTouchSelection,
    Camera_GetAreaForCurrentPreset,
    Transform_UpdateFrame,
    Transform_UpdateInputContext,
    Transform_GetInputContextStatus,
    Transform_GetContextStatePlusOne,
    Transform_SetContextEnabled,
    Projection_SetCallback,
    GameFlags_Assign,
    GameFlags_Test,
    GameFlags_SetBitRange,
    GameFlags_HandleTimeModeChange,
    GameFlags_GetField0xe0,
    GameFlags_SetField0x0,
    GameFlags_GetField0x0,
    GameFlags_PushField0x4,
    GameFlags_SetField0xc,
    GameFlags_SetField0x98,
    GameFlags_AdvanceCalendarEvents,
    GameFlags_RunClockSynchronization,
    SaveData_InitializeClockFields,
    GameFlags_ConfigureField0x90,
    GameFlags_UpdateField0xa4,
    GameFlags_ClearSequenceState,
    GameFlags_SetField0x102,
    GameFlags_GetField0x102,
    GameFlags_SetByte0x104,
    GameFlags_SetByte0x105,
    GameFlags_ComputeField0x104,
    GameFlags_CopyVec4_Ofs0xb8_0xc8,
    GameFlags_CallHookWithConst,
    GameFlags_SetField0x110,
    GameClock_ClearUpdateCallback,
    GameClock_LoadState,
    GameClock_SaveState,
    GameClock_ClearRtcCache,
    GameClock_ClearManualTime,
    TimePeriod_FromHour,
    GameClock_CallHookWith1,
    GameClock_CallHookWith0,
    GameClock_GetHour,
    GameClock_GetYear,
    GameClock_GetMonth,
    GameClock_GetDay,
    GameClock_CallHookGetYear,
    GameClock_CallHookGetMonth,
    GameClock_CallHookGetDay,
    GameClock_CallHookGetWeekday,
    GameClock_CallHookGetHour,
    GameClock_CallHookGetMinute,
    GameClock_CallHookGetSecond,
    GameClock_GetTimePeriod,
    GameClock_GetDate,
    SaveSlot_CloseStorage,
    SaveSlot_CallStateHook,
    SaveSlot_ProcessStateRecord,
    SaveSlot_UpdateRecordState,
    GameState_SetPointer,
    GameState_GetPtrForSlotOrDefault,
    GameState_GetField0x78,
    GameState_GetField0x7a,
    GameState_GetField0x98,
    SaveSlot_ClearWord_Ofs0x50,
    SaveSlot_GetLowNibble_Ofs0x67,
    SaveSlot_SetLowNibble_Ofs0x67,
    SaveSlot_GetHighNibble_Ofs0x67,
    SaveSlot_SetHighNibble_Ofs0x67,
    SaveSlot_GetCombinedFlag_Ofs0x39ff_3a00_3a02,
    TaskList_InitGlobalWithFnPtr,
    TaskList_ClearGlobalPtr,
    MapArc_SetCurrentCharId,
    MapArc_GetCurrentCharId,
    MapArc_ClearCurrentCharId,
    MapArc_SetCurrentMode,
    MapArc_GetCurrentMode,
    MapArc_SetField34,
    MapArc_SetBgmId,
    MapArc_GetBgmId,
    MapArc_GetActiveCharIdAndMode,
    GameSnd_PlaySceneAudio,
    GameSnd_SetSceneAudioLock,
    SaveSelect_Init,
    SaveSelect_Shutdown,
    SaveSelect_RefreshNewestSlotDisplay,
    SaveSelect_ProcessScreen,
    Display_ResetBgAndVram,
    SaveSelect_TickWaitCounter,
    DebugMenu_Main,
    DateUi_SelectNightStyle,
    UiLists_ClearBoth,
    UiLists_ClearSub,
    Bg_ClearState,
    Bg_Initialize,
    Bg_SelectState,
    Bg_DispatchState,
    Bg_ResetAuxiliary,
    Bg_ComputeTimedValuePrimary,
    Screen_RunCurrent,
    Screen_GetNext,
    Screen_GetResult,
    Screen_GetAltResult,
    Sprite_SetField8,
    Sprite_SetFieldc,
    Sprite_SetField20,
    Sprite_ClearAnimation,
    Sprite_ProcessAnimationFrame,
    Screen_Request,
    Screen_Run,
    Screen_GetSaved,
    Sprite_ClearContext,
    Screen_GetSavedSize,
    Screen_Transition,
    Screen_LoadOverlay,
    Screen_UnloadOverlay,
    ColorMath_ComputeRampWrapper,
    FadeState_InvokeCallbackA,
    GameFlow_InvokeCallbackMode8,
    GameFlow_GetStateField4,
    SaveSlot_SetField1c,
    SaveSlot_GetField1c,
    SaveCheck_UpdateStateTimerExtended,
    SaveCheck_ResetCounter,
    SaveData_EncodeRecord,
    SaveData_IsRecord0Used,
    SaveData_InvokeCallbackWithField1ec,
    Menu_RunOptionScreenState,
    Settings_RunScreenState,
    GameFlags_TestScopedBit,
    GameFlags_SetScopedBit,
    UiScreen_InitializeDateFields,
    UiScreen_RunStateMachine,
    TextRes_ProcessInputText,
    TextRes_FreeBuffer,
    SceneRoute_ProcessClockTransition,
    SceneLoop_InitializeSpriteSystemA,
    SceneLoop_NopCallbackA,
    SceneLoop_RunSpriteSystemA,
    SceneLoop_InitializeSpriteSystemB,
    SceneLoop_NopCallbackB,
    SceneLoop_RunSpriteSystemB,
    EventLogic_GetContextField,
    EventLogic_SelectEvent,
    EventLogic_RunEventStateMachine,
    ClockWidget_UpdateStateB,
    ClockWidget_RunStateMachine,
    MainScene_RunStateMachine,
    TextRes_UpdateRandomSelection,
    TextRes_GetRandomSelection,
    Schedule_CacheCurrentSlot,
    Schedule_DispatchEventLoad,
    EventQueue_BuildOutput,
    UiFlow_ThunkTertiary,
    UiChoice_ResetPrimary,
    UiChoice_ResetSecondary,
    UiChoice_DispatchInput,
    UiInput_Reset,
    SelectScreen_Update,
    UiSelection_SetValue,
    FlowState_InitSingleton,
    FlowState_SetFlag0x20AndNotify,
    FlowState_ClearFlag0x20,
    UiMain_UpdateMode,
    MsgBox_Open,
    MsgBox_DrawAdvanceMarker,
    MsgBox_PrintLine,
    EventAssets_SetPrimary,
    EventAssets_CheckLoaded,
    EventLogic_DispatchTrigger,
    EventLogic_SetRecordFlagPrimary,
    Stats_GetBase_Ofs0x2b5,
    StateLogic_GetDataChi_cc3bc,
    SprFx_SelectDataMu_d2c38,
    GameState_SetBitFlag,
    TransitionBg_ClearVramBanks,
    Schedule_UpdateTimeDataRho_d7f70,
    Schedule_UpdateDataUpsilon_d83e0,
    Schedule_UpdateDataPhi_d84c8,
    Schedule_GetDataPsi_d85cc,
    RandEvent_SelectDataUpsilon_d9c7c,
    RandEvent_UpdateData_d9ce8,
    RandEvent_UpdateData_d9d94,
    RandEvent_InvokeCallbackData_d9fc8,
    RandEvent_UpdateData_d9fd4,
    RandEvent_UpdateData_d9ffc,
    RandEvent_UpdateData_da024,
    StateSegment_UpdateStateDataPhi_dbf4c,
    StateLogic_UpdateDataTau_dc9f8,
    StateLogic_UpdateData_dd028,
    StateLogic_CalculateData_dd2e8,
    StateLogic_UpdateData_dd4dc,
    StateLogic_CalculateDataMu_dd5d8,
    StateLogic_ClearDataPi_dd750,
    DayEvent_GetResultIdForSlotEvent,
    GameState_RequestSlotTransitionIfCond,
    GameState_GetSlotTableValue_0210e1a0,
    GameState_RequestFlaggedSlotTransition,
    GameState_GetSlotTableValue_0210e1ac,
    Overlay_LoadInit,
    Overlay_UpdateInitState,
    Overlay_UnloadCleanup,
    GameState_ResetToTitleAudio,
    Overlay_Load,
    Overlay_Unload,
    Overlay_IsLoaded,
    Schedule_AllocateStorage,
    Schedule_FreeStorage,
    Schedule_LoadStorage,
    Schedule_UpdateStorage,
    Schedule_ProcessStorage,
    Schedule_GetDateEntry,
    Schedule_GetFilteredCount,
    Schedule_IncrementPrimaryCounters,
    Schedule_IncrementPrimaryNormalCounters,
    Schedule_SubtractSecondaryField0x04By16,
    Schedule_IncrementSecondaryCounters,
    Schedule_TestSecondaryDateAndFlag,
    Schedule_AllocateAuxState,
    Schedule_FreeAuxState,
    Schedule_AllocateEventState,
    Schedule_FreeEventState,
    ItemDb_StoreQueuedValue,
    ItemDb_IncrementQueueIndex,
    Callback_InvokeWith0100,
    DateScene_Initialize,
    DateScene_Update,
    GameState_HasAsymmetricEventPair,
    GameState_SetSecondEventBits,
    GameState_UpdateProgressUnlocks,
    SceneRoute_SelectTextRoute,
    StateLogic_UpdateData_dd26c,
    ItemDb_GetSelectedQueueValue,
    MapEvent_CheckWorkState,
    SpriteSystem_Init,
    Sprite_DispatchDrawState,
    SaveData_InitializeRecord,
    GameState_RunSequence,
    SaveData_AllocHeaderBlock,
    SaveData_ValidateRecord,
    SlotWorkflow_ClearAction,
];
