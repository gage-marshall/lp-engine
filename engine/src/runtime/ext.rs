//! The registry: every game function the ported code calls, by address, with what answers it (see `runtime`).
//! `ret` says whether the caller uses the return value (Void functions return whatever r0 held; the live check does
//! not compare those) or whether it is a heap address (the engine's heap places blocks elsewhere).
#![allow(non_upper_case_globals)]
use super::natives as n;
use super::{Machine, Subsystems};
use crate::flow::addr::*;
use crate::flow::Arg;

pub type NativeFn = fn(&mut Machine, &mut dyn Subsystems, &mut [Arg]) -> i32;

#[derive(Clone, Copy)]
pub enum Kind {
    Native(NativeFn),
    /// provided by an engine subsystem (the name routes it)
    Subsystem(&'static str),
    Pending,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Ret {
    Value,
    Void,
    Addr,
}

#[derive(Clone, Copy)]
pub struct Entry {
    pub name: &'static str,
    pub kind: Kind,
    pub ret: Ret,
}

use Kind::*;
use Ret::*;

macro_rules! e {
    ($name:expr, $kind:expr, $ret:expr) => {
        Some(Entry { name: $name, kind: $kind, ret: $ret })
    };
}

pub fn entry(a: u32) -> Option<Entry> {
    if let Some(e) = super::sdk::entry(a) {
        return Some(e);
    }
    match a {
        // ---- the state stack (Main_*) ----
        Main_IncStateDepth => e!("Main_IncStateDepth", Native(n::main_inc_depth), Void),
        Main_DecStateDepth => e!("Main_DecStateDepth", Native(n::main_dec_depth), Void),
        Main_GetStateAtDepth => e!("Main_GetStateAtDepth", Native(n::main_get_state_fn), Value),
        Main_SetState => e!("Main_SetState", Native(n::main_set_state_fn), Void),
        Main_AdvanceStateSequence => e!("Main_AdvanceStateSequence", Native(n::main_advance), Void),
        memset => e!("memset", Native(n::memset), Addr),

        // ---- heap and overlays ----
        // the game's heap runs as its own code (interp); GameHeap_AllocZeroed 0x203fda8, LoadOverlayInfo 0x203ff24/9c


        Overlay_Load => e!("Overlay_Load", Native(n::overlay_load), Void),
        Overlay_Unload => e!("Overlay_Unload", Native(n::overlay_unload), Void),
        Overlay_IsLoaded => e!("Overlay_IsLoaded", Native(n::overlay_is_loaded), Value),

        // ---- display hardware: the flow's register writes land in RAM, the renderer presents from game state ----
        0x201a73c => e!("GX_DispOn", Native(n::gx_disp_on), Void),
        // ---- the voice decoder (G.722.1): the engine's decoder (`siren`), output identical to the game's ----
        0x2001a58 => e!("G7221_DecodeFrame", Native(n::g7221_decode_frame), Value),
        0x20019e4 => e!("G7221_ResetDecoderBuffers", Native(n::g7221_reset), Void),
        // ---- memory routines, cache maintenance, the compiler's helpers (pure; tests/natives_exact.rs) ----
        0x201e88c => e!("MIi_CpuCopy16", Native(n::mi_cpu_copy16), Void),
        0x201e874 => e!("MIi_CpuClear16", Native(n::mi_cpu_clear16), Void),
        0x201e8d4 => e!("MIi_CpuSend32", Native(n::mi_cpu_send32), Void),
        0x201e8ec => e!("MIi_CpuClearFast", Native(n::mi_cpu_clear_fast), Void),
        0x202caf0 => e!("MemCopyBytes", Native(n::mem_copy_bytes), Value),
        0x20307b8 => e!("Libc_Bzero", Native(n::libc_bzero), Value),
        0x20211e8 => e!("DC_FlushRange", Native(n::dc_flush_range), Void),
        0x20211b0 => e!("DC_InvalidateRange", Native(n::dc_invalidate_range), Void),
        0x202f3fc => e!("strncmp", Native(n::strncmp), Value),
        0x20333c8 => e!("_s32_div_f", Native(n::s32_div_f), Value),
        0x2004dcc => e!("NNSi_G3dFuncSbc_NODEMIX", Native(n::g3d_sbc_nodemix), Void),
        0x2019a10 => e!("MTX_Concat44", Native(n::mtx_concat44), Void),
        0x208ba18 => e!("TextCursor_Initialize", Native(n::text_cursor_initialize), Value),
        0x208bb14 => e!("TextCursor_Update", Native(n::text_cursor_update), Value),
        0x2084a94 => e!("Text_GetGlyphWidth", Native(n::text_get_glyph_width), Value),
        0x206fc6c => e!("CharId_GetGlobalValue", Native(n::char_id_get_global), Value),
        0x1ff8000 => e!("MIi_DmaSetParameters", Native(n::mi_dma_set_parameters), Value),
        0x201fd44 => e!("OS_ResetRequestIrqMask", Native(n::os_reset_request_irq_mask), Value),
        0x201fce4 => e!("OS_EnableIrqMask", Native(n::os_enable_irq_mask), Value),
        0x201fc2c => e!("OSi_EnterDmaCallback", Native(n::osi_enter_dma_callback), Void),
        0x201e678 => e!("MIi_FIFOCallback", Native(n::mi_fifo_callback), Void),
        0x201fb18 => e!("OS_SetIrqFunction", Native(n::os_set_irq_function), Void),
        0x201fba0 => e!("OS_GetIrqFunction", Native(n::os_get_irq_function), Value),
        0x2000950 => e!("MIi_UncompressBackward", Native(n::mi_uncompress_backward), Void),
        0x201ef8c => e!("MI_UncompressLZ8", Native(n::mi_uncompress_lz8), Void),
        0x20135d0 => e!("NNSi_G2dBitReaderRead", Native(n::g2d_bit_reader_read), Value),
        0x2012694 => e!("LetterChar", Native(n::g2d_letter_char), Void),
        0x201107c => e!("CpuLoadOAMMain_", Native(n::g2d_cpu_load_oam_main), Void),
        0x2011068 => e!("CpuLoadOAMSub_", Native(n::g2d_cpu_load_oam_sub), Void),
        0x2011454 => e!("NNS_G2dApplyOamManagerToHW", Native(n::g2d_apply_oam_manager), Void),
        0x2011650 => e!("NNS_G2dResetOamManagerBuffer", Native(n::g2d_reset_oam_manager), Void),
        0x20112cc => e!("NNS_G2dEntryOamManagerOam", Native(n::g2d_entry_oam_manager_oam), Value),
        0x2011398 => e!("NNS_G2dEntryOamManagerAffine", Native(n::g2d_entry_oam_manager_affine), Value),
        0x204948c => e!("Sprite_CopyCellOamsWithFlip", Native(n::sprite_copy_cell_oams), Value),
        0x20485d0 => e!("Sprite_BuildPriorityLists", Native(n::sprite_build_priority_lists), Void),
        0x20490e0 => e!("Sprite_DrawAll", Native(n::sprite_draw_all), Void),
        0x2033180 => e!("_ll_sdiv", Native(n::ll_sdiv), Value),
        0x201a950 => e!("G2x_SetBlendAlpha_", Native(n::g2x_set_blend_alpha), Void),
        0x20414bc => e!("Display_SetLcdScreenSwap", Native(n::display_set_lcd_swap), Void),
        0x20410e0 => e!("Ui_SetFlagsOfs4_0_1", Native(n::ui_set_flags_4_0_1), Void),
        Gfx_SetBgVisibleFlag => e!("Gfx_SetBgVisibleFlag", Native(n::gfx_set_bg_visible_flag), Void),
        Gfx_GetBgScreenSize => e!("Gfx_GetBgScreenSize", Subsystem("display"), Void),
        Display_ResetBgAndVram => e!("Display_ResetBgAndVram", Subsystem("display"), Void),
        Projection_SetCallback => e!("Projection_SetCallback", Native(n::projection_set_callback), Void),

        // ---- hooks: function pointers in the game's data ----
        GfxRes_CallHookPtr => e!("GfxRes_CallHookPtr", Native(n::hook_gfxres), Void),
        BgLoad_CallHookPtr => e!("BgLoad_CallHookPtr", Native(n::hook_bgload), Void),
        GameClock_CallHookWith0 => e!("GameClock_CallHookWith0", Native(n::hook_clock0), Void),
        GameClock_CallHookWith1 => e!("GameClock_CallHookWith1", Native(n::hook_clock1), Void),
        GameFlags_CallHookWithConst => e!("GameFlags_CallHookWithConst", Native(n::hook_flags_const), Value),
        GameFlags_SetBitRange => e!("GameFlags_SetBitRange", Native(n::hook_flags_bitrange), Value),
        UiFlow_ThunkTertiary => e!("UiFlow_ThunkTertiary", Native(n::hook_uiflow_tertiary), Void),
        FadeState_InvokeCallbackA => e!("FadeState_InvokeCallbackA", Native(n::hook_fadestate_a), Value),
        SceneLoop_NopCallbackA => e!("SceneLoop_NopCallbackA", Native(n::hook_sceneloop_a), Void),
        SceneLoop_NopCallbackB => e!("SceneLoop_NopCallbackB", Native(n::hook_sceneloop_b), Void),
        RandEvent_InvokeCallbackData_d9fc8 => e!("RandEvent_InvokeCallbackData_d9fc8", Native(n::hook_randevent_d9fc8), Void),
        SaveData_InvokeCallbackWithField1ec => e!("SaveData_InvokeCallbackWithField1ec", Native(n::hook_savedata_1ec), Void),

        // ---- fades ----
        ScreenFade_ToBlack => e!("ScreenFade_ToBlack", Native(n::fade_to_black), Void),
        ScreenFade_ToWhite => e!("ScreenFade_ToWhite", Native(n::fade_to_white), Void),
        ScreenFade_ToClear => e!("ScreenFade_ToClear", Native(n::fade_to_clear), Void),
        ScreenFade_IsActive => e!("ScreenFade_IsActive", Native(n::fade_is_active), Value),

        // ---- game flags (the heap block at *0x2141908) ----
        GameFlags_Assign => e!("GameFlags_Assign", Native(n::flags_assign), Void),
        GameFlags_Test => e!("GameFlags_Test", Native(n::flags_test), Value),
        GameFlags_SetField0x0 => e!("GameFlags_SetField0x0", Native(n::flags_set_0), Void),
        GameFlags_GetField0x0 => e!("GameFlags_GetField0x0", Native(n::flags_get_0), Value),
        GameFlags_PushField0x4 => e!("GameFlags_PushField0x4", Native(n::flags_push_4), Void),
        GameFlags_SetField0x98 => e!("GameFlags_SetField0x98", Native(n::flags_set_98), Void),
        GameFlags_ComputeField0x104 => e!("GameFlags_ComputeField0x104", Native(n::flags_compute_104), Void),
        GameFlags_CopyVec4_Ofs0xb8_0xc8 => e!("GameFlags_CopyVec4_Ofs0xb8_0xc8", Native(n::flags_copy_vec4), Void),
        0x20537f0 => e!("GameFlags_GetField0xe0", Native(n::flags_get_e0), Value),
        GameFlags_SetByte0x104 => e!("GameFlags_SetByte0x104", Pending, Void),
        GameFlags_SetByte0x105 => e!("GameFlags_SetByte0x105", Pending, Void),
        GameFlags_SetField0xc => e!("GameFlags_SetField0xc", Pending, Void),
        GameFlags_SetField0x102 => e!("GameFlags_SetField0x102", Pending, Void),
        GameFlags_GetField0x102 => e!("GameFlags_GetField0x102", Pending, Value),
        GameFlags_SetField0x110 => e!("GameFlags_SetField0x110", Pending, Void),
        GameFlags_TestScopedBit => e!("GameFlags_TestScopedBit", Subsystem("scoped flags"), Value),
        GameFlags_SetScopedBit => e!("GameFlags_SetScopedBit", Subsystem("scoped flags"), Void),
        GameFlags_ClearSequenceState => e!("GameFlags_ClearSequenceState", Pending, Void),
        GameFlags_HandleTimeModeChange => e!("GameFlags_HandleTimeModeChange", Subsystem("calendar"), Void),
        GameFlags_AdvanceCalendarEvents => e!("GameFlags_AdvanceCalendarEvents", Subsystem("calendar"), Void),
        GameFlags_RunClockSynchronization => e!("GameFlags_RunClockSynchronization", Subsystem("calendar"), Value),
        GameFlags_ConfigureField0x90 => e!("GameFlags_ConfigureField0x90", Subsystem("calendar"), Void),
        GameFlags_UpdateField0xa4 => e!("GameFlags_UpdateField0xa4", Subsystem("calendar"), Void),
        SaveData_InitializeClockFields => e!("SaveData_InitializeClockFields", Subsystem("calendar"), Void),

        // ---- game state (the static block at *0x21246ac) ----
        GameState_GetField0x78 => e!("GameState_GetField0x78", Native(n::gs_get_78), Value),
        GameState_GetField0x7a => e!("GameState_GetField0x7a", Native(n::gs_get_7a), Value),
        GameState_GetField0x98 => e!("GameState_GetField0x98", Native(n::gs_get_98), Value),
        GameState_SetPointer => e!("GameState_SetPointer", Native(n::gs_set_pointer), Void),

        // ---- the map / scene selection block (0x214695c) ----
        MapArc_GetCurrentCharId => e!("MapArc_GetCurrentCharId", Native(n::maparc_get_char), Value),
        MapArc_ClearCurrentCharId => e!("MapArc_ClearCurrentCharId", Native(n::maparc_clear_char), Void),
        MapArc_SetCurrentMode => e!("MapArc_SetCurrentMode", Native(n::maparc_set_mode), Void),
        MapArc_GetCurrentMode => e!("MapArc_GetCurrentMode", Native(n::maparc_get_mode), Value),
        MapArc_SetField34 => e!("MapArc_SetField34", Native(n::maparc_set_34), Void),
        MapArc_SetBgmId => e!("MapArc_SetBgmId", Native(n::maparc_set_bgm), Void),
        MapArc_GetBgmId => e!("MapArc_GetBgmId", Native(n::maparc_get_bgm), Value),
        MapArc_GetActiveCharIdAndMode => e!("MapArc_GetActiveCharIdAndMode", Native(n::maparc_get_active), Value),
        MapArc_SetCurrentCharId => e!("MapArc_SetCurrentCharId", Subsystem("scene selection"), Void),

        // ---- the clock ----
        TimePeriod_FromHour => e!("TimePeriod_FromHour", Native(n::time_period_from_hour), Value),
        GameClock_GetHour => e!("GameClock_GetHour", Native(n::clock_hour), Value),
        GameClock_GetYear => e!("GameClock_GetYear", Native(n::clock_year), Value),
        GameClock_GetMonth => e!("GameClock_GetMonth", Native(n::clock_month), Value),
        GameClock_GetDay => e!("GameClock_GetDay", Native(n::clock_day), Value),
        GameClock_GetTimePeriod => e!("GameClock_GetTimePeriod", Native(n::clock_time_period), Value),
        GameClock_GetDate => e!("GameClock_GetDate", Native(n::clock_get_date), Void),
        GameClock_LoadState => e!("GameClock_LoadState", Subsystem("scripts"), Void),
        GameClock_SaveState => e!("GameClock_SaveState", Subsystem("scripts"), Value),
        GameClock_ClearRtcCache => e!("GameClock_ClearRtcCache", Subsystem("scripts"), Value),
        GameClock_ClearManualTime => e!("GameClock_ClearManualTime", Subsystem("scripts"), Value),
        GameClock_ClearUpdateCallback => e!("GameClock_ClearUpdateCallback", Subsystem("scripts"), Void),
        GameClock_CallHookGetWeekday | GameClock_CallHookGetHour | GameClock_CallHookGetMinute | GameClock_CallHookGetSecond | GameClock_CallHookGetYear | GameClock_CallHookGetMonth | GameClock_CallHookGetDay => {
            e!("GameClock_CallHookGet*", Subsystem("calendar"), Value)
        }

        // ---- the screen manager's screen side (0x214fde0) ----
        Screen_RunCurrent => e!("Screen_RunCurrent", Subsystem("screens"), Value),
        Screen_GetNext => e!("Screen_GetNext", Native(n::screen_get_next), Value),
        Screen_GetResult => e!("Screen_GetResult", Native(n::screen_get_result), Value),
        Screen_GetAltResult => e!("Screen_GetAltResult", Native(n::screen_get_alt), Value),
        0x20885f4 => e!("Screen_GetField10", Native(n::screen_get_10), Value),
        Screen_GetSaved => e!("Screen_GetSaved", Native(n::screen_get_saved), Addr),
        Screen_GetSavedSize => e!("Screen_GetSavedSize", Native(n::screen_get_saved_size), Value),
        0x208a1f0 => e!("Screen_GetEntryArg", Native(n::screen_get_entry_arg), Value),
        0x208a200 => e!("Screen_SetEntryArg", Native(n::screen_set_entry_arg), Void),
        Sprite_SetField8 => e!("Sprite_SetField8", Native(n::screen_set_8), Void),
        Sprite_SetFieldc => e!("Sprite_SetFieldc", Native(n::screen_set_c), Void),
        Sprite_SetField20 => e!("Sprite_SetField20", Native(n::screen_set_20), Void),
        Sprite_ClearContext => e!("Sprite_ClearContext", Native(n::screen_clear_saved), Void),
        Sprite_ClearAnimation => e!("Sprite_ClearAnimation", Native(n::screen_clear_all), Void),
        0x2087538 => e!("Screen_ProcedureTable", Native(n::screen_procedure_table), Value),
        0x208757c => e!("Screen_ClearObject", Native(n::screen_clear_object), Void),
        0x2088298 => e!("Screen_Begin", Native(n::screen_begin), Void),
        0x2088288 => e!("Screen_RestartSprites", Subsystem("screens"), Void),
        0x2085018 => e!("Bg_AdvanceGfxSequence", Subsystem("screens"), Void),
        0x20875fc => e!("Screen_LoadLayout", Subsystem("screens"), Void),
        0x208a344 => e!("Screen_InitializeBgs", Subsystem("screens"), Void),
        0x208a488 => e!("Screen_LoadSprites", Subsystem("screens"), Void),
        0x208a5e4 => e!("Screen_DrawSprites", Subsystem("screens"), Void),
        0x2085360 => e!("Bg_SelectState", Subsystem("screens"), Void),
        0x2085568 => e!("Bg_DispatchState", Subsystem("backdrop"), Void),
        Sprite_ProcessAnimationFrame => e!("Sprite_ProcessAnimationFrame", Subsystem("screens"), Void),

        // ---- per-frame subsystems the top machine ticks ----
        SceneRoute_ProcessClockTransition => e!("SceneRoute_ProcessClockTransition", Subsystem("calendar"), Void),
        Transform_UpdateFrame => e!("Transform_UpdateFrame", Subsystem("camera"), Void),
        MapCtl_UpdateTouchSelection => e!("MapCtl_UpdateTouchSelection", Subsystem("room heroine"), Void),
        SprFx_SelectDataMu_d2c38 => e!("SprFx_SelectDataMu_d2c38", Subsystem("calendar"), Void),
        DateUi_SelectNightStyle => e!("DateUi_SelectNightStyle", Subsystem("date ui"), Void),

        // ---- boot ----
        Mod204af08_InitGameSystems => e!("Mod204af08_InitGameSystems", Subsystem("boot"), Void),
        Transform_GetInputContextStatus => e!("Transform_GetInputContextStatus", Subsystem("boot"), Value),
        0x2161674 | 0x216168c | 0x21616a8 => e!("ov00 boot pass", Subsystem("boot"), Void),
        0x2161528 => e!("ov00 boot pass done", Subsystem("boot"), Value),
        Mod204af08_ShutdownGameSystems => e!("Mod204af08_ShutdownGameSystems", Subsystem("boot"), Void),

        // ---- screens and menus the machines run directly ----
        0x216a9a8 => e!("ov13 title screen", Subsystem("title"), Value),
        SaveSelect_Init => e!("SaveSelect_Init", Subsystem("save select"), Value),
        SaveSelect_Shutdown => e!("SaveSelect_Shutdown", Subsystem("save select"), Void),
        SaveSelect_RefreshNewestSlotDisplay => e!("SaveSelect_RefreshNewestSlotDisplay", Subsystem("save select"), Void),
        SaveSelect_TickWaitCounter => e!("SaveSelect_TickWaitCounter", Native(n::save_select_tick_wait), Void),
        SceneLoop_InitializeSpriteSystemA | SceneLoop_InitializeSpriteSystemB => None, // ported (flow)
        FlowState_InitSingleton => e!("FlowState_InitSingleton", Subsystem("flow state"), Void),
        UiChoice_ResetPrimary => e!("UiChoice_ResetPrimary", Subsystem("continue"), Void),
        UiChoice_ResetSecondary => e!("UiChoice_ResetSecondary", Subsystem("continue"), Void),
        UiChoice_DispatchInput => e!("UiChoice_DispatchInput", Subsystem("continue"), Value),
        0x216ac5c | 0x216a920 | 0x216a938 | 0x216ab44 => e!("ov20/ov43 time screens", Subsystem("continue"), Value),
        Transform_UpdateInputContext => e!("Transform_UpdateInputContext", Subsystem("camera"), Void),
        Transform_SetContextEnabled => e!("Transform_SetContextEnabled", Subsystem("camera"), Void),
        UiInput_Reset => e!("UiInput_Reset", Native(n::ui_input_reset), Void),

        // ---- the scene machine's callees ----
        Mod2049a04_EnterSceneSaveClock | Mod2049a04_ResetSceneSystems | Mod2049a04_TeardownSceneSystems | Mod2049a04_TeardownSceneIfMapState0xb_0x14
        | Mod2049a04_ApplyMapSelection | Mod2049a04_CallThreeExtFuncs | Mod2049a04_ResetClockScriptSoundFade => e!("scene machine helper", Subsystem("scene"), Void),
        Mod2049a04_IsIdInTables => e!("Mod2049a04_IsIdInTables", Native(n::scene_id_in_tables), Value),
        0x21582f4 | 0x215836c | 0x2159a6c | 0x2159a88 | 0x215847c | 0x2158584 | 0x2159c84 | 0x215a148 => e!("ov02 scene poll", Subsystem("scene"), Value),
        0x21583e4 | 0x2158298 | 0x21598d4 | 0x2159888 | 0x2159a98 => e!("ov02 scene sub-screen", Subsystem("scene"), Void),
        ClockWidget_UpdateStateB => e!("ClockWidget_UpdateStateB", Subsystem("scene"), Value),
        ClockWidget_RunStateMachine => e!("ClockWidget_RunStateMachine", Subsystem("scene"), Value),
        GameFlow_GetStateField4 => e!("GameFlow_GetStateField4", Native(n::gameflow_get_4), Value),
        GameFlow_InvokeCallbackMode8 => e!("GameFlow_InvokeCallbackMode8", Subsystem("scene"), Void),
        Schedule_DispatchEventLoad => e!("Schedule_DispatchEventLoad", Subsystem("scene selection"), Value),
        UiSelection_SetValue => e!("UiSelection_SetValue", Native(n::ui_selection_set), Void),
        GameSnd_PlaySceneAudio | GameSnd_SetSceneAudioLock => e!("GameSnd scene audio", Subsystem("audio"), Void),
        SaveData_IsRecord0Used => e!("SaveData_IsRecord0Used", Native(n::savedata_record0_used), Value),
        TaskList_InitGlobalWithFnPtr | TaskList_ClearGlobalPtr => e!("TaskList global", Subsystem("scene"), Void),

        // ---- everything else the machines call: game logic not ported yet ----
        _ => Some(Entry { name: crate::flow::addr::name(a).unwrap_or("?"), kind: Pending, ret: Value }),
    }
}

/// the static-code functions the registry answers (the interpreter traps them); overlay addresses are not trapped,
/// since another overlay may occupy them
pub fn trapped_addresses() -> Vec<u32> {
    let mut v: Vec<u32> = crate::flow::addr::ALL.iter().copied().chain(EXTRA.iter().copied()).chain(super::sdk::TRAPPED.iter().copied()).filter(|&a| a < STATIC_CODE_END).filter(|&a| matches!(entry(a), Some(Entry { kind: Native(_) | Subsystem(_), .. }))).collect();
    v.sort();
    v.dedup();
    v
}

/// the end of the static module's code (the game's own engine code ends at 0x20fc000; data follows)
const STATIC_CODE_END: u32 = 0x20fc000;

/// the registry addresses not named in flow::addr (for listings)
pub fn extra() -> Vec<u32> {
    EXTRA.to_vec()
}

/// registry addresses not named in flow::addr
const EXTRA: &[u32] = &[0x2001a58, 0x20019e4, 0x201e88c, 0x201e874, 0x201e8d4, 0x201e8ec, 0x202caf0, 0x20307b8, 0x20211e8, 0x20211b0, 0x202f3fc, 0x20333c8, 0x2033180, 0x201a73c, 0x201a950, 0x20414bc, 0x2041de0, 0x2041e1c, 0x2084ed4, 0x20410e0, 0x20537f0, 0x20885f4, 0x208a1f0, 0x208a200, 0x2087538, 0x208757c, 0x2088298, 0x2088288, 0x2085018, 0x20875fc, 0x208a344, 0x208a488, 0x208a5e4, 0x2085360, 0x2085568, 0x2004dcc, 0x2019a10, 0x20135d0, 0x2012694, 0x201107c, 0x2011068, 0x2011454, 0x2011650, 0x20112cc, 0x2011398, 0x204948c, 0x20485d0, 0x20490e0, 0x2000950, 0x201ef8c, 0x1ff8000, 0x201fd44, 0x201fce4, 0x201fc2c, 0x201e678, 0x201fb18, 0x201fba0, 0x206fc6c, 0x2084a94, 0x208bb14, 0x208ba18];

/// a native with side effects against the game's function: the game's code from a snapshot, then the native from
/// the same snapshot; memory and the geometry engine compared (`Machine::check_counts`), the native's run kept
fn check_native(m: &mut Machine, s: &mut dyn Subsystems, target: u32, f: NativeFn, args: &mut [Arg]) -> i32 {
    let snap = m.ram.snapshot();
    let (tick, carry, vblank, irqs) = (m.tick, m.instr_carry, m.vblank_tick, m.irqs);
    let mut w: Vec<Arg> = args.iter().map(|a| if let Arg::I(v) = a { Arg::I(*v) } else { Arg::Any }).collect();
    m.ram.unset_trap(target);
    let g = super::interp::interpret(m, s, target, &mut w);
    m.ram.set_trap(target);
    // the game's time: its instructions and what the natives it calls charge
    let game_instr = m.tick * super::interp::INSTR_PER_TICK + m.instr_carry - (tick * super::interp::INSTR_PER_TICK + carry);
    let interrupted = m.irqs != irqs;
    let game_ram = m.ram.snapshot();
    let game_gx = m.ram.gx.state();
    m.ram.restore(&snap);
    (m.tick, m.instr_carry, m.vblank_tick) = (tick, carry, vblank);
    let t0 = m.tick * super::interp::INSTR_PER_TICK + m.instr_carry;
    let r = f(m, s, args);
    let native_instr = m.tick * super::interp::INSTR_PER_TICK + m.instr_carry - t0;
    if interrupted {
        *m.check_counts.2.entry("(skipped: an interrupt ran inside the game's run)".into()).or_default() += 1;
        return r;
    }
    let c = &mut m.check_counts;
    c.0 += 1;
    let mut diffs: Vec<String> = m.ram.differences(&game_ram, m.sp_top.saturating_sub(0x2000)..m.sp_top).into_iter().map(|(k, a, n)| format!("{k} at {a:#x} ({n} bytes)")).collect();
    for ((name, a), (_, b)) in m.ram.gx.state().iter().zip(&game_gx) {
        if (a - b).abs() > 1e-9 {
            diffs.push(if name.contains("translation") { "geometry: a vector matrix's translation row (not used by the hardware)".into() } else { format!("geometry: {name}") });
        }
    }
    let c = &mut m.check_counts;
    if diffs.is_empty() {
        c.1 += 1;
    }
    diffs.sort();
    diffs.dedup();
    for d in diffs {
        *c.2.entry(d).or_default() += 1;
    }
    *c.2.entry("(time) game instructions".into()).or_default() += game_instr;
    *c.2.entry("(time) native charged".into()).or_default() += native_instr;
    if std::env::var_os("LP_CHECK_LOG").is_some() {
        eprintln!("check {target:#x}: game {game_instr} instructions, native {native_instr}");
    }
    let _ = g; // the native's result is kept; void natives return nothing comparable
    r
}

/// call a game function by address through the registry; what nobody answers runs as the game's code
pub fn call(m: &mut Machine, s: &mut dyn Subsystems, target: u32, args: &mut [Arg]) -> i32 {
    if target == 0x205e7c4 && m.event_override.is_some() && args.first().and_then(|a| a.int()) == m.event_override.map(|v| v as i32) {
        // the requested event's script starts: the override is spent (Machine::event_override)
        let id = m.event_override.take().unwrap();
        m.events_served.push((m.frame, id));
    }
    let Some(en) = entry(target) else {
        *m.unknown.entry(target).or_default() += 1;
        return super::interp::interpret(m, s, target, args);
    };
    match en.kind {
        Native(f) if m.shadow.is_some() && en.ret == Value && is_pure_getter(en.name) && target < STATIC_CODE_END && super::sdk::entry(target).is_none() => {
            // the native, then the game's own function on the same state (a getter changes nothing)
            let words: Vec<Arg> = args.iter().map(|a| match a {
                Arg::I(v) => Arg::I(*v),
                _ => Arg::Any,
            }).collect();
            let r = f(m, s, args);
            if words.iter().all(|a| matches!(a, Arg::I(_))) {
                let mut w = words;
                let irqs = m.irqs;
                m.ram.unset_trap(target);
                let g = super::interp::interpret(m, s, target, &mut w);
                m.ram.set_trap(target);
                let frame = m.frame;
                // an interrupt taken during the game's run may have changed what it read: not a comparison
                let interrupted = m.irqs != irqs;
                let e = m.shadow.as_mut().unwrap().entry(if interrupted { "(interrupted, not compared)" } else { en.name }).or_default();
                if !interrupted && g != r && std::env::var("LP_SHADOW").map_or(false, |v| v == "verbose") {
                    eprintln!("shadow {} {target:#x}: native {r:#x} game {g:#x}; args {:x?}; chain {:x?}", en.name, args.iter().map(|a| if let Arg::I(v) = a { *v } else { -1 }).collect::<Vec<_>>(), m.chain);
                }
                if interrupted || g == r {
                    e.0 += 1;
                } else {
                    e.1 += 1;
                    if e.2.len() < 5 {
                        e.2.push((frame, r, g));
                    }
                }
            }
            r
        }
        Native(f) if m.check_native == Some(target) && m.trap_depth <= 1 => check_native(m, s, target, f, args),
        Native(f) => f(m, s, args),
        Subsystem(_) => match s.call(m, target, en.name, args) {
            Some(r) => r,
            None => {
                *m.pending.entry(en.name).or_default() += 1;
                super::interp::interpret(m, s, target, args)
            }
        },
        Pending => {
            *m.pending.entry(en.name).or_default() += 1;
            super::interp::interpret(m, s, target, args)
        }
    }
}

/// a trapped function reached from the game's code
pub fn call_answered(m: &mut Machine, s: &mut dyn Subsystems, target: u32, args: &mut [Arg]) -> i32 {
    call(m, s, target, args)
}

/// natives that only read state (the shadow check runs the game's function after them)
fn is_pure_getter(name: &str) -> bool {
    (name.contains("Get") || name.contains("_Is") || name == "GameFlags_Test" || name == "TimePeriod_FromHour") && !name.contains("CallHook")
}

/// whether a registry name is routed to an engine subsystem (Some(true)), registered otherwise (Some(false)) or
/// unknown (None) - for reports
pub fn kind_of(name: &str) -> Option<bool> {
    crate::flow::addr::ALL.iter().chain(EXTRA.iter()).filter_map(|&a| entry(a)).find(|e| e.name == name).map(|e| matches!(e.kind, Subsystem(_)))
}
