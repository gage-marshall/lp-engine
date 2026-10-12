# Heroine save record (GameState_GetSlotPtr 0x2063618), derived from the accessors

Generated from `tools/save_fields.py` (machine-code scan of every small function that calls the record-pointer getter) plus the notes decoded from `SaveSlot_GetLevel/ProcessStorageStatus/GetLevelFromPoints/AddPointsClamped/ProcessStateRecord/ResetFromField8` and their threshold tables. Three heroine records (slot 0 Manaka, 1 Rinko, 2 Nene). Offsets are bytes from the record base; many names in `fid/labels_all.csv` are generic (`SaveSlot_GetSlotPointerX`).

| offset | type | stride | meaning | accessors |
|---|---|---|---|---|
| +0x0 | 4u | - | record flags: bit0 = lovers state (SaveSlot_AddPointsClamped only works with it; ProcessStorageStatus returns 3), bit 0x400 = lost/broken-up (status 0, level 0) | GameClock_GetCurrentSlot, GameClock_UpdateCalendar, SaveSlot_AddPointsClamped, SaveSlot_GetLevel, SlotWorkflow_ProcessPhase |
| +0x4 | 1s | - | relationship phase (signed byte, 0..2): TextRes_UpdateSlotState maps it to the stage column 0..2 used by every id table | SaveSlot_GetSlotPointerA |
| +0x4 | 1u | - | relationship phase (signed byte, 0..2): TextRes_UpdateSlotState maps it to the stage column 0..2 used by every id table | SaveSlot_GetSlotPointerB, SaveSlot_InitStatPairFromTable |
| +0x5 | 1u | - | state byte: bucketed by table 0x2101e63 (16 steps of 16) then via 0x2101e72 `[phase*4]` to a 0..4 state (SaveSlot_ProcessStateRecord; the value `SaveSlot_CallStateHook` / `SaveSlot_GetSlotPointerI` read) | EventCond_CompareSlotByteOfs5, SaveSlot_GetSlotPointerI, SaveSlot_GetSlotPointerL, SaveSlot_ProcessRecordFlags, SaveSlot_ProcessStateRecord, SaveSlot_ |
| +0x6 | 1u | - | affection value (u8): `SaveSlot_GetLevel`; `SaveSlot_GetLevelFromPoints` maps it to hearts level 0..16 with per-heroine 17-byte threshold tables at 0x2101e92 | SaveSlot_GetLevel, SaveSlot_GetSlotPointerH, SaveSlot_GetSlotPointerK |
| +0x7 | 1u | - |  | SaveSlot_GetByteOfs0x07 |
| +0x8 | 4u | - | love points (u32): `SaveSlot_AddPointsClamped` (needs bit0), level 1..20 from thresholds 100,250,750,1200,2500,3500,5000,7500,9999,13000,14500,16250,18250,20500,23000,25750,28750,32000,35500 (`SaveSlot_ResetFromField8`) | SaveSlot_AddPointsClamped, SaveSlot_ResetFromField8 |
| +0xc | 1u | - | clamped byte counter (SaveSlot_AddClampedByte_Ofs0x0c) | SaveSlot_AddClampedByte_Ofs0x0c, SaveSlot_GetSlotPointerJ, SaveSlot_UpdateStateMetadataTimer |
| +0xd | 1s | - | signed byte 0..2 (set with range check) | SaveSlot_GetSlotPointerE |
| +0xd | 1u | - | signed byte 0..2 (set with range check) | SaveSlot_SetByte0xd_Range0to2 |
| +0x10 | 2s | - | s16[20] table | SaveSlot_GetShortArray_Ofs0x10 |
| +0x10 | 2u | 2 | s16[20] table | SaveSlot_GetSlotPointerP |
| +0x38 | 2s | - | s16[20] table | SaveSlot_GetShortArray_Ofs0x38 |
| +0x38 | 2u | - | s16[20] table | SaveSlot_ProcessRecordTimer |
| +0x60 | 4u | - | {u32,u32}[20] table | SaveSlot_SetWordPairByIndex_Ofs0x60 |
| +0x64 | 4u | - |  | SaveSlot_SetWordPairByIndex_Ofs0x60 |
| +0xba | 2u | - |  | SlotData_ProcessState |
| +0x105 | 1u | - |  | SlotCache_GetSlotPointerA, SlotCache_GetSlotPointerB, SlotCache_GetSlotPointerC, SlotCache_ProcessRecord |
| +0x106 | 1u | - |  | SlotCache_GetSlotPointerD, SlotCache_GetSlotPointerE, SlotCache_GetSlotPointerF, SlotCache_UpdateRecord |
| +0x107 | 1s | - |  | SaveSlot_GetSlotPointerC |
| +0x107 | 1u | - |  | SaveSlot_GetSlotPointerO |
| +0x108 | 1s | - |  | SaveSlot_GetSlotPointerF |
| +0x108 | 1u | - |  | SaveSlot_GetSlotPointerQ |
| +0x109 | 1s | - |  | SaveSlot_GetSlotPointerG |
| +0x109 | 1u | - |  | SaveSlot_GetSlotPointerR |
| +0x10a | 1u | - |  | SaveSlot_UpdateStateMetadataTimer |
| +0x10b | 1u | - |  | SlotCache_GetSlotPointerG, SlotField_GetSlotPointerA |
| +0x10d | 1s | - |  | SlotField_GetSlotPointerD |
| +0x10d | 1u | - |  | SaveSlot_SetByteOfs0x10dLimit20 |
| +0x10e | 1u | - |  | SaveSlot_GetLow7BitsOfs0x10eIfHighBit, SlotField_UpdateState |
| +0x110 | 4u | - |  | SlotField_GetSlotPointerB, SlotField_GetSlotPointerC |
| +0x114 | 4u | - |  | SlotField_GetSlotPointerE |
| +0x118 | 1u | - |  | SlotArchive_ProcessState |
| +0x11c | 4u | - |  | SlotField_CommitState, SlotField_ProcessState |
| +0x120 | 4u | - |  | SlotField_CommitState, SlotField_ProcessState |
| +0x14c | 2u | - |  | SaveSlot_PushHistoryU16_Ofs0x14c |
| +0x160 | 1u | - |  | SlotWorkflow_SelectAction |
| +0x161 | 1u | - |  | SlotWorkflow_SelectAction |
| +0x162 | 2s | - |  | SaveSlot_GetSlotPointerM |
| +0x162 | 2u | - |  | SaveSlot_GetSlotPointerN |
| +0x170 | 1u | - |  | SaveSlot_GetByteOfs0x170, SaveSlot_SetByteOfs0x170Max20 |
| +0x171 | 1u | - | clamped stat byte (script cmd family 0x27/0x29 siblings) | SaveSlot_AddClampedStat0x171 |
| +0x172 | 1s | - | metadata byte bucketed by a 4-entry table (SaveSlot_ClearStateMetadata) | SaveSlot_ClearStateMetadata |
| +0x172 | 1u | - | metadata byte bucketed by a 4-entry table (SaveSlot_ClearStateMetadata) | SaveSlot_UpdateRecordFlags |
| +0x180 | 1u | - | clamped stat 0x180 (script cmd 0x27 char_add_stat_a, task 0x57) | SaveSlot_AddClampedStat0x180, SaveSlot_AddSignedClampedByte_Ofs0x180, SaveSlot_InitStatPairFromTable, SlotWorkflow_GetSlotPointerB |
| +0x181 | 1u | - | clamped stat 0x181 (script cmd 0x29 char_add_stat_b, task 0x58) | SaveSlot_AddClampedStat0x181, SaveSlot_AddSignedClampedByte_Ofs0x181, SaveSlot_InitStatPairFromTable, SlotWorkflow_GetSlotPointerA, SlotWorkflow_GetSl |
| +0x183 | 1u | - |  | SaveSlot_AddSaturatedByte_Ofs0x183, SlotArchive_GetSlotPointerA, SlotArchive_GetSlotPointerB |
| +0x184 | 1u | - |  | SlotField_GetSlotPointerF, SlotField_GetSlotPointerG, SlotField_GetSlotPointerH, SlotField_GetSlotPointerI, SlotField_GetSlotPointerJ |
| +0x186 | 2s | - |  | SlotField_GetSlotPointerL |
| +0x186 | 2u | - |  | SlotField_GetSlotPointerK |
| +0x188 | 2s | - |  | SlotField_GetSlotPointerM |
| +0x188 | 2u | - |  | SlotField_GetSlotPointerK |
| +0x18a | 2s | - |  | SlotField_GetSlotPointerO |
| +0x18a | 2u | - |  | SlotField_GetSlotPointerN |
| +0x18c | 2u | - |  | SlotField_GetSlotPointerN |
| +0x190 | 4u | - |  | SlotField_GetSlotPointerP, SlotField_GetSlotPointerQ |
| +0x194 | 1u | - |  | SlotField_GetSlotPointerR, SlotField_GetSlotPointerS, SlotField_GetSlotPointerT |
| +0x19c | 1u | - |  | SaveSlot_AddByteRange0to14_Ofs0x19d, SaveSlot_GetSlotPointerS, SaveSlot_GetSlotPointerT |
| +0x19d | 1u | - |  | SaveSlot_AddByteRange0to14_Ofs0x19d, SaveSlot_GetSlotPointerT |
| +0x19e | 1u | - |  | SaveSlot_GetSlotPointerU, SaveSlot_GetSlotPointerV |
| +0x1a0 | 4u | - |  | SaveSlot_GetSlotPointerW |
| +0x1a4 | 1u | - |  | SlotData_GetSlotPointerA, SlotData_GetSlotPointerB |
| +0x1a5 | 1u | - |  | SaveSlot_GetSlotPointerX |
| +0x1a7 | 1u | - |  | SlotData_GetSlotPointerC |
| +0x1a8 | 1u | - |  | SlotData_GetSlotPointerD |
| +0x1a9 | 1u | - |  | SlotData_GetSlotPointerE |
| +0x1ac | 4u | - |  | SlotData_GetSlotPointerF, SlotData_GetSlotPointerG |
| +0x1b0 | 4u | - |  | SlotInfo_GetSlotPointerA, SlotInfo_GetSlotPointerB |
| +0x1b4 | 4u | - |  | SlotData_GetSlotPointerH, SlotInfo_GetSlotPointerC |
| +0x1b8 | 1u | - |  | SaveSlot_InitByteOfs0x1b8To30IfZero, SlotInfo_GetSlotPointerA |
| +0x1b9 | 1u | - |  | SlotInfo_GetSlotPointerC |
| +0x1ba | 2u | - |  | SlotData_UpdateState |
