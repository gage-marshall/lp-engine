"""BGM selection tables from the game's ARM9 static image (written to content/engine/bgm_tables.json by lp_extract; the values are derived from
the user's ROM and are not stored in the repo).

Script command 0x68 (`GameSnd_PlayBgm`) takes a sequence number; two aliases pick it from the running heroine slot:
  1000 -> by_slot[slot]                              (GameSnd_GetBgmForCurrentSlot, table pointer at 0x20733e4)
  1001 -> by_mode[slot * 2 + (GameState field 0x78 != 0)]   (GameSnd_GetBgmForCurrentSlotByMode, pointer at 0x2073380; alias id const at 0x20733b8)
  cmd 0x1d1 (heroine stage BGM): stage[slot * 5 + stage] (GameSnd_GetHeroineStageBgm, pointer at 0x207332c)
"""
import struct

BASE = 0x2000000


def extract(arm9):
    def w(a): return struct.unpack_from('<I', arm9, a - BASE)[0]
    def table(ptr_addr, n):
        p = w(ptr_addr); return list(arm9[p - BASE:p - BASE + n])
    return {'alias_by_slot': 1000, 'alias_by_mode': w(0x20733b8), 'by_slot': table(0x20733e4, 3), 'by_mode': table(0x2073380, 6), 'stage': table(0x207332c, 15)}
