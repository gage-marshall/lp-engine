# LovePlus+ decomp workspace

- rom/loveplus_plus_j.nds : BOXJ, DS-only (unit code 0x00)
- sdk/ : TwlSDK 5.4 patch1 subset (lib, include, build, man, docs, tools, components). Game SDK version word = 0x05047530 (5.4 release)
- toolchain/ : CodeWarrior for DSi zips
- docs/ : NITRO programming/card manuals
- work/arm9.bin, arm7.bin : raw extracts. ARM9 is BLZ-compressed (compressed_static_end=0x020B7738), decompress before Ghidra. ARM9 loads 0x02000000 entry 0x02000800; ARM7 loads 0x02380000.
- FID plan: plain (non-.TWL) ARM + .thumb libs in sdk/lib/ARM9-TS/{Release,Rom}; skip FUNCTIONCOST/CALLTRACE.
- 3rd-party libs in game (no FID source): Zi DLTLIB, Panasonic ASR106, NINTENDO:BACKUP
